//! Test stubs for the trait boundary — the state machine's entire test suite
//! runs against these, with no OS service, no real child process, and no
//! symlinks (so the suite passes identically on Linux, macOS, and Windows).
//!
//! `StubStore` is a REAL `VersionStore` over a tempdir (pointer files instead
//! of symlinks) built on `store_common` — so exercising the state machine
//! against it also exercises the shared helpers every platform store uses.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

use super::store_common;
use super::traits::{
    ChildSupervisor, Control, ExitStatus, FailureRecord, HealthProbe, OperatorHandle,
    PendingMarker, ProbationMarker, ServiceHost, UpdateEnv, Version, VersionStore,
};

// ---------------------------------------------------------------------------
// StubStore — a full VersionStore over a tempdir, symlink-free
// ---------------------------------------------------------------------------

/// Layout mirrors the real store; `current` / `last-stable` are pointer FILES
/// (`current.txt` / `last-stable.txt` holding the version string) so the stub
/// runs on every OS without symlink or junction support.
pub struct StubStore {
    root: PathBuf,
    /// Scripted "free disk space" for `free_space_for_snapshot`.
    /// Defaults to effectively-unlimited.
    pub available_bytes: AtomicU64,
}

impl StubStore {
    pub fn new(root: &Path) -> Self {
        for dir in ["versions", "state", "state-snapshots", "failed"] {
            std::fs::create_dir_all(root.join(dir)).expect("stub store layout should create");
        }
        Self {
            root: root.to_path_buf(),
            available_bytes: AtomicU64::new(u64::MAX),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    /// Create `versions/<v>/` with a placeholder binary, as staging would.
    pub fn install_version(&self, version: &Version) {
        let dir = self.stage_dir(version);
        std::fs::create_dir_all(&dir).expect("version dir should create");
        std::fs::write(dir.join("alien-operator"), format!("binary-{version}"))
            .expect("placeholder binary should write");
    }

    fn pointer_path(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.txt"))
    }

    fn read_pointer(&self, name: &str) -> Result<Option<Version>> {
        let path = self.pointer_path(name);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(AlienError::new(ErrorData::StoreCorrupt {
                    path: path.display().to_string(),
                    message: format!("failed to read pointer: {e}"),
                }));
            }
        };
        Version::parse(raw.trim())
            .map(Some)
            .map_err(|e| {
                AlienError::new(ErrorData::StoreCorrupt {
                    path: path.display().to_string(),
                    message: format!("pointer holds an unparseable version: {e}"),
                })
            })
    }

    fn write_pointer(&self, name: &str, version: &Version) -> Result<()> {
        // Pointer writes go through the same atomic temp+rename discipline.
        let path = self.pointer_path(name);
        let tmp = self.root.join(format!("{name}.txt.tmp"));
        std::fs::write(&tmp, version.as_str()).map_err(|e| {
            AlienError::new(ErrorData::Other {
                message: format!("failed to write pointer temp '{}': {e}", tmp.display()),
            })
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            AlienError::new(ErrorData::Other {
                message: format!("failed to commit pointer '{}': {e}", path.display()),
            })
        })
    }

    fn marker_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn failure_path(&self, version: &Version) -> PathBuf {
        self.root.join("failed").join(format!("{version}.json"))
    }
}

impl VersionStore for StubStore {
    fn stage_dir(&self, version: &Version) -> PathBuf {
        self.root.join("versions").join(version.as_str())
    }

    fn current(&self) -> Result<Option<Version>> {
        self.read_pointer("current")
    }

    fn last_stable(&self) -> Result<Option<Version>> {
        self.read_pointer("last-stable")
    }

    fn set_current(&self, version: &Version) -> Result<()> {
        self.write_pointer("current", version)
    }

    fn set_last_stable(&self, version: &Version) -> Result<()> {
        self.write_pointer("last-stable", version)
    }

    fn snapshot_state(&self, tag: &Version) -> Result<()> {
        store_common::snapshot_state_dir(
            &self.state_dir(),
            &self.root.join("state-snapshots"),
            tag,
        )
    }

