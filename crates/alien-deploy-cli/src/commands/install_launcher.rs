//! Launcher-based install layout for the operator OS service.
//!
//! The service the init system runs is `alien-launcher`; the launcher spawns
//! the operator from the version store and performs health-gated binary
//! swaps with last-stable rollback. The launcher itself is FROZEN — it never
//! rewrites its own binary — so re-running this installer is the one and only
//! way it changes ("redeploy"), and the install must therefore be
//! **idempotent and state-preserving**: `state/` and the secret files are
//! never recreated or overwritten (wiping them would re-initialize the
//! deployment identity and orphan the deployment on the manager), while the
//! binaries and the unit file are always refreshed.
//!
//! On-disk layout (under the data dir):
//!
//! ```text
//! launcher/alien-launcher          # the supervisor; replaced on redeploy only
//! versions/<v>/alien-operator      # installed operator version(s)
//! current      -> versions/<v>     # desired version (symlink)
//! last-stable  -> versions/<v>     # proven-good fallback (symlink)
//! state/                           # encrypted DB — NEVER touched here
//! sync-token, encryption-key       # secrets — written once, then reused
//! ```
//!
//! The operator's configuration flows through the unit's `Environment=`
//! lines: every operator flag has an env alias (SYNC_URL, SYNC_TOKEN_FILE,
//! DATA_DIR, …), and the launcher's child inherits the launcher's
//! environment, so no argv plumbing is needed through the supervisor.

#[cfg(unix)]
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::ffi::OsString;

use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(windows)]
use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceManager, ServiceStartCtx, ServiceStopCtx,
};

use crate::error::{ErrorData, Result};
use crate::output;

/// Shorthand for the operator-service error variant this module uses
/// throughout.
fn service_error(message: String) -> ErrorData {
    ErrorData::OperatorServiceError { message }
}

/// The launcher layout is the default on Linux and Windows (the OSes whose
/// service shim is wired); macOS keeps the legacy direct-operator service until
/// its launcher phase lands. `--no-launcher` forces legacy anywhere.
pub fn use_launcher_layout(no_launcher: bool) -> bool {
    use_launcher_layout_for(no_launcher, std::env::consts::OS)
}

fn use_launcher_layout_for(no_launcher: bool, os: &str) -> bool {
    !no_launcher && matches!(os, "linux" | "windows")
}

/// The operator binary filename inside `versions/<v>/` — `.exe` on Windows.
fn operator_binary_name() -> String {
    format!("alien-operator{}", std::env::consts::EXE_SUFFIX)
}

/// The launcher binary filename inside `launcher/` — `.exe` on Windows.
fn launcher_binary_name() -> String {
    format!("alien-launcher{}", std::env::consts::EXE_SUFFIX)
}

/// Locate the launcher binary: explicit flag → `ALIEN_LAUNCHER_BINARY` env →
/// next to the operator binary → on PATH.
pub fn which_launcher_binary(
    explicit: Option<PathBuf>,
    operator_binary: &Path,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path);
        }
        return Err(AlienError::new(service_error(format!(
            "--launcher-binary '{}' does not exist",
            path.display()
        ))));
    }
    if let Ok(env_path) = std::env::var("ALIEN_LAUNCHER_BINARY") {
        let path = PathBuf::from(&env_path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(AlienError::new(service_error(format!(
            "ALIEN_LAUNCHER_BINARY is set to '{env_path}' but the file does not exist"
        ))));
    }
    if let Some(sibling) = operator_binary
        .parent()
        .map(|dir| dir.join(launcher_binary_name()))
    {
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    which::which("alien-launcher").into_alien_error().context(
        service_error(
            "alien-launcher binary not found. Pass --launcher-binary, set \
             ALIEN_LAUNCHER_BINARY, or place it next to the operator binary"
                .to_string(),
        ),
    )
}

/// Ask a binary for its version (`<binary> --version` → last token).
pub fn binary_version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .into_alien_error()
        .context(service_error(format!(
            "failed to run '{} --version'",
            binary.display()
        )))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_version_output(&stdout).ok_or_else(|| {
        AlienError::new(service_error(format!(
            "could not parse a version from '{} --version' output: '{}'",
            binary.display(),
            stdout.trim()
        )))
    })
}

