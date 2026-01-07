use crate::LayerSurfaceContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::WindowContainer;
use log::info;
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
use smithay_client_toolkit::delegate_subcompositor;
use smithay_client_toolkit::delegate_xdg_popup;
use smithay_client_toolkit::delegate_xdg_shell;
use smithay_client_toolkit::delegate_xdg_window;
use smithay_client_toolkit::output::OutputHandler;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::ProvidesRegistryState;
use smithay_client_toolkit::registry::RegistryState;
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
use std::mem::MaybeUninit;
use wayland_backend::client::ObjectId;
use wayland_client::Connection;
use wayland_client::EventQueue;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_output;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_seat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;

/// Callback for output events (add/remove)
pub type OutputCallback = Box<dyn FnMut(OutputEvent)>;

/// Output event types for hot-plug support
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// A new output was added
    Added(wl_output::WlOutput),
    /// An output was updated (e.g., resolution change)
    Updated(wl_output::WlOutput),
    /// An output was removed
    Removed(wl_output::WlOutput),
}

/// Enum representing the kind of surface container stored in the application
enum Kind {
    Window(Box<dyn WindowContainer>),
    LayerSurface(Box<dyn LayerSurfaceContainer>),
    Popup(Box<dyn PopupContainer>),
    Subsurface(Box<dyn SubsurfaceContainer>),
}

pub static mut WAYAPP: MaybeUninit<Application> = MaybeUninit::uninit();

pub fn get_init_app() -> &'static mut Application {
    // Look behind you! A three-headed monkey!
    #[allow(static_mut_refs)]
    unsafe {
        WAYAPP.write(Application::new())
    };
    #[allow(static_mut_refs)]
    unsafe {
        WAYAPP.assume_init_mut()
    }
}

