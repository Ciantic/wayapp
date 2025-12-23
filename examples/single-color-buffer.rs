use log::trace;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::XdgPositioner;
use smithay_client_toolkit::shell::xdg::XdgSurface;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use std::borrow::BorrowMut;
use wayapp::*;
use wayland_client::Proxy;

fn main() {
    env_logger::init();
    let app = get_init_app();
    let mut single_color_manager = SingleColorManager::default();

    let surface1 = app.compositor_state.create_surface(&app.qh);

    let example_layer_surface = app.layer_shell.create_layer_surface(
        &app.qh,
        surface1.clone(),
        Layer::Top,
        Some("Example"),
        None,
    );
    example_layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    example_layer_surface.set_margin(0, 0, 20, 20);
    example_layer_surface.set_size(256, 256);
    example_layer_surface.commit();
    single_color_manager.push(&example_layer_surface, (None, (255, 0, 0)));

    let surface2 = app.compositor_state.create_surface(&app.qh);

    let example_layer_surface2 = app.layer_shell.create_layer_surface(
        &app.qh,
        surface2.clone(),
        Layer::Top,
        Some("Example2"),
        None,
    );
    example_layer_surface2.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    example_layer_surface2.set_margin(0, 20, 20, 0);
    example_layer_surface2.set_size(512, 256);
    example_layer_surface2.commit();
    single_color_manager.push(&example_layer_surface2, (None, (0, 255, 0)));

    // Example window --------------------------
    let example_win_surface = app.compositor_state.create_surface(&app.qh);
    let example_window = app.xdg_shell.create_window(
        example_win_surface.clone(),
        WindowDecorations::ServerDefault,
        &app.qh,
    );
    example_window.set_title("Example Window");
    example_window.set_app_id("io.github.ciantic.wayapp.SingleColorExample");
    example_window.set_min_size(Some((256, 256)));
    example_window.commit();
    single_color_manager.push(&example_window, (None, (0, 0, 255)));

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
    child_window.set_app_id("io.github.ciantic.wayapp.SingleColorExample.Child");
    child_window.set_min_size(Some((128, 128)));
    child_window.commit();
    single_color_manager.push(&child_window, (None, (255, 0, 255)));

    // Example subsurface --------------------------
    let (subsurface, sub_wlsurface) = app
        .subcompositor_state
        .create_subsurface(example_win_surface.clone(), &app.qh);
    subsurface.set_position(20, 20);
    trace!(
        "Created subsurface: {:?}",
        sub_wlsurface.id().as_ptr() as usize
    );
    single_color_manager.push(
        (
            example_win_surface.clone(),
            subsurface.clone(),
            sub_wlsurface.clone(),
        ),
        (None, (128, 255, 0)),
    );

    // app.push_subsurface(sub_example);

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
    single_color_manager.push(&popup, (None, (255, 255, 0)));

    trace!("Starting event loop for common example");
    drop(example_window);

    // Run the Wayland event loop. This example will run until the process is killed
    let mut event_queue = app.event_queue.take().unwrap();
    loop {
        event_queue
            .blocking_dispatch(app)
            .expect("Wayland dispatch failed");
        app.wayland_events.drain(..).for_each(|event| {
            single_color_manager.handle_events(&[event])
            //
        });
    }
}
