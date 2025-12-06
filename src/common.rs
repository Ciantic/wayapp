use std::{cell::RefCell, collections::HashMap, num::NonZero, rc::{Rc, Weak}};

use log::trace;
use smithay_client_toolkit::{compositor::{CompositorHandler, CompositorState}, delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_subcompositor, delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window, output::{OutputHandler, OutputState}, registry::{ProvidesRegistryState, RegistryState}, registry_handlers, seat::{Capability, SeatHandler, SeatState, keyboard::{KeyEvent, KeyboardHandler, Keysym}, pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager}}, shell::{WaylandSurface, wlr_layer::{Anchor, KeyboardInteractivity, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure}, xdg::{XdgShell, popup::{Popup, PopupConfigure, PopupHandler}, window::{Window, WindowConfigure, WindowHandler}}}, shm::{Shm, ShmHandler, slot::SlotPool}, subcompositor::SubcompositorState};
use smithay_clipboard::Clipboard;
use wayland_backend::client::ObjectId;
use wayland_client::{Connection, EventQueue, Proxy, QueueHandle, globals::registry_queue_init, protocol::{wl_keyboard::WlKeyboard, wl_output, wl_pointer::WlPointer, wl_seat, wl_shm, wl_surface::WlSurface}};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};

use crate::{InputState};


pub struct Application {
    pub conn: Connection,
    pub event_queue: Option<EventQueue<Self>>,
    pub qh: QueueHandle<Self>,
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub shm_state: Shm,
    pub compositor_state: CompositorState,
    pub subcompositor_state: SubcompositorState,
    pub xdg_shell: XdgShell,
    pub layer_shell: LayerShell,
    pub windows: Vec<Box<dyn WindowContainer>>,
    pub layer_surfaces: Vec<Box<dyn LayerSurfaceContainer>>,
    pub popups: Vec<Box<dyn PopupContainer>>,

    pub input_state: InputState,
    pub cursor_shape_manager: CursorShapeManager,
    
    // Pool used to create shm buffers for simple software presentation in examples
    pub pool: Option<SlotPool>,

    /// For cursor set_shape to work serial parameter must match the latest wl_pointer.enter or zwp_tablet_tool_v2.proximity_in serial number sent to the client.
    last_pointer_enter_serial: Option<u32>,
    // Cache cursor shape devices per pointer to avoid repeated protocol calls
    pointer_shape_devices: HashMap<u32, WpCursorShapeDeviceV1>,
}

impl Application {
    /// Create a new Application, initializing all Wayland globals and state.
    pub fn new() -> Self {
        let conn = Connection::connect_to_env().expect("Failed to connect to Wayland");
        let (globals, event_queue) = registry_queue_init::<Self>(&conn).expect("Failed to init registry");
        let qh: QueueHandle<Self> = event_queue.handle();

        // Bind required globals
        let compositor_state = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
        let subcompositor_state = SubcompositorState::bind(compositor_state.wl_compositor().clone(), &globals, &qh).expect("wl_subcompositor not available");
        let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell not available");
        let shm_state = Shm::bind(&globals, &qh).expect("wl_shm not available");
        let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell not available");
        let cursor_shape_manager = CursorShapeManager::bind(&globals, &qh).expect("cursor shape manager not available");
        let clipboard = unsafe { Clipboard::new(conn.display().id().as_ptr() as *mut _) };
        
        Self {
            event_queue: Some(event_queue),
            conn,
            qh: qh.clone(),
            subcompositor_state,
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state: OutputState::new(&globals, &qh),
            shm_state,
            compositor_state,
            xdg_shell,
            layer_shell,
            windows: Vec::new(),
            layer_surfaces: Vec::new(),
            popups: Vec::new(),
            // windows: Vec::new(),
            // layer_surfaces: Vec::new(),
            input_state: InputState::new(clipboard),
            cursor_shape_manager,
            pool: None,
            last_pointer_enter_serial: None,
            pointer_shape_devices: HashMap::new(),
        }
    }

    pub fn run_blocking(&mut self) {       
        // Run the Wayland event loop. This example will run until the process is killed
        let mut event_queue = self.event_queue.take().unwrap();
        loop {
            event_queue.blocking_dispatch(self).expect("Wayland dispatch failed");
        }
    }

    // fn find_window_by_surface(&self, surface: &WlSurface) -> Option<Weak<Window>> {
    //     for win in &self.windows {
    //         if let Some(strong_win) = win.upgrade() {
    //             if strong_win.wl_surface().id().as_ptr() == surface.id().as_ptr() {
    //                 return Some(Rc::downgrade(&strong_win));
    //             }
    //         }
    //     }
    //     None
    // }

    // fn find_layer_by_surface(&self, surface: &WlSurface) -> Option<Weak<LayerSurface>> {
    //     for layer in &self.layer_surfaces {
    //         if let Some(strong_layer) = layer.upgrade() {
        
    //             if strong_layer.wl_surface().id().as_ptr() == surface.id().as_ptr() {
    //                 return Some(Rc::downgrade(&strong_layer));
    //             }
    //         }
    //     }
    //     None
    // }