pub fn get_app<'a>() -> &'a mut Application {
    // Look behind you! A three-headed monkey!
    #[allow(static_mut_refs)]
    unsafe {
        WAYAPP.assume_init_mut()
    }
}

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
    windows: Vec<ObjectId>,
    layer_surfaces: Vec<ObjectId>,
    popups: Vec<ObjectId>,
    subsurfaces: Vec<ObjectId>,
    /// HashMap storing surface kind by ObjectId for quick lookup
    surfaces_by_id: HashMap<ObjectId, Kind>,
    pub clipboard: Clipboard,

    cursor_shape_manager: CursorShapeManager,

    /// For cursor set_shape to work serial parameter must match the latest
    /// wl_pointer.enter or zwp_tablet_tool_v2.proximity_in serial number sent
    /// to the client.
    last_pointer_enter_serial: Option<u32>,
    last_pointer: Option<WlPointer>,
    // Cache cursor shape devices per pointer to avoid repeated protocol calls
    pointer_shape_devices: HashMap<ObjectId, WpCursorShapeDeviceV1>,
    /// Currently focused keyboard surface
    keyboard_focused_surface: Option<ObjectId>,
    /// Callback for output events (hot-plug support)
    output_callback: Vec<OutputCallback>,
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
        let clipboard = unsafe { Clipboard::new(conn.display().id().as_ptr() as *mut _) };

        // Note: initally, output state is empty!
        let output_state = OutputState::new(&globals, &qh);

        Self {
            event_queue: Some(event_queue),
            conn,
            qh: qh.clone(),
            subcompositor_state,
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state,
            shm_state,
            compositor_state,
            xdg_shell,
            layer_shell,
            windows: Vec::new(),
            layer_surfaces: Vec::new(),
            popups: Vec::new(),
            subsurfaces: Vec::new(),
            surfaces_by_id: HashMap::new(),
            // windows: Vec::new(),
            // layer_surfaces: Vec::new(),
            clipboard,
            cursor_shape_manager,
            last_pointer_enter_serial: None,
            last_pointer: None,
            pointer_shape_devices: HashMap::new(),
            keyboard_focused_surface: None,
            output_callback: vec![],
        }
    }

    /// Set a callback to be notified of output events (add/update/remove)
    /// This is useful for multi-monitor support with hot-plug.
    pub fn set_output_callback<F>(&mut self, callback: F)
    where
        F: FnMut(OutputEvent) + 'static,
    {
        self.output_callback.push(Box::new(callback));
    }

    /// Get an iterator over all currently connected outputs
    pub fn outputs(&self) -> impl Iterator<Item = wl_output::WlOutput> + '_ {
        self.output_state.outputs()
    }

    /// Get information about a specific output
    pub fn output_info(
        &self,
        output: &wl_output::WlOutput,
    ) -> Option<smithay_client_toolkit::output::OutputInfo> {
        self.output_state.info(output)
    }

    pub fn run_blocking(&mut self) {
        // Run the Wayland event loop. This example will run until the process is killed
        let mut event_queue = self.event_queue.take().unwrap();
        loop {
            event_queue
                .blocking_dispatch(self)
                .expect("Wayland dispatch failed");
        }
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

    /// Push a window container to the application
    pub fn push_window<W: WindowContainer + 'static>(&mut self, window: W) {
        let boxed_window: Box<dyn WindowContainer> = Box::new(window);
        let surface_id = boxed_window.get_object_id();
        self.windows.push(surface_id.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::Window(boxed_window));
    }

    /// Push a layer surface container to the application
    pub fn push_layer_surface(&mut self, layer_surface: impl LayerSurfaceContainer + 'static) {
        let boxed_layer_surface: Box<dyn LayerSurfaceContainer> = Box::new(layer_surface);
        let surface_id = boxed_layer_surface.get_object_id();
        self.layer_surfaces.push(surface_id.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::LayerSurface(boxed_layer_surface));
    }

    /// Push a popup container to the application
    pub fn push_popup<P: PopupContainer + 'static>(&mut self, popup: P) {
        let boxed_popup: Box<dyn PopupContainer> = Box::new(popup);
        let surface_id = boxed_popup.get_object_id();
        self.popups.push(surface_id.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::Popup(boxed_popup));
    }

    /// Push a subsurface container to the application
    pub fn push_subsurface<S: SubsurfaceContainer + 'static>(&mut self, subsurface: S) {
        let boxed_subsurface: Box<dyn SubsurfaceContainer> = Box::new(subsurface);
        let surface_id = boxed_subsurface.get_object_id();
        self.subsurfaces.push(surface_id.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::Subsurface(boxed_subsurface));
    }

    /// Remove a window by its Window reference
    fn remove_window(&mut self, window: &Window) {
        let surface_id = window.wl_surface().id();
        self.windows.retain(|id| id != &surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a layer surface by its LayerSurface reference
    #[allow(dead_code)]
    fn remove_layer_surface(&mut self, layer_surface: &LayerSurface) {
        let surface_id = layer_surface.wl_surface().id();
        self.layer_surfaces.retain(|id| id != &surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a popup by its Popup reference
    #[allow(dead_code)]
    fn remove_popup(&mut self, popup: &Popup) {
        let surface_id = popup.wl_surface().id();
        self.popups.retain(|id| id != &surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a subsurface by its WlSurface reference
    #[allow(dead_code)]
    fn remove_subsurface(&mut self, subsurface: &WlSurface) {
        let surface_id = subsurface.id();
        self.subsurfaces.retain(|id| id != &surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    fn get_by_surface_id_mut(&mut self, surface_id: &ObjectId) -> Option<&mut Kind> {
        self.surfaces_by_id.get_mut(surface_id)
    }
}

impl CompositorHandler for Application {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        new_factor: i32,
    ) {
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.scale_factor_changed(new_factor);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.scale_factor_changed(new_factor);
                }
                Kind::Popup(popup) => {
                    popup.scale_factor_changed(new_factor);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.scale_factor_changed(new_factor);
                }
            }
        }

        // _surface.frame(qh, _surface.clone());
        // _surface.commit();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        new_transform: wl_output::Transform,
    ) {
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.transform_changed(&new_transform);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.transform_changed(&new_transform);
                }
                Kind::Popup(popup) => {
                    popup.transform_changed(&new_transform);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.transform_changed(&new_transform);
                }
            }
        }
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        time: u32,
    ) {
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.frame(time);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.frame(time);
                }
                Kind::Popup(popup) => {
                    popup.frame(time);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.frame(time);
                }
            }
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &wl_output::WlOutput,
    ) {
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.surface_enter(output);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.surface_enter(output);
                }
                Kind::Popup(popup) => {
                    popup.surface_enter(output);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.surface_enter(output);
                }
            }
        }
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &wl_output::WlOutput,
    ) {
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.surface_leave(output);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.surface_leave(output);
                }
                Kind::Popup(popup) => {
                    popup.surface_leave(output);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.surface_leave(output);
                }
            }
        }
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
        output: wl_output::WlOutput,
    ) {
        info!("[OUTPUT] New output detected: {:?}", output.id());
        for callback in &mut self.output_callback {
            callback(OutputEvent::Added(output.clone()));
        }
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        info!("[OUTPUT] Output updated: {:?}", output.id());
        for callback in &mut self.output_callback {
            callback(OutputEvent::Updated(output.clone()));
        }
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        info!("[OUTPUT] Output destroyed: {:?}", output.id());
        for callback in &mut self.output_callback {
            callback(OutputEvent::Removed(output.clone()));
        }
    }
}

