//! Animated spinner for thinking indicator

use std::io::{stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

const BARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const NUM_BARS: usize = 8;
const FRAME_DURATION: Duration = Duration::from_millis(80);

/// An animated spinner that runs in the background
pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// Start a new spinner with the given message
    pub fn new(message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let message = message.to_string();

        let handle = tokio::spawn(async move {
            // Each bar has its own height (0-7) and velocity
            let mut heights = [3i8, 5, 4, 6, 3, 5, 4, 3];
            let mut velocities = [1i8, -1, 1, -1, 1, -1, 1, -1];

            while running_clone.load(Ordering::Relaxed) {
                // Smooth: blend each bar slightly toward its neighbors
                let smoothed: Vec<i8> = (0..NUM_BARS)
                    .map(|i| {
                        let left = if i > 0 { heights[i - 1] } else { heights[i] };
                        let right = if i < NUM_BARS - 1 {
                            heights[i + 1]
                        } else {
                            heights[i]
                        };
                        // 60% self, 20% each neighbor
                        ((heights[i] as i16 * 3 + left as i16 + right as i16) / 5) as i8
                    })
                    .collect();

                let frame: String = smoothed.iter().map(|&h| BARS[h as usize]).collect();
                print!("\r\x1b[2m{} {}\x1b[0m", frame, message);
                let _ = stdout().flush();

                // Update with bounce physics (floor at 1, ceiling at 7)
                for i in 0..NUM_BARS {
                    heights[i] += velocities[i];
                    if heights[i] <= 1 || heights[i] >= 7 {
                        velocities[i] = -velocities[i];
                        heights[i] = heights[i].clamp(1, 7);
                    }
                }
                tokio::time::sleep(FRAME_DURATION).await;
            }
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Stop the spinner and clear the line
    pub async fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
        print!("\r\x1b[2K");
        let _ = stdout().flush();
    }
}
