//! EGUI view manager implementation
//!
//! This module provides a ViewManager-based approach to handling EGUI surfaces
//! following the pattern from single_color.rs

use egui::Event;
use egui::ImeEvent;
use egui::Key;
use egui::Modifiers as EguiModifiers;
use egui::PointerButton;
use egui::Pos2;
use egui::RawInput;
use log::trace;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::AxisScroll;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_clipboard::Clipboard;
use std::time::Instant;
use wayland_client::protocol::wl_pointer::AxisSource;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

/// Handles input events from Wayland and converts them to EGUI RawInput
pub struct WaylandToEguiInput {
    modifiers: EguiModifiers,
    pointer_pos: Pos2,
    events: Vec<Event>,
    screen_width: u32,
    screen_height: u32,
    start_time: Instant,
    predicted_dt: f32,
    clipboard: Clipboard,
    last_key_utf8: Option<String>,
    has_keyboard_focus: bool,
}

impl WaylandToEguiInput {
    pub fn new(clipboard: Clipboard, width: u32, height: u32, fps: f32) -> Self {
        Self {
            modifiers: EguiModifiers::default(),
            pointer_pos: Pos2::ZERO,
            events: Vec::new(),
            screen_width: width,
            screen_height: height,
            start_time: Instant::now(),
            predicted_dt: 1.0 / fps,
            clipboard,
            last_key_utf8: None,
            has_keyboard_focus: false,
        }
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Set the expected time between two frames in seconds
    pub fn set_predicted_dt(&mut self, predicted_dt: f32) {
        self.predicted_dt = predicted_dt;
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
                source,
                ..
            } => {
                if let Some(event) =
                    wayland_mouse_wheel_to_egui(horizontal, vertical, *source, self.modifiers)
                {
                    self.events.push(event);
                }
            }
        }
    }

    pub fn handle_keyboard_enter(&mut self) {
        self.has_keyboard_focus = true;
        self.events.push(Event::WindowFocused(true));
    }

    pub fn handle_keyboard_leave(&mut self) {
        self.has_keyboard_focus = false;
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
                modifiers: modifiers_for_keysym(event.keysym, self.modifiers),
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
            predicted_dt: self.predicted_dt,
            events,
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: self.has_keyboard_focus,
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
            egui::OutputCommand::TextSelectionSettled(str) => {
                trace!("[INPUT] TextSelectionSettled command received: {}", str);
            }
            egui::OutputCommand::OpenUrl(url) => {
                trace!("[INPUT] OpenUrl command received: {}", url.url);
            }
        }
    }

    pub fn handle_ime_enter(&mut self) {
        trace!("[INPUT] IME enabled");
        // ImeEvent::Enabled is deprecated and no longer used by egui
    }

    pub fn handle_ime_leave(&mut self) {
        trace!("[INPUT] IME disabled");
        // ImeEvent::Disabled is deprecated and no longer used by egui
    }

    pub fn handle_ime_commit(&mut self, text: &str) {
        trace!("[INPUT] Committing IME text: {}", text);
        self.events
            .push(Event::Ime(ImeEvent::Commit(text.to_string())));
    }

    pub fn handle_ime_preedit_string(&mut self, text: &str, _cursor_begin: i32, _cursor_end: i32) {
        trace!("[INPUT] IME preedit: {:?}", text);
        self.events.push(Event::Ime(ImeEvent::Preedit {
            text: text.to_string(),
            active_range_chars: None,
        }));
    }

    pub fn handle_ime_delete_surrounding_text(&mut self, before_length: u32, after_length: u32) {
        trace!(
            "[INPUT] IME delete surrounding text: before={} bytes, after={} bytes",
            before_length, after_length
        );
        // egui has no native DeleteSurroundingText event. Approximate with key
        // presses: divide byte counts by 3 (UTF-8 CJK chars are 3
        // bytes) to get character counts. (SLOP needs testing)

        // for _ in 0..before_length.div_ceil(3) {
        //     self.events.push(Event::Key {
        //         key: Key::Backspace,
        //         physical_key: None,
        //         pressed: true,
        //         repeat: false,
        //         modifiers: self.modifiers,
        //     });
        // }
        // for _ in 0..after_length.div_ceil(3) {
        //     self.events.push(Event::Key {
        //         key: Key::Delete,
        //         physical_key: None,
        //         pressed: true,
        //         repeat: false,
        //         modifiers: self.modifiers,
        //     });
        // }
    }
}

