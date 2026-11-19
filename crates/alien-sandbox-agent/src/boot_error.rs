//! Why the image did not start, when the image declares permissions.
//!
//! Such an image starts from the provider's run hook, and a failed run hook ends the sandbox: the
//! reason would be lost with it. Instead the agent stays up without starting the image, writes the
//! reason to a world-readable file and answers every command with it as an error.

use std::path::Path;
use std::sync::OnceLock;

use alien_error::AlienError;

use crate::error::ErrorData;

/// The root-owned, world-readable record of the reason.
pub const PATH: &str = "/run/alien/boot-error";

static ENABLED: OnceLock<()> = OnceLock::new();
static REASON: OnceLock<String> = OnceLock::new();

/// Turns the record on: from now on a failure to start the image keeps the agent up. Called only
/// when the image declares permissions; without them nothing here changes the agent.
pub fn enable() {
    let _ = ENABLED.set(());
}

/// Whether a failure to start the image is recorded instead of failing the caller.
pub fn enabled() -> bool {
    ENABLED.get().is_some()
}

/// Records the first reason: logged, written to [`PATH`] and returned by [`refuse`] from then on.
pub fn record(reason: String) {
    tracing::error!(%reason, "the image will not start");
    eprintln!("alien-sandbox-agent: the image will not start: {reason}");
    console(&reason);
    if REASON.set(reason).is_err() {
        return;
    }
    let reason = REASON.get().expect("just set");
    if let Err(error) = write(Path::new(PATH), reason) {
        tracing::error!(%error, path = PATH, "cannot write the boot error");
    }
}

/// The kernel log, which the VM's console shows even where the agent's own output is not kept.
/// Best effort: one line at error level, cut to what one record holds.
fn console(reason: &str) {
    let line: String = format!("<3>alien-sandbox-agent: the image will not start: {reason}")
        .chars()
        .filter(|character| *character != '\n')
        .take(900)
        .collect();
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .and_then(|mut kmsg| std::io::Write::write_all(&mut kmsg, format!("{line}\n").as_bytes()));
}

pub(crate) fn write(path: &Path, reason: &str) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    if let Some(directory) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(directory)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    std::io::Write::write_all(&mut file, format!("{reason}\n").as_bytes())
}

/// The recorded reason, if the image could not start.
pub fn reason() -> Option<&'static str> {
    REASON.get().map(String::as_str)
}

/// Fails with the recorded reason; commands refuse to run in a sandbox whose image never started.
pub fn refuse() -> Result<(), AlienError<ErrorData>> {
    reason().map_or(Ok(()), |reason| Err(refusal(reason)))
}

fn refusal(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationFailed {
        operation: "start the image".into(),
        reason: format!("the image did not start ({PATH}): {reason}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reason_is_written_world_readable_and_refuses_commands() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("run/alien/boot-error");
        let reason = "cannot seal: step usermodehelper bset: EPERM";
        write(&path, reason).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{reason}\n")
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let error = refusal(reason).to_string();
        assert!(error.contains(reason), "{error}");
        assert!(error.contains(PATH), "{error}");
    }
}
