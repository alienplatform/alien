//! The update state machine:
//! Running → Staged → Swapping → Probation → Promoted / RollingBack.
//!
//! Everything here drives the `traits` boundary — no direct filesystem access
//! (enforced by `tests/platform_blind.rs`, which additionally forbids
//! `std::fs` in this file) and no platform knowledge. The machine is
//! reconstructable from the on-disk store alone: `classify_startup` maps
//! every reachable intermediate state to a recovery action, so a launcher
//! killed at ANY point resumes to promote or rollback. Store errors
//! deliberately propagate out — the OS init respawns the launcher and
//! classification recovers; self-healing in place would mask corruption.

use std::time::{Duration, Instant};

use alien_error::AlienError;
use alien_core::sync::OperatorUpdatePhase;
use chrono::Utc;
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

use super::store_common;
use super::traits::{
    ChildSupervisor, Control, ExitStatus, FailureRecord, HealthProbe, OperatorHandle,
    PendingMarker, ProbationMarker, ServiceHost, UpdateEnv, Version, VersionStore,
    EXIT_CODE_UPDATE_HANDOFF,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// All tunables, injected so tests run the identical machine with millisecond
/// windows.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// Probation window after a swap (~90 s in production).
    pub probation_window: Duration,
    /// How often the gate polls `/readyz`.
    pub probe_interval: Duration,
    /// Supervise-loop tick (child poll + control poll).
    pub poll_interval: Duration,
    /// Watchdog heartbeat interval (systemd: `WatchdogSec/3`).
    pub heartbeat_interval: Duration,
    /// Grace given to the operator between SIGTERM-equivalent and force-kill.
    pub stop_grace: Duration,
    /// Crash-respawn backoff: base doubling per consecutive crash…
    pub restart_backoff_base: Duration,
    /// …capped here…
    pub restart_backoff_cap: Duration,
    /// …and reset after the child has run healthy this long.
    pub healthy_reset: Duration,
    /// Give up resuming a crashed swap after this many attempts.
    pub max_swap_attempts: u32,
    /// File name of the operator binary inside `versions/<v>/`.
    pub operator_binary: String,
    /// Environment handed to every spawned operator.
    pub update_env: UpdateEnv,
}

impl RunConfig {
    /// The `/readyz` URL derived from the health address the launcher itself
    /// hands to the operator on spawn.
    pub fn readyz_url(&self) -> String {
        format!("http://{}/readyz", self.update_env.health_addr)
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            probation_window: Duration::from_secs(90),
            probe_interval: Duration::from_secs(2),
            poll_interval: Duration::from_millis(100),
            heartbeat_interval: Duration::from_secs(20),
            stop_grace: Duration::from_secs(10),
            restart_backoff_base: Duration::from_secs(1),
            restart_backoff_cap: Duration::from_secs(30),
            healthy_reset: Duration::from_secs(60),
            max_swap_attempts: 3,
            operator_binary: "alien-operator".to_string(),
            update_env: UpdateEnv {
                health_addr: std::net::SocketAddr::from(([127, 0, 0, 1], 7799)),
                launcher_version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Startup classification (the normative recovery table for the on-disk protocol)
// ---------------------------------------------------------------------------

/// What the launcher must do first, decided purely from the on-disk store.
#[derive(Debug, Clone, PartialEq)]
pub enum StartupAction {
    /// Row 1 — steady state: spawn `current` (first install additionally
    /// promotes it to `last-stable` after one passed gate).
    SpawnCurrent,
    /// Row 2 — staged, swap not begun: run the swap from step 1.
    RunSwap { pending: PendingMarker },
    /// Row 2 guard — pending names the version that is already current AND
    /// stable (crash after promote removed probation but not pending):
    /// just clean up.
    DiscardLeftoverPending { pending: PendingMarker },
    /// Row 3 — partial stage (binary missing / digest mismatch): delete the
    /// marker and spawn `current`; the operator will re-stage.
    DiscardInvalidPending { pending: PendingMarker },
    /// Row 4 — crashed mid-probation: spawn `current` (= the new version) and
    /// resume the gate with the remaining window (`0` ⇒ roll back now).
    ResumeProbation {
        probation: ProbationMarker,
        remaining: Duration,
    },
    /// Row 4b — promote already began (`last-stable == current == new`):
    /// finish the idempotent promote cleanup.
    FinishPromote { probation: ProbationMarker },
    /// Row 5 — crashed after the probation marker, before the flip:
    /// resume the swap at the flip (attempt budget permitting).
    ResumeSwapAtFlip {
        probation: ProbationMarker,
        pending: PendingMarker,
    },
    /// Row 5 cap — attempt budget exhausted (or the stage went invalid):
    /// run the rollback cleanup and stay on `current`.
    AbortSwap { probation: ProbationMarker },
    /// Row 6 — crashed mid-rollback (failure record already written):
    /// re-run the idempotent rollback steps.
    FinishRollback { probation: ProbationMarker },
}

/// Classify the store per the startup-recovery table of the on-disk handoff
/// protocol. The two crashed-swap rows share pointer
/// state (`current == probation.old`); they are discriminated by the failure
/// record — rollback's first persistent effect after the flip-back is writing
/// `failed/<new>.json` with `attempts >= probation.attempt`, so its presence
/// means a rollback was underway.
pub fn classify_startup<S: VersionStore>(store: &S, config: &RunConfig) -> Result<StartupAction> {
    let current = store.current()?;
    let last_stable = store.last_stable()?;

    if let Some(probation) = store.read_probation()? {
        let current = current.ok_or_else(|| corrupt(store, "probation exists but no current"))?;

        if current == probation.new {
            if last_stable.as_ref() == Some(&probation.new) {
                return Ok(StartupAction::FinishPromote { probation });
            }
            let elapsed = (Utc::now() - probation.started_at)
                .to_std()
                .unwrap_or(Duration::ZERO); // NTP stepped backwards → clamp
            let remaining = config.probation_window.saturating_sub(elapsed);
            return Ok(StartupAction::ResumeProbation {
                probation,
                remaining,
            });
        }

        if current == probation.old {
            let rollback_recorded = store
                .read_failure(&probation.new)?
                .is_some_and(|record| record.attempts >= probation.attempt);
            if rollback_recorded {
                return Ok(StartupAction::FinishRollback { probation });
            }
            if probation.attempt >= config.max_swap_attempts {
                return Ok(StartupAction::AbortSwap { probation });
            }
            // Resuming the swap needs a valid stage to flip to.
            match store.read_pending()? {
                Some(pending) if pending.version == probation.new => {
                    return Ok(StartupAction::ResumeSwapAtFlip { probation, pending });
                }
                _ => return Ok(StartupAction::AbortSwap { probation }),
            }
        }

        return Err(corrupt(
            store,
            "probation marker matches neither current nor its old version",
        ));
    }

    if let Some(pending) = store.read_pending()? {
        if Some(&pending.version) == current.as_ref()
            && Some(&pending.version) == last_stable.as_ref()
        {
            return Ok(StartupAction::DiscardLeftoverPending { pending });
        }
        let binary = store
            .stage_dir(&pending.version)
            .join(&config.operator_binary);
        if store_common::validate_staged_binary(&binary, &pending.sha256)? {
            return Ok(StartupAction::RunSwap { pending });
        }
        return Ok(StartupAction::DiscardInvalidPending { pending });
    }

    Ok(StartupAction::SpawnCurrent)
}

fn corrupt<S: VersionStore>(store: &S, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreCorrupt {
        path: store.stage_dir(&Version::parse("0.0.0").expect("static version parses"))
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        message: message.to_string(),
    })
}

// ---------------------------------------------------------------------------
// The machine
// ---------------------------------------------------------------------------

/// Why the probation gate ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateOutcome {
    Ready,
    TimedOut,
    ChildExited(ExitStatus),
}

/// Why `supervise` returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopExit {
    /// The OS supervisor asked us to stop; the child was stopped gracefully.
    ControlStop(Control),
}

