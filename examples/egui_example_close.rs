use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
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
            ui.heading("Egui WGPU / Smithay example");

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

            ui.label("This is a simple EGUI app running on Wayland via Smithay toolkit!");
        });
    }
}

enum AppEvent {
    WaylandDispatch(DispatchToken),
    // Other events can be added here
}

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();

    // Create channel for external events
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();

    let mut app = Application::new(move |t| {
        let _ = tx.send(AppEvent::WaylandDispatch(t));
    });
    let mut myapp1 = EguiApp::new();

    // Example window --------------------------
    let example_window = app.xdg_shell.create_window(
        app.compositor_state.create_surface(&app.qh),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.ciantic.wayapp.ExampleWindow");
    example_window.commit();

    let mut example_window_app = Some(EguiSurfaceState::new(&app, &example_window, 256, 256));

    // Run the Wayland event loop
    app.run_dispatcher();

    loop {
        if let Ok(event) = rx.recv() {
            match event {
                AppEvent::WaylandDispatch(token) => {
                    let events = app.dispatch_pending(token);
                    example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp1.ui(ctx));

                    // Handle close requests
                    for event in &events {
                        if let WaylandEvent::WindowRequestClose(win) = event {
                            println!("Example window close requested, exiting...");

                            if example_window_app.contains(win) {
                                example_window_app.take();
                            }
                            return;
                        }
                    }
                } // Handle other events here
            }
        }
    }
}
