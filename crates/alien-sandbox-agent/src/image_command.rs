//! Runs the image's saved OCI command under the same identity boundary as every exec request.

use crate::{
    error::{ErrorData, Result},
    exec::ExecIdentity,
};
use alien_core::sandbox_image::{SandboxImageCommand, IMAGE_COMMAND_PATH};
use alien_error::{Context, IntoAlienError};
use std::{fs, os::unix::process::CommandExt, process::Stdio};

pub fn start(identity: ExecIdentity) -> Result<Option<tokio::process::Child>> {
    start_from(std::path::Path::new(IMAGE_COMMAND_PATH), identity)
}

fn start_from(
    path: &std::path::Path,
    identity: ExecIdentity,
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
    let mut command = std::process::Command::new(program);
    command
        .args(&image.command[1..])
        .env_clear()
        .envs(image.env)
        .current_dir(image.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    unsafe {
        command.pre_exec(move || crate::privilege::drop_to(identity));
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
            command: vec!["/bin/sh".to_string(), "-c".to_string(), "id -u > identity; printf '%s' \"$IMAGE_VAR\" >> identity; test -z \"$ALIEN_SANDBOX_PUBLIC_KEY\"".to_string()],
            env: std::collections::BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string()), ("IMAGE_VAR".to_string(), "image-only".to_string())]),
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
