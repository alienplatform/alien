//! Jobs owned by the local manager, with bounded replay and a private cancellation pipe.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_bindings::traits::{CommandOutput, JobError, JobExit, JobPoll, RunCommandRequest};
use alien_error::{AlienError, Context, IntoAlienError};
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

use crate::error::{ErrorData, Result};

const MAX_JOBS: usize = 16;
const OUTPUT_CAP: usize = 4 * 1024 * 1024;
const STOP_GRACE: Duration = Duration::from_secs(5);

// The control reader inherits fd 3 explicitly: shells otherwise give background commands
// /dev/null on stdin. The workload only gets /dev/null, never the manager's control pipe.
// setsid makes the shell the group leader, so cancellation reaches its descendants too.
const SUPERVISE: &str = r#"
exec 3<&0
(read -r signal <&3; kill -9 -$$) &
control=$!
"$@" < /dev/null 3<&- &
child=$!
wait "$child"
status=$?
kill "$control" 2>/dev/null
wait "$control" 2>/dev/null
exit "$status"
"#;

#[derive(Debug)]
struct Job {
    snapshot: JobPoll,
    cancel: Option<oneshot::Sender<()>>,
    delivered: bool,
    bytes: usize,
    truncated: bool,
}

type ContainerJobs = HashMap<String, Arc<Mutex<Job>>>;

/// Results belong to a container, not a client connection. Reconnecting clients can poll them.
#[derive(Debug, Default)]
pub(crate) struct SandboxJobs {
    sessions: Mutex<HashMap<String, ContainerJobs>>,
}

impl SandboxJobs {
    pub(crate) fn start(
        self: &Arc<Self>,
        docker: Docker,
        container: String,
        request: RunCommandRequest,
    ) -> Result<String> {
        let cwd = working_directory(request.cwd.as_deref())?;
        if request.timeout < Duration::from_millis(1)
            || request.timeout > Duration::from_secs(24 * 60 * 60)
            || request.command.is_empty()
            || request.command.contains(['=', '\0'])
            || request.args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(failed(
                &container,
                "a job requires a valid command and a timeout between 1ms and 24 hours",
            ));
        }
        for (name, value) in &request.env {
            if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
                return Err(failed(&container, "invalid environment variable"));
            }
        }
        let (cancel, cancelled) = oneshot::channel();
        let job = Arc::new(Mutex::new(Job {
            snapshot: JobPoll {
                running: true,
                frames: Vec::new(),
                exit: None,
                error: None,
            },
            cancel: Some(cancel),
            delivered: false,
            bytes: 0,
            truncated: false,
        }));
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut sessions = self.sessions.lock().expect("jobs lock");
            let jobs = sessions.entry(container.clone()).or_default();
            if jobs.len() >= MAX_JOBS {
                let evict = jobs.iter().find_map(|(id, job)| {
                    let job = job.lock().expect("job lock");
                    (!job.snapshot.running && job.delivered).then(|| id.clone())
                });
                match evict {
                    Some(id) => {
                        jobs.remove(&id);
                    }
                    None => {
                        return Err(failed(
                            &container,
                            "all job slots are occupied; poll completed jobs before starting more",
                        ))
                    }
                }
            }
            jobs.insert(id.clone(), Arc::clone(&job));
        }
        let jobs = Arc::clone(self);
        tokio::spawn(async move {
            // Also bound a Docker create/start/inspect call that never acknowledges its result.
            let deadline = request.timeout + STOP_GRACE;
            let result = match tokio::time::timeout(deadline, run(&docker, &container, request, cwd, cancelled, &job, &jobs)).await {
                Ok(result) => result,
                Err(_) => match remove_unresponsive(&docker, &container, &jobs).await {
                    Ok(()) => Err(failed(&container, "Docker did not confirm the job outcome before its deadline; sandbox removed")),
                    Err(error) => Err(error),
                },
            };
            let mut state = job.lock().expect("job lock");
            state.snapshot.running = false;
            state.cancel = None;
            match result {
                Ok(Some(code)) => {
                    state.snapshot.exit = Some(JobExit {
                        code,
                        truncated: state.truncated,
                    })
                }
                Ok(None) => {}
                Err(error) => {
                    state.snapshot.error = Some(JobError {
                        code: "outcomeUnknown".to_string(),
                        message: error.to_string(),
                    })
                }
            }
        });
        Ok(id)
    }

    pub(crate) fn poll(&self, container: &str, id: &str, since: Option<u64>) -> Result<JobPoll> {
        let job = self.job(container, id)?;
        let mut job = job.lock().expect("job lock");
        let mut snapshot = job.snapshot.clone();
        snapshot.frames.retain(|frame| match frame {
            CommandOutput::Stdout { seq, .. } | CommandOutput::Stderr { seq, .. } => {
                since.is_none_or(|since| *seq > since)
            }
            CommandOutput::Exit { .. } => false,
        });
        job.delivered |= !snapshot.running;
        Ok(snapshot)
    }

    pub(crate) fn cancel(&self, container: &str, id: &str) -> Result<()> {
        let job = self.job(container, id)?;
        if let Some(cancel) = job.lock().expect("job lock").cancel.take() {
            let _ = cancel.send(());
        }
        Ok(())
    }

    pub(crate) fn forget(&self, container: &str) {
        self.sessions.lock().expect("jobs lock").remove(container);
    }

    fn job(&self, container: &str, id: &str) -> Result<Arc<Mutex<Job>>> {
        self.sessions
            .lock()
            .expect("jobs lock")
            .get(container)
            .and_then(|jobs| jobs.get(id))
            .cloned()
            .ok_or_else(|| failed(container, &format!("job '{id}' was not found")))
    }
}

