use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use wayapp::*;

struct EguiApp {
    counter: i32,
    text: String,
    fps: f32,
}

impl EguiApp {
    fn new() -> Self {
        Self {
            fps: 0.0,
            counter: 0,
            text: "Hello from EGUI!".into(),
        }
    }

    fn ui(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Egui WGPU / Smithay example");
            ui.label(format!("FPS: {:.2}", self.fps));

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
    let mut myapp2 = EguiApp::new();
    let first_monitor = app
        .output_state
        .outputs()
        .collect::<Vec<_>>()
        .get(0)
        .cloned();

    // Example window --------------------------
    let example_window = app.xdg_shell.create_window(
        app.compositor_state.create_surface(&app.qh),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.ciantic.wayapp.ExampleWindow");
    // example_window.set_min_size(Some((1, 1)));
    example_window.commit();

    let mut example_window_app = EguiSurfaceState::new(&app, &example_window, 256, 256);

    // Example layer surface --------------------------
    let layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        app.compositor_state.create_surface(&app.qh),
        Layer::Top,
        Some("Example2"),
        first_monitor.as_ref(),
    );
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    layer_surface.set_margin(0, 0, 20, 20);
    layer_surface.set_size(256, 256);

    // Example how to restrict input region
    layer_surface.set_input_region(Some(&{
        let region = app
            .compositor_state
            .wl_compositor()
            .create_region(&app.qh, ());
        region.add(20, 20, 150, 150);
        region
    }));
    layer_surface.commit();

    let mut layer_surface_app = EguiSurfaceState::new(&app, &layer_surface, 256, 256);

    // Run the Wayland event loop
    app.run_dispatcher();

    loop {
        if let Ok(event) = rx.recv() {
            match event {
                AppEvent::WaylandDispatch(token) => {
                    let events = app.dispatch_pending(token);
                    example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp1.ui(ctx));
                    layer_surface_app.handle_events(&mut app, &events, &mut |ctx| myapp2.ui(ctx));
                } // Handle other events here
            }
        }
    }
}
