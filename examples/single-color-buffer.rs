use log::trace;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use smithay_client_toolkit::shell::wlr_layer::Layer;
use smithay_client_toolkit::shell::xdg::XdgPositioner;
use smithay_client_toolkit::shell::xdg::XdgSurface;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::window::WindowDecorations;
use wayapp::*;
use wayland_client::Proxy;

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "wayapp=trace") };
    env_logger::init();
    let mut app = Application::new();

    let surface1 = app.compositor_state.create_surface(&app.qh);
    let example_layer_surface =
        app.layer_shell
            .create_layer_surface(&app.qh, surface1, Layer::Top, Some("Example"), None);
    example_layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
    example_layer_surface.set_margin(0, 0, 20, 20);
    example_layer_surface.set_size(256, 256);
    example_layer_surface.commit();
    let mut example_layer_state =
        SingleColorState::new(&example_layer_surface, (255, 0, 0), 256, 256);

    let surface2 = app.compositor_state.create_surface(&app.qh);
    let example_layer_surface2 =
        app.layer_shell
            .create_layer_surface(&app.qh, surface2, Layer::Top, Some("Example2"), None);
    example_layer_surface2.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    example_layer_surface2.set_margin(0, 20, 20, 0);
    example_layer_surface2.set_size(512, 256);
    example_layer_surface2.commit();
    let mut example_layer_state2 =
        SingleColorState::new(&example_layer_surface2, (0, 255, 0), 512, 256);

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
    let mut example_window_state = SingleColorState::new(&example_window, (0, 0, 255), 256, 256);

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
    let mut child_window_state = SingleColorState::new(&child_window, (255, 0, 255), 128, 128);

    // Example subsurface --------------------------
    // let (subsurface, sub_wlsurface) = app
    //     .subcompositor_state
    //     .create_subsurface(example_win_surface.clone(), &app.qh);
    // subsurface.set_position(20, 20);
    // trace!(
    //     "Created subsurface: {:?}",
    //     sub_wlsurface.id().as_ptr() as usize
    // );
    // single_color_manager.push(
    //     (
    //         example_win_surface.clone(),
    //         subsurface.clone(),
    //         sub_wlsurface.clone(),
    //     ),
    //     SingleColorState::new((128, 255, 0)),
    // );

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
    let mut popup_state = SingleColorState::new(&popup, (255, 255, 0), 50, 20);

    // Run the Wayland event loop. This example will run until the process is killed
    let mut event_queue = app.event_queue.take().unwrap();
    loop {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("Wayland dispatch failed");
        let events = app.take_wayland_events();
        example_layer_state.handle_events(&mut app, &events);
        example_layer_state2.handle_events(&mut app, &events);
        example_window_state.handle_events(&mut app, &events);
        child_window_state.handle_events(&mut app, &events);
        popup_state.handle_events(&mut app, &events);
    }
}
