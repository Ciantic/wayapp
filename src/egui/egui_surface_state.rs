//! EGUI view manager implementation
//!
//! This module provides a ViewManager-based approach to handling EGUI surfaces
//! following the pattern from single_color.rs

use crate::Application;
use crate::EguiWgpuRenderer;
#[allow(unused_imports)]
use crate::EguiWgpuRendererThread;
use crate::FrameScheduler;
use crate::Kind;
use crate::WaylandEvent;
use crate::WaylandToEguiInput;
use crate::egui_to_cursor_shape;
use egui::Context;
use log::trace;
use smithay_client_toolkit::reexports::csd_frame::WindowState;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_clipboard::Clipboard;
use std::num::NonZero;
use std::ops::Deref;
use std::ops::DerefMut;
use std::time::Duration;
use std::time::Instant;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

/// Surface-specific EGUI state
pub struct EguiSurfaceState<T: Into<Kind> + Clone> {
    viewport: Option<WpViewport>,
    t: T,
    kind: Kind,
    // renderer: EguiWgpuRendererThread, // for async rendering thread
    renderer: EguiWgpuRenderer, // for direct rendering (sync)
    input_state: WaylandToEguiInput,
    init_width: u32,
    init_height: u32,
    width: u32,  // WGPU Surface width in logical pixels
    height: u32, // WGPU Surface height in logical pixels
    scale_factor: i32,
    suspended: bool,
    last_fulloutput: Option<egui::FullOutput>,
    frame_timings: Option<(Instant, Instant)>,
    has_keyboard_focus: bool,
    egui_context: Context,
    frame_scheduler: FrameScheduler,
}

impl<T: Into<Kind> + Clone> EguiSurfaceState<T> {
    pub fn new(app: &Application, t: T, width: u32, height: u32) -> Self {
        let kind = t.clone().into();
        let wl_surface = kind.get_wl_surface();
        let egui_context = Context::default();
        let renderer = EguiWgpuRenderer::new(&egui_context, wl_surface, &app.conn);
        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = WaylandToEguiInput::new(clipboard);
        let emitter = app.get_event_emitter();
        let wl_surface_clone = wl_surface.clone();
        let frame_scheduler = FrameScheduler::new(move || {
            // Note: Using wl_surface.frame(), wl_surface.commit(), conn.flush()
            // caused crashes with WGPU handling, so I created a way to emit Frame
            // event without Wayland dispatching.
            emitter.emit_events(vec![crate::WaylandEvent::Frame(
                wl_surface_clone.clone(),
                0,
            )]);
        });
        let frame_scheduler_fn = frame_scheduler.create_scheduler();
        egui_context.set_request_repaint_callback(move |i| {
            frame_scheduler_fn(i.delay);
        });

        Self {
            viewport: None,
            t,
            kind,
            renderer,
            input_state,
            init_height: height,
            init_width: width,
            width,
            height,
            scale_factor: 1,
            suspended: false,
            last_fulloutput: None,
            frame_timings: None,
            has_keyboard_focus: false,
            egui_context,
            frame_scheduler,
        }
    }

    pub fn wl_surface(&self) -> &WlSurface {
        self.kind.get_wl_surface()
    }

    pub fn get_kind(&self) -> &Kind {
        &self.kind
    }

    pub fn contains<V: Into<Kind>>(&self, other: V) -> bool {
        self.kind == other.into()
    }

    fn configure(
        &mut self,
        app: &Application,
        width: u32,
        height: u32,
        window_state: Option<WindowState>,
    ) {
        self.resize_viewport(app, width, height);
        self.width = width.max(1);
        self.height = height.max(1);
        self.input_state.set_screen_size(self.width, self.height);
        self.suspended = window_state.map_or(false, |state| state.contains(WindowState::SUSPENDED));
        self.frame_scheduler.set_fps_target(
            if window_state.map_or(false, |state| state.contains(WindowState::SUSPENDED)) {
                0.05 // 0.0001
            } else {
                60.0
            },
        );
    }

    fn resize_viewport(&mut self, app: &Application, width: u32, height: u32) {
        let wl_surface = self.wl_surface().clone();
        let viewport = self.viewport.get_or_insert_with(|| {
            trace!("[EGUI] Creating viewport for surface {:?}", wl_surface.id());
            app.viewporter
                .get()
                .expect("wp_viewporter not available")
                .get_viewport(&wl_surface, &app.qh, ())
        });

        viewport.set_destination(width as i32, height as i32);
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        self.input_state.handle_pointer_event(event);
    }

    fn handle_keyboard_enter(&mut self) {
        self.input_state.handle_keyboard_enter();
    }

    fn handle_keyboard_leave(&mut self) {
        self.input_state.handle_keyboard_leave();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state
            .handle_keyboard_event(event, pressed, repeat);
    }

