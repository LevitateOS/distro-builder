//! Build timing utilities.

use std::time::Instant;

/// A simple timer for measuring build phase durations.
pub struct Timer {
    name: String,
    start: Instant,
}

impl Timer {
    /// Start a new timer with the given phase name.
    pub fn start(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start: Instant::now(),
        }
    }

    /// Finish the timer and print the elapsed time.
    pub fn finish(self) {
        let secs = self.start.elapsed().as_secs_f64();
        if secs >= 60.0 {
            println!("  [{:.1}m] {}", secs / 60.0, self.name);
        } else {
            println!("  [{:.1}s] {}", secs, self.name);
        }
    }
}
