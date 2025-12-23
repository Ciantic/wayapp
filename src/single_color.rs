///! Single color buffer example implementations for containers.
///!
///! Use this as an example to how to start implementing your own containers.
use crate::Application;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::KeyboardHandlerContainer;
use crate::Kind;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::ViewManager;
use crate::WaylandEvent;
use crate::WindowContainer;
use crate::get_app;
use egui::ahash::HashMap;
use log::trace;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::SlotPool;
use std::num::NonZero;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;
use wgpu::wgc::id;

#[derive(Debug, Default)]
pub struct SingleColorManager {
    view_manager: ViewManager<(Option<SlotPool>, (u8, u8, u8))>,
}

// Deref to ViewManager
impl std::ops::Deref for SingleColorManager {
    type Target = ViewManager<(Option<SlotPool>, (u8, u8, u8))>;

    fn deref(&self) -> &Self::Target {
        &self.view_manager
    }
}

impl std::ops::DerefMut for SingleColorManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.view_manager
    }
}

impl SingleColorManager {
    fn configure(&mut self, surface: &WlSurface, width: u32, height: u32) {
        // Configuration logic if needed
        if let Some((pool, color)) = self.view_manager.get_data_by_id_mut(&surface.id()) {
            let app = get_app();

            let pool = pool.get_or_insert_with(|| {
                SlotPool::new((width * height * 4).try_into().unwrap(), &app.shm_state)
                    .expect("Failed to create SlotPool")
            });

            single_color_example_buffer_configure(pool, surface, &app.qh, width, height, *color);
        }

        self.view_manager.execute_recursively_to_all_subsurfaces(
            &surface,
            |_subsurface, sub_wlsurface, (pool_opt, color)| {
                let app = get_app();
                trace!("Configuring subsurfaces of surface id: {:?}", surface.id());

                let pool = pool_opt.get_or_insert_with(|| {
                    SlotPool::new((width * height * 4).try_into().unwrap(), &app.shm_state)
                        .expect("Failed to create SlotPool")
                });
                single_color_example_buffer_configure(
                    pool,
                    sub_wlsurface,
                    &app.qh,
                    100,
                    30,
                    *color,
                );
            },
        );
    }

    pub fn handle_events(&mut self, events: &[WaylandEvent]) {
        for event in events {
            match event {
                WaylandEvent::WindowConfigure(window, configure) => {
                    let width = configure
                        .new_size
                        .0
                        .unwrap_or_else(|| NonZero::new(256).unwrap())
                        .get();
                    let height = configure
                        .new_size
                        .1
                        .unwrap_or_else(|| NonZero::new(256).unwrap())
                        .get();
                    self.configure(&window.wl_surface(), width, height);
                }
                WaylandEvent::LayerShellConfigure(layer_surface, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;
                    self.configure(&layer_surface.wl_surface(), width, height);
                }
                WaylandEvent::PopupConfigure(popup, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;
                    self.configure(&popup.wl_surface(), width, height);
                }
                _ => {}
            }
        }
    }
}

fn single_color_example_buffer_configure(
    pool: &mut SlotPool,
    surface: &WlSurface,
    qh: &QueueHandle<Application>,
    new_width: u32,
    new_height: u32,
    color: (u8, u8, u8),
) {
    trace!("[COMMON] Create Brown Buffer");

    let stride = new_width as i32 * 4;
    // Create a buffer and paint it a simple color
    let (buffer, _maybe_canvas) = pool
        .create_buffer(
            new_width as i32,
            new_height as i32,
            stride,
            wl_shm::Format::Argb8888,
        )
        .expect("create buffer");
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
