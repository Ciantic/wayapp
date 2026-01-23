// use crate::LayerSurfaceContainer;
// use crate::PopupContainer;
// use crate::SubsurfaceContainer;
// use crate::WindowContainer;
use log::trace;
use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::delegate_compositor;
use smithay_client_toolkit::delegate_keyboard;
use smithay_client_toolkit::delegate_layer;
use smithay_client_toolkit::delegate_output;
use smithay_client_toolkit::delegate_pointer;
use smithay_client_toolkit::delegate_registry;
use smithay_client_toolkit::delegate_seat;
use smithay_client_toolkit::delegate_shm;
use smithay_client_toolkit::delegate_simple;
use smithay_client_toolkit::delegate_subcompositor;
use smithay_client_toolkit::delegate_xdg_popup;
use smithay_client_toolkit::delegate_xdg_shell;
use smithay_client_toolkit::delegate_xdg_window;
use smithay_client_toolkit::output::OutputHandler;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::ProvidesRegistryState;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::registry::SimpleGlobal;
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::Capability;
use smithay_client_toolkit::seat::SeatHandler;
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::KeyboardHandler;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::seat::pointer::PointerHandler;
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerShell;
use smithay_client_toolkit::shell::wlr_layer::LayerShellHandler;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
use smithay_client_toolkit::shell::xdg::popup::PopupHandler;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
use smithay_client_toolkit::shell::xdg::window::WindowHandler;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::ShmHandler;
use smithay_client_toolkit::subcompositor::SubcompositorState;
use smithay_clipboard::Clipboard;
use std::collections::HashMap;
use std::thread::JoinHandle;
use wayland_backend::client::ObjectId;
use wayland_backend::client::WaylandError;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::EventQueue;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_output;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_seat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewport::{self};
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

/// Enum representing different Wayland events
///
/// This is not same as smithay_client_toolkit events, this is an
/// application-level event enum.
#[derive(Debug, Clone)]
pub enum WaylandEvent {
    /// WlSurface and the timestamp
    Frame(WlSurface, u32),
    ScaleFactorChanged(WlSurface, i32),
    TransformChanged(WlSurface),
    SurfaceEnteredOutput(WlSurface, WlOutput),
    SurfaceLeftOutput(WlSurface, WlOutput),
    LayerShellClosed(LayerSurface),
    LayerShellConfigure(LayerSurface, LayerSurfaceConfigure),
    PopupConfigure(Popup, PopupConfigure),
    PopupDone(Popup),
    WindowRequestClose(Window),
    WindowConfigure(Window, WindowConfigure),
    OutputCreated(WlOutput),
    OutputUpdated(WlOutput),
    OutputDestroyed(WlOutput),
    KeyboardEnter(WlSurface, Vec<u32>, Vec<Keysym>),
    KeyboardLeave(WlSurface),
    KeyPress(KeyEvent),
    KeyRelease(KeyEvent),
    KeyRepeat(KeyEvent),
    PointerEvent((WlSurface, (f64, f64), PointerEventKind)),
    ModifiersChanged(smithay_client_toolkit::seat::keyboard::Modifiers),
}

impl WaylandEvent {
    pub fn get_wl_surface(&self) -> Option<&WlSurface> {
        match self {
            WaylandEvent::Frame(s, _) => Some(s),
            WaylandEvent::ScaleFactorChanged(s, _) => Some(s),
            WaylandEvent::TransformChanged(s) => Some(s),
            WaylandEvent::SurfaceEnteredOutput(s, _) => Some(s),
            WaylandEvent::SurfaceLeftOutput(s, _) => Some(s),
            WaylandEvent::WindowConfigure(w, _) => Some(&w.wl_surface()),
            WaylandEvent::LayerShellConfigure(layer, _) => Some(&layer.wl_surface()),
            WaylandEvent::LayerShellClosed(layer) => Some(&layer.wl_surface()),
            WaylandEvent::PopupConfigure(popup, _) => Some(&popup.wl_surface()),
            WaylandEvent::PopupDone(popup) => Some(&popup.wl_surface()),
            WaylandEvent::KeyboardEnter(s, _, _) => Some(s),
            WaylandEvent::KeyboardLeave(s) => Some(s),
            WaylandEvent::PointerEvent((s, _, _)) => Some(s),
            _ => None,
        }
    }
}

