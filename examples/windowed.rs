// Original here: https://github.com/Smithay/client-toolkit/blob/master/examples/wgpu.rs
//
// This is old example that doesn't use the Application wrapper

use egui::{CentralPanel, Context};
use egui_smithay::*;

use crate::EguiWgpuRenderer;
use crate::WaylandToEguiInput;
use log::trace;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler},
        pointer::{
            CursorIcon as WaylandCursorIcon, PointerEvent, PointerHandler, ThemeSpec, ThemedPointer,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use smithay_clipboard::Clipboard;
use std::ptr::NonNull;
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
};
use wgpu::DeviceDescriptor;

struct EguiApp {
    counter: i32,
    text: String,
}

impl EguiApp {
    pub fn new() -> Self {
        Self {
            counter: 0,
            text: String::from("Hello from EGUI!"),
        }
    }

    pub fn ui(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Egui WGPU / Smithay example");

            ui.separator();

            ui.label(format!("Counter: {}", self.counter));
            if ui.button("Increment").clicked() {
                self.counter += 1;
            }
            if ui.button("Decrement").clicked() {
                self.counter -= 1;
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Text input:");
                ui.text_edit_singleline(&mut self.text);
            });

            ui.label(format!("You wrote: {}", self.text));

            ui.separator();

            ui.label("This is a simple EGUI app running on Wayland via Smithay toolkit!");
        });
    }
}

impl Default for EguiApp {
    fn default() -> Self {
        Self::new()
    }
}
fn main() {
    env_logger::init();

    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    // Initialize xdg_shell handlers so we can select the correct adapter
    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let xdg_shell_state = XdgShell::bind(&globals, &qh).expect("xdg shell not available");
    let shm_state = Shm::bind(&globals, &qh).expect("wl_shm not available");

    let surface = compositor_state.create_surface(&qh);
    // Create the window for adapter selection
    let window = xdg_shell_state.create_window(surface, WindowDecorations::ServerDefault, &qh);
    window.set_title("wgpu wayland window");
    // GitHub does not let projects use the `org.github` domain but the `io.github` domain is fine.
    window.set_app_id("io.github.smithay.client-toolkit.WgpuExample");
    window.set_min_size(Some((256, 256)));
    window.commit();

    // Initialize wgpu
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    // Create the raw window handle for the surface.
    let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
        NonNull::new(conn.backend().display_ptr() as *mut _).unwrap(),
    ));
    let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(window.wl_surface().id().as_ptr() as *mut _).unwrap(),
    ));

    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
            .unwrap()
    };

    // Pick a supported adapter
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .expect("Failed to find suitable adapter");

    log::info!("Selected backend: {:?}", adapter.get_info().backend);

    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))
    .expect("Failed to request device");

    // Initialize clipboard
    let clipboard = unsafe { Clipboard::new(conn.display().id().as_ptr() as *mut _) };

    let mut main_state = MainState {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm_state,

        exit: false,
        width: 256,
        height: 256,
        scale_factor: 1,
        window,
        device,
        surface,
        adapter,
        queue,

        egui_renderer: None,
        egui_app: EguiApp::new(),
        input_state: WaylandToEguiInput::new(clipboard),
        themed_pointer: None,
    };

    // We don't draw immediately, the configure will notify us when to first draw.
    loop {
        event_queue.blocking_dispatch(&mut main_state).unwrap();

        if main_state.exit {
            trace!("exiting example");
            break;
        }
    }

    // On exit we must destroy the surface before the window is destroyed.
    drop(main_state.surface);
    drop(main_state.window);
}

struct MainState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm_state: Shm,

    exit: bool,
    width: u32,
    height: u32,
    scale_factor: i32,
    window: Window,

    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,

    egui_renderer: Option<EguiWgpuRenderer>,
    egui_app: EguiApp,
    input_state: WaylandToEguiInput,
    themed_pointer: Option<ThemedPointer>,
}

