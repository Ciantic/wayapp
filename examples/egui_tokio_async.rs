use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use tokio::sync::mpsc::UnboundedSender;
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
    WaylandDispatch,
    TimerTick(u32),
}

#[tokio::main(flavor = "current_thread")] // This works
// #[tokio::main] // This also works
#[hotpath::main(percentiles = [100])]
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

    spawn_ticking_thread(tx.clone());

    let mut dispatcher = app.run_dispatcher(move || tx.send(AppEvent::WaylandDispatch).unwrap());
    loop {
        if let Some(event) = rx.recv().await {
            match event {
                AppEvent::TimerTick(tick) => {
                    println!(
                        "[ASYNC MAIN] ✓ Received timer tick: {} on thread {:?}",
                        tick,
                        std::thread::current().id()
                    );
                }
                AppEvent::WaylandDispatch => {
                    let events = dispatcher.dispatch_pending(&mut app);
                    example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp1.ui(ctx));
                    layer_surface_app.handle_events(&mut app, &events, &mut |ctx| myapp2.ui(ctx));
                }
            }
        }
    }
}

fn spawn_ticking_thread(sender: UnboundedSender<AppEvent>) {
    std::thread::spawn(move || {
        let mut tick = 0u32;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            tick += 1;
            println!(
                "[ASYNC TASK] Timer tick {} on thread {:?}",
                tick,
                std::thread::current().id()
            );
            let _ = sender.send(AppEvent::TimerTick(tick));
        }
    });
}
