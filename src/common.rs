use std::{num::NonZero, rc::{Rc, Weak}};

use log::trace;
use smithay_client_toolkit::{compositor::{CompositorHandler, CompositorState}, delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_subcompositor, delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window, output::{OutputHandler, OutputState}, registry::{ProvidesRegistryState, RegistryState}, registry_handlers, seat::{Capability, SeatHandler, SeatState, keyboard::{KeyEvent, KeyboardHandler, Keysym}, pointer::{PointerEvent, PointerHandler, ThemedPointer}}, shell::{WaylandSurface, wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure}, xdg::{XdgShell, popup::{Popup, PopupConfigure, PopupHandler}, window::{Window, WindowConfigure, WindowDecorations, WindowHandler}}}, shm::{Shm, ShmHandler, slot::{Buffer, SlotPool}}};
use wayland_client::{Connection, Proxy, QueueHandle, protocol::{wl_keyboard::WlKeyboard, wl_output, wl_pointer::WlPointer, wl_seat, wl_shm, wl_surface::WlSurface}};

use crate::InputState;

pub struct LayerSurfaceOptions {
    anchor: Anchor,
    keyboard_interactivity: KeyboardInteractivity,
    size: (u32, u32),
    surface: WlSurface,
}

pub struct Application {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub shm_state: Shm,
    pub windows: Vec<Weak<Window>>,
    pub layer_surfaces: Vec<Weak<LayerSurface>>,
    pub input_state: InputState,
    // Pool used to create shm buffers for simple software presentation in examples
    pub pool: Option<SlotPool>,
}

impl Application {
    /// Create a new Application container from the provided globals state pieces.
    pub fn new(registry_state: RegistryState, seat_state: SeatState, output_state: OutputState, shm_state: Shm, input_state: InputState) -> Self {
        Self {
            registry_state,
            seat_state,
            output_state,
            shm_state,
            windows: Vec::new(),
            layer_surfaces: Vec::new(),
            input_state,
            pool: None,
        }
    }


    fn find_window_by_surface(&self, surface: &WlSurface) -> Option<Weak<Window>> {
        for win in &self.windows {
            if let Some(strong_win) = win.upgrade() {
                if strong_win.wl_surface().id().as_ptr() == surface.id().as_ptr() {
                    return Some(Rc::downgrade(&strong_win));
                }
            }
        }
        None
    }

    fn find_layer_by_surface(&self, surface: &WlSurface) -> Option<Weak<LayerSurface>> {
        for layer in &self.layer_surfaces {
            if let Some(strong_layer) = layer.upgrade() {
        
                if strong_layer.wl_surface().id().as_ptr() == surface.id().as_ptr() {
                    return Some(Rc::downgrade(&strong_layer));
                }
            }
        }
        None
    }

    pub fn single_color_example_buffer_configure(&mut self, surface: &WlSurface, qh: &QueueHandle<Self>, new_width: u32, new_height: u32, color: (u8, u8, u8)) {

        trace!("[COMMON] Create Brown Buffer");

        // Ensure pool exists
        if self.pool.is_none() {
            let size = (new_width as usize) * (new_height as usize) * 4;
            self.pool = Some(SlotPool::new(size, &self.shm_state).expect("Failed to create SlotPool"));
        }

        let pool = self.pool.as_mut().unwrap();
        let stride = new_width as i32 * 4;

        // Create a buffer and paint it a simple color
        let (buffer, _maybe_canvas) = pool.create_buffer(new_width as i32, new_height as i32, stride, wl_shm::Format::Argb8888).expect("create buffer");
        if let Some(canvas) = pool.canvas(&buffer) {
            for chunk in canvas.chunks_exact_mut(4) {
                // ARGB little-endian: B, G, R, A
                chunk[0] = color.2; // B
                chunk[1] = color.1; // G
                chunk[2] = color.0; // R
                chunk[3] = 0xFF; // A
            }
        }

        // Damage, frame and attach
        surface.damage_buffer(0, 0, new_width as i32, new_height as i32);
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface).expect("buffer attach");
        surface.commit();
    }
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

        _surface.frame(qh, _surface.clone());
        _surface.commit();
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
        surface: &WlSurface,
        _time: u32,
    ) {
        trace!("[MAIN] Frame callback {}", surface.id().as_ptr() as usize);
        if let Some(layer) = self.find_layer_by_surface(surface).and_then(|weak| weak.upgrade()) {
            trace!("[MAIN] Found layer surface for frame");
            // layer.wl_surface().frame(qh, layer.wl_surface().clone());
            // layer.wl_surface().commit();
        }

        if let Some(window) = self.find_window_by_surface(surface).and_then(|weak| weak.upgrade()) {
            trace!("[MAIN] Found xdg window for frame");
            // window.wl_surface().frame(qh, window.wl_surface().clone());
            // window.wl_surface().commit();
        }
        // self.render(conn, qh);
        // if needs_repaint {

        // This would render in loop:
        // _surface.damage_buffer(0, 0, 256, 256);
        // _surface.frame(qh, _surface.clone());
        // _surface.commit();
        // }
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
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        target_layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        trace!("[COMMON] XDG layer configure");
        self.single_color_example_buffer_configure(target_layer.wl_surface(), &qh, configure.new_size.0, configure.new_size.1, (0, 255, 0));
    }
}

impl PopupHandler for Application {
    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        trace!("[COMMON] XDG popup configure");

        self.single_color_example_buffer_configure(popup.wl_surface(), qh, config.width as u32, config.height as u32, (255, 0, 255));
    }

    fn done(&mut self, conn: &Connection, qh: &QueueHandle<Self>, popup: &Popup) {
        
    }
}

impl WindowHandler for Application {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        // No-op for this simple helper container
        trace!("[COMMON] XDG window close requested");
        // self.windows.clear();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        target_window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        trace!("[COMMON] XDG window configure");
        let width = configure.new_size.0.unwrap_or_else(|| NonZero::new(256).unwrap()).get();
        let height = configure.new_size.1.unwrap_or_else(|| NonZero::new(256).unwrap()).get();
        self.single_color_example_buffer_configure(target_window.wl_surface(), &qh, width, height, (255,0, 0));
    }
}

impl PointerHandler for Application {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &WlPointer,
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
        _keyboard: &WlKeyboard,
        _surface: &WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        trace!("[MAIN] Keyboard focus gained");
        // Keyboard focus gained
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
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
        _keyboard: &WlKeyboard,
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
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.input_state.handle_keyboard_event(&event, false, false);
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
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
        _keyboard: &WlKeyboard,
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
delegate_subcompositor!(Application);
delegate_output!(Application);
delegate_shm!(Application);

delegate_seat!(Application);
delegate_keyboard!(Application);
delegate_pointer!(Application);

delegate_layer!(Application);

delegate_xdg_shell!(Application);
delegate_xdg_window!(Application);
delegate_xdg_popup!(Application);

delegate_registry!(Application);
