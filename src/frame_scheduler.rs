use crate::WaylandEventEmitter;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

/// Schedules frame updates based on egui's repaint requests.
pub(crate) struct FrameScheduler {
    #[allow(dead_code)]
    thread: std::thread::JoinHandle<()>,
    next_frame_time: Arc<Mutex<Option<Instant>>>,
    frame_time_changed: Arc<Condvar>,
}

impl FrameScheduler {
    pub fn new(
        event_emitter: WaylandEventEmitter,
        wl_surface: wayland_client::protocol::wl_surface::WlSurface,
    ) -> Self {
        let next_frame_time = Arc::new(Mutex::new(None::<Instant>));
        let frame_time_changed = Arc::new(Condvar::new());
        let next_frame_time_thread = next_frame_time.clone();
        let frame_time_changed_thread = frame_time_changed.clone();

        FrameScheduler {
            thread: std::thread::spawn(move || {
                loop {
                    let mut next = next_frame_time_thread.lock().unwrap();

                    // Wait for a frame time to be set
                    next = frame_time_changed_thread
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
                            let mut next = next_frame_time_thread.lock().unwrap();
                            *next = None;
                            break;
                        }

                        // Sleep with timeout until deadline, but wake if notified of earlier
                        // deadline
                        let timeout = deadline - now;
                        let (new_next, _) = frame_time_changed_thread
                            .wait_timeout(next, timeout)
                            .unwrap();
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
            next_frame_time,
            frame_time_changed,
        }
    }

    pub fn create_scheduler(&self) -> impl Fn(Duration) + Send + Sync + 'static {
        let next_frame_time = self.next_frame_time.clone();
        let frame_time_changed = self.frame_time_changed.clone();
        move |delay: Duration| {
            Self::schedule_frame_at(&next_frame_time, &frame_time_changed, delay);
        }
    }

    /// Internal method to schedule a frame at a specific duration from now.
    pub fn schedule_frame_at(
        next_frame_time: &Arc<Mutex<Option<Instant>>>,
        frame_time_changed: &Arc<Condvar>,
        delay: Duration,
    ) {
        let min_delay = std::time::Duration::from_nanos(16_666_666); // ~60 FPS
        let delay = if delay < min_delay { min_delay } else { delay };
        let deadline = Instant::now() + delay;
        let mut next = next_frame_time.lock().unwrap();
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
            frame_time_changed.notify_all();
        }
    }

    /// Schedule a frame update at the specified duration from now.
    #[allow(dead_code)]
    pub fn schedule_frame(&mut self, at: Duration) {
        Self::schedule_frame_at(&self.next_frame_time, &self.frame_time_changed, at);
    }
}
