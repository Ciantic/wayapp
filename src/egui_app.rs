use egui::{CentralPanel, Context};

pub struct EguiApp {
    counter: i32,
    text: String,
}

impl EguiApp {
    pub fn new() -> Self {
        Self {
            counter: 0,
            text: String::from("Hello from EGUI!"),
        }
    }

    pub fn ui(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clock for Smithay - EGUI Demo");
            
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

impl Default for EguiApp {
    fn default() -> Self {
        Self::new()
    }
}
