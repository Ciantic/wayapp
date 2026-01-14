//! EGUI view manager implementation
//!
//! This module provides a ViewManager-based approach to handling EGUI surfaces
//! following the pattern from single_color.rs

#![allow(dead_code)]

use crate::Application;
use crate::ViewManager;
use crate::WaylandEvent;
use crate::egui_new::EguiWgpuRenderer;
use egui::Event;
use egui::Key;
use egui::Modifiers as EguiModifiers;
use egui::PlatformOutput;
use egui::PointerButton;
use egui::Pos2;
use egui::RawInput;
use egui::ahash::HashMap;
use egui_wgpu::Renderer;
use egui_wgpu::RendererOptions;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu;
use log::trace;
use pollster::block_on;
use raw_window_handle::RawDisplayHandle;
use raw_window_handle::RawWindowHandle;
use raw_window_handle::WaylandDisplayHandle;
use raw_window_handle::WaylandWindowHandle;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_clipboard::Clipboard;
use std::num::NonZero;
use std::ptr::NonNull;
use std::time::Duration;
use std::time::Instant;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

/// Trait that applications must implement to provide EGUI UI
pub trait EguiAppData {
    fn ui(&mut self, ctx: &egui::Context);
}

/// Handles input events from Wayland and converts them to EGUI RawInput
struct WaylandToEguiInput {
    modifiers: EguiModifiers,
    pointer_pos: Pos2,
    events: Vec<Event>,
    screen_width: u32,
    screen_height: u32,
    start_time: Instant,
    clipboard: Clipboard,
    last_key_utf8: Option<String>,
}

impl WaylandToEguiInput {
    fn new(clipboard: Clipboard) -> Self {
        Self {
            modifiers: EguiModifiers::default(),
            pointer_pos: Pos2::ZERO,
            events: Vec::new(),
            screen_width: 256,
            screen_height: 256,
            start_time: Instant::now(),
            clipboard,
            last_key_utf8: None,
        }
    }

    fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        match &event.kind {
            PointerEventKind::Enter { .. } => {}
            PointerEventKind::Leave { .. } => {
                self.events.push(Event::PointerGone);
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = Pos2::new(x as f32, y as f32);
                self.events.push(Event::PointerMoved(self.pointer_pos));
            }
            PointerEventKind::Press { button, .. } => {
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: true,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Release { button, .. } => {
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: false,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                let scroll_delta = egui::vec2(
                    horizontal.discrete as f32 * 10.0,
                    vertical.discrete as f32 * 10.0,
                );
                if scroll_delta != egui::Vec2::ZERO {
                    self.events.push(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: scroll_delta,
                        modifiers: self.modifiers,
                    });
                }
            }
        }
    }

    fn handle_keyboard_enter(&mut self) {
        self.events.push(Event::WindowFocused(true));
    }

    fn handle_keyboard_leave(&mut self) {
        self.events.push(Event::WindowFocused(false));
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, is_repeat: bool) {
        if pressed && !is_repeat && self.modifiers.ctrl {
            match event.keysym {
                Keysym::c => self.events.push(Event::Copy),
                Keysym::x => self.events.push(Event::Cut),
                Keysym::v => self
                    .events
                    .push(Event::Paste(self.clipboard.load().unwrap_or_default())),
                _ => (),
            }
        }

        if let Some(key) = keysym_to_egui_key(event.keysym) {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: is_repeat,
                modifiers: self.modifiers,
            });
        }

        if pressed || is_repeat {
            let mut text = event.utf8.clone();
            if is_repeat && text.is_none() {
                text = self.last_key_utf8.clone();
            }
            if let Some(text) = text {
                if !text.chars().any(|c| c.is_control()) {
                    self.events.push(Event::Text(text.clone()));
                }
            }
        }

        if event.utf8.is_some() {
            self.last_key_utf8 = event.utf8.clone();
        }
    }

    fn update_modifiers(&mut self, wayland_mods: &WaylandModifiers) {
        self.modifiers = EguiModifiers {
            alt: wayland_mods.alt,
            ctrl: wayland_mods.ctrl,
            shift: wayland_mods.shift,
            mac_cmd: false,
            command: wayland_mods.ctrl,
        };
    }

    fn take_raw_input(&mut self) -> RawInput {
        let events = std::mem::take(&mut self.events);
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(self.screen_width as f32, self.screen_height as f32),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            modifiers: self.modifiers,
            events,
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true,
            ..Default::default()
        }
    }

    fn handle_output_command(&mut self, output: &egui::OutputCommand) {
        match output {
            egui::OutputCommand::CopyText(text) => {
                self.clipboard.store(text.clone());
            }
            egui::OutputCommand::CopyImage(_) => {}
            egui::OutputCommand::OpenUrl(_) => {}
        }
    }
}

