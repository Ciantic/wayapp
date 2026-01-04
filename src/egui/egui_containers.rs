use crate::Application;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::EguiWgpuRenderer;
use crate::KeyboardHandlerContainer;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::RenderOutput;
use crate::SubsurfaceContainer;
use crate::WaylandToEguiInput;
use crate::WindowContainer;
use crate::get_app;
use log::trace;
use pollster::block_on;
use raw_window_handle::RawDisplayHandle;
use raw_window_handle::RawWindowHandle;
use raw_window_handle::WaylandDisplayHandle;
use raw_window_handle::WaylandWindowHandle;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Modifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
use smithay_clipboard::Clipboard;
use std::ptr::NonNull;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

/// Rectangle for input region: (x, y, width, height)
pub type InputRect = (i32, i32, i32, i32);

/// Configuration options for the WGPU surface
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceOptions {
    /// Clear color for the surface background
    pub clear_color: wgpu::Color,
    /// Present mode for vsync behavior
    pub present_mode: wgpu::PresentMode,
    /// Alpha compositing mode
    pub alpha_mode: wgpu::CompositeAlphaMode,
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        Self {
            clear_color: wgpu::Color::BLACK,
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
        }
    }
}

impl SurfaceOptions {
    /// Preset for transparent overlay surfaces
    pub fn transparent_overlay() -> Self {
        Self {
            clear_color: wgpu::Color::TRANSPARENT,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
        }
    }
}

pub trait EguiAppData {
    fn ui(&mut self, ctx: &egui::Context);

    /// Return input regions (clickable areas). If None, entire surface receives
    /// input. If Some(vec), only those rectangles receive input (rest is
    /// click-through).
    fn input_regions(&self) -> Option<Vec<InputRect>> {
        None
    }

    /// Return surface configuration options. Override to customize clear color,
    /// present mode, and alpha compositing.
    fn surface_options(&self) -> SurfaceOptions {
        SurfaceOptions::default()
    }
}

struct EguiSurfaceState<A: EguiAppData> {
    wl_surface: WlSurface,
    // instance: wgpu::Instance, // docs says it doesn't need to be kept alive
    surface: wgpu::Surface<'static>,
    // adapter: wgpu::Adapter, // docs says it doesn't need to be kept alive
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: EguiWgpuRenderer,
    egui_app: A,
    input_state: WaylandToEguiInput,
    queue_handle: QueueHandle<Application>,
    width: u32,
    height: u32,
    scale_factor: i32,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    output_format: wgpu::TextureFormat,
    last_input_regions: Option<Vec<InputRect>>,
    last_surface_options: SurfaceOptions,
    /// was a new frame already requested? Set by e.g. mouse events, cleared on render
    is_frame_requested: bool,
}

