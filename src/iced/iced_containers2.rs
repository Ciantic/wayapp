// Second attempt at iced containers, using a different approach than
// iced_containers.rs
use crate::Application;
use crate::BaseTrait;
use crate::CompositorHandlerContainer;
use crate::KeyboardHandlerContainer;
use crate::LayerSurfaceContainer;
use crate::PointerHandlerContainer;
use crate::PopupContainer;
use crate::SubsurfaceContainer;
use crate::WaylandToIcedInput;
use crate::WindowContainer;
use crate::get_app;
use iced::Color;
use iced::Font;
use iced::Pixels;
use iced::Size;
use iced::Theme;
use iced::keyboard::Modifiers;
use iced::mouse;
use iced::theme;
use iced::window;
use iced_core::Event;
use iced_core::renderer::Style;
use iced_core::window::Event::RedrawRequested;
use iced_graphics::Viewport;
use iced_renderer::Renderer;
use iced_runtime::user_interface;
use iced_wgpu::Engine;
use log::trace;
use pollster::block_on;
// use raw_window_handle::RawDisplayHandle;
// use raw_window_handle::RawWindowHandle;
// use raw_window_handle::WaylandDisplayHandle;
// use raw_window_handle::WaylandWindowHandle;
// use smithay_client_toolkit::seat::keyboard::KeyEvent;
// use smithay_client_toolkit::seat::keyboard::Modifiers;
// use smithay_client_toolkit::seat::pointer::PointerEvent;
// use smithay_client_toolkit::shell::WaylandSurface;
// use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
// use smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure;
// use smithay_client_toolkit::shell::xdg::popup::Popup;
// use smithay_client_toolkit::shell::xdg::popup::PopupConfigure;
// use smithay_client_toolkit::shell::xdg::window::Window;
// use smithay_client_toolkit::shell::xdg::window::WindowConfigure;
// use smithay_clipboard::Clipboard;
// use std::ptr::NonNull;
// use wayland_client::Proxy;
// use wayland_client::QueueHandle;
// use wayland_client::protocol::wl_surface::WlSurface;
// use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

struct IcedSurfaceState2<'a, P: iced_program::Program>
where
    P::Theme: theme::Base,
{
    title: String,
    scale_factor: f32,
    viewport: Viewport,
    surface_version: u64,
    cursor_position: Option<(f32, f32)>,
    smithay_modifiers: Modifiers,
    theme: Option<P::Theme>,
    theme_mode: theme::Mode,
    default_theme: P::Theme,
    style: theme::Style,
    // renderer: P::Renderer,
    user_interface: Option<user_interface::UserInterface<'a, P::Message, P::Theme, P::Renderer>>,
}