    fn update_modifiers(&mut self, modifiers: &WaylandModifiers) {
        self.input_state.update_modifiers(modifiers);
    }

    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.wl_surface().set_buffer_scale(new_factor);
        let factor = new_factor.max(1);
        if factor == self.scale_factor {
            return;
        }
        self.scale_factor = factor;
    }

    /// Request a frame via dispatching
    ///
    /// Strictly this wouldn't be necessary, as
    /// `egui_frame_scheduler.schedule_frame` could be used. However, this
    /// method can be used to break the FPS limit, and I wanted to break it
    /// for the window resizing events at least for now.
    fn request_dispatch_frame(&mut self, app: &mut Application) {
        self.wl_surface().frame(&app.qh, self.wl_surface().clone());
        self.wl_surface().commit();
        app.conn.flush().unwrap();
    }

    /// Request a frame via Frame scheduler
    pub fn request_frame(&mut self) {
        self.frame_scheduler.schedule_frame(Duration::ZERO);
    }

    /// Process EGUI frame (layout, input) without GPU rendering
    /// This is cheap and can be called frequently
    fn process_egui_frame(&mut self, ui: &mut impl FnMut(&egui::Context)) {
        let raw_input = self.input_state.take_raw_input();
        self.egui_context
            .set_pixels_per_point(self.physical_scale() as f32);
        let full_output = self.egui_context.run(raw_input, ui);
        for command in &full_output.platform_output.commands {
            self.input_state.handle_output_command(command);
        }
        if let Some(last_fulloutput) = &mut self.last_fulloutput {
            last_fulloutput.append(full_output);
        } else {
            self.last_fulloutput = Some(full_output);
        }
    }

    /// Render the last processed EGUI frame to WGPU
    /// Only call this when necessary (e.g., on frame callback or when content
    /// changed)
    fn render_to_wgpu(&mut self, full_output: egui::FullOutput) {
        let width = self.width.saturating_mul(self.physical_scale());
        let height = self.height.saturating_mul(self.physical_scale());
        let pixels_per_point = self.physical_scale() as f32;

        self.renderer
            .render_to_wgpu(full_output, width, height, pixels_per_point);

        // Update frame timings
        let now = Instant::now();
        let old = self
            .frame_timings
            .as_ref()
            .map(|(_, end)| *end)
            .unwrap_or(now);
        self.frame_timings = Some((old, now));
    }

    /// Full render of EGUI frame (layout, input + GPU rendering)
    fn render(&mut self, ui: &mut impl FnMut(&egui::Context)) {
        self.process_egui_frame(ui);

        if self.suspended {
            trace!(
                "[EGUI] Skipping rendering for suspended surface {:?}",
                self.wl_surface().id()
            );
            return;
        }

        if let Some(full_output) = self.last_fulloutput.take() {
            self.render_to_wgpu(full_output);
        }
    }

    fn physical_scale(&self) -> u32 {
        self.scale_factor.max(1) as u32
    }

    /// Get the last frame timings (previous frame time, current frame time)
    pub fn get_frame_timings(&self) -> Option<(Instant, Instant)> {
        self.frame_timings
    }

    /// Handle Wayland events and update surfaces accordingly
    /// Returns an optional cursor shape change
    pub fn handle_events(
        &mut self,
        app: &mut Application,
        events: &[WaylandEvent],
        ui: &mut impl FnMut(&egui::Context),
    ) {
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

                    self.configure(app, width, height, Some(configure.state));
                    self.request_dispatch_frame(app);
                }
                WaylandEvent::LayerShellConfigure(_, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;

                    self.configure(app, width, height, None);
                    self.request_dispatch_frame(app);
                }
                WaylandEvent::PopupConfigure(_, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;

                    self.configure(app, width, height, None);
                    self.request_dispatch_frame(app);
                }
                WaylandEvent::Frame(_, _) => {
                    self.render(ui);
                }
                WaylandEvent::ScaleFactorChanged(_, factor) => {
                    self.scale_factor_changed(*factor);
                    self.process_egui_frame(ui);
                    self.request_frame();
                }
                WaylandEvent::PointerEvent((surface, position, event_kind)) => {
                    self.handle_pointer_event(&PointerEvent {
                        surface: surface.clone(),
                        position: position.clone(),
                        kind: event_kind.clone(),
                    });
                    self.process_egui_frame(ui);
                    if let Some(cursor) = self
                        .last_fulloutput
                        .as_ref()
                        .map(|o| o.platform_output.cursor_icon)
                    {
                        app.set_cursor(egui_to_cursor_shape(cursor));
                    }
                }
                WaylandEvent::KeyboardEnter(_, _serials, _keysyms) => {
                    self.handle_keyboard_enter();
                    self.has_keyboard_focus = true;
                    self.process_egui_frame(ui);
                }
                WaylandEvent::KeyboardLeave(_) => {
                    self.handle_keyboard_leave();
                    self.has_keyboard_focus = false;
                    self.process_egui_frame(ui);
                }
                WaylandEvent::KeyPress(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, true, false);
                        self.process_egui_frame(ui);
                    }
                }
                WaylandEvent::KeyRelease(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, false, false);
                        self.process_egui_frame(ui);
                    }
                }
                WaylandEvent::KeyRepeat(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, true, true);
                        self.process_egui_frame(ui);
                    }
                }
                WaylandEvent::ModifiersChanged(modifiers) => {
                    self.update_modifiers(modifiers);
                    self.process_egui_frame(ui);
                }
                _ => {}
            }
        }
    }
}

impl<T: Into<Kind> + Clone> Deref for EguiSurfaceState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<T: Into<Kind> + Clone> DerefMut for EguiSurfaceState<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

/// Helper trait to allow calling `contains` on `Option<EguiSurfaceState<T>>`
pub trait OptionEguiSurfaceStateExt<T: Into<Kind> + Clone> {
    fn contains<V: Into<Kind>>(&self, other: V) -> bool;

    fn handle_events(
        &mut self,
        app: &mut Application,
        events: &[WaylandEvent],
        ui: &mut impl FnMut(&egui::Context),
    ) -> ();
}

impl<T: Into<Kind> + Clone> OptionEguiSurfaceStateExt<T> for Option<EguiSurfaceState<T>> {
    fn contains<V: Into<Kind>>(&self, other: V) -> bool {
        self.as_ref().map_or(false, |s| s.contains(other))
    }

    fn handle_events(
        &mut self,
        app: &mut Application,
        events: &[WaylandEvent],
        ui: &mut impl FnMut(&egui::Context),
    ) -> () {
        if let Some(surface_state) = self {
            surface_state.handle_events(app, events, ui);
        }
    }
}