pub struct Application {
    wayland_events: Vec<WaylandEvent>,
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
    pub clipboard: Clipboard,
    pub viewporter: SimpleGlobal<WpViewporter, 1>,
    cursor_shape_manager: CursorShapeManager,
    last_pointer_enter_serial: Option<u32>,
    last_pointer: Option<WlPointer>,
    pointer_shape_devices: HashMap<ObjectId, WpCursorShapeDeviceV1>,
    keyboard_focused_surface: Option<ObjectId>,
}

impl Application {
    /// Create a new Application, initializing all Wayland globals and state.
    pub fn new() -> Self {
        let conn = Connection::connect_to_env().expect("Failed to connect to Wayland");
        let (globals, event_queue) =
            registry_queue_init::<Self>(&conn).expect("Failed to init registry");
        let qh: QueueHandle<Self> = event_queue.handle();

        // Bind required globals
        let compositor_state =
            CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
        let subcompositor_state =
            SubcompositorState::bind(compositor_state.wl_compositor().clone(), &globals, &qh)
                .expect("wl_subcompositor not available");
        let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell not available");
        let shm_state = Shm::bind(&globals, &qh).expect("wl_shm not available");
        let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell not available");
        let cursor_shape_manager =
            CursorShapeManager::bind(&globals, &qh).expect("cursor shape manager not available");
        let viewporter = SimpleGlobal::<WpViewporter, 1>::bind(&globals, &qh)
            .expect("wp_viewporter not available");
        let clipboard = unsafe { Clipboard::new(conn.display().id().as_ptr() as *mut _) };

        Self {
            wayland_events: Vec::new(),
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
            clipboard,
            viewporter,
            cursor_shape_manager,
            last_pointer_enter_serial: None,
            last_pointer: None,
            pointer_shape_devices: HashMap::new(),
            keyboard_focused_surface: None,
        }
    }

    pub fn take_wayland_events(&mut self) -> Vec<WaylandEvent> {
        self.wayland_events.drain(..).collect()
    }

    pub fn set_cursor(&mut self, shape: Shape) {
        if let Some(serial) = self.last_pointer_enter_serial
            && let Some(pointer) = &self.last_pointer
        {
            let pointer_id = pointer.id();
            let device = self
                .pointer_shape_devices
                .entry(pointer_id)
                .or_insert_with(|| {
                    trace!(
                        "[COMMON] Creating new cursor shape device for pointer id {}",
                        pointer.id()
                    );
                    self.cursor_shape_manager
                        .get_shape_device(pointer, &self.qh)
                });
            device.set_shape(serial, shape);
        }
    }

    /// Blocking way to run the Wayland event loop
    ///
    /// For tokio or other async uses see `run_dispatcher` method.
    ///
    /// This may panic if the event queue has already been taken.
    pub fn run_blocking(&mut self) {
        // Run the Wayland event loop. This example will run until the process is killed
        let mut event_queue = self.event_queue.take().expect("Event queue already used");
        loop {
            event_queue
                .blocking_dispatch(self)
                .expect("Wayland dispatch failed");
        }
    }

    /// Asynchronous way to run the Wayland event loop
    ///
    /// Connection reading happens blockingly in separate thread, but
    /// dispatching is done by the user via `AsyncDispatcher::dispatch_pending`
    ///
    /// See egui_tokio_async.rs example for usage.
    ///
    /// This may panic if the event queue has already been taken.
    pub fn run_dispatcher<T: Fn() + Send + 'static>(&mut self, dispatch_fn: T) -> AsyncReader<T> {
        let event_queue = self.event_queue.take().expect("Event queue already used");
        let mut dispatcher = AsyncReader::new(self.conn.clone(), event_queue, dispatch_fn);
        dispatcher.start_thread();
        dispatcher
    }
}

enum AsyncReaderError {
    RecvError(std::sync::mpsc::RecvError),
    WaylandError(WaylandError),
}
impl From<std::sync::mpsc::RecvError> for AsyncReaderError {
    fn from(e: std::sync::mpsc::RecvError) -> Self {
        AsyncReaderError::RecvError(e)
    }
}
impl From<WaylandError> for AsyncReaderError {
    fn from(e: WaylandError) -> Self {
        AsyncReaderError::WaylandError(e)
    }
}

pub struct AsyncReader<T: Fn() + Send + 'static> {
    conn: Option<Connection>,
    event_queue: EventQueue<Application>,
    count_reader: Option<std::sync::mpsc::Receiver<Option<usize>>>,
    count_sender: std::sync::mpsc::Sender<Option<usize>>,
    dispatch_fn: Option<T>,
    #[allow(dead_code)]
    reader_thread: Option<JoinHandle<()>>,
}

