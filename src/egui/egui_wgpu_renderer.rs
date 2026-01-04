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
use std::time::Duration;

/// Output from rendering a frame, including repaint scheduling info
pub struct RenderOutput {
    pub platform_output: egui::PlatformOutput,
    /// How long to wait before next repaint. Duration::ZERO means repaint ASAP.
    /// Duration::MAX (or very large) means no repaint needed until next input
    /// event.
    pub repaint_delay: Duration,
}

pub struct EguiWgpuRenderer {
    context: Context,
    renderer: Renderer,
    frame_started: bool,
}

impl EguiWgpuRenderer {
    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    pub fn new(
        device: &Device,
        output_color_format: TextureFormat,
        output_depth_format: Option<TextureFormat>,
        msaa_samples: u32,
    ) -> EguiWgpuRenderer {
        let egui_context = Context::default();

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
            context: egui_context,
            renderer: egui_renderer,
            frame_started: false,
        }
    }

    pub fn ppp(&mut self, v: f32) {
        self.context.set_pixels_per_point(v);
    }

    pub fn begin_frame(&mut self, raw_input: egui::RawInput) {
        self.context.begin_pass(raw_input);
        self.frame_started = true;
    }

    pub fn end_frame_and_draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) -> RenderOutput {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw can be called!");
        }

        self.ppp(screen_descriptor.pixels_per_point);

        let full_output = self.context.end_pass();

        // Extract repaint delay from viewport output
        let repaint_delay = full_output
            .viewport_output
            .values()
            .next()
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::MAX);

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

        RenderOutput {
            platform_output: full_output.platform_output,
            repaint_delay,
        }
    }
}
