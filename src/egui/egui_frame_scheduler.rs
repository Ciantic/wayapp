use crate::WaylandEventEmitter;
use egui::Context;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Instant;

/// Schedules frame updates based on egui's repaint requests.
pub(crate) struct EguiFrameScheduler {
    #[allow(dead_code)]
    thread: std::thread::JoinHandle<()>,
}

impl EguiFrameScheduler {
    pub fn new(
        context: &Context,
        event_emitter: WaylandEventEmitter,
        wl_surface: wayland_client::protocol::wl_surface::WlSurface,
    ) -> Self {
        let next_frame_time = Arc::new(Mutex::new(None::<Instant>));
        let frame_time_changed = Arc::new(Condvar::new());
        let next_frame_time_clone = next_frame_time.clone();
        let frame_time_changed_clone = frame_time_changed.clone();

        context.set_request_repaint_callback(move |info| {
            let min_delay = std::time::Duration::from_nanos(16_666_666); // ~60 FPS
            let delay = if info.delay < min_delay {
                min_delay
            } else {
                info.delay
            };
            let deadline = Instant::now() + delay;
            let mut next = next_frame_time_clone.lock().unwrap();
            let should_notify = match *next {
                None => true,
                Some(prev) => deadline < prev,
            };
            *next = Some(match *next {
                None => deadline,
                Some(prev) => prev.min(deadline),
            });
            drop(next);
            if should_notify {
                frame_time_changed_clone.notify_all();
            }
        });

        EguiFrameScheduler {
            thread: std::thread::spawn(move || {
                loop {
                    let mut next = next_frame_time.lock().unwrap();

                    // Wait for a frame time to be set
                    next = frame_time_changed
                        .wait_while(next, |next| next.is_none())
                        .unwrap();

                    loop {
                        let deadline = next.unwrap();
                        let now = Instant::now();

                        if now >= deadline {
                            // Deadline has passed, emit the event
                            drop(next);

                            // Note: Using wl_surface.frame(), wl_surface.commit(), conn.flush()
                            // caused crashes with WGPU handling, so I created a way to emit Frame
                            // event without Wayland dispatching.
                            event_emitter.emit_events(vec![crate::WaylandEvent::Frame(
                                wl_surface.clone(),
                                0,
                            )]);

                            // Clear the frame time after emitting
                            let mut next = next_frame_time.lock().unwrap();
                            *next = None;
                            break;
                        }

                        // Sleep with timeout until deadline, but wake if notified of earlier
                        // deadline
                        let timeout = deadline - now;
                        let (new_next, _) = frame_time_changed.wait_timeout(next, timeout).unwrap();
                        next = new_next;

                        // Check if a new earlier deadline was set
                        if let Some(new_deadline) = *next {
                            if new_deadline < deadline {
                                // Loop back to wait for the new earlier deadline
                                continue;
                            }
                        }
                    }
                }
            }),
        }
    }
}