impl<'a, P: iced_program::Program> IcedSurfaceState2<'a, P>
where
    P::Theme: theme::Base,
{
    pub fn new(
        program: &'a iced_program::Instance<P>,
        system_theme: theme::Mode,
        window_id: window::Id,
        initial_size: Size<u32>,
        renderer: &mut P::Renderer,
    ) -> Self {
        let title = program.title(window_id);
        let scale_factor = program.scale_factor(window_id);
        let theme = program.theme(window_id);
        let theme_mode = theme.as_ref().map(theme::Base::mode).unwrap_or_default();
        let default_theme = <P::Theme as theme::Base>::default(system_theme);
        let style = program.style(theme.as_ref().unwrap_or(&default_theme));

        let viewport = { Viewport::with_physical_size(initial_size, 1 as f32 * scale_factor) };
        let cache = user_interface::Cache::default();
        let user_interface = user_interface::UserInterface::build(
            program.view(window_id),
            Size::new(initial_size.width as f32, initial_size.height as f32),
            cache,
            renderer,
        );

        Self {
            title,
            scale_factor,
            viewport,
            surface_version: 0,
            cursor_position: None,
            smithay_modifiers: Modifiers::default(),
            theme,
            theme_mode,
            default_theme,
            style,
            user_interface: Some(user_interface),
        }
    }

    pub fn rebuild_ui(
        &mut self,
        program: &'a iced_program::Instance<P>,
        window_id: window::Id,
        renderer: &mut P::Renderer,
    ) {
        let new_view = program.view(window_id);
        let size = self.viewport.logical_size();
        let cache = self
            .user_interface
            .take()
            .expect("User interface should be present")
            .into_cache();
        self.user_interface = Some(user_interface::UserInterface::build(
            new_view,
            Size::new(size.width as f32, size.height as f32),
            cache,
            renderer,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_init_app;
    use iced::Center;
    use iced::widget::Column;
    use iced::widget::button;
    use iced::widget::column;
    use iced::widget::text;
    use iced_core::widget::Tree;
    use raw_window_handle::RawDisplayHandle;
    use raw_window_handle::RawWindowHandle;
    use raw_window_handle::WaylandDisplayHandle;
    use raw_window_handle::WaylandWindowHandle;
    use std::ptr::NonNull;
    use wayland_client::Proxy;
    use wayland_client::protocol::wl_surface;

    #[derive(Default)]
    struct Counter {
        value: i64,
    }

    #[derive(Debug, Clone, Copy)]
    enum Message {
        Increment,
        Decrement,
    }

    impl Counter {
        fn update(&mut self, message: Message) {
            match message {
                Message::Increment => {
                    self.value += 1;
                }
                Message::Decrement => {
                    self.value -= 1;
                }
            }
        }

        fn view(&self) -> Column<'_, Message> {
            column![
                button("Increment").on_press(Message::Increment),
                text(self.value).size(50),
                button("Decrement").on_press(Message::Decrement)
            ]
            .padding(20)
            .align_x(Center)
        }
    }

    #[test]
    fn test_counter_view() {
        // Implement `iced_program::Program` for `Counter` so we can create
        // an `iced_program::Instance` and pass it to `IcedSurfaceState2::new`.
        impl iced_program::Program for Counter {
            type Executor = iced::executor::Default;
            type Message = Message;
            type Renderer = Renderer;
            type State = Counter;
            type Theme = theme::Theme;

            fn name() -> &'static str {
                "Counter"
            }

            fn settings(&self) -> iced::Settings {
                iced::Settings::default()
            }

            fn window(&self) -> Option<iced_core::window::Settings> {
                Some(iced_core::window::Settings::default())
            }

            fn boot(&self) -> (Self::State, iced::Task<Self::Message>) {
                (Counter::default(), iced::Task::none())
            }

            fn update(
                &self,
                state: &mut Self::State,
                message: Self::Message,
            ) -> iced::Task<Self::Message> {
                state.update(message);
                iced::Task::none()
            }

            fn view<'a>(
                &self,
                state: &'a Self::State,
                window_id: iced_core::window::Id,
            ) -> iced_core::Element<'a, Self::Message, Self::Theme, Self::Renderer> {
                state.view().into()
            }
        }
        let app = get_init_app();
        let counter = Counter::default();

        let wl_surface = app.compositor_state.create_surface(&app.qh);

        let (mut instance_iced, _task_message) = iced_program::Instance::new(counter);
        let window_id = window::Id::unique();

        let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(app.conn.backend().display_ptr() as *mut _)
                .expect("Wayland display pointer was null"),
        ));
        let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(wl_surface.id().as_ptr() as *mut _)
                .expect("Wayland surface handle was null"),
        ));

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle,
                    raw_window_handle,
                })
                .expect("Failed to create WGPU surface")
        };

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("Failed to find a suitable adapter");

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("Failed to request WGPU device");

        let caps = surface.get_capabilities(&adapter);
        let output_format = *caps
            .formats
            .get(0)
            .unwrap_or(&wgpu::TextureFormat::Bgra8Unorm);

        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue.clone(),
            output_format,
            Some(iced_graphics::Antialiasing::MSAAx4),
            iced_graphics::Shell::headless(),
        );

        let wgpu_renderer = iced_wgpu::Renderer::new(engine.clone(), Font::DEFAULT, Pixels(16.0));
        let mut renderer = iced_renderer::Renderer::Primary(wgpu_renderer);

        let mut state = IcedSurfaceState2::new(
            &instance_iced,
            theme::Mode::Light,
            window_id,
            Size::new(800, 600),
            &mut renderer,
        );

        state.rebuild_ui(&instance_iced, window_id, &mut renderer);

        instance_iced.view(window_id);

        state.rebuild_ui(&instance_iced, window_id, &mut renderer);

        // Trigger an increment message on the `Instance` and ignore the returned
        // `Task`.

        // Call `view` to exercise the rendering path after the update.
        let root = instance_iced.view(window_id);
    }
}