/// The launcher core, generic over the platform boundary. Borrows its
/// components so the run loop, the heartbeat thread, and tests share them.
pub struct Machine<'a, S, C, P, H>
where
    S: VersionStore,
    C: ChildSupervisor,
    P: HealthProbe,
    H: ServiceHost,
{
    pub store: &'a S,
    pub child: &'a mut C,
    pub probe: &'a P,
    pub host: &'a H,
    pub config: &'a RunConfig,
}

impl<S, C, P, H> Machine<'_, S, C, P, H>
where
    S: VersionStore,
    C: ChildSupervisor,
    P: HealthProbe,
    H: ServiceHost,
{
    // -- startup ----------------------------------------------------------

    /// Execute a startup classification to a supervised steady state and
    /// return the handle of the running operator.
    pub fn execute_startup(&mut self, action: StartupAction) -> Result<OperatorHandle> {
        match action {
            StartupAction::SpawnCurrent => self.spawn_current_and_gate(),
            StartupAction::DiscardLeftoverPending { pending } => {
                info!(version = %pending.version, "discarding leftover pending marker from a completed promote");
                self.store.clear_pending()?;
                self.spawn_current_and_gate()
            }
            StartupAction::DiscardInvalidPending { pending } => {
                warn!(
                    version = %pending.version,
                    "pending stage is invalid (missing binary or digest mismatch); discarding — the operator will re-stage"
                );
                self.store.clear_pending()?;
                self.spawn_current_and_gate()
            }
            StartupAction::RunSwap { pending } => {
                let attempt = self.next_attempt(&pending)?;
                self.perform_swap(&pending, attempt)
            }
            StartupAction::ResumeProbation {
                probation,
                remaining,
            } => {
                let handle = self.spawn_version(&probation.new)?;
                if remaining.is_zero() {
                    return self.rollback(&probation, Some(&handle), "probation window expired before the launcher restart".to_string());
                }
                match self.gate(&handle, remaining)? {
                    GateOutcome::Ready => {
                        self.promote(&probation)?;
                        Ok(handle)
                    }
                    outcome => self.rollback(&probation, Some(&handle), gate_failure_message(outcome)),
                }
            }
            StartupAction::FinishPromote { probation } => {
                self.promote(&probation)?;
                self.spawn_current_and_gate()
            }
            StartupAction::ResumeSwapAtFlip { probation, pending } => {
                // The new version never ran; give the gate a fresh window
                // (the marker's started_at predates the crash) but keep the
                // attempt number.
                let refreshed = ProbationMarker {
                    started_at: Utc::now(),
                    ..probation
                };
                self.store.write_probation(&refreshed)?;
                self.flip_spawn_and_gate(&refreshed, &pending)
            }
            StartupAction::AbortSwap { probation } => {
                let message = format!(
                    "giving up on swap to {} after {} attempts",
                    probation.new, probation.attempt
                );
                self.rollback(&probation, None, message)
            }
            StartupAction::FinishRollback { probation } => {
                // Re-run the idempotent rollback tail; the failure record is
                // already on disk (that is how this row was classified).
                self.store.set_current(&probation.old)?;
                self.store.restore_state(&probation.old)?;
                self.store.clear_probation()?;
                self.store.clear_pending()?;
                self.spawn_current_and_gate()
            }
        }
    }

    // -- the supervise loop ------------------------------------------------

    /// Supervise the operator until the OS asks us to stop. Handles the
    /// exit-code contract: 0 = respawn, 10 = validated swap, other = crash
    /// with exponential respawn backoff (reset after `healthy_reset` up).
    pub fn supervise(&mut self, initial: OperatorHandle) -> Result<LoopExit> {
        let mut handle = initial;
        let mut spawned_at = Instant::now();
        let mut consecutive_crashes: u32 = 0;

        loop {
            if let Some(control) = self.host.poll_control() {
                info!(?control, "stop requested; stopping the operator");
                self.host.report_stopping();
                self.child.stop(&handle, self.config.stop_grace)?;
                return Ok(LoopExit::ControlStop(control));
            }

            let Some(status) = self.child.try_wait(&handle)? else {
                if consecutive_crashes > 0 && spawned_at.elapsed() >= self.config.healthy_reset {
                    consecutive_crashes = 0;
                }
                std::thread::sleep(self.config.poll_interval);
                continue;
            };

            match status {
                ExitStatus::Code(0) => {
                    info!("operator exited cleanly without a stop request; respawning");
                    consecutive_crashes = 0;
                    handle = self.spawn_current_and_gate()?;
                    spawned_at = Instant::now();
                }
                ExitStatus::Code(EXIT_CODE_UPDATE_HANDOFF) => {
                    match self.validated_pending()? {
                        Some(pending) => {
                            info!(version = %pending.version, "update handoff received; swapping");
                            let attempt = self.next_attempt(&pending)?;
                            handle = self.perform_swap(&pending, attempt)?;
                            spawned_at = Instant::now();
                            consecutive_crashes = 0;
                        }
                        None => {
                            warn!("handoff exit (10) without a valid pending stage; treating as a crash");
                            self.store.clear_pending()?;
                            consecutive_crashes += 1;
                            if let Some(exit) = self.backoff_pause(consecutive_crashes)? {
                                return Ok(exit);
                            }
                            handle = self.spawn_current_and_gate()?;
                            spawned_at = Instant::now();
                        }
                    }
                }
                other => {
                    warn!(?other, "operator crashed; respawning with backoff");
                    consecutive_crashes += 1;
                    if let Some(exit) = self.backoff_pause(consecutive_crashes)? {
                        return Ok(exit);
                    }
                    handle = self.spawn_current_and_gate()?;
                    spawned_at = Instant::now();
                }
            }
        }
    }

    /// Sleep the crash backoff (1·2ⁿ⁻¹ × base, capped), staying responsive to
    /// stop controls. Returns `Some(exit)` if a control arrived mid-backoff.
    fn backoff_pause(&mut self, consecutive_crashes: u32) -> Result<Option<LoopExit>> {
        let factor = 2u32.saturating_pow(consecutive_crashes.saturating_sub(1));
        let delay = self
            .config
            .restart_backoff_base
            .saturating_mul(factor)
            .min(self.config.restart_backoff_cap);
        let deadline = Instant::now() + delay;
        while Instant::now() < deadline {
            if let Some(control) = self.host.poll_control() {
                self.host.report_stopping();
                return Ok(Some(LoopExit::ControlStop(control)));
            }
            std::thread::sleep(self.config.poll_interval.min(deadline - Instant::now()));
        }
        Ok(None)
    }

    // -- swap / promote / rollback (the protocol's swap ordering) -----------

    /// Swap steps 1–6 of the handoff protocol for a validated pending stage.
    fn perform_swap(&mut self, pending: &PendingMarker, attempt: u32) -> Result<OperatorHandle> {
        // Preflight — an out-of-space attempt aborts cleanly before any
        // mutation, recorded as a spawn-phase failure.
        if let Err(err) = self.store.free_space_for_snapshot() {
            warn!(%err, "disk-space preflight failed; aborting the update attempt");
            self.record_failure(&pending.version, &pending.sha256, attempt,
                OperatorUpdatePhase::Spawn, format!("disk-space preflight failed: {err}"))?;
            self.store.clear_pending()?;
            return self.spawn_current_and_gate();
        }

        let old = self.store.current()?.ok_or_else(|| {
            corrupt(self.store, "swap requested but the store has no current version")
        })?;

        // 1. snapshot state/ (logging the copy cost so growth is visible).
        let state_bytes = self.store.state_size()?;
        let snapshot_started = Instant::now();
        self.store.snapshot_state(&old)?;
        info!(
            state_bytes,
            duration_ms = snapshot_started.elapsed().as_millis() as u64,
            old = %old,
            new = %pending.version,
            "state snapshot taken"
        );

        // 2. probation marker — before the flip, so any later crash is
        // classifiable.
        let probation = ProbationMarker {
            new: pending.version.clone(),
            old,
            started_at: Utc::now(),
            attempt,
        };
        self.store.write_probation(&probation)?;

        self.flip_spawn_and_gate(&probation, pending)
    }

    /// Protocol steps 3–6: flip `current`, spawn, gate, then promote or roll back.
    fn flip_spawn_and_gate(
        &mut self,
        probation: &ProbationMarker,
        pending: &PendingMarker,
    ) -> Result<OperatorHandle> {
        // 3. flip.
        self.store.set_current(&probation.new)?;

        // 4. spawn — a spawn failure never ran the new binary, so the state
        // is untouched: flip back, record, clean up, restart the old version.
        let handle = match self.spawn_version(&probation.new) {
            Ok(handle) => handle,
            Err(err) => {
                warn!(%err, new = %probation.new, "spawning the new operator failed; reverting");
                self.record_failure(&probation.new, &pending.sha256, probation.attempt,
                    OperatorUpdatePhase::Spawn, format!("spawn failed: {err}"))?;
                self.store.set_current(&probation.old)?;
                self.store.clear_probation()?;
                self.store.clear_pending()?;
                return self.spawn_current_and_gate();
            }
        };

        // 5. the probation gate.
        match self.gate(&handle, self.config.probation_window)? {
            GateOutcome::Ready => {
                // 6a. promote.
                self.promote(probation)?;
                info!(version = %probation.new, "promoted");
                Ok(handle)
            }
            outcome => {
                // 6b. rollback.
                self.rollback(probation, Some(&handle), gate_failure_message(outcome))
            }
        }
    }

    /// Promote cleanup — idempotent, re-runnable from any crash point within.
    fn promote(&mut self, probation: &ProbationMarker) -> Result<()> {
        self.store.set_last_stable(&probation.new)?;
        self.host.report_ready();
        self.store.clear_probation()?;
        self.store.clear_pending()?;
        self.store.drop_snapshot(&probation.old)?;
        self.store.gc(&[])?;
        Ok(())
    }

    /// Rollback — stop the failed child (if any), restore the (binary +
    /// state) pair, record the failure, and restart the old version.
    fn rollback(
        &mut self,
        probation: &ProbationMarker,
        failed_child: Option<&OperatorHandle>,
        message: String,
    ) -> Result<OperatorHandle> {
        warn!(new = %probation.new, old = %probation.old, %message, "rolling back");
        if let Some(handle) = failed_child {
            self.child.stop(handle, self.config.stop_grace)?;
        }
        let restore_to = self.store.last_stable()?.unwrap_or_else(|| probation.old.clone());
        self.store.set_current(&restore_to)?;
        self.store.restore_state(&probation.old)?;
        let sha256 = self
            .store
            .read_pending()?
            .map(|pending| pending.sha256)
            .unwrap_or_default();
        self.record_failure(
            &probation.new,
            &sha256,
            probation.attempt,
            OperatorUpdatePhase::Apply,
            message,
        )?;
        self.store.clear_probation()?;
        self.store.clear_pending()?;
        self.spawn_current_and_gate()
    }

    fn record_failure(
        &mut self,
        version: &Version,
        sha256: &str,
        attempts: u32,
        phase: OperatorUpdatePhase,
        message: String,
    ) -> Result<()> {
        self.store.write_failure(&FailureRecord {
            version: version.clone(),
            sha256: sha256.to_string(),
            phase,
            message,
            attempts,
            last_failed_at: Utc::now(),
        })
    }

    // -- gate + spawn helpers ----------------------------------------------

    /// Poll `/readyz` until ready, child exit, or the deadline.
    fn gate(&mut self, handle: &OperatorHandle, window: Duration) -> Result<GateOutcome> {
        let url = self.config.readyz_url();
        let deadline = Instant::now() + window;
        loop {
            if let Some(status) = self.child.try_wait(handle)? {
                return Ok(GateOutcome::ChildExited(status));
            }
            if self.probe.is_ready(&url) {
                return Ok(GateOutcome::Ready);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(GateOutcome::TimedOut);
            }
            std::thread::sleep(self.config.probe_interval.min(deadline - now));
        }
    }

    /// Spawn `current` and run a best-effort readiness gate: on ready, report
    /// ready to the host and — on a first install with no fallback yet —
    /// promote `current` to `last-stable`. A gate timeout here keeps
    /// supervising (the manager-side heartbeat is the ground truth); a child
    /// exit is handed back for the supervise loop to classify.
    fn spawn_current_and_gate(&mut self) -> Result<OperatorHandle> {
        let current = self.store.current()?.ok_or_else(|| {
            corrupt(self.store, "no current version to spawn — install is incomplete")
        })?;
        let handle = self.spawn_version(&current)?;
        match self.gate(&handle, self.config.probation_window)? {
            GateOutcome::Ready => {
                if self.store.last_stable()?.is_none() {
                    info!(version = %current, "first install passed the gate; recording last-stable");
                    self.store.set_last_stable(&current)?;
                }
                self.host.report_ready();
            }
            GateOutcome::TimedOut => {
                warn!(version = %current, "operator did not become ready within the window; supervising anyway");
            }
            GateOutcome::ChildExited(status) => {
                // Hand the exit to the supervise loop's contract by just
                // returning — try_wait will re-observe it immediately.
                warn!(?status, "operator exited during the startup gate");
            }
        }
        Ok(handle)
    }

    fn spawn_version(&mut self, version: &Version) -> Result<OperatorHandle> {
        let binary = self
            .store
            .stage_dir(version)
            .join(&self.config.operator_binary);
        self.child.spawn(&binary, &self.config.update_env)
    }

    /// Read + validate `pending.json` in one step for the handoff path.
    fn validated_pending(&mut self) -> Result<Option<PendingMarker>> {
        let Some(pending) = self.store.read_pending()? else {
            return Ok(None);
        };
        let binary = self
            .store
            .stage_dir(&pending.version)
            .join(&self.config.operator_binary);
        if store_common::validate_staged_binary(&binary, &pending.sha256)? {
            Ok(Some(pending))
        } else {
            Ok(None)
        }
    }

    /// Attempt numbering: continue the count from a prior failure of the SAME
    /// artifact (version + sha256); a different artifact starts fresh.
    fn next_attempt(&mut self, pending: &PendingMarker) -> Result<u32> {
        Ok(match self.store.read_failure(&pending.version)? {
            Some(record) if record.sha256 == pending.sha256 => record.attempts + 1,
            _ => 1,
        })
    }
}

