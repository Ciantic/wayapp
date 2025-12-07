use log::trace;

use egui_smithay::*;

use smithay_client_toolkit::shell::{
    WaylandSurface,
    wlr_layer::{Anchor, Layer},
    xdg::{XdgPositioner, XdgSurface, popup::Popup, window::WindowDecorations},
};
use wayland_client::Proxy;

fn main() {
    env_logger::init();
    let app = get_init_app();

    // Experiment to share the same surface between multiple layer surfaces
    let shared_surface = app.compositor_state.create_surface(&app.qh);

    let example_layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        shared_surface.clone(),
        Layer::Top,
        Some("Example"),
        None,
    );
    example_layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    example_layer_surface.set_margin(0, 0, 20, 20);
    example_layer_surface.set_size(256, 256);
    example_layer_surface.commit();

    app.push_layer_surface(ExampleSingleColorLayerSurface {
        layer_surface: example_layer_surface,
        color: (255, 0, 0),
        pool: None,
    });

    let example_layer_surface2 = app.layer_shell.create_layer_surface(
        &app.qh,
        shared_surface.clone(),
        Layer::Top,
        Some("Example2"),
        None,
    );
    example_layer_surface2.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    example_layer_surface2.set_margin(0, 20, 20, 0);
    example_layer_surface2.set_size(512, 256);
    example_layer_surface2.commit();

    app.push_layer_surface(ExampleSingleColorLayerSurface {
        layer_surface: example_layer_surface2,
        color: (0, 255, 0),
        pool: None,
    });

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

    app.push_window(ExampleSingleColorWindow {
        window: example_window.clone(),
        color: (0, 0, 255),
        pool: None,
    });

    // Example child window --------------------------
    // Create a surface for the child window
    let child_surface = app.compositor_state.create_surface(&app.qh);
    let child_window = app.xdg_shell.create_window(
        child_surface.clone(),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    child_window.set_parent(Some(&example_window));
    child_window.set_title("Child Window");
    child_window.set_app_id("io.github.smithay.client-toolkit.EguiExample.Child");
    child_window.set_min_size(Some((128, 128)));
    child_window.commit();

    app.push_window(ExampleSingleColorWindow {
        window: child_window,
        color: (255, 0, 255),
        pool: None,
    });

    // Example subsurface --------------------------
    let (subsurface, sub_wlsurface) = app
        .subcompositor_state
        .create_subsurface(example_win_surface.clone(), &app.qh);
    subsurface.set_position(20, 20);
    trace!(
        "Created subsurface: {:?}",
        sub_wlsurface.id().as_ptr() as usize
    );

    let mut sub_example = ExampleSingleColorSubsurface {
        wl_surface: sub_wlsurface,
        color: (128, 128, 0),
        pool: None,
    };
    
    // Configure initial size for subsurface
    sub_example.configure(100, 100);
    
    app.push_subsurface(sub_example);

    // Example popup, attached to example window --------------------------
    let xdg_surface = example_window.xdg_surface();
    let positioner = XdgPositioner::new(&app.xdg_shell).unwrap();
    positioner.set_anchor_rect(100, 100, 1, 1);
    positioner.set_offset(130, 180);
    positioner.set_size(50, 20);
    let popup = Popup::new(
        &xdg_surface,
        &positioner,
        &app.qh,
        &app.compositor_state,
        &app.xdg_shell,
    )
    .unwrap();

    app.push_popup(ExampleSingleColorPopup {
        popup,
        color: (255, 255, 0),
        pool: None,
    });

    trace!("Starting event loop for common example");
	drop(example_window);

    // Run the Wayland event loop. This example will run until the process is killed
    app.run_blocking();
}
