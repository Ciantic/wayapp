use std::rc::Weak;

use log::trace;
use smithay_client_toolkit::{delegate_keyboard, delegate_pointer, seat::{Capability, SeatHandler, SeatState, keyboard::{KeyEvent, KeyboardHandler, Modifiers}, pointer::{PointerEvent, PointerHandler, ThemeSpec, ThemedPointer}}, shell::{WaylandSurface, wlr_layer::{LayerShellHandler, LayerSurface}, xdg::{popup::{ConfigureKind, Popup, PopupConfigure, PopupHandler}, window::{Window, WindowConfigure, WindowHandler}}}, shm::{Shm, slot::Buffer}};
use wayland_client::{Connection, QueueHandle, protocol::{wl_keyboard::WlKeyboard, wl_surface::WlSurface}};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;


trait CustomKeyboardHandler {
    fn enter(&mut self) {

    }

    fn leave(&mut self) {

    }

    fn press_key(&mut self, event: KeyEvent) {

    }

    fn release_key(&mut self, event: KeyEvent) {

    }

    fn update_modifiers(&mut self, modifiers: Modifiers) {

    }

    fn repeat_key(&mut self, event: KeyEvent) {
    }
}

trait CustomPointerHandler {
    fn pointer_frame(&mut self, events: &[PointerEvent]) {

    }
}

pub struct CustomWindowContainer {
    pub xdg_window: Window,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
}

pub struct CustomLayerSurfaceContainer {
    pub layer_surface: LayerSurface,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
}

pub struct CustomPopupContainer {
    pub popup: Popup,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
}

impl CustomWindowContainer {
    fn configure(
        &mut self,
        configure: WindowConfigure,
    ) {
    }

    fn request_close(&mut self) {
    }
}


impl CustomPopupContainer {
    fn configure(
        &mut self,
        config: PopupConfigure,
    ) {
        
    }
    
    fn done(&mut self) {
    }
}


impl CustomLayerSurfaceContainer {
    fn configure(
        &mut self,
        width: i32,
        height: i32,
    ) {
        
    }
    fn closed(&mut self) {
        
    }
}