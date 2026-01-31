///! View manager for different kinds of surfaces
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::window::Window;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;

#[derive(Debug, Clone)]
pub enum Kind {
    Window(Window),
    LayerSurface(LayerSurface),
    Popup(Popup),
    Subsurface {
        parent: WlSurface,
        subsurface: WlSubsurface,
        surface: WlSurface,
    },
}
impl Kind {
    fn get_object_id(&self) -> ObjectId {
        match self {
            Kind::Window(window) => window.wl_surface().id(),
            Kind::LayerSurface(layer_surface) => layer_surface.wl_surface().id(),
            Kind::Popup(popup) => popup.wl_surface().id(),
            Kind::Subsurface { surface, .. } => surface.id(),
        }
    }

    pub fn get_wl_surface(&self) -> &WlSurface {
        match self {
            Kind::Window(window) => window.wl_surface(),
            Kind::LayerSurface(layer_surface) => layer_surface.wl_surface(),
            Kind::Popup(popup) => popup.wl_surface(),
            Kind::Subsurface { surface, .. } => surface,
        }
    }

    // pub fn is_window(&self, other: &Window) -> bool {
    //     match self {
    //         Kind::Window(_) => self.get_object_id() == other.wl_surface().id(),
    //         _ => false,
    //     }
    // }

    // pub fn is_layer_surface(&self, other: &LayerSurface) -> bool {
    //     match self {
    //         Kind::LayerSurface(_) => self.get_object_id() ==
    // other.wl_surface().id(),         _ => false,
    //     }
    // }

    // pub fn is_popup(&self, other: &Popup) -> bool {
    //     match self {
    //         Kind::Popup(_) => self.get_object_id() == other.wl_surface().id(),
    //         _ => false,
    //     }
    // }

    // pub fn is_subsurface(&self, other: &WlSurface) -> bool {
    //     match self {
    //         Kind::Subsurface { .. } => self.get_object_id() == other.id(),
    //         _ => false,
    //     }
    // }

    // pub fn request_frame(&self, app: &Application) {
    //     let wl_surface = self.get_wl_surface();
    //     wl_surface.frame(&app.qh, wl_surface.clone());
    //     wl_surface.commit();
    // }
}
impl PartialEq for Kind {
    fn eq(&self, other: &Self) -> bool {
        self.get_object_id() == other.get_object_id()
    }
}
impl Eq for Kind {}

// Cast implementations
impl From<Window> for Kind {
    fn from(window: Window) -> Self {
        Kind::Window(window)
    }
}

impl From<&Window> for Kind {
    fn from(window: &Window) -> Self {
        Kind::Window(window.clone())
    }
}

impl From<LayerSurface> for Kind {
    fn from(layer_surface: LayerSurface) -> Self {
        Kind::LayerSurface(layer_surface)
    }
}

impl From<&LayerSurface> for Kind {
    fn from(layer_surface: &LayerSurface) -> Self {
        Kind::LayerSurface(layer_surface.clone())
    }
}

impl From<Popup> for Kind {
    fn from(popup: Popup) -> Self {
        Kind::Popup(popup)
    }
}

impl From<&Popup> for Kind {
    fn from(popup: &Popup) -> Self {
        Kind::Popup(popup.clone())
    }
}

impl From<(WlSurface, WlSubsurface, WlSurface)> for Kind {
    fn from((parent, subsurface, surface): (WlSurface, WlSubsurface, WlSurface)) -> Self {
        Kind::Subsurface {
            parent,
            subsurface,
            surface,
        }
    }
}
