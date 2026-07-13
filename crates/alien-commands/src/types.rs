// Re-export all commands types from alien-core
pub use alien_core::commands_types::*;

/// Default interval between lease polls, in seconds.
///
/// Shared by every pull-side poller of the commands protocol (the worker
/// runtime's `commands_polling` transport and the app-owned `Receiver`).
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
