use std::rc::Weak;

use log::trace;
use smithay_client_toolkit::{delegate_keyboard, delegate_pointer, seat::{Capability, SeatHandler, SeatState, keyboard::KeyboardHandler, pointer::{PointerHandler, ThemeSpec, ThemedPointer}}, shell::{WaylandSurface, wlr_layer::{LayerShellHandler, LayerSurface}, xdg::{popup::{Popup, PopupConfigure, PopupHandler}, window::{Window, WindowHandler}}}, shm::{Shm, slot::Buffer}};
use wayland_client::{Connection, QueueHandle, protocol::{wl_keyboard::WlKeyboard, wl_surface::WlSurface}};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

pub struct CustomPlatformHandler<K: KeyboardHandler, P: PointerHandler> {
    pub keyboard_handler: Option<K>,
    pub pointer_handler: Option<P>,
    shm_state: Shm,
    themed_pointer: Option<ThemedPointer>,
    wl_keyboard: Option<WlKeyboard>,
    wl_surface: Weak<WlSurface>,
}



pub struct CustomWindowContainer<K: KeyboardHandler, P: PointerHandler> {
    pub xdg_window: Window,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
    pub platform_handler: CustomPlatformHandler<K, P>,
}

impl<K: KeyboardHandler, P: PointerHandler> WindowHandler for CustomWindowContainer<K, P> {
    fn request_close(&mut self, conn: &Connection, qh: &QueueHandle<Self>, window: &Window) {
    }

    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        window: &Window,
        configure: smithay_client_toolkit::shell::xdg::window::WindowConfigure,
        serial: u32,
    ) {
    }
}

pub struct CustomPopupContainer {
    pub popup: Popup,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
}

impl PopupHandler for CustomPopupContainer {
    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        
    }

    
    fn done(&mut self, conn: &wayland_client::Connection, qh: &wayland_client::QueueHandle<Self>, popup: &Popup) {
        todo!()
    }
}

pub struct CustomLayerSurfaceContainer {
    pub layer_surface: LayerSurface,
    // pub surface: WlSurface,
    pub buffer: Option<Buffer>,
    pub wp_viewporter: Option<WpViewport>,
}

impl LayerShellHandler for CustomLayerSurfaceContainer {
    fn closed(&mut self, conn: &wayland_client::Connection, qh: &wayland_client::QueueHandle<Self>, layer: &LayerSurface) {
        
    }

    fn configure(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        layer: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        serial: u32,
    ) {
        
    }
}