    fn get_or_create_shape_device(&mut self, pointer: &WlPointer, qh: &QueueHandle<Self>) -> &WpCursorShapeDeviceV1 {
        let pointer_id = pointer.id().as_ptr() as u32;
        self.pointer_shape_devices.entry(pointer_id).or_insert_with(|| {
            trace!("[COMMON] Creating new cursor shape device for pointer id {}", pointer_id);
            self.cursor_shape_manager.get_shape_device(pointer, qh)
        })
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
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        _time: u32,
    ) {
        trace!("[MAIN] Frame callback {}", surface.id().as_ptr() as usize);
        // if let Some(_layer) = self.find_layer_by_surface(surface).and_then(|weak| weak.upgrade()) {
        //     trace!("[MAIN] Found layer surface for frame");
        //     // layer.wl_surface().frame(qh, layer.wl_surface().clone());
        //     // layer.wl_surface().commit();
        // }

        // if let Some(_window) = self.find_window_by_surface(surface).and_then(|weak| weak.upgrade()) {
        //     trace!("[MAIN] Found xdg window for frame");
        //     // window.wl_surface().frame(qh, window.wl_surface().clone());
        //     // window.wl_surface().commit();
        // }
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
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        trace!("[COMMON] XDG popup configure");

        self.single_color_example_buffer_configure(popup.wl_surface(), qh, config.width as u32, config.height as u32, (255, 0, 255));
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _popup: &Popup) {
        
    }
}

impl WindowHandler for Application {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, target_window: &Window) {
        // No-op for this simple helper container
        trace!("[COMMON] XDG window close requested");
        
        if let Some(idx) = self.windows.iter().position(|w| w.get_window() == target_window) {
            let mut win = self.windows.remove(idx);
            if !win.request_close(self) {
                self.windows.insert(idx, win); // Re-insert if not closed
            }
        }
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
        
        if let Some(idx) = self.windows.iter().position(|w| w.get_window() == target_window) {
            let mut win = self.windows.remove(idx);
            win.configure(self, configure.clone());
            self.windows.insert(idx, win);
        }
    }
}

impl PointerHandler for Application {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        trace!("[MAIN] Pointer frame with {} events", events.len());

        for event in events {
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.last_pointer_enter_serial = Some(serial)
                },
                _ => {}
            }
            // self.input_state.handle_pointer_event(event);
        }

        // Example how to set cursor shape
        if let Some(serial) = self.last_pointer_enter_serial && let Some(last_event) = events.last() {
            trace!("[MAIN] Setting cursor shape to Move for pointer event");
            // If last event was within 20x20 region at top-left, set to Move shape
            let (x, y) = last_event.position;
            if x < 20.0 && y < 20.0 {
                trace!("[MAIN] Pointer within top-left 20x20 region, setting Move shape");
                let device = self.get_or_create_shape_device(pointer, qh);
                device.set_shape(serial, Shape::Move);
            } else {
                trace!("[MAIN] Pointer outside top-left 20x20 region, setting Pointer shape");
                let device = self.get_or_create_shape_device(pointer, qh);
                device.set_shape(serial, Shape::Pointer);
            }
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
        if capability == Capability::Keyboard {
            trace!("[MAIN] Creating wl_keyboard");
            // match self.seat_state.get_keyboard(qh, &seat) {
            //     Ok(wl_keyboard) => {
            //         self.wl_keyboard = Some(wl_keyboard);
            //         trace!("[MAIN] wl_keyboard created successfully");
            //     }
            //     Err(e) => {
            //         trace!("[MAIN] Failed to create wl_keyboard: {:?}", e);
            //     }
            // }
        }
        if capability == Capability::Pointer {
            let _ = self.seat_state.get_pointer(&qh, &seat);
            trace!("[MAIN] Creating themed pointer");
            
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



pub trait WindowContainer {
    fn configure(
        &mut self,
        app: &mut Application,
        configure: WindowConfigure,
    );

    fn request_close(&mut self, app: &mut Application) -> bool;

    fn get_window(&self) -> &Window;
}

pub trait LayerSurfaceContainer {
    fn configure(
        &mut self,
        qh: &QueueHandle<Application>,
        config: LayerSurfaceConfigure,
    );

    fn request_close(&mut self);
}

pub trait PopupContainer {
    fn configure(
        &mut self,
        qh: &QueueHandle<Application>,
        config: PopupConfigure,
    );

    fn done(&mut self);
}

pub struct ExampleSingleColorWindow {
    pub window: Window,
    pub color: (u8, u8, u8),
    pub pool: Option<SlotPool>,
}

impl ExampleSingleColorWindow {
    pub fn single_color_example_buffer_configure(&mut self, shm_state: &Shm, surface: &WlSurface, qh: &QueueHandle<Application>, new_width: u32, new_height: u32, color: (u8, u8, u8)) {

        trace!("[COMMON] Create Brown Buffer");

        // Ensure pool exists
        if self.pool.is_none() {
            let size = (new_width as usize) * (new_height as usize) * 4;
            self.pool = Some(SlotPool::new(size, shm_state).expect("Failed to create SlotPool"));
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

impl WindowContainer for ExampleSingleColorWindow {
    fn configure(
        &mut self,
        app: &mut Application,
        configure: WindowConfigure,
    ) {
        let width = configure.new_size.0.unwrap_or_else(|| NonZero::new(256).unwrap()).get();
        let height = configure.new_size.1.unwrap_or_else(|| NonZero::new(256).unwrap()).get();

        // Handle window configuration changes here
        self.single_color_example_buffer_configure(&app.shm_state, &self.window.wl_surface().clone(), &app.qh, width, height, self.color);
    }

    fn request_close(&mut self, app: &mut Application) -> bool {
        // Handle window close request here
        true
    }

    fn get_window(&self) -> &Window {
        &self.window
    }
}