fn parse_version_output(stdout: &str) -> Option<String> {
    let token = stdout.split_whitespace().last()?;
    // Sanity: semver-ish (the store directory name must parse launcher-side).
    if token.split('.').count() >= 3 || token.split('.').count() == 3 {
        Some(token.to_string())
    } else {
        None
    }
}

/// Write (or refresh) the launcher layout under `data_dir`.
///
/// Normative idempotency rules:
/// - `state/` is created if missing and otherwise NEVER touched;
/// - binaries are ALWAYS refreshed (copy to a temp name + rename, so a
///   still-running old binary keeps its inode — no ETXTBSY, no torn write);
/// - the `current`/`last-stable` pointers are created only when absent: on a
///   redeploy over a live store they are the launcher's truth, not ours.
pub fn write_layout(
    data_dir: &Path,
    operator_binary: &Path,
    operator_version: &str,
    launcher_binary: &Path,
) -> Result<()> {
    for dir in ["versions", "state", "state-snapshots", "failed", "download", "launcher"] {
        std::fs::create_dir_all(data_dir.join(dir))
            .into_alien_error()
            .context(service_error(format!(
                "failed to create '{}'",
                data_dir.join(dir).display()
            )))?;
    }

    let version_dir = data_dir.join("versions").join(operator_version);
    std::fs::create_dir_all(&version_dir)
        .into_alien_error()
        .context(service_error(format!(
            "failed to create '{}'",
            version_dir.display()
        )))?;
    install_binary(operator_binary, &version_dir.join(operator_binary_name()))?;
    install_binary(
        launcher_binary,
        &data_dir.join("launcher").join(launcher_binary_name()),
    )?;

    let target = Path::new("versions").join(operator_version);
    ensure_pointer(data_dir, "current", &target)?;
    ensure_pointer(data_dir, "last-stable", &target)?;
    Ok(())
}

/// Copy a binary into place via temp + rename; always refreshes; preserves a
/// running old inode.
fn install_binary(from: &Path, to: &Path) -> Result<()> {
    let tmp = to.with_extension("tmp");
    std::fs::copy(from, &tmp)
        .into_alien_error()
        .context(service_error(format!(
            "failed to copy '{}' to '{}'",
            from.display(),
            tmp.display()
        )))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(service_error(format!(
                "failed to mark '{}' executable",
                tmp.display()
            )))?;
    }
    std::fs::rename(&tmp, to)
        .into_alien_error()
        .context(service_error(format!(
            "failed to move '{}' into place",
            to.display()
        )))
}

/// Create a pointer only if it does not exist yet — a symlink on Unix, a
/// directory junction on Windows (which needs an absolute target, so the
/// relative `target` is rooted at `data_dir`). Matches the launcher's store,
/// which reads the version from the pointer target's final component.
fn ensure_pointer(data_dir: &Path, name: &str, target: &Path) -> Result<()> {
    let path = data_dir.join(name);
    if path.symlink_metadata().is_ok() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &path)
            .into_alien_error()
            .context(service_error(format!(
                "failed to create the '{name}' pointer"
            )))?;
    }
    #[cfg(windows)]
    {
        junction::create(data_dir.join(target), &path)
            .into_alien_error()
            .context(service_error(format!(
                "failed to create the '{name}' junction"
            )))?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = target;
        return Err(AlienError::new(service_error(
            "the launcher layout is only supported on Unix and Windows".to_string(),
        )));
    }
    Ok(())
}

