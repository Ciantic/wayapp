use crate::Application;
use crate::EguiWgpuRenderer;
use egui::Context;
use std::sync::Arc;
use std::sync::Mutex;
use wayland_client::Connection;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;

#[derive(Debug)]
enum EguiWgpuRendererThreadCommand {
    Render,
    ReconfigureSurface { width: u32, height: u32 },
    RequestFrame,
}

struct RenderInput {
    egui_fulloutput: egui::FullOutput,
    egui_context: Context,
    width: u32,
    height: u32,
    pixels_per_point: f32,
}

pub struct EguiWgpuRendererThread {
    #[allow(dead_code)]
    thread: std::thread::JoinHandle<()>,
    render_input: Arc<Mutex<Option<RenderInput>>>,
    tx: std::sync::mpsc::SyncSender<EguiWgpuRendererThreadCommand>,
}

impl EguiWgpuRendererThread {
    pub fn new(
        wl_surface: &WlSurface,
        qh: &QueueHandle<Application>,
        conn: &Connection,
    ) -> EguiWgpuRendererThread {
        let render_input: Arc<Mutex<Option<RenderInput>>> = Arc::new(Mutex::new(None));

        let (tx, rx) = std::sync::mpsc::sync_channel(9999);

        // Moved to thread:
        let render_input_ = Arc::clone(&render_input);
        let wl_surface_ = wl_surface.clone();
        let qh_ = qh.clone();
        let conn_ = conn.clone();
        let thread = std::thread::spawn(move || {
            let mut renderer = EguiWgpuRenderer::new(&wl_surface_, &qh_, &conn_);

            loop {
                // Block until next command arrives
                if let Ok(command) = rx.recv() {
                    match command {
                        EguiWgpuRendererThreadCommand::Render => {
                            // Render if we have render input available
                            if let Ok(mut input_opt) = render_input_.lock() {
                                if let Some(input) = input_opt.take() {
                                    drop(input_opt);
                                    renderer.render_to_wgpu(
                                        input.egui_fulloutput,
                                        &input.egui_context,
                                        input.width,
                                        input.height,
                                        input.pixels_per_point,
                                    );
                                }
                            }
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

        EguiWgpuRendererThread {
            thread,
            render_input,
            tx,
        }
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
        egui_context: &Context,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) {
        if let Ok(mut input_opt) = self.render_input.lock() {
            *input_opt = Some(RenderInput {
                egui_fulloutput,
                egui_context: egui_context.clone(),
                width,
                height,
                pixels_per_point,
            });
        }
        let _ = self.tx.send(EguiWgpuRendererThreadCommand::Render);
    }

    /// Request frame callback and commit (must be called after render
    /// completes) This is thread-safe as it queues the operation in the
    /// renderer thread
    pub fn request_frame(&mut self) {
        let _ = self.tx.send(EguiWgpuRendererThreadCommand::RequestFrame);
    }
}
