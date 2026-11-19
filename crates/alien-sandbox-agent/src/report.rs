//! The agent's own account of the mounts, written where a command can read it: a status file and
//! a copy of the agent's log lines. Paths, counts and step results only; never a credential, a
//! file system id or any other value of a mount source.

use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Directory holding both files. Mode 0755, owned by the agent's user.
pub const DIR: &str = "/run/alien-sandbox";
const STATUS_FILE: &str = "mounts-status.json";
const LOG_FILE: &str = "agent.log";
/// Entries kept in the status file's lists, so a retrying caller cannot grow it without bound.
const KEPT: usize = 64;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Whether the declared-mounts variable was in the agent's environment.
    pub env_present: bool,
    /// How many mounts it declared; `None` when it was absent or could not be read.
    pub declared_count: Option<usize>,
    pub declared_paths: Vec<String>,
    /// Why the variable could not be read, when it could not.
    pub env_error: Option<String>,
    pub helper_started: bool,
    pub helper_cap_eff: Option<String>,
    /// Every `PUT /v1/mounts` received: the paths it carried.
    pub puts: Vec<Vec<String>>,
    /// Every step the helper was asked for, with its result.
    pub steps: Vec<Step>,
    /// The state of each declared mount now.
    pub mounts: Vec<MountState>,
}

#[derive(Debug, Serialize)]
pub struct Step {
    pub step: String,
    pub result: String,
}

#[derive(Debug, Serialize)]
pub struct MountState {
    pub path: String,
    pub state: String,
}

struct Report {
    dir: PathBuf,
    status: Status,
}

static REPORT: Mutex<Option<Report>> = Mutex::new(None);

/// Starts the report in `dir` and writes the first status file.
pub fn init(dir: &Path, status: Status) {
    if let Ok(mut slot) = REPORT.lock() {
        *slot = Some(Report {
            dir: dir.to_path_buf(),
            status,
        });
    }
    update(|_| {});
}

/// Changes the status and rewrites the file. Does nothing before [`init`].
pub fn update(change: impl FnOnce(&mut Status)) {
    let Ok(mut slot) = REPORT.lock() else { return };
    let Some(report) = slot.as_mut() else { return };
    change(&mut report.status);
    trim(&mut report.status.puts);
    trim(&mut report.status.steps);
    if let Err(error) = write_status(&report.dir, &report.status) {
        eprintln!("alien-sandbox-agent: mounts: cannot write the status file: {error}");
    }
}

fn trim<T>(list: &mut Vec<T>) {
    if list.len() > KEPT {
        list.drain(..list.len() - KEPT);
    }
}

/// Prints the line on stderr and appends it to the log file.
pub fn log(line: &str) {
    eprintln!("{line}");
    let dir = REPORT
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|r| r.dir.clone()));
    if let Some(dir) = dir {
        let _ = append_log(&dir, line);
    }
}

fn prepare(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
}

fn write_status(dir: &Path, status: &Status) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    prepare(dir)?;
    let temporary = dir.join(format!("{STATUS_FILE}.tmp"));
    let body = serde_json::to_vec_pretty(status).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, body)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o644))?;
    std::fs::rename(&temporary, dir.join(STATUS_FILE))
}

fn append_log(dir: &Path, line: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    prepare(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o644)
        .open(dir.join(LOG_FILE))?;
    writeln!(file, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn status_and_log_are_world_readable_and_hold_no_values() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("alien-sandbox");
        init(
            &dir,
            Status {
                env_present: true,
                declared_count: Some(2),
                declared_paths: vec!["/workspace".into(), "/data".into()],
                ..Status::default()
            },
        );
        update(|s| {
            s.helper_started = true;
            s.helper_cap_eff = Some("CapEff: 0000000000200000".into());
            s.puts.push(vec!["/data".into()]);
            s.steps.push(Step {
                step: "tmpfs /workspace".into(),
                result: "ok".into(),
            });
        });
        log("alien-sandbox-agent: mounts: test line");
        let status = dir.join(STATUS_FILE);
        let log_path = dir.join(LOG_FILE);
        assert_eq!(std::fs::metadata(&status).unwrap().permissions().mode() & 0o777, 0o644);
        assert_eq!(std::fs::metadata(&log_path).unwrap().permissions().mode() & 0o777, 0o644);
        let parsed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&status).unwrap()).unwrap();
        assert_eq!(parsed["envPresent"], true);
        assert_eq!(parsed["declaredCount"], 2);
        assert_eq!(parsed["helperStarted"], true);
        assert_eq!(parsed["puts"][0][0], "/data");
        assert_eq!(parsed["steps"][0]["result"], "ok");
        assert!(std::fs::read_to_string(&log_path).unwrap().contains("test line"));
    }
}
