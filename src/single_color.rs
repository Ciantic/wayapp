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
use std::cell::RefCell;
use std::num::NonZero;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wgpu::wgc::id;

#[derive(Debug)]
pub struct SingleColorManager {
    view_manager: ViewManager<(Option<SlotPool>, Option<WpViewport>, (u8, u8, u8))>,
    // Track last buffer update per surface
    last_buffer_update: HashMap<ObjectId, Instant>,
}

impl Default for SingleColorManager {
    fn default() -> Self {
        Self {
            view_manager: ViewManager::default(),
            last_buffer_update: HashMap::default(),
        }
    }
}

// Deref to ViewManager
impl std::ops::Deref for SingleColorManager {
    type Target = ViewManager<(Option<SlotPool>, Option<WpViewport>, (u8, u8, u8))>;

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
    pub fn new() -> Self {
        Self {
            view_manager: ViewManager::default(),
            last_buffer_update: HashMap::default(),
        }
    }

    fn resize_viewport(&mut self, app: &Application, surface: &WlSurface, width: u32, height: u32) {
        let surface_id = surface.id();

        if let Some((_, viewport, _)) = self.view_manager.get_data_by_id_mut(&surface_id) {
            let viewport = viewport.get_or_insert_with(|| {
                trace!(
                    "[SINGLE_COLOR] Creating viewport for surface {:?}",
                    surface_id
                );
                app.viewporter
                    .get()
                    .expect("wp_viewporter not available")
                    .get_viewport(surface, &app.qh, ())
            });

            viewport.set_destination(width as i32, height as i32);
        }

        // Handle subsurfaces
        let viewporter = app.viewporter.get().expect("wp_viewporter not available");
        let qh = &app.qh;

        self.view_manager.execute_recursively_to_all_subsurfaces(
            &surface,
            move |_subsurface, sub_wlsurface, (_, viewport_opt, _)| {
                let viewport = viewport_opt
                    .get_or_insert_with(|| viewporter.get_viewport(sub_wlsurface, qh, ()));
                viewport.set_destination(100, 30);
            },
        );
    }

    fn update_buffers(&mut self, app: &Application, surface: &WlSurface, width: u32, height: u32) {
        let surface_id = surface.id();

        if let Some((pool, viewport, color)) = self.view_manager.get_data_by_id_mut(&surface_id) {
            let viewport = viewport.as_ref().expect("Viewport should exist");

            let pool = pool.get_or_insert_with(|| {
                trace!("[SINGLE_COLOR] Creating buffer pool");
                SlotPool::new((width * height * 4).try_into().unwrap(), &app.shm_state)
                    .expect("Failed to create SlotPool")
            });

            single_color_example_buffer_configure(
                pool, surface, viewport, &app.qh, width, height, *color,
            );
        }

        // Handle subsurfaces
        let shm_state = &app.shm_state;
        let qh = &app.qh;

        self.view_manager.execute_recursively_to_all_subsurfaces(
            &surface,
            move |_subsurface, sub_wlsurface, (pool_opt, viewport_opt, color)| {
                let viewport = viewport_opt.as_ref().expect("Viewport should exist");

                let pool = pool_opt.get_or_insert_with(|| {
                    SlotPool::new((100 * 30 * 4).try_into().unwrap(), shm_state)
                        .expect("Failed to create SlotPool")
                });

                single_color_example_buffer_configure(
                    pool,
                    sub_wlsurface,
                    viewport,
                    qh,
                    100,
                    30,
                    *color,
                );
            },
        );
    }

    fn configure(&mut self, app: &Application, surface: &WlSurface, width: u32, height: u32) {
        const DEBOUNCE_MS: u64 = 32; // ~30fps, adjust as needed

        let surface_id = surface.id();
        let now = Instant::now();

        // Always resize viewport (fast operation)
        self.resize_viewport(app, surface, width, height);

        // Check if we should update buffers (debounced)
        let should_update_buffer = if let Some(last_time) = self.last_buffer_update.get(&surface_id)
        {
            now.duration_since(*last_time) >= Duration::from_millis(DEBOUNCE_MS)
        } else {
            true // First configure, always update
        };

        if should_update_buffer {
            // Update buffers (slow operation)
            self.update_buffers(app, surface, width, height);
            // TODO: BUG, this is not called when configures come too fast
        } else {
            // Just commit the surface with the new viewport destination
            surface.commit();
        }

        // Always update the timestamp to reset the debounce timer
        self.last_buffer_update.insert(surface_id, now);
    }

    pub fn handle_events(&mut self, app: &Application, events: &[WaylandEvent]) {
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
                    self.configure(app, &window.wl_surface(), width, height);
                }
                WaylandEvent::LayerShellConfigure(layer_surface, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;
                    self.configure(app, &layer_surface.wl_surface(), width, height);
                }
                WaylandEvent::PopupConfigure(popup, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;
                    self.configure(app, &popup.wl_surface(), width, height);
                }
                _ => {}
            }
        }
    }
}

fn single_color_example_buffer_configure(
    pool: &mut SlotPool,
    surface: &WlSurface,
    viewport: &WpViewport,
    qh: &QueueHandle<Application>,
    buffer_width: u32,
    buffer_height: u32,
    color: (u8, u8, u8),
) {
    trace!(
        "[COMMON] Create Color Buffer {}x{}",
        buffer_width, buffer_height
    );

    let stride = buffer_width as i32 * 4;
    // Create a buffer and paint it a simple color
    let (buffer, _maybe_canvas) = pool
        .create_buffer(
            buffer_width as i32,
            buffer_height as i32,
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

    // Set the source rectangle to the entire buffer
    viewport.set_source(0.0, 0.0, buffer_width as f64, buffer_height as f64);

    // Damage, frame and attach
    surface.damage_buffer(0, 0, buffer_width as i32, buffer_height as i32);
    surface.frame(qh, surface.clone());
    buffer.attach_to(surface).expect("buffer attach");
    surface.commit();
}