impl<T: Fn() + Send + 'static> AsyncReader<T> {
    fn new(conn: Connection, event_queue: EventQueue<Application>, dispatch_fn: T) -> Self {
        let (count_sender, count_reader) = std::sync::mpsc::channel::<Option<usize>>();
        AsyncReader {
            conn: Some(conn),
            event_queue,
            count_sender,
            count_reader: Some(count_reader),
            dispatch_fn: Some(dispatch_fn),
            reader_thread: None,
        }
    }

    /// Start the blocking reading thread
    pub(crate) fn start_thread(&mut self) {
        let count_reader = self.count_reader.take().expect("Count reader missing");
        let dispatch_fn = self.dispatch_fn.take().expect("Dispatch function missing");
        let conn = self.conn.take().expect("Connection missing");

        self.reader_thread = Some(std::thread::spawn(move || {
            // Initial trigger dispatching
            (dispatch_fn)();

            loop {
                match AsyncReader::read_blocking(&conn, &dispatch_fn, &count_reader) {
                    Ok(cont) => {
                        if !cont {
                            break;
                        }
                    }
                    Err(AsyncReaderError::RecvError(_)) => {
                        // Sender dropped, exit thread
                        break;
                    }
                    Err(AsyncReaderError::WaylandError(e)) => {
                        eprintln!("Error in Wayland reader thread: {:?}", e);
                        break;
                    }
                }
            }
        }));
    }

    /// Reads the wayland connection blockingly, and calls the user defined
    /// dispatch function
    #[inline]
    fn read_blocking(
        conn: &Connection,
        dispatch_fn: &T,
        count_reader: &std::sync::mpsc::Receiver<Option<usize>>,
    ) -> Result<bool, AsyncReaderError> {
        // Implementation follows `EventQueue::blocking_dispatch` logic
        match count_reader.recv()? {
            Some(count) => {
                if count > 0 {
                    (dispatch_fn)();
                    return Ok(true);
                }
            }
            None => return Ok(false),
        }

        conn.flush()?;

        // This function execution can take sometimes seconds (if no events are coming)
        if let Some(guard) = conn.prepare_read() {
            guard.read_without_dispatch()?;
        } else {
            // Goal is that this branch is never or very seldomly hit
            #[cfg(feature = "_example")]
            println!("♦️♦️♦️♦️♦️ Failed to read");
        }

        (dispatch_fn)();
        Ok(true) // Continue
    }

    /// Dispatch pending events, and return collected Wayland events
    pub fn dispatch_pending(&mut self, app: &mut Application) -> Vec<WaylandEvent> {
        let count = self
            .event_queue
            .dispatch_pending(app)
            .expect("Wayland dispatch failed");
        let _ = self.count_sender.send(Some(count));
        app.take_wayland_events()
    }
}

// This doesn't work properly because it is
// more likely that the locking thread is stuck at prepare_read or
// read_without_dispatch, then the signaling here won't be received until those
// calls return.
// impl Drop for AsyncDispatcher {
//     fn drop(&mut self) {
//         // Terminate the lock thread
//         self.count_sender.send(None).unwrap();
//         if let Some(thread) = self.lock_thread.take() {
//             let _ = thread.join();
//         }
//     }
// }

impl CompositorHandler for Application {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        new_factor: i32,
    ) {
        self.wayland_events.push(WaylandEvent::ScaleFactorChanged(
            surface.clone(),
            new_factor,
        ));
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        self.wayland_events
            .push(WaylandEvent::TransformChanged(surface.clone()));
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        time: u32,
    ) {
        self.wayland_events
            .push(WaylandEvent::Frame(surface.clone(), time));
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &WlOutput,
    ) {
        self.wayland_events.push(WaylandEvent::SurfaceEnteredOutput(
            surface.clone(),
            output.clone(),
        ));
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &WlOutput,
    ) {
        self.wayland_events.push(WaylandEvent::SurfaceLeftOutput(
            surface.clone(),
            output.clone(),
        ));
    }
}

impl OutputHandler for Application {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, output: WlOutput) {
        self.wayland_events
            .push(WaylandEvent::OutputCreated(output));
    }

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, output: WlOutput) {
        self.wayland_events
            .push(WaylandEvent::OutputUpdated(output));
    }

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, output: WlOutput) {
        self.wayland_events
            .push(WaylandEvent::OutputDestroyed(output));
    }
}

impl LayerShellHandler for Application {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_layer: &LayerSurface) {
        self.wayland_events
            .push(WaylandEvent::LayerShellClosed(target_layer.clone()));
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        target_layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.wayland_events.push(WaylandEvent::LayerShellConfigure(
            target_layer.clone(),
            configure.clone(),
        ));
    }
}

