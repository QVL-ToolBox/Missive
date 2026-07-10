use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    max_per_window: u32,
    window: Duration,
    state: Mutex<WindowState>,
}

struct WindowState {
    window_start: Instant,
    count: u32,
}

impl RateLimiter {
    pub fn per_minute(max_per_window: u32) -> Self {
        Self {
            max_per_window,
            window: Duration::from_secs(60),
            state: Mutex::new(WindowState {
                window_start: Instant::now(),
                count: 0,
            }),
        }
    }

    pub fn try_acquire(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let now = Instant::now();
        if now.duration_since(state.window_start) >= self.window {
            state.window_start = now;
            state.count = 0;
        }
        if state.count >= self.max_per_window {
            return false;
        }
        state.count += 1;
        true
    }
}