impl<A: EguiAppData> EguiSurfaceState<A> {
    fn new(wl_surface: WlSurface, egui_app: A) -> Self {
        let app = get_app();
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(app.conn.backend().display_ptr() as *mut _)
                .expect("Wayland display pointer was null"),
        ));
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(wl_surface.id().as_ptr() as *mut _)
                .expect("Wayland surface handle was null"),
        ));

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle,
                    raw_window_handle,
                })
                .expect("Failed to create WGPU surface")
        };

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("Failed to find a suitable adapter");

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("Failed to request WGPU device");

        let caps = surface.get_capabilities(&adapter);
        let output_format = *caps
            .formats
            .get(0)
            .unwrap_or(&wgpu::TextureFormat::Bgra8Unorm);

        let renderer = EguiWgpuRenderer::new(&device, output_format, None, 1);
        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = WaylandToEguiInput::new(clipboard);

        Self {
            wl_surface,
            // instance,
            surface,
            // adapter,
            device,
            queue,
            renderer,
            egui_app,
            input_state,
            queue_handle: app.qh.clone(),
            width: 256,
            height: 256,
            scale_factor: 1,
            surface_config: None,
            output_format,
            last_input_regions: None,
            last_surface_options: SurfaceOptions::default(),
            is_frame_requested: false,
        }
    }

    fn configure(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.input_state.set_screen_size(self.width, self.height);
        self.reconfigure_surface();
        self.render();
    }

    fn frame(&mut self, _time: u32) {
        self.render();
    }

    /// Request a frame callback to ensure pending input gets processed.
    /// This is needed when input arrives while idle (no frame callback pending).
    fn request_frame(&mut self) {
        if self.is_frame_requested {
            return;
        }
        self.is_frame_requested = true;
        self.wl_surface
            .frame(&self.queue_handle, self.wl_surface.clone());
        self.wl_surface.commit();
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        self.input_state.handle_pointer_event(event);
        self.request_frame();
    }

    fn handle_keyboard_enter(&mut self) {
        self.input_state.handle_keyboard_enter();
        self.request_frame();
    }

    fn handle_keyboard_leave(&mut self) {
        self.input_state.handle_keyboard_leave();
        self.request_frame();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state
            .handle_keyboard_event(event, pressed, repeat);
        self.request_frame();
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.input_state.update_modifiers(modifiers);
        self.request_frame();
    }

    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.wl_surface.set_buffer_scale(new_factor);
        let factor = new_factor.max(1);
        if factor == self.scale_factor {
            return;
        }
        self.scale_factor = factor;
        self.reconfigure_surface();
        self.render();
    }

    fn render(&mut self) -> RenderOutput {
        self.is_frame_requested = false;

        // Check if surface options changed and reconfigure if needed
        let current_options = self.egui_app.surface_options();
        let options_changed = current_options != self.last_surface_options;
        if options_changed {
            self.last_surface_options = current_options;
            self.reconfigure_surface();
        }

        trace!("Rendering surface {}", self.wl_surface.id());
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire next surface texture");

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.last_surface_options.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        let raw_input = self.input_state.take_raw_input();
        self.renderer.begin_frame(raw_input);
        self.egui_app.ui(self.renderer.context());

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [
                self.width.saturating_mul(self.physical_scale()),
                self.height.saturating_mul(self.physical_scale()),
            ],
            pixels_per_point: self.physical_scale() as f32,
        };

        let render_output = self.renderer.end_frame_and_draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &texture_view,
            screen_descriptor,
        );

        for command in &render_output.platform_output.commands {
            self.input_state.handle_output_command(command);
        }

        // Update cursor shape based on egui's request
        get_app().set_cursor(egui_to_cursor_shape(
            render_output.platform_output.cursor_icon,
        ));

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // Apply input region for click-through support (only when changed)
        let current_regions = self.egui_app.input_regions();
        let regions_changed = current_regions != self.last_input_regions;
        if regions_changed {
            if let Some(ref regions) = current_regions {
                let app = get_app();
                let region = app
                    .compositor_state
                    .wl_compositor()
                    .create_region(&app.qh, ());
                for (x, y, w, h) in regions {
                    region.add(*x, *y, *w, *h);
                }
                self.wl_surface.set_input_region(Some(&region));
                region.destroy();
            }
            self.last_input_regions = current_regions;
        }

        // ! TODO! This immediatly schedules a rerender if repaint_delay != max, even if that should be, say, 10 seconds.
        // Request next frame if:
        // - egui needs a repaint (animation, cursor blink, etc.) via repaint_delay
        // - there are output events (interactions occurred)
        // Apps that want continuous animation should call ctx.request_repaint() in their ui()
        let needs_repaint = render_output.repaint_delay != std::time::Duration::MAX
            || !render_output.platform_output.events.is_empty();
        let request_frame = needs_repaint && !self.is_frame_requested;
        if request_frame {
            self.is_frame_requested = true;
            self.wl_surface
                .frame(&self.queue_handle, self.wl_surface.clone());
        }
        // Commit if anything changed
        if request_frame || regions_changed || options_changed {
            self.wl_surface.commit();
        }

        render_output
    }

    fn reconfigure_surface(&mut self) {
        let width = self.width.saturating_mul(self.physical_scale()).max(1);
        let height = self.height.saturating_mul(self.physical_scale()).max(1);
        let options = &self.last_surface_options;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.output_format,
            width,
            height,
            present_mode: options.present_mode,
            alpha_mode: options.alpha_mode,
            view_formats: vec![self.output_format],
            desired_maximum_frame_latency: 2,
        };
        self.surface.configure(&self.device, &config);
        self.surface_config = Some(config);
    }

    fn physical_scale(&self) -> u32 {
        self.scale_factor.max(1) as u32
    }
}

