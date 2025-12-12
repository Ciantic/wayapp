use crate::Application;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::KeyboardHandlerContainer;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::WaylandToIcedInput;
use crate::WindowContainer;
use crate::get_app;
use iced::Color;
use iced::Font;
use iced::Pixels;
use iced::Size;
use iced::Theme;
use iced::mouse;
use iced_core::Event;
use iced_core::renderer::Style;
use iced_core::window::Event::RedrawRequested;
use iced_graphics::Viewport;
use iced_renderer::Renderer;
use iced_runtime::user_interface;
use iced_wgpu::Engine;
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

pub trait IcedAppData {
    type Message: std::fmt::Debug + Clone + Send + 'static;

    fn view(&'_ self) -> iced::Element<'_, Self::Message>;
    fn update(&mut self, message: Self::Message);
}

struct IcedSurfaceState<A: IcedAppData> {
    wl_surface: WlSurface,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    _queue: wgpu::Queue,
    _engine: Engine,
    renderer: Renderer,
    input_state: WaylandToIcedInput,
    iced_app: A,
    queue_handle: QueueHandle<Application>,
    width: u32,
    height: u32,
    scale_factor: i32,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    output_format: wgpu::TextureFormat,
    cache: user_interface::Cache,
    mouse_interaction: mouse::Interaction,
}

impl<A: IcedAppData> IcedSurfaceState<A> {
    fn new(wl_surface: WlSurface, iced_app: A) -> Self {
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

        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            output_format,
            Some(iced_graphics::Antialiasing::MSAAx4),
            iced_graphics::Shell::headless(),
        );

        let wgpu_renderer = iced_wgpu::Renderer::new(engine.clone(), Font::DEFAULT, Pixels(16.0));
        let renderer = iced_renderer::Renderer::Primary(wgpu_renderer);

        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = WaylandToIcedInput::new(clipboard);

        Self {
            wl_surface,
            surface,
            device,
            _queue: queue,
            _engine: engine,
            renderer,
            input_state,
            iced_app,
            queue_handle: app.qh.clone(),
            width: 256,
            height: 256,
            scale_factor: 1,
            surface_config: None,
            output_format,
            cache: user_interface::Cache::default(),
            mouse_interaction: mouse::Interaction::default(),
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
        self.request_next_frame();
    }

    fn handle_keyboard_enter(&mut self) {
        self.input_state.handle_keyboard_enter();
        self.request_next_frame();
    }

    fn handle_keyboard_leave(&mut self) {
        self.input_state.handle_keyboard_leave();
        self.request_next_frame();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state
            .handle_keyboard_event(event, pressed, repeat);
        self.request_next_frame();
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.input_state.update_modifiers(modifiers);
        self.request_next_frame();
    }

    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.wl_surface.set_buffer_scale(new_factor);
        let factor = new_factor.max(1);
        if factor == self.scale_factor {
            return;
        }
        self.scale_factor = factor;
        self.reconfigure_surface();
        self.request_next_frame();
    }

    fn render(&mut self) {
        trace!("Rendering surface {}", self.wl_surface.id());

        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire next surface texture");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let viewport = self.create_viewport();
        let events = self.input_state.take_events();
        let cursor = self.get_cursor_position();

        // Update: Process events and messages, then draw
        self.update_and_draw(&viewport, &events, cursor);

        // Present the rendered frame
        self.present_frame(&viewport, &texture_view, surface_texture);

        if !events.is_empty() {
            self.request_next_frame();
        }
    }

    fn create_viewport(&self) -> Viewport {
        let physical_width = self.width.saturating_mul(self.physical_scale());
        let physical_height = self.height.saturating_mul(self.physical_scale());
        Viewport::with_physical_size(
            Size::new(physical_width, physical_height),
            self.physical_scale() as f32,
        )
    }

    fn get_cursor_position(&self) -> mouse::Cursor {
        mouse::Cursor::Available(iced::Point::new(
            self.input_state.get_pointer_position().0 as f32,
            self.input_state.get_pointer_position().1 as f32,
        ))
    }

