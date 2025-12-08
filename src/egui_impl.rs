use log::trace;
use pollster::block_on;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use smithay_clipboard::Clipboard;
use smithay_client_toolkit::{
    seat::{keyboard::{KeyEvent, Modifiers}, pointer::PointerEvent},
    shell::{WaylandSurface, xdg::window::{Window, WindowConfigure}},
};
use std::ptr::NonNull;
use wayland_client::{Proxy, QueueHandle, protocol::wl_surface::WlSurface};

use crate::{
    Application, BaseTrait, CompositorHandlerContainer, EguiRenderer, InputState,
    KeyboardHandlerContainer, PointerHandlerContainer, WindowContainer,
};

pub trait EguiAppData {
    fn ui(&mut self, ctx: &egui::Context);
}


struct EguiSurfaceState<A: EguiAppData> {
    wl_surface: WlSurface,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: EguiRenderer,
    egui_app: A,
    input_state: InputState,
    queue_handle: QueueHandle<Application>,
    width: u32,
    height: u32,
    scale_factor: i32,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    output_format: wgpu::TextureFormat,
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
        }, ))
        .expect("Failed to request WGPU device");

        let caps = surface.get_capabilities(&adapter);
        let output_format = *caps
            .formats
            .get(0)
            .unwrap_or(&wgpu::TextureFormat::Bgra8Unorm);

        let renderer = EguiRenderer::new(&device, output_format, None, 1);
        let clipboard = unsafe { Clipboard::new(app.conn.display().id().as_ptr() as *mut _) };
        let input_state = InputState::new(clipboard);

        Self {
            wl_surface,
            instance,
            surface,
            adapter,
            device,
            queue,
            renderer,
            egui_app,
            input_state,
            queue_handle: app.qh.clone(),
            width: 1,
            height: 1,
            scale_factor: 1,
            surface_config: None,
            output_format,
        }
    }

    fn configure(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.input_state.set_screen_size(self.width, self.height);
        self.reconfigure_surface();
        // Render immediately to attach a buffer to the surface so Wayland will send frame callbacks
        self.render();
    }

    fn frame(&mut self, _time: u32) {
        self.render();
    }

    fn handle_pointer_event(&mut self, event: &PointerEvent) {
        self.input_state.handle_pointer_event(event);
        self.needs_frame();
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        self.input_state.handle_keyboard_event(event, pressed, repeat);
        self.needs_frame();
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.input_state.update_modifiers(modifiers);
    }

    fn scale_factor_changed(&mut self, new_factor: i32) {
        let factor = new_factor.max(1);
        if factor == self.scale_factor {
            return;
        }
        self.scale_factor = factor;
        self.reconfigure_surface();
        self.needs_frame();
    }

    fn needs_frame(&self) {
        if self.surface_config.is_none() {
            return;
        }
        self.request_frame();
    }

    fn render(&mut self) {
        if self.surface_config.is_none() {
            return;
        }

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(_) => {
                return;
            }
        };

        let texture_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
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

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
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

        // Always request the next frame to keep rendering
        self.request_frame();
    }

    fn reconfigure_surface(&mut self) {
        let config = self.create_surface_config();
        self.surface.configure(&self.device, &config);
        self.surface_config = Some(config);
    }

    fn request_frame(&self) {
        if self.surface_config.is_none() {
            return;
        }
        trace!("[EGUI] Calling wl_surface.frame and commit");
        let callback = self.wl_surface.clone();
        self.wl_surface.frame(&self.queue_handle, callback);
        self.wl_surface.commit();
    }

    fn create_surface_config(&self) -> wgpu::SurfaceConfiguration {
        let width = self.width.saturating_mul(self.physical_scale()).max(1);
        let height = self.height.saturating_mul(self.physical_scale()).max(1);
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.output_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![self.output_format],
            desired_maximum_frame_latency: 2,
        }
    }

    fn physical_scale(&self) -> u32 {
        self.scale_factor.max(1) as u32
    }
}

pub struct EguiWindow<A: EguiAppData> {
    pub window: Window,
    surface: EguiSurfaceState<A>,
}

impl<A: EguiAppData> EguiWindow<A> {
    pub fn new(app: &Application, window: Window, egui_app: A) -> Self {
        let surface = EguiSurfaceState::new(app, window.wl_surface().clone(), egui_app);
        Self { window, surface }
    }
}

impl<A: EguiAppData> CompositorHandlerContainer for EguiWindow<A> {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.window.wl_surface().set_buffer_scale(new_factor);
        self.surface.scale_factor_changed(new_factor);
    }

    fn frame(&mut self, time: u32) {
        self.surface.frame(time);
    }
}

impl<A: EguiAppData> KeyboardHandlerContainer for EguiWindow<A> {
    fn press_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, false);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, false, false);
    }

    fn update_modifiers(&mut self, modifiers: &Modifiers) {
        self.surface.update_modifiers(modifiers);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        self.surface.handle_keyboard_event(event, true, true);
    }
}

impl<A: EguiAppData> PointerHandlerContainer for EguiWindow<A> {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        self.surface.handle_pointer_event(event);
    }
}

impl<A: EguiAppData> BaseTrait for EguiWindow<A> {}

impl<A: EguiAppData> WindowContainer for EguiWindow<A> {
    fn configure(&mut self, configure: &WindowConfigure) {
        let width = configure.new_size.0.map_or(256, |size| size.get());
        let height = configure.new_size.1.map_or(256, |size| size.get());
        self.window
            .wl_surface()
            .set_buffer_scale(self.surface.scale_factor);
        self.surface.configure(width, height);
    }

    fn get_window(&self) -> &Window {
        &self.window
    }
}