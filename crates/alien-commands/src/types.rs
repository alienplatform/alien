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

/// A lease request that asked to wait and came back empty sooner than this was
/// answered by a server that does not wait; the receiver then backs off as it
/// does for any empty answer.
pub const LEASE_WAIT_HONORED_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// Shortest time between two lease request starts after a waited empty answer
/// or a non-empty one,
/// so a misbehaving server cannot make the receiver spin.
pub const LEASE_WAIT_REPOLL_FLOOR: std::time::Duration = std::time::Duration::from_millis(250);
