use std::path::Path;

use super::*;

pub(super) fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) async fn run_command(mut cmd: Command, label: &str) -> anyhow::Result<()> {
    let output = cmd
        .output()
        .await
        .with_context(|| format!("failed to start {label}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub(super) async fn command_output(
    mut cmd: Command,
    label: &str,
) -> anyhow::Result<std::process::Output> {
    let output = cmd
        .output()
        .await
        .with_context(|| format!("failed to start {label}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

pub(super) fn apply_env(cmd: &mut Command, env: &[(String, String)]) {
    for (key, value) in env {
        cmd.env(key, value);
    }
}
