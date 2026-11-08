//! Rate limit for value verification.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::ValueScope;

/// Allows a few verification requests per scope per minute. Verification
/// tells a caller whether a guess is right, so it is slowed down.
///
/// Verification exists only to move values between control planes. It is
/// removed in Deploy 2, together with this limiter.
pub struct VerifyLimiter {
    max_per_window: u32,
    window: Duration,
    seen: Mutex<HashMap<ValueScope, (Instant, u32)>>,
}

impl Default for VerifyLimiter {
    fn default() -> Self {
        Self::new(10, Duration::from_secs(60))
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

    /// Counts one request against the scope. Returns false when over the limit.
    pub fn allow(&self, scope: &ValueScope) -> bool {
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        let entry = seen.entry(scope.clone()).or_insert((now, 0));
        if entry.1 >= self.max_per_window {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_requests_a_minute_is_the_default() {
        let limiter = VerifyLimiter::default();
        let scope: ValueScope = "dep:a".parse().unwrap();
        assert_eq!((0..20).filter(|_| limiter.allow(&scope)).count(), 10);
    }

    #[test]
    fn the_limit_applies_per_scope_and_resets_after_the_window() {
        let limiter = VerifyLimiter::new(3, Duration::from_millis(50));
        let a: ValueScope = "dep:a".parse().unwrap();
        let b: ValueScope = "dep:b".parse().unwrap();
        for _ in 0..3 {
            assert!(limiter.allow(&a));
        }
        assert!(!limiter.allow(&a));
        for _ in 0..3 {
            assert!(limiter.allow(&b));
        }
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.allow(&a));
    }
}
