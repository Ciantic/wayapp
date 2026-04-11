//! EGL + glow based renderer for imgui on Wayland.
//!
//! Creates a `wl_egl_window` from the Wayland surface, sets up an OpenGL ES
//! 3.0 context via EGL, and drives `imgui_glow_renderer::AutoRenderer`.

use glow::HasContext;
use imgui::DrawData;
use khronos_egl as egl;
use std::ffi::c_void;
use std::sync::Arc;
use wayland_client::Connection;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_sys::client::wl_proxy;
use wayland_sys::egl::wl_egl_window;

pub struct ImguiGlowRenderer {
    egl: Arc<egl::DynamicInstance<egl::EGL1_4>>,
    egl_display: egl::Display,
    egl_context: egl::Context,
    egl_surface: egl::Surface,
    egl_window: *mut wl_egl_window,
    renderer: imgui_glow_renderer::AutoRenderer,
    width: u32,
    height: u32,
    scale_factor: i32,
}

// Safety: the EGL/GL resources are only ever used from the thread that created
// them (the main event loop), so this is sound in practice.
unsafe impl Send for ImguiGlowRenderer {}

impl ImguiGlowRenderer {
    pub fn new(
        wl_surface: &WlSurface,
        conn: &Connection,
        imgui: &mut imgui::Context,
        width: u32,
        height: u32,
        scale_factor: i32,
    ) -> Self {
        // ── Load EGL dynamically ──────────────────────────────────────────
        let egl_lib = unsafe {
            Arc::new(
                egl::DynamicInstance::<egl::EGL1_4>::load_required()
                    .expect("unable to load libEGL.so.1"),
            )
        };

        egl_lib
            .bind_api(egl::OPENGL_ES_API)
            .expect("unable to bind OpenGL ES API");

        // ── EGL display from the Wayland display pointer ──────────────────
        let display_ptr = conn.backend().display_ptr() as *mut c_void;
        let egl_display = unsafe {
            egl_lib
                .get_display(display_ptr)
                .expect("unable to get EGL display")
        };
        egl_lib
            .initialize(egl_display)
            .expect("unable to initialize EGL");

        // ── EGL config ────────────────────────────────────────────────────
        let config_attribs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES3_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::NONE,
        ];
        let egl_config = egl_lib
            .choose_first_config(egl_display, &config_attribs)
            .expect("unable to query EGL configs")
            .expect("no suitable EGL config found");

        // ── EGL context (OpenGL ES 3.0) ───────────────────────────────────
        let context_attribs = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ];
        let egl_context = egl_lib
            .create_context(egl_display, egl_config, None, &context_attribs)
            .expect("unable to create EGL context");

        // ── wl_egl_window (native Wayland EGL window) ────────────────────
        let physical_width = width.max(1) as i32 * scale_factor.max(1);
        let physical_height = height.max(1) as i32 * scale_factor.max(1);
        let egl_window = unsafe {
            (wayland_sys::egl::wayland_egl_handle().wl_egl_window_create)(
                wl_surface.id().as_ptr() as *mut wl_proxy,
                physical_width,
                physical_height,
            )
        };
        assert!(!egl_window.is_null(), "failed to create wl_egl_window");

        // ── EGL surface from wl_egl_window ────────────────────────────────
        let egl_surface = unsafe {
            egl_lib
                .create_window_surface(egl_display, egl_config, egl_window as *mut c_void, None)
                .expect("unable to create EGL window surface")
        };

        // Make the context current before creating glow / imgui objects.
        egl_lib
            .make_current(
                egl_display,
                Some(egl_surface),
                Some(egl_surface),
                Some(egl_context),
            )
            .expect("unable to make EGL context current");

        // Disable EGL's built-in vsync — on Wayland, frame pacing is driven by
        // the compositor via the wl_surface.frame callback.  Blocking inside
        // eglSwapBuffers would stall the event loop and delay input handling.
        egl_lib
            .swap_interval(egl_display, 0)
            .expect("unable to set EGL swap interval");

        // ── glow context ──────────────────────────────────────────────────
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                egl_lib
                    .get_proc_address(name.to_str().unwrap_or(""))
                    .map(|p| p as *const c_void)
                    .unwrap_or(std::ptr::null())
            })
        };

        // Upload default fonts before handing imgui to the renderer.
        imgui
            .fonts()
            .add_font(&[imgui::FontSource::DefaultFontData { config: None }]);

        // ── imgui-glow-renderer ───────────────────────────────────────────
        let renderer = imgui_glow_renderer::AutoRenderer::new(gl, imgui)
            .expect("failed to create imgui glow renderer");

        Self {
            egl: egl_lib,
            egl_display,
            egl_context,
            egl_surface,
            egl_window,
            renderer,
            width: width.max(1),
            height: height.max(1),
            scale_factor: scale_factor.max(1),
        }
    }

    /// Render one imgui frame then swap buffers.
    pub fn render(&mut self, draw_data: &DrawData, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);

        if width != self.width || height != self.height {
            self.resize(width, height);
        }

        self.egl
            .make_current(
                self.egl_display,
                Some(self.egl_surface),
                Some(self.egl_surface),
                Some(self.egl_context),
            )
            .expect("make_current failed");

        let gl = self.renderer.gl_context();
        let physical_width = width.max(1) as i32 * self.scale_factor.max(1);
        let physical_height = height.max(1) as i32 * self.scale_factor.max(1);
        unsafe {
            gl.viewport(0, 0, physical_width, physical_height);
            gl.clear_color(0.114, 0.118, 0.122, 1.0); // dark grey
            gl.clear(glow::COLOR_BUFFER_BIT);
        }

        self.renderer
            .render(draw_data)
            .expect("imgui render failed");

        self.egl
            .swap_buffers(self.egl_display, self.egl_surface)
            .expect("unable to swap EGL buffers");
    }

    /// Set the scale factor for converting logical to physical pixels.
    pub fn set_scale_factor(&mut self, scale_factor: i32) {
        self.scale_factor = scale_factor.max(1);
    }

    /// Resize the native EGL window (call on configure events).
    /// `width` and `height` are logical (surface) dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        self.width = width;
        self.height = height;
        let physical_width = width as i32 * self.scale_factor;
        let physical_height = height as i32 * self.scale_factor;
        unsafe {
            (wayland_sys::egl::wayland_egl_handle().wl_egl_window_resize)(
                self.egl_window,
                physical_width,
                physical_height,
                0,
                0,
            );
        }
    }
}

impl Drop for ImguiGlowRenderer {
    fn drop(&mut self) {
        // Detach context before destroying resources.
        let _ = self.egl.make_current(self.egl_display, None, None, None);
        let _ = self.egl.destroy_surface(self.egl_display, self.egl_surface);
        let _ = self.egl.destroy_context(self.egl_display, self.egl_context);
        unsafe {
            (wayland_sys::egl::wayland_egl_handle().wl_egl_window_destroy)(self.egl_window);
        }
        let _ = self.egl.terminate(self.egl_display);
    }
}
