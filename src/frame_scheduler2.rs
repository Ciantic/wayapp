//! Frame scheduler for repaint requests
//!
//! Retains a next frame time and signals frame updates accordingly
//!
//! This is not supposed to hold many frame times, but instead only the next
//! frame time. This is how EGUI works.

// Note: Try not to edit this with LLMs, locking logic needs to be verified
// manually.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::sync::mpsc::channel;
use std::time::Duration;
use std::time::Instant;

enum FrameSchedulerSignal {
    /// New earlier frame scheduled
    ScheduleFrameAt(Instant),

    /// Exit the frame scheduler thread
    Exit,
}

/// Schedules frame updates based on egui's repaint requests.
pub(crate) struct FrameScheduler2 {
    #[allow(dead_code)]
    thread: Option<std::thread::JoinHandle<()>>,
    current_deadline: Arc<Mutex<Option<Instant>>>,
    fps_target: Arc<Mutex<f32>>,
    sender: Sender<FrameSchedulerSignal>,
}

impl FrameScheduler2 {
    pub fn new(emit_frame: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = channel::<FrameSchedulerSignal>();
        let current_deadline = Arc::new(Mutex::new(None));
        let current_deadline_thrd = current_deadline.clone();
        FrameScheduler2 {
            thread: Some(std::thread::spawn(move || {
                let mut now;
                let mut next_deadline: Option<Instant>;
                let mut wait_time: Duration;
                loop {
                    now = Instant::now();
                    next_deadline = *current_deadline_thrd.lock().unwrap();
                    wait_time = match next_deadline {
                        Some(t) => t - now,
                        None => Duration::MAX,
                    };

                    match rx.recv_timeout(wait_time) {
                        Ok(FrameSchedulerSignal::Exit) => {
                            break;
                        }
                        Ok(FrameSchedulerSignal::ScheduleFrameAt(new_deadline)) => {
                            current_deadline_thrd.lock().unwrap().replace(new_deadline);
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            current_deadline_thrd.lock().unwrap().take();
                            emit_frame();
                        }
                        Err(_) => {
                            // Channel closed
                            break;
                        }
                    }
                }
            })),
            fps_target: Arc::new(Mutex::new(60.0)),
            current_deadline,
            sender: tx,
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

    /// Create scheduler function
    pub fn create_scheduler(&self) -> impl Fn(Duration) + Send + Sync + 'static {
        let fps_target = self.fps_target.clone();
        let current_deadline = self.current_deadline.clone();
        let sender = self.sender.clone();
        move |delay: Duration| {
            Self::schedule_frame_at(&fps_target, &current_deadline, &sender, delay)
        }
    }

    /// Internal method to schedule a frame at a specific duration from now.
    fn schedule_frame_at(
        fps_target: &Arc<Mutex<f32>>,
        current_deadline: &Arc<Mutex<Option<Instant>>>,
        sender: &Sender<FrameSchedulerSignal>,
        delay: Duration,
    ) {
        let min_delay = Duration::from_secs_f32(1.0 / *fps_target.lock().unwrap());
        // let min_delay = std::time::Duration::from_secs(1); // ~1 FPS
        // let min_delay = std::time::Duration::from_nanos(16_666_666); // ~60 FPS
        // let min_delay = std::time::Duration::from_nanos(3_333_333); // ~300 FPS
        let delay = delay.max(min_delay);
        let new_deadline = Instant::now() + delay;
        if let Some(current_deadline) = *current_deadline.lock().unwrap() {
            if new_deadline >= current_deadline {
                return;
            }
        }

        sender
            .send(FrameSchedulerSignal::ScheduleFrameAt(new_deadline))
            .unwrap();
    }

    /// Schedule a frame update at the specified duration from now.
    #[allow(dead_code)]
    pub fn schedule_frame(&mut self, at: Duration) {
        Self::schedule_frame_at(&self.fps_target, &self.current_deadline, &self.sender, at);
    }
}

impl Drop for FrameScheduler2 {
    fn drop(&mut self) {
        let _ = self.sender.send(FrameSchedulerSignal::Exit);
        self.thread.take().unwrap().join().unwrap();
    }
}
