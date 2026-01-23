use crate::Application;
use crate::EguiWgpuRenderer;
use egui::Context;
use wayland_client::Connection;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;

enum EguiWgpuRendererThreadCommand {
    Render {
        fulloutput: egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    },
    ReconfigureSurface {
        width: u32,
        height: u32,
    },
    RequestFrame,
}

pub struct EguiWgpuRendererThread {
    #[allow(dead_code)]
    thread: std::thread::JoinHandle<()>,
    tx: std::sync::mpsc::SyncSender<EguiWgpuRendererThreadCommand>,
}

impl EguiWgpuRendererThread {
    pub fn new(
        egui_context: &Context,
        wl_surface: &WlSurface,
        qh: &QueueHandle<Application>,
        conn: &Connection,
    ) -> EguiWgpuRendererThread {
        let (tx, rx) = std::sync::mpsc::sync_channel(9999);

        // Moved to thread:
        let wl_surface_ = wl_surface.clone();
        let qh_ = qh.clone();
        let conn_ = conn.clone();
        let egui_context = egui_context.clone();
        let thread = std::thread::spawn(move || {
            let mut renderer = EguiWgpuRenderer::new(&egui_context, &wl_surface_, &qh_, &conn_);

            loop {
                // Block until next command arrives
                if let Ok(command) = rx.recv() {
                    match command {
                        EguiWgpuRendererThreadCommand::Render {
                            fulloutput: full_output,
                            width,
                            height,
                            pixels_per_point,
                        } => {
                            // Render if we have render input available
                            renderer.render_to_wgpu(full_output, width, height, pixels_per_point);
                        }
                        EguiWgpuRendererThreadCommand::ReconfigureSurface { width, height } => {
                            renderer.reconfigure_surface(width, height);
                        }
                        EguiWgpuRendererThreadCommand::RequestFrame => {
                            renderer.request_frame();
                        }
                    }
                }
            }
        });

        EguiWgpuRendererThread { thread, tx }
    }

    /// Resize and reconfigure the WGPU surface
    pub fn reconfigure_surface(&mut self, width: u32, height: u32) {
        let _ = self
            .tx
            .send(EguiWgpuRendererThreadCommand::ReconfigureSurface { width, height });
    }

    /// Renders EGUI output to the WGPU surface
    pub fn render_to_wgpu(
        &mut self,
        egui_fulloutput: egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) {
        let _ = self.tx.send(EguiWgpuRendererThreadCommand::Render {
            fulloutput: egui_fulloutput,
            width,
            height,
            pixels_per_point,
        });
    }

    /// Request frame callback and commit (must be called after render
    /// completes) This is thread-safe as it queues the operation in the
    /// renderer thread
    pub fn request_frame(&mut self) {
        let _ = self.tx.send(EguiWgpuRendererThreadCommand::RequestFrame);
    }
}
