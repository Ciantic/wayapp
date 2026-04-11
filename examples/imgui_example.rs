use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use std::time::Instant;
use wayapp::*;

struct MyApp {
    counter: i32,
    show_demo: bool,
    fps: f32,
    last_render: Instant,
}

impl MyApp {
    fn new() -> Self {
        Self {
            fps: 0.0,
            counter: 0,
            show_demo: false,
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

    fn ui(&mut self, ui: &imgui::Ui) {
        ui.window("imgui Wayland example")
            .size([300.0, 200.0], imgui::Condition::FirstUseEver)
            .build(|| {
                ui.text("Hello from imgui on Wayland!");
                ui.separator();
                ui.text(format!("Counter: {}", self.counter));
                if ui.button("Increment") {
                    self.counter += 1;
                }
                ui.same_line();
                if ui.button("Decrement") {
                    self.counter -= 1;
                }
                ui.separator();
                ui.text(format!("Last render time: {:?}", self.last_render));
                ui.text(format!("FPS between two last frames: {:.2}", self.fps));
                ui.separator();
                ui.checkbox("Show demo window", &mut self.show_demo);
            });

        if self.show_demo {
            ui.show_demo_window(&mut self.show_demo);
        }
    }
}

enum AppEvent {
    WaylandDispatch(DispatchToken),
}

fn main() {
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let mut app = Application::new(move |t| {
        let _ = tx.send(AppEvent::WaylandDispatch(t));
    });

    let window = app.xdg_shell.create_window(
        app.compositor_state.create_surface(&app.qh),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    window.set_title("imgui Wayland example");
    window.set_app_id("io.github.ciantic.wayapp.ImguiExample");
    window.commit();

    let mut surface = ImguiSurfaceState::new(&app, &window, 800, 600);
    let mut my_app = MyApp::new();

    app.run_dispatcher();

    'main_loop: loop {
        if let Ok(AppEvent::WaylandDispatch(token)) = rx.recv() {
            let events = app.dispatch_pending(token);
            surface.handle_events(&mut app, &events, &mut |ui| my_app.ui(ui));

            // Update FPS info
            if let Some(last_render) = surface.get_frame_timings() {
                my_app.set_last_render(last_render);
                my_app.set_fps(surface.get_fps());
            }

            for event in &events {
                if let WaylandEvent::WindowRequestClose(_) = event {
                    break 'main_loop;
                }
            }
        }
    }
}
