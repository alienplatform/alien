//! Runs the image's saved OCI command under the same identity boundary as every exec request.

use crate::{
    error::{ErrorData, Result},
    exec::ExecIdentity,
};
use alien_core::sandbox_image::{SandboxImageCommand, IMAGE_COMMAND_PATH};
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
}

/// Image process lifecycle, independent of session/network readiness.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ImageStatus {
    /// Released images without a saved startup command retain their old behavior.
    NotConfigured,
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
    /// Compatibility for released images that never saved or started an OCI command.
    pub fn not_configured() -> Self {
        Self {
            status: Arc::new(Mutex::new(ImageStatus::NotConfigured)),
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
            ImageStatus::Pending => (),
            ImageStatus::Failed => return Err(monitor_failed()),
            _ => return Ok(()),
        }
        // Hold the lock until spawn has completed its pre-exec work. A simultaneous
        // session operation cannot observe "started" before the actual exec succeeds.
        let child = match start_from_with_env(path, identity, env) {
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

fn monitor_failed() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationFailed {
        operation: "image process lifecycle".into(),
        reason: "image startup or observation failed; this boot will not retry it".into(),
    })
}

#[cfg(test)]
fn start_from(path: &Path, identity: ExecIdentity) -> Result<Option<tokio::process::Child>> {
    start_from_with_env(path, identity, &Default::default())
}

fn start_from_with_env(
    path: &Path,
    identity: ExecIdentity,
    env: &BTreeMap<String, String>,
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
            crate::privilege::drop_to(identity)?;
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
    async fn image_starts_once_without_waiting_for_readiness_and_retains_its_exit() {
        let (temp, path, identity) = process_fixture(vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf started >> starts; while ! test -e release; do sleep 0.02; done; exit 7".into(),
        ]);
        let process = ImageProcess::default();
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
}
