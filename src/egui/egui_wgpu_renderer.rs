// Original for Winit:
// https://github.com/kaphula/winit-egui-wgpu-template/blob/master/src/egui_tools.rs
//
// MIT License
// Copyright (c) 2024 Valtteri Vallius

use egui::Context;
use egui_wgpu::Renderer;
use egui_wgpu::RendererOptions;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::CommandEncoder;
use egui_wgpu::wgpu::Device;
use egui_wgpu::wgpu::Queue;
use egui_wgpu::wgpu::StoreOp;
use egui_wgpu::wgpu::TextureFormat;
use egui_wgpu::wgpu::TextureView;

pub struct EguiWgpuRenderer {
    renderer: Renderer,
}

impl EguiWgpuRenderer {
    pub fn new(
        device: &Device,
        output_color_format: TextureFormat,
        output_depth_format: Option<TextureFormat>,
        msaa_samples: u32,
    ) -> EguiWgpuRenderer {
        let egui_renderer = Renderer::new(
            device,
            output_color_format,
            RendererOptions {
                msaa_samples,
                depth_stencil_format: output_depth_format,

                ..Default::default()
            },
        );

        EguiWgpuRenderer {
            renderer: egui_renderer,
        }
    }

    /// Render the last processed frame to WGPU
    /// Call end_frame() before this
    pub fn render_to_wgpu(
        &mut self,
        egui_fulloutput: egui::FullOutput,
        egui_context: &Context,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) {
        // Draw EGUI shapes with WGPU
        let tris = egui_context.tessellate(egui_fulloutput.shapes, egui_context.pixels_per_point());
        for (id, image_delta) in &egui_fulloutput.textures_delta.set {
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
    }

    /*
    pub fn end_frame_and_draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) -> egui::PlatformOutput {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw can be called!");
        }

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
        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x)
        }

        self.frame_started = false;

        full_output.platform_output
    }
    */
}
