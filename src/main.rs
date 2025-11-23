mod egui_renderer;
mod egui_app;
mod input_handler;

use crate::egui_renderer::EguiRenderer;
use crate::egui_app::EguiApp;
use crate::input_handler::InputState;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_seat, delegate_xdg_shell,
    delegate_xdg_window, delegate_keyboard, delegate_pointer,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyboardHandler, KeyEvent},
        pointer::{PointerHandler, PointerEvent},
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
};
use std::ptr::NonNull;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
    Connection, Proxy, QueueHandle,
};

fn main() {
    env_logger::init();

    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    // Initialize xdg_shell handlers so we can select the correct adapter
    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let xdg_shell_state = XdgShell::bind(&globals, &qh).expect("xdg shell not available");

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

    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
        .expect("Failed to request device");

    let mut wgpu = Wgpu {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),

        exit: false,
        width: 256,
        height: 256,
        window,
        device,
        surface,
        adapter,
        queue,
        
        egui_renderer: None,
        egui_app: EguiApp::new(),
        input_state: InputState::new(),
    };

    // We don't draw immediately, the configure will notify us when to first draw.
    loop {
        event_queue.blocking_dispatch(&mut wgpu).unwrap();

        if wgpu.exit {
            println!("exiting example");
            break;
        }
    }

    // On exit we must destroy the surface before the window is destroyed.
    drop(wgpu.surface);
    drop(wgpu.window);
}

struct Wgpu {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,

    exit: bool,
    width: u32,
    height: u32,
    window: Window,

    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    
    egui_renderer: Option<EguiRenderer>,
    egui_app: EguiApp,
    input_state: InputState,
}

impl Wgpu {
    fn render(&mut self, qh: &QueueHandle<Self>) {
        println!("[MAIN] Render called");
        
        if self.egui_renderer.is_none() {
            println!("[MAIN] Skipping render - EGUI renderer not initialized yet");
            return;
        }

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(e) => {
                println!("[MAIN] Failed to acquire swapchain texture: {:?}", e);
                return;
            }
        };
        
        let texture_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
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
            
            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [self.width, self.height],
                pixels_per_point: 1.0,
            };
            
            let platform_output = renderer.end_frame_and_draw(
                &self.device,
                &self.queue,
                &mut encoder,
                &texture_view,
                screen_descriptor,
            );
            
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
            println!("[MAIN] EGUI has events, scheduling next frame");
            self.window.wl_surface().frame(qh, self.window.wl_surface().clone());
            self.window.wl_surface().commit();
        }
    }
}

impl CompositorHandler for Wgpu {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // Not needed for this example.
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
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        println!("[MAIN] Frame callback");
        self.render(qh);
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

impl OutputHandler for Wgpu {
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

impl WindowHandler for Wgpu {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        println!("[MAIN] Configure called");
        let (new_width, new_height) = configure.new_size;
        self.width = new_width.map_or(256, |v| v.get());
        self.height = new_height.map_or(256, |v| v.get());
        self.input_state.set_screen_size(self.width, self.height);
        println!("[MAIN] Window size: {}x{}", self.width, self.height);

        let adapter = &self.adapter;
        let surface = &self.surface;
        let device = &self.device;

        let cap = surface.get_capabilities(&adapter);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: cap.formats[0],
            view_formats: vec![cap.formats[0]],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.width,
            height: self.height,
            desired_maximum_frame_latency: 2,
            // Wayland is inherently a mailbox system.
            present_mode: wgpu::PresentMode::Mailbox,
        };

        surface.configure(&self.device, &surface_config);

        // Initialize EGUI renderer if not already done
        if self.egui_renderer.is_none() {
            self.egui_renderer = Some(EguiRenderer::new(
                device,
                surface_config.format,
                None,
                1,
            ));
        }

        // Render the frame
        self.render(qh);
    }
}

impl PointerHandler for Wgpu {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wayland_client::protocol::wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        println!("[MAIN] Pointer frame with {} events", events.len());
        for event in events {
            self.input_state.handle_pointer_event(event);
        }
        // Request a redraw after input
        println!("[MAIN] Requesting frame after pointer input");
        self.window.wl_surface().frame(&_qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }
}

impl KeyboardHandler for Wgpu {
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
        println!("[MAIN] Keyboard focus gained");
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
        println!("[MAIN] Keyboard focus lost");
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
        println!("[MAIN] Key pressed");
        self.input_state.handle_keyboard_event(&event, true);
        // Request a redraw after input
        println!("[MAIN] Requesting frame after key press");
        self.window.wl_surface().frame(&_qh, self.window.wl_surface().clone());
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
        self.input_state.handle_keyboard_event(&event, false);
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
        self.input_state.handle_keyboard_event(&event, true);
        // Request a redraw after input
        self.window.wl_surface().frame(&_qh, self.window.wl_surface().clone());
        self.window.wl_surface().commit();
    }
}

impl SeatHandler for Wgpu {
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
        println!("[MAIN] New seat capability: {:?}", capability);
        if capability == Capability::Keyboard && self.seat_state.get_keyboard(qh, &seat, None).is_err() {
            println!("[MAIN] Failed to get keyboard");
        }
        if capability == Capability::Pointer && self.seat_state.get_pointer(qh, &seat).is_err() {
            println!("[MAIN] Failed to get pointer");
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

delegate_compositor!(Wgpu);
delegate_output!(Wgpu);

delegate_seat!(Wgpu);
delegate_keyboard!(Wgpu);
delegate_pointer!(Wgpu);

delegate_xdg_shell!(Wgpu);
delegate_xdg_window!(Wgpu);

delegate_registry!(Wgpu);

impl ProvidesRegistryState for Wgpu {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}