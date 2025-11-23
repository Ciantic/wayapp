use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym, Modifiers as WaylandModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_clipboard::Clipboard;
use std::time::Instant;

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
        println!("[INPUT] Pointer event: {:?}", event.kind);
        match &event.kind {
            PointerEventKind::Enter { .. } => {
                println!("[INPUT] Pointer entered surface");
                // Pointer entered the surface
            }
            PointerEventKind::Leave { .. } => {
                println!("[INPUT] Pointer left surface");
                // Pointer left the surface
                self.events.push(Event::PointerGone);
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = Pos2::new(x as f32, y as f32);
                println!("[INPUT] Pointer moved to: ({}, {})", x, y);
                self.events.push(Event::PointerMoved(self.pointer_pos));
            }
            PointerEventKind::Press { button, .. } => {
                println!("[INPUT] Pointer button pressed: {}", button);
                if let Some(egui_button) = wayland_button_to_egui(*button) {
                    println!("[INPUT] Mapped to EGUI button: {:?}", egui_button);
                    self.events.push(Event::PointerButton {
                        pos: self.pointer_pos,
                        button: egui_button,
                        pressed: true,
                        modifiers: self.modifiers,
                    });
                }
            }
            PointerEventKind::Release { button, .. } => {
                println!("[INPUT] Pointer button released: {}", button);
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
        println!("[INPUT] Keyboard event - keysym: {:?}, raw_code: {}, pressed: {}, repeat: {}, utf8: {:?}", 
                 event.keysym.raw(), event.raw_code, pressed, is_repeat, event.utf8);
        
        // Check for clipboard operations BEFORE general key handling
        if pressed && !is_repeat && self.modifiers.ctrl {
            // XKB key constants
            const XKB_KEY_c: u32 = 0x0063;
            const XKB_KEY_x: u32 = 0x0078;
            const XKB_KEY_v: u32 = 0x0076;

            match event.keysym.raw() {
                XKB_KEY_c => self.events.push(Event::Copy),
                XKB_KEY_x => self.events.push(Event::Cut),
                XKB_KEY_v => self.events.push(Event::Paste(self.clipboard.load().unwrap_or_default())),
                _ => (),
            }
        }

        if let Some(key) = keysym_to_egui_key(event.keysym) {
            println!("[INPUT] Mapped to EGUI key: {:?}, repeat: {}", key, is_repeat);
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
                        println!("[INPUT] Text input: '{}'", text);
                        self.events.push(Event::Text(text.clone()));
                    }
                }
            }
        } else {
            println!("[INPUT] No EGUI key mapping for keysym: {:?}", event.keysym.raw());
        }

        if event.utf8.is_some() {
            self.last_key_utf8 = event.utf8.clone();
        }
    }

    pub fn update_modifiers(&mut self, wayland_mods: &WaylandModifiers) {
        println!("[INPUT] Modifiers updated - ctrl: {}, shift: {}, alt: {}", 
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
        println!("[INPUT] Taking raw input with {} events", events.len());
        if !events.is_empty() {
            println!("[INPUT] Events: {:?}", events);
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
                println!("[INPUT] Copied text to clipboard: {:?}", text);
            },
            egui::OutputCommand::CopyImage(_image) => {
                // Handle image copy if needed
                println!("[INPUT] CopyImage command received (not implemented)");
                // TODO: Implement image copying to clipboard if required
            },
            egui::OutputCommand::OpenUrl(url) => {
                println!("[INPUT] OpenUrl command received: {}", url.url);
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
    // XKB key constants from xkbcommon
    const XKB_KEY_Escape: u32 = 0xff1b;
    const XKB_KEY_Return: u32 = 0xff0d;
    const XKB_KEY_KP_Enter: u32 = 0xff8d;
    const XKB_KEY_Tab: u32 = 0xff09;
    const XKB_KEY_BackSpace: u32 = 0xff08;
    const XKB_KEY_Insert: u32 = 0xff63;
    const XKB_KEY_Delete: u32 = 0xffff;
    const XKB_KEY_Home: u32 = 0xff50;
    const XKB_KEY_End: u32 = 0xff57;
    const XKB_KEY_Page_Up: u32 = 0xff55;
    const XKB_KEY_Page_Down: u32 = 0xff56;
    const XKB_KEY_Left: u32 = 0xff51;
    const XKB_KEY_Right: u32 = 0xff53;
    const XKB_KEY_Up: u32 = 0xff52;
    const XKB_KEY_Down: u32 = 0xff54;
    const XKB_KEY_space: u32 = 0x0020;
    
    const XKB_KEY_a: u32 = 0x0061;
    const XKB_KEY_b: u32 = 0x0062;
    const XKB_KEY_c: u32 = 0x0063;
    const XKB_KEY_d: u32 = 0x0064;
    const XKB_KEY_e: u32 = 0x0065;
    const XKB_KEY_f: u32 = 0x0066;
    const XKB_KEY_g: u32 = 0x0067;
    const XKB_KEY_h: u32 = 0x0068;
    const XKB_KEY_i: u32 = 0x0069;
    const XKB_KEY_j: u32 = 0x006a;
    const XKB_KEY_k: u32 = 0x006b;
    const XKB_KEY_l: u32 = 0x006c;
    const XKB_KEY_m: u32 = 0x006d;
    const XKB_KEY_n: u32 = 0x006e;
    const XKB_KEY_o: u32 = 0x006f;
    const XKB_KEY_p: u32 = 0x0070;
    const XKB_KEY_q: u32 = 0x0071;
    const XKB_KEY_r: u32 = 0x0072;
    const XKB_KEY_s: u32 = 0x0073;
    const XKB_KEY_t: u32 = 0x0074;
    const XKB_KEY_u: u32 = 0x0075;
    const XKB_KEY_v: u32 = 0x0076;
    const XKB_KEY_w: u32 = 0x0077;
    const XKB_KEY_x: u32 = 0x0078;
    const XKB_KEY_y: u32 = 0x0079;
    const XKB_KEY_z: u32 = 0x007a;
    
    const XKB_KEY_0: u32 = 0x0030;
    const XKB_KEY_1: u32 = 0x0031;
    const XKB_KEY_2: u32 = 0x0032;
    const XKB_KEY_3: u32 = 0x0033;
    const XKB_KEY_4: u32 = 0x0034;
    const XKB_KEY_5: u32 = 0x0035;
    const XKB_KEY_6: u32 = 0x0036;
    const XKB_KEY_7: u32 = 0x0037;
    const XKB_KEY_8: u32 = 0x0038;
    const XKB_KEY_9: u32 = 0x0039;
    
    const XKB_KEY_F1: u32 = 0xffbe;
    const XKB_KEY_F2: u32 = 0xffbf;
    const XKB_KEY_F3: u32 = 0xffc0;
    const XKB_KEY_F4: u32 = 0xffc1;
    const XKB_KEY_F5: u32 = 0xffc2;
    const XKB_KEY_F6: u32 = 0xffc3;
    const XKB_KEY_F7: u32 = 0xffc4;
    const XKB_KEY_F8: u32 = 0xffc5;
    const XKB_KEY_F9: u32 = 0xffc6;
    const XKB_KEY_F10: u32 = 0xffc7;
    const XKB_KEY_F11: u32 = 0xffc8;
    const XKB_KEY_F12: u32 = 0xffc9;
    
    Some(match keysym.raw() {
        XKB_KEY_Escape => Key::Escape,
        XKB_KEY_Return | XKB_KEY_KP_Enter => Key::Enter,
        XKB_KEY_Tab => Key::Tab,
        XKB_KEY_BackSpace => Key::Backspace,
        XKB_KEY_Insert => Key::Insert,
        XKB_KEY_Delete => Key::Delete,
        XKB_KEY_Home => Key::Home,
        XKB_KEY_End => Key::End,
        XKB_KEY_Page_Up => Key::PageUp,
        XKB_KEY_Page_Down => Key::PageDown,
        XKB_KEY_Left => Key::ArrowLeft,
        XKB_KEY_Right => Key::ArrowRight,
        XKB_KEY_Up => Key::ArrowUp,
        XKB_KEY_Down => Key::ArrowDown,
        
        XKB_KEY_space => Key::Space,
        
        // Letters (lowercase)
        XKB_KEY_a => Key::A,
        XKB_KEY_b => Key::B,
        XKB_KEY_c => Key::C,
        XKB_KEY_d => Key::D,
        XKB_KEY_e => Key::E,
        XKB_KEY_f => Key::F,
        XKB_KEY_g => Key::G,
        XKB_KEY_h => Key::H,
        XKB_KEY_i => Key::I,
        XKB_KEY_j => Key::J,
        XKB_KEY_k => Key::K,
        XKB_KEY_l => Key::L,
        XKB_KEY_m => Key::M,
        XKB_KEY_n => Key::N,
        XKB_KEY_o => Key::O,
        XKB_KEY_p => Key::P,
        XKB_KEY_q => Key::Q,
        XKB_KEY_r => Key::R,
        XKB_KEY_s => Key::S,
        XKB_KEY_t => Key::T,
        XKB_KEY_u => Key::U,
        XKB_KEY_v => Key::V,
        XKB_KEY_w => Key::W,
        XKB_KEY_x => Key::X,
        XKB_KEY_y => Key::Y,
        XKB_KEY_z => Key::Z,
        
        // Numbers
        XKB_KEY_0 => Key::Num0,
        XKB_KEY_1 => Key::Num1,
        XKB_KEY_2 => Key::Num2,
        XKB_KEY_3 => Key::Num3,
        XKB_KEY_4 => Key::Num4,
        XKB_KEY_5 => Key::Num5,
        XKB_KEY_6 => Key::Num6,
        XKB_KEY_7 => Key::Num7,
        XKB_KEY_8 => Key::Num8,
        XKB_KEY_9 => Key::Num9,
        
        // Function keys
        XKB_KEY_F1 => Key::F1,
        XKB_KEY_F2 => Key::F2,
        XKB_KEY_F3 => Key::F3,
        XKB_KEY_F4 => Key::F4,
        XKB_KEY_F5 => Key::F5,
        XKB_KEY_F6 => Key::F6,
        XKB_KEY_F7 => Key::F7,
        XKB_KEY_F8 => Key::F8,
        XKB_KEY_F9 => Key::F9,
        XKB_KEY_F10 => Key::F10,
        XKB_KEY_F11 => Key::F11,
        XKB_KEY_F12 => Key::F12,
        
        _ => return None,
    })
}