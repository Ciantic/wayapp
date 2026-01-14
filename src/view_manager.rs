///! View manager for different kinds of surfaces
use egui::ahash::HashMap;
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
    pub fn get_object_id(&self) -> ObjectId {
        match self {
            Kind::Window(window) => window.wl_surface().id(),
            Kind::LayerSurface(layer_surface) => layer_surface.wl_surface().id(),
            Kind::Popup(popup) => popup.wl_surface().id(),
            Kind::Subsurface { surface, .. } => surface.id(),
        }
    }

    pub fn is_window(&self, other: &Window) -> bool {
        match self {
            Kind::Window(_) => self.get_object_id() == other.wl_surface().id(),
            _ => false,
        }
    }

    pub fn is_layer_surface(&self, other: &LayerSurface) -> bool {
        match self {
            Kind::LayerSurface(_) => self.get_object_id() == other.wl_surface().id(),
            _ => false,
        }
    }

    pub fn is_popup(&self, other: &Popup) -> bool {
        match self {
            Kind::Popup(_) => self.get_object_id() == other.wl_surface().id(),
            _ => false,
        }
    }

    pub fn is_subsurface(&self, other: &WlSurface) -> bool {
        match self {
            Kind::Subsurface { .. } => self.get_object_id() == other.id(),
            _ => false,
        }
    }
}
impl PartialEq for Kind {
    fn eq(&self, other: &Self) -> bool {
        self.get_object_id() == other.get_object_id()
    }
}
impl Eq for Kind {}

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

#[derive(Debug, Clone, Default)]
pub struct ViewManager<T> {
    surfaces_by_id: HashMap<ObjectId, Kind>,
    data_by_id: HashMap<ObjectId, T>,

    // Parent object ID mapped to list of subsurface's WlSurface object IDs
    subsurfaces_by_parent: HashMap<ObjectId, Vec<(WlSubsurface, WlSurface)>>,
}

impl<D> ViewManager<D> {
    pub fn new() -> Self {
        Self {
            surfaces_by_id: HashMap::default(),
            data_by_id: HashMap::default(),
            subsurfaces_by_parent: HashMap::default(),
        }
    }

    pub fn push<T: Into<Kind>>(&mut self, kind: T, data: D) {
        let kind = kind.into();
        self.surfaces_by_id
            .insert(kind.get_object_id(), kind.clone());
        self.data_by_id.insert(kind.get_object_id(), data);

        if let Kind::Subsurface {
            parent,
            subsurface,
            surface,
        } = kind
        {
            self.subsurfaces_by_parent
                .entry(parent.id())
                .or_insert_with(Vec::new)
                .push((subsurface.clone(), surface.clone()));
        }
    }

    pub fn remove<T: Into<Kind>>(&mut self, kind: T) {
        let kind = kind.into();
        self.data_by_id.remove(&kind.get_object_id());
        self.surfaces_by_id.remove(&kind.get_object_id());
        if let Kind::Subsurface {
            parent,
            subsurface: _,
            surface: _,
        } = kind
        {
            let parent_id = parent.id();
            self.subsurfaces_by_parent.remove(&parent_id);
        }
    }

    pub fn get_data_by_id_mut(&mut self, id: &ObjectId) -> Option<&mut D> {
        self.data_by_id.get_mut(id)
    }

    fn get_sub_wlsurfaces(&self, parent: &WlSurface) -> &[(WlSubsurface, WlSurface)] {
        self.subsurfaces_by_parent
            .get(&parent.id())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn execute_recursively_to_all_subsurfaces<F>(&mut self, parent: &WlSurface, mut func: F)
    where
        F: FnMut(&WlSubsurface, &WlSurface, &mut D),
    {
        self.execute_recursively_to_all_subsurfaces_impl(parent, &mut func);
    }

    fn execute_recursively_to_all_subsurfaces_impl<F>(&mut self, parent: &WlSurface, func: &mut F)
    where
        F: FnMut(&WlSubsurface, &WlSurface, &mut D),
    {
        let subsurfaces = self.get_sub_wlsurfaces(parent).to_vec();
        for (wlsubsurface, sub_wlsurface) in subsurfaces {
            if let Some(data) = self.get_data_by_id_mut(&sub_wlsurface.id()) {
                func(&wlsubsurface, &sub_wlsurface, data);
            }
            // Recurse into subsurfaces of this subsurface
            self.execute_recursively_to_all_subsurfaces_impl(&sub_wlsurface, func);
        }
    }
}
