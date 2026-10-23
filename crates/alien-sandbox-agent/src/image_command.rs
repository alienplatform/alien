//! Runs the saved OCI command once under its image identity; exec identity stays separate.

use crate::{
    error::{ErrorData, Result},
    exec::ExecIdentity,
};
use alien_core::{
    sandbox_identity::ImageUser,
    sandbox_image::{SandboxImageCommand, IMAGE_COMMAND_PATH},
};
use alien_error::{AlienError, Context, IntoAlienError};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
};

/// One image process per agent boot. Its exit is observable but never triggers a restart.
#[derive(Default)]
pub struct ImageProcess {
    status: Arc<Mutex<ImageStatus>>,
    image_user: bool,
}

/// Image process lifecycle, independent of session/network readiness.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ImageStatus {
    /// Released images without a saved startup command retain their old behavior.
    NotConfigured,
    /// The provider lifecycle hook or native-file binding has not activated this boot.
    AwaitingActivation,
    /// Waiting for network checks and, when required, gateway identity delivery.
    #[default]
    Pending,
    /// OCI metadata contains no ENTRYPOINT or CMD.
    Empty,
    /// The command was successfully executed; no application readiness is implied.
    Running,
    /// The command exited. Session operations may continue.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// Startup or process observation failed. Never retry the boot process implicitly.
    Failed,
}

impl ImageProcess {
    /// New packages explicitly opt into OCI USER semantics. Released packages keep
    /// their exec identity even if their metadata happened to preserve USER.
    pub fn with_image_user() -> Self {
        Self {
            image_user: true,
            ..Default::default()
        }
    }

    /// Defer boot until the provider hook or authenticated binding activates it.
    pub fn awaiting_activation() -> Self {
        Self {
            status: Arc::new(Mutex::new(ImageStatus::AwaitingActivation)),
            image_user: true,
        }
    }

    pub fn activate(&self) -> Result<()> {
        let mut status = self.status.lock().map_err(|_| monitor_failed())?;
        if *status == ImageStatus::AwaitingActivation {
            *status = ImageStatus::Pending;
        }
        Ok(())
    }

    pub fn waiting_for_activation(&self) -> Result<bool> {
        Ok(self.status()? == ImageStatus::AwaitingActivation)
    }

    /// Compatibility for released images that never saved or started an OCI command.
    pub fn not_configured() -> Self {
        Self {
            status: Arc::new(Mutex::new(ImageStatus::NotConfigured)),
            image_user: false,
        }
    }

    pub fn status(&self) -> Result<ImageStatus> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| monitor_failed())
    }

    pub fn start_once(&self, identity: ExecIdentity, env: &BTreeMap<String, String>) -> Result<()> {
        self.start_once_from(Path::new(IMAGE_COMMAND_PATH), identity, env)
    }

    fn start_once_from(
        &self,
        path: &Path,
        identity: ExecIdentity,
        env: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut status = self.status.lock().map_err(|_| monitor_failed())?;
        match *status {
            ImageStatus::AwaitingActivation => {
                return Err(AlienError::new(ErrorData::EgressPathUnexpected))
            }
            ImageStatus::Pending => (),
            ImageStatus::Failed => return Err(monitor_failed()),
            _ => return Ok(()),
        }
        // Hold the lock until spawn has completed its pre-exec work. A simultaneous
        // session operation cannot observe "started" before the actual exec succeeds.
        let child = match start_from_with_env(path, identity, env, self.image_user) {
            Ok(child) => child,
            Err(error) => {
                *status = ImageStatus::Failed;
                return Err(error);
            }
        };
        let Some(mut child) = child else {
            *status = ImageStatus::Empty;
            return Ok(());
        };
        *status = ImageStatus::Running;
        let shared = Arc::clone(&self.status);
        tokio::spawn(async move {
            let outcome = match child.wait().await {
                Ok(exit) => {
                    tracing::info!(%exit, "image command exited; it will not be restarted");
                    ImageStatus::Exited {
                        code: exit.code(),
                        signal: exit.signal(),
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "could not reap image command");
                    ImageStatus::Failed
                }
            };
            match shared.lock() {
                Ok(mut status) => *status = outcome,
                Err(_) => tracing::error!("image process state lock is poisoned; health will fail"),
            }
        });
        Ok(())
    }
}

