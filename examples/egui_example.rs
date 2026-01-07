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
use wayapp::get_init_app;

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
    env_logger::init();
    let app = get_init_app();

    // Example window --------------------------
    let example_win_surface = app.compositor_state.create_surface(&app.qh);
    let example_window = app.xdg_shell.create_window(
        example_win_surface,
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.ciantic.wayapp.ExampleWindow");
    example_window.set_min_size(Some((256, 256)));
    example_window.commit();

    let egui_app = EguiApp::default();
    app.push_window(EguiWindow::new(example_window, egui_app, 256, 256));

    // Example layer surface --------------------------

    // Get the first monitor/output
    let first_monitor = app
        .output_state
        .outputs()
        .collect::<Vec<_>>()
        .get(0)
        .cloned();
    let shared_surface = app.compositor_state.create_surface(&app.qh);
    let layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        shared_surface.clone(),
        Layer::Top,
        Some("Example2"),
        first_monitor.as_ref(),
    );
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    layer_surface.set_margin(0, 0, 20, 20);
    layer_surface.set_size(256, 256);

    // Restrict mouse inputs to a 50x50 box at (20,20)
    /*
    let region = app
        .compositor_state
        .wl_compositor()
        .create_region(&app.qh, ());
    region.add(20, 20, 150, 150);
    layer_surface.set_input_region(Some(&region));
    */

    layer_surface.commit();

    let egui_layer_surface = EguiLayerSurface::new(layer_surface, EguiApp::default(), 256, 256);

    app.push_layer_surface(egui_layer_surface);

    // let shared_layer_surface = Rc::new(RefCell::new();

    // app.push_layer_surface(shared_layer_surface.clone());

    app.run_blocking();
}
