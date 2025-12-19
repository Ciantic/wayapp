use crate::iced::iced_input_handler::keysym_to_iced_key_and_loc;
use crate::iced::iced_input_handler::keysym_to_physical_key;
use crate::iced::iced_input_handler::wayland_button_to_iced;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::KeyboardHandlerContainer;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::WindowContainer;
use iced::keyboard;
use iced::mouse;
use iced::touch;
use iced::window;
use iced::window::Id;
use iced_core::input_method;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::pointer::PointerEvent;
use smithay_client_toolkit::seat::pointer::PointerEventKind;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
use smithay_client_toolkit::shell::xdg::popup::Popup;
use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
use smithay_client_toolkit::shell::xdg::window::Window;
use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
use smol_str::SmolStr;
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;

pub enum Kind {
    WaylandWindow(Window),
    WaylandLayerSurface(LayerSurface),
    WaylandPopup(Popup),
    WaylandSubsurface,
}

pub struct IcedSurface {
    wl_surface: WlSurface,
    scale_factor: i32,
    width: u32,
    height: u32,
    kind: Kind,

    iced_window_id: Id,
    modifiers: iced::keyboard::Modifiers,
    last_key_utf8: Option<SmolStr>,
    pointer_pos: (f32, f32),
}
impl IcedSurface {
    fn process_iced_events(&mut self, events: &[iced_core::event::Event]) {
        todo!()
    }

    fn reconfigure(&mut self) {}
}
impl BaseTrait for IcedSurface {
    fn get_object_id(&self) -> ObjectId {
        self.wl_surface.id()
    }
}
impl CompositorHandlerContainer for IcedSurface {
    fn scale_factor_changed(&mut self, new_factor: i32) {
        self.scale_factor = new_factor;
        self.process_iced_events(&[iced::Event::Window(window::Event::Rescaled(
            self.scale_factor as f32,
        ))]);
        self.reconfigure();
    }

    fn transform_changed(
        &mut self,
        new_transform: &wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(&mut self, time: u32) {}

    fn surface_enter(&mut self, output: &wayland_client::protocol::wl_output::WlOutput) {
        // Monitor changed
    }

    fn surface_leave(&mut self, output: &wayland_client::protocol::wl_output::WlOutput) {
        // Monitor changed
    }
}
impl KeyboardHandlerContainer for IcedSurface {
    fn enter(&mut self) {
        self.process_iced_events(&[iced::Event::Window(window::Event::Focused)]);
    }

    fn leave(&mut self) {
        self.process_iced_events(&[iced::Event::Window(window::Event::Unfocused)]);
    }

    fn press_key(&mut self, event: &KeyEvent) {
        let (key, location) = keysym_to_iced_key_and_loc(event.keysym);
        let physical_key = keysym_to_physical_key(event.keysym, event.raw_code);
        let text = event.utf8.as_ref().map(|s| SmolStr::new(s.as_str()));

        if let Some(text) = &text {
            self.last_key_utf8 = Some(text.clone());
        }

        self.process_iced_events(&[iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            location,
            modifiers: self.modifiers,
            text,
            modified_key: key,
            physical_key,
            repeat: false,
        })]);
    }

    fn release_key(&mut self, event: &KeyEvent) {
        let (key, location) = keysym_to_iced_key_and_loc(event.keysym);
        let physical_key = keysym_to_physical_key(event.keysym, event.raw_code);

        self.process_iced_events(&[iced::Event::Keyboard(iced::keyboard::Event::KeyReleased {
            key: key.clone(),
            location,
            modifiers: self.modifiers,
            modified_key: key,
            physical_key,
        })]);
    }

    fn update_modifiers(&mut self, modifiers: &smithay_client_toolkit::seat::keyboard::Modifiers) {
        let mut mods = iced::keyboard::Modifiers::empty();
        if modifiers.shift {
            mods |= iced::keyboard::Modifiers::SHIFT;
        }
        if modifiers.ctrl {
            mods |= iced::keyboard::Modifiers::CTRL;
        }
        if modifiers.alt {
            mods |= iced::keyboard::Modifiers::ALT;
        }
        if modifiers.logo {
            mods |= iced::keyboard::Modifiers::LOGO;
        }
        self.modifiers = mods;

        self.process_iced_events(&[iced::Event::Keyboard(
            iced::keyboard::Event::ModifiersChanged(mods),
        )]);
    }

    fn repeat_key(&mut self, event: &KeyEvent) {
        let (key, location) = keysym_to_iced_key_and_loc(event.keysym);
        let physical_key = keysym_to_physical_key(event.keysym, event.raw_code);
        let text = event
            .utf8
            .as_ref()
            .map(|s| SmolStr::new(s.as_str()))
            .or_else(|| self.last_key_utf8.clone());

        self.process_iced_events(&[iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            location,
            modifiers: self.modifiers,
            text,
            modified_key: key,
            physical_key,
            repeat: true,
        })]);
    }
}
impl PointerHandlerContainer for IcedSurface {
    fn pointer_frame(&mut self, event: &PointerEvent) {
        let mut events = Vec::new();

        match &event.kind {
            PointerEventKind::Enter { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = (x as f32, y as f32);
                events.push(iced::Event::Mouse(iced::mouse::Event::CursorEntered));
                events.push(iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(x as f32, y as f32),
                }));
            }
            PointerEventKind::Leave { .. } => {
                events.push(iced::Event::Mouse(iced::mouse::Event::CursorLeft));
            }
            PointerEventKind::Motion { .. } => {
                let (x, y) = event.position;
                self.pointer_pos = (x as f32, y as f32);
                events.push(iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(x as f32, y as f32),
                }));
            }
            PointerEventKind::Press { button, .. } => {
                if let Some(iced_button) = wayland_button_to_iced(*button) {
                    events.push(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced_button,
                    )));
                }
            }
            PointerEventKind::Release { button, .. } => {
                if let Some(iced_button) = wayland_button_to_iced(*button) {
                    events.push(iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                        iced_button,
                    )));
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                let scroll_delta = iced::mouse::ScrollDelta::Lines {
                    x: horizontal.discrete as f32,
                    y: vertical.discrete as f32,
                };

                if horizontal.discrete != 0 || vertical.discrete != 0 {
                    events.push(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                        delta: scroll_delta,
                    }));
                }
            }
        }

        if !events.is_empty() {
            self.process_iced_events(&events);
        }
    }
}
impl WindowContainer for IcedSurface {
    fn configure(&mut self, configure: &WindowConfigure) {
        /*
            pub struct WindowConfigure {
                pub new_size: (Option<NonZero<u32>>, Option<NonZero<u32>>),
                pub suggested_bounds: Option<(u32, u32)>,
                pub decoration_mode: DecorationMode,
                pub state: WindowState,
                pub capabilities: WindowManagerCapabilities,
        }   */

        // Set new size
        if let (Some(width), Some(height)) = configure.new_size {
            self.width = width.get();
            self.height = height.get();
        }

        // TODO: Create ICED specific events related to WindowConfigure
        let mut events: Vec<iced::Event> = Vec::new();

        // Create Resized event if size changed
        if let (Some(width), Some(height)) = configure.new_size {
            events.push(iced::Event::Window(window::Event::Resized(
                iced::Size::new(width.get() as f32, height.get() as f32),
            )));
        }

        // Create Focused/Unfocused events based on ACTIVATED state
        if configure.is_activated() {
            events.push(iced::Event::Window(window::Event::Focused));
        } else {
            events.push(iced::Event::Window(window::Event::Unfocused));
        }

        self.reconfigure();
        self.process_iced_events(&events);
    }