/// Render the systemd unit for the launcher. `Type=notify` + `WatchdogSec`
/// let systemd supervise the launcher's liveness while the launcher owns the
/// operator's version health (two-level supervision).
pub fn render_unit(
    launcher_path: &Path,
    data_dir: &Path,
    service_user: Option<&str>,
    environment: &[(String, String)],
) -> String {
    let mut env_lines = String::new();
    for (key, value) in environment {
        env_lines.push_str(&format!("Environment=\"{key}={value}\"\n"));
    }
    let user_line = service_user
        .map(|user| format!("User={user}\n"))
        .unwrap_or_default();
    // StateDirectory only applies to the canonical /var/lib path; custom data
    // dirs rely on ReadWritePaths alone.
    let state_directory_line = if data_dir == Path::new("/var/lib/alien-operator") {
        "StateDirectory=alien-operator\n"
    } else {
        ""
    };

    format!(
        "\
[Unit]
Description=Alien Operator Launcher
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=main
WatchdogSec=60
ExecStart={launcher} --data-dir {data_dir}
Restart=always
RestartSec=2
{user_line}{state_directory_line}NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths={data_dir}
{env_lines}
[Install]
WantedBy=multi-user.target
",
        launcher = launcher_path.display(),
        data_dir = data_dir.display(),
    )
}

/// Install (or redeploy) the launcher-based service: stop it, refresh the
/// layout, then register + start it — systemd on Linux, the SCM on Windows.
pub fn install_launcher_service(
    service_label: &str,
    data_dir: &Path,
    operator_binary: &Path,
    launcher_binary: &Path,
    service_user: Option<&str>,
    environment: &[(String, String)],
) -> Result<()> {
    let operator_version = binary_version(operator_binary)?;

    output::step(1, 4, "Stopping the service (if running)");
    stop_service_best_effort(service_label);

    output::step(
        2,
        4,
        &format!("Writing the version store (operator {operator_version})"),
    );
    write_layout(data_dir, operator_binary, &operator_version, launcher_binary)?;

    register_and_start(service_label, data_dir, service_user, environment)
}

/// Stop the service if running (best-effort — a first install has nothing to
/// stop; the restart is what adopts the refreshed layout).
#[cfg(unix)]
fn stop_service_best_effort(service_label: &str) {
    let _ = systemctl(&["stop", &format!("{service_label}.service")]);
}

#[cfg(windows)]
fn stop_service_best_effort(service_label: &str) {
    if let (Ok(label), Ok(manager)) = (
        service_label.parse::<ServiceLabel>(),
        <dyn ServiceManager>::native(),
    ) {
        let _ = manager.stop(ServiceStopCtx { label });
    }
}

#[cfg(not(any(unix, windows)))]
fn stop_service_best_effort(_service_label: &str) {}

/// Linux: write the systemd unit, daemon-reload, enable + start.
#[cfg(unix)]
fn register_and_start(
    service_label: &str,
    data_dir: &Path,
    service_user: Option<&str>,
    environment: &[(String, String)],
) -> Result<()> {
    output::step(3, 4, "Installing the systemd unit");
    let launcher_path = data_dir.join("launcher").join(launcher_binary_name());
    let unit = render_unit(&launcher_path, data_dir, service_user, environment);
    let unit_path = PathBuf::from(format!("/etc/systemd/system/{service_label}.service"));
    write_file(&unit_path, unit.as_bytes())?;

    output::step(4, 4, "Enabling + starting the service");
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", &format!("{service_label}.service")])?;
    Ok(())
}

