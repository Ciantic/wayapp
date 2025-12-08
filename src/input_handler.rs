use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym, Modifiers as WaylandModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_clipboard::Clipboard;
use std::time::Instant;
use log::trace;

/// Handles input events from Wayland and converts them to EGUI RawInput
pub struct InputState {
    modifiers: Modifiers,
    pointer_pos: Pos2,
    events: Vec<Event>,
    screen_width: u32,
    screen_height: u32,
    start_time: Instant,
    // pressed_keys: std::collections::HashSet<u32>,
    clipboard: Clipboard,
    last_key_utf8: Option<String>,
}

impl InputState {
    pub fn new(clipboard: Clipboard) -> Self {
        Self {
            modifiers: Modifiers::default(),
            pointer_pos: Pos2::ZERO,
            events: Vec::new(),
            screen_width: 256,
            screen_height: 256,
            start_time: Instant::now(),
            // pressed_keys: std::collections::HashSet::new(),
            clipboard,
            last_key_utf8: None,
        }
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    pub fn handle_pointer_event(&mut self, event: &PointerEvent) {
        trace!("[INPUT] Pointer event: {:?}", event.kind);
        match &event.kind {
            PointerEventKind::Enter { .. } => {
                trace!("[INPUT] Pointer entered surface");
                // Pointer entered the surface
            }
            PointerEventKind::Leave { .. } => {
                trace!("[INPUT] Pointer left surface");
                // Pointer left the surface
                self.events.push(Event::PointerGone);
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = Pos2::new(x as f32, y as f32);
                trace!("[INPUT] Pointer moved to: ({}, {})", x, y);
                self.events.push(Event::PointerMoved(self.pointer_pos));
            }
            PointerEventKind::Press { button, .. } => {
                trace!("[INPUT] Pointer button pressed: {}", button);
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    trace!("[INPUT] Mapped to EGUI button: {:?}", egui_button);
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: true,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Release { button, .. } => {
                trace!("[INPUT] Pointer button released: {}", button);
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
                // Handle scroll events
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

    pub fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, is_repeat: bool) {
        trace!("[INPUT] Keyboard event - keysym: {:?}, raw_code: {}, pressed: {}, repeat: {}, utf8: {:?}", 
                 event.keysym.raw(), event.raw_code, pressed, is_repeat, event.utf8);
        
        // Check for clipboard operations BEFORE general key handling
        if pressed && !is_repeat && self.modifiers.ctrl {
            match event.keysym {
                Keysym::c => self.events.push(Event::Copy),
                Keysym::x => self.events.push(Event::Cut),
                Keysym::v => self.events.push(Event::Paste(self.clipboard.load().unwrap_or_default())),
                _ => (),
            }
        }

        if let Some(key) = keysym_to_egui_key(event.keysym) {
            trace!("[INPUT] Mapped to EGUI key: {:?}, repeat: {}", key, is_repeat);
            // Note: Egui expects repeats to have pressed=true
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: is_repeat,
                modifiers: self.modifiers,
            });
            if pressed || is_repeat {
                let text = event.utf8.clone().or(self.last_key_utf8.clone());
                if let Some(text) = text {
                    if !text.chars().any(|c| c.is_control()) {
                        trace!("[INPUT] Text input: '{}'", text);
                        self.events.push(Event::Text(text.clone()));
                    }
                }
            }
        } else {
            trace!("[INPUT] No EGUI key mapping for keysym: {:?}", event.keysym.raw());
        }

        if event.utf8.is_some() {
            self.last_key_utf8 = event.utf8.clone();
        }
    }

    pub fn update_modifiers(&mut self, wayland_mods: &WaylandModifiers) {
        trace!("[INPUT] Modifiers updated - ctrl: {}, shift: {}, alt: {}", 
                 wayland_mods.ctrl, wayland_mods.shift, wayland_mods.alt);
        self.modifiers = Modifiers {
            alt: wayland_mods.alt,
            ctrl: wayland_mods.ctrl,
            shift: wayland_mods.shift,
            mac_cmd: false, // Not applicable on Linux/Wayland
            command: wayland_mods.ctrl, // On non-Mac, command is ctrl
        };
    }

    /// Get current modifiers state
    // pub fn get_modifiers(&self) -> &Modifiers {
    //     &self.modifiers
    // }

    pub fn take_raw_input(&mut self) -> RawInput {
        let events = std::mem::take(&mut self.events);
        trace!("[INPUT] Taking raw input with {} events", events.len());
        if !events.is_empty() {
            trace!("[INPUT] Events: {:?}", events);
        }
        
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(self.screen_width as f32, self.screen_height as f32),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0, // Assume 60 FPS
            modifiers: self.modifiers,
            events,
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true, // Assume focused when we have the input
            ..Default::default()
        }
    }

    pub fn handle_output_command(&mut self, output: &egui::OutputCommand) {
        match output {
            egui::OutputCommand::CopyText(text) => {
                self.clipboard.store(text.clone());
                trace!("[INPUT] Copied text to clipboard: {:?}", text);
            },
            egui::OutputCommand::CopyImage(_image) => {
                // Handle image copy if needed
                trace!("[INPUT] CopyImage command received (not implemented)");
                // TODO: Implement image copying to clipboard if required
            },
            egui::OutputCommand::OpenUrl(url) => {
                trace!("[INPUT] OpenUrl command received: {}", url.url);
            },
        }
    }
}

fn wayland_button_to_egui(button: u32) -> Option<PointerButton> {
    // Linux button codes (from linux/input-event-codes.h)
    match button {
        0x110 => Some(PointerButton::Primary),   // BTN_LEFT
        0x111 => Some(PointerButton::Secondary), // BTN_RIGHT
        0x112 => Some(PointerButton::Middle),    // BTN_MIDDLE
        _ => None,
    }
}

fn keysym_to_egui_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Tab => Key::Tab,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Insert => Key::Insert,
        Keysym::Delete => Key::Delete,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Page_Up => Key::PageUp,
        Keysym::Page_Down => Key::PageDown,
        Keysym::Left => Key::ArrowLeft,
        Keysym::Right => Key::ArrowRight,
        Keysym::Up => Key::ArrowUp,
        Keysym::Down => Key::ArrowDown,
        
        Keysym::space => Key::Space,
        
        // Letters
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
        
        // Numbers
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
        
        // Function keys
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
        
        _ => return None,
    })
}