/// AWS snapshots the listening agent before Run. Image code must wait for that hook,
/// including packages whose declared network mode does not need transparent forwarding.
pub fn defer_to_run_hook() -> Result<bool> {
    const SETTING: &str = "ALIEN_SANDBOX_EGRESS_RUN_HOOK";
    match std::env::var(SETTING) {
        Ok(value) if value == "true" => Ok(true),
        Ok(value) if value == "false" => Ok(false),
        Err(std::env::VarError::NotPresent) => Ok(false),
        _ => Err(AlienError::new(ErrorData::ConfigInvalid {
            setting: SETTING.into(),
            reason: "must be true or false".into(),
        })),
    }
}

fn monitor_failed() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationFailed {
        operation: "image process lifecycle".into(),
        reason: "image startup or observation failed; this boot will not retry it".into(),
    })
}

#[cfg(test)]
fn start_from(path: &Path, identity: ExecIdentity) -> Result<Option<tokio::process::Child>> {
    start_from_with_env(path, identity, &Default::default(), false)
}

fn start_from_with_env(
    path: &Path,
    identity: ExecIdentity,
    env: &BTreeMap<String, String>,
    image_user: bool,
) -> Result<Option<tokio::process::Child>> {
    let failed = || ErrorData::OperationFailed {
        operation: "start image command".to_string(),
        reason: "the bundle must preserve the image's OCI command in root-owned metadata"
            .to_string(),
    };
    let bytes = fs::read(path).into_alien_error().context(failed())?;
    let image: SandboxImageCommand = serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(failed())?;
    let Some(program) = image.command.first() else {
        return Ok(None);
    };
    let image_identity = if image_user {
        let user = image
            .user
            .as_deref()
            .ok_or_else(|| AlienError::new(failed()))?;
        let passwd = fs::read_to_string("/etc/passwd")
            .into_alien_error()
            .context(failed())?;
        let group = fs::read_to_string("/etc/group")
            .into_alien_error()
            .context(failed())?;
        let resolved = ImageUser {
            user,
            passwd: &passwd,
            group: &group,
        }
        .image_identity()
        .context(failed())?;
        ExecIdentity {
            uid: resolved.uid,
            gid: resolved.gid,
        }
    } else {
        identity
    };
    #[cfg(target_os = "linux")]
    if image_identity.uid == 0 && crate::egress::legacy_from_env()?.is_some() {
        return Err(AlienError::new(ErrorData::ConfigInvalid {
            setting: "ALIEN_SANDBOX_EGRESS".into(),
            reason: "root image USER cannot run with legacy UID-scoped egress; rebuild with a gateway-backed policy".into(),
        }));
    }
    // Allocate before fork; enter the directory only after dropping privileges.
    let directory = CString::new(image.working_directory)
        .into_alien_error()
        .context(failed())?;
    let mut command = std::process::Command::new(program);
    command
        .args(&image.command[1..])
        .env_clear()
        .envs(image.env)
        .envs(env)
        .env_remove("ALIEN_SANDBOX_EGRESS_TOKEN")
        .env("ALIEN_SANDBOX_EXEC_UID", identity.uid.to_string())
        .env("ALIEN_SANDBOX_EXEC_GID", identity.gid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(root) = std::env::var_os("ALIEN_SANDBOX_ROOT") {
        command.env("ALIEN_SANDBOX_ROOT", root);
    }
    unsafe {
        command.pre_exec(move || {
            if image_user {
                #[cfg(target_os = "linux")]
                crate::privilege::drop_image_to(image_identity)?;
                #[cfg(not(target_os = "linux"))]
                return Err(std::io::Error::from_raw_os_error(libc::ENOTSUP));
            } else {
                crate::privilege::drop_to(image_identity)?;
            }
            if libc::chdir(directory.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    command
        .spawn()
        .map(Some)
        .into_alien_error()
        .context(failed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn process_fixture(
        command: Vec<String>,
    ) -> (tempfile::TempDir, std::path::PathBuf, ExecIdentity) {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o777)).unwrap();
        let path = temp.path().join("command.json");
        let image = SandboxImageCommand {
            user: None,
            command,
            env: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            working_directory: temp.path().display().to_string(),
        };
        fs::write(&path, serde_json::to_vec(&image).unwrap()).unwrap();
        let identity = ExecIdentity {
            uid: unsafe {
                if libc::geteuid() == 0 {
                    60000
                } else {
                    libc::geteuid()
                }
            },
            gid: unsafe {
                if libc::getegid() == 0 {
                    60000
                } else {
                    libc::getegid()
                }
            },
        };
        (temp, path, identity)
    }

    #[tokio::test]
    #[ignore = "requires root with Docker default capabilities and setpriv"]
    async fn saved_image_user_controls_boot_without_changing_exec_identity() {
        for (user, uid, gid) in [("root", 0, 0), ("", 0, 0), ("2001:2002", 2001, 2002)] {
            let script = if uid == 0 {
                "cat /proc/self/status > observed; setpriv --reuid 2000 --regid 2000 --clear-groups id -u > worker"
            } else {
                "cat /proc/self/status > observed"
            };
            let (temp, path, exec) =
                process_fixture(vec!["/bin/sh".into(), "-c".into(), script.into()]);
            let mut metadata: SandboxImageCommand =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            metadata.user = Some(user.into());
            fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
            let mut child = start_from_with_env(&path, exec, &Default::default(), true)
                .unwrap()
                .unwrap();
            assert!(child.wait().await.unwrap().success());
            let status = fs::read_to_string(temp.path().join("observed")).unwrap();
            assert!(
                status.contains(&format!("Uid:\t{uid}\t{uid}\t{uid}\t{uid}")),
                "{status}"
            );
            assert!(
                status.contains(&format!("Gid:\t{gid}\t{gid}\t{gid}\t{gid}")),
                "{status}"
            );
            let caps = if uid == 0 {
                crate::privilege::IMAGE_ROOT_CAPABILITIES
            } else {
                0
            };
            for set in ["CapEff", "CapPrm", "CapBnd"] {
                assert!(status.contains(&format!("{set}:\t{caps:016x}")), "{status}");
            }
            if uid == 0 {
                assert_eq!(
                    fs::read_to_string(temp.path().join("worker")).unwrap(),
                    "2000\n"
                );
            }
            assert_ne!(exec.uid, 0);
        }
    }

    #[tokio::test]
    async fn new_startup_refuses_metadata_without_an_image_user() {
        let (_temp, path, identity) = process_fixture(vec!["/bin/true".into()]);
        assert!(start_from_with_env(&path, identity, &Default::default(), true).is_err());
        let mut legacy = start_from(&path, identity).unwrap().unwrap();
        assert!(legacy.wait().await.unwrap().success());
    }

    #[tokio::test]
    async fn image_starts_once_without_waiting_for_readiness_and_retains_its_exit() {
        for deferred in [false, true] {
            let (temp, path, identity) = process_fixture(vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf started >> starts; while ! test -e release; do sleep 0.02; done; exit 7"
                    .into(),
            ]);
            let process = if deferred {
                ImageProcess {
                    status: Arc::new(Mutex::new(ImageStatus::AwaitingActivation)),
                    ..Default::default()
                }
            } else {
                ImageProcess::default()
            };
            if deferred {
                let refused = process
                    .start_once_from(&path, identity, &Default::default())
                    .unwrap_err();
                assert_eq!(refused.code, "EGRESS_PATH_UNEXPECTED");
                assert!(!temp.path().join("starts").exists());
                assert!(process.waiting_for_activation().unwrap());
                process.activate().unwrap();
            }
            process
                .start_once_from(&path, identity, &Default::default())
                .unwrap();
            // The app is still waiting for its own readiness condition. Spawn must return.
            assert_eq!(process.status().unwrap(), ImageStatus::Running);
            process
                .start_once_from(&path, identity, &Default::default())
                .unwrap();
            fs::write(temp.path().join("release"), b"ready").unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if matches!(process.status().unwrap(), ImageStatus::Exited { .. }) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("image exit must be observed");
            assert_eq!(
                process.status().unwrap(),
                ImageStatus::Exited {
                    code: Some(7),
                    signal: None
                }
            );
            process.activate().unwrap();
            process
                .start_once_from(&path, identity, &Default::default())
                .unwrap();
            assert_eq!(
                fs::read_to_string(temp.path().join("starts")).unwrap(),
                "started"
            );
            assert_eq!(
                process.status().unwrap(),
                ImageStatus::Exited {
                    code: Some(7),
                    signal: None
                }
            );
        }
    }

    #[tokio::test]
    async fn a_failed_boot_spawn_is_not_retried_by_a_later_session_operation() {
        let (temp, path, identity) = process_fixture(vec!["/nonexistent-image-entrypoint".into()]);
        let process = ImageProcess::default();
        assert!(process
            .start_once_from(&path, identity, &Default::default())
            .is_err());
        assert_eq!(process.status().unwrap(), ImageStatus::Failed);
        let mut image: SandboxImageCommand =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        image.command = vec!["/bin/sh".into(), "-c".into(), "touch restarted".into()];
        fs::write(&path, serde_json::to_vec(&image).unwrap()).unwrap();
        assert!(process
            .start_once_from(&path, identity, &Default::default())
            .is_err());
        assert!(!temp.path().join("restarted").exists());
        assert_eq!(process.status().unwrap(), ImageStatus::Failed);
    }

    #[tokio::test]
    async fn image_command_refuses_a_directory_the_command_cannot_enter() {
        let temp = tempfile::tempdir().expect("private directory");
        let image = SandboxImageCommand {
            user: None,
            command: vec!["/bin/true".to_string()],
            env: Default::default(),
            working_directory: temp.path().display().to_string(),
        };
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o000))
            .expect("make working directory inaccessible");
        let identity = ExecIdentity {
            uid: unsafe {
                if libc::geteuid() == 0 {
                    60001
                } else {
                    libc::geteuid()
                }
            },
            gid: unsafe {
                if libc::getegid() == 0 {
                    60001
                } else {
                    libc::getegid()
                }
            },
        };
        // Keep metadata readable to the supervisor even when it is not root.
        let metadata = tempfile::NamedTempFile::new().expect("readable metadata");
        fs::write(
            metadata.path(),
            serde_json::to_vec(&image).expect("serialize command"),
        )
        .expect("save command outside inaccessible directory");
        let error = start_from(metadata.path(), identity)
            .expect_err("inaccessible working directory must fail before exec");
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))
            .expect("restore directory for cleanup");
        assert!(error.to_string().contains("Permission denied"), "{error}");
    }

    #[tokio::test]
    async fn image_command_uses_the_declared_identity_and_image_environment() {
        let temp = tempfile::tempdir().expect("temporary image metadata");
        let output = temp.path().join("identity");
        let uid = unsafe {
            if libc::geteuid() == 0 {
                60000
            } else {
                libc::geteuid()
            }
        };
        let gid = unsafe {
            if libc::getegid() == 0 {
                60000
            } else {
                libc::getegid()
            }
        };
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o777))
            .expect("command can write its test output");
        let image = SandboxImageCommand {
            user: None,
            command: vec!["/bin/sh".to_string(), "-c".to_string(), "id -u > identity; printf '%s' \"$IMAGE_VAR\" >> identity; test -z \"$ALIEN_SANDBOX_PUBLIC_KEY\"".to_string()],
            env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string()), ("IMAGE_VAR".to_string(), "image-only".to_string())]),
            working_directory: temp.path().display().to_string(),
        };
        let path = temp.path().join("command.json");
        fs::write(
            &path,
            serde_json::to_vec(&image).expect("serialize command"),
        )
        .expect("save command");
        let mut child = start_from(&path, ExecIdentity { uid, gid })
            .expect("start image command")
            .expect("image has a command");
        assert!(child.wait().await.expect("reap image command").success());
        assert_eq!(
            fs::read_to_string(output).expect("image output"),
            format!("{uid}\nimage-only")
        );
    }
    #[test]
    #[ignore = "requires root with Docker default capabilities"]
    fn legacy_egress_refuses_root_image_before_spawn() {
        const CHILD: &str = "ALIEN_TEST_LEGACY_BOOT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            for mode in [
                r#"{"mode":"deny"}"#,
                r#"{"mode":"allow"}"#,
                r#"{"mode":"allowDomains","domains":["example.com"]}"#,
            ] {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "image_command::tests::legacy_egress_refuses_root_image_before_spawn",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env(CHILD, "1")
                    .env("ALIEN_SANDBOX_EGRESS", mode)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{mode}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return;
        }
        assert_eq!(unsafe { libc::geteuid() }, 0);
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for user in ["root", "0", "", "0:2001"] {
                let (temp, path, exec) =
                    process_fixture(vec!["/bin/sh".into(), "-c".into(), "touch started".into()]);
                let mut metadata: SandboxImageCommand =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                metadata.user = Some(user.into());
                fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
                let result = start_from_with_env(&path, exec, &Default::default(), true);
                // Reap even the broken version so the regression never leaves a root process.
                let error = match result {
                    Ok(Some(mut child)) => {
                        child.wait().await.unwrap();
                        panic!("legacy UID-scoped egress started root USER {user:?}");
                    }
                    Ok(None) => panic!("fixture must contain a command"),
                    Err(error) => error,
                };
                assert_eq!(error.code, "AGENT_CONFIG_INVALID");
                assert!(
                    error.to_string().contains("legacy UID-scoped egress"),
                    "{error}"
                );
                assert!(!temp.path().join("started").exists());
                // Released packages continue to run their command as the exec identity.
                let mut legacy = start_from_with_env(&path, exec, &Default::default(), false)
                    .unwrap()
                    .unwrap();
                assert!(legacy.wait().await.unwrap().success());
            }
        });
    }
}
