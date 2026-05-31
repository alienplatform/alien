use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::future::Future;
use std::process::{ExitStatus, Output};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutputLine {
    pub(crate) stream: CommandOutputStream,
    pub(crate) line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CapturedCommandOutput {
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl CapturedCommandOutput {
    pub(crate) fn from_output(output: &Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(ToString::to_string)
                .collect(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(ToString::to_string)
                .collect(),
        }
    }

    pub(crate) fn push(&mut self, line: CommandOutputLine) {
        match line.stream {
            CommandOutputStream::Stdout => self.stdout.push(line.line),
            CommandOutputStream::Stderr => self.stderr.push(line.line),
        }
    }

    pub(crate) fn display(&self) -> String {
        let stdout = self.stdout.join("\n");
        let stderr = self.stderr.join("\n");

        match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
            (true, true) => String::new(),
            (false, true) => stdout,
            (true, false) => stderr,
            (false, false) => format!("stdout:\n{stdout}\n\nstderr:\n{stderr}"),
        }
    }
}

pub(crate) async fn wait_with_captured_output<F, Fut>(
    child: &mut Child,
    resource_name: &str,
    read_reason: &str,
    wait_reason: &str,
    mut on_line: F,
) -> Result<(ExitStatus, CapturedCommandOutput)>
where
    F: FnMut(CommandOutputLine) -> Fut,
    Fut: Future<Output = ()>,
{
    let (tx, mut rx) = mpsc::unbounded_channel();

    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, CommandOutputStream::Stdout, tx.clone());
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, CommandOutputStream::Stderr, tx.clone());
    }

    drop(tx);

    let mut captured = CapturedCommandOutput::default();
    while let Some(line_result) = rx.recv().await {
        let line = line_result
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: resource_name.to_string(),
                reason: read_reason.to_string(),
                build_output: Some(captured.display()),
            })?;

        captured.push(line.clone());
        on_line(line).await;
    }

    let status = child
        .wait()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: wait_reason.to_string(),
            build_output: Some(captured.display()),
        })?;

    Ok((status, captured))
}

fn spawn_reader<R>(
    stream: R,
    stream_kind: CommandOutputStream,
    tx: mpsc::UnboundedSender<std::io::Result<CommandOutputLine>>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    if tx
                        .send(Ok(CommandOutputLine {
                            stream: stream_kind,
                            line,
                        }))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    break;
                }
            }
        }
    });
}

pub(crate) fn image_build_error_with_output(
    resource_name: impl Into<String>,
    reason: impl Into<String>,
    output: &Output,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ImageBuildFailed {
        resource_name: resource_name.into(),
        reason: reason.into(),
        build_output: Some(CapturedCommandOutput::from_output(output).display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::Command;

    #[test]
    fn display_prefers_raw_stdout_when_only_stdout_is_present() {
        let mut output = CapturedCommandOutput::default();
        output.push(CommandOutputLine {
            stream: CommandOutputStream::Stdout,
            line: "hello".to_string(),
        });

        assert_eq!(output.display(), "hello");
    }

    #[test]
    fn display_prefers_raw_stderr_when_only_stderr_is_present() {
        let mut output = CapturedCommandOutput::default();
        output.push(CommandOutputLine {
            stream: CommandOutputStream::Stderr,
            line: "error".to_string(),
        });

        assert_eq!(output.display(), "error");
    }

    #[test]
    fn display_labels_streams_when_both_are_present() {
        let mut output = CapturedCommandOutput::default();
        output.push(CommandOutputLine {
            stream: CommandOutputStream::Stdout,
            line: "out".to_string(),
        });
        output.push(CommandOutputLine {
            stream: CommandOutputStream::Stderr,
            line: "err".to_string(),
        });

        assert_eq!(output.display(), "stdout:\nout\n\nstderr:\nerr");
    }

    #[test]
    fn display_is_empty_when_no_output_was_captured() {
        assert_eq!(CapturedCommandOutput::default().display(), "");
    }

    #[tokio::test]
    async fn captures_stdout_and_stderr_without_waiting_for_process_exit_first() {
        let mut child = Command::new("sh")
            .args(["-c", "printf 'out\\n'; printf 'err\\n' >&2; exit 7"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn test process");

        let (status, output) = wait_with_captured_output(
            &mut child,
            "test",
            "failed to read test output",
            "failed to wait for test process",
            |_| async {},
        )
        .await
        .expect("capture output");

        assert_eq!(status.code(), Some(7));
        let display = output.display();
        assert!(display.contains("out"));
        assert!(display.contains("err"));
    }
}