    fn update_and_draw(&mut self, viewport: &Viewport, events: &[Event], cursor: mouse::Cursor) {
        // Build user interface (View)
        let mut user_interface = user_interface::UserInterface::build(
            self.iced_app.view(),
            viewport.logical_size(),
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        );

        // First pass: Update with input events and collect messages
        let mut messages = Vec::new();
        let (ui_state, _) = user_interface.update(
            &events,
            cursor,
            &mut self.renderer,
            &mut iced_core::clipboard::Null,
            &mut messages,
        );

        // Update mouse interaction based on UI state
        if let user_interface::State::Updated {
            mouse_interaction, ..
        } = ui_state
        {
            if self.mouse_interaction != mouse_interaction {
                self.mouse_interaction = mouse_interaction;
                get_app().set_cursor(iced_to_cursor_shape(mouse_interaction));
                trace!("Mouse interaction changed to: {:?}", mouse_interaction);
            }
        }

        // If we have messages, we need to update state and rebuild
        let has_messages = !messages.is_empty();

        if !has_messages {
            user_interface.update(
                &[Event::Window(RedrawRequested(std::time::Instant::now()))],
                cursor,
                &mut self.renderer,
                &mut iced_core::clipboard::Null,
                &mut Vec::new(),
            );
            user_interface.draw(
                &mut self.renderer,
                &Theme::Light,
                &Style {
                    text_color: Color::BLACK,
                },
                cursor,
            );
            self.cache = user_interface.into_cache();
        } else {
            // Store cache and update app state with messages
            self.cache = user_interface.into_cache();

            for message in &messages {
                self.iced_app.update(message.clone());
            }

            // Rebuild UI with updated app state
            let mut user_interface = user_interface::UserInterface::build(
                self.iced_app.view(),
                viewport.logical_size(),
                std::mem::take(&mut self.cache),
                &mut self.renderer,
            );

            // Draw the rebuilt UI
            user_interface.update(
                &[Event::Window(RedrawRequested(std::time::Instant::now()))],
                cursor,
                &mut self.renderer,
                &mut iced_core::clipboard::Null,
                &mut Vec::new(), // Discard any messages from this pass
            );
            user_interface.draw(
                &mut self.renderer,
                &Theme::Light,
                &Style {
                    text_color: Color::BLACK,
                },
                cursor,
            );
            self.cache = user_interface.into_cache();
        }
    }

    fn present_frame(
        &mut self,
        viewport: &Viewport,
        texture_view: &wgpu::TextureView,
        surface_texture: wgpu::SurfaceTexture,
    ) {
        if let iced_renderer::Renderer::Primary(wgpu_renderer) = &mut self.renderer {
            wgpu_renderer.present(
                Some(Color::WHITE),
                self.output_format,
                texture_view,
                viewport,
            );
        }
        surface_texture.present();
    }

    fn request_next_frame(&self) {
        self.wl_surface
            .frame(&self.queue_handle, self.wl_surface.clone());
        self.wl_surface.commit();
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

pub struct IcedWindow<A: IcedAppData> {
    pub window: Window,
    surface: IcedSurfaceState<A>,
}

impl<A: IcedAppData> IcedWindow<A> {
    pub fn new(window: Window, iced_app: A, width: u32, height: u32) -> Self {
        let mut surface = IcedSurfaceState::new(window.wl_surface().clone(), iced_app);
        surface.width = width;
        surface.height = height;
        Self { window, surface }
    }
}

impl<A: IcedAppData> CompositorHandlerContainer for IcedWindow<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: IcedAppData> KeyboardHandlerContainer for IcedWindow<A> {
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

impl<A: IcedAppData> PointerHandlerContainer for IcedWindow<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: IcedAppData> BaseTrait for IcedWindow<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.window.wl_surface().id()
    }
}

impl<A: IcedAppData> WindowContainer for IcedWindow<A> {
    fn configure(&mut self, configure: &WindowConfigure) {
        let width = configure.new_size.0.map_or(256, |size| size.get());
        let height = configure.new_size.1.map_or(256, |size| size.get());
        self.window
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }
}

pub struct IcedLayerSurface<A: IcedAppData> {
    pub layer_surface: LayerSurface,
    surface: IcedSurfaceState<A>,
}

impl<A: IcedAppData> IcedLayerSurface<A> {
    pub fn new(layer_surface: LayerSurface, iced_app: A, width: u32, height: u32) -> Self {
        let mut surface = IcedSurfaceState::new(layer_surface.wl_surface().clone(), iced_app);
        surface.width = width;
        surface.height = height;
        Self {
            layer_surface,
            surface,
        }
    }
}

impl<A: IcedAppData> CompositorHandlerContainer for IcedLayerSurface<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: IcedAppData> KeyboardHandlerContainer for IcedLayerSurface<A> {
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

impl<A: IcedAppData> PointerHandlerContainer for IcedLayerSurface<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: IcedAppData> BaseTrait for IcedLayerSurface<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.layer_surface.wl_surface().id()
    }
}

