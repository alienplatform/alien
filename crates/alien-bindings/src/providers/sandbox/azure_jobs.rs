//! Jobs supervised by the native shell, with records surviving provider reconnection.
//!
//! Azure supplies no stream API. Poll cursors pack the two byte offsets; frames are reconstructed
//! stdout first, then stderr, rather than claiming wall-clock ordering across separate pipes.
//! The job files and supervisor share the guest's trust boundary, as native exec itself does.

use base64::{engine::general_purpose::STANDARD, Engine as _};

use super::azure::AzureSandbox;
use crate::error::{ErrorData, Result};
use crate::traits::{CommandOutput, JobError, JobExit, JobPoll, JobStart, RunCommandRequest};
use alien_error::AlienError;

const ROOT: &str = "/.alien-jobs";
const START: &str = include_str!("azure_jobs/start.sh");
const POLL: &str = include_str!("azure_jobs/poll.sh");
const CANCEL: &str = include_str!("azure_jobs/cancel.sh");
const CAP: u64 = 4 * 1024 * 1024;

fn invalid(detail: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidInput {
        operation_context: "sandbox.jobs".to_string(),
        details: detail.to_string(),
        field_name: None,
    })
}

fn job_path(id: &str) -> Result<String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| invalid("invalid job id"))?;
    if parsed.to_string() != id {
        return Err(invalid("job id must be a canonical UUID"));
    }
    Ok(format!("{ROOT}/{id}"))
}

fn shell(program: &str, args: &[String]) -> String {
    std::iter::once("sh".to_string())
        .chain(["-c".to_string(), program.to_string(), "sh".to_string()])
        .chain(args.iter().cloned())
        .map(|value| format!("'{}'", value.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

impl AzureSandbox {
    pub(super) async fn start_native_job(
        &self,
        sandbox: &str,
        request: RunCommandRequest,
    ) -> Result<JobStart> {
        super::guard_for(request.timeout)?;
        if request.command.is_empty() || request.command.contains('=') {
            return Err(invalid("a command must name a program without '='"));
        }
        for name in request.env.keys() {
            super::checked_env_name("sandbox.jobStart", name)?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut args = vec![
            ROOT.to_string(),
            id.clone(),
            format!(
                "{}.{:03}",
                request.timeout.as_secs(),
                request.timeout.subsec_millis()
            ),
            request.cwd.unwrap_or_else(|| "/".to_string()),
            "env".to_string(),
            "--".to_string(),
        ];
        args.extend(
            request
                .env
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
        );
        args.push(request.command);
        args.extend(request.args);
        if args.iter().any(|value| value.contains('\0')) {
            return Err(invalid("job arguments cannot contain NUL"));
        }
        self.native_job_exec(sandbox, "sandbox.jobStart", shell(START, &args))
            .await?;
        Ok(JobStart { job_id: id })
    }

    pub(super) async fn poll_native_job(
        &self,
        sandbox: &str,
        id: &str,
        since: Option<u64>,
    ) -> Result<JobPoll> {
        let path = job_path(id)?;
        let cursor = since.unwrap_or(0);
        let mut out = cursor >> 32;
        let mut err = cursor & 0xffff_ffff;
        if out > CAP || err > CAP {
            return Err(invalid("job cursor exceeds the output bounds"));
        }
        let text = self
            .native_job_exec(
                sandbox,
                "sandbox.jobPoll",
                shell(POLL, &[path.clone(), out.to_string(), err.to_string()]),
            )
            .await?;
        let bad = || {
            AlienError::new(ErrorData::SandboxOutcomeUnknown {
                operation: "sandbox.jobPoll".to_string(),
                reason: "the native job record is missing or malformed".to_string(),
            })
        };
        let lines: Vec<_> = text.split_terminator('\n').collect();
        if lines.len() != 5 {
            return Err(bad());
        }
        let stdout = STANDARD.decode(lines[3]).map_err(|_| bad())?;
        let stderr = STANDARD.decode(lines[4]).map_err(|_| bad())?;
        let mut frames = Vec::new();
        if !stdout.is_empty() {
            out += stdout.len() as u64;
            frames.push(CommandOutput::Stdout {
                seq: (out << 32) | err,
                data: stdout,
            });
        }
        if !stderr.is_empty() {
            err += stderr.len() as u64;
            frames.push(CommandOutput::Stderr {
                seq: (out << 32) | err,
                data: stderr,
            });
        }
        // Full pages may have more bytes behind them. Finish only on a short page from both.
        let drained = frames.iter().all(|frame| match frame {
            CommandOutput::Stdout { data, .. } | CommandOutput::Stderr { data, .. } => {
                data.len() < 65536
            }
            CommandOutput::Exit { .. } => false,
        });
        let running = lines[0] == "running" || !drained;
        let mut result = JobPoll {
            running,
            frames,
            exit: None,
            error: None,
        };
        if !running {
            let code = lines[0].parse().map_err(|_| bad())?;
            if lines[1].is_empty() {
                result.exit = Some(JobExit {
                    code,
                    truncated: lines[2] == "true",
                });
            } else {
                result.error = Some(JobError {
                    code: lines[1].to_string(),
                    message: "the job process group was stopped".to_string(),
                });
            }
            self.native_job_exec(
                sandbox,
                "sandbox.jobPoll",
                shell("touch \"$1/delivered\"", &[path]),
            )
            .await?;
        }
        Ok(result)
    }

    pub(super) async fn cancel_native_job(&self, sandbox: &str, id: &str) -> Result<()> {
        self.native_job_exec(
            sandbox,
            "sandbox.jobCancel",
            shell(CANCEL, &[job_path(id)?]),
        )
        .await?;
        // A signal is only a request. Observe completion without consuming retained output.
        self.native_job_exec(
            sandbox,
            "sandbox.jobCancel",
            shell(
                "timeout 10 sh -c 'while [ ! -f \"$1/exit\" ]; do sleep 0.1; done' sh \"$1\"",
                &[job_path(id)?],
            ),
        )
        .await?;
        Ok(())
    }
}