/// Surface-specific EGUI state
struct EguiSurfaceState<A: EguiAppData> {
    viewport: Option<WpViewport>,
    wl_surface: WlSurface,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: EguiWgpuRenderer,
    egui_app: A,
    input_state: WaylandToEguiInput,
    queue_handle: QueueHandle<Application>,
    width: u32,
    height: u32,
    scale_factor: i32,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    output_format: wgpu::TextureFormat,
    last_buffer_update: Option<Instant>,
}

impl<A: EguiAppData> EguiSurfaceState<A> {
    fn new(app: &Application, wl_surface: WlSurface, egui_app: A) -> Self {
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(app.conn.backend().display_ptr() as *mut _)
                .expect("Wayland display pointer was null"),
        ));
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(wl_surface.id().as_ptr() as *mut _)
                .expect("Wayland surface handle was null"),
        ));

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle,
                    raw_window_handle,
                })
                .expect("Failed to create WGPU surface")
        };

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("Failed to find a suitable adapter");

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("Failed to request WGPU device");

        let caps = surface.get_capabilities(&adapter);
        let output_format = *caps
            .formats
            .get(0)
            .unwrap_or(&wgpu::TextureFormat::Bgra8Unorm);

        let renderer = EguiWgpuRenderer::new(&device, output_format, 1);
        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = WaylandToEguiInput::new(clipboard);

        Self {
            viewport: None,
            wl_surface,
            surface,
            device,
            queue,
            renderer,
            egui_app,
            input_state,
            queue_handle: app.qh.clone(),
            width: 256,
            height: 256,
            scale_factor: 1,
            surface_config: None,
            output_format,
            last_buffer_update: None,
        }
    }

    fn configure(&mut self, app: &Application, width: u32, height: u32) {
        const DEBOUNCE_MS: u64 = 16; // ~60fps, adjust as needed

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
            self.update_buffers(width, height);
            self.last_buffer_update = Some(now);
        } else {
            // Just commit the surface with the new viewport destination
            self.wl_surface.commit();
        }
    }

    fn resize_viewport(&mut self, app: &Application, width: u32, height: u32) {
        let viewport = self.viewport.get_or_insert_with(|| {
            trace!(
                "[EGUI] Creating viewport for surface {:?}",
                self.wl_surface.id()
            );
            app.viewporter
                .get()
                .expect("wp_viewporter not available")
                .get_viewport(&self.wl_surface, &app.qh, ())
        });

        viewport.set_destination(width as i32, height as i32);
    }

    fn update_buffers(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.input_state.set_screen_size(self.width, self.height);
        self.reconfigure_surface();
        self.render();
    }

    fn frame(&mut self, _time: u32) {
        self.render();
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) -> Option<Shape> {
        self.input_state.handle_pointer_event(event);
        let platform_output = self.render();
        Some(egui_to_cursor_shape(platform_output.cursor_icon))
    }

    fn handle_keyboard_enter(&mut self) {
        self.input_state.handle_keyboard_enter();
        self.render();
    }

    fn handle_keyboard_leave(&mut self) {
        self.input_state.handle_keyboard_leave();
        self.render();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state
            .handle_keyboard_event(event, pressed, repeat);
        self.render();
    }

    fn update_modifiers(&mut self, modifiers: &WaylandModifiers) {
        self.input_state.update_modifiers(modifiers);
        self.render();
    }

    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.wl_surface.set_buffer_scale(new_factor);
        let factor = new_factor.max(1);
        if factor == self.scale_factor {
            return;
        }
        self.scale_factor = factor;
        self.reconfigure_surface();
        self.render();
    }

    fn render(&mut self) -> PlatformOutput {
        trace!("Rendering EGUI surface {}", self.wl_surface.id());
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire next surface texture");

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // Clear pass
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        let raw_input = self.input_state.take_raw_input();
        self.renderer.begin_frame(raw_input);
        self.egui_app.ui(self.renderer.context());

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [
                self.width.saturating_mul(self.physical_scale()),
                self.height.saturating_mul(self.physical_scale()),
            ],
            pixels_per_point: self.physical_scale() as f32,
        };

        let platform_output = self.renderer.end_frame_and_draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &texture_view,
            screen_descriptor,
        );

        for command in &platform_output.commands {
            self.input_state.handle_output_command(command);
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // Only request next frame if there are events
        if !platform_output.events.is_empty() {
            self.wl_surface
                .frame(&self.queue_handle, self.wl_surface.clone());
            self.wl_surface.commit();
        }

        platform_output
    }

    fn reconfigure_surface(&mut self) {
        let width = self.width.saturating_mul(self.physical_scale()).max(1);
        let height = self.height.saturating_mul(self.physical_scale()).max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.output_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![self.output_format],
            desired_maximum_frame_latency: 2,
        };
        self.surface.configure(&self.device, &config);
        self.surface_config = Some(config);
    }

    fn physical_scale(&self) -> u32 {
        self.scale_factor.max(1) as u32
    }
}

