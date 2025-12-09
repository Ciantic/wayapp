#![allow(unused_variables)]

use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Modifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
use wayland_client::protocol::wl_output::Transform;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_surface::WlSurface;

pub trait KeyboardHandlerContainer {
    fn enter(&mut self) {}

    fn leave(&mut self) {}

    fn press_key(&mut self, event: &KeyEvent) {}

    fn release_key(&mut self, event: &KeyEvent) {}

    fn update_modifiers(&mut self, modifiers: &Modifiers) {}

    fn repeat_key(&mut self, event: &KeyEvent) {}
}

pub trait PointerHandlerContainer {
    fn pointer_frame(&mut self, events: &PointerEvent) {}
}

pub trait CompositorHandlerContainer {
    fn scale_factor_changed(&mut self, new_factor: i32) {}

    fn transform_changed(&mut self, new_transform: &Transform) {}

    fn frame(&mut self, time: u32) {}

    fn surface_enter(&mut self, output: &WlOutput) {}

    fn surface_leave(&mut self, output: &WlOutput) {}
}

pub trait BaseTrait:
    CompositorHandlerContainer + KeyboardHandlerContainer + PointerHandlerContainer
{
}

pub trait WindowContainer: BaseTrait {
    fn configure(&mut self, configure: &WindowConfigure);

    fn get_window(&self) -> &Window;

    fn allowed_to_close(&self) -> bool {
        true
    }

    fn request_close(&mut self) {}
}

pub trait LayerSurfaceContainer: BaseTrait {
    fn configure(&mut self, config: &LayerSurfaceConfigure);

    fn closed(&mut self) {}

    fn get_layer_surface(&self) -> &LayerSurface;
}

pub trait PopupContainer: BaseTrait {
    fn configure(&mut self, config: &PopupConfigure);

    fn done(&mut self) {}

    fn get_popup(&self) -> &Popup;
}

pub trait SubsurfaceContainer: BaseTrait {
    fn configure(&mut self, width: u32, height: u32);

    fn get_wl_surface(&self) -> &WlSurface;
}
