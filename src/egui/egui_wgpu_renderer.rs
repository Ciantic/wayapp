use egui::Context;
use egui_wgpu::Renderer;
use egui_wgpu::RendererOptions;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::Device;
use egui_wgpu::wgpu::Queue;
use egui_wgpu::wgpu::StoreOp;
use egui_wgpu::wgpu::Surface;
use egui_wgpu::wgpu::SurfaceConfiguration;
use egui_wgpu::wgpu::TextureFormat;
use raw_window_handle::RawDisplayHandle;
use raw_window_handle::RawWindowHandle;
use raw_window_handle::WaylandDisplayHandle;
use raw_window_handle::WaylandWindowHandle;
use std::ptr::NonNull;
use wayland_client::Connection;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;

pub struct EguiWgpuRenderer {
    egui_context: Context,
    renderer: Renderer,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    surface_config: Option<SurfaceConfiguration>,
    output_format: TextureFormat,
    width: u32,
    height: u32,
}

impl EguiWgpuRenderer {
    pub fn new(
        egui_context: &Context,
        wl_surface: &WlSurface,
        conn: &Connection,
    ) -> EguiWgpuRenderer {
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr() as *mut _)
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

        let adapter =
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .expect("Failed to find a suitable adapter");

        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }))
            .expect("Failed to request WGPU device");

        let caps = surface.get_capabilities(&adapter);
        let output_format = *caps
            .formats
            .get(0)
            .unwrap_or(&wgpu::TextureFormat::Bgra8Unorm);

        let egui_renderer = Renderer::new(
            &device,
            output_format,
            RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                ..Default::default()
            },
        );

        // Enabling set_request_repaint_callback would require a way to throttle the
        // repaint let wl_surface_ = wl_surface.clone();
        // let qh_ = qh.clone();
        // let conn_ = conn.clone();
        // egui_context.set_request_repaint_callback(move |_info| {
        //     // if _info.delay == Duration::ZERO {
        //     wl_surface_.frame(&qh_, wl_surface_.clone());
        //     wl_surface_.commit();
        //     conn_.flush().unwrap();
        //     // }
        // });

        EguiWgpuRenderer {
            renderer: egui_renderer,
            surface,
            device,
            queue,
            surface_config: None,
            output_format,
            width: 0,
            height: 0,
            egui_context: egui_context.clone(),
        }
    }

    /// Resize and reconfigure the WGPU surface
    pub fn reconfigure_surface(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        self.width = width;
        self.height = height;
        let config = SurfaceConfiguration {
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

    /// Renders EGUI output to the WGPU surface
    pub fn render_to_wgpu(
        &mut self,
        egui_fulloutput: egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) {
        // println!(
        //     "EGUI render_to_wgpu called with size {}x{} at {:?}",
        //     width,
        //     height,
        //     Instant::now()
        // );
        if (width != self.width) || (height != self.height) {
            println!(
                "Unexpected size change in EguiWgpuRenderer::render_to_wgpu, reconfiguring \
                 surface from {}x{} to {}x{}",
                self.width, self.height, width, height
            );
            self.reconfigure_surface(width, height);
        }

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(e) => {
                log::warn!("Failed to acquire surface texture: {:?}", e);
                return;
            }
        };

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

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };

        // Draw EGUI shapes with WGPU
        let tris = self
            .egui_context
            .tessellate(egui_fulloutput.shapes, egui_fulloutput.pixels_per_point);
        for (id, image_delta) in &egui_fulloutput.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &tris,
            &screen_descriptor,
        );
        let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: egui_wgpu::wgpu::Operations {
                    load: egui_wgpu::wgpu::LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            label: Some("egui main render pass"),
            occlusion_query_set: None,
        });

        self.renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        for x in &egui_fulloutput.textures_delta.free {
            self.renderer.free_texture(x)
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}
