//! The image's declared `onPause` and `onResume` steps.
//!
//! A step runs inside the image with the image's own grant: in the image cgroup (through the
//! mount broker), as root with the bounded root capabilities and the image filters, never with
//! more. Its environment is the image command's own, without agent settings. It runs in a cgroup
//! of its own below the image's; at the deadline every process in it is killed (a process that
//! left the step's process group too) and the step fails. Exit 0 within the deadline is success. Best effort: an image must still survive a stop with no warning.

use std::{os::unix::process::CommandExt, process::Stdio, time::Duration};

use crate::exec::ExecIdentity;

/// Variables never handed to a step: the agent's own settings.
fn kept(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with("ALIEN_SANDBOX_IMAGE_PERMISSIONS")
        && key != "ALIEN_SANDBOX_EGRESS_TOKEN"
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.starts_with(|c: char| c.is_ascii_digit())
}

/// The image command's environment, filtered: `KEY=VALUE` entries with a plain name.
pub fn filtered_environment(environ: &[u8]) -> Vec<(String, String)> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (key, value) = entry.split_once('=')?;
            kept(key).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// Runs `command` as the image's step `name`.
pub async fn run(name: &str, command: &[String], deadline: Duration) -> Result<(), String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| format!("{name} has no program"))?;
    let leader = crate::mount_broker::image_leader()
        .ok_or_else(|| format!("{name}: the image command is not running"))?;
    // The recorded PID may have exited and been reused: it must still be in the image cgroup.
    let in_image = std::fs::read_to_string(format!("/proc/{leader}/cgroup"))
        .map(|cgroup| {
            cgroup.lines().any(|line| {
                line.strip_prefix("0::") == Some(crate::mount_broker::IMAGE_CGROUP_NAME)
            })
        })
        .unwrap_or(false);
    if !in_image {
        return Err(format!("{name}: the image command is not running"));
    }
    let environ = std::fs::read(format!("/proc/{leader}/environ"))
        .map_err(|error| format!("{name}: cannot read the image's environment: {error}"))?;
    let mut process = std::process::Command::new(program);
    process
        .args(args)
        .env_clear()
        .envs(filtered_environment(&environ))
        .current_dir("/")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // SAFETY: both calls are async-signal-safe and allocate nothing.
    unsafe {
        process.pre_exec(|| {
            crate::mount_broker::join_steps_cgroup()?;
            crate::privilege::drop_image_to(ExecIdentity { uid: 0, gid: 0 })
        });
    }
    let mut process = tokio::process::Command::from(process);
    process.kill_on_drop(true);
    let (mut child, _registration) = crate::children::spawn(&mut process)
        .map_err(|error| format!("{name}: cannot start {program:?}: {error}"))?;
    let group = child.id().map(|pid| pid as i32);
    match tokio::time::timeout(deadline, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("{name} exited with {status}")),
        Ok(Err(error)) => Err(format!("{name}: {error}")),
        Err(_) => {
            if let Some(group) = group {
                // SAFETY: signalling the step's own process group.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            // Whatever left the group is still in the steps cgroup.
            let killed = tokio::task::spawn_blocking(crate::mount_broker::kill_steps).await;
            let _ = child.wait().await;
            if !matches!(killed, Ok(Ok(()))) {
                return Err(format!(
                    "{name} did not finish within {}s; its processes could not all be killed",
                    deadline.as_secs()
                ));
            }
            Err(format!(
                "{name} did not finish within {}s; its processes were killed",
                deadline.as_secs()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_settings_are_filtered_out() {
        let environ = b"PATH=/usr/bin\0ALIEN_SANDBOX_EGRESS_TOKEN=secret\0ALIEN_SANDBOX_IMAGE_PERMISSIONS={}\0HOME=/root\0=x\0BAD KEY=1\0noequals\0";
        assert_eq!(
            filtered_environment(environ),
            vec![
                ("PATH".to_string(), "/usr/bin".to_string()),
                ("HOME".to_string(), "/root".to_string())
            ]
        );
    }
}
