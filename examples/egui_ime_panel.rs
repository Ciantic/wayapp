use egui::CentralPanel;
use egui::Context;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use std::time::Instant;
use wayapp::*;

struct EguiApp {
    fps: f32,
    last_render: Instant,
}

impl EguiApp {
    fn new() -> Self {
        Self {
            fps: 0.0,
            last_render: Instant::now(),
        }
    }

    fn set_last_render(&mut self, prev_next_frame: (Instant, Instant)) {
        let (_, next_frame) = prev_next_frame;
        self.last_render = next_frame;
    }

    fn set_fps(&mut self, fps: f32) {
        self.fps = fps;
    }

    fn ui(&mut self, ctx: &Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(255, 128, 128, 128);
        ctx.set_visuals(visuals);

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Egui Transparency Example");
            ui.label(format!("Last render time: {:?}", self.last_render));
            ui.label(format!("FPS between two last frames: {:.2}", self.fps));
            ui.label(format!("Frame number: {}", ctx.cumulative_pass_nr()));
            ui.add(egui::Spinner::new());
            ui.add(egui::Spinner::new());
            ui.add(egui::Spinner::new());
            ui.add(egui::Spinner::new());
            ui.add(egui::Spinner::new());
            ui.add(egui::Spinner::new());
        });

        // For continuous rendering:
        // ctx.request_repaint();
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

    let mut example_window_app = EguiSurfaceState::new(&app, &example_window, 300, 300);

    let wl = app.compositor_state.create_surface(&app.qh);
    let panel_surface = app
        .input_panel
        .get()
        .unwrap()
        .get_input_panel_surface(&wl, &app.qh, ());
    panel_surface.set_overlay_panel();

    // Run the Wayland event loop
    app.run_dispatcher();

    'main_loop: loop {
        if let Ok(event) = rx.recv() {
            match event {
                AppEvent::WaylandDispatch(token) => {
                    // Normal Wayland event dispatching to the windows and surfaces
                    let events = app.dispatch_pending(token);
                    example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp1.ui(ctx));

                    // Update FPS info
                    if let Some(last_render) = example_window_app.get_frame_timings() {
                        myapp1.set_last_render(last_render);
                        myapp1.set_fps(example_window_app.get_fps());
                    }

                    // Handle other Wayland events here if needed
                    for event in events {
                        match event {
                            WaylandEvent::WindowRequestClose(_) => {
                                break 'main_loop;
                            }
                            _ => {}
                        }
                    }
                } // Handle other events here
            }
        }
    }
}
