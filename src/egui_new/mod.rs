#![allow(unused_imports)]

mod egui_input_handler;
mod egui_view_manager;
mod egui_wgpu_renderer;
use egui::PlatformOutput;
use egui::RawInput;
pub use egui_input_handler::*;
pub use egui_view_manager::*;
use egui_wgpu::Renderer;
use egui_wgpu::RendererOptions;
use egui_wgpu::ScreenDescriptor;
pub use egui_wgpu_renderer::*;

/// EGUI WGPU Renderer wrapper
struct EguiWgpuRenderer {
    context: egui::Context,
    renderer: Renderer,
    frame_started: bool,
}

impl EguiWgpuRenderer {
    fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat, msaa_samples: u32) -> Self {
        let egui_context = egui::Context::default();
        let egui_renderer = Renderer::new(
            device,
            output_format,
            RendererOptions {
                msaa_samples,
                depth_stencil_format: None,
                ..Default::default()
            },
        );

        Self {
            context: egui_context,
            renderer: egui_renderer,
            frame_started: false,
        }
    }

    fn context(&self) -> &egui::Context {
        &self.context
    }

    fn begin_frame(&mut self, raw_input: RawInput) {
        self.context.begin_pass(raw_input);
        self.frame_started = true;
    }

    fn end_frame_and_draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window_surface_view: &wgpu::TextureView,
        screen_descriptor: ScreenDescriptor,
    ) -> PlatformOutput {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw!");
        }

        self.context
            .set_pixels_per_point(screen_descriptor.pixels_per_point);
        let full_output = self.context.end_pass();

        let tris = self
            .context
            .tessellate(full_output.shapes, self.context.pixels_per_point());

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }

        self.renderer
            .update_buffers(device, queue, encoder, &tris, &screen_descriptor);

        let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            label: Some("egui main render pass"),
            occlusion_query_set: None,
        });

        self.renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);

        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x);
        }

        self.frame_started = false;
        full_output.platform_output
    }
}
