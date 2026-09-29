//! How long a public worker waits for its managed certificate.

use std::time::Duration;

/// Certificate issuance is asynchronous: an attempt can take minutes and a
/// failed attempt is retried after a backoff of minutes, so the wait covers
/// several attempts instead of failing the deployment during the first retry.
const CERTIFICATE_WAIT: Duration = Duration::from_secs(30 * 60);
pub const CERTIFICATE_WAIT_POLL_SECS: u64 = 5;
pub const CERTIFICATE_WAIT_MAX_POLLS: u32 =
    (CERTIFICATE_WAIT.as_secs() / CERTIFICATE_WAIT_POLL_SECS) as u32;