/// EGUI View Manager - manages all EGUI surfaces using the ViewManager pattern
pub struct EguiViewManager<A: EguiAppData> {
    view_manager: ViewManager<EguiSurfaceState<A>>,
    keyboard_focused_surface: Option<ObjectId>,
}

impl<A: EguiAppData> EguiViewManager<A> {
    pub fn new() -> Self {
        Self {
            view_manager: ViewManager::new(),
            keyboard_focused_surface: None,
        }
    }

    /// Handle Wayland events and update surfaces accordingly
    /// Returns an optional cursor shape change
    pub fn handle_events(&mut self, app: &mut Application, events: &[WaylandEvent]) {
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

                    if let Some(surface_state) = self
                        .view_manager
                        .get_data_by_id_mut(&window.wl_surface().id())
                    {
                        surface_state.configure(app, width, height);
                    }
                }
                WaylandEvent::LayerShellConfigure(layer_surface, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;

                    if let Some(surface_state) = self
                        .view_manager
                        .get_data_by_id_mut(&layer_surface.wl_surface().id())
                    {
                        surface_state.configure(app, width, height);
                    }
                }
                WaylandEvent::PopupConfigure(popup, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;

                    if let Some(surface_state) = self
                        .view_manager
                        .get_data_by_id_mut(&popup.wl_surface().id())
                    {
                        surface_state.configure(app, width, height);
                    }
                }
                WaylandEvent::Frame(surface, time) => {
                    if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&surface.id())
                    {
                        surface_state.frame(*time);
                    }
                }
                WaylandEvent::ScaleFactorChanged(surface, factor) => {
                    if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&surface.id())
                    {
                        surface_state.scale_factor_changed(*factor);
                    }
                }
                WaylandEvent::PointerEvent(events) => {
                    for (surface, _pos, event_kind) in events {
                        if let Some(surface_state) =
                            self.view_manager.get_data_by_id_mut(&surface.id())
                        {
                            let pointer_event = PointerEvent {
                                surface: surface.clone(),
                                position: _pos.clone(),
                                kind: event_kind.clone(),
                            };
                            if let Some(cursor_shape) =
                                surface_state.handle_pointer_event(&pointer_event)
                            {
                                app.set_cursor(cursor_shape);
                            }
                        }
                    }
                }
                WaylandEvent::KeyboardEnter(surface, _serials, _keysyms) => {
                    self.keyboard_focused_surface = Some(surface.id());
                    if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&surface.id())
                    {
                        surface_state.handle_keyboard_enter();
                    }
                }
                WaylandEvent::KeyboardLeave(surface) => {
                    if self.keyboard_focused_surface == Some(surface.id()) {
                        self.keyboard_focused_surface = None;
                    }
                    if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&surface.id())
                    {
                        surface_state.handle_keyboard_leave();
                    }
                }
                WaylandEvent::KeyPress(key_event) => {
                    if let Some(id) = self.keyboard_focused_surface.clone() {
                        if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&id) {
                            surface_state.handle_keyboard_event(key_event, true, false);
                        }
                    }
                }
                WaylandEvent::KeyRelease(key_event) => {
                    if let Some(id) = self.keyboard_focused_surface.clone() {
                        if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&id) {
                            surface_state.handle_keyboard_event(key_event, false, false);
                        }
                    }
                }
                WaylandEvent::KeyRepeat(key_event) => {
                    if let Some(id) = self.keyboard_focused_surface.clone() {
                        if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&id) {
                            surface_state.handle_keyboard_event(key_event, true, true);
                        }
                    }
                }
                WaylandEvent::ModifiersChanged(modifiers) => {
                    // Update modifiers for focused surface only
                    if let Some(id) = self.keyboard_focused_surface.clone() {
                        if let Some(surface_state) = self.view_manager.get_data_by_id_mut(&id) {
                            surface_state.update_modifiers(modifiers);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Add a window surface with EGUI app
    pub fn add_window(
        &mut self,
        app: &Application,
        window: Window,
        egui_app: A,
        width: u32,
        height: u32,
    ) {
        let mut surface_state = EguiSurfaceState::new(app, window.wl_surface().clone(), egui_app);
        surface_state.width = width;
        surface_state.height = height;
        self.view_manager.push(&window, surface_state);
    }

    /// Add a layer surface with EGUI app
    pub fn add_layer_surface(
        &mut self,
        app: &Application,
        layer_surface: LayerSurface,
        egui_app: A,
        width: u32,
        height: u32,
    ) {
        let mut surface_state =
            EguiSurfaceState::new(app, layer_surface.wl_surface().clone(), egui_app);
        surface_state.width = width;
        surface_state.height = height;
        self.view_manager.push(&layer_surface, surface_state);
    }

    /// Add a popup surface with EGUI app
    pub fn add_popup(
        &mut self,
        app: &Application,
        popup: Popup,
        egui_app: A,
        width: u32,
        height: u32,
    ) {
        let mut surface_state = EguiSurfaceState::new(app, popup.wl_surface().clone(), egui_app);
        surface_state.width = width;
        surface_state.height = height;
        self.view_manager.push(&popup, surface_state);
    }
}

impl<A: EguiAppData> Default for EguiViewManager<A> {
    fn default() -> Self {
        Self::new()
    }
}

// Helper functions

fn wayland_button_to_egui(button: u32) -> Option<PointerButton> {
    match button {
        0x110 => Some(PointerButton::Primary),
        0x111 => Some(PointerButton::Secondary),
        0x112 => Some(PointerButton::Middle),
        _ => None,
    }
}

fn keysym_to_egui_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        Keysym::downarrow | Keysym::Down => Key::ArrowDown,
        Keysym::leftarrow | Keysym::Left => Key::ArrowLeft,
        Keysym::rightarrow | Keysym::Right => Key::ArrowRight,
        Keysym::uparrow | Keysym::Up => Key::ArrowUp,
        Keysym::Escape => Key::Escape,
        Keysym::Tab => Key::Tab,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Return => Key::Enter,
        Keysym::Insert => Key::Insert,
        Keysym::Delete => Key::Delete,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Prior => Key::PageUp,
        Keysym::Next => Key::PageDown,
        Keysym::space => Key::Space,
        Keysym::colon => Key::Colon,
        Keysym::comma => Key::Comma,
        Keysym::minus => Key::Minus,
        Keysym::period => Key::Period,
        Keysym::plus => Key::Plus,
        Keysym::equal => Key::Equals,
        Keysym::semicolon => Key::Semicolon,
        Keysym::bracketleft => Key::OpenBracket,
        Keysym::bracketright => Key::CloseBracket,
        Keysym::grave => Key::Backtick,
        Keysym::backslash => Key::Backslash,
        Keysym::slash => Key::Slash,
        Keysym::bar => Key::Pipe,
        Keysym::question => Key::Questionmark,
        Keysym::apostrophe => Key::Quote,
        Keysym::_0 => Key::Num0,
        Keysym::_1 => Key::Num1,
        Keysym::_2 => Key::Num2,
        Keysym::_3 => Key::Num3,
        Keysym::_4 => Key::Num4,
        Keysym::_5 => Key::Num5,
        Keysym::_6 => Key::Num6,
        Keysym::_7 => Key::Num7,
        Keysym::_8 => Key::Num8,
        Keysym::_9 => Key::Num9,
        Keysym::a => Key::A,
        Keysym::b => Key::B,
        Keysym::c => Key::C,
        Keysym::d => Key::D,
        Keysym::e => Key::E,
        Keysym::f => Key::F,
        Keysym::g => Key::G,
        Keysym::h => Key::H,
        Keysym::i => Key::I,
        Keysym::j => Key::J,
        Keysym::k => Key::K,
        Keysym::l => Key::L,
        Keysym::m => Key::M,
        Keysym::n => Key::N,
        Keysym::o => Key::O,
        Keysym::p => Key::P,
        Keysym::q => Key::Q,
        Keysym::r => Key::R,
        Keysym::s => Key::S,
        Keysym::t => Key::T,
        Keysym::u => Key::U,
        Keysym::v => Key::V,
        Keysym::w => Key::W,
        Keysym::x => Key::X,
        Keysym::y => Key::Y,
        Keysym::z => Key::Z,
        Keysym::F1 => Key::F1,
        Keysym::F2 => Key::F2,
        Keysym::F3 => Key::F3,
        Keysym::F4 => Key::F4,
        Keysym::F5 => Key::F5,
        Keysym::F6 => Key::F6,
        Keysym::F7 => Key::F7,
        Keysym::F8 => Key::F8,
        Keysym::F9 => Key::F9,
        Keysym::F10 => Key::F10,
        Keysym::F11 => Key::F11,
        Keysym::F12 => Key::F12,
        Keysym::F13 => Key::F13,
        Keysym::F14 => Key::F14,
        Keysym::F15 => Key::F15,
        Keysym::F16 => Key::F16,
        Keysym::F17 => Key::F17,
        Keysym::F18 => Key::F18,
        Keysym::F19 => Key::F19,
        Keysym::F20 => Key::F20,
        _ => return None,
    })
}

