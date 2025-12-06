use std::{cell::{RefCell, RefMut}, num::NonZero, rc::Rc};

use egui::{CentralPanel, Context};
use egui_smithay::{Application, ExampleSingleColorWindow, InputState, WindowContainer};
use smithay_client_toolkit::{compositor::CompositorState, output::OutputState, registry::RegistryState, seat::{SeatState, pointer::cursor_shape::CursorShapeManager}, shell::{WaylandSurface, wlr_layer::LayerShell, xdg::{XdgShell, window::{Window, WindowConfigure, WindowDecorations}}}, shm::Shm, subcompositor::SubcompositorState};
use smithay_clipboard::Clipboard;
use wayland_client::{Connection, Proxy, QueueHandle, globals::registry_queue_init};

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

impl Default for EguiApp {
    fn default() -> Self {
        Self::new()
    }
}


fn main() {
    env_logger::init();
    // let mut egui_app = EguiApp::new();
    let app_ = Rc::new(RefCell::new(Application::new()));
    let mut app = app_.borrow_mut();

        // Example window --------------------------
    let example_win_surface = app.compositor_state.create_surface(&app.qh);
    let example_window = app.xdg_shell.create_window(
        example_win_surface.clone(),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.smithay.client-toolkit.EguiExample");
    example_window.set_min_size(Some((256, 256)));
    example_window.commit();

    app.windows.push(Box::new(ExampleSingleColorWindow {
        window: example_window,
        color: (0, 255, 0),
        pool: None,
    }));

    // app.window_configure.insert(example_window.wl_surface().id().clone(), Box::new(|app, config, wl_surface| {
    //     let width = config.new_size.0.unwrap().get();
    //     let height = config.new_size.1.unwrap().get();
    //     app.single_color_example_buffer_configure(&wl_surface, &app.qh.clone(), width, height, (0, 0,  255));
    //     // Here we would normally set up the EGUI renderer with the window's surface
    //     // For simplicity, we just log that the window was configured
    //     log::info!("Configured window: {:?}", config);
    // }));

    app.run_blocking();
}
