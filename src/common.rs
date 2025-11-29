use log::trace;
use smithay_client_toolkit::{compositor::CompositorHandler, delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, output::{OutputHandler, OutputState}, registry::{ProvidesRegistryState, RegistryState}, registry_handlers, seat::{Capability, SeatHandler, SeatState, keyboard::{KeyEvent, KeyboardHandler}, pointer::{PointerEvent, PointerHandler, ThemeSpec, ThemedPointer}}, shell::{wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure}, xdg::window::Window}, shm::{Shm, ShmHandler}};
use wayland_client::{Connection, QueueHandle, protocol::{wl_output, wl_seat, wl_surface::WlSurface}};

use crate::InputState;

enum WindowKind {
    LayerSurface(LayerSurface),
    Window(Window)
}

struct WaylandWindow {
    width: u32,
    height: u32,
    scale_factor: i32,
    themed_pointer: Option<ThemedPointer>,
    kind: WindowKind,
}

struct Application {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm_state: Shm,
    windows: Vec<WaylandWindow>,
    input_state: InputState,
}

impl CompositorHandler for Application {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        new_factor: i32,
    ) {
        trace!("[MAIN] Scale factor changed to {}", new_factor);
        // self.scale_factor = new_factor;
        // // Request a redraw with the new scale factor
        // self.layer_surface.wl_surface().frame(qh, self.layer_surface.wl_surface().clone());
        // self.layer_surface.wl_surface().commit();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        // Not needed for this example.
    }

    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _time: u32,
    ) {
        trace!("[MAIN] Frame callback");
        // self.render(conn, qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Not needed for this example.
    }
}

impl OutputHandler for Application {
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

impl LayerShellHandler for Application {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        // self.exit = true;
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // trace!("[MAIN] Configure called");
        // let (new_width, new_height) = configure.new_size;
        // self.width = new_width.max(1);
        // self.height = new_height.max(1);
        // self.input_state.set_screen_size(self.width, self.height);
        // trace!("[MAIN] Layer surface size: {}x{}", self.width, self.height);

        // let adapter = &self.adapter;
        // let surface = &self.surface;
        // let device = &self.device;

        // let cap = surface.get_capabilities(&adapter);
        // let surface_config = wgpu::SurfaceConfiguration {
        //     usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        //     format: cap.formats[0],
        //     view_formats: vec![cap.formats[0]],
        //     alpha_mode: wgpu::CompositeAlphaMode::Auto,
        //     width: self.width * self.scale_factor as u32,
        //     height: self.height * self.scale_factor as u32,
        //     desired_maximum_frame_latency: 2,
        //     // Wayland is inherently a mailbox system.
        //     present_mode: wgpu::PresentMode::Mailbox,
        // };

        // surface.configure(&self.device, &surface_config);
        
        // // Tell Wayland we're providing a buffer at scale_factor resolution
        // self.layer_surface.wl_surface().set_buffer_scale(self.scale_factor);

        // // Initialize EGUI renderer if not already done
        // // if self.egui_renderer.is_none() {
        // //     self.egui_renderer = Some(EguiRenderer::new(
        // //         device,
        // //         surface_config.format,
        // //         None,
        // //         1,
        // //     ));
        // // }

        // // Render the frame
        // self.render(conn, qh);
    }
}

impl PointerHandler for Application {
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
        // self.layer_surface.wl_surface().frame(&_qh, self.layer_surface.wl_surface().clone());
        // self.layer_surface.wl_surface().commit();
    }
}

impl KeyboardHandler for Application {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &WlSurface,
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
        _surface: &WlSurface,
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
        // self.layer_surface.wl_surface().frame(&_qh, self.layer_surface.wl_surface().clone());
        // self.layer_surface.wl_surface().commit();
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
        // self.layer_surface.wl_surface().frame(&_qh, self.layer_surface.wl_surface().clone());
        // self.layer_surface.wl_surface().commit();
    }
}

impl SeatHandler for Application {
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
        if capability == Capability::Keyboard && self.seat_state.get_keyboard(qh, &seat, None).is_err() {
            trace!("[MAIN] Failed to get keyboard");
        }
        // if capability == Capability::Pointer && self.themed_pointer.is_none() {
        //     trace!("[MAIN] Creating themed pointer");
        //     let surface = self.layer_surface.wl_surface().clone();
        //     match self.seat_state.get_pointer_with_theme(
        //         qh,
        //         &seat,
        //         self.shm_state.wl_shm(),
        //         surface,
        //         ThemeSpec::default(),
        //     ) {
        //         Ok(themed_pointer) => {
        //             self.themed_pointer = Some(themed_pointer);
        //             trace!("[MAIN] Themed pointer created successfully");
        //         }
        //         Err(e) => {
        //             trace!("[MAIN] Failed to create themed pointer: {:?}", e);
        //         }
        //     }
        // }
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

impl ShmHandler for Application {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl ProvidesRegistryState for Application {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(Application);
delegate_output!(Application);
delegate_shm!(Application);

delegate_seat!(Application);
delegate_keyboard!(Application);
delegate_pointer!(Application);

delegate_layer!(Application);

delegate_registry!(Application);