impl LayerShellHandler for Application {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_layer: &LayerSurface) {
        let surface_id = target_layer.wl_surface().id();
        let index = self
            .layer_surfaces
            .iter()
            .position(|id| id == &surface_id)
            .expect("Layer surface is not added to application");

        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::LayerSurface(layer_surface) = kind {
                layer_surface.closed();
            }
        }

        // TODO: Should it be removed?
        self.layer_surfaces.remove(index);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        target_layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        trace!("[COMMON] XDG layer configure");

        let surface_id = target_layer.wl_surface().id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::LayerSurface(layer_surface) = kind {
                layer_surface.configure(&configure);
            }
        }
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
        trace!("[COMMON] XDG popup configure");

        let surface_id = target_popup.wl_surface().id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::Popup(popup) = kind {
                popup.configure(&config);
            }
        }
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_popup: &Popup) {
        trace!("[COMMON] XDG popup done");

        let surface_id = target_popup.wl_surface().id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::Popup(popup) = kind {
                popup.done();
            }
        }
    }
}

impl WindowHandler for Application {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, target_window: &Window) {
        trace!("[COMMON] XDG window close requested");
        let surface_id = target_window.wl_surface().id();

        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::Window(window) = kind {
                window.request_close();
                if window.allowed_to_close() {
                    self.remove_window(target_window);
                }
            }
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        target_window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        trace!("[COMMON] XDG window configure");

        let surface_id = target_window.wl_surface().id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            if let Kind::Window(window) = kind {
                window.configure(&configure);
            }
        }
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
        trace!("[MAIN] Pointer frame with {} events", events.len());

        for event in events {
            match event.kind {
                // Changing cursor shape requires last enter serial number, we are storing it here
                PointerEventKind::Enter { serial } => {
                    self.last_pointer_enter_serial = Some(serial);
                    self.last_pointer = Some(pointer.clone());
                }
                _ => {}
            }

            let surface_id = event.surface.id();
            if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.pointer_frame(event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.pointer_frame(event);
                    }
                    Kind::Popup(popup) => {
                        popup.pointer_frame(event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.pointer_frame(event);
                    }
                }
            }
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
        let surface_id = surface.id();
        self.keyboard_focused_surface = Some(surface_id.clone());
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.enter();
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.enter();
                }
                Kind::Popup(popup) => {
                    popup.enter();
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.enter();
                }
            }
        }
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
        let surface_id = surface.id();
        if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
            match kind {
                Kind::Window(window) => {
                    window.leave();
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.leave();
                }
                Kind::Popup(popup) => {
                    popup.leave();
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.leave();
                }
            }
        }
        self.keyboard_focused_surface = None;
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

        if let Some(surface_id) = self.keyboard_focused_surface.clone() {
            if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.press_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.press_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.press_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.press_key(&event);
                    }
                }
            }
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(surface_id) = self.keyboard_focused_surface.clone() {
            if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.release_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.release_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.release_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.release_key(&event);
                    }
                }
            }
        }
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
        if let Some(surface_id) = self.keyboard_focused_surface.clone() {
            if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.update_modifiers(&modifiers);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.update_modifiers(&modifiers);
                    }
                    Kind::Popup(popup) => {
                        popup.update_modifiers(&modifiers);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.update_modifiers(&modifiers);
                    }
                }
            }
        }
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(surface_id) = self.keyboard_focused_surface.clone() {
            if let Some(kind) = self.get_by_surface_id_mut(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.repeat_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.repeat_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.repeat_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.repeat_key(&event);
                    }
                }
            }
        }
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

// Required Dispatch impl for WlRegion to support click-through/input region
// functionality.
//
// In wayland-client, every protocol object needs a Dispatch implementation,
// even if it has no events. WlRegion is used in egui_containers.rs (around line
// 299) where `create_region()` is called to define which parts of a transparent
// surface should receive input events (for click-through overlay support).
//
// WlRegion is a stateless object: you create it, add rectangles, assign it to a
// surface's input region, then destroy it. It never emits any events.
impl wayland_client::Dispatch<WlRegion, ()> for Application {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegion,
        _event: <WlRegion as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // WlRegion has no events - this impl exists only to satisfy
        // wayland-client's requirement that all protocol objects have a
        // Dispatch implementation.
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
