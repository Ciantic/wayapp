use crate::Application;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::EguiWgpuRenderer;
use crate::KeyboardHandlerContainer;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::WaylandToEguiInput;
use crate::WindowContainer;
use crate::get_app;
use egui::PlatformOutput;
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

pub trait EguiAppData {
    fn ui(&mut self, ctx: &egui::Context);
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

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        self.input_state.handle_pointer_event(event);
        let platform_output = self.render();

        // Handle cursor icon changes from EGUI
        get_app().set_cursor(egui_to_cursor_shape(platform_output.cursor_icon));
    }

    fn handle_keyboard_enter(&mut self) {
        self.input_state.handle_keyboard_enter();
        self.render();
    }

    fn handle_keyboard_leave(&mut self) {
        self.input_state.handle_keyboard_leave();
        self.render();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state
            .handle_keyboard_event(event, pressed, repeat);
        self.render();
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.input_state.update_modifiers(modifiers);
        self.render();
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

    fn render(&mut self) -> PlatformOutput {
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
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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

        let platform_output = self.renderer.end_frame_and_draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &texture_view,
            screen_descriptor,
        );

        for command in &platform_output.commands {
            self.input_state.handle_output_command(command);
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // Only request next frame if there are events (similar to windowed.rs behavior)
        if !platform_output.events.is_empty() {
            self.wl_surface
                .frame(&self.queue_handle, self.wl_surface.clone());
            self.wl_surface.commit();
        }
        platform_output
    }

    fn reconfigure_surface(&mut self) {
        let width = self.width.saturating_mul(self.physical_scale()).max(1);
        let height = self.height.saturating_mul(self.physical_scale()).max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.output_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
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

impl<A: EguiAppData> BaseTrait for EguiWindow<A> {}

impl<A: EguiAppData> WindowContainer for EguiWindow<A> {
    fn configure(&mut self, configure: &WindowConfigure) {
        let width = configure.new_size.0.map_or(256, |size| size.get());
        let height = configure.new_size.1.map_or(256, |size| size.get());
        self.window
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }

    fn get_window(&self) -> &Window {
        &self.window
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

impl<A: EguiAppData> BaseTrait for EguiLayerSurface<A> {}

impl<A: EguiAppData> LayerSurfaceContainer for EguiLayerSurface<A> {
    fn configure(&mut self, config: &LayerSurfaceConfigure) {
        self.layer_surface
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(config.new_size.0, config.new_size.1);
    }

    fn get_layer_surface(&self) -> &LayerSurface {
        &self.layer_surface
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

impl<A: EguiAppData> BaseTrait for EguiPopup<A> {}

impl<A: EguiAppData> PopupContainer for EguiPopup<A> {
    fn configure(&mut self, config: &PopupConfigure) {
        self.popup
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface
            .configure(config.width as u32, config.height as u32);
    }

    fn done(&mut self) {}

    fn get_popup(&self) -> &Popup {
        &self.popup
    }
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

impl<A: EguiAppData> BaseTrait for EguiSubsurface<A> {}

impl<A: EguiAppData> SubsurfaceContainer for EguiSubsurface<A> {
    fn configure(&mut self, width: u32, height: u32) {
        self.wl_surface.set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }

    fn get_wl_surface(&self) -> &WlSurface {
        &self.wl_surface
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
