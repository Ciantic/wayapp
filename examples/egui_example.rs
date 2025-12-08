use egui::{CentralPanel, Context};
use egui_smithay::{EguiAppData, EguiLayerSurface, EguiWindow, get_init_app};
use smithay_client_toolkit::shell::{WaylandSurface, wlr_layer::{Anchor, Layer}, xdg::window::WindowDecorations};

struct EguiApp {
    counter: i32,
    text: String,
}

impl Default for EguiApp {
    fn default() -> Self {
        Self { counter: 0, text: "Hello from EGUI!".into() }
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
    example_window.set_app_id("io.github.smithay.client-toolkit.EguiExample");
    example_window.set_min_size(Some((256, 256)));
    example_window.commit();

    let egui_app = EguiApp::default();
    app.push_window(EguiWindow::new(example_window, egui_app, 256, 256));

    let shared_surface = app.compositor_state.create_surface(&app.qh);
    let layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        shared_surface.clone(),
        Layer::Top,
        Some("Example2"),
        None,
    );
    layer_surface.set_keyboard_interactivity(smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity::Exclusive);
    layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    layer_surface.set_margin(0, 0, 20, 20);
    layer_surface.set_size(256, 256);
    layer_surface.commit();

    app.push_layer_surface(EguiLayerSurface::new(
        layer_surface,
        EguiApp::default(),
        256,
        256));

    app.run_blocking();
}