fn working_directory(cwd: Option<&str>) -> Result<String> {
    let path = cwd.unwrap_or("/");
    let relative = path.trim_matches('/');
    if path.is_empty()
        || (!relative.is_empty()
            && relative
                .split('/')
                .any(|part| part.is_empty() || part == ".."))
    {
        return Err(failed(
            path,
            "working directory must be under the sandbox root",
        ));
    }
    Ok(format!("/sandbox/{relative}"))
}

fn failed(container: &str, operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxSessionFailed {
        session_id: container.to_string(),
        operation: operation.to_string(),
    })
}

async fn run(
    docker: &Docker,
    container: &str,
    request: RunCommandRequest,
    cwd: String,
    cancelled: oneshot::Receiver<()>,
    job: &Arc<Mutex<Job>>,
    jobs: &SandboxJobs,
) -> Result<Option<i32>> {
    let mut command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "exec 3<&0; setsid \"$@\" <&3 & child=$!; wait \"$child\"".to_string(),
        "sh".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        SUPERVISE.to_string(),
        "sh".to_string(),
    ];
    // Environment applies to the workload only, so it cannot replace the supervisor's tools.
    command.extend(["env".to_string(), "--".to_string()]);
    command.extend(
        request
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    command.extend(request.argv());
    let exec = docker
        .create_exec(
            container,
            CreateExecOptions {
                cmd: Some(command),
                working_dir: Some(cwd),
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
        .into_alien_error()
        .context(ErrorData::SandboxSessionFailed {
            session_id: container.to_string(),
            operation: "create job exec".to_string(),
        })?;
    let started = docker
        .start_exec(&exec.id, None)
        .await
        .into_alien_error()
        .context(ErrorData::SandboxSessionFailed {
            session_id: container.to_string(),
            operation: "start job exec".to_string(),
        })?;
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = started
    else {
        return Err(failed(container, "job output was not attached"));
    };
    let collect = async {
        while let Some(frame) = output.next().await {
            let frame = frame
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: container.to_string(),
                    operation: "read job output".to_string(),
                })?;
            let (stdout, data) = match frame {
                LogOutput::StdOut { message } => (true, message),
                LogOutput::StdErr { message } => (false, message),
                _ => continue,
            };
            let mut job = job.lock().expect("job lock");
            let kept = if job.snapshot.frames.len() < 1024 {
                data.len().min(OUTPUT_CAP.saturating_sub(job.bytes))
            } else {
                0
            };
            job.truncated |= kept < data.len();
            job.bytes += kept;
            if kept > 0 {
                let seq = job.snapshot.frames.len() as u64;
                let data = data[..kept].to_vec();
                job.snapshot.frames.push(if stdout {
                    CommandOutput::Stdout { seq, data }
                } else {
                    CommandOutput::Stderr { seq, data }
                });
            }
        }
        Ok::<_, AlienError<ErrorData>>(())
    };
    tokio::pin!(collect);
    let reason = tokio::select! {
        result = &mut collect => {
            if let Err(error) = result {
                // Losing the output transport must not also lose the only external deadline.
                remove_unresponsive(docker, container, jobs).await?;
                return Err(error);
            }
            None
        }
        _ = tokio::time::sleep(request.timeout) => Some("timeoutExceeded"),
        _ = cancelled => Some("cancelled"),
    };
    if let Some(reason) = reason {
        // A completed pipe proves the process group stopped. If the guest killed its control
        // reader, the host's backstop removes the container before claiming it was stopped.
        let stopped = async {
            input
                .write_all(b"stop\n")
                .await
                .into_alien_error()
                .context(ErrorData::SandboxSessionFailed {
                    session_id: container.to_string(),
                    operation: "cancel job".to_string(),
                })?;
            collect.await
        };
        if !matches!(tokio::time::timeout(STOP_GRACE, stopped).await, Ok(Ok(()))) {
            remove_unresponsive(docker, container, jobs).await?;
        }
        job.lock().expect("job lock").snapshot.error = Some(JobError {
            code: reason.to_string(),
            message: format!("job stopped: {reason}"),
        });
        return Ok(None);
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .into_alien_error()
        .context(ErrorData::SandboxSessionFailed {
            session_id: container.to_string(),
            operation: "read job exit status".to_string(),
        })?;
    let code = status
        .exit_code
        .ok_or_else(|| failed(container, "job ended without an exit status"))?;
    Ok(Some(code as i32))
}

/// The host owns this backstop even if a guest has killed its shell supervisor.
async fn remove_unresponsive(docker: &Docker, container: &str, jobs: &SandboxJobs) -> Result<()> {
    match docker
        .remove_container(
            container,
            Some(bollard::container::RemoveContainerOptions {
                force: true,
                v: true,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(())
        | Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {
            jobs.forget(container);
            Ok(())
        }
        Err(error) => Err(error)
            .into_alien_error()
            .context(ErrorData::SandboxSessionFailed {
                session_id: container.to_string(),
                operation: "remove unresponsive job sandbox".to_string(),
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use axum::{http::StatusCode, Json, Router};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn a_lost_docker_start_acknowledgement_keeps_the_host_deadline() {
        let removed = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let observed_remove = removed.clone();
        let observed_start = started.clone();
        let server =
            Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri| {
                let removed = observed_remove.clone();
                let started = observed_start.clone();
                async move {
                    if method == axum::http::Method::DELETE {
                        removed.store(true, Ordering::SeqCst);
                        return StatusCode::NO_CONTENT.into_response();
                    }
                    if uri.path().ends_with("/start") {
                        started.store(true, Ordering::SeqCst);
                        // The exec may have started, but the manager never receives its stream.
                        return std::future::pending().await;
                    }
                    Json(serde_json::json!({"Id": "exec"})).into_response()
                }
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let address = listener.local_addr().expect("address");
        let serving = tokio::spawn(async move {
            axum::serve(listener, server).await.expect("serve");
        });
        let docker = Docker::connect_with_http(
            &format!("http://{address}"),
            120,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("Docker client");
        let jobs = Arc::new(SandboxJobs::default());
        let id = jobs
            .start(
                docker,
                "container".to_string(),
                RunCommandRequest {
                    command: "true".to_string(),
                    args: vec![],
                    cwd: None,
                    env: Default::default(),
                    timeout: Duration::from_millis(100),
                },
            )
            .expect("job accepted");
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if jobs.poll("container", &id, None).is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("host deadline remains active");
        serving.abort();
        assert!(started.load(Ordering::SeqCst), "the probe reached start");
        assert!(
            removed.load(Ordering::SeqCst),
            "unacknowledged work is contained by deleting its sandbox"
        );
        assert!(
            jobs.sessions.lock().expect("jobs lock").is_empty(),
            "removed sandboxes must release all retained job output"
        );
    }
}