fn egui_to_cursor_shape(cursor: egui::CursorIcon) -> Shape {
    use Shape as C;
    use egui::CursorIcon::*;

    match cursor {
        Default => C::Default,
        None => C::Default,
        ContextMenu => C::ContextMenu,
        Help => C::Help,
        PointingHand => C::Pointer,
        Progress => C::Progress,
        Wait => C::Wait,
        Cell => C::Cell,
        Crosshair => C::Crosshair,
        Text => C::Text,
        VerticalText => C::VerticalText,
        Alias => C::Alias,
        Copy => C::Copy,
        Move => C::Move,
        NoDrop => C::NoDrop,
        NotAllowed => C::NotAllowed,
        Grab => C::Grab,
        Grabbing => C::Grabbing,
        AllScroll => C::AllScroll,
        ResizeHorizontal => C::EwResize,
        ResizeNeSw => C::NeswResize,
        ResizeNwSe => C::NwseResize,
        ResizeVertical => C::NsResize,
        ResizeEast => C::EResize,
        ResizeSouthEast => C::SeResize,
        ResizeSouth => C::SResize,
        ResizeSouthWest => C::SwResize,
        ResizeWest => C::WResize,
        ResizeNorthWest => C::NwResize,
        ResizeNorth => C::NResize,
        ResizeNorthEast => C::NeResize,
        ResizeColumn => C::ColResize,
        ResizeRow => C::RowResize,
        ZoomIn => C::ZoomIn,
        ZoomOut => C::ZoomOut,
    }
}
