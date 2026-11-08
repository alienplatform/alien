//! Rate limit for value verification.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::ValueScope;

/// Allows a few verifications per scope per minute. Verification tells a
/// caller whether a guess is right, so it is slowed down.
pub struct VerifyLimiter {
    max_per_window: u32,
    window: Duration,
    seen: Mutex<HashMap<ValueScope, (Instant, u32)>>,
}

impl Default for VerifyLimiter {
    fn default() -> Self {
        Self::new(60, Duration::from_secs(60))
    }
}

impl VerifyLimiter {
    pub fn new(max_per_window: u32, window: Duration) -> Self {
        Self {
            max_per_window,
            window,
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Counts `attempts` against the scope. Returns false when over the limit.
    pub fn allow(&self, scope: &ValueScope, attempts: u32) -> bool {
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        let entry = seen.entry(scope.clone()).or_insert((now, 0));
        if entry.1.saturating_add(attempts) > self.max_per_window {
            return false;
        }
        entry.1 += attempts;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_applies_per_scope_and_resets_after_the_window() {
        let limiter = VerifyLimiter::new(3, Duration::from_millis(50));
        let a: ValueScope = "dep:a".parse().unwrap();
        let b: ValueScope = "dep:b".parse().unwrap();
        assert!(limiter.allow(&a, 2));
        assert!(limiter.allow(&a, 1));
        assert!(!limiter.allow(&a, 1));
        assert!(limiter.allow(&b, 3));
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.allow(&a, 3));
    }
}