/// Windows: register `alien-launcher` as the SCM service, apply the doc-12
/// recovery config (`sc.exe` can't set restart policy via `create`), and start
/// it. The launcher passes its environment through to the operator child, so
/// the operator's config flows via the service `environment` — the mirror of
/// the systemd `Environment=` lines. `service_user` is Linux-only.
#[cfg(windows)]
fn register_and_start(
    service_label: &str,
    data_dir: &Path,
    _service_user: Option<&str>,
    environment: &[(String, String)],
) -> Result<()> {
    output::step(3, 4, "Registering the launcher service");
    let label: ServiceLabel = service_label.parse().map_err(|_| {
        AlienError::new(service_error(format!(
            "invalid service label '{service_label}'"
        )))
    })?;
    let manager = <dyn ServiceManager>::native()
        .into_alien_error()
        .context(service_error("no supported service manager found".to_string()))?;
    let launcher_path = data_dir.join("launcher").join(launcher_binary_name());
    manager
        .install(ServiceInstallCtx {
            label: label.clone(),
            program: launcher_path,
            args: vec![OsString::from("--data-dir"), OsString::from(data_dir)],
            contents: None,
            username: None,
            working_directory: None,
            environment: Some(environment.to_vec()),
            autostart: true,
            // `sc create` can't set restart policy; we apply the doc-12 recovery
            // config right after via `sc failure`. `Never` here avoids
            // service-manager's misleading "will not restart" warning.
            restart_policy: RestartPolicy::Never,
        })
        .into_alien_error()
        .context(service_error(
            "failed to register the launcher service".to_string(),
        ))?;

    configure_failure_actions(&label.to_qualified_name())?;

    output::step(4, 4, "Starting the service");
    manager
        .start(ServiceStartCtx { label })
        .into_alien_error()
        .context(service_error(
            "failed to start the launcher service".to_string(),
        ))?;
    Ok(())
}

/// Apply the SCM recovery config via `sc.exe failure` (doc 12).
#[cfg(windows)]
fn configure_failure_actions(service_name: &str) -> Result<()> {
    let status = Command::new("sc")
        .args(failure_action_args(service_name))
        .status()
        .into_alien_error()
        .context(service_error("failed to run 'sc failure'".to_string()))?;
    if !status.success() {
        return Err(AlienError::new(service_error(format!(
            "'sc failure' exited with {status}"
        ))));
    }
    Ok(())
}

/// The `sc failure` argument vector (doc 12): reset the failure counter after
/// 24 h and restart three times at 5 s each. OS-agnostic so it is unit-tested on
/// any host.
#[cfg(any(windows, test))]
fn failure_action_args(service_name: &str) -> Vec<String> {
    vec![
        "failure".to_string(),
        service_name.to_string(),
        "reset=".to_string(),
        "86400".to_string(),
        "actions=".to_string(),
        "restart/5000/restart/5000/restart/5000".to_string(),
    ]
}

#[cfg(unix)]
fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = std::fs::File::create(path)
        .into_alien_error()
        .context(service_error(format!(
            "failed to create '{}' (are you root?)",
            path.display()
        )))?;
    file.write_all(contents)
        .into_alien_error()
        .context(service_error(format!(
            "failed to write '{}'",
            path.display()
        )))
}