fn gate_failure_message(outcome: GateOutcome) -> String {
    match outcome {
        GateOutcome::TimedOut => "readyz never returned 200 within the probation window".to_string(),
        GateOutcome::ChildExited(ExitStatus::Code(code)) => {
            format!("operator exited during probation with code {code}")
        }
        GateOutcome::ChildExited(status) => {
            format!("operator exited during probation ({status:?})")
        }
        GateOutcome::Ready => unreachable!("Ready is not a failure"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testing::{SpawnOutcome, StubChild, StubHost, StubProbe, StubStore};
    use std::path::PathBuf;

    fn version(s: &str) -> Version {
        Version::parse(s).expect("test version should parse")
    }

    fn test_config() -> RunConfig {
        RunConfig {
            probation_window: Duration::from_millis(200),
            probe_interval: Duration::from_millis(5),
            poll_interval: Duration::from_millis(5),
            heartbeat_interval: Duration::from_millis(10),
            stop_grace: Duration::from_millis(50),
            restart_backoff_base: Duration::from_millis(5),
            restart_backoff_cap: Duration::from_millis(40),
            healthy_reset: Duration::from_millis(150),
            max_swap_attempts: 3,
            operator_binary: "alien-operator".to_string(),
            ..RunConfig::default()
        }
    }

    /// Store with 1.3.5 installed, both pointers on it, and a state DB.
    fn base_store(dir: &std::path::Path) -> StubStore {
        let store = StubStore::new(dir);
        store.install_version(&version("1.3.5"));
        store.set_current(&version("1.3.5")).unwrap();
        store.set_last_stable(&version("1.3.5")).unwrap();
        std::fs::write(store.state_dir().join("db"), b"state-v1").unwrap();
        store
    }

    /// Install 1.4.0 and write a valid pending marker for it (real sha256).
    fn stage_valid(store: &StubStore, config: &RunConfig) -> PendingMarker {
        store.install_version(&version("1.4.0"));
        let binary = store.stage_dir(&version("1.4.0")).join(&config.operator_binary);
        let sha256 = store_common::file_sha256(&binary).unwrap();
        let pending = PendingMarker {
            version: version("1.4.0"),
            sha256,
            staged_at: Utc::now(),
        };
        store.write_pending(&pending).unwrap();
        pending
    }

    fn probation(attempt: u32, started_ago: Duration) -> ProbationMarker {
        ProbationMarker {
            new: version("1.4.0"),
            old: version("1.3.5"),
            started_at: Utc::now() - chrono::Duration::from_std(started_ago).unwrap(),
            attempt,
        }
    }

    macro_rules! machine {
        ($store:expr, $child:expr, $probe:expr, $host:expr, $config:expr) => {
            Machine {
                store: &$store,
                child: &mut $child,
                probe: &$probe,
                host: &$host,
                config: &$config,
            }
        };
    }

    fn assert_steady_promoted(store: &StubStore) {
        assert_eq!(store.current().unwrap(), Some(version("1.4.0")));
        assert_eq!(store.last_stable().unwrap(), Some(version("1.4.0")));
        assert!(store.read_pending().unwrap().is_none(), "pending cleared");
        assert!(store.read_probation().unwrap().is_none(), "probation cleared");
        assert!(
            !store.root().join("state-snapshots/1.3.5").exists(),
            "snapshot dropped on promote"
        );
    }

    fn assert_rolled_back(store: &StubStore, expected_attempts: u32) {
        assert_eq!(store.current().unwrap(), Some(version("1.3.5")));
        assert_eq!(store.last_stable().unwrap(), Some(version("1.3.5")));
        assert_eq!(
            std::fs::read(store.state_dir().join("db")).unwrap(),
            b"state-v1",
            "state restored from the snapshot"
        );
        assert!(store.read_pending().unwrap().is_none());
        assert!(store.read_probation().unwrap().is_none());
        let record = store
            .read_failure(&version("1.4.0"))
            .unwrap()
            .expect("failure record written");
        assert_eq!(record.attempts, expected_attempts);
        assert_eq!(record.phase, OperatorUpdatePhase::Apply);
    }

    // -- promote / rollback core flows ----------------------------------

    #[test]
    fn happy_promote() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        let pending = stage_valid(&store, &config);

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::ReadyAt(Instant::now() + Duration::from_millis(30));
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);

        let action = classify_startup(&store, &config).unwrap();
        assert_eq!(action, StartupAction::RunSwap { pending });
        let handle = m.execute_startup(action).unwrap();

        assert_steady_promoted(&store);
        assert_eq!(
            store.list_versions().unwrap(),
            vec![version("1.4.0")],
            "old version gc'd after promote"
        );
        assert_eq!(child.spawned.len(), 1, "exactly the new operator spawned");
        assert_eq!(
            child.spawned[0].0,
            store.stage_dir(&version("1.4.0")).join("alien-operator")
        );
        assert!(child.try_wait(&handle).unwrap().is_none(), "still running");
        assert!(
            host.ready_calls.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "READY reported on promote"
        );
    }

    #[test]
    fn rollback_when_probe_never_ready_and_state_restored() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);

        // The "new operator" (1.4.0) migrates the DB on spawn; rollback must
        // undo it. The old operator's respawn must NOT re-mutate, hence the
        // path filter.
        let state_db = store.state_dir().join("db");
        let mut child = StubChild::new([SpawnOutcome::UpNotReady, SpawnOutcome::UpNotReady]);
        child.on_spawn = Some(Box::new(move |binary: &std::path::Path| {
            if binary.to_string_lossy().contains("1.4.0") {
                std::fs::write(&state_db, b"MIGRATED").unwrap();
            }
        }));
        let probe = StubProbe::Always(false);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);

        let action = classify_startup(&store, &config).unwrap();
        m.execute_startup(action).unwrap();

        assert_rolled_back(&store, 1);
        assert_eq!(child.stop_calls.len(), 1, "failed child was stopped");
        assert_eq!(child.spawned.len(), 2, "old operator respawned");
        assert_eq!(
            child.spawned[1].0,
            store.stage_dir(&version("1.3.5")).join("alien-operator")
        );
        let record = store.read_failure(&version("1.4.0")).unwrap().unwrap();
        assert!(
            record.message.contains("probation window"),
            "message explains the timeout: {}",
            record.message
        );

        // Second identical attempt increments the count.
        let config2 = test_config();
        let pending2 = stage_valid(&store, &config2);
        let mut child2 = StubChild::new([SpawnOutcome::UpNotReady, SpawnOutcome::UpNotReady]);
        let probe2 = StubProbe::Always(false);
        let host2 = StubHost::new();
        let mut m2 = machine!(store, child2, probe2, host2, config2);
        let attempt = m2.next_attempt(&pending2).unwrap();
        assert_eq!(attempt, 2, "same artifact continues the attempt count");
        m2.execute_startup(StartupAction::RunSwap { pending: pending2 }).unwrap();
        assert_rolled_back(&store, 2);
    }

    #[test]
    fn rollback_when_child_crashes_during_probation() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);

        let mut child = StubChild::new([
            SpawnOutcome::ExitImmediately(1),
            SpawnOutcome::UpNotReady,
        ]);
        let probe = StubProbe::Always(false);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);

        let action = classify_startup(&store, &config).unwrap();
        m.execute_startup(action).unwrap();

        assert_rolled_back(&store, 1);
        let record = store.read_failure(&version("1.4.0")).unwrap().unwrap();
        assert!(
            record.message.contains("code 1"),
            "message names the exit: {}",
            record.message
        );
    }

    #[test]
    fn disk_preflight_failure_aborts_cleanly_before_any_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        store
            .available_bytes
            .store(0, std::sync::atomic::Ordering::SeqCst);

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);

        let action = classify_startup(&store, &config).unwrap();
        m.execute_startup(action).unwrap();

        // Old version still running; nothing mutated beyond the cleared pending.
        assert_eq!(store.current().unwrap(), Some(version("1.3.5")));
        assert!(store.read_pending().unwrap().is_none());
        assert!(store.read_probation().unwrap().is_none());
        let record = store.read_failure(&version("1.4.0")).unwrap().unwrap();
        assert_eq!(record.phase, OperatorUpdatePhase::Spawn);
        assert_eq!(
            child.spawned[0].0,
            store.stage_dir(&version("1.3.5")).join("alien-operator"),
            "the OLD operator was (re)spawned"
        );
    }

    // -- startup classification: one test per recovery-table row ---------

    #[test]
    fn classify_row1_steady_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        assert_eq!(
            classify_startup(&store, &config).unwrap(),
            StartupAction::SpawnCurrent
        );

        // Execute: spawns current; with last-stable unset (first install), a
        // passed gate records it.
        store.set_last_stable(&version("1.3.5")).unwrap();
        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(StartupAction::SpawnCurrent).unwrap();
        assert_eq!(child.spawned.len(), 1);
    }

    #[test]
    fn classify_row1_first_install_sets_last_stable_after_gate() {
        let dir = tempfile::tempdir().unwrap();
        let store = StubStore::new(dir.path());
        store.install_version(&version("1.3.5"));
        store.set_current(&version("1.3.5")).unwrap();
        // No last-stable: fresh install.
        let config = test_config();

        assert_eq!(
            classify_startup(&store, &config).unwrap(),
            StartupAction::SpawnCurrent
        );
        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(StartupAction::SpawnCurrent).unwrap();
        assert_eq!(
            store.last_stable().unwrap(),
            Some(version("1.3.5")),
            "first passed gate records last-stable"
        );
        assert!(host.ready_calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

    #[test]
    fn classify_row2_staged_valid() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        let pending = stage_valid(&store, &config);
        assert_eq!(
            classify_startup(&store, &config).unwrap(),
            StartupAction::RunSwap { pending }
        );
        // Execution == the happy-promote test.
    }

    #[test]
    fn classify_row2_guard_leftover_pending_after_promote() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        // Promote finished for 1.4.0 except pending removal.
        store.install_version(&version("1.4.0"));
        store.set_current(&version("1.4.0")).unwrap();
        store.set_last_stable(&version("1.4.0")).unwrap();
        let binary = store.stage_dir(&version("1.4.0")).join(&config.operator_binary);
        let pending = PendingMarker {
            version: version("1.4.0"),
            sha256: store_common::file_sha256(&binary).unwrap(),
            staged_at: Utc::now(),
        };
        store.write_pending(&pending).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert_eq!(action, StartupAction::DiscardLeftoverPending { pending });

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert!(store.read_pending().unwrap().is_none());
        assert_eq!(store.current().unwrap(), Some(version("1.4.0")));
    }

    #[test]
    fn classify_row3_invalid_pending_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        // Stage with a WRONG sha — simulates a partial/corrupt download.
        store.install_version(&version("1.4.0"));
        let pending = PendingMarker {
            version: version("1.4.0"),
            sha256: "0".repeat(64),
            staged_at: Utc::now(),
        };
        store.write_pending(&pending).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert_eq!(
            action,
            StartupAction::DiscardInvalidPending { pending }
        );

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert!(store.read_pending().unwrap().is_none(), "invalid pending deleted");
        assert_eq!(
            child.spawned[0].0,
            store.stage_dir(&version("1.3.5")).join("alien-operator"),
            "current (old) spawned, no swap"
        );
        assert_eq!(store.current().unwrap(), Some(version("1.3.5")));
    }

    #[test]
    fn classify_row4_mid_probation_resume_and_promote() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        // Crash happened mid-probation: current already flipped to new.
        store.snapshot_state(&version("1.3.5")).unwrap();
        store.write_probation(&probation(1, Duration::from_millis(50))).unwrap();
        store.set_current(&version("1.4.0")).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        let StartupAction::ResumeProbation { remaining, .. } = &action else {
            panic!("expected ResumeProbation, got {action:?}");
        };
        assert!(*remaining > Duration::ZERO && *remaining < config.probation_window);

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_steady_promoted(&store);
    }

    #[test]
    fn classify_row4_expired_probation_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        store.snapshot_state(&version("1.3.5")).unwrap();
        store.write_probation(&probation(1, Duration::from_secs(10))).unwrap();
        store.set_current(&version("1.4.0")).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        let StartupAction::ResumeProbation { remaining, .. } = &action else {
            panic!("expected ResumeProbation, got {action:?}");
        };
        assert_eq!(*remaining, Duration::ZERO, "window long expired");

        let mut child = StubChild::new([SpawnOutcome::UpNotReady, SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true); // even a ready probe cannot save an expired window
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_rolled_back(&store, 1);
    }

    #[test]
    fn classify_row4b_promote_began_finishes_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        // Crash after `set_last_stable(new)` but before the marker cleanup.
        store.snapshot_state(&version("1.3.5")).unwrap();
        store.write_probation(&probation(1, Duration::from_millis(10))).unwrap();
        store.set_current(&version("1.4.0")).unwrap();
        store.set_last_stable(&version("1.4.0")).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert!(
            matches!(action, StartupAction::FinishPromote { .. }),
            "got {action:?}"
        );

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_steady_promoted(&store);
    }

    #[test]
    fn classify_row5_pre_flip_crash_resumes_swap() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        // Crash after snapshot + probation marker, before the flip.
        store.snapshot_state(&version("1.3.5")).unwrap();
        store.write_probation(&probation(1, Duration::from_millis(10))).unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert!(
            matches!(action, StartupAction::ResumeSwapAtFlip { .. }),
            "got {action:?}"
        );

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_steady_promoted(&store);
    }

    #[test]
    fn classify_row5_attempt_cap_aborts() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        store.snapshot_state(&version("1.3.5")).unwrap();
        store
            .write_probation(&probation(config.max_swap_attempts, Duration::from_millis(10)))
            .unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert!(matches!(action, StartupAction::AbortSwap { .. }), "got {action:?}");

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_rolled_back(&store, config.max_swap_attempts);
    }

    #[test]
    fn classify_row6_mid_rollback_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);
        store.snapshot_state(&version("1.3.5")).unwrap();
        store.write_probation(&probation(1, Duration::from_millis(10))).unwrap();
        // Rollback got as far as recording the failure (its discriminator)…
        store
            .write_failure(&FailureRecord {
                version: version("1.4.0"),
                sha256: "beef".to_string(),
                phase: OperatorUpdatePhase::Apply,
                message: "gate failed before the crash".to_string(),
                attempts: 1,
                last_failed_at: Utc::now(),
            })
            .unwrap();
        // …and the state was mutated by the failed new version.
        std::fs::write(store.state_dir().join("db"), b"MIGRATED").unwrap();

        let action = classify_startup(&store, &config).unwrap();
        assert!(
            matches!(action, StartupAction::FinishRollback { .. }),
            "got {action:?}"
        );

        let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let mut m = machine!(store, child, probe, host, config);
        m.execute_startup(action).unwrap();
        assert_eq!(store.current().unwrap(), Some(version("1.3.5")));
        assert_eq!(
            std::fs::read(store.state_dir().join("db")).unwrap(),
            b"state-v1",
            "restore re-ran"
        );
        assert!(store.read_probation().unwrap().is_none());
        assert!(store.read_pending().unwrap().is_none());
    }

    // -- crash-injection matrix ------------------------------------------

    /// A store decorator that fails the k-th MUTATING operation, simulating a
    /// launcher crash at every swap-step boundary.
    struct FailingStore<'a> {
        inner: &'a StubStore,
        fail_at: std::cell::Cell<u32>,
        mutations: std::cell::Cell<u32>,
    }

    impl<'a> FailingStore<'a> {
        fn new(inner: &'a StubStore, fail_at: u32) -> Self {
            Self {
                inner,
                fail_at: std::cell::Cell::new(fail_at),
                mutations: std::cell::Cell::new(0),
            }
        }

        fn trip(&self, op: &str) -> Result<()> {
            let n = self.mutations.get() + 1;
            self.mutations.set(n);
            if n == self.fail_at.get() {
                Err(AlienError::new(ErrorData::Other {
                    message: format!("injected crash at mutation #{n} ({op})"),
                }))
            } else {
                Ok(())
            }
        }
    }

    impl VersionStore for FailingStore<'_> {
        fn stage_dir(&self, v: &Version) -> PathBuf {
            self.inner.stage_dir(v)
        }
        fn current(&self) -> Result<Option<Version>> {
            self.inner.current()
        }
        fn last_stable(&self) -> Result<Option<Version>> {
            self.inner.last_stable()
        }
        fn set_current(&self, v: &Version) -> Result<()> {
            self.trip("set_current")?;
            self.inner.set_current(v)
        }
        fn set_last_stable(&self, v: &Version) -> Result<()> {
            self.trip("set_last_stable")?;
            self.inner.set_last_stable(v)
        }
        fn snapshot_state(&self, tag: &Version) -> Result<()> {
            self.trip("snapshot_state")?;
            self.inner.snapshot_state(tag)
        }
        fn restore_state(&self, tag: &Version) -> Result<()> {
            self.trip("restore_state")?;
            self.inner.restore_state(tag)
        }
        fn drop_snapshot(&self, tag: &Version) -> Result<()> {
            self.trip("drop_snapshot")?;
            self.inner.drop_snapshot(tag)
        }
        fn state_size(&self) -> Result<u64> {
            self.inner.state_size()
        }
        fn gc(&self, keep: &[Version]) -> Result<()> {
            self.trip("gc")?;
            self.inner.gc(keep)
        }
        fn read_pending(&self) -> Result<Option<PendingMarker>> {
            self.inner.read_pending()
        }
        fn write_pending(&self, m: &PendingMarker) -> Result<()> {
            self.trip("write_pending")?;
            self.inner.write_pending(m)
        }
        fn clear_pending(&self) -> Result<()> {
            self.trip("clear_pending")?;
            self.inner.clear_pending()
        }
        fn read_probation(&self) -> Result<Option<ProbationMarker>> {
            self.inner.read_probation()
        }
        fn write_probation(&self, m: &ProbationMarker) -> Result<()> {
            self.trip("write_probation")?;
            self.inner.write_probation(m)
        }
        fn clear_probation(&self) -> Result<()> {
            self.trip("clear_probation")?;
            self.inner.clear_probation()
        }
        fn read_failure(&self, v: &Version) -> Result<Option<FailureRecord>> {
            self.inner.read_failure(v)
        }
        fn write_failure(&self, r: &FailureRecord) -> Result<()> {
            self.trip("write_failure")?;
            self.inner.write_failure(r)
        }
        fn list_versions(&self) -> Result<Vec<Version>> {
            self.inner.list_versions()
        }
        fn free_space_for_snapshot(&self) -> Result<()> {
            self.inner.free_space_for_snapshot()
        }
    }

    /// Crash-inject at every mutating store call of the promote path, then
    /// recover with a fresh machine over the same tempdir and assert the
    /// system converges to a coherent terminal state (promoted or rolled
    /// back) with all markers cleaned.
    #[test]
    fn crash_injection_matrix_converges_from_every_step() {
        // A successful promote performs 8 mutations:
        // snapshot, write_probation, set_current, set_last_stable,
        // clear_probation, clear_pending, drop_snapshot, gc.
        for fail_at in 1..=8u32 {
            let dir = tempfile::tempdir().unwrap();
            let store = base_store(dir.path());
            let config = test_config();
            stage_valid(&store, &config);

            // Attempt 1: crash injected at mutation #fail_at.
            {
                let failing = FailingStore::new(&store, fail_at);
                let mut child = StubChild::new([SpawnOutcome::UpNotReady]);
                let probe = StubProbe::Always(true);
                let host = StubHost::new();
                let mut m = machine!(failing, child, probe, host, config);
                let action = classify_startup(&failing, &config).unwrap();
                let result = m.execute_startup(action);
                assert!(
                    result.is_err(),
                    "fail_at={fail_at}: the injected crash must surface"
                );
            }

            // "Restart": classify the same store and let recovery run clean.
            let mut child = StubChild::new([
                SpawnOutcome::UpNotReady,
                SpawnOutcome::UpNotReady,
                SpawnOutcome::UpNotReady,
            ]);
            let probe = StubProbe::Always(true);
            let host = StubHost::new();
            let mut m = machine!(store, child, probe, host, config);
            let action = classify_startup(&store, &config).unwrap();
            m.execute_startup(action)
                .unwrap_or_else(|e| panic!("fail_at={fail_at}: recovery must succeed: {e}"));

            // Converged: markers gone, pointers coherent, exactly one of
            // promoted / rolled-back.
            assert!(
                store.read_probation().unwrap().is_none(),
                "fail_at={fail_at}: probation cleaned"
            );
            assert!(
                store.read_pending().unwrap().is_none(),
                "fail_at={fail_at}: pending cleaned"
            );
            let current = store.current().unwrap().expect("current set");
            let stable = store.last_stable().unwrap().expect("stable set");
            assert_eq!(
                current, stable,
                "fail_at={fail_at}: pointers agree after convergence"
            );
            assert!(
                current == version("1.4.0") || current == version("1.3.5"),
                "fail_at={fail_at}: terminal version is one of the pair"
            );
            if current == version("1.3.5") {
                assert_eq!(
                    std::fs::read(store.state_dir().join("db")).unwrap(),
                    b"state-v1",
                    "fail_at={fail_at}: rolled back ⇒ state restored"
                );
            }
        }
    }

    /// Same matrix on the ROLLBACK path (probe never ready): inject at the
    /// rollback's own mutations and assert recovery still converges.
    #[test]
    fn crash_injection_on_rollback_path_converges() {
        // Failing gate ⇒ mutations: snapshot(1), write_probation(2),
        // set_current(3), [gate fails] set_current(4, flip back),
        // restore_state(5), write_failure(6), clear_probation(7),
        // clear_pending(8).
        for fail_at in 4..=8u32 {
            let dir = tempfile::tempdir().unwrap();
            let store = base_store(dir.path());
            let config = test_config();
            stage_valid(&store, &config);

            {
                let failing = FailingStore::new(&store, fail_at);
                let mut child = StubChild::new([SpawnOutcome::UpNotReady, SpawnOutcome::UpNotReady]);
                let probe = StubProbe::Always(false);
                let host = StubHost::new();
                let mut m = machine!(failing, child, probe, host, config);
                let action = classify_startup(&failing, &config).unwrap();
                assert!(m.execute_startup(action).is_err(), "fail_at={fail_at}");
            }

            let mut child = StubChild::new([
                SpawnOutcome::UpNotReady,
                SpawnOutcome::UpNotReady,
                SpawnOutcome::UpNotReady,
            ]);
            let probe = StubProbe::Always(false);
            let host = StubHost::new();
            let mut m = machine!(store, child, probe, host, config);
            let action = classify_startup(&store, &config).unwrap();
            m.execute_startup(action)
                .unwrap_or_else(|e| panic!("fail_at={fail_at}: recovery must succeed: {e}"));

            assert!(store.read_probation().unwrap().is_none(), "fail_at={fail_at}");
            assert!(store.read_pending().unwrap().is_none(), "fail_at={fail_at}");
            assert_eq!(
                store.current().unwrap(),
                Some(version("1.3.5")),
                "fail_at={fail_at}: rollback path ends on the old version"
            );
            assert_eq!(
                std::fs::read(store.state_dir().join("db")).unwrap(),
                b"state-v1",
                "fail_at={fail_at}: state restored"
            );
        }
    }

    // -- supervise loop: exit-code contract + watchdog --------------------

    #[test]
    fn supervise_exit_code_contract() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        // Invalid pending present when a handoff-10 arrives → discarded.
        store.install_version(&version("1.4.0"));
        store
            .write_pending(&PendingMarker {
                version: version("1.4.0"),
                sha256: "0".repeat(64),
                staged_at: Utc::now(),
            })
            .unwrap();

        // Script: initial spawn exits 0 (clean) → respawn exits 10 with the
        // INVALID pending → treated as crash, pending deleted, backoff respawn
        // exits 7 (crash) → backoff respawn stays up → Stop.
        let mut child = StubChild::new([
            SpawnOutcome::ExitImmediately(0),
            SpawnOutcome::ExitImmediately(10),
            SpawnOutcome::ExitImmediately(7),
            SpawnOutcome::UpNotReady,
        ]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let controls = host.controls_tx.clone();
        let mut m = machine!(store, child, probe, host, config);

        let first = m.spawn_current_and_gate().unwrap();
        // Deliver Stop once the fourth child is up.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            controls.send(Control::Stop).unwrap();
        });
        let exit = m.supervise(first).unwrap();
        assert_eq!(exit, LoopExit::ControlStop(Control::Stop));

        assert_eq!(child.spawned.len(), 4, "0→respawn, 10-invalid→respawn, 7→respawn");
        assert!(
            store.read_pending().unwrap().is_none(),
            "invalid pending deleted on the bogus handoff"
        );
        assert!(
            store.current().unwrap() == Some(version("1.3.5")),
            "no swap ever happened"
        );
        assert_eq!(
            child.stop_calls.len(),
            1,
            "the last (running) child was stopped gracefully"
        );
        assert!(host.stopping_calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

    #[test]
    fn supervise_valid_handoff_swaps() {
        let dir = tempfile::tempdir().unwrap();
        let store = base_store(dir.path());
        let config = test_config();
        stage_valid(&store, &config);

        // Initial operator exits 10 (it staged 1.4.0) → swap → new stays up.
        let mut child = StubChild::new([
            SpawnOutcome::ExitImmediately(10),
            SpawnOutcome::UpNotReady,
        ]);
        let probe = StubProbe::Always(true);
        let host = StubHost::new();
        let controls = host.controls_tx.clone();
        let mut m = machine!(store, child, probe, host, config);

        let first = m.spawn_current_and_gate().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            controls.send(Control::Stop).unwrap();
        });
        let exit = m.supervise(first).unwrap();
        assert_eq!(exit, LoopExit::ControlStop(Control::Stop));
        assert_steady_promoted(&store);
    }
}
