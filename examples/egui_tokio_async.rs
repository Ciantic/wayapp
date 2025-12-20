use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use wayapp::EguiAppData;
use wayapp::EguiLayerSurface;
use wayapp::EguiWindow;
use wayapp::RunApplication;
use wayapp::get_init_app;
use std::os::unix::io::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use tokio::io::unix::AsyncFd;
use futures::future::Either;
use futures::future::{poll_fn, select};
use std::pin::pin;

struct EguiApp {
    counter: i32,
    text: String,
}

impl Default for EguiApp {
    fn default() -> Self {
        Self {
            counter: 0,
            text: "Hello from EGUI!".into(),
        }
    }
}

impl EguiAppData for EguiApp {
    fn ui(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Egui WGPU / Smithay - Async Multi-Source");

            ui.separator();

            ui.label(format!("Counter: {}", self.counter));
            if ui.button("Increment").clicked() {
                self.counter += 1;
            }
            if ui.button("Decrement").clicked() {
                self.counter -= 1;
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Text input:");
                ui.text_edit_singleline(&mut self.text);
            });

            ui.label(format!("You wrote: {}", self.text));

            ui.separator();

            ui.label("This demonstrates async multi-source event handling!");
        });
    }
}

#[derive(Debug)]
enum AppEvent {
    TimerTick(u32),
    ExternalCommand(String),
    NetworkData(String),
}

// Helper struct to wrap the Wayland connection FD for async
struct WaylandFd(OwnedFd);

impl AsRawFd for WaylandFd {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.0.as_raw_fd()
    }
}

#[tokio::main]
async fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();
    let app = get_init_app();

    // Create example window
    let example_win_surface = app.compositor_state.create_surface(&app.qh);
    let example_window = app.xdg_shell.create_window(
        example_win_surface,
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Async Multi-Source Example");
    example_window.set_app_id("io.github.ciantic.wayapp.AsyncExample");
    example_window.set_min_size(Some((256, 256)));
    example_window.commit();

    let egui_app = EguiApp::default();
    app.push_window(EguiWindow::new(example_window, egui_app, 256, 256));

    // Create layer surface
    let shared_surface = app.compositor_state.create_surface(&app.qh);
    let layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        shared_surface.clone(),
        Layer::Top,
        Some("AsyncExample"),
        None,
    );
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    layer_surface.set_margin(0, 0, 20, 20);
    layer_surface.set_size(256, 256);
    layer_surface.commit();

    let egui_layer_surface = EguiLayerSurface::new(layer_surface, EguiApp::default(), 256, 256);
    app.push_layer_surface(egui_layer_surface);

    // Create channel for external events
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

    // Spawn background tasks that generate events
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut tick = 0u32;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            tick += 1;
            println!("[ASYNC TASK] Timer tick {} on thread {:?}", tick, std::thread::current().id());
            let _ = tx_clone.send(AppEvent::TimerTick(tick));
        }
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let _ = tx_clone.send(AppEvent::ExternalCommand("Hello from async task!".to_string()));
        
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let _ = tx_clone.send(AppEvent::NetworkData("Simulated network response".to_string()));
    });

    println!("[ASYNC MAIN] Starting async multi-source event loop on thread {:?}", std::thread::current().id());

    loop {
        match select(
            // TODO: This freezes for ~1 second, then continues, then freezes ~1s ...
            poll_fn(|cx| app.poll_dispatch_pending(cx)),
            pin!(rx.recv()),
        ).await {
            Either::Left((_, _)) => {
                println!("[ASYNC MAIN] ✓ Dispatched Wayland events on thread {:?}", std::thread::current().id());
            }
            Either::Right((Some(event), _)) => {
                match event {
                    AppEvent::TimerTick(tick) => {
                        println!("[ASYNC MAIN] ✓ Received timer tick: {} on thread {:?}", 
                            tick, std::thread::current().id());
                    }
                    AppEvent::ExternalCommand(cmd) => {
                        println!("[ASYNC MAIN] ✓ Received external command: '{}' on thread {:?}", 
                            cmd, std::thread::current().id());
                    }
                    AppEvent::NetworkData(data) => {
                        println!("[ASYNC MAIN] ✓ Received network data: '{}' on thread {:?}", 
                            data, std::thread::current().id());
                    }
                }
            },
            Either::Right((None, _)) => {
                // Channel closed, exit loop
                break;
            }
        }
    }
}
