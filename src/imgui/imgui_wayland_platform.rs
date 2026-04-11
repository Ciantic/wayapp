use imgui::BackendFlags;
use imgui::Context;
use imgui::Io;
use imgui::Key;
use imgui::MouseButton;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use std::time::Instant;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

/// Wayland platform support for imgui-rs.
///
/// Translates Wayland / smithay-client-toolkit input events into imgui [`Io`]
/// calls, following the same approach as `imgui-winit-support`.
pub struct ImguiWaylandPlatform {
    hidpi_factor: f64,
    start_time: Instant,
    pointer_pos: [f32; 2],
}

impl ImguiWaylandPlatform {
    /// Initialize the platform and configure the imgui [`Context`].
    pub fn new(imgui: &mut Context) -> Self {
        let io = imgui.io_mut();
        io.backend_flags.insert(BackendFlags::HAS_MOUSE_CURSORS);
        io.backend_flags.insert(BackendFlags::HAS_SET_MOUSE_POS);
        imgui.set_platform_name(Some(format!(
            "imgui-wayapp-support {}",
            env!("CARGO_PKG_VERSION")
        )));
        Self {
            hidpi_factor: 1.0,
            start_time: Instant::now(),
            pointer_pos: [0.0, 0.0],
        }
    }

    /// Attach to a surface with the given surface (logical) size and DPI scale.
    pub fn attach_window(&mut self, io: &mut Io, width: u32, height: u32, scale_factor: f64) {
        self.hidpi_factor = scale_factor;
        io.display_framebuffer_scale = [scale_factor as f32, scale_factor as f32];
        io.display_size = [width as f32, height as f32];
    }

    /// Call once per frame before calling `Context::new_frame`.
    pub fn prepare_frame(&mut self, io: &mut Io) {
        io.update_delta_time(self.start_time.elapsed());
        self.start_time = Instant::now();
    }

    /// Returns the current DPI factor.
    pub fn hidpi_factor(&self) -> f64 {
        self.hidpi_factor
    }

