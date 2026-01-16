use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use wayapp::*;

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

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();
    let mut app = Application::new();
    let mut myapp1 = EguiApp::default();

    // Example window --------------------------
    let example_window = app.xdg_shell.create_window(
        app.compositor_state.create_surface(&app.qh),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.ciantic.wayapp.ExampleWindow");
    example_window.commit();

    let mut example_window_app = Some(EguiSurfaceState::new(&app, example_window, 256, 256));

    // Run the Wayland event loop
    let mut event_queue = app.event_queue.take().unwrap();
    loop {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");

        // Handle Wayland events for the example window
        let events = app.take_wayland_events();
        example_window_app.handle_events(&mut app, &events, &mut myapp1);

        // Handle close requests
        for event in &events {
            if let WaylandEvent::WindowRequestClose(win) = event {
                println!("Example window close requested, exiting...");

                // example_window_app.take_if(|v| v.contains(win));

                if example_window_app.contains(win) {
                    example_window_app.take();
                }
                return;
            }
        }
    }
}
