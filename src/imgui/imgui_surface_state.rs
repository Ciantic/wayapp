//! Surface-level imgui state — analogous to `EguiSurfaceState`.
//!
//! Owns the imgui [`Context`], the [`ImguiWaylandPlatform`] input bridge,
//! and the [`ImguiGlowRenderer`] GPU renderer, and routes [`WaylandEvent`]s
//! to all of them.

use crate::Application;
use crate::FrameScheduler;
use crate::ImguiGlowRenderer;
use crate::ImguiWaylandPlatform;
use crate::Kind;
use crate::WaylandEvent;
use imgui::Context;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use std::num::NonZero;
use std::time::Duration;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;

pub struct ImguiSurfaceState<T: Into<Kind> + Clone> {
    t: T,
    kind: Kind,
    imgui: Context,
    platform: ImguiWaylandPlatform,
    renderer: ImguiGlowRenderer,
    init_width: u32,
    init_height: u32,
    width: u32,
    height: u32,
    scale_factor: i32,
    has_keyboard_focus: bool,
    /// Frame scheduler to cap FPS and batch render requests
    frame_scheduler: FrameScheduler,
    frame_timings: Option<(std::time::Instant, std::time::Instant)>,
}

impl<T: Into<Kind> + Clone> ImguiSurfaceState<T> {
    pub fn new(app: &Application, t: T, width: u32, height: u32) -> Self {
        let kind = t.clone().into();
        let wl_surface = kind.get_wl_surface();

        let mut imgui = Context::create();
        imgui.set_ini_filename(None);

        let mut platform = ImguiWaylandPlatform::new(&mut imgui);
        let renderer = ImguiGlowRenderer::new(wl_surface, &app.conn, &mut imgui, width, height, 1);

        // Set up display size on the platform now that the renderer is ready.
        {
            let io = imgui.io_mut();
            platform.attach_window(io, width, height, 1.0);
        }

        // Create frame scheduler to cap FPS (default 60 FPS)
        let emitter = app.get_event_emitter();
        let wl_surface_clone = wl_surface.clone();
        let frame_scheduler = FrameScheduler::new(move || {
            // Emit Frame event to trigger render at scheduled time
            emitter.emit_events(vec![crate::WaylandEvent::Frame(
                wl_surface_clone.clone(),
                0,
            )]);
        });

        Self {
            t,
            kind,
            imgui,
            platform,
            renderer,
            init_width: width,
            init_height: height,
            width,
            height,
            scale_factor: 1,
            has_keyboard_focus: false,
            frame_scheduler,
            frame_timings: None,
        }
    }

    /// Request a repaint, which will be scheduled according to FPS target
    fn request_repaint(&mut self) {
        self.frame_scheduler.schedule_frame(Duration::ZERO);
    }

    pub fn get_content(&self) -> &T {
        &self.t
    }

    fn wl_surface(&self) -> &WlSurface {
        self.kind.get_wl_surface()
    }