    fn restore_state(&self, tag: &Version) -> Result<()> {
        store_common::restore_state_dir(
            &self.state_dir(),
            &self.root.join("state-snapshots"),
            tag,
        )
    }

    fn drop_snapshot(&self, tag: &Version) -> Result<()> {
        let dir = self.root.join("state-snapshots").join(tag.as_str());
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AlienError::new(ErrorData::Other {
                message: format!("failed to drop snapshot '{}': {e}", dir.display()),
            })),
        }
    }

    fn state_size(&self) -> Result<u64> {
        store_common::dir_size(&self.state_dir())
    }

    fn gc(&self, keep: &[Version]) -> Result<()> {
        let all = self.list_versions()?;
        let current = self.current()?;
        let last_stable = self.last_stable()?;
        for candidate in store_common::gc_candidates(
            &all,
            keep,
            current.as_ref(),
            last_stable.as_ref(),
        ) {
            std::fs::remove_dir_all(self.stage_dir(&candidate)).map_err(|e| {
                AlienError::new(ErrorData::Other {
                    message: format!("gc failed for version {candidate}: {e}"),
                })
            })?;
        }
        Ok(())
    }

    fn read_pending(&self) -> Result<Option<PendingMarker>> {
        store_common::read_marker(&self.marker_path("pending.json"))
    }

    fn write_pending(&self, marker: &PendingMarker) -> Result<()> {
        store_common::write_marker_atomic(&self.marker_path("pending.json"), marker)
    }

    fn clear_pending(&self) -> Result<()> {
        store_common::remove_marker(&self.marker_path("pending.json"))
    }

    fn read_probation(&self) -> Result<Option<ProbationMarker>> {
        store_common::read_marker(&self.marker_path("probation.json"))
    }

    fn write_probation(&self, marker: &ProbationMarker) -> Result<()> {
        store_common::write_marker_atomic(&self.marker_path("probation.json"), marker)
    }

    fn clear_probation(&self) -> Result<()> {
        store_common::remove_marker(&self.marker_path("probation.json"))
    }

    fn read_failure(&self, version: &Version) -> Result<Option<FailureRecord>> {
        store_common::read_marker(&self.failure_path(version))
    }

    fn write_failure(&self, record: &FailureRecord) -> Result<()> {
        let path = self.failure_path(&record.version);
        store_common::write_marker_atomic(&path, record)
    }

    fn list_versions(&self) -> Result<Vec<Version>> {
        let versions_dir = self.root.join("versions");
        let mut versions = Vec::new();
        for entry in std::fs::read_dir(&versions_dir).map_err(|e| {
            AlienError::new(ErrorData::Other {
                message: format!("failed to read '{}': {e}", versions_dir.display()),
            })
        })? {
            let entry = entry.map_err(|e| {
                AlienError::new(ErrorData::Other {
                    message: format!("failed to read versions entry: {e}"),
                })
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = Version::parse(&name).map_err(|e| {
                AlienError::new(ErrorData::StoreCorrupt {
                    path: entry.path().display().to_string(),
                    message: format!("versions/ entry is not a version: {e}"),
                })
            })?;
            versions.push(version);
        }
        versions.sort();
        Ok(versions)
    }

    fn free_space_for_snapshot(&self) -> Result<()> {
        let required = store_common::dir_size(&self.state_dir())?;
        store_common::check_space(
            required,
            self.available_bytes.load(Ordering::SeqCst),
            "state snapshot",
        )
    }
}

// ---------------------------------------------------------------------------
// StubChild — scripted ChildSupervisor
// ---------------------------------------------------------------------------

/// What a scripted spawn does.
#[derive(Debug, Clone)]
pub enum SpawnOutcome {
    /// Child runs and never becomes ready (probe stays false); exits only on `stop`.
    UpNotReady,
    /// Child runs; the paired `StubProbe` should be scripted to flip ready
    /// after the same duration.
    UpReadyAfter(Duration),
    /// Child exits immediately with this code (e.g. crash `1`, handoff `10`).
    ExitImmediately(i32),
}

#[derive(Debug)]
struct ChildState {
    outcome: SpawnOutcome,
    spawned_at: Instant,
    stopped: bool,
}

/// Scripted `ChildSupervisor`: each `spawn` consumes the next outcome from the
/// script and records what was spawned for assertions.
pub struct StubChild {
    script: VecDeque<SpawnOutcome>,
    children: Vec<ChildState>,
    /// Every spawn call: (binary path, env), for assertions.
    pub spawned: Vec<(PathBuf, UpdateEnv)>,
    /// pids passed to `stop`, for assertions.
    pub stop_calls: Vec<u32>,
    /// Optional hook run on every spawn — lets a test simulate the "new
    /// operator migrates the state DB" side effect between snapshot and
    /// rollback (the stub child is not a real process and touches nothing
    /// by itself).
    pub on_spawn: Option<Box<dyn FnMut(&Path) + Send>>,
}

impl StubChild {
    pub fn new(script: impl IntoIterator<Item = SpawnOutcome>) -> Self {
        Self {
            script: script.into_iter().collect(),
            children: Vec::new(),
            spawned: Vec::new(),
            stop_calls: Vec::new(),
            on_spawn: None,
        }
    }
}

impl ChildSupervisor for StubChild {
    fn spawn(&mut self, binary: &Path, env: &UpdateEnv) -> Result<OperatorHandle> {
        let outcome = self.script.pop_front().ok_or_else(|| {
            AlienError::new(ErrorData::SpawnFailed {
                binary_path: binary.display().to_string(),
                message: "stub script exhausted — test spawned more children than scripted"
                    .to_string(),
            })
        })?;
        if let Some(hook) = self.on_spawn.as_mut() {
            hook(binary);
        }
        self.spawned.push((binary.to_path_buf(), env.clone()));
        self.children.push(ChildState {
            outcome,
            spawned_at: Instant::now(),
            stopped: false,
        });
        // pid is the 1-based child index — unique per spawn within a test.
        Ok(OperatorHandle {
            pid: self.children.len() as u32,
        })
    }

    fn stop(&mut self, handle: &OperatorHandle, _grace: Duration) -> Result<()> {
        self.stop_calls.push(handle.pid);
        let child = self.child_mut(handle)?;
        child.stopped = true;
        Ok(())
    }

    fn try_wait(&mut self, handle: &OperatorHandle) -> Result<Option<ExitStatus>> {
        let child = self.child_mut(handle)?;
        if child.stopped {
            return Ok(Some(ExitStatus::Code(0)));
        }
        match child.outcome {
            SpawnOutcome::ExitImmediately(code) => Ok(Some(ExitStatus::Code(code))),
            SpawnOutcome::UpNotReady | SpawnOutcome::UpReadyAfter(_) => Ok(None),
        }
    }
}

impl StubChild {
    fn child_mut(&mut self, handle: &OperatorHandle) -> Result<&mut ChildState> {
        let index = handle.pid as usize - 1;
        self.children.get_mut(index).ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: format!("unknown stub child pid {}", handle.pid),
            })
        })
    }

    /// Test helper: how long ago the child for `handle` was spawned.
    pub fn spawned_elapsed(&self, handle: &OperatorHandle) -> Duration {
        self.children[handle.pid as usize - 1].spawned_at.elapsed()
    }
}

