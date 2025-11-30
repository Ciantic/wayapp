use log::trace;

use egui_smithay::*;

use smithay_client_toolkit::{
	compositor::CompositorState, output::OutputState, registry::{ProvidesRegistryState, RegistryState}, seat::SeatState, shell::{WaylandSurface, wlr_layer::{Anchor, Layer, LayerShell}, xdg::{XdgPositioner, XdgShell, XdgSurface, popup::Popup, window::WindowDecorations}}, shm::Shm, subcompositor::SubcompositorState
};
use smithay_clipboard::Clipboard;
use wayland_client::{Connection, Proxy, globals::registry_queue_init};
use wayland_protocols::xdg::{self, shell::client::{xdg_popup::XdgPopup, xdg_positioner::ConstraintAdjustment}};

fn main() {
	env_logger::init();

	let conn = Connection::connect_to_env().expect("Failed to connect to Wayland");
	let (globals, mut event_queue) = registry_queue_init(&conn).expect("Failed to init registry");
	let qh = event_queue.handle();

	// Bind required globals
	let compositor_state = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
	let subscompositor_state = SubcompositorState::bind(compositor_state.wl_compositor().clone(), &globals, &qh).expect("wl_subcompositor not available");
	let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell not available");
	let shm_state = Shm::bind(&globals, &qh).expect("wl_shm not available");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell not available");
	

	// Clipboard (needed for InputState)
	let clipboard = unsafe { Clipboard::new(conn.display().id().as_ptr() as *mut _) };

	// Build our Application container from `common.rs`
	let mut app = Application::new(
		RegistryState::new(&globals),
		SeatState::new(&globals, &qh),
		OutputState::new(&globals, &qh),
		shm_state,
		InputState::new(clipboard),
	);

	// Experiment to share the same surface between multiple layer surfaces
	let shared_surface = compositor_state.create_surface(&qh);

	let example_layer_surface = layer_shell.create_layer_surface(
		&qh,
		shared_surface.clone(),
		Layer::Top,
		Some("Example"),
		None,
	);
	example_layer_surface.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
	example_layer_surface.set_margin(0, 0, 20, 20);
	example_layer_surface.set_size(256, 256);
	example_layer_surface.commit();

	let example_layer_surface2 = layer_shell.create_layer_surface(
		&qh,
		shared_surface.clone(),
		Layer::Top,
		Some("Example2"),
		None,
	);
	example_layer_surface2.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
	example_layer_surface2.set_margin(0, 20, 20, 0);
	example_layer_surface2.set_size(512, 256);
	example_layer_surface2.commit();

	// Example window --------------------------
	let example_win_surface = compositor_state.create_surface(&qh);
	let example_window = xdg_shell.create_window(example_win_surface.clone(), WindowDecorations::ServerDefault, &qh);
	example_window.set_title("Example Window");
	example_window.set_app_id("io.github.smithay.client-toolkit.EguiExample");
	example_window.set_min_size(Some((256,256)));
	example_window.commit();

	// Example child window --------------------------
	// Create a surface for the child window
	let child_surface = compositor_state.create_surface(&qh);
	let child_window = xdg_shell.create_window(child_surface.clone(), WindowDecorations::ServerDefault, &qh);
	child_window.set_parent(Some(&example_window));
	child_window.set_title("Child Window");
	child_window.set_app_id("io.github.smithay.client-toolkit.EguiExample.Child");
	child_window.set_min_size(Some((128, 128)));
	child_window.commit();

	// Example subsurface --------------------------
	let (subsurface, sub_wlsurface) = subscompositor_state.create_subsurface(example_win_surface.clone(), &qh);
	subsurface.set_position(20, 20);
	app.single_color_example_buffer_configure(&sub_wlsurface, &qh, 128, 128, (0, 0, 255));
	trace!("Created subsurface: {:?}", sub_wlsurface.id().as_ptr() as usize);

	// Example popup, attached to example window --------------------------
	let xdg_surface = example_window.xdg_surface();
	let positioner = XdgPositioner::new(&xdg_shell).unwrap();
	positioner.set_anchor_rect(100, 100, 1, 1);
	positioner.set_offset(130, 180);
	positioner.set_size(50, 20);
	let popup = Popup::new(
		&xdg_surface,
		&positioner,
		&qh,
		&compositor_state,
		&xdg_shell
	).unwrap();

	trace!("Starting event loop for common example");

	// Run the Wayland event loop. This example will run until the process is killed
	loop {
		event_queue.blocking_dispatch(&mut app).expect("Wayland dispatch failed");
	}
}
