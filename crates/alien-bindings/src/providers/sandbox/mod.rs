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

/// The longest command deadline these backends accept.
///
/// A ceiling rather than a guard: a timer takes a point in time, and a duration near
/// `Duration::MAX` has no representable one, so accepting it would panic the caller's task.
/// Nothing legitimate reaches this — a session outlives its commands and no cloud lets one run
/// a day — so the far end is refused as the invalid request it is, and the arithmetic below
/// cannot overflow.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) const MAX_DEADLINE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// How long past a command's deadline a provider without a supervising agent waits for the
/// in-session `timeout` to report back before it ends the session itself. Generous against the
/// transport's own latency and short against a caller's patience.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) const DEADLINE_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Bounds one command inside a session, and reports its own kill.
///
/// Neither the exit status nor the clock can say whether a command was killed at its deadline:
/// `timeout -s KILL` exits 137 on GNU and BusyBox alike, `-k` exits 124 on one and 143 on the
/// other, BusyBox reports 0 for a command that traps `TERM`, and what a provider measures is its
/// own round trip rather than how long the command ran. So the wrapper does the killing itself
/// and says so — measured on both shells rather than read off a manual page.
///
/// The report is a nonce the **session** draws, announced on the first line of stderr and
/// repeated only if the killer's signal landed. Drawn there rather than passed in because the
/// command can read its parent's `/proc/<pid>/cmdline` and `environ`: a nonce that travelled in
/// either could be echoed back, and untrusted code would be able to claim its own deadline. A
/// shell variable is in neither, and the command cannot read what has already been written to
/// the stream it inherits. `unset` first, because an inherited *exported* variable of the same
/// name keeps its export attribute across re-assignment and would carry the nonce straight back
/// into the command's own environment.
///
/// Nothing but `sh` and `/dev/urandom` is required, which every session image has.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) struct DeadlineReport;

#[cfg(any(feature = "azure", feature = "local"))]
impl DeadlineReport {
    /// The shell program that runs a command under this deadline.
    ///
    /// The command arrives as `"$@"`, so nothing re-parses its text. It is started in a session
    /// of its own so the kill reaches its process group rather than one pid: a command that
    /// spawned children would otherwise leave them running while the caller is told the deadline
    /// contained it, which is the claim this path exists to make good on. An image that cannot do
    /// that runs nothing — a deadline that cannot be enforced is refused, not approximated.
    ///
    /// The killer repeats the nonce when its signal was delivered, which the status has to confirm:
    /// a command already exited and awaiting reaping takes the signal too. Once the command is
    /// reaped the killer is told to stop and then
    /// awaited: asleep, it reaps its own sleeper and leaves at once, so a command that ended early —
    /// or was killed by something other than the deadline — is not held for the rest of the deadline;
    /// past its sleep it ignores the stop, so its report lands before it leaves rather than being
    /// cut off mid-write. It stays a subshell so the nonce reaches it as a variable and never through
    /// argv, which the command could read.
    pub(crate) fn bounded_program(deadline: std::time::Duration) -> String {
        format!(
            "unset nonce command_pid killer_pid sleeper status; \
             command -v setsid >/dev/null 2>&1 || exit {unboundable}; \
             nonce=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \\n') || exit {unboundable}; \
             printf '%s\\n' \"$nonce\" >&2; \
             setsid \"$@\" & command_pid=$!; \
             ( sleep {deadline} & sleeper=$!; trap 'kill \"$sleeper\" 2>/dev/null; exit' TERM; \
               wait \"$sleeper\"; \
               trap '' TERM; kill -KILL -\"$command_pid\" 2>/dev/null && printf %s \"$nonce\" >&2 ) & killer_pid=$!; \
             wait \"$command_pid\"; status=$?; \
             kill \"$killer_pid\" 2>/dev/null; wait \"$killer_pid\"; \
             exit \"$status\"",
            unboundable = Self::UNBOUNDABLE_EXIT_CODE,
            deadline = deadline_seconds(deadline)
        )
    }

    /// What the wrapper exits with when the session cannot bound a command at all.
    ///
    /// Distinct from anything the command could return, because the command never ran: the
    /// wrapper exits before starting it.
    const UNBOUNDABLE_EXIT_CODE: i32 = 126;

    /// What a shell reports for a command `SIGKILL` ended, which is how the deadline ends one.
    const KILLED_EXIT_CODE: i32 = 137;

