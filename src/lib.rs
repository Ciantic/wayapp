mod application;
// mod egui;
mod egui;
mod frame_scheduler;
mod kind;
mod single_color;

pub use application::*;
// pub use egui::*;
pub use egui::*;
pub(crate) use frame_scheduler::*;
pub use kind::*;
pub use single_color::*;
