mod application;
mod containers;
mod egui;
mod single_color;

pub use application::*;
pub use containers::*;
pub use egui::*;
pub use single_color::*;
// Re-export wgpu types needed for SurfaceOptions configuration
pub use wgpu::{Color as WgpuColor, CompositeAlphaMode, PresentMode};

// Re-export wayland types needed for multi-output support
pub use wayland_client::protocol::wl_output::WlOutput;
pub use wayland_client::Proxy;
