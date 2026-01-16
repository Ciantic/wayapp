//! EGUI view manager implementation
//!
//! This module provides a ViewManager-based approach to handling EGUI surfaces
//! following the pattern from single_color.rs

#![allow(dead_code)]

use crate::Application;
use crate::EguiWgpuRenderer;
use crate::Kind;
use crate::WaylandEvent;
use crate::WaylandToEguiInput;
use crate::egui_to_cursor_shape;
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
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr::NonNull;
use std::time::Duration;
use std::time::Instant;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

/// Trait that applications must implement to provide EGUI UI
pub trait EguiAppData {
    fn ui(&mut self, ctx: &egui::Context);
}

/// Surface-specific EGUI state
pub struct EguiSurfaceState<T: Into<Kind> + Clone> {
    viewport: Option<WpViewport>,
    t: T,
    kind: Kind,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: EguiWgpuRenderer,
    input_state: WaylandToEguiInput,
    queue_handle: QueueHandle<Application>,
    init_width: u32,
    init_height: u32,
    width: u32,  // WGPU Surface width in logical pixels
    height: u32, // WGPU Surface height in logical pixels
    scale_factor: i32,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    output_format: wgpu::TextureFormat,
    last_buffer_update: Option<Instant>,
    has_keyboard_focus: bool,
}

impl<T: Into<Kind> + Clone> EguiSurfaceState<T> {
    pub fn new(app: &Application, t: T, width: u32, height: u32) -> Self {
        let kind = t.clone().into();
        let wl_surface = kind.get_wl_surface();
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

        let renderer = EguiWgpuRenderer::new(&device, output_format, None, 1);
        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = WaylandToEguiInput::new(clipboard);

        Self {
            viewport: None,
            t,
            kind,
            surface,
            device,
            queue,
            renderer,
            input_state,
            queue_handle: app.qh.clone(),
            init_height: height,
            init_width: width,
            width,
            height,
            scale_factor: 1,
            surface_config: None,
            output_format,
            last_buffer_update: None,
            has_keyboard_focus: false,
        }
    }

    pub fn wl_surface(&self) -> &WlSurface {
        self.kind.get_wl_surface()
    }

    fn configure(&mut self, app: &Application, width: u32, height: u32) {
        trace!(
            "Configuring EGUI surface {} to {}x{}",
            self.wl_surface().id(),
            width,
            height
        );
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
            self.wl_surface().commit();
        }
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

    fn update_buffers(&mut self, width: u32, height: u32) {
        trace!(
            "Updating EGUI surface {} buffers to {}x{}",
            self.wl_surface().id(),
            width,
            height
        );
        self.width = width.max(1);
        self.height = height.max(1);
        self.input_state.set_screen_size(self.width, self.height);
        self.reconfigure_surface();
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
        self.reconfigure_surface();
    }

    pub fn request_frame(&mut self) {
        let wl_surface = self.wl_surface();
        wl_surface.frame(&self.queue_handle, wl_surface.clone());
        wl_surface.commit();
    }

    fn render(&mut self, ui: &mut impl EguiAppData) -> PlatformOutput {
        // trace!("Rendering EGUI surface {}", self.wl_surface().id());
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
        ui.ui(self.renderer.context());

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
            let wl_surface = self.wl_surface();
            wl_surface.frame(&self.queue_handle, wl_surface.clone());
            wl_surface.commit();
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

    /// Handle Wayland events and update surfaces accordingly
    /// Returns an optional cursor shape change
    pub fn handle_events(
        &mut self,
        app: &mut Application,
        events: &[WaylandEvent],
        ui: &mut impl EguiAppData,
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

                    self.configure(app, width, height);
                    self.render(ui);
                }
                WaylandEvent::LayerShellConfigure(_, config) => {
                    let width = config.new_size.0;
                    let height = config.new_size.1;

                    self.configure(app, width, height);
                    self.render(ui);
                }
                WaylandEvent::PopupConfigure(_, config) => {
                    let width = config.width as u32;
                    let height = config.height as u32;

                    self.configure(app, width, height);
                    self.render(ui);
                }
                WaylandEvent::Frame(_, _) => {
                    let output = self.render(ui);
                    app.set_cursor(egui_to_cursor_shape(output.cursor_icon));
                }
                WaylandEvent::ScaleFactorChanged(_, factor) => {
                    self.scale_factor_changed(*factor);
                    self.request_frame();
                    let _ = app.conn.flush();
                }
                WaylandEvent::PointerEvent((surface, position, event_kind)) => {
                    self.handle_pointer_event(&PointerEvent {
                        surface: surface.clone(),
                        position: position.clone(),
                        kind: event_kind.clone(),
                    });
                    self.request_frame();
                    let _ = app.conn.flush();
                }
                WaylandEvent::KeyboardEnter(_, _serials, _keysyms) => {
                    self.handle_keyboard_enter();
                    self.request_frame();
                    let _ = app.conn.flush();
                    self.has_keyboard_focus = true;
                }
                WaylandEvent::KeyboardLeave(_) => {
                    self.handle_keyboard_leave();
                    self.request_frame();
                    let _ = app.conn.flush();
                    self.has_keyboard_focus = false;
                }
                WaylandEvent::KeyPress(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, true, false);
                        self.request_frame();
                        let _ = app.conn.flush();
                    }
                }
                WaylandEvent::KeyRelease(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, false, false);
                        self.request_frame();
                        let _ = app.conn.flush();
                    }
                }
                WaylandEvent::KeyRepeat(key_event) => {
                    if self.has_keyboard_focus {
                        self.handle_keyboard_event(key_event, true, true);
                        self.request_frame();
                        let _ = app.conn.flush();
                    }
                }
                WaylandEvent::ModifiersChanged(modifiers) => {
                    self.update_modifiers(modifiers);
                    self.request_frame();
                    let _ = app.conn.flush();
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
