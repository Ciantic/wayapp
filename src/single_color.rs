
use std::num::NonZero;

use log::trace;
use smithay_client_toolkit::{seat::{keyboard::{KeyEvent, Modifiers}, pointer::PointerEvent}, shell::{WaylandSurface, wlr_layer::{LayerSurface, LayerSurfaceConfigure}, xdg::{popup::{Popup, PopupConfigure}, window::{Window, WindowConfigure}}}, shm::{Shm, slot::{Buffer, SlotPool}}};
use wayland_client::{QueueHandle, protocol::{wl_shm, wl_surface::WlSurface}};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

use crate::{Application, BaseTrait, CompositorHandlerContainer, KeyboardHandlerContainer, LayerSurfaceContainer, PointerHandlerContainer, PopupContainer, SubsurfaceContainer, WAYAPP, WindowContainer, get_app};


fn single_color_example_buffer_configure(pool: &mut SlotPool, shm_state: &Shm, surface: &WlSurface, qh: &QueueHandle<Application>, new_width: u32, new_height: u32, color: (u8, u8, u8)) {

    trace!("[COMMON] Create Brown Buffer");

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

pub struct ExampleSingleColorWindow {
    pub window: Window,
    pub color: (u8, u8, u8),
    pub pool: Option<SlotPool>,
}

impl CompositorHandlerContainer for ExampleSingleColorWindow {}
impl KeyboardHandlerContainer for ExampleSingleColorWindow {}
impl PointerHandlerContainer for ExampleSingleColorWindow {}
impl BaseTrait for ExampleSingleColorWindow {}

impl WindowContainer for ExampleSingleColorWindow {
    fn configure(
        &mut self,
        configure: &WindowConfigure,
    ) {
        let app = get_app();
        let width = configure.new_size.0.unwrap_or_else(|| NonZero::new(256).unwrap()).get();
        let height = configure.new_size.1.unwrap_or_else(|| NonZero::new(256).unwrap()).get();
        
        // Ensure pool exists
        let pool = self.pool.get_or_insert_with(|| {
            SlotPool::new( (width * height * 4).try_into().unwrap(), &app.shm_state).expect("Failed to create SlotPool")
        });

        // Handle window configuration changes here
        single_color_example_buffer_configure(pool, &app.shm_state, &self.window.wl_surface().clone(), &app.qh, width, height, self.color);
    }

    fn allowed_to_close(&self) -> bool {
        true
    }

    fn get_window(&self) -> &Window {
        &self.window
    }
}

pub struct ExampleSingleColorLayerSurface {
    pub layer_surface: LayerSurface,
    pub color: (u8, u8, u8),
    pub pool: Option<SlotPool>,
}

impl CompositorHandlerContainer for ExampleSingleColorLayerSurface {}
impl KeyboardHandlerContainer for ExampleSingleColorLayerSurface {}
impl PointerHandlerContainer for ExampleSingleColorLayerSurface {}
impl BaseTrait for ExampleSingleColorLayerSurface {}

impl LayerSurfaceContainer for ExampleSingleColorLayerSurface {
    fn configure(
        &mut self,
        config: &LayerSurfaceConfigure,
    ) {
        let app = get_app();
        let width = config.new_size.0;
        let height = config.new_size.1;
        
        // Ensure pool exists
        let pool = self.pool.get_or_insert_with(|| {
            SlotPool::new( (width * height * 4).try_into().unwrap(), &app.shm_state).expect("Failed to create SlotPool")
        });

        // Handle layer surface configuration changes here
        single_color_example_buffer_configure(pool, &app.shm_state, &self.layer_surface.wl_surface().clone(), &app.qh, width, height, self.color);
    }

    fn closed(&mut self) {
        // Handle layer surface close request here
    }

    fn get_layer_surface(&self) -> &LayerSurface {
        &self.layer_surface
    }
}

pub struct ExampleSingleColorPopup {
    pub popup: Popup,
    pub color: (u8, u8, u8),
    pub pool: Option<SlotPool>,
}

impl CompositorHandlerContainer for ExampleSingleColorPopup {}
impl KeyboardHandlerContainer for ExampleSingleColorPopup {}
impl PointerHandlerContainer for ExampleSingleColorPopup {}
impl BaseTrait for ExampleSingleColorPopup {}

impl PopupContainer for ExampleSingleColorPopup {
    fn configure(
        &mut self,
        config: &PopupConfigure,
    ) {
        let app = get_app();
        let width = config.width as u32;
        let height = config.height as u32;
        
        // Ensure pool exists
        let pool = self.pool.get_or_insert_with(|| {
            SlotPool::new( (width * height * 4).try_into().unwrap(), &app.shm_state).expect("Failed to create SlotPool")
        });

        // Handle popup configuration changes here
        single_color_example_buffer_configure(pool, &app.shm_state, &self.popup.wl_surface().clone(), &app.qh, width, height, self.color);
    }

    fn done(&mut self) {
        // Handle popup done event here
    }

    fn get_popup(&self) -> &Popup {
        &self.popup
    }
}

pub struct ExampleSingleColorSubsurface {
    pub wl_surface: WlSurface,
    pub color: (u8, u8, u8),
    pub pool: Option<SlotPool>,
}

impl CompositorHandlerContainer for ExampleSingleColorSubsurface {}
impl KeyboardHandlerContainer for ExampleSingleColorSubsurface {}
impl PointerHandlerContainer for ExampleSingleColorSubsurface {}
impl BaseTrait for ExampleSingleColorSubsurface {}

impl SubsurfaceContainer for ExampleSingleColorSubsurface {
    fn configure(&mut self, width: u32, height: u32) {
        let app = get_app();
        let pool = self.pool.get_or_insert_with(|| {
            SlotPool::new( (width * height * 4).try_into().unwrap(), &app.shm_state).expect("Failed to create SlotPool")
        });

        // Handle subsurface configuration changes here
        single_color_example_buffer_configure(pool, &app.shm_state, &self.wl_surface.clone(), &app.qh, width, height, self.color);
    }

    fn get_wl_surface(&self) -> &WlSurface {
        &self.wl_surface
    }
}
