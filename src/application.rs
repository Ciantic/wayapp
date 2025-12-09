use std::{cell::RefCell, collections::HashMap, mem::MaybeUninit, rc::Rc};

use log::trace;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_subcompositor, delegate_xdg_popup,
    delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym},
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        xdg::{
            XdgShell,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
    subcompositor::SubcompositorState,
};
use smithay_clipboard::Clipboard;
use wayland_backend::client::ObjectId;
use wayland_client::{
    Connection, EventQueue, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{
        wl_keyboard::WlKeyboard, wl_output, wl_pointer::WlPointer, wl_seat, wl_surface::WlSurface,
    },
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};

use crate::{LayerSurfaceContainer, PopupContainer, SubsurfaceContainer, WindowContainer};

/// Enum representing the kind of surface container stored in the application
pub enum Kind {
    Window(Rc<RefCell<dyn WindowContainer>>),
    LayerSurface(Rc<RefCell<dyn LayerSurfaceContainer>>),
    Popup(Rc<RefCell<dyn PopupContainer>>),
    Subsurface(Rc<RefCell<dyn SubsurfaceContainer>>),
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
    windows: Vec<Rc<RefCell<dyn WindowContainer>>>,
    layer_surfaces: Vec<Rc<RefCell<dyn LayerSurfaceContainer>>>,
    popups: Vec<Rc<RefCell<dyn PopupContainer>>>,
    subsurfaces: Vec<Rc<RefCell<dyn SubsurfaceContainer>>>,
    /// HashMap storing surface kind by ObjectId for quick lookup
    surfaces_by_id: HashMap<ObjectId, Kind>,
    pub clipboard: Clipboard,

    cursor_shape_manager: CursorShapeManager,

    /// For cursor set_shape to work serial parameter must match the latest wl_pointer.enter or zwp_tablet_tool_v2.proximity_in serial number sent to the client.
    last_pointer_enter_serial: Option<u32>,
    last_pointer: Option<WlPointer>,
    // Cache cursor shape devices per pointer to avoid repeated protocol calls
    pointer_shape_devices: HashMap<ObjectId, WpCursorShapeDeviceV1>,
    /// Currently focused keyboard surface
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
        }
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

    /// Push a window container to the application
    pub fn push_window<W: WindowContainer + 'static>(&mut self, window: W) {
        let window = Rc::new(RefCell::new(window)) as Rc<RefCell<dyn WindowContainer>>;
        let surface_id = window.borrow().get_window().wl_surface().id();
        self.windows.push(window.clone());
        self.surfaces_by_id.insert(surface_id, Kind::Window(window));
    }

    /// Push a layer surface container to the application
    pub fn push_layer_surface<L: LayerSurfaceContainer + 'static>(&mut self, layer_surface: L) {
        let layer_surface =
            Rc::new(RefCell::new(layer_surface)) as Rc<RefCell<dyn LayerSurfaceContainer>>;
        let surface_id = layer_surface.borrow().get_layer_surface().wl_surface().id();
        self.layer_surfaces.push(layer_surface.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::LayerSurface(layer_surface));
    }

    /// Push a popup container to the application
    pub fn push_popup<P: PopupContainer + 'static>(&mut self, popup: P) {
        let popup = Rc::new(RefCell::new(popup)) as Rc<RefCell<dyn PopupContainer>>;
        let surface_id = popup.borrow().get_popup().wl_surface().id();
        self.popups.push(popup.clone());
        self.surfaces_by_id.insert(surface_id, Kind::Popup(popup));
    }

    /// Push a subsurface container to the application
    pub fn push_subsurface<S: SubsurfaceContainer + 'static>(&mut self, subsurface: S) {
        let subsurface = Rc::new(RefCell::new(subsurface)) as Rc<RefCell<dyn SubsurfaceContainer>>;
        let surface_id = subsurface.borrow().get_wl_surface().id();
        self.subsurfaces.push(subsurface.clone());
        self.surfaces_by_id
            .insert(surface_id, Kind::Subsurface(subsurface));
    }

    /// Remove a window by its Window reference
    fn remove_window(&mut self, window: &Window) {
        let surface_id = window.wl_surface().id();
        self.windows
            .retain(|w| w.borrow().get_window().wl_surface().id() != surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a layer surface by its LayerSurface reference
    fn remove_layer_surface(&mut self, layer_surface: &LayerSurface) {
        let surface_id = layer_surface.wl_surface().id();
        self.layer_surfaces
            .retain(|l| l.borrow().get_layer_surface().wl_surface().id() != surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a popup by its Popup reference
    fn remove_popup(&mut self, popup: &Popup) {
        let surface_id = popup.wl_surface().id();
        self.popups
            .retain(|p| p.borrow().get_popup().wl_surface().id() != surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    /// Remove a subsurface by its WlSurface reference
    fn remove_subsurface(&mut self, subsurface: &WlSurface) {
        let surface_id = subsurface.id();
        self.subsurfaces
            .retain(|s| s.borrow().get_wl_surface().id() != surface_id);
        self.surfaces_by_id.remove(&surface_id);
    }

    fn get_by_surface_id(&self, surface_id: &ObjectId) -> Option<&Kind> {
        self.surfaces_by_id.get(surface_id)
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
        self.get_by_surface_id(&surface.id()).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().scale_factor_changed(new_factor);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().scale_factor_changed(new_factor);
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().scale_factor_changed(new_factor);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().scale_factor_changed(new_factor);
                }
            }
            Some(())
        });

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
        self.get_by_surface_id(&surface.id()).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().transform_changed(&new_transform);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().transform_changed(&new_transform);
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().transform_changed(&new_transform);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().transform_changed(&new_transform);
                }
            }
            Some(())
        });
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        time: u32,
    ) {
        if let Some(kind) = self.get_by_surface_id(&surface.id()) {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().frame(time);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().frame(time);
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().frame(time);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().frame(time);
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
        self.get_by_surface_id(&surface.id()).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().surface_enter(output);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().surface_enter(output);
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().surface_enter(output);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().surface_enter(output);
                }
            }
            Some(())
        });
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &wl_output::WlOutput,
    ) {
        self.get_by_surface_id(&surface.id()).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().surface_leave(output);
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().surface_leave(output);
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().surface_leave(output);
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().surface_leave(output);
                }
            }
            Some(())
        });
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
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_layer: &LayerSurface) {
        let index = self
            .layer_surfaces
            .iter()
            .position(|w| w.borrow().get_layer_surface() == target_layer)
            .expect("Layer surface is not added to application");

        if let Some(layer_surface) = self.layer_surfaces.get(index) {
            layer_surface.borrow_mut().closed();

            // TODO: Should it be removed?
            self.layer_surfaces.remove(index);
        }
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

        let index = self
            .layer_surfaces
            .iter()
            .position(|w| w.borrow().get_layer_surface() == target_layer)
            .expect("Layer surface is not added to application");

        if let Some(layer_surface) = self.layer_surfaces.get(index) {
            layer_surface.borrow_mut().configure(&configure);
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

        let index = self
            .popups
            .iter()
            .position(|p| p.borrow().get_popup() == target_popup)
            .expect("Popup is not added to application");

        if let Some(popup) = self.popups.get(index) {
            popup.borrow_mut().configure(&config);
        }
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, target_popup: &Popup) {
        trace!("[COMMON] XDG popup done");

        let index = self
            .popups
            .iter()
            .position(|p| p.borrow().get_popup() == target_popup)
            .expect("Popup is not added to application");

        if let Some(popup) = self.popups.get(index) {
            popup.borrow_mut().done();
        }
    }
}