    /// Handle a pointer event from smithay-client-toolkit.
    pub fn handle_pointer_event(&mut self, io: &mut Io, event: &PointerEvent) {
        match &event.kind {
            PointerEventKind::Enter { .. } => {}
            PointerEventKind::Leave { .. } => {
                // Move the cursor off-screen so imgui stops tracking it.
                io.add_mouse_pos_event([-f32::MAX, -f32::MAX]);
            }
            PointerEventKind::Motion { .. } => {
                // Wayland sends pointer coordinates in surface-local coordinates
                // (already accounted for scale), so use them directly.
                let (x, y) = event.position;
                self.pointer_pos = [x as f32, y as f32];
                io.add_mouse_pos_event(self.pointer_pos);
            }
            PointerEventKind::Press { button, .. } => {
                if let Some(mb) = wayland_button_to_imgui(*button) {
                    io.add_mouse_button_event(mb, true);
                }
            }
            PointerEventKind::Release { button, .. } => {
                if let Some(mb) = wayland_button_to_imgui(*button) {
                    io.add_mouse_button_event(mb, false);
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                // Prefer high-res value120 (120 per logical tick), fall back to
                // continuous absolute pixels (divide by ~10 to get lines), then
                // the deprecated discrete field. Vertical is negated because
                // Wayland positive = down but imgui positive = up.
                let scroll_x = if horizontal.value120 != 0 {
                    horizontal.value120 as f32 / 120.0
                } else if horizontal.absolute != 0.0 {
                    horizontal.absolute as f32 / 10.0
                } else {
                    horizontal.discrete as f32
                };
                let scroll_y = if vertical.value120 != 0 {
                    -(vertical.value120 as f32 / 120.0)
                } else if vertical.absolute != 0.0 {
                    -(vertical.absolute as f32 / 10.0)
                } else {
                    -(vertical.discrete as f32)
                };
                io.add_mouse_wheel_event([scroll_x, scroll_y]);
            }
        }
    }

    /// Handle keyboard focus gained.
    pub fn handle_keyboard_enter(&mut self, io: &mut Io) {
        io.add_focus_event(true);
    }

    /// Handle keyboard focus lost.
    pub fn handle_keyboard_leave(&mut self, io: &mut Io) {
        io.add_focus_event(false);
    }

    /// Handle a key press / release / repeat from smithay-client-toolkit.
    pub fn handle_keyboard_event(
        &mut self,
        io: &mut Io,
        event: &KeyEvent,
        pressed: bool,
        _is_repeat: bool,
    ) {
        // Feed modifier keys first.
        if let Some(key) = keysym_to_modifier_key(event.keysym) {
            io.add_key_event(key, pressed);
        }

        // Feed the main key.
        if let Some(key) = keysym_to_imgui_key(event.keysym) {
            io.add_key_event(key, pressed);
        }

        // Feed text input characters on press (imgui ignores control chars internally).
        if pressed {
            if let Some(ref text) = event.utf8 {
                for ch in text.chars() {
                    // Skip DEL and other C0/C1 control characters.
                    if !ch.is_control() {
                        io.add_input_character(ch);
                    }
                }
            }
        }
    }

    /// Handle modifier state changes from smithay-client-toolkit.
    pub fn update_modifiers(&mut self, io: &mut Io, mods: &WaylandModifiers) {
        io.add_key_event(Key::ModCtrl, mods.ctrl);
        io.add_key_event(Key::ModShift, mods.shift);
        io.add_key_event(Key::ModAlt, mods.alt);
    }

    /// Handle a surface configure / resize event.
    /// `width` and `height` are surface (logical) dimensions.
    pub fn handle_resize(&mut self, io: &mut Io, width: u32, height: u32) {
        io.display_size = [width as f32, height as f32];
    }

    /// Handle a DPI / scale-factor change.
    pub fn handle_scale_factor_changed(&mut self, io: &mut Io, scale_factor: f64) {
        self.hidpi_factor = scale_factor;
        io.display_framebuffer_scale = [scale_factor as f32, scale_factor as f32];
    }

    /// Returns the Wayland cursor shape that matches the current imgui mouse
    /// cursor, or `None` when imgui wants to draw its own cursor
    /// (`io.mouse_draw_cursor`).
    pub fn cursor_shape(&self, imgui: &Context) -> Option<Shape> {
        let io = imgui.io();
        if io.mouse_draw_cursor {
            return None;
        }
        imgui.mouse_cursor().map(imgui_cursor_to_shape)
    }
}

// ── button mapping ───────────────────────────────────────────────────────────

fn wayland_button_to_imgui(button: u32) -> Option<MouseButton> {
    // Linux evdev button codes (linux/input-event-codes.h)
    match button {
        0x110 => Some(MouseButton::Left),
        0x111 => Some(MouseButton::Right),
        0x112 => Some(MouseButton::Middle),
        0x113 => Some(MouseButton::Extra1),
        0x114 => Some(MouseButton::Extra2),
        _ => None,
    }
}

// ── cursor mapping ───────────────────────────────────────────────────────────

fn imgui_cursor_to_shape(cursor: imgui::MouseCursor) -> Shape {
    match cursor {
        imgui::MouseCursor::Arrow => Shape::Default,
        imgui::MouseCursor::TextInput => Shape::Text,
        imgui::MouseCursor::ResizeAll => Shape::Move,
        imgui::MouseCursor::ResizeNS => Shape::NsResize,
        imgui::MouseCursor::ResizeEW => Shape::EwResize,
        imgui::MouseCursor::ResizeNESW => Shape::NeswResize,
        imgui::MouseCursor::ResizeNWSE => Shape::NwseResize,
        imgui::MouseCursor::Hand => Shape::Grab,
        imgui::MouseCursor::NotAllowed => Shape::NotAllowed,
    }
}

// ── key mapping ──────────────────────────────────────────────────────────────

/// Returns the imgui modifier pseudo-key for a modifier keysym, if any.
fn keysym_to_modifier_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        Keysym::Control_L => Key::LeftCtrl,
        Keysym::Control_R => Key::RightCtrl,
        Keysym::Shift_L => Key::LeftShift,
        Keysym::Shift_R => Key::RightShift,
        Keysym::Alt_L => Key::LeftAlt,
        Keysym::Alt_R => Key::RightAlt,
        Keysym::Super_L => Key::LeftSuper,
        Keysym::Super_R => Key::RightSuper,
        _ => return None,
    })
}