impl PopupHandler for Application {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        target_popup: &Popup,
        config: PopupConfigure,
    ) {
        self.wayland_events.push(WaylandEvent::PopupConfigure(
            target_popup.clone(),
            config.clone(),
        ));

        trace!("[COMMON] XDG popup configure");
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_popup: &Popup) {
        self.wayland_events
            .push(WaylandEvent::PopupDone(target_popup.clone()));

        trace!("[COMMON] XDG popup done");
    }
}

impl WindowHandler for Application {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, target_window: &Window) {
        trace!("[COMMON] XDG window close requested");
        self.wayland_events
            .push(WaylandEvent::WindowRequestClose(target_window.clone()));
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        target_window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        self.wayland_events.push(WaylandEvent::WindowConfigure(
            target_window.clone(),
            configure.clone(),
        ));

        trace!("[COMMON] XDG window configure");
    }
}

impl PointerHandler for Application {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                // Changing cursor shape requires last enter serial number, we are storing it here
                PointerEventKind::Enter { serial } => {
                    self.last_pointer_enter_serial = Some(serial);
                    self.last_pointer = Some(pointer.clone());
                }
                _ => {}
            }

            self.wayland_events.push(WaylandEvent::PointerEvent((
                event.surface.clone(),
                event.position,
                event.kind.clone(),
            )));
        }
    }
}

impl KeyboardHandler for Application {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        surface: &WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        trace!("[MAIN] Keyboard focus gained on surface {:?}", surface.id());
        self.wayland_events.push(WaylandEvent::KeyboardEnter(
            surface.clone(),
            _raw.to_vec(),
            _keysyms.to_vec(),
        ));

        self.keyboard_focused_surface = Some(surface.id());
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        surface: &WlSurface,
        _serial: u32,
    ) {
        trace!("[MAIN] Keyboard focus lost");
        self.wayland_events
            .push(WaylandEvent::KeyboardLeave(surface.clone()));
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        trace!("[MAIN] Key pressed: keycode={}", event.raw_code);
        self.wayland_events
            .push(WaylandEvent::KeyPress(event.clone()));
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        trace!("[MAIN] Key released: keycode={}", event.raw_code);
        self.wayland_events
            .push(WaylandEvent::KeyRelease(event.clone()));
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
        self.wayland_events
            .push(WaylandEvent::ModifiersChanged(modifiers.clone()));
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        trace!("[MAIN] Key repeated: keycode={}", event.raw_code);
        self.wayland_events
            .push(WaylandEvent::KeyRepeat(event.clone()));
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
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(_wl_keyboard) => {
                    trace!("[MAIN] wl_keyboard created successfully");
                }
                Err(e) => {
                    trace!("[MAIN] Failed to create wl_keyboard: {:?}", e);
                }
            }
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
    registry_handlers![OutputState];

    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
}

impl AsMut<SimpleGlobal<WpViewporter, 1>> for Application {
    fn as_mut(&mut self) -> &mut SimpleGlobal<WpViewporter, 1> {
        &mut self.viewporter
    }
}

impl Dispatch<WpViewport, ()> for Application {
    fn event(
        _: &mut Application,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // No events expected from wp_viewport
    }
}

impl Dispatch<WlRegion, ()> for Application {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegion,
        _event: <WlRegion as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
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
delegate_simple!(Application, WpViewporter, 1);

// ----------------------------------------------------------------
// Request frame helper
// ----------------------------------------------------------------
// pub trait RequestFrame {
//     fn request_frame(&self, qh: &QueueHandle<Application>);
// }

// impl RequestFrame for LayerSurface {
//     fn request_frame(&self, qh: &QueueHandle<Application>) {
//         let wl_surface = self.wl_surface();
//         wl_surface.frame(qh, wl_surface.clone());
//         wl_surface.commit();
//     }
// }

// impl RequestFrame for Window {
//     fn request_frame(&self, qh: &QueueHandle<Application>) {
//         let wl_surface = self.wl_surface();
//         wl_surface.frame(qh, wl_surface.clone());
//         wl_surface.commit();
//     }
// }

// impl RequestFrame for Popup {
//     fn request_frame(&self, qh: &QueueHandle<Application>) {
//         let wl_surface = self.wl_surface();
//         wl_surface.frame(qh, wl_surface.clone());
//         wl_surface.commit();
//     }
// }

// impl RequestFrame for WlSurface {
//     fn request_frame(&self, qh: &QueueHandle<Application>) {
//         self.frame(qh, self.clone());
//         self.commit();
//     }
// }