pub struct EguiWindow<A: EguiAppData> {
    pub window: Window,
    surface: EguiSurfaceState<A>,
}

impl<A: EguiAppData> EguiWindow<A> {
    pub fn new(window: Window, egui_app: A, width: u32, height: u32) -> Self {
        let mut surface = EguiSurfaceState::new(window.wl_surface().clone(), egui_app);
        surface.width = width;
        surface.height = height;
        Self { window, surface }
    }
}

impl<A: EguiAppData> CompositorHandlerContainer for EguiWindow<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: EguiAppData> KeyboardHandlerContainer for EguiWindow<A> {
    fn enter(&mut self) {
        self.surface.handle_keyboard_enter();
    }

    fn leave(&mut self) {
        self.surface.handle_keyboard_leave();
    }

    fn press_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, false);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, false, false);
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.surface.update_modifiers(modifiers);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, true);
    }
}

impl<A: EguiAppData> PointerHandlerContainer for EguiWindow<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: EguiAppData> BaseTrait for EguiWindow<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.window.wl_surface().id()
    }
}

impl<A: EguiAppData> WindowContainer for EguiWindow<A> {
    fn configure(&mut self, configure: &WindowConfigure) {
        let width = configure.new_size.0.map_or(256, |size| size.get());
        let height = configure.new_size.1.map_or(256, |size| size.get());
        self.window
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }
}

pub struct EguiLayerSurface<A: EguiAppData> {
    pub layer_surface: LayerSurface,
    surface: EguiSurfaceState<A>,
}

impl<A: EguiAppData> EguiLayerSurface<A> {
    pub fn new(layer_surface: LayerSurface, egui_app: A, width: u32, height: u32) -> Self {
        let mut surface = EguiSurfaceState::new(layer_surface.wl_surface().clone(), egui_app);
        surface.width = width;
        surface.height = height;
        Self {
            layer_surface,
            surface,
        }
    }
}

impl<A: EguiAppData> CompositorHandlerContainer for EguiLayerSurface<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: EguiAppData> KeyboardHandlerContainer for EguiLayerSurface<A> {
    fn enter(&mut self) {
        self.surface.handle_keyboard_enter();
    }

    fn leave(&mut self) {
        self.surface.handle_keyboard_leave();
    }

    fn press_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, false);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, false, false);
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.surface.update_modifiers(modifiers);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, true);
    }
}

impl<A: EguiAppData> PointerHandlerContainer for EguiLayerSurface<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: EguiAppData> BaseTrait for EguiLayerSurface<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.layer_surface.wl_surface().id()
    }
}

impl<A: EguiAppData> LayerSurfaceContainer for EguiLayerSurface<A> {
    fn configure(&mut self, config: &LayerSurfaceConfigure) {
        self.layer_surface
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(config.new_size.0, config.new_size.1);
    }
}

pub struct EguiPopup<A: EguiAppData> {
    pub popup: Popup,
    surface: EguiSurfaceState<A>,
}

impl<A: EguiAppData> EguiPopup<A> {
    pub fn new(popup: Popup, egui_app: A, width: u32, height: u32) -> Self {
        let mut surface = EguiSurfaceState::new(popup.wl_surface().clone(), egui_app);
        surface.width = width;
        surface.height = height;
        Self { popup, surface }
    }
}

impl<A: EguiAppData> CompositorHandlerContainer for EguiPopup<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: EguiAppData> KeyboardHandlerContainer for EguiPopup<A> {
    fn enter(&mut self) {
        self.surface.handle_keyboard_enter();
    }

