//! EGUI view manager implementation
//!
//! This module provides a ViewManager-based approach to handling EGUI surfaces
//! following the pattern from single_color.rs

#![allow(dead_code)]

use crate::Application;
use crate::WaylandEvent;
use crate::egui_new::EguiWgpuRenderer;
use egui::Event;
use egui::Key;
use egui::Modifiers as EguiModifiers;
use egui::PlatformOutput;
use egui::PointerButton;
use egui::Pos2;
use egui::RawInput;
use egui::ahash::HashMap;
use egui_wgpu::Renderer;
use egui_wgpu::RendererOptions;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu;
use log::trace;
use pollster::block_on;
use raw_window_handle::RawDisplayHandle;
use raw_window_handle::RawWindowHandle;
use raw_window_handle::WaylandDisplayHandle;
use raw_window_handle::WaylandWindowHandle;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_clipboard::Clipboard;
use std::num::NonZero;
use std::ptr::NonNull;
use std::time::Duration;
use std::time::Instant;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

/// Handles input events from Wayland and converts them to EGUI RawInput
pub struct WaylandToEguiInput {
    modifiers: EguiModifiers,
    pointer_pos: Pos2,
    events: Vec<Event>,
    screen_width: u32,
    screen_height: u32,
    start_time: Instant,
    clipboard: Clipboard,
    last_key_utf8: Option<String>,
}

impl WaylandToEguiInput {
    pub fn new(clipboard: Clipboard) -> Self {
        Self {
            modifiers: EguiModifiers::default(),
            pointer_pos: Pos2::ZERO,
            events: Vec::new(),
            screen_width: 256,
            screen_height: 256,
            start_time: Instant::now(),
            clipboard,
            last_key_utf8: None,
        }
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    pub fn handle_pointer_event(&mut self, event: &PointerEvent) {
        match &event.kind {
            PointerEventKind::Enter { .. } => {}
            PointerEventKind::Leave { .. } => {
                self.events.push(Event::PointerGone);
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = Pos2::new(x as f32, y as f32);
                self.events.push(Event::PointerMoved(self.pointer_pos));
            }
            PointerEventKind::Press { button, .. } => {
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: true,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Release { button, .. } => {
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: false,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                let scroll_delta = egui::vec2(
                    horizontal.discrete as f32 * 10.0,
                    vertical.discrete as f32 * 10.0,
                );
                if scroll_delta != egui::Vec2::ZERO {
                    self.events.push(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: scroll_delta,
                        modifiers: self.modifiers,
                    });
                }
            }
        }
    }

    pub fn handle_keyboard_enter(&mut self) {
        self.events.push(Event::WindowFocused(true));
    }

    pub fn handle_keyboard_leave(&mut self) {
        self.events.push(Event::WindowFocused(false));
    }

    pub fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, is_repeat: bool) {
        if pressed && !is_repeat && self.modifiers.ctrl {
            match event.keysym {
                Keysym::c => self.events.push(Event::Copy),
                Keysym::x => self.events.push(Event::Cut),
                Keysym::v => self
                    .events
                    .push(Event::Paste(self.clipboard.load().unwrap_or_default())),
                _ => (),
            }
        }

        if let Some(key) = keysym_to_egui_key(event.keysym) {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: is_repeat,
                modifiers: self.modifiers,
            });
        } else {
            trace!(
                "[INPUT] No EGUI key mapping for keysym: {:?}",
                event.keysym.raw()
            );
        }

        if pressed || is_repeat {
            let mut text = event.utf8.clone();
            if is_repeat && text.is_none() {
                text = self.last_key_utf8.clone();
            }
            if let Some(text) = text {
                if !text.chars().any(|c| c.is_control()) {
                    self.events.push(Event::Text(text.clone()));
                }
            }
        }

        if event.utf8.is_some() {
            self.last_key_utf8 = event.utf8.clone();
        }
    }

    pub fn update_modifiers(&mut self, wayland_mods: &WaylandModifiers) {
        self.modifiers = EguiModifiers {
            alt: wayland_mods.alt,
            ctrl: wayland_mods.ctrl,
            shift: wayland_mods.shift,
            mac_cmd: false,
            command: wayland_mods.ctrl,
        };
    }

    pub fn take_raw_input(&mut self) -> RawInput {
        let events = std::mem::take(&mut self.events);
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(self.screen_width as f32, self.screen_height as f32),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            modifiers: self.modifiers,
            events,
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true,
            ..Default::default()
        }
    }

    pub fn handle_output_command(&mut self, output: &egui::OutputCommand) {
        match output {
            egui::OutputCommand::CopyText(text) => {
                self.clipboard.store(text.clone());
            }
            egui::OutputCommand::CopyImage(_image) => {
                // Handle image copy if needed
                trace!("[INPUT] CopyImage command received (not implemented)");
                // TODO: Implement image copying to clipboard if required
            }
            egui::OutputCommand::OpenUrl(url) => {
                trace!("[INPUT] OpenUrl command received: {}", url.url);
            }
        }
    }
}

fn wayland_button_to_egui(button: u32) -> Option<PointerButton> {
    // Linux button codes (from linux/input-event-codes.h)
    match button {
        0x110 => Some(PointerButton::Primary),
        0x111 => Some(PointerButton::Secondary),
        0x112 => Some(PointerButton::Middle),
        _ => None,
    }
}