    fn allowed_to_close(&self) -> bool {
        true
    }

    fn request_close(&mut self) {}
}
impl LayerSurfaceContainer for IcedSurface {
    fn configure(&mut self, configure: &LayerSurfaceConfigure) {}

    fn closed(&mut self) {}
}
impl PopupContainer for IcedSurface {
    fn configure(&mut self, configure: &PopupConfigure) {}

    fn done(&mut self) {}
}

// iced_core::event::Event
pub fn convert_iced_event_to_wayland_event(iced_event: &iced_core::event::Event) {
    match iced_event {
        iced::Event::Keyboard(event) => match event {
            keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                location,
                modifiers,
                text,
                repeat,
            } => todo!(),
            keyboard::Event::KeyReleased {
                key,
                modified_key,
                physical_key,
                location,
                modifiers,
            } => todo!(),
            keyboard::Event::ModifiersChanged(modifiers) => todo!(),
        },
        iced::Event::Mouse(event) => match event {
            mouse::Event::CursorEntered => todo!(),
            mouse::Event::CursorLeft => todo!(),
            mouse::Event::CursorMoved { position } => todo!(),
            mouse::Event::ButtonPressed(button) => todo!(),
            mouse::Event::ButtonReleased(button) => todo!(),
            mouse::Event::WheelScrolled { delta } => todo!(),
        },
        iced::Event::Window(event) => match event {
            window::Event::Opened { position, size } => todo!(),
            window::Event::Closed => todo!(),
            window::Event::Moved(point) => todo!(),
            window::Event::Resized(size) => todo!(),
            window::Event::Rescaled(scale_factor) => todo!(),
            window::Event::RedrawRequested(instant) => todo!(),
            window::Event::CloseRequested => todo!(),
            window::Event::Focused => todo!(),
            window::Event::Unfocused => todo!(),
            window::Event::FileHovered(path_buf) => todo!(),
            window::Event::FileDropped(path_buf) => todo!(),
            window::Event::FilesHoveredLeft => todo!(),
        },
        iced::Event::Touch(event) => match event {
            touch::Event::FingerPressed { id, position } => todo!(),
            touch::Event::FingerMoved { id, position } => todo!(),
            touch::Event::FingerLifted { id, position } => todo!(),
            touch::Event::FingerLost { id, position } => todo!(),
        },
        iced::Event::InputMethod(event) => match event {
            input_method::Event::Opened => todo!(),
            input_method::Event::Preedit(_, range) => todo!(),
            input_method::Event::Commit(_) => todo!(),
            input_method::Event::Closed => todo!(),
        },
    }
}