    fn leave(&mut self) {
        self.surface.handle_keyboard_leave();
    }

    fn press_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, false);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, false, false);
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.surface.update_modifiers(modifiers);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, true);
    }
}

impl<A: EguiAppData> PointerHandlerContainer for EguiPopup<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: EguiAppData> BaseTrait for EguiPopup<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.popup.wl_surface().id()
    }
}

impl<A: EguiAppData> PopupContainer for EguiPopup<A> {
    fn configure(&mut self, config: &PopupConfigure) {
        self.popup
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface
            .configure(config.width as u32, config.height as u32);
    }

    fn done(&mut self) {}
}

pub struct EguiSubsurface<A: EguiAppData> {
    pub wl_surface: WlSurface,
    surface: EguiSurfaceState<A>,
}

impl<A: EguiAppData> EguiSubsurface<A> {
    pub fn new(wl_surface: WlSurface, egui_app: A, width: u32, height: u32) -> Self {
        let mut surface = EguiSurfaceState::new(wl_surface.clone(), egui_app);
        surface.width = width;
        surface.height = height;
        Self {
            wl_surface,
            surface,
        }
    }
}

impl<A: EguiAppData> CompositorHandlerContainer for EguiSubsurface<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: EguiAppData> KeyboardHandlerContainer for EguiSubsurface<A> {
    fn enter(&mut self) {
        self.surface.handle_keyboard_enter();
    }

    fn leave(&mut self) {
        self.surface.handle_keyboard_leave();
    }

    fn press_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, false);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, false, false);
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.surface.update_modifiers(modifiers);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, true);
    }
}

impl<A: EguiAppData> PointerHandlerContainer for EguiSubsurface<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: EguiAppData> BaseTrait for EguiSubsurface<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.wl_surface.id()
    }
}

impl<A: EguiAppData> SubsurfaceContainer for EguiSubsurface<A> {
    fn configure(&mut self, width: u32, height: u32) {
        self.wl_surface.set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }
}

/// Convert EGUI cursor icon to Wayland cursor shape
fn egui_to_cursor_shape(cursor: egui::CursorIcon) -> Shape {
    use egui::CursorIcon::*;
    use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape as CursorShape;

    match cursor {
        Default => CursorShape::Default,
        None => CursorShape::Default,
        ContextMenu => CursorShape::ContextMenu,
        Help => CursorShape::Help,
        PointingHand => CursorShape::Pointer,
        Progress => CursorShape::Progress,
        Wait => CursorShape::Wait,
        Cell => CursorShape::Cell,
        Crosshair => CursorShape::Crosshair,
        Text => CursorShape::Text,
        VerticalText => CursorShape::VerticalText,
        Alias => CursorShape::Alias,
        Copy => CursorShape::Copy,
        Move => CursorShape::Move,
        NoDrop => CursorShape::NoDrop,
        NotAllowed => CursorShape::NotAllowed,
        Grab => CursorShape::Grab,
        Grabbing => CursorShape::Grabbing,
        AllScroll => CursorShape::AllScroll,
        ResizeHorizontal => CursorShape::EwResize,
        ResizeNeSw => CursorShape::NeswResize,
        ResizeNwSe => CursorShape::NwseResize,
        ResizeVertical => CursorShape::NsResize,
        ResizeEast => CursorShape::EResize,
        ResizeSouthEast => CursorShape::SeResize,
        ResizeSouth => CursorShape::SResize,
        ResizeSouthWest => CursorShape::SwResize,
        ResizeWest => CursorShape::WResize,
        ResizeNorthWest => CursorShape::NwResize,
        ResizeNorth => CursorShape::NResize,
        ResizeNorthEast => CursorShape::NeResize,
        ResizeColumn => CursorShape::ColResize,
        ResizeRow => CursorShape::RowResize,
        ZoomIn => CursorShape::ZoomIn,
        ZoomOut => CursorShape::ZoomOut,
    }
}