impl WindowHandler for Application {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, target_window: &Window) {
        trace!("[COMMON] XDG window close requested");
        let index = self
            .windows
            .iter()
            .position(|w| w.borrow().get_window() == target_window)
            .expect("Window is not added to application");
        if let Some(window) = self.windows.get(index) {
            window.borrow_mut().request_close();
            if window.borrow_mut().allowed_to_close() {
                self.remove_window(target_window);
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

        let index = self
            .windows
            .iter()
            .position(|w| w.borrow().get_window() == target_window)
            .expect("Window is not added to application");

        if let Some(window) = self.windows.get(index) {
            window.borrow_mut().configure(&configure);
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
                // Changing cursor shape requires last enter serial number, we are storing it here
                PointerEventKind::Enter { serial } => {
                    self.last_pointer_enter_serial = Some(serial);
                    self.last_pointer = Some(pointer.clone());
                }
                _ => {}
            }

            if let Some(kind) = self.get_by_surface_id(&event.surface.id()) {
                match kind {
                    Kind::Window(window) => {
                        window.borrow_mut().pointer_frame(event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.borrow_mut().pointer_frame(event);
                    }
                    Kind::Popup(popup) => {
                        popup.borrow_mut().pointer_frame(event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.borrow_mut().pointer_frame(event);
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
        self.get_by_surface_id(&surface_id).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().enter();
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().enter();
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().enter();
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().enter();
                }
            }
            Some(())
        });
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
        self.get_by_surface_id(&surface_id).and_then(|kind| {
            match kind {
                Kind::Window(window) => {
                    window.borrow_mut().leave();
                }
                Kind::LayerSurface(layer_surface) => {
                    layer_surface.borrow_mut().leave();
                }
                Kind::Popup(popup) => {
                    popup.borrow_mut().leave();
                }
                Kind::Subsurface(subsurface) => {
                    subsurface.borrow_mut().leave();
                }
            }
            Some(())
        });
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
            if let Some(kind) = self.get_by_surface_id(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.borrow_mut().press_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.borrow_mut().press_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.borrow_mut().press_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.borrow_mut().press_key(&event);
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
        if let Some(surface_id) = &self.keyboard_focused_surface {
            if let Some(kind) = self.get_by_surface_id(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.borrow_mut().release_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.borrow_mut().release_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.borrow_mut().release_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.borrow_mut().release_key(&event);
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
        if let Some(surface_id) = &self.keyboard_focused_surface {
            if let Some(kind) = self.get_by_surface_id(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.borrow_mut().update_modifiers(&modifiers);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.borrow_mut().update_modifiers(&modifiers);
                    }
                    Kind::Popup(popup) => {
                        popup.borrow_mut().update_modifiers(&modifiers);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.borrow_mut().update_modifiers(&modifiers);
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
        if let Some(surface_id) = &self.keyboard_focused_surface {
            if let Some(kind) = self.get_by_surface_id(&surface_id) {
                match kind {
                    Kind::Window(window) => {
                        window.borrow_mut().repeat_key(&event);
                    }
                    Kind::LayerSurface(layer_surface) => {
                        layer_surface.borrow_mut().repeat_key(&event);
                    }
                    Kind::Popup(popup) => {
                        popup.borrow_mut().repeat_key(&event);
                    }
                    Kind::Subsurface(subsurface) => {
                        subsurface.borrow_mut().repeat_key(&event);
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
