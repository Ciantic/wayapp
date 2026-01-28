//! Frame scheduler for repaint requests
//!
//! Retains a next frame time and signals frame updates accordingly
//!
//! This is not supposed to hold many frame times, but instead only the next
//! frame time. This is how EGUI works.

// Note: Try not to edit this with LLMs, locking logic needs to be verified
// manually.

use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

enum FrameSchedulerSignal {
    NothingScheduled,
    ScheduleFrameAt(Instant),
    Exit,
}

/// Schedules frame updates based on egui's repaint requests.
pub(crate) struct FrameScheduler {
    #[allow(dead_code)]
    thread: Option<std::thread::JoinHandle<()>>,
    next_frame_time: Arc<Mutex<FrameSchedulerSignal>>,
    frame_time_changed: Arc<Condvar>,
    fps_target: Arc<Mutex<f32>>,
}

impl FrameScheduler {
    pub fn new(emit_frame: impl Fn() + Send + Sync + 'static) -> Self {
        let next_frame_time = Arc::new(Mutex::new(FrameSchedulerSignal::NothingScheduled));
        let frame_time_changed = Arc::new(Condvar::new());
        let next_frame_time_thread = next_frame_time.clone();
        let frame_time_changed_thread = frame_time_changed.clone();
        let emit_frame = Arc::new(emit_frame);

        FrameScheduler {
            thread: Some(std::thread::spawn(move || {
                loop {
                    let mut next = next_frame_time_thread.lock().unwrap();

                    // Wait for a next signal
                    next = frame_time_changed_thread
                        .wait_while(next, |next| {
                            matches!(next, FrameSchedulerSignal::NothingScheduled)
                        })
                        .unwrap();

                    let now = Instant::now();
                    let deadline = match &*next {
                        FrameSchedulerSignal::Exit => {
                            break;
                        }
                        FrameSchedulerSignal::ScheduleFrameAt(deadline) => {
                            if &now >= deadline {
                                // Time to emit a frame
                                *next = FrameSchedulerSignal::NothingScheduled;
                                drop(next);
                                emit_frame();
                                continue;
                            }
                            *deadline
                        }
                        FrameSchedulerSignal::NothingScheduled => {
                            // Should not happen due to wait_while above
                            continue;
                        }
                    };

                    // Sleep with timeout until deadline, but wake if notified of new signal
                    let timeout = deadline - now;
                    let _ = frame_time_changed_thread
                        .wait_timeout(next, timeout)
                        .unwrap();

                    // Either timeout expired or a new earlier frame time was
                    // set, check on next iteration which
                    // one it was
                }
            })),
            fps_target: Arc::new(Mutex::new(60.0)),
            next_frame_time,
            frame_time_changed,
        }
    }

    /// Set the target FPS for frame scheduling
    ///
    /// If set zero, the value is clamped to a very low value (0.0001) to avoid
    /// division by zero.
    pub fn set_fps_target(&mut self, fps: f32) {
        let mut fps_target = self.fps_target.lock().unwrap();
        *fps_target = fps.abs().max(0.0001);
    }

    pub fn create_scheduler(&self) -> impl Fn(Duration) + Send + Sync + 'static {
        let next_frame_time = self.next_frame_time.clone();
        let frame_time_changed = self.frame_time_changed.clone();
        let fps_target = self.fps_target.clone();
        move |delay: Duration| {
            Self::schedule_frame_at(&fps_target, &next_frame_time, &frame_time_changed, delay);
        }
    }

    /// Internal method to schedule a frame at a specific duration from now.
    fn schedule_frame_at(
        fps_target: &Arc<Mutex<f32>>,
        next_frame_time: &Arc<Mutex<FrameSchedulerSignal>>,
        frame_time_changed: &Arc<Condvar>,
        delay: Duration,
    ) {
        let min_delay = Duration::from_secs_f32(1.0 / *fps_target.lock().unwrap());
        // let min_delay = std::time::Duration::from_secs(1); // ~1 FPS
        // let min_delay = std::time::Duration::from_nanos(16_666_666); // ~60 FPS
        // let min_delay = std::time::Duration::from_nanos(3_333_333); // ~300 FPS
        let delay = if delay < min_delay { min_delay } else { delay };
        let deadline = Instant::now() + delay;
        let mut next = next_frame_time.lock().unwrap();
        let new_deadline: Option<Instant> = match *next {
            FrameSchedulerSignal::NothingScheduled => Some(deadline),
            FrameSchedulerSignal::ScheduleFrameAt(prev) => {
                if deadline < prev {
                    Some(deadline)
                } else {
                    None
                }
            }
            FrameSchedulerSignal::Exit => None,
        };

        if let Some(new_deadline) = new_deadline {
            *next = FrameSchedulerSignal::ScheduleFrameAt(new_deadline);
        }

        drop(next);
        if new_deadline.is_some() {
            frame_time_changed.notify_all();
        }
    }

    /// Schedule a frame update at the specified duration from now.
    #[allow(dead_code)]
    pub fn schedule_frame(&mut self, at: Duration) {
        Self::schedule_frame_at(
            &self.fps_target,
            &self.next_frame_time,
            &self.frame_time_changed,
            at,
        );
    }
}

impl Drop for FrameScheduler {
    fn drop(&mut self) {
        let mut next = self.next_frame_time.lock().unwrap();
        *next = FrameSchedulerSignal::Exit;
        drop(next);
        self.frame_time_changed.notify_all();
        self.thread.take().unwrap().join().unwrap();
    }
}