impl<A: IcedAppData> LayerSurfaceContainer for IcedLayerSurface<A> {
    fn configure(&mut self, config: &LayerSurfaceConfigure) {
        self.layer_surface
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(config.new_size.0, config.new_size.1);
    }
}

pub struct IcedPopup<A: IcedAppData> {
    pub popup: Popup,
    surface: IcedSurfaceState<A>,
}

impl<A: IcedAppData> IcedPopup<A> {
    pub fn new(popup: Popup, iced_app: A, width: u32, height: u32) -> Self {
        let mut surface = IcedSurfaceState::new(popup.wl_surface().clone(), iced_app);
        surface.width = width;
        surface.height = height;
        Self { popup, surface }
    }
}

impl<A: IcedAppData> CompositorHandlerContainer for IcedPopup<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: IcedAppData> KeyboardHandlerContainer for IcedPopup<A> {
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

impl<A: IcedAppData> PointerHandlerContainer for IcedPopup<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: IcedAppData> BaseTrait for IcedPopup<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.popup.wl_surface().id()
    }
}

impl<A: IcedAppData> PopupContainer for IcedPopup<A> {
    fn configure(&mut self, config: &PopupConfigure) {
        self.popup
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface
            .configure(config.width as u32, config.height as u32);
    }

    fn done(&mut self) {}
}

pub struct IcedSubsurface<A: IcedAppData> {
    pub wl_surface: WlSurface,
    surface: IcedSurfaceState<A>,
}

impl<A: IcedAppData> IcedSubsurface<A> {
    pub fn new(wl_surface: WlSurface, iced_app: A, width: u32, height: u32) -> Self {
        let mut surface = IcedSurfaceState::new(wl_surface.clone(), iced_app);
        surface.width = width;
        surface.height = height;
        Self {
            wl_surface,
            surface,
        }
    }
}

impl<A: IcedAppData> CompositorHandlerContainer for IcedSubsurface<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: IcedAppData> KeyboardHandlerContainer for IcedSubsurface<A> {
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

impl<A: IcedAppData> PointerHandlerContainer for IcedSubsurface<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: IcedAppData> BaseTrait for IcedSubsurface<A> {
    fn get_object_id(&self) -> wayland_backend::client::ObjectId {
        self.wl_surface.id()
    }
}

impl<A: IcedAppData> SubsurfaceContainer for IcedSubsurface<A> {
    fn configure(&mut self, width: u32, height: u32) {
        self.wl_surface.set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }
}

fn iced_to_cursor_shape(interaction: mouse::Interaction) -> Shape {
    use mouse::Interaction;
    match interaction {
        Interaction::Idle | Interaction::None => Shape::Default,
        Interaction::Pointer => Shape::Pointer,
        Interaction::Grab => Shape::Grab,
        Interaction::Grabbing => Shape::Grabbing,
        Interaction::Crosshair => Shape::Crosshair,
        Interaction::Text => Shape::Text,
        Interaction::NotAllowed => Shape::NotAllowed,
        Interaction::ZoomIn => Shape::ZoomIn,
        Interaction::ZoomOut => Shape::ZoomOut,
        Interaction::Cell => Shape::Cell,
        Interaction::Move => Shape::Move,
        Interaction::Copy => Shape::Copy,
        Interaction::NoDrop => Shape::NoDrop,
        Interaction::Alias => Shape::Alias,
        Interaction::ContextMenu => Shape::ContextMenu,
        Interaction::Help => Shape::Help,
        Interaction::AllScroll => Shape::AllScroll,
        Interaction::Progress => Shape::Progress,
        Interaction::Wait => Shape::Wait,
        Interaction::ResizingHorizontally => Shape::EwResize,
        Interaction::ResizingVertically => Shape::NsResize,
        Interaction::Hidden => {
            // TODO: This is not a cursor, it's request to hide it
            Shape::Default
        }
        // What is ResizingDiagonallyUp and ResizingDiagonallyDown?
        Interaction::ResizingDiagonallyUp => Shape::NeswResize,
        Interaction::ResizingDiagonallyDown => Shape::NwseResize,
        Interaction::ResizingColumn => Shape::ColResize,
        Interaction::ResizingRow => Shape::RowResize,
    }
}