impl MainState {
    fn render(&mut self, conn: &Connection, qh: &QueueHandle<Self>) {
        trace!("[MAIN] Render called");

        if self.egui_renderer.is_none() {
            trace!("[MAIN] Skipping render - EGUI renderer not initialized yet");
            return;
        }

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(e) => {
                trace!("[MAIN] Failed to acquire swapchain texture: {:?}", e);
                return;
            }
        };

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // Clear the surface first
        {
            let _renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    depth_slice: None,
                    resolve_target: None,
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

        // Render EGUI
        let needs_repaint = if let Some(renderer) = &mut self.egui_renderer {
            let raw_input = self.input_state.take_raw_input();

            renderer.begin_frame(raw_input);
            self.egui_app.ui(renderer.context());

            // For Wayland: configure surface at physical resolution, render egui at logical resolution
            // pixels_per_point tells egui how many physical pixels per logical point
            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [
                    self.width * self.scale_factor as u32,
                    self.height * self.scale_factor as u32,
                ],
                pixels_per_point: self.scale_factor as f32,
            };

            let platform_output = renderer.end_frame_and_draw(
                &self.device,
                &self.queue,
                &mut encoder,
                &texture_view,
                screen_descriptor,
            );

            // Handle clipboard commands from egui
            for command in &platform_output.commands {
                self.input_state.handle_output_command(command);
            }

            // Handle cursor icon changes from EGUI
            if let Some(themed_pointer) = &self.themed_pointer {
                let cursor_icon = egui_to_wayland_cursor(platform_output.cursor_icon);
                let _ = themed_pointer.set_cursor(conn, cursor_icon);
            }

            // For now, just check if there are any platform commands (indicates interaction)
            !platform_output.events.is_empty()
        } else {
            false
        };

        // Submit the command in the queue to execute
        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // Only request next frame if EGUI needs repaint (animations, etc.)
        if needs_repaint {
            trace!("[MAIN] EGUI has events, scheduling next frame");
            self.window
                .wl_surface()
                .frame(qh, self.window.wl_surface().clone());
            self.window.wl_surface().commit();
        }
    }
}

impl CompositorHandler for MainState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        trace!("[MAIN] Scale factor changed to {}", new_factor);
        self.scale_factor = new_factor;
        // Request a redraw with the new scale factor
        self.window
            .wl_surface()
            .frame(qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        // Not needed for this example.
    }

    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        trace!("[MAIN] Frame callback");
        self.render(conn, qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }
}

impl OutputHandler for MainState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for MainState {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        trace!("[MAIN] Configure called");
        let (new_width, new_height) = configure.new_size;
        self.width = new_width.map_or(256, |v| v.get());
        self.height = new_height.map_or(256, |v| v.get());
        self.input_state.set_screen_size(self.width, self.height);
        trace!("[MAIN] Window size: {}x{}", self.width, self.height);

        let adapter = &self.adapter;
        let surface = &self.surface;
        let device = &self.device;

        let cap = surface.get_capabilities(&adapter);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: cap.formats[0],
            view_formats: vec![cap.formats[0]],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.width * self.scale_factor as u32,
            height: self.height * self.scale_factor as u32,
            desired_maximum_frame_latency: 2,
            // Wayland is inherently a mailbox system.
            present_mode: wgpu::PresentMode::Mailbox,
        };

        surface.configure(&self.device, &surface_config);

        // Tell Wayland we're providing a buffer at scale_factor resolution
        self.window.wl_surface().set_buffer_scale(self.scale_factor);

        // Initialize EGUI renderer if not already done
        if self.egui_renderer.is_none() {
            self.egui_renderer = Some(EguiWgpuRenderer::new(
                device,
                surface_config.format,
                None,
                1,
            ));
        }

        // Render the frame
        self.render(conn, qh);
    }
}

impl PointerHandler for MainState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wayland_client::protocol::wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        trace!("[MAIN] Pointer frame with {} events", events.len());
        for event in events {
            self.input_state.handle_pointer_event(event);
        }
        // Request a redraw after input
        trace!("[MAIN] Requesting frame after pointer input");
        self.window
            .wl_surface()
            .frame(&_qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }
}