fn keysym_to_egui_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        // Commands:
        Keysym::downarrow | Keysym::Down => Key::ArrowDown,
        Keysym::leftarrow | Keysym::Left => Key::ArrowLeft,
        Keysym::rightarrow | Keysym::Right => Key::ArrowRight,
        Keysym::uparrow | Keysym::Up => Key::ArrowUp,
        Keysym::Escape => Key::Escape,
        Keysym::Tab => Key::Tab,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Return => Key::Enter,
        Keysym::Insert => Key::Insert,
        Keysym::Delete => Key::Delete,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Prior => Key::PageUp,
        Keysym::Next => Key::PageDown,
        // Punctuation:
        Keysym::space => Key::Space,
        Keysym::colon => Key::Colon,
        Keysym::comma => Key::Comma,
        Keysym::minus => Key::Minus,
        Keysym::period => Key::Period,
        Keysym::plus => Key::Plus,
        Keysym::equal => Key::Equals,
        Keysym::semicolon => Key::Semicolon,
        Keysym::bracketleft => Key::OpenBracket,
        Keysym::bracketright => Key::CloseBracket,
        Keysym::braceleft => Key::OpenCurlyBracket,
        Keysym::braceright => Key::CloseCurlyBracket,
        Keysym::grave => Key::Backtick,
        Keysym::backslash => Key::Backslash,
        Keysym::slash => Key::Slash,
        Keysym::bar => Key::Pipe,
        Keysym::question => Key::Questionmark,
        Keysym::exclam => Key::Exclamationmark,
        Keysym::apostrophe => Key::Quote,
        // Digits:
        Keysym::_0 => Key::Num0,
        Keysym::_1 => Key::Num1,
        Keysym::_2 => Key::Num2,
        Keysym::_3 => Key::Num3,
        Keysym::_4 => Key::Num4,
        Keysym::_5 => Key::Num5,
        Keysym::_6 => Key::Num6,
        Keysym::_7 => Key::Num7,
        Keysym::_8 => Key::Num8,
        Keysym::_9 => Key::Num9,
        // Letters:
        Keysym::a => Key::A,
        Keysym::b => Key::B,
        Keysym::c => Key::C,
        Keysym::d => Key::D,
        Keysym::e => Key::E,
        Keysym::f => Key::F,
        Keysym::g => Key::G,
        Keysym::h => Key::H,
        Keysym::i => Key::I,
        Keysym::j => Key::J,
        Keysym::k => Key::K,
        Keysym::l => Key::L,
        Keysym::m => Key::M,
        Keysym::n => Key::N,
        Keysym::o => Key::O,
        Keysym::p => Key::P,
        Keysym::q => Key::Q,
        Keysym::r => Key::R,
        Keysym::s => Key::S,
        Keysym::t => Key::T,
        Keysym::u => Key::U,
        Keysym::v => Key::V,
        Keysym::w => Key::W,
        Keysym::x => Key::X,
        Keysym::y => Key::Y,
        Keysym::z => Key::Z,
        // Function keys:
        Keysym::F1 => Key::F1,
        Keysym::F2 => Key::F2,
        Keysym::F3 => Key::F3,
        Keysym::F4 => Key::F4,
        Keysym::F5 => Key::F5,
        Keysym::F6 => Key::F6,
        Keysym::F7 => Key::F7,
        Keysym::F8 => Key::F8,
        Keysym::F9 => Key::F9,
        Keysym::F10 => Key::F10,
        Keysym::F11 => Key::F11,
        Keysym::F12 => Key::F12,
        Keysym::F13 => Key::F13,
        Keysym::F14 => Key::F14,
        Keysym::F15 => Key::F15,
        Keysym::F16 => Key::F16,
        Keysym::F17 => Key::F17,
        Keysym::F18 => Key::F18,
        Keysym::F19 => Key::F19,
        Keysym::F20 => Key::F20,
        Keysym::F21 => Key::F21,
        Keysym::F22 => Key::F22,
        Keysym::F23 => Key::F23,
        Keysym::F24 => Key::F24,
        Keysym::F25 => Key::F25,
        Keysym::F26 => Key::F26,
        Keysym::F27 => Key::F27,
        Keysym::F28 => Key::F28,
        Keysym::F29 => Key::F29,
        Keysym::F30 => Key::F30,
        Keysym::F31 => Key::F31,
        Keysym::F32 => Key::F32,
        Keysym::F33 => Key::F33,
        Keysym::F34 => Key::F34,
        Keysym::F35 => Key::F35,
        // Navigation keys:
        // Keysym::BrowserBack => Key::BrowserBack,
        _ => return None,
    })
}

pub fn egui_to_cursor_shape(cursor: egui::CursorIcon) -> Shape {
    use Shape as C;
    use egui::CursorIcon::*;

    match cursor {
        Default => C::Default,
        None => C::Default,
        ContextMenu => C::ContextMenu,
        Help => C::Help,
        PointingHand => C::Pointer,
        Progress => C::Progress,
        Wait => C::Wait,
        Cell => C::Cell,
        Crosshair => C::Crosshair,
        Text => C::Text,
        VerticalText => C::VerticalText,
        Alias => C::Alias,
        Copy => C::Copy,
        Move => C::Move,
        NoDrop => C::NoDrop,
        NotAllowed => C::NotAllowed,
        Grab => C::Grab,
        Grabbing => C::Grabbing,
        AllScroll => C::AllScroll,
        ResizeHorizontal => C::EwResize,
        ResizeNeSw => C::NeswResize,
        ResizeNwSe => C::NwseResize,
        ResizeVertical => C::NsResize,
        ResizeEast => C::EResize,
        ResizeSouthEast => C::SeResize,
        ResizeSouth => C::SResize,
        ResizeSouthWest => C::SwResize,
        ResizeWest => C::WResize,
        ResizeNorthWest => C::NwResize,
        ResizeNorth => C::NResize,
        ResizeNorthEast => C::NeResize,
        ResizeColumn => C::ColResize,
        ResizeRow => C::RowResize,
        ZoomIn => C::ZoomIn,
        ZoomOut => C::ZoomOut,
    }
}