/// Maps a smithay keysym to an imgui [`Key`].
fn keysym_to_imgui_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        // Navigation
        Keysym::Tab => Key::Tab,
        Keysym::leftarrow | Keysym::Left => Key::LeftArrow,
        Keysym::rightarrow | Keysym::Right => Key::RightArrow,
        Keysym::uparrow | Keysym::Up => Key::UpArrow,
        Keysym::downarrow | Keysym::Down => Key::DownArrow,
        Keysym::Prior => Key::PageUp,
        Keysym::Next => Key::PageDown,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Insert => Key::Insert,
        Keysym::Delete => Key::Delete,
        Keysym::BackSpace => Key::Backspace,
        Keysym::space => Key::Space,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Escape => Key::Escape,
        Keysym::Menu => Key::Menu,
        // Punctuation
        Keysym::apostrophe => Key::Apostrophe,
        Keysym::comma => Key::Comma,
        Keysym::minus => Key::Minus,
        Keysym::period => Key::Period,
        Keysym::slash => Key::Slash,
        Keysym::semicolon => Key::Semicolon,
        Keysym::equal => Key::Equal,
        Keysym::bracketleft => Key::LeftBracket,
        Keysym::backslash => Key::Backslash,
        Keysym::bracketright => Key::RightBracket,
        Keysym::grave => Key::GraveAccent,
        // Lock keys
        Keysym::Caps_Lock => Key::CapsLock,
        Keysym::Scroll_Lock => Key::ScrollLock,
        Keysym::Num_Lock => Key::NumLock,
        Keysym::Print => Key::PrintScreen,
        Keysym::Pause => Key::Pause,
        // Digits
        Keysym::_0 => Key::Alpha0,
        Keysym::_1 => Key::Alpha1,
        Keysym::_2 => Key::Alpha2,
        Keysym::_3 => Key::Alpha3,
        Keysym::_4 => Key::Alpha4,
        Keysym::_5 => Key::Alpha5,
        Keysym::_6 => Key::Alpha6,
        Keysym::_7 => Key::Alpha7,
        Keysym::_8 => Key::Alpha8,
        Keysym::_9 => Key::Alpha9,
        // Letters
        Keysym::a | Keysym::A => Key::A,
        Keysym::b | Keysym::B => Key::B,
        Keysym::c | Keysym::C => Key::C,
        Keysym::d | Keysym::D => Key::D,
        Keysym::e | Keysym::E => Key::E,
        Keysym::f | Keysym::F => Key::F,
        Keysym::g | Keysym::G => Key::G,
        Keysym::h | Keysym::H => Key::H,
        Keysym::i | Keysym::I => Key::I,
        Keysym::j | Keysym::J => Key::J,
        Keysym::k | Keysym::K => Key::K,
        Keysym::l | Keysym::L => Key::L,
        Keysym::m | Keysym::M => Key::M,
        Keysym::n | Keysym::N => Key::N,
        Keysym::o | Keysym::O => Key::O,
        Keysym::p | Keysym::P => Key::P,
        Keysym::q | Keysym::Q => Key::Q,
        Keysym::r | Keysym::R => Key::R,
        Keysym::s | Keysym::S => Key::S,
        Keysym::t | Keysym::T => Key::T,
        Keysym::u | Keysym::U => Key::U,
        Keysym::v | Keysym::V => Key::V,
        Keysym::w | Keysym::W => Key::W,
        Keysym::x | Keysym::X => Key::X,
        Keysym::y | Keysym::Y => Key::Y,
        Keysym::z | Keysym::Z => Key::Z,
        // Keypad
        Keysym::KP_0 => Key::Keypad0,
        Keysym::KP_1 => Key::Keypad1,
        Keysym::KP_2 => Key::Keypad2,
        Keysym::KP_3 => Key::Keypad3,
        Keysym::KP_4 => Key::Keypad4,
        Keysym::KP_5 => Key::Keypad5,
        Keysym::KP_6 => Key::Keypad6,
        Keysym::KP_7 => Key::Keypad7,
        Keysym::KP_8 => Key::Keypad8,
        Keysym::KP_9 => Key::Keypad9,
        Keysym::KP_Decimal => Key::KeypadDecimal,
        Keysym::KP_Divide => Key::KeypadDivide,
        Keysym::KP_Multiply => Key::KeypadMultiply,
        Keysym::KP_Subtract => Key::KeypadSubtract,
        Keysym::KP_Add => Key::KeypadAdd,
        Keysym::KP_Equal => Key::KeypadEqual,
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
        _ => return None,
    })
}