    /// What became of a command the wrapper was asked to bound.
    ///
    /// The announcement is removed either way; a repeat of it after that is the killer's signal,
    /// because only the session knows the value. Whether that signal ended the command is the
    /// status's to say.
    pub(crate) fn read(exit_code: Option<i32>, stderr: &str) -> Bounded {
        let announced = stderr
            .split_once('\n')
            .filter(|(nonce, _)| !nonce.is_empty() && nonce.chars().all(|c| c.is_ascii_hexdigit()));

        let Some((nonce, rest)) = announced else {
            // No announcement means the wrapper exited before starting anything, so nothing of
            // the caller's ran and nothing about a deadline can be claimed.
            return Bounded::NotRun {
                reason: if exit_code == Some(Self::UNBOUNDABLE_EXIT_CODE) {
                    "the session image cannot hold a command to a deadline: `setsid` and \
                     `/dev/urandom` are required"
                        .to_string()
                } else {
                    format!(
                        "the session could not start a bounded command: {}",
                        stderr.trim()
                    )
                },
            };
        };

        match rest.find(nonce) {
            Some(at) => Bounded::Ran {
                // The repeat says the killer fired, not that it ended anything: `kill` succeeds on
                // a command that has exited and is not yet reaped. Only the status separates the
                // two, so a command that finished at its deadline keeps its own result.
                killed: exit_code == Some(Self::KILLED_EXIT_CODE),
                stderr: format!("{}{}", &rest[..at], &rest[at + nonce.len()..]),
            },
            None => Bounded::Ran {
                killed: false,
                stderr: rest.to_string(),
            },
        }
    }
}

/// What became of a command the wrapper was asked to bound.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) enum Bounded {
    /// The wrapper never started it, so there is nothing to report but why.
    NotRun { reason: String },
    /// It ran; `killed` is the session's own report of ending it at the deadline.
    Ran { killed: bool, stderr: String },
}

/// The deadline as `sleep` takes it, to the millisecond — both shells accept a fraction, so a
/// sub-second deadline is not rounded up to the next second.
#[cfg(any(feature = "azure", feature = "local"))]
fn deadline_seconds(deadline: std::time::Duration) -> String {
    let millis = deadline.as_millis().max(1);
    if millis % 1000 == 0 {
        (millis / 1000).to_string()
    } else {
        format!("{}.{:03}", millis / 1000, millis % 1000)
    }
}

/// Refuses a deadline these backends cannot honour, and returns the guard the caller waits on.
///
/// Both ends are refused rather than quietly adjusted, as the agent-supervised backends refuse a
/// deadline that floors to zero milliseconds. Below a millisecond the in-session bound cannot
/// express it. At the other end the guard is a point in time, not a length: a duration near
/// `Duration::MAX` has no representable instant, and asking a timer for one panics the caller's
/// task instead of failing its request.
#[cfg(any(feature = "azure", feature = "local"))]
pub(crate) fn guard_for(
    deadline: std::time::Duration,
) -> crate::error::Result<std::time::Duration> {
    let refuse = |reason: &str| {
        alien_error::AlienError::new(crate::error::ErrorData::SandboxCommandFailed {
            failure: "invalidRequest".to_string(),
            reason: reason.to_string(),
        })
    };

    if deadline < std::time::Duration::from_millis(1) {
        return Err(refuse(
            "a command deadline must be at least one millisecond",
        ));
    }

    if deadline > MAX_DEADLINE {
        return Err(refuse(&format!(
            "a command deadline must be at most {} hours",
            MAX_DEADLINE.as_secs() / 3600
        )));
    }

    Ok(deadline + DEADLINE_GRACE)
}

