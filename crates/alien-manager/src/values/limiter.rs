//! Rate limit for value verification.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{ValueScope, ValueSlot};

/// Allows a few verification guesses per slot per minute. Verification
/// tells a caller whether a guess is right, so it is slowed down. Each slot in
/// a request counts as one guess, so a batch cannot hide many guesses at one
/// slot in one request.
///
/// Verification exists only to move values between control planes. It is
/// removed in Deploy 2, together with this limiter.
pub struct VerifyLimiter {
    max_per_window: u32,
    window: Duration,
    seen: Mutex<HashMap<(ValueScope, ValueSlot), (Instant, u32)>>,
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

    /// Counts one guess against each slot, all or nothing. Returns false, and
    /// counts nothing, when any slot is over the limit.
    pub fn allow(&self, scope: &ValueScope, slots: &[ValueSlot]) -> bool {
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        let over = slots.iter().any(|slot| {
            seen.get(&(scope.clone(), slot.clone()))
                .is_some_and(|(_, count)| *count >= self.max_per_window)
        });
        if over {
            return false;
        }
        for slot in slots {
            seen.entry((scope.clone(), slot.clone()))
                .or_insert((now, 0))
                .1 += 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str) -> Vec<ValueSlot> {
        vec![name.parse().unwrap()]
    }

    #[test]
    fn ten_guesses_a_minute_per_slot_is_the_default() {
        let limiter = VerifyLimiter::default();
        let scope: ValueScope = "dep:a".parse().unwrap();
        assert_eq!(
            (0..20)
                .filter(|_| limiter.allow(&scope, &slot("env:A")))
                .count(),
            10
        );
    }

    #[test]
    fn a_batch_counts_every_slot_and_is_refused_whole_when_one_is_over() {
        let limiter = VerifyLimiter::new(2, Duration::from_secs(60));
        let scope: ValueScope = "dep:a".parse().unwrap();
        let a_and_b: Vec<ValueSlot> = vec!["env:A".parse().unwrap(), "env:B".parse().unwrap()];
        assert!(limiter.allow(&scope, &a_and_b));
        assert!(limiter.allow(&scope, &slot("env:A")));
        // A is now at its limit; the batch is refused and B is not charged.
        assert!(!limiter.allow(&scope, &a_and_b));
        assert!(limiter.allow(&scope, &slot("env:B")));
        assert!(!limiter.allow(&scope, &slot("env:B")));
    }

    #[test]
    fn the_limit_applies_per_scope_and_resets_after_the_window() {
        let limiter = VerifyLimiter::new(3, Duration::from_millis(50));
        let a: ValueScope = "dep:a".parse().unwrap();
        let b: ValueScope = "dep:b".parse().unwrap();
        for _ in 0..3 {
            assert!(limiter.allow(&a, &slot("env:X")));
        }
        assert!(!limiter.allow(&a, &slot("env:X")));
        for _ in 0..3 {
            assert!(limiter.allow(&b, &slot("env:X")));
        }
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.allow(&a, &slot("env:X")));
    }
}