#[cfg(unix)]
fn systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .args(args)
        .status()
        .into_alien_error()
        .context(service_error(format!(
            "failed to run systemctl {args:?}"
        )))?;
    if !status.success() {
        return Err(AlienError::new(service_error(format!(
            "systemctl {args:?} exited with {status}"
        ))));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_decision_matrix() {
        assert!(use_launcher_layout_for(false, "linux"));
        assert!(use_launcher_layout_for(false, "windows"));
        assert!(!use_launcher_layout_for(true, "linux"), "--no-launcher wins");
        assert!(
            !use_launcher_layout_for(true, "windows"),
            "--no-launcher wins on Windows too"
        );
        assert!(
            !use_launcher_layout_for(false, "macos"),
            "macOS keeps legacy until its launcher phase"
        );
    }

    #[test]
    fn failure_actions_match_doc_12() {
        assert_eq!(
            failure_action_args("dev.alien.operator"),
            vec![
                "failure",
                "dev.alien.operator",
                "reset=",
                "86400",
                "actions=",
                "restart/5000/restart/5000/restart/5000",
            ]
        );
    }

    #[test]
    fn version_output_parses() {
        assert_eq!(parse_version_output("operator 1.11.2\n"), Some("1.11.2".to_string()));
        assert_eq!(
            parse_version_output("alien-launcher 1.11.2"),
            Some("1.11.2".to_string())
        );
        assert_eq!(parse_version_output(""), None);
        assert_eq!(parse_version_output("no version here"), None);
    }

    #[cfg(unix)]
    #[test]
    fn layout_is_exact_and_idempotency_preserves_state_and_secrets() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        let operator = root.path().join("alien-operator-artifact");
        let launcher = root.path().join("alien-launcher-artifact");
        std::fs::write(&operator, b"operator-v1-bytes").unwrap();
        std::fs::write(&launcher, b"launcher-v1-bytes").unwrap();

        write_layout(&data_dir, &operator, "1.11.2", &launcher).expect("fresh install");

        // Exact tree (sorted relative paths of files + symlinks).
        let mut entries: Vec<String> = walk(&data_dir);
        entries.sort();
        assert_eq!(
            entries,
            vec![
                "current".to_string(),
                "last-stable".to_string(),
                "launcher/alien-launcher".to_string(),
                "versions/1.11.2/alien-operator".to_string(),
            ],
            "empty dirs (state/, download/, …) plus exactly these entries"
        );
        for dir in ["state", "state-snapshots", "failed", "download"] {
            assert!(data_dir.join(dir).is_dir(), "{dir}/ must exist");
        }
        assert_eq!(
            std::fs::read_link(data_dir.join("current")).unwrap(),
            Path::new("versions/1.11.2")
        );
        // Executable bits set.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data_dir.join("versions/1.11.2/alien-operator"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);

        // Simulate a live system: state written, secrets present, current
        // moved forward by a self-update to a newer version.
        std::fs::write(data_dir.join("state/db"), b"live-state").unwrap();
        std::fs::write(data_dir.join("sync-token"), b"secret-token").unwrap();
        std::fs::create_dir_all(data_dir.join("versions/2.0.0")).unwrap();
        std::fs::remove_file(data_dir.join("current")).unwrap();
        std::os::unix::fs::symlink("versions/2.0.0", data_dir.join("current")).unwrap();

        // Redeploy with refreshed artifacts.
        std::fs::write(&operator, b"operator-v2-bytes").unwrap();
        std::fs::write(&launcher, b"launcher-v2-bytes").unwrap();
        write_layout(&data_dir, &operator, "1.11.2", &launcher).expect("redeploy");

        assert_eq!(
            std::fs::read(data_dir.join("state/db")).unwrap(),
            b"live-state",
            "state/ must survive a redeploy byte-identical"
        );
        assert_eq!(
            std::fs::read(data_dir.join("sync-token")).unwrap(),
            b"secret-token",
            "secrets must survive a redeploy"
        );
        assert_eq!(
            std::fs::read(data_dir.join("launcher/alien-launcher")).unwrap(),
            b"launcher-v2-bytes",
            "the launcher binary is always refreshed — this IS the redeploy"
        );
        assert_eq!(
            std::fs::read(data_dir.join("versions/1.11.2/alien-operator")).unwrap(),
            b"operator-v2-bytes"
        );
        assert_eq!(
            std::fs::read_link(data_dir.join("current")).unwrap(),
            Path::new("versions/2.0.0"),
            "a live store's pointers are the launcher's truth — never clobbered"
        );
    }

    /// Windows analogue: junction pointers + `.exe` binary names, same
    /// state/secret-preserving redeploy guarantees. Pointer reads resolve the
    /// junction target's final component (the version), like the launcher store.
    #[cfg(windows)]
    #[test]
    fn layout_is_exact_and_idempotency_preserves_state_and_secrets_windows() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        let operator = root.path().join("alien-operator-artifact");
        let launcher = root.path().join("alien-launcher-artifact");
        std::fs::write(&operator, b"operator-v1-bytes").unwrap();
        std::fs::write(&launcher, b"launcher-v1-bytes").unwrap();

        write_layout(&data_dir, &operator, "1.11.2", &launcher).expect("fresh install");

        let mut entries: Vec<String> = walk(&data_dir);
        entries.sort();
        assert_eq!(
            entries,
            vec![
                "current".to_string(),
                "last-stable".to_string(),
                "launcher/alien-launcher.exe".to_string(),
                "versions/1.11.2/alien-operator.exe".to_string(),
            ]
        );
        for dir in ["state", "state-snapshots", "failed", "download"] {
            assert!(data_dir.join(dir).is_dir(), "{dir}/ must exist");
        }
        let current = junction::get_target(data_dir.join("current")).unwrap();
        assert_eq!(current.file_name().unwrap().to_str().unwrap(), "1.11.2");

        // Live system: state + secrets present, current advanced by a self-update.
        std::fs::write(data_dir.join("state/db"), b"live-state").unwrap();
        std::fs::write(data_dir.join("sync-token"), b"secret-token").unwrap();
        std::fs::create_dir_all(data_dir.join("versions/2.0.0")).unwrap();
        std::fs::remove_dir(data_dir.join("current")).unwrap();
        junction::create(data_dir.join("versions/2.0.0"), data_dir.join("current")).unwrap();

        std::fs::write(&operator, b"operator-v2-bytes").unwrap();
        std::fs::write(&launcher, b"launcher-v2-bytes").unwrap();
        write_layout(&data_dir, &operator, "1.11.2", &launcher).expect("redeploy");

        assert_eq!(std::fs::read(data_dir.join("state/db")).unwrap(), b"live-state");
        assert_eq!(
            std::fs::read(data_dir.join("sync-token")).unwrap(),
            b"secret-token"
        );
        assert_eq!(
            std::fs::read(data_dir.join("launcher/alien-launcher.exe")).unwrap(),
            b"launcher-v2-bytes",
            "the launcher binary is always refreshed — this IS the redeploy"
        );
        let current = junction::get_target(data_dir.join("current")).unwrap();
        assert_eq!(
            current.file_name().unwrap().to_str().unwrap(),
            "2.0.0",
            "a live store's pointers are the launcher's truth — never clobbered"
        );
    }

    fn walk(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn rec(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = path.symlink_metadata().unwrap();
                if meta.is_dir() {
                    rec(root, &path, out);
                } else {
                    out.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        rec(root, root, &mut out);
        out
    }

    #[test]
    fn unit_file_golden() {
        let unit = render_unit(
            Path::new("/var/lib/alien-operator/launcher/alien-launcher"),
            Path::new("/var/lib/alien-operator"),
            None,
            &[
                ("PLATFORM".to_string(), "aws".to_string()),
                ("SYNC_URL".to_string(), "https://manager.example.com".to_string()),
                (
                    "SYNC_TOKEN_FILE".to_string(),
                    "/var/lib/alien-operator/sync-token".to_string(),
                ),
            ],
        );
        let expected = "\
[Unit]
Description=Alien Operator Launcher
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=main
WatchdogSec=60
ExecStart=/var/lib/alien-operator/launcher/alien-launcher --data-dir /var/lib/alien-operator
Restart=always
RestartSec=2
StateDirectory=alien-operator
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/alien-operator
Environment=\"PLATFORM=aws\"
Environment=\"SYNC_URL=https://manager.example.com\"
Environment=\"SYNC_TOKEN_FILE=/var/lib/alien-operator/sync-token\"

[Install]
WantedBy=multi-user.target
";
        assert_eq!(unit, expected);

        // Custom data dir: no StateDirectory, ReadWritePaths follows; user set.
        let unit = render_unit(
            Path::new("/opt/x/launcher/alien-launcher"),
            Path::new("/opt/x"),
            Some("alien"),
            &[],
        );
        assert!(unit.contains("User=alien\n"));
        assert!(!unit.contains("StateDirectory="));
        assert!(unit.contains("ReadWritePaths=/opt/x\n"));
        assert!(unit.contains("ExecStart=/opt/x/launcher/alien-launcher --data-dir /opt/x\n"));
    }
}