#[cfg(all(test, any(feature = "azure", feature = "local")))]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_reaches_the_shell_to_the_millisecond() {
        assert_eq!(deadline_seconds(std::time::Duration::from_secs(30)), "30");
        assert_eq!(
            deadline_seconds(std::time::Duration::from_millis(1500)),
            "1.500"
        );
        assert_eq!(
            deadline_seconds(std::time::Duration::from_millis(500)),
            "0.500"
        );
    }

    /// A command cannot claim the deadline: the nonce is drawn inside the session, and the
    /// command can read neither the parent's command line nor its environment for it.
    #[test]
    fn only_the_session_can_report_a_deadline() {
        // The shell writes its own notice after the signal, so the repeat is not always last.
        let killed = match DeadlineReport::read(Some(137), "abc123\nboom\nabc123Killed\n") {
            Bounded::Ran { killed, stderr } => {
                assert_eq!(stderr, "boom\nKilled\n");
                killed
            }
            Bounded::NotRun { reason } => panic!("the command ran: {reason}"),
        };
        assert!(killed);

        // A command echoing something nonce-shaped repeats nothing the session announced.
        assert!(matches!(
            DeadlineReport::read(Some(0), "abc123\nboom\ndeadbeef\n"),
            Bounded::Ran { killed: false, .. }
        ));
    }

    /// An image that cannot hold a command to its deadline runs nothing, and says so: reporting
    /// an ordinary exit there would be a deadline the resource never enforced.
    #[test]
    fn a_session_that_cannot_bound_a_command_runs_nothing() {
        let Bounded::NotRun { reason } = DeadlineReport::read(Some(126), "") else {
            panic!("an unboundable session must not look like a command that ran");
        };
        assert!(reason.contains("setsid"), "{reason}");

        // Any other silence is the session failing to start the wrapper at all.
        let Bounded::NotRun { reason } = DeadlineReport::read(Some(127), "sh: not found\n") else {
            panic!("stderr with no announcement is not a command that ran");
        };
        assert!(reason.contains("could not start"), "{reason}");
    }

    /// A command that finished as the killer fired keeps its own result. `kill` succeeds on a
    /// process that has exited and is not yet reaped, so the repeat alone would turn a command
    /// that beat its deadline into a deadline failure and throw away what it returned.
    #[test]
    fn a_command_that_finished_as_the_killer_fired_keeps_its_result() {
        let Bounded::Ran { killed, stderr } = DeadlineReport::read(Some(0), "abc123\nboom\nabc123")
        else {
            panic!("the command ran");
        };
        assert!(
            !killed,
            "a command that exited 0 was not ended by the deadline, whatever the signal reached"
        );
        assert_eq!(stderr, "boom\n", "the announcement is removed either way");
    }

    /// The command reaches the shell as arguments, the nonce is drawn in the session, the kill
    /// reaches the whole process group, and the killer only claims a kill it made.
    #[test]
    fn the_bounded_program_kills_and_reports_only_what_it_killed() {
        let program = DeadlineReport::bounded_program(std::time::Duration::from_millis(1500));
        assert!(program.contains("/dev/urandom"), "{program}");
        assert!(program.contains("command -v setsid"), "{program}");
        assert!(
            program.contains("setsid \"$@\" & command_pid=$!"),
            "{program}"
        );
        assert!(program.contains("sleep 1.500"), "{program}");
        assert!(
            program.contains("kill -KILL -\"$command_pid\""),
            "the kill reaches the command's process group, not one pid: {program}"
        );
        assert!(
            program.contains("2>/dev/null && printf %s \"$nonce\""),
            "the repeat follows a signal that was delivered: {program}"
        );
        assert!(
            program.contains("kill \"$killer_pid\" 2>/dev/null; wait \"$killer_pid\""),
            "the killer is stopped and then awaited, whatever the command's exit: {program}"
        );
        assert!(
            program.contains(r#"wait "$sleeper"; trap '' TERM; kill -KILL"#),
            "past its sleep the killer ignores the stop, so its report is never cut off, and the \
             pid is quoted so an inherited IFS cannot split it into words that are not children: \
             {program}"
        );
        assert!(
            program.contains(r#"trap 'kill "$sleeper" 2>/dev/null; exit' TERM"#),
            "a stopped killer reaps its own sleeper, so none outlives the command: {program}"
        );
        assert!(
            !program.contains("\"$nonce\" &") && program.contains("( sleep"),
            "the killer is a subshell: the nonce reaches it as a variable, never as an argument \
             the command could read from /proc: {program}"
        );
    }

    /// An inherited variable of the wrapper's own name never reaches the command.
    ///
    /// Run against a real `sh`: an exported variable keeps its export attribute across
    /// re-assignment, so `nonce=$(…)` would hand the command the session's own nonce. A
    /// stand-in `setsid` is supplied because macOS ships none.
    #[test]
    #[cfg(unix)]
    fn the_wrapper_never_hands_its_nonce_to_the_command() {
        use std::os::unix::fs::PermissionsExt;

        let bin = std::env::temp_dir().join(format!("alien-sandbox-{}", std::process::id()));
        std::fs::create_dir_all(&bin).expect("a directory for the stand-in");
        let setsid = bin.join("setsid");
        std::fs::write(&setsid, "#!/bin/sh\nexec \"$@\"\n").expect("the stand-in is written");
        std::fs::set_permissions(&setsid, std::fs::Permissions::from_mode(0o755))
            .expect("the stand-in is executable");

        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let run = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(DeadlineReport::bounded_program(std::time::Duration::from_secs(5)))
            .arg("sh")
            .arg("printenv")
            .arg("nonce")
            .env("PATH", path)
            .env("nonce", "inherited-from-the-session")
            .output()
            .expect("a shell runs");
        std::fs::remove_dir_all(&bin).ok();

        let announced = String::from_utf8_lossy(&run.stderr);
        let announced = announced.lines().next().unwrap_or_default().to_string();
        assert!(
            announced.len() == 32 && announced.chars().all(|c| c.is_ascii_hexdigit()),
            "the session has to reach the point of drawing a nonce, or this proves nothing: \
             stderr {:?}",
            String::from_utf8_lossy(&run.stderr)
        );

        let seen = String::from_utf8_lossy(&run.stdout);
        assert!(
            seen.trim().is_empty(),
            "the command must inherit no `nonce` at all, and it saw {seen:?}"
        );
    }

    /// A deadline neither end can honour is refused, not stretched or waited on.
    #[tokio::test]
    async fn a_deadline_outside_what_the_backends_can_honour_is_refused() {
        let too_fine = guard_for(std::time::Duration::from_micros(500))
            .expect_err("half a millisecond cannot be bounded");
        assert!(
            too_fine.to_string().contains("invalidRequest"),
            "{too_fine}"
        );

        let too_long = guard_for(std::time::Duration::MAX)
            .expect_err("a deadline with no representable instant cannot be waited out");
        assert!(
            too_long.to_string().contains("invalidRequest"),
            "{too_long}"
        );

        assert_eq!(
            guard_for(std::time::Duration::from_secs(30)).expect("an ordinary deadline"),
            std::time::Duration::from_secs(30) + DEADLINE_GRACE,
            "the guard is the deadline plus the grace"
        );
    }
}
