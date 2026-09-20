use crate::error::map_alien_error;
use crate::future_into_py;
use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, JobPoll, RunCommandRequest, Sandbox, SandboxInstance,
    SandboxState,
};
use futures::lock::Mutex;
use futures::stream::BoxStream;
use futures::StreamExt;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct SandboxInfo {
    sandbox_id: String,
    state: String,
    generation: u64,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct ResolvedSandbox {
    sandbox: SandboxInfo,
    created: bool,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct CommandFrame {
    kind: String,
    seq: Option<u64>,
    data: Option<Vec<u8>>,
    exit_code: Option<i32>,
    truncated: Option<bool>,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct JobResult {
    running: bool,
    frames: Vec<CommandFrame>,
    exit_code: Option<i32>,
    truncated: Option<bool>,
    error_code: Option<String>,
    error_message: Option<String>,
}

fn sandbox_info(value: SandboxInstance) -> SandboxInfo {
    SandboxInfo {
        sandbox_id: value.sandbox_id,
        state: match value.state {
            SandboxState::Starting => "starting",
            SandboxState::Running => "running",
            SandboxState::Paused => "paused",
            SandboxState::Terminated => "terminated",
        }
        .to_string(),
        generation: value.generation,
    }
}

fn command_frame(value: CommandOutput) -> CommandFrame {
    match value {
        CommandOutput::Stdout { seq, data } => CommandFrame {
            kind: "stdout".to_string(),
            seq: Some(seq),
            data: Some(data),
            exit_code: None,
            truncated: None,
        },
        CommandOutput::Stderr { seq, data } => CommandFrame {
            kind: "stderr".to_string(),
            seq: Some(seq),
            data: Some(data),
            exit_code: None,
            truncated: None,
        },
        CommandOutput::Exit { code, truncated } => CommandFrame {
            kind: "exit".to_string(),
            seq: None,
            data: None,
            exit_code: Some(code),
            truncated: Some(truncated),
        },
    }
}

fn job_result(value: JobPoll) -> JobResult {
    JobResult {
        running: value.running,
        frames: value.frames.into_iter().map(command_frame).collect(),
        exit_code: value.exit.as_ref().map(|exit| exit.code),
        truncated: value.exit.map(|exit| exit.truncated),
        error_code: value.error.as_ref().map(|error| error.code.clone()),
        error_message: value.error.map(|error| error.message),
    }
}

#[pyclass]
pub(crate) struct CommandStreamHandle {
    frames: Arc<Mutex<Option<BoxStream<'static, alien_bindings::error::Result<CommandOutput>>>>>,
}

#[pymethods]
impl CommandStreamHandle {
    fn next<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let frames = self.frames.clone();
        future_into_py(py, async move {
            let mut frames = frames.lock().await;
            let Some(stream) = frames.as_mut() else {
                return Ok(None);
            };
            match stream.next().await {
                Some(Ok(frame)) => Ok(Some(command_frame(frame))),
                Some(Err(error)) => Err(map_alien_error(error)),
                None => {
                    *frames = None;
                    Ok(None)
                }
            }
        })
    }

    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let frames = self.frames.clone();
        future_into_py(py, async move {
            *frames.lock().await = None;
            Ok(())
        })
    }
}

#[pyclass]
pub(crate) struct SandboxHandle {
    pub(crate) inner: Arc<dyn Sandbox>,
}

fn sandbox_request(
    sandbox_id: Option<String>,
    tenant_key: Option<String>,
    env: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> CreateSandboxRequest {
    CreateSandboxRequest {
        sandbox_id,
        tenant_key,
        env: env
            .map(|value| value.into_iter().collect())
            .unwrap_or_default(),
        timeout_ms,
    }
}

fn command_request(
    command: String,
    args: Vec<String>,
    timeout_ms: u64,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
) -> RunCommandRequest {
    RunCommandRequest {
        command,
        args,
        cwd,
        env: env
            .map(|value| value.into_iter().collect())
            .unwrap_or_default(),
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[pymethods]
impl SandboxHandle {
    fn capabilities(&self) -> Vec<String> {
        let value = self.inner.capabilities();
        [
            (value.files, "files"),
            (value.reconnect, "reconnect"),
            (value.jobs, "jobs"),
            (value.pause_resume, "pauseResume"),
            (value.domain_egress_rules, "domainEgressRules"),
            (value.egress_deny, "egressDeny"),
            (value.enforced_limits, "enforcedLimits"),
            (value.process_limit, "processLimit"),
            (value.sandbox_lifetime, "sandboxLifetime"),
            (value.supervisor_pid_namespace, "supervisorPidNamespace"),
            (value.supervisor_isolation, "supervisorIsolation"),
        ]
        .into_iter()
        .filter(|(supported, _)| *supported)
        .map(|(_, name)| name.to_string())
        .collect()
    }

    #[pyo3(signature = (sandbox_id=None, tenant_key=None, env=None, timeout_ms=None))]
    fn create<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .create(sandbox_request(sandbox_id, tenant_key, env, timeout_ms))
                .await
                .map(sandbox_info)
                .map_err(map_alien_error)
        })
    }

    fn get<'py>(&self, py: Python<'py>, sandbox_id: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .get(&sandbox_id)
                .await
                .map(|value| value.map(sandbox_info))
                .map_err(map_alien_error)
        })
    }

    #[pyo3(signature = (sandbox_id=None, tenant_key=None, env=None, timeout_ms=None))]
    fn get_or_create<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let value = inner
                .get_or_create(sandbox_request(sandbox_id, tenant_key, env, timeout_ms))
                .await
                .map_err(map_alien_error)?;
            Ok(ResolvedSandbox {
                sandbox: sandbox_info(value.sandbox),
                created: value.created,
            })
        })
    }

    fn list<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .list()
                .await
                .map(|values| values.into_iter().map(sandbox_info).collect::<Vec<_>>())
                .map_err(map_alien_error)
        })
    }

    #[pyo3(signature = (sandbox_id, command, args, timeout_ms, cwd=None, env=None))]
    #[allow(clippy::too_many_arguments)] // PyO3 exposes these as named Python arguments.
    fn run_command<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        command: String,
        args: Vec<String>,
        timeout_ms: u64,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let frames = inner
                .run_command(
                    &sandbox_id,
                    command_request(command, args, timeout_ms, cwd, env),
                )
                .await
                .map_err(map_alien_error)?;
            Ok(CommandStreamHandle {
                frames: Arc::new(Mutex::new(Some(frames))),
            })
        })
    }

    #[pyo3(signature = (sandbox_id, command, args, timeout_ms, cwd=None, env=None))]
    #[allow(clippy::too_many_arguments)] // Keep parity with run_command's Python API.
    fn start_job<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        command: String,
        args: Vec<String>,
        timeout_ms: u64,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .start_job(
                    &sandbox_id,
                    command_request(command, args, timeout_ms, cwd, env),
                )
                .await
                .map(|value| value.job_id)
                .map_err(map_alien_error)
        })
    }

    #[pyo3(signature = (sandbox_id, job_id, since_seq=None))]
    fn poll_job<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        job_id: String,
        since_seq: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .poll_job(&sandbox_id, &job_id, since_seq)
                .await
                .map(job_result)
                .map_err(map_alien_error)
        })
    }

    fn cancel_job<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        job_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .cancel_job(&sandbox_id, &job_id)
                .await
                .map_err(map_alien_error)
        })
    }

    fn read_file<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        path: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .read_file(&sandbox_id, &path)
                .await
                .map_err(map_alien_error)
        })
    }

    fn write_file<'py>(
        &self,
        py: Python<'py>,
        sandbox_id: String,
        path: String,
        contents: Vec<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .write_files(&sandbox_id, BTreeMap::from([(path, contents)]))
                .await
                .map_err(map_alien_error)
        })
    }

    fn pause<'py>(&self, py: Python<'py>, sandbox_id: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.pause(&sandbox_id).await.map_err(map_alien_error)
        })
    }

    fn resume<'py>(&self, py: Python<'py>, sandbox_id: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.resume(&sandbox_id).await.map_err(map_alien_error)
        })
    }

    fn terminate<'py>(&self, py: Python<'py>, sandbox_id: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.terminate(&sandbox_id).await.map_err(map_alien_error)
        })
    }
}