// ---------------------------------------------------------------------------
// StubHost — call-recording ServiceHost
// ---------------------------------------------------------------------------

/// Records lifecycle reporting for assertions; `next_control` blocks on a
/// channel the test feeds via `controls_tx`.
pub struct StubHost {
    pub ready_calls: AtomicU32,
    pub heartbeat_calls: AtomicU32,
    pub stopping_calls: AtomicU32,
    controls: Mutex<Receiver<Control>>,
    pub controls_tx: Sender<Control>,
}

impl StubHost {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            ready_calls: AtomicU32::new(0),
            heartbeat_calls: AtomicU32::new(0),
            stopping_calls: AtomicU32::new(0),
            controls: Mutex::new(rx),
            controls_tx: tx,
        }
    }
}

impl ServiceHost for StubHost {
    fn poll_control(&self) -> Option<Control> {
        self.controls
            .lock()
            .expect("controls receiver lock should not be poisoned")
            .try_recv()
            .ok()
    }

    fn report_ready(&self) {
        self.ready_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn heartbeat(&self) {
        self.heartbeat_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn report_stopping(&self) {
        self.stopping_calls.fetch_add(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// StubProbe — scripted HealthProbe
// ---------------------------------------------------------------------------

/// Scripted readiness for the probation gate.
pub enum StubProbe {
    /// Always this value.
    Always(bool),
    /// `false` until the instant, then `true` — pairs with
    /// `SpawnOutcome::UpReadyAfter`.
    ReadyAt(Instant),
}

impl HealthProbe for StubProbe {
    fn is_ready(&self, _url: &str) -> bool {
        match self {
            StubProbe::Always(ready) => *ready,
            StubProbe::ReadyAt(instant) => Instant::now() >= *instant,
        }
    }
}

/// A loopback `UpdateEnv` for tests.
pub fn test_update_env() -> UpdateEnv {
    UpdateEnv {
        health_addr: SocketAddr::from(([127, 0, 0, 1], 7799)),
        launcher_version: "0.1.0-test".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests — the stubs themselves must honor the trait contracts
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn version(s: &str) -> Version {
        Version::parse(s).expect("test version should parse")
    }

    fn store() -> (tempfile::TempDir, StubStore) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = StubStore::new(dir.path());
        (dir, store)
    }

    /// Pointers: unset → None; set → read back; flip → new value. No symlinks
    /// anywhere (the whole point of the stub).
    #[test]
    fn stub_store_pointers_roundtrip_without_symlinks() {
        let (_dir, store) = store();
        assert_eq!(store.current().unwrap(), None);
        assert_eq!(store.last_stable().unwrap(), None);

        store.install_version(&version("1.3.5"));
        store.set_current(&version("1.3.5")).unwrap();
        store.set_last_stable(&version("1.3.5")).unwrap();
        assert_eq!(store.current().unwrap(), Some(version("1.3.5")));
        assert_eq!(store.last_stable().unwrap(), Some(version("1.3.5")));

        store.install_version(&version("1.4.0"));
        store.set_current(&version("1.4.0")).unwrap();
        assert_eq!(store.current().unwrap(), Some(version("1.4.0")));
        assert_eq!(
            store.last_stable().unwrap(),
            Some(version("1.3.5")),
            "flipping current must not move last-stable"
        );
    }

    /// Markers ride the shared atomic helpers: write/read/clear round-trip and
    /// clearing twice stays Ok (idempotent).
    #[test]
    fn stub_store_markers_roundtrip_and_clear_idempotent() {
        let (_dir, store) = store();
        assert!(store.read_pending().unwrap().is_none());

        let pending = PendingMarker {
            version: version("1.4.0"),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            staged_at: "2026-07-08T12:00:00Z".parse().unwrap(),
        };
        store.write_pending(&pending).unwrap();
        assert_eq!(store.read_pending().unwrap(), Some(pending));
        store.clear_pending().unwrap();
        store.clear_pending().unwrap();
        assert!(store.read_pending().unwrap().is_none());

        let record = FailureRecord {
            version: version("1.4.0"),
            sha256: "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3f9c71a1e4a6f2e0e6d5c4b3a"
                .to_string(),
            phase: alien_core::sync::OperatorUpdatePhase::Apply,
            message: "probe timeout".to_string(),
            attempts: 1,
            last_failed_at: "2026-07-08T12:05:00Z".parse().unwrap(),
        };
        store.write_failure(&record).unwrap();
        assert_eq!(store.read_failure(&version("1.4.0")).unwrap(), Some(record));
        assert!(store.read_failure(&version("9.9.9")).unwrap().is_none());
    }

    /// gc through the store: current + last-stable survive, others go.
    #[test]
    fn stub_store_gc_preserves_pointer_targets() {
        let (_dir, store) = store();
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            store.install_version(&version(v));
        }
        store.set_current(&version("1.2.0")).unwrap();
        store.set_last_stable(&version("1.1.0")).unwrap();

        store.gc(&[]).unwrap();

        assert_eq!(
            store.list_versions().unwrap(),
            vec![version("1.1.0"), version("1.2.0")],
            "only the unpointed version is collected"
        );
    }

    /// snapshot + restore through the store round-trips state bytes.
    #[test]
    fn stub_store_snapshot_restore_roundtrip() {
        let (_dir, store) = store();
        std::fs::write(store.state_dir().join("db"), b"pre-migration").unwrap();
        store.snapshot_state(&version("1.3.5")).unwrap();
        std::fs::write(store.state_dir().join("db"), b"migrated!").unwrap();
        store.restore_state(&version("1.3.5")).unwrap();
        assert_eq!(
            std::fs::read(store.state_dir().join("db")).unwrap(),
            b"pre-migration"
        );
    }

    /// The scripted free-space check trips the shared DiskSpace policy.
    #[test]
    fn stub_store_free_space_scriptable() {
        let (_dir, store) = store();
        std::fs::write(store.state_dir().join("db"), vec![0u8; 1000]).unwrap();

        store.free_space_for_snapshot().expect("unlimited space passes");

        store.available_bytes.store(1100, Ordering::SeqCst);
        let err = store
            .free_space_for_snapshot()
            .expect_err("1000 needed + 20% headroom > 1100 available");
        assert_eq!(err.code, "DISK_SPACE");
    }

    /// StubChild: outcomes script spawn-by-spawn; handoff code 10 and crash
    /// codes surface through try_wait; stop() records and terminates.
    #[test]
    fn stub_child_scripts_outcomes() {
        let env = test_update_env();
        let mut child = StubChild::new([
            SpawnOutcome::ExitImmediately(10),
            SpawnOutcome::UpNotReady,
        ]);

        let first = child.spawn(Path::new("/versions/1.3.5/op"), &env).unwrap();
        assert_eq!(
            child.try_wait(&first).unwrap(),
            Some(ExitStatus::Code(10)),
            "scripted handoff exit"
        );

        let second = child.spawn(Path::new("/versions/1.4.0/op"), &env).unwrap();
        assert_eq!(child.try_wait(&second).unwrap(), None, "still running");
        child.stop(&second, Duration::from_secs(2)).unwrap();
        assert_eq!(child.try_wait(&second).unwrap(), Some(ExitStatus::Code(0)));

        assert_eq!(child.spawned.len(), 2);
        assert_eq!(child.stop_calls, vec![second.pid]);
        // Script exhausted → further spawns fail loudly.
        let err = child
            .spawn(Path::new("/versions/1.5.0/op"), &env)
            .expect_err("exhausted script must not silently succeed");
        assert_eq!(err.code, "SPAWN_FAILED");
    }

    /// StubHost records lifecycle calls and delivers scripted controls.
    #[test]
    fn stub_host_records_and_delivers_controls() {
        let host = StubHost::new();
        host.report_ready();
        host.heartbeat();
        host.heartbeat();
        host.report_stopping();
        assert_eq!(host.ready_calls.load(Ordering::SeqCst), 1);
        assert_eq!(host.heartbeat_calls.load(Ordering::SeqCst), 2);
        assert_eq!(host.stopping_calls.load(Ordering::SeqCst), 1);

        assert_eq!(host.poll_control(), None, "no control queued yet");
        host.controls_tx.send(Control::Stop).unwrap();
        assert_eq!(host.poll_control(), Some(Control::Stop));
        assert_eq!(host.poll_control(), None, "controls are drained once");
    }

    /// StubProbe: Always and ReadyAt behave as scripted.
    #[test]
    fn stub_probe_scripts_readiness() {
        assert!(StubProbe::Always(true).is_ready("http://127.0.0.1:7799/readyz"));
        assert!(!StubProbe::Always(false).is_ready("http://127.0.0.1:7799/readyz"));

        let soon = StubProbe::ReadyAt(Instant::now() + Duration::from_millis(50));
        assert!(!soon.is_ready("x"), "not ready before the instant");
        std::thread::sleep(Duration::from_millis(60));
        assert!(soon.is_ready("x"), "ready after the instant");
    }
}
