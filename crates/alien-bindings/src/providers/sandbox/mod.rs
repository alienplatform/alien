//! Sandbox binding providers.
//!
//! Per-cloud backends land with their controllers. Local is here because it speaks the same
//! authenticated transport the cloud backends do, so it exercises the real path rather than a
//! shortcut.

#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub mod agent_protocol;

#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "gcp")]
pub mod gcp;

#[cfg(feature = "kubernetes")]
pub mod kubernetes;

#[cfg(feature = "local")]
pub mod local;

/// How long past a command's deadline a provider without a supervising agent waits for the
/// in-session `timeout` to report back before it ends the session itself. Generous against the
/// transport's own latency and short against a caller's patience.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) const DEADLINE_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// What `timeout` exits with when the command ran out of time — GNU coreutils and BusyBox alike,
/// whatever signal was sent.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) const TIMEOUT_EXIT_CODE: i32 = 124;

/// Whether the wrapper killed the command, rather than the command exiting 124 of its own accord.
///
/// The exit code alone cannot say: 124 is an ordinary status a command may return. The clock
/// settles it — the wrapper cannot fire before the deadline — so a 124 that arrives early is the
/// command's own, and is reported as an exit like any other.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) fn wrapper_killed(
    exit_code: Option<i32>,
    elapsed: std::time::Duration,
    deadline: std::time::Duration,
) -> bool {
    exit_code == Some(TIMEOUT_EXIT_CODE) && elapsed >= deadline
}

/// The `timeout` prefix that bounds a command inside the session: `KILL` so a process that traps
/// `TERM` cannot outlive its deadline. The bound is the deadline itself, to the millisecond —
/// GNU coreutils and BusyBox both take a fractional duration — so a sub-second deadline is not
/// rounded up to the next second.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) fn timeout_prefix(deadline: std::time::Duration) -> [String; 4] {
    let millis = deadline.as_millis().max(1);
    let secs = if millis % 1000 == 0 {
        (millis / 1000).to_string()
    } else {
        format!("{}.{:03}", millis / 1000, millis % 1000)
    };
    [
        "timeout".to_string(),
        "-s".to_string(),
        "KILL".to_string(),
        secs,
    ]
}

/// The shell probe for `timeout`, run once per provider before the first bounded command.
///
/// Asked, not inferred from a failed run: a command of the caller's own can exit 127 and print
/// anything, so classifying its output would risk running it a second time. The probe runs
/// nothing of the caller's and answers with its exit code alone.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) const TIMEOUT_PROBE: &str = "command -v timeout >/dev/null 2>&1";

/// The finest deadline the in-session bound can express. Below it the request is refused, as
/// the agent-supervised backends refuse a deadline that floors to zero milliseconds, rather
/// than quietly rounded up.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) fn refuse_unrepresentable_deadline(
    deadline: std::time::Duration,
) -> crate::error::Result<()> {
    if deadline < std::time::Duration::from_millis(1) {
        return Err(alien_error::AlienError::new(
            crate::error::ErrorData::SandboxCommandFailed {
                failure: "invalidRequest".to_string(),
                reason: "a command deadline must be at least one millisecond".to_string(),
            },
        ));
    }
    Ok(())
}

#[cfg(all(test, any(feature = "azure", feature = "local")))]
mod tests {
    use super::*;

    #[test]
    fn the_bound_is_the_deadline_to_the_millisecond() {
        assert_eq!(timeout_prefix(std::time::Duration::from_secs(30))[3], "30");
        assert_eq!(
            timeout_prefix(std::time::Duration::from_millis(1500))[3],
            "1.500"
        );
        assert_eq!(
            timeout_prefix(std::time::Duration::from_millis(500))[3],
            "0.500"
        );
    }

    /// 124 is an ordinary exit status, so the clock is what tells the wrapper's kill from a
    /// command that returned it itself.
    #[test]
    fn a_command_exiting_124_early_is_not_a_deadline() {
        let deadline = std::time::Duration::from_secs(30);
        assert!(wrapper_killed(
            Some(TIMEOUT_EXIT_CODE),
            std::time::Duration::from_secs(30),
            deadline
        ));
        assert!(!wrapper_killed(
            Some(TIMEOUT_EXIT_CODE),
            std::time::Duration::from_secs(2),
            deadline
        ));
        assert!(!wrapper_killed(Some(0), deadline, deadline));
    }

    /// A deadline the bound cannot express is refused, not stretched.
    #[test]
    fn a_sub_millisecond_deadline_is_refused() {
        let error = refuse_unrepresentable_deadline(std::time::Duration::from_micros(500))
            .expect_err("half a millisecond cannot be bounded");
        assert!(error.to_string().contains("invalidRequest"), "{error}");
        refuse_unrepresentable_deadline(std::time::Duration::from_millis(1))
            .expect("one millisecond is the finest bound");
    }
}
