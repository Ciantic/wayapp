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
    pub fn new(emit_frame: impl Fn() + Send + Sync + 'static) -> Self {
        let next_frame_time = Arc::new(Mutex::new(None::<Instant>));
        let frame_time_changed = Arc::new(Condvar::new());
        let next_frame_time_thread = next_frame_time.clone();
        let frame_time_changed_thread = frame_time_changed.clone();
        let emit_frame = Arc::new(emit_frame);

        FrameScheduler {
            thread: std::thread::spawn(move || {
                loop {
                    let mut next = next_frame_time_thread.lock().unwrap();

                    // Wait for a frame time to be set
                    next = frame_time_changed_thread
                        .wait_while(next, |next| next.is_none())
                        .unwrap();
                    let deadline = next.unwrap();
                    let now = Instant::now();
                    if now >= deadline {
                        // Time to emit a frame
                        *next = None;
                        drop(next);
                        emit_frame();
                        continue;
                    }

                    // Sleep with timeout until deadline, but wake if notified of earlier
                    // deadline
                    let timeout = deadline - now;
                    let (new_next, _) = frame_time_changed_thread
                        .wait_timeout(next, timeout)
                        .unwrap();
                    next = new_next;
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
        // let min_delay = std::time::Duration::from_nanos(3_333_333); // ~300 FPS
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
