
use smithay_client_toolkit::{seat::{keyboard::{KeyEvent, Modifiers}, pointer::PointerEvent}, shell::{wlr_layer::LayerSurface, xdg::{popup::{Popup, PopupConfigure}, window::{Window, WindowConfigure}}}, shm::slot::Buffer};
use wayland_client::QueueHandle;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

use crate::Application;



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