impl KeyboardHandler for MainState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
        trace!("[MAIN] Keyboard focus gained");
        // Keyboard focus gained
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        trace!("[MAIN] Keyboard focus lost");
        // Keyboard focus lost
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        trace!("[MAIN] Key pressed");

        self.input_state.handle_keyboard_event(&event, true, false);

        // Request a redraw after input
        trace!("[MAIN] Requesting frame after key press");
        self.window
            .wl_surface()
            .frame(&_qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.input_state.handle_keyboard_event(&event, false, false);
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: smithay_client_toolkit::seat::keyboard::Modifiers,
        _raw_modifiers: smithay_client_toolkit::seat::keyboard::RawModifiers,
        _layout: u32,
    ) {
        self.input_state.update_modifiers(&modifiers);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.input_state.handle_keyboard_event(&event, true, true);
        // Request a redraw after input
        self.window
            .wl_surface()
            .frame(&_qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }
}

impl SeatHandler for MainState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        trace!("[MAIN] New seat capability: {:?}", capability);
        if capability == Capability::Keyboard
            && self.seat_state.get_keyboard(qh, &seat, None).is_err()
        {
            trace!("[MAIN] Failed to get keyboard");
        }
        if capability == Capability::Pointer && self.themed_pointer.is_none() {
            trace!("[MAIN] Creating themed pointer");
            let surface = self.window.wl_surface().clone();
            match self.seat_state.get_pointer_with_theme(
                qh,
                &seat,
                self.shm_state.wl_shm(),
                surface,
                ThemeSpec::default(),
            ) {
                Ok(themed_pointer) => {
                    self.themed_pointer = Some(themed_pointer);
                    trace!("[MAIN] Themed pointer created successfully");
                }
                Err(e) => {
                    trace!("[MAIN] Failed to create themed pointer: {:?}", e);
                }
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl ShmHandler for MainState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

/// Convert EGUI cursor icon to Wayland cursor icon
fn egui_to_wayland_cursor(cursor: egui::CursorIcon) -> WaylandCursorIcon {
    use egui::CursorIcon::*;
    use smithay_client_toolkit::seat::pointer::CursorIcon as WCI;

    match cursor {
        Default => WCI::Default,
        None => WCI::Default,
        ContextMenu => WCI::ContextMenu,
        Help => WCI::Help,
        PointingHand => WCI::Pointer,
        Progress => WCI::Progress,
        Wait => WCI::Wait,
        Cell => WCI::Cell,
        Crosshair => WCI::Crosshair,
        Text => WCI::Text,
        VerticalText => WCI::VerticalText,
        Alias => WCI::Alias,
        Copy => WCI::Copy,
        Move => WCI::Move,
        NoDrop => WCI::NoDrop,
        NotAllowed => WCI::NotAllowed,
        Grab => WCI::Grab,
        Grabbing => WCI::Grabbing,
        AllScroll => WCI::AllScroll,
        ResizeHorizontal => WCI::EwResize,
        ResizeNeSw => WCI::NeswResize,
        ResizeNwSe => WCI::NwseResize,
        ResizeVertical => WCI::NsResize,
        ResizeEast => WCI::EResize,
        ResizeSouthEast => WCI::SeResize,
        ResizeSouth => WCI::SResize,
        ResizeSouthWest => WCI::SwResize,
        ResizeWest => WCI::WResize,
        ResizeNorthWest => WCI::NwResize,
        ResizeNorth => WCI::NResize,
        ResizeNorthEast => WCI::NeResize,
        ResizeColumn => WCI::ColResize,
        ResizeRow => WCI::RowResize,
        ZoomIn => WCI::ZoomIn,
        ZoomOut => WCI::ZoomOut,
    }
}

delegate_compositor!(MainState);
delegate_output!(MainState);
delegate_shm!(MainState);

delegate_seat!(MainState);
delegate_keyboard!(MainState);
delegate_pointer!(MainState);

delegate_xdg_shell!(MainState);
delegate_xdg_window!(MainState);

delegate_registry!(MainState);

impl ProvidesRegistryState for MainState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