/// Converts a Wayland `wl_pointer.axis` event to an egui [`Event::MouseWheel`].
///
/// Returns `None` if the event contains no scroll delta and does not end a
/// gesture.
fn wayland_mouse_wheel_to_egui(
    horizontal: &AxisScroll,
    vertical: &AxisScroll,
    axis_source: Option<AxisSource>,
    modifiers: EguiModifiers,
) -> Option<Event> {
    // Wayland reports how the *view* moves (positive = down/right), while egui
    // reports how the *content* moves, so the sign is inverted.
    let (unit, delta) = if horizontal.value120 != 0 || vertical.value120 != 0 {
        // High-resolution wheel: 120 units per logical scroll step.
        (
            egui::MouseWheelUnit::Line,
            egui::vec2(
                horizontal.value120 as f32 / 120.0,
                vertical.value120 as f32 / 120.0,
            ),
        )
    } else if horizontal.discrete != 0 || vertical.discrete != 0 {
        // Deprecated discrete steps, still sent by older compositors.
        (
            egui::MouseWheelUnit::Line,
            egui::vec2(horizontal.discrete as f32, vertical.discrete as f32),
        )
    } else {
        // Continuous scrolling (touchpads), already in surface-local points.
        (
            egui::MouseWheelUnit::Point,
            egui::vec2(horizontal.absolute as f32, vertical.absolute as f32),
        )
    };
    let delta = -delta;

    let source = match (axis_source, unit) {
        (Some(AxisSource::Wheel | AxisSource::WheelTilt), _) => egui::MouseWheelSource::Wheel,
        (Some(AxisSource::Finger), _) => egui::MouseWheelSource::Trackpad,
        (_, egui::MouseWheelUnit::Line) => egui::MouseWheelSource::Wheel,
        _ => egui::MouseWheelSource::Unknown,
    };

    // `stop` ends a continuous scroll. Regular scroll events must use
    // `TouchPhase::Move`: egui discards the delta of a `TouchPhase::End`.
    let phase = if horizontal.stop || vertical.stop {
        egui::TouchPhase::End
    } else {
        egui::TouchPhase::Move
    };

    if delta == egui::Vec2::ZERO && phase != egui::TouchPhase::End {
        None
    } else {
        Some(Event::MouseWheel {
            unit,
            delta,
            phase,
            source,
            modifiers,
        })
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

fn modifiers_for_keysym(keysym: Keysym, mut modifiers: EguiModifiers) -> EguiModifiers {
    // In wayland ISO_Left_Tab equals Shift+Tab, this can't be detected from the
    // modifier state alone.
    if keysym == Keysym::ISO_Left_Tab {
        modifiers.shift = true;
    }
    modifiers
}

fn keysym_to_egui_key(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        // Commands:
        Keysym::downarrow | Keysym::Down | Keysym::KP_Down => Key::ArrowDown,
        Keysym::leftarrow | Keysym::Left | Keysym::KP_Left => Key::ArrowLeft,
        Keysym::rightarrow | Keysym::Right | Keysym::KP_Right => Key::ArrowRight,
        Keysym::uparrow | Keysym::Up | Keysym::KP_Up => Key::ArrowUp,
        Keysym::Escape => Key::Escape,
        Keysym::Tab | Keysym::ISO_Left_Tab | Keysym::KP_Tab => Key::Tab,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Return | Keysym::KP_Enter | Keysym::ISO_Enter => Key::Enter,
        Keysym::Insert | Keysym::KP_Insert => Key::Insert,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        // `KP_Page_Up`/`KP_Page_Down` are aliases of `KP_Prior`/`KP_Next` and
        // cannot be listed separately (duplicate match arm).
        Keysym::Prior | Keysym::KP_Prior => Key::PageUp,
        Keysym::Next | Keysym::KP_Next => Key::PageDown,
        // Punctuation:
        Keysym::space | Keysym::KP_Space => Key::Space,
        Keysym::colon => Key::Colon,
        Keysym::comma | Keysym::KP_Separator => Key::Comma,
        Keysym::minus | Keysym::KP_Subtract => Key::Minus,
        Keysym::period | Keysym::KP_Decimal => Key::Period,
        Keysym::plus | Keysym::KP_Add => Key::Plus,
        Keysym::equal | Keysym::KP_Equal => Key::Equals,
        Keysym::semicolon => Key::Semicolon,
        Keysym::bracketleft => Key::OpenBracket,
        Keysym::bracketright => Key::CloseBracket,
        Keysym::braceleft => Key::OpenCurlyBracket,
        Keysym::braceright => Key::CloseCurlyBracket,
        Keysym::grave => Key::Backtick,
        Keysym::backslash => Key::Backslash,
        Keysym::slash | Keysym::KP_Divide => Key::Slash,
        Keysym::bar => Key::Pipe,
        Keysym::question => Key::Questionmark,
        Keysym::exclam => Key::Exclamationmark,
        Keysym::apostrophe => Key::Quote,
        // Digits:
        Keysym::_0 | Keysym::KP_0 => Key::Num0,
        Keysym::_1 | Keysym::KP_1 => Key::Num1,
        Keysym::_2 | Keysym::KP_2 => Key::Num2,
        Keysym::_3 | Keysym::KP_3 => Key::Num3,
        Keysym::_4 | Keysym::KP_4 => Key::Num4,
        Keysym::_5 | Keysym::KP_5 => Key::Num5,
        Keysym::_6 | Keysym::KP_6 => Key::Num6,
        Keysym::_7 | Keysym::KP_7 => Key::Num7,
        Keysym::_8 | Keysym::KP_8 => Key::Num8,
        Keysym::_9 | Keysym::KP_9 => Key::Num9,
        // Letters: shift produces the uppercase keysym for the same key.
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
        // Lock / system keys:
        Keysym::Caps_Lock => Key::CapsLock,
        Keysym::Num_Lock => Key::NumLock,
        Keysym::Scroll_Lock => Key::ScrollLock,
        Keysym::Print | Keysym::Sys_Req => Key::PrintScreen,
        Keysym::Pause | Keysym::Break => Key::Pause,
        Keysym::Menu => Key::Menu,
        Keysym::Help => Key::Help,
        Keysym::Clear => Key::Clear,
        Keysym::XF86_PowerOff => Key::Power,
        Keysym::XF86_Sleep | Keysym::XF86_Suspend | Keysym::XF86_Standby => Key::Sleep,
        Keysym::XF86_Eject => Key::Eject,
        // Modifier keys are reported as ordinary key presses too, so that
        // input-capture UIs can bind them. egui derives `Modifiers` from
        // `RawInput::modifiers`, not from these.
        Keysym::Shift_L => Key::ShiftLeft,
        Keysym::Shift_R => Key::ShiftRight,
        Keysym::Control_L => Key::ControlLeft,
        Keysym::Control_R => Key::ControlRight,
        Keysym::Alt_L => Key::AltLeft,
        // AltGr is reported as level-3 shift rather than as `Alt_R`.
        Keysym::Alt_R | Keysym::ISO_Level3_Shift => Key::AltRight,
        Keysym::Super_L => Key::SuperLeft,
        Keysym::Super_R => Key::SuperRight,
        // Browser / media / launch keys:
        Keysym::XF86_Back => Key::BrowserBack,
        Keysym::XF86_Forward => Key::BrowserForward,
        Keysym::XF86_Refresh => Key::BrowserRefresh,
        Keysym::XF86_Search => Key::BrowserSearch,
        Keysym::XF86_HomePage => Key::BrowserHome,
        Keysym::XF86_Favorites => Key::BrowserFavorites,
        Keysym::XF86_Stop => Key::BrowserStop,
        Keysym::XF86_AudioPlay | Keysym::XF86_AudioPause => Key::MediaPlayPause,
        Keysym::XF86_AudioNext => Key::MediaTrackNext,
        Keysym::XF86_AudioPrev => Key::MediaTrackPrevious,
        Keysym::XF86_AudioStop => Key::MediaStop,
        Keysym::XF86_AudioMute => Key::AudioVolumeMute,
        Keysym::XF86_AudioLowerVolume => Key::AudioVolumeDown,
        Keysym::XF86_AudioRaiseVolume => Key::AudioVolumeUp,
        Keysym::XF86_Mail => Key::LaunchMail,
        Keysym::XF86_Launch1 | Keysym::XF86_Explorer => Key::LaunchApp1,
        Keysym::XF86_Launch2 | Keysym::XF86_Calculator => Key::LaunchApp2,
        // Function keys:
        Keysym::F1 | Keysym::KP_F1 => Key::F1,
        Keysym::F2 | Keysym::KP_F2 => Key::F2,
        Keysym::F3 | Keysym::KP_F3 => Key::F3,
        Keysym::F4 | Keysym::KP_F4 => Key::F4,
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
