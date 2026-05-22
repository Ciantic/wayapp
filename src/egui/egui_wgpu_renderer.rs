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

// How this works:
// 1. `new()` creates a wgpu Instance, Device, Queue, and the egui Renderer. The
//    WlSurface + Connection are saved so the surface can be recreated.
// 2. `suspend()` drops only the wgpu Surface (the swapchain), freeing GPU
//    memory. The Device, Queue, and egui Renderer stay alive — dropping the
//    egui Renderer would lose texture state and panic on the next frame.
// 3. `resume()` recreates the wgpu Surface from the saved WlSurface, using the
//    *same* Instance (a surface from a new Instance can't find the Device).
//    Width/height are left at 0 to force a reconfigure on the next render.
// 4. `render_to_wgpu()` acquires the next swapchain image, clears it, then
//    draws the EGUI shapes / textures and presents. Skips silently if
//    suspended.

/// WGPU renderer for EGUI.
pub struct EguiWgpuRenderer {
    egui_context: Context,
    egui_renderer: Renderer,

    // Fields are dropped in declaration order. `wgpu_surface` must come before
    // `wgpu_device`, and `wgpu_device` before `wgpu_instance` — otherwise the
    // surface cleanup will panic trying to access a device that no longer
    // exists.
    wgpu_surface: Option<Surface<'static>>,
    wgpu_device: Device,
    wgpu_queue: Queue,
    wgpu_surface_config: Option<SurfaceConfiguration>,
    wgpu_instance: wgpu::Instance,
    output_format: TextureFormat,
    width: u32,
    height: u32,
    wl_surface: WlSurface,
    wl_conn: Connection,
}

impl EguiWgpuRenderer {
    pub fn new(
        egui_context: &Context,
        wl_surface: &WlSurface,
        conn: &Connection,
    ) -> EguiWgpuRenderer {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = Self::create_wgpu_surface(&instance, conn, wl_surface);

        let adapter =
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .expect("Failed to find a suitable adapter");

        let (wgpu_device, wgpu_queue) =
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
            &wgpu_device,
            output_format,
            RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                ..Default::default()
            },
        );

        EguiWgpuRenderer {
            egui_context: egui_context.clone(),
            egui_renderer,
            wgpu_surface: Some(surface),
            wgpu_device,
            wgpu_queue,
            wgpu_surface_config: None,
            output_format,
            width: 0,
            height: 0,
            wl_surface: wl_surface.clone(),
            wl_conn: conn.clone(),
            wgpu_instance: instance,
        }
    }

    /// Suspend the renderer — drops the WGPU surface and configuration to
    /// free GPU resources. The device, queue, and egui renderer are kept
    /// alive to preserve texture state.
    pub fn suspend(&mut self) {
        if self.wgpu_surface.is_some() {
            log::trace!("[EGUI] Suspending WGPU surface to free GPU resources");
            self.wgpu_surface = None;
            self.wgpu_surface_config = None;
            self.width = 0;
            self.height = 0;
        }
    }

    /// Resume the renderer — recreates the WGPU surface from the saved
    /// Wayland surface handle, using the same Instance that owns the Device.
    pub fn resume(&mut self) {
        if self.wgpu_surface.is_none() {
            log::trace!("[EGUI] Resuming WGPU surface");
            self.wgpu_surface = Some(Self::create_wgpu_surface(
                &self.wgpu_instance,
                &self.wl_conn,
                &self.wl_surface,
            ));
        }
    }

    /// Create a WGPU surface from Wayland connection and surface.
    fn create_wgpu_surface(
        instance: &wgpu::Instance,
        conn: &Connection,
        wl_surface: &WlSurface,
    ) -> Surface<'static> {
        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr() as *mut _)
                .expect("Wayland display pointer was null"),
        ));
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(wl_surface.id().as_ptr() as *mut _)
                .expect("Wayland surface handle was null"),
        ));
        // SAFETY: display_ptr and surface_id_ptr are valid Wayland pointers
        // borrowed from the WlSurface and Connection stored in
        // EguiWgpuRenderer, which outlive the returned Surface.
        //
        // We must use SurfaceTargetUnsafe::RawHandle because the safe
        // SurfaceTarget enum has no Wayland variant — DisplayAndWindow
        // requires HasDisplayHandle + HasWindowHandle, which Wayland
        // proxy types don't implement.
        unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .expect("Failed to create WGPU surface")
        }
    }

    /// Resize and reconfigure the WGPU surface
    fn reconfigure_surface(&mut self, width: u32, height: u32) {
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
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![self.output_format],
            desired_maximum_frame_latency: 2,
        };
        if let Some(surface) = &self.wgpu_surface {
            surface.configure(&self.wgpu_device, &config);
        }
        self.wgpu_surface_config = Some(config);
    }

    /// Acquire the next surface texture, retrying once after a reconfigure if
    /// the surface reports a suboptimal texture.
    fn acquire_surface_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
        {
            let surface = match &self.wgpu_surface {
                Some(surface) => surface,
                None => {
                    log::trace!("[EGUI] Skipping render, surface is suspended");
                    return None;
                }
            };

            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture) => return Some(texture),
                wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                    drop(texture);
                }
                status => {
                    log::warn!("Failed to acquire surface texture: {:?}", status);
                    return None;
                }
            }
        }

        self.reconfigure_surface(self.width, self.height);

        let surface = match self.wgpu_surface.as_ref() {
            Some(surface) => surface,
            None => {
                log::trace!("[EGUI] Surface disappeared during render retry");
                return None;
            }
        };

        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Some(texture),
            status => {
                log::warn!(
                    "Failed to acquire surface texture after reconfig: {:?}",
                    status
                );
                None
            }
        }
    }

    /// Renders EGUI output to the WGPU surface
    /// Returns silently if the surface is suspended.
    pub fn render_to_wgpu(
        &mut self,
        egui_fulloutput: egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) {
        // EGUI Screen descriptor for this frame
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };

        // Reconfigure if size changed (must happen before borrowing surface)
        let needs_reconfig = (width != self.width) || (height != self.height);
        if needs_reconfig {
            self.reconfigure_surface(width, height);
        }

        let surface_texture = match self.acquire_surface_texture() {
            Some(texture) => texture,
            None => {
                return;
            }
        };

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.wgpu_device.create_command_encoder(&Default::default());

        // Clear pass
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Draw EGUI shapes with WGPU
        let tris = self
            .egui_context
            .tessellate(egui_fulloutput.shapes, egui_fulloutput.pixels_per_point);

        for (id, image_delta) in &egui_fulloutput.textures_delta.set {
            self.egui_renderer.update_texture(
                &self.wgpu_device,
                &self.wgpu_queue,
                *id,
                image_delta,
            );
        }

        self.egui_renderer.update_buffers(
            &self.wgpu_device,
            &self.wgpu_queue,
            &mut encoder,
            &tris,
            &screen_descriptor,
        );

        // Render pass to draw EGUI output to the surface
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
            multiview_mask: None,
        });

        // Cleanup any textures marked for deletion by EGUI before rendering
        self.egui_renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        for x in &egui_fulloutput.textures_delta.free {
            self.egui_renderer.free_texture(x)
        }

        // Submit commands and present
        self.wgpu_queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}
