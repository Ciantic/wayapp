use egui::{CentralPanel, Context};
use egui_smithay::{EguiAppData, EguiWindow, get_init_app};
use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::WindowDecorations};

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
    app.push_window(EguiWindow::new(&app, example_window, egui_app));

    app.run_blocking();
}