    fn configure(
        &mut self,
        _app: &mut Application,
        width: u32,
        height: u32,
        ui_fn: &mut impl FnMut(&imgui::Ui),
    ) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.renderer.resize(self.width, self.height);
        let io = self.imgui.io_mut();
        self.platform.handle_resize(io, self.width, self.height);
        self.render(ui_fn);
    }

    fn render(&mut self, ui_fn: &mut impl FnMut(&imgui::Ui)) {
        let io = self.imgui.io_mut();
        self.platform.prepare_frame(io);

        let ui = self.imgui.new_frame();
        ui_fn(ui);

        let draw_data = self.imgui.render();
        self.renderer.render(draw_data, self.width, self.height);

        // Update frame timings
        let now = std::time::Instant::now();
        let old = self
            .frame_timings
            .as_ref()
            .map(|(_, end)| *end)
            .unwrap_or(now);
        self.frame_timings = Some((old, now));

        if let Some(shape) = self.platform.cursor_shape(&self.imgui) {
            // Cursor shape changes can be wired to app.set_cursor(shape) here.
            let _ = shape;
        }
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        let io = self.imgui.io_mut();
        self.platform.handle_pointer_event(io, event);
        // Input events may change UI state, request a render
        self.request_repaint();
    }

    fn handle_keyboard_enter(&mut self) {
        let io = self.imgui.io_mut();
        self.platform.handle_keyboard_enter(io);
        self.has_keyboard_focus = true;
        self.request_repaint();
    }

    fn handle_keyboard_leave(&mut self) {
        let io = self.imgui.io_mut();
        self.platform.handle_keyboard_leave(io);
        self.has_keyboard_focus = false;
        self.request_repaint();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, is_repeat: bool) {
        if self.has_keyboard_focus {
            let io = self.imgui.io_mut();
            self.platform
                .handle_keyboard_event(io, event, pressed, is_repeat);
            self.request_repaint();
        }
    }

    fn update_modifiers(&mut self, mods: &WaylandModifiers) {
        let io = self.imgui.io_mut();
        self.platform.update_modifiers(io, mods);
        self.request_repaint();
    }

    fn scale_factor_changed(&mut self, factor: i32) {
        let factor = factor.max(1);
        self.scale_factor = factor;
        self.wl_surface().set_buffer_scale(factor);
        self.renderer.set_scale_factor(factor);
        // Resize EGL window with new scale factor
        self.renderer.resize(self.width, self.height);
        let scale = factor as f64;
        let io = self.imgui.io_mut();
        self.platform.handle_scale_factor_changed(io, scale);
        self.request_repaint();
    }

    /// Handle all pending Wayland events for this surface.
    pub fn handle_events(
        &mut self,
        app: &mut Application,
        events: &[WaylandEvent],
        ui_fn: &mut impl FnMut(&imgui::Ui),
    ) {
        for event in events {
            // Filter surface-specific events.
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
                    self.configure(app, width, height, ui_fn);
                }
                WaylandEvent::LayerShellConfigure(_, config) => {
                    self.configure(app, config.new_size.0, config.new_size.1, ui_fn);
                }
                WaylandEvent::Frame(_, _) => {
                    self.render(ui_fn);
                }
                WaylandEvent::ScaleFactorChanged(_, factor) => {
                    self.scale_factor_changed(*factor);
                }
                WaylandEvent::PointerEvent((surface, position, event_kind)) => {
                    self.handle_pointer_event(&PointerEvent {
                        surface: surface.clone(),
                        position: *position,
                        kind: event_kind.clone(),
                    });
                }
                WaylandEvent::KeyboardEnter(_, _, _) => {
                    self.handle_keyboard_enter();
                }
                WaylandEvent::KeyboardLeave(_) => {
                    self.handle_keyboard_leave();
                }
                WaylandEvent::KeyPress(key_event) => {
                    self.handle_keyboard_event(key_event, true, false);
                }
                WaylandEvent::KeyRelease(key_event) => {
                    self.handle_keyboard_event(key_event, false, false);
                }
                WaylandEvent::KeyRepeat(key_event) => {
                    self.handle_keyboard_event(key_event, true, true);
                }
                WaylandEvent::ModifiersChanged(mods) => {
                    self.update_modifiers(mods);
                }
                _ => {}
            }
        }
    }

    /// Get the last frame timings (previous frame time, current frame time)
    pub fn get_frame_timings(&self) -> Option<(std::time::Instant, std::time::Instant)> {
        self.frame_timings
    }

    /// Get FPS of last two frames
    ///
    /// ImGui is immediate mode GUI, this means it easier to show historical
    /// FPS, as most of the time it could be zero.
    pub fn get_fps(&self) -> f32 {
        if let Some((prev_frame, next_frame)) = &self.frame_timings {
            let frame_time = next_frame.duration_since(prev_frame.clone()).as_secs_f32();
            if frame_time > 0.0 {
                return 1.0 / frame_time;
            }
        }
        return 0.0;
    }
}
