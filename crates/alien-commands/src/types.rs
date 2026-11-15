// Re-export all commands types from alien-core
pub use alien_core::commands_types::*;

/// Default interval between lease polls, in seconds.
///
/// Used by app-owned pull receivers.
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 5;

/// Default maximum number of leases requested per poll.
pub const DEFAULT_MAX_LEASES: usize = 1;

/// Default lease duration requested per poll, in seconds.
///
/// There is no lease-renew call in the protocol: a command's execution
/// budget is `min(envelope.deadline, lease_expires_at - safety_margin)`.
pub const DEFAULT_LEASE_SECONDS: u64 = 60;

/// Maximum interval reached by the receiver's empty/error poll backoff.
pub const DEFAULT_POLL_MAX_INTERVAL_MS: u64 = 30_000;

/// Fractional randomization applied to receiver poll sleeps.
pub const DEFAULT_POLL_JITTER: f64 = 0.1;

/// Time allowed for in-flight handlers to finish before abort and release.
pub const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 30_000;

/// How long a receiver asks the manager to hold an empty lease request open,
/// in seconds. A waiting request is answered as soon as a command is queued.
pub const DEFAULT_LEASE_WAIT_SECONDS: u64 = 25;

/// How long an empty answer to a lease request that asked to wait must have
/// taken for the receiver to conclude the manager waited: the requested wait
/// minus a margin for a wait that ends a little early (never less than 2 s,
/// and never more than the wait itself). A faster empty answer came from a
/// server that does not wait, or one at its limit of open waits; the receiver
/// then backs off as it does for any empty answer.
pub fn lease_wait_honored_after(wait_seconds: u64) -> std::time::Duration {
    let seconds = wait_seconds.saturating_sub(3).max(2).min(wait_seconds);
    std::time::Duration::from_secs(seconds)
}

/// A poll that asked to wait may be abandoned at shutdown once it has been
/// open this long. A faster answer may already be a lease the server wrote, so
/// the receiver still awaits it.
pub const LEASE_WAIT_ABANDON_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// How long the client waits for the answer to a lease request that asks the
/// manager to wait: the wait plus 15 s. `None` for a request that does not
/// wait, which keeps the client's own 30 s limit.
pub fn lease_request_timeout(wait_seconds: u64) -> Option<std::time::Duration> {
    (wait_seconds > 0).then(|| std::time::Duration::from_secs(wait_seconds + 15))
}

/// Shortest time between two lease request starts after a waited empty answer
/// or a non-empty one,
/// so a misbehaving server cannot make the receiver spin.
pub const LEASE_WAIT_REPOLL_FLOOR: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(test)]
mod lease_wait_timing_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_client_waits_for_the_requested_wait_plus_fifteen_seconds() {
        assert_eq!(lease_request_timeout(25), Some(Duration::from_secs(40)));
        assert_eq!(lease_request_timeout(1), Some(Duration::from_secs(16)));
        assert_eq!(lease_request_timeout(0), None);
    }

    #[test]
    fn an_empty_answer_counts_as_waited_after_the_wait_minus_a_margin() {
        assert_eq!(lease_wait_honored_after(25), Duration::from_secs(22));
        assert_eq!(lease_wait_honored_after(10), Duration::from_secs(7));
        assert_eq!(lease_wait_honored_after(4), Duration::from_secs(2));
        // Never more than the wait itself.
        assert_eq!(lease_wait_honored_after(2), Duration::from_secs(2));
        assert_eq!(lease_wait_honored_after(1), Duration::from_secs(1));
    }
}
