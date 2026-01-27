use crate::Application;
use crate::EguiWgpuRenderer;
use egui::Context;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use wayland_client::Connection;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;

enum EguiWgpuRendererThreadCommand {
    Render,
    // ReconfigureSurface { width: u32, height: u32 },
    RequestFrame,
}

struct RenderInput {
    fulloutput: egui::FullOutput,
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
        egui_context: &Context,
        wl_surface: &WlSurface,
        _qh: &QueueHandle<Application>,
        conn: &Connection,
    ) -> EguiWgpuRendererThread {
        let (tx, rx) = std::sync::mpsc::sync_channel(9999);
        let render_input = Arc::new(Mutex::new(None));
        let tx_ = tx.clone();
        egui_context.set_request_repaint_callback(move |_info| {
            if _info.delay == Duration::ZERO {
                let _ = tx_.send(EguiWgpuRendererThreadCommand::RequestFrame);
            } else {
                // This is not implemented yet
            }
        });

        // Moved to thread:
        let wl_surface_ = wl_surface.clone();
        let conn_ = conn.clone();
        let egui_context = egui_context.clone();
        let render_input_ = render_input.clone();
        let thread = std::thread::spawn(move || {
            let mut renderer = EguiWgpuRenderer::new(&egui_context, &wl_surface_, &conn_);
            let mut last_render = std::time::Instant::now()
                .checked_sub(Duration::from_secs(3))
                .unwrap();
            loop {
                // Block until next command arrives
                if let Ok(command) = rx.recv() {
                    match command {
                        EguiWgpuRendererThreadCommand::Render => {
                            if last_render.elapsed().as_millis() < 16 {
                                // Limit to ~60 FPS
                                println!("Skipping frame to limit FPS");
                                // This is not exactly working, because it should schedule the next
                                // frame if render calls stop.
                                continue;
                            }

                            // Get render input
                            let render_input = {
                                if let Ok(mut guard) = render_input_.lock() {
                                    guard.take()
                                } else {
                                    None
                                }
                            };

                            // Render if we have render input available
                            if let Some(RenderInput {
                                fulloutput,
                                width,
                                height,
                                pixels_per_point,
                            }) = render_input
                            {
                                renderer.render_to_wgpu(
                                    fulloutput,
                                    width,
                                    height,
                                    pixels_per_point,
                                );
                                last_render = std::time::Instant::now();
                            }
                        }
                        // EguiWgpuRendererThreadCommand::ReconfigureSurface { .. } => {
                        //     // renderer.reconfigure_surface(width, height);
                        // }
                        EguiWgpuRendererThreadCommand::RequestFrame => {
                            // renderer.request_frame();
                        }
                    }
                }
            }
        });

        EguiWgpuRendererThread {
            thread,
            tx,
            render_input,
        }
    }

    /// Resize and reconfigure the WGPU surface
    pub fn reconfigure_surface(&mut self, _width: u32, _height: u32) {
        // let _ = self
        //     .tx
        //     .send(EguiWgpuRendererThreadCommand::ReconfigureSurface { width,
        // height });
    }

    /// Renders EGUI output to the WGPU surface
    pub fn render_to_wgpu(
        &mut self,
        egui_fulloutput: egui::FullOutput,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) {
        // If skipping frame rendering is implemented, then fulloutputs need to be
        // combined with append:
        //
        // older_egui_fulloutput.append(egui_fulloutput);

        // Append to any existing render input
        if let Ok(mut render_input) = self.render_input.lock() {
            if let Some(input) = render_input.as_mut() {
                input.fulloutput.append(egui_fulloutput.clone());
            } else {
                *render_input = Some(RenderInput {
                    fulloutput: egui_fulloutput.clone(),
                    width,
                    height,
                    pixels_per_point,
                });
            }
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
