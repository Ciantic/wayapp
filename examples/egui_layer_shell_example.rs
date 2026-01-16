use egui::CentralPanel;
use egui::Context;
use log::debug;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use wayapp::*;

struct EguiApp {
    layer_surface: LayerSurface,
    width: u32,
    height: u32,
    margin_top: i32,
    margin_right: i32,
    margin_bottom: i32,
    margin_left: i32,
    anchor_top: bool,
    anchor_bottom: bool,
    anchor_left: bool,
    anchor_right: bool,
}

impl EguiApp {
    fn new(layer_surface: &LayerSurface) -> Self {
        Self {
            layer_surface: layer_surface.clone(),
            width: 512,
            height: 512,
            margin_top: 0,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
            anchor_top: false,
            anchor_bottom: false,
            anchor_left: false,
            anchor_right: false,
        }
    }
}

impl EguiAppData for EguiApp {
    fn ui(&mut self, ctx: &Context) {
        ctx.set_visuals(egui::Visuals::light());

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Egui WGPU / Smithay example");

            ui.separator();

            // Size section
            ui.heading("Size");
            ui.horizontal(|ui| {
                ui.label("Width:");
                ui.add(egui::Slider::new(&mut self.width, 100..=1024).text("Width"));
            });
            ui.horizontal(|ui| {
                ui.label("Height:");
                ui.add(egui::Slider::new(&mut self.height, 100..=1024).text("Height"));
            });
            if ui.button("Apply Size").clicked() {
                debug!("Setting size to {}x{}", self.width, self.height);
                self.layer_surface.set_size(self.width, self.height);
            }

            ui.separator();

            // Anchor section
            ui.heading("Anchor");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.anchor_top, "Top");
                ui.checkbox(&mut self.anchor_bottom, "Bottom");
                ui.checkbox(&mut self.anchor_left, "Left");
                ui.checkbox(&mut self.anchor_right, "Right");
            });
            if ui.button("Apply Anchor").clicked() {
                let mut anchor = Anchor::empty();
                if self.anchor_top {
                    anchor |= Anchor::TOP;
                }
                if self.anchor_bottom {
                    anchor |= Anchor::BOTTOM;
                }
                if self.anchor_left {
                    anchor |= Anchor::LEFT;
                }
                if self.anchor_right {
                    anchor |= Anchor::RIGHT;
                }
                debug!("Setting anchor to {:?}", anchor);
                self.layer_surface.set_anchor(anchor);
            }

            ui.separator();

            // Margin section
            ui.heading("Margin");
            ui.horizontal(|ui| {
                ui.label("Top:");
                ui.add(egui::Slider::new(&mut self.margin_top, 0..=100).text("Top"));
            });
            ui.horizontal(|ui| {
                ui.label("Right:");
                ui.add(egui::Slider::new(&mut self.margin_right, 0..=100).text("Right"));
            });
            ui.horizontal(|ui| {
                ui.label("Bottom:");
                ui.add(egui::Slider::new(&mut self.margin_bottom, 0..=100).text("Bottom"));
            });
            ui.horizontal(|ui| {
                ui.label("Left:");
                ui.add(egui::Slider::new(&mut self.margin_left, 0..=100).text("Left"));
            });
            if ui.button("Apply Margin").clicked() {
                debug!(
                    "Setting margin to ({}, {}, {}, {})",
                    self.margin_top, self.margin_right, self.margin_bottom, self.margin_left
                );
                self.layer_surface.set_margin(
                    self.margin_top,
                    self.margin_right,
                    self.margin_bottom,
                    self.margin_left,
                );
            }
        });
    }
}

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();
    let mut app = Application::new();

    // Example layer surface --------------------------
    let layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        app.compositor_state.create_surface(&app.qh),
        Layer::Top,
        Some("Example2"),
        None,
    );
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_size(512, 512);
    layer_surface.commit();

    let mut my_app = EguiApp::new(&layer_surface);
    let mut egui_surface = EguiSurfaceState::new(&app, &layer_surface, 512, 512);

    // Run the Wayland event loop
    let mut event_queue = app.event_queue.take().unwrap();
    loop {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");

        let events = app.take_wayland_events();
        egui_surface.handle_events(&mut app, &events, &mut my_app);
    }
}
