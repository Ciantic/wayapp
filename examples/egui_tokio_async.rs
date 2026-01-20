use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use tokio::select;
use tokio::task::spawn_blocking;
use wayapp::*;

struct EguiApp {
    counter: i32,
    text: String,
}

impl EguiApp {
    fn new() -> Self {
        Self {
            counter: 0,
            text: "Hello from EGUI!".into(),
        }
    }

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
}

// #[tokio::main(flavor = "current_thread")]
#[tokio::main]
async fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();
    let mut app = Application::new();
    let mut myapp1 = EguiApp::new();
    let mut myapp2 = EguiApp::new();

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

    let mut example_window_app = EguiSurfaceState::new(&app, &example_window, 256, 256);

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

    let mut layer_surface_app = EguiSurfaceState::new(&app, &layer_surface, 256, 256);

    // Create channel for external events
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

    // Spawn background tasks that generate events
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut tick = 0u32;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            tick += 1;
            println!(
                "[ASYNC TASK] Timer tick {} on thread {:?}",
                tick,
                std::thread::current().id()
            );
            let _ = tx_clone.send(AppEvent::TimerTick(tick));
        }
    });

    let mut event_queue = app.event_queue.take().unwrap();
    loop {
        select! {
            // Wait for Wayland events, ideally I would like to figure out how to do this in a separate thread loop, because now it opens a lot of spawn_blocking threads
            _ = spawn_blocking({
                // Dispatch pending events and flush before blocking on read
                let count = event_queue.dispatch_pending(&mut app).unwrap();
                let conn = app.conn.clone();
                move || {
                    // See `EventQueue::blocking_dispatch` implementation
                    if count > 0 {
                        return;
                    }
                    conn.flush().unwrap();
                    // This function execution can take sometimes seconds (if no events are coming)
                    if let Some(guard) = conn.prepare_read() {
                        guard.read_without_dispatch().unwrap();
                    } else {
                        // Goal is that this branch is never hit, it might hit on the first iteration though
                        println!("♦️ Failed to read");
                    }
                }
            }) => {
                println!("[ASYNC MAIN] ✓ Dispatched Wayland events on thread {:?}", std::thread::current().id());
                let _ = event_queue.dispatch_pending(&mut app);
                let events = app.take_wayland_events();
                example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp1.ui(ctx));
                layer_surface_app.handle_events(&mut app, &events, &mut |ctx| myapp2.ui(ctx));
            }

            // Mock of other async events
            Some(event) = rx.recv() => {
                match event {
                    AppEvent::TimerTick(tick) => {
                        println!("[ASYNC MAIN] ✓ Received timer tick: {} on thread {:?}",
                            tick, std::thread::current().id());
                    }
                }
            }
        }
    }
}
