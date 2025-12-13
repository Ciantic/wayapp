use iced::Point;
use iced::Size;
use iced::event::Event as IcedEvent;
use iced::keyboard::Key;
use iced::keyboard::Location;
use iced::keyboard::Modifiers as IcedModifiers;
use iced::keyboard::key::Code;
use iced::keyboard::key::Named;
use iced::keyboard::key::NativeCode;
use iced::keyboard::key::Physical;
use iced::mouse::Button as IcedMouseButton;
use iced::mouse::ScrollDelta;
use iced_core::window;
use log::trace;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::seat::keyboard::Modifiers as WaylandModifiers;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_clipboard::Clipboard;
use smol_str::SmolStr;
use std::time::Instant;

/// Handles input events from Wayland and converts them to Iced events
pub struct WaylandToIcedInput {
    modifiers: IcedModifiers,
    pointer_pos: (f64, f64),
    events: Vec<IcedEvent>,
    screen_width: u32,
    screen_height: u32,
    start_time: Instant,
    clipboard: Clipboard,
    last_key_utf8: Option<SmolStr>,
}

impl WaylandToIcedInput {
    pub fn new(clipboard: Clipboard) -> Self {
        Self {
            modifiers: IcedModifiers::default(),
            pointer_pos: (0.0, 0.0),
            events: vec![IcedEvent::Window(window::Event::Opened {
                position: None,
                size: Size::new(256.0, 256.0),
            })],
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

    pub fn set_pointer_position(&mut self, x: f64, y: f64) {
        self.pointer_pos = (x, y);
    }

    pub fn handle_pointer_event(&mut self, event: &PointerEvent) {
        trace!("[INPUT] Pointer event: {:?}", event.kind);
        match &event.kind {
            PointerEventKind::Enter { .. } => {
                trace!("[INPUT] Pointer entered surface");
                let (x, y) = event.position;
                self.pointer_pos = (x, y);
                self.events
                    .push(IcedEvent::Mouse(iced::mouse::Event::CursorEntered));
            }
            PointerEventKind::Leave { .. } => {
                trace!("[INPUT] Pointer left surface");
                self.events
                    .push(IcedEvent::Mouse(iced::mouse::Event::CursorLeft));
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = (x, y);
                trace!("[INPUT] Pointer moved to: ({}, {})", x, y);
                self.events
                    .push(IcedEvent::Mouse(iced::mouse::Event::CursorMoved {
                        position: Point::new(x as f32, y as f32),
                    }));
            }
            PointerEventKind::Press { button, .. } => {
                trace!("[INPUT] Pointer button pressed: {}", button);
                if let Some(iced_button) = wayland_button_to_iced(*button) {
                    trace!("[INPUT] Mapped to Iced button: {:?}", iced_button);
                    self.events
                        .push(IcedEvent::Mouse(iced::mouse::Event::ButtonPressed(
                            iced_button,
                        )));
                }
            }
            PointerEventKind::Release { button, .. } => {
                trace!("[INPUT] Pointer button released: {}", button);
                if let Some(iced_button) = wayland_button_to_iced(*button) {
                    self.events
                        .push(IcedEvent::Mouse(iced::mouse::Event::ButtonReleased(
                            iced_button,
                        )));
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                // Handle scroll events
                let scroll_delta = ScrollDelta::Lines {
                    x: horizontal.discrete as f32,
                    y: vertical.discrete as f32,
                };

                if horizontal.discrete != 0 || vertical.discrete != 0 {
                    self.events
                        .push(IcedEvent::Mouse(iced::mouse::Event::WheelScrolled {
                            delta: scroll_delta,
                        }));
                }
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<IcedEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn handle_keyboard_enter(&mut self) {
        trace!("[INPUT] Keyboard focus entered surface");
        self.events.push(IcedEvent::Window(window::Event::Focused));
    }

    pub fn handle_keyboard_leave(&mut self) {
        trace!("[INPUT] Keyboard focus left surface");
        self.events
            .push(IcedEvent::Window(window::Event::Unfocused));
    }

    pub fn handle_keyboard_event(&mut self, event: &KeyEvent, pressed: bool, is_repeat: bool) {
        let (key, location) = keysym_to_iced_key_and_loc(event.keysym);
        let physical_key = keysym_to_physical_key(event.keysym, event.raw_code);

        // For text input, use the current event's UTF-8 if available,
        // otherwise reuse the last UTF-8 for repeat events
        let mut text = event.utf8.as_ref().map(|s| SmolStr::new(s.as_str()));
        if is_repeat && text.is_none() {
            text = self.last_key_utf8.clone();
        }

        trace!(
            "[INPUT] Keyboard event: pressed={:?}, repeat={:?}, key={:?}, location={:?}, \
             text={:?}, modifiers={:?}, physical_key={:?}",
            pressed, is_repeat, key, location, text, self.modifiers, physical_key
        );

        if pressed {
            self.events
                .push(IcedEvent::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: key.clone(),
                    location,
                    modifiers: self.modifiers,
                    text: text.clone(),
                    modified_key: key,
                    physical_key,
                    repeat: is_repeat,
                }));
        } else {
            self.events
                .push(IcedEvent::Keyboard(iced::keyboard::Event::KeyReleased {
                    key: key.clone(),
                    location,
                    modifiers: self.modifiers,
                    physical_key,
                    modified_key: key,
                }));
        }

        // Cache the UTF-8 text for use in repeat events
        if event.utf8.is_some() {
            self.last_key_utf8 = event.utf8.as_ref().map(|s| SmolStr::new(s.as_str()));
        }
    }

    pub fn update_modifiers(&mut self, wayland_mods: &WaylandModifiers) {
        trace!(
            "[INPUT] Modifiers updated - ctrl: {}, shift: {}, alt: {}",
            wayland_mods.ctrl, wayland_mods.shift, wayland_mods.alt
        );
        let mut mods = IcedModifiers::empty();
        if wayland_mods.shift {
            mods |= IcedModifiers::SHIFT;
        }
        if wayland_mods.ctrl {
            mods |= IcedModifiers::CTRL;
        }
        if wayland_mods.alt {
            mods |= IcedModifiers::ALT;
        }
        self.modifiers = mods;

        // Generate ModifiersChanged event so ICED widgets can update their internal
        // state
        self.events.push(IcedEvent::Keyboard(
            iced::keyboard::Event::ModifiersChanged(mods),
        ));
    }

    pub fn get_modifiers(&self) -> IcedModifiers {
        self.modifiers
    }

    pub fn get_pointer_position(&self) -> (f64, f64) {
        self.pointer_pos
    }
}

fn keysym_to_iced_key(keysym: Keysym) -> Key {
    let named = match keysym {
        // TTY function keys
        Keysym::BackSpace => Named::Backspace,
        Keysym::Tab => Named::Tab,
        Keysym::Clear => Named::Clear,
        Keysym::Return => Named::Enter,
        Keysym::Pause => Named::Pause,
        Keysym::Scroll_Lock => Named::ScrollLock,
        Keysym::Sys_Req => Named::PrintScreen,
        Keysym::Escape => Named::Escape,
        Keysym::Delete => Named::Delete,

        // IME keys
        Keysym::Multi_key => Named::Compose,
        Keysym::Codeinput => Named::CodeInput,
        Keysym::SingleCandidate => Named::SingleCandidate,
        Keysym::MultipleCandidate => Named::AllCandidates,
        Keysym::PreviousCandidate => Named::PreviousCandidate,

        // Japanese key
        Keysym::Kanji => Named::KanjiMode,
        Keysym::Muhenkan => Named::NonConvert,
        Keysym::Henkan_Mode => Named::Convert,
        Keysym::Romaji => Named::Romaji,
        Keysym::Hiragana => Named::Hiragana,
        Keysym::Hiragana_Katakana => Named::HiraganaKatakana,
        Keysym::Zenkaku => Named::Zenkaku,
        Keysym::Hankaku => Named::Hankaku,
        Keysym::Zenkaku_Hankaku => Named::ZenkakuHankaku,
        Keysym::Kana_Lock => Named::KanaMode,
        Keysym::Kana_Shift => Named::KanaMode,
        Keysym::Eisu_Shift => Named::Alphanumeric,
        Keysym::Eisu_toggle => Named::Alphanumeric,

        // Cursor control & motion
        Keysym::Home => Named::Home,
        Keysym::Left => Named::ArrowLeft,
        Keysym::Up => Named::ArrowUp,
        Keysym::Right => Named::ArrowRight,
        Keysym::Down => Named::ArrowDown,
        Keysym::Page_Up => Named::PageUp,
        Keysym::Page_Down => Named::PageDown,
        Keysym::End => Named::End,

        // Misc. functions
        Keysym::Select => Named::Select,
        Keysym::Print => Named::PrintScreen,
        Keysym::Execute => Named::Execute,
        Keysym::Insert => Named::Insert,
        Keysym::Undo => Named::Undo,
        Keysym::Redo => Named::Redo,
        Keysym::Menu => Named::ContextMenu,
        Keysym::Find => Named::Find,
        Keysym::Cancel => Named::Cancel,
        Keysym::Help => Named::Help,
        Keysym::Break => Named::Pause,
        Keysym::Mode_switch => Named::ModeChange,
        Keysym::Num_Lock => Named::NumLock,

        // Keypad keys
        Keysym::KP_Tab => Named::Tab,
        Keysym::KP_Enter => Named::Enter,
        Keysym::KP_F1 => Named::F1,
        Keysym::KP_F2 => Named::F2,
        Keysym::KP_F3 => Named::F3,
        Keysym::KP_F4 => Named::F4,
        Keysym::KP_Home => Named::Home,
        Keysym::KP_Left => Named::ArrowLeft,
        Keysym::KP_Up => Named::ArrowUp,
        Keysym::KP_Right => Named::ArrowRight,
        Keysym::KP_Down => Named::ArrowDown,
        Keysym::KP_Page_Up => Named::PageUp,
        Keysym::KP_Page_Down => Named::PageDown,
        Keysym::KP_End => Named::End,
        Keysym::KP_Insert => Named::Insert,
        Keysym::KP_Delete => Named::Delete,

        // Function keys
        Keysym::F1 => Named::F1,
        Keysym::F2 => Named::F2,
        Keysym::F3 => Named::F3,
        Keysym::F4 => Named::F4,
        Keysym::F5 => Named::F5,
        Keysym::F6 => Named::F6,
        Keysym::F7 => Named::F7,
        Keysym::F8 => Named::F8,
        Keysym::F9 => Named::F9,
        Keysym::F10 => Named::F10,
        Keysym::F11 => Named::F11,
        Keysym::F12 => Named::F12,
        Keysym::F13 => Named::F13,
        Keysym::F14 => Named::F14,
        Keysym::F15 => Named::F15,
        Keysym::F16 => Named::F16,
        Keysym::F17 => Named::F17,
        Keysym::F18 => Named::F18,
        Keysym::F19 => Named::F19,
        Keysym::F20 => Named::F20,
        Keysym::F21 => Named::F21,
        Keysym::F22 => Named::F22,
        Keysym::F23 => Named::F23,
        Keysym::F24 => Named::F24,
        Keysym::F25 => Named::F25,
        Keysym::F26 => Named::F26,
        Keysym::F27 => Named::F27,
        Keysym::F28 => Named::F28,
        Keysym::F29 => Named::F29,
        Keysym::F30 => Named::F30,
        Keysym::F31 => Named::F31,
        Keysym::F32 => Named::F32,
        Keysym::F33 => Named::F33,
        Keysym::F34 => Named::F34,
        Keysym::F35 => Named::F35,

        // Modifiers
        Keysym::Shift_L => Named::Shift,
        Keysym::Shift_R => Named::Shift,
        Keysym::Control_L => Named::Control,
        Keysym::Control_R => Named::Control,
        Keysym::Caps_Lock => Named::CapsLock,
        Keysym::Alt_L => Named::Alt,
        Keysym::Alt_R => Named::Alt,
        Keysym::Super_L => Named::Super,
        Keysym::Super_R => Named::Super,
        Keysym::Hyper_L => Named::Hyper,
        Keysym::Hyper_R => Named::Hyper,

        // XKB function and modifier keys
        Keysym::ISO_Level3_Shift => Named::AltGraph,
        Keysym::ISO_Level3_Latch => Named::AltGraph,
        Keysym::ISO_Level3_Lock => Named::AltGraph,
        Keysym::ISO_Next_Group => Named::GroupNext,
        Keysym::ISO_Prev_Group => Named::GroupPrevious,
        Keysym::ISO_First_Group => Named::GroupFirst,
        Keysym::ISO_Last_Group => Named::GroupLast,
        Keysym::ISO_Left_Tab => Named::Tab,
        Keysym::ISO_Enter => Named::Enter,

        // 3270 terminal keys
        Keysym::_3270_EraseEOF => Named::EraseEof,
        // Keysym::_3270_Quit => Named::Quit, // Not available in current iced version
        Keysym::_3270_Attn => Named::Attn,
        Keysym::_3270_Play => Named::Play,
        Keysym::_3270_ExSelect => Named::ExSel,
        Keysym::_3270_CursorSelect => Named::CrSel,
        Keysym::_3270_PrintScreen => Named::PrintScreen,
        Keysym::_3270_Enter => Named::Enter,

        Keysym::space => Named::Space,

        // XFree86 - Backlight controls
        Keysym::XF86_MonBrightnessUp => Named::BrightnessUp,
        Keysym::XF86_MonBrightnessDown => Named::BrightnessDown,

        // XFree86 - "Internet"
        Keysym::XF86_Standby => Named::Standby,
        Keysym::XF86_AudioLowerVolume => Named::AudioVolumeDown,
        Keysym::XF86_AudioRaiseVolume => Named::AudioVolumeUp,
        Keysym::XF86_AudioPlay => Named::MediaPlay,
        Keysym::XF86_AudioStop => Named::MediaStop,
        Keysym::XF86_AudioPrev => Named::MediaTrackPrevious,
        Keysym::XF86_AudioNext => Named::MediaTrackNext,
        Keysym::XF86_HomePage => Named::BrowserHome,
        Keysym::XF86_Mail => Named::LaunchMail,
        Keysym::XF86_Search => Named::BrowserSearch,
        Keysym::XF86_AudioRecord => Named::MediaRecord,

        // XFree86 - PDA
        Keysym::XF86_Calculator => Named::LaunchApplication2,
        Keysym::XF86_Calendar => Named::LaunchCalendar,
        Keysym::XF86_PowerDown => Named::Power,

        // XFree86 - More "Internet"
        Keysym::XF86_Back => Named::BrowserBack,
        Keysym::XF86_Forward => Named::BrowserForward,
        Keysym::XF86_Refresh => Named::BrowserRefresh,
        Keysym::XF86_PowerOff => Named::Power,
        Keysym::XF86_WakeUp => Named::WakeUp,
        Keysym::XF86_Eject => Named::Eject,
        Keysym::XF86_ScreenSaver => Named::LaunchScreenSaver,
        Keysym::XF86_WWW => Named::LaunchWebBrowser,
        Keysym::XF86_Sleep => Named::Standby,
        Keysym::XF86_Favorites => Named::BrowserFavorites,
        Keysym::XF86_AudioPause => Named::MediaPause,
        Keysym::XF86_MyComputer => Named::LaunchApplication1,
        Keysym::XF86_AudioRewind => Named::MediaRewind,
        Keysym::XF86_Calculater => Named::LaunchApplication2, // libxkbcommon typo
        Keysym::XF86_Close => Named::Close,
        Keysym::XF86_Copy => Named::Copy,
        Keysym::XF86_Cut => Named::Cut,
        Keysym::XF86_Excel => Named::LaunchSpreadsheet,
        Keysym::XF86_LogOff => Named::LogOff,
        Keysym::XF86_MySites => Named::BrowserFavorites,
        Keysym::XF86_New => Named::New,
        Keysym::XF86_Open => Named::Open,
        Keysym::XF86_Paste => Named::Paste,
        Keysym::XF86_Phone => Named::LaunchPhone,
        Keysym::XF86_Reply => Named::MailReply,
        Keysym::XF86_Reload => Named::BrowserRefresh,
        Keysym::XF86_Save => Named::Save,
        Keysym::XF86_Send => Named::MailSend,
        Keysym::XF86_Spell => Named::SpellCheck,
        Keysym::XF86_SplitScreen => Named::SplitScreenToggle,
        Keysym::XF86_Video => Named::LaunchMediaPlayer,
        Keysym::XF86_Word => Named::LaunchWordProcessor,
        Keysym::XF86_ZoomIn => Named::ZoomIn,
        Keysym::XF86_ZoomOut => Named::ZoomOut,
        Keysym::XF86_WebCam => Named::LaunchWebCam,
        Keysym::XF86_MailForward => Named::MailForward,
        Keysym::XF86_Music => Named::LaunchMusicPlayer,
        Keysym::XF86_AudioForward => Named::MediaFastForward,
        Keysym::XF86_AudioRandomPlay => Named::RandomToggle,
        Keysym::XF86_Subtitle => Named::Subtitle,
        Keysym::XF86_AudioCycleTrack => Named::MediaAudioTrack,
        Keysym::XF86_Suspend => Named::Standby,
        Keysym::XF86_Hibernate => Named::Hibernate,
        Keysym::XF86_AudioMute => Named::AudioVolumeMute,
        Keysym::XF86_Next_VMode => Named::VideoModeNext,

        // Sun keyboard keys
        Keysym::SUN_Copy => Named::Copy,
        Keysym::SUN_Open => Named::Open,
        Keysym::SUN_Paste => Named::Paste,
        Keysym::SUN_Cut => Named::Cut,
        Keysym::SUN_AudioLowerVolume => Named::AudioVolumeDown,
        Keysym::SUN_AudioMute => Named::AudioVolumeMute,
        Keysym::SUN_AudioRaiseVolume => Named::AudioVolumeUp,
        Keysym::SUN_VideoLowerBrightness => Named::BrightnessDown,
        Keysym::SUN_VideoRaiseBrightness => Named::BrightnessUp,

        _ => return Key::Unidentified,
    };

    Key::Named(named)
}

fn keysym_location(keysym: Keysym) -> Location {
    match keysym {
        Keysym::Shift_L | Keysym::Control_L | Keysym::Alt_L | Keysym::Super_L | Keysym::Hyper_L => {
            Location::Left
        }
        Keysym::Shift_R | Keysym::Control_R | Keysym::Alt_R | Keysym::Super_R | Keysym::Hyper_R => {
            Location::Right
        }
        Keysym::KP_0
        | Keysym::KP_1
        | Keysym::KP_2
        | Keysym::KP_3
        | Keysym::KP_4
        | Keysym::KP_5
        | Keysym::KP_6
        | Keysym::KP_7
        | Keysym::KP_8
        | Keysym::KP_9
        | Keysym::KP_Space
        | Keysym::KP_Tab
        | Keysym::KP_Enter
        | Keysym::KP_F1
        | Keysym::KP_F2
        | Keysym::KP_F3
        | Keysym::KP_F4
        | Keysym::KP_Home
        | Keysym::KP_Left
        | Keysym::KP_Up
        | Keysym::KP_Right
        | Keysym::KP_Down
        | Keysym::KP_Page_Up
        | Keysym::KP_Page_Down
        | Keysym::KP_End
        | Keysym::KP_Begin
        | Keysym::KP_Insert
        | Keysym::KP_Delete
        | Keysym::KP_Equal
        | Keysym::KP_Multiply
        | Keysym::KP_Add
        | Keysym::KP_Separator
        | Keysym::KP_Subtract
        | Keysym::KP_Decimal
        | Keysym::KP_Divide => Location::Numpad,
        _ => Location::Standard,
    }
}

pub fn keysym_to_iced_key_and_loc(keysym: Keysym) -> (Key, Location) {
    let key = keysym_to_iced_key(keysym);
    let location = keysym_location(keysym);
    (key, location)
}

fn wayland_button_to_iced(button: u32) -> Option<IcedMouseButton> {
    // Linux button codes (from linux/input-event-codes.h)
    // BTN_LEFT = 0x110 = 272
    // BTN_RIGHT = 0x111 = 273
    // BTN_MIDDLE = 0x112 = 274
    match button {
        0x110 => Some(IcedMouseButton::Left),
        0x111 => Some(IcedMouseButton::Right),
        0x112 => Some(IcedMouseButton::Middle),
        _ => None,
    }
}

fn keysym_to_physical_key(keysym: Keysym, raw_code: u32) -> Physical {
    let code = match keysym {
        // Digit keys
        Keysym::_0 => Code::Digit0,
        Keysym::_1 => Code::Digit1,
        Keysym::_2 => Code::Digit2,
        Keysym::_3 => Code::Digit3,
        Keysym::_4 => Code::Digit4,
        Keysym::_5 => Code::Digit5,
        Keysym::_6 => Code::Digit6,
        Keysym::_7 => Code::Digit7,
        Keysym::_8 => Code::Digit8,
        Keysym::_9 => Code::Digit9,

        // Letter keys
        Keysym::a | Keysym::A => Code::KeyA,
        Keysym::b | Keysym::B => Code::KeyB,
        Keysym::c | Keysym::C => Code::KeyC,
        Keysym::d | Keysym::D => Code::KeyD,
        Keysym::e | Keysym::E => Code::KeyE,
        Keysym::f | Keysym::F => Code::KeyF,
        Keysym::g | Keysym::G => Code::KeyG,
        Keysym::h | Keysym::H => Code::KeyH,
        Keysym::i | Keysym::I => Code::KeyI,
        Keysym::j | Keysym::J => Code::KeyJ,
        Keysym::k | Keysym::K => Code::KeyK,
        Keysym::l | Keysym::L => Code::KeyL,
        Keysym::m | Keysym::M => Code::KeyM,
        Keysym::n | Keysym::N => Code::KeyN,
        Keysym::o | Keysym::O => Code::KeyO,
        Keysym::p | Keysym::P => Code::KeyP,
        Keysym::q | Keysym::Q => Code::KeyQ,
        Keysym::r | Keysym::R => Code::KeyR,
        Keysym::s | Keysym::S => Code::KeyS,
        Keysym::t | Keysym::T => Code::KeyT,
        Keysym::u | Keysym::U => Code::KeyU,
        Keysym::v | Keysym::V => Code::KeyV,
        Keysym::w | Keysym::W => Code::KeyW,
        Keysym::x | Keysym::X => Code::KeyX,
        Keysym::y | Keysym::Y => Code::KeyY,
        Keysym::z | Keysym::Z => Code::KeyZ,

        // Punctuation
        Keysym::grave => Code::Backquote,
        Keysym::minus => Code::Minus,
        Keysym::equal => Code::Equal,
        Keysym::bracketleft => Code::BracketLeft,
        Keysym::bracketright => Code::BracketRight,
        Keysym::backslash => Code::Backslash,
        Keysym::semicolon => Code::Semicolon,
        Keysym::apostrophe => Code::Quote,
        Keysym::comma => Code::Comma,
        Keysym::period => Code::Period,
        Keysym::slash => Code::Slash,

        // Whitespace and control
        Keysym::space => Code::Space,
        Keysym::Tab => Code::Tab,
        Keysym::Return => Code::Enter,
        Keysym::BackSpace => Code::Backspace,
        Keysym::Delete => Code::Delete,
        Keysym::Escape => Code::Escape,

        // Modifiers
        Keysym::Shift_L => Code::ShiftLeft,
        Keysym::Shift_R => Code::ShiftRight,
        Keysym::Control_L => Code::ControlLeft,
        Keysym::Control_R => Code::ControlRight,
        Keysym::Alt_L => Code::AltLeft,
        Keysym::Alt_R => Code::AltRight,
        Keysym::Super_L => Code::SuperLeft,
        Keysym::Super_R => Code::SuperRight,
        Keysym::Caps_Lock => Code::CapsLock,
        Keysym::Num_Lock => Code::NumLock,
        Keysym::Scroll_Lock => Code::ScrollLock,

        // Navigation
        Keysym::Home => Code::Home,
        Keysym::End => Code::End,
        Keysym::Page_Up => Code::PageUp,
        Keysym::Page_Down => Code::PageDown,
        Keysym::Left => Code::ArrowLeft,
        Keysym::Right => Code::ArrowRight,
        Keysym::Up => Code::ArrowUp,
        Keysym::Down => Code::ArrowDown,

        // Insert/Print
        Keysym::Insert => Code::Insert,
        Keysym::Print => Code::PrintScreen,
        Keysym::Sys_Req => Code::PrintScreen,

        // Function keys
        Keysym::F1 => Code::F1,
        Keysym::F2 => Code::F2,
        Keysym::F3 => Code::F3,
        Keysym::F4 => Code::F4,
        Keysym::F5 => Code::F5,
        Keysym::F6 => Code::F6,
        Keysym::F7 => Code::F7,
        Keysym::F8 => Code::F8,
        Keysym::F9 => Code::F9,
        Keysym::F10 => Code::F10,
        Keysym::F11 => Code::F11,
        Keysym::F12 => Code::F12,
        Keysym::F13 => Code::F13,
        Keysym::F14 => Code::F14,
        Keysym::F15 => Code::F15,
        Keysym::F16 => Code::F16,
        Keysym::F17 => Code::F17,
        Keysym::F18 => Code::F18,
        Keysym::F19 => Code::F19,
        Keysym::F20 => Code::F20,
        Keysym::F21 => Code::F21,
        Keysym::F22 => Code::F22,
        Keysym::F23 => Code::F23,
        Keysym::F24 => Code::F24,
        Keysym::F25 => Code::F25,
        Keysym::F26 => Code::F26,
        Keysym::F27 => Code::F27,
        Keysym::F28 => Code::F28,
        Keysym::F29 => Code::F29,
        Keysym::F30 => Code::F30,
        Keysym::F31 => Code::F31,
        Keysym::F32 => Code::F32,
        Keysym::F33 => Code::F33,
        Keysym::F34 => Code::F34,
        Keysym::F35 => Code::F35,

        // Keypad
        Keysym::KP_0 => Code::Numpad0,
        Keysym::KP_1 => Code::Numpad1,
        Keysym::KP_2 => Code::Numpad2,
        Keysym::KP_3 => Code::Numpad3,
        Keysym::KP_4 => Code::Numpad4,
        Keysym::KP_5 => Code::Numpad5,
        Keysym::KP_6 => Code::Numpad6,
        Keysym::KP_7 => Code::Numpad7,
        Keysym::KP_8 => Code::Numpad8,
        Keysym::KP_9 => Code::Numpad9,
        Keysym::KP_Decimal => Code::NumpadDecimal,
        Keysym::KP_Divide => Code::NumpadDivide,
        Keysym::KP_Multiply => Code::NumpadMultiply,
        Keysym::KP_Subtract => Code::NumpadSubtract,
        Keysym::KP_Add => Code::NumpadAdd,
        Keysym::KP_Enter => Code::NumpadEnter,
        Keysym::KP_Equal => Code::NumpadEqual,

        // Pause/Break
        Keysym::Pause => Code::Pause,
        Keysym::Break => Code::Pause,

        // Media keys
        Keysym::XF86_AudioMute => Code::AudioVolumeMute,
        Keysym::XF86_AudioLowerVolume => Code::AudioVolumeDown,
        Keysym::XF86_AudioRaiseVolume => Code::AudioVolumeUp,
        Keysym::XF86_AudioPlay => Code::MediaPlayPause,
        Keysym::XF86_AudioStop => Code::MediaStop,
        Keysym::XF86_AudioPrev => Code::MediaTrackPrevious,
        Keysym::XF86_AudioNext => Code::MediaTrackNext,

        // Browser keys
        Keysym::XF86_Back => Code::BrowserBack,
        Keysym::XF86_Forward => Code::BrowserForward,
        Keysym::XF86_Refresh => Code::BrowserRefresh,
        Keysym::XF86_Stop => Code::BrowserStop,
        Keysym::XF86_Search => Code::BrowserSearch,
        Keysym::XF86_HomePage => Code::BrowserHome,
        Keysym::XF86_Favorites => Code::BrowserFavorites,

        // Power/Sleep
        Keysym::XF86_PowerOff => Code::Power,
        Keysym::XF86_Sleep => Code::Sleep,
        Keysym::XF86_WakeUp => Code::WakeUp,

        // Application keys
        Keysym::XF86_Calculator => Code::LaunchApp2,
        Keysym::XF86_Mail => Code::LaunchMail,
        Keysym::XF86_MyComputer => Code::LaunchApp1,
        Keysym::XF86_Music => Code::MediaSelect,
        Keysym::XF86_Video => Code::LaunchApp2,

        // Edit keys
        Keysym::XF86_Copy => Code::Copy,
        Keysym::XF86_Cut => Code::Cut,
        Keysym::XF86_Paste => Code::Paste,

        // Japanese
        Keysym::Henkan_Mode => Code::Convert,
        Keysym::Muhenkan => Code::NonConvert,
        Keysym::Kanji => Code::Lang2,
        Keysym::Hiragana => Code::Lang4,
        Keysym::Katakana => Code::Lang3,

        // Context menu
        Keysym::Menu => Code::ContextMenu,

        // Everything else returns unidentified
        _ => return Physical::Unidentified(NativeCode::Xkb(raw_code)),
    };

    Physical::Code(code)
}
