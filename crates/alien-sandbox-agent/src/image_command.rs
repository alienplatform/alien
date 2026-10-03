//! Runs the image's saved OCI command under the same identity boundary as every exec request.

use crate::{
    error::{ErrorData, Result},
    exec::ExecIdentity,
};
use alien_core::sandbox_image::{SandboxImageCommand, IMAGE_COMMAND_PATH};
use alien_error::{Context, IntoAlienError};
use std::{fs, os::unix::process::CommandExt, process::Stdio};

pub fn start(identity: ExecIdentity) -> Result<Option<tokio::process::Child>> {
    let failed = || ErrorData::OperationFailed {
        operation: "start image command".to_string(),
        reason: "the bundle must preserve the image's OCI command in root-owned metadata"
            .to_string(),
    };
    let bytes = fs::read(IMAGE_COMMAND_PATH)
        .into_alien_error()
        .context(failed())?;
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
