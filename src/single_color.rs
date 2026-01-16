///! Single color buffer example implementations for containers.
///!
///! Use this as an example to how to start implementing your own containers.
use crate::Application;
use crate::Kind;
use crate::WaylandEvent;
use log::trace;
use smithay_client_toolkit::shm::slot::SlotPool;
use std::num::NonZero;
use std::ops::Deref;
use std::ops::DerefMut;
use std::time::Duration;
use std::time::Instant;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

#[derive(Debug)]
pub struct SingleColorState<T: Into<Kind> + Clone> {
    t: T,
    kind: Kind,
    slotpool: Option<SlotPool>,
    viewport: Option<WpViewport>,
    color: (u8, u8, u8),
    last_buffer_update: Option<Instant>,
    init_width: u32,
    init_height: u32,
}

impl<T: Into<Kind> + Clone> SingleColorState<T> {
    pub fn new(t: T, color: (u8, u8, u8), width: u32, height: u32) -> Self {
        Self {
            kind: t.clone().into(),
            t,
            slotpool: None,
            viewport: None,
            color,
            last_buffer_update: None,
            init_width: width,
            init_height: height,
        }
    }

    pub fn wl_surface(&self) -> &WlSurface {
        self.kind.get_wl_surface()
    }

    fn resize_viewport(&mut self, app: &Application, width: u32, height: u32) {
        let surface = self.wl_surface().clone();
        let surface_id = surface.id();

        let viewport = self.viewport.get_or_insert_with(|| {
            trace!(
                "[SINGLE_COLOR] Creating viewport for surface {:?}",
                surface_id
            );
            app.viewporter
                .get()
                .expect("wp_viewporter not available")
                .get_viewport(&surface, &app.qh, ())
        });

        viewport.set_destination(width as i32, height as i32);
    }

    fn update_buffers(&mut self, app: &Application, width: u32, height: u32) {
        let surface = self.wl_surface().clone();
        let viewport = self.viewport.as_ref().expect("Viewport should exist");
        let pool = self.slotpool.get_or_insert_with(|| {
            trace!("[SINGLE_COLOR] Creating buffer pool");
            SlotPool::new((width * height * 4).try_into().unwrap(), &app.shm_state)
                .expect("Failed to create SlotPool")
        });

        single_color_example_buffer_configure(
            pool, &surface, viewport, &app.qh, width, height, self.color,
        );
    }

    fn configure(&mut self, app: &Application, width: u32, height: u32) {
        trace!(
            "[SINGLE_COLOR] Configure received for surface {:?}: {}x{}",
            self.wl_surface().id(),
            width,
            height
        );
        const DEBOUNCE_MS: u64 = 32; // ~30fps, adjust as needed
        let surface = self.wl_surface().clone();

        let now = Instant::now();

        // Always resize viewport (fast operation)
        self.resize_viewport(app, width, height);

        // Check if we should update buffers (debounced)
        let should_update_buffer = if let Some(last_time) = self.last_buffer_update {
            now.duration_since(last_time) >= Duration::from_millis(DEBOUNCE_MS)
        } else {
            true // First configure, always update
        };

        if should_update_buffer {
            // Update buffers (slow operation)
            self.update_buffers(app, width, height);
            // TODO: BUG, this is not called when configures come too fast
        } else {
            // Just commit the surface with the new viewport destination
            surface.commit();
        }

        // Always update the timestamp to reset the debounce timer
        self.last_buffer_update = Some(now);
    }

    pub fn handle_events(&mut self, app: &Application, events: &[WaylandEvent]) {
        for event in events {
            if let Some(surface) = event.get_wl_surface() {
                if surface.id() != self.wl_surface().id() {
                    continue;
                }
            }
            match event {
                WaylandEvent::WindowConfigure(_, configure) => {
                    let width = configure
                        .new_size
                        .0
                        .unwrap_or_else(|| NonZero::new(self.init_width).unwrap())
                        .get();
                    let height = configure
                        .new_size
                        .1
                        .unwrap_or_else(|| NonZero::new(self.init_height).unwrap())
                        .get();
                    self.configure(app, width, height);
                }
                WaylandEvent::LayerShellConfigure(_, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;
                    self.configure(app, width, height);
                }
                WaylandEvent::PopupConfigure(_, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;
                    self.configure(app, width, height);
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

impl<T: Into<Kind> + Clone> Deref for SingleColorState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<T: Into<Kind> + Clone> DerefMut for SingleColorState<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}
