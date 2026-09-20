//! Python extension for Alien's application-facing resource bindings.
//!
//! This crate only translates Python values and exceptions. Provider selection,
//! workload identity, credential refresh, and resource behavior remain in
//! `alien-bindings`.

#![deny(clippy::all)]

mod error;
mod sandbox;
mod storage;
mod worker;

use crate::error::map_alien_error;
use alien_bindings::traits::{MessagePayload, PutCondition, PutOptions};
use alien_bindings::{Bindings, BoundQueue, Container, Key, Kv, Postgres, Vault};
use alien_core::bindings::{parse_binding_from_env, AiBinding};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use sandbox::{
    CommandFrame, CommandStreamHandle, JobResult, ResolvedSandbox, SandboxHandle, SandboxInfo,
};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use storage::{ObjectInfo, SignedRequest, StorageHandle};
use worker::{WorkerHandle, WorkerResponse};

pub(crate) fn future_into_py<F, T>(py: Python<'_>, future: F) -> PyResult<Bound<'_, PyAny>>
where
    F: Future<Output = PyResult<T>> + Send + 'static,
    T: for<'py> IntoPyObject<'py> + Send + 'static,
{
    pyo3_async_runtimes::tokio::future_into_py(py, future)
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct PostgresConnection {
    connection_string: String,
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
    sslmode: String,
    ca_certificates: Vec<String>,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct AiConnection {
    base_url: String,
    api_key: Option<String>,
    provider: Option<String>,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct KvEntry {
    key: String,
    value: Vec<u8>,
    version: String,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct KvPage {
    items: Vec<KvEntry>,
    next_cursor: Option<String>,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
struct QueueMessage {
    payload_type: String,
    payload: String,
    receipt_handle: String,
    attempt: u32,
}

#[pyclass]
struct BindingsHandle {
    inner: Arc<Bindings>,
}

#[pymethods]
impl BindingsHandle {
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: Arc::new(Bindings::from_env().map_err(map_alien_error)?),
        })
    }

    fn storage<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            let storage = bindings.storage(&name).await.map_err(map_alien_error)?;
            Ok(StorageHandle {
                inner: storage,
                binding: name,
            })
        })
    }

    fn key<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(KeyHandle {
                inner: bindings.key(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn kv<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(KvHandle {
                inner: bindings.kv(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn queue<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(QueueHandle {
                inner: bindings.queue(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn vault<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(VaultHandle {
                inner: bindings.vault(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn sandbox<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(SandboxHandle {
                inner: bindings.sandbox(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn postgres<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(PostgresHandle {
                inner: bindings.postgres(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn container<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(ContainerHandle {
                inner: bindings.container(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn worker<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let bindings = self.inner.clone();
        future_into_py(py, async move {
            Ok(WorkerHandle {
                inner: bindings.worker(&name).await.map_err(map_alien_error)?,
            })
        })
    }

    fn ai<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let env = std::env::vars().collect::<HashMap<_, _>>();
        future_into_py(py, async move {
            let binding: AiBinding =
                parse_binding_from_env(&env, &name).map_err(map_alien_error)?;
            match binding {
                AiBinding::External(binding) => {
                    let provider = binding.provider.to_lowercase();
                    let base_url = match provider.as_str() {
                        "openai" => "https://api.openai.com".to_string(),
                        "anthropic" => "https://api.anthropic.com".to_string(),
                        _ => {
                            return Err(PyValueError::new_err(format!(
                                "unsupported external AI provider '{provider}'"
                            )))
                        }
                    };
                    let base_url = std::env::var("ALIEN_AI_LOCAL_BASE_URL")
                        .unwrap_or(base_url)
                        .trim_end_matches('/')
                        .to_string();
                    let api_key = binding
                        .api_key
                        .into_value(&name, "apiKey")
                        .map_err(map_alien_error)?;
                    Ok(AiHandle {
                        connection: AiConnection {
                            base_url,
                            api_key: Some(api_key),
                            provider: Some(provider),
                        },
                        _gateway: None,
                    })
                }
                _ => {
                    let gateway = Arc::new(
                        alien_ai_gateway::start_gateway(
                            alien_ai_gateway::bindings_from_env().map_err(map_alien_error)?,
                        )
                        .await
                        .map_err(map_alien_error)?,
                    );
                    let segment = name.to_lowercase().replace('_', "-");
                    Ok(AiHandle {
                        connection: AiConnection {
                            base_url: format!("{}/{segment}", gateway.url),
                            api_key: None,
                            provider: None,
                        },
                        _gateway: Some(gateway),
                    })
                }
            }
        })
    }
}

#[pyclass]
struct AiHandle {
    connection: AiConnection,
    // Keep the in-process gateway alive for the lifetime of the Python resource.
    _gateway: Option<Arc<alien_ai_gateway::GatewayHandle>>,
}

#[pymethods]
impl AiHandle {
    fn connection(&self) -> AiConnection {
        self.connection.clone()
    }
}

#[pyclass]
struct KeyHandle {
    inner: Arc<dyn Key>,
}

#[pymethods]
impl KeyHandle {
    fn encrypt<'py>(
        &self,
        py: Python<'py>,
        plaintext: Vec<u8>,
        context: Option<HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let context: Option<BTreeMap<_, _>> = context.map(|value| value.into_iter().collect());
        future_into_py(py, async move {
            inner
                .encrypt(&plaintext, context.as_ref())
                .await
                .map_err(map_alien_error)
        })
    }

    fn decrypt<'py>(
        &self,
        py: Python<'py>,
        ciphertext: Vec<u8>,
        context: Option<HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let context: Option<BTreeMap<_, _>> = context.map(|value| value.into_iter().collect());
        future_into_py(py, async move {
            inner
                .decrypt(&ciphertext, context.as_ref())
                .await
                .map_err(map_alien_error)
        })
    }
}

#[pyclass]
struct KvHandle {
    inner: Arc<dyn Kv>,
}

fn kv_options(
    ttl_seconds: Option<u64>,
    condition: Option<String>,
    version: Option<String>,
) -> PyResult<Option<PutOptions>> {
    let condition = match condition.as_deref() {
        None => PutCondition::None,
        Some("absent") => PutCondition::Absent,
        Some("version") => PutCondition::Version(
            version
                .ok_or_else(|| PyValueError::new_err("a version condition requires a version"))?,
        ),
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "unsupported KV put condition '{other}'"
            )))
        }
    };
    if ttl_seconds.is_none() && matches!(condition, PutCondition::None) {
        return Ok(None);
    }
    Ok(Some(PutOptions {
        ttl: ttl_seconds.map(Duration::from_secs),
        condition,
    }))
}

fn kv_entry(entry: alien_bindings::KvEntry) -> KvEntry {
    KvEntry {
        key: entry.key,
        value: entry.value,
        version: entry.version,
    }
}

#[pymethods]
impl KvHandle {
    fn get<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            Ok(inner
                .get(&key)
                .await
                .map_err(map_alien_error)?
                .map(kv_entry))
        })
    }

    #[pyo3(signature = (key, value, ttl_seconds=None, condition=None, version=None))]
    fn put<'py>(
        &self,
        py: Python<'py>,
        key: String,
        value: Vec<u8>,
        ttl_seconds: Option<u64>,
        condition: Option<String>,
        version: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let options = kv_options(ttl_seconds, condition, version)?;
        future_into_py(py, async move {
            inner
                .put(&key, value, options)
                .await
                .map_err(map_alien_error)
        })
    }

    #[pyo3(signature = (key, if_version=None))]
    fn delete<'py>(
        &self,
        py: Python<'py>,
        key: String,
        if_version: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .delete(&key, if_version.as_deref())
                .await
                .map_err(map_alien_error)
        })
    }

    fn exists<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.exists(&key).await.map_err(map_alien_error)
        })
    }

    #[pyo3(signature = (prefix, limit=None, cursor=None))]
    fn scan<'py>(
        &self,
        py: Python<'py>,
        prefix: String,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let page = inner
                .scan_prefix(&prefix, limit, cursor)
                .await
                .map_err(map_alien_error)?;
            Ok(KvPage {
                items: page.items.into_iter().map(kv_entry).collect(),
                next_cursor: page.next_cursor,
            })
        })
    }
}

#[pyclass]
struct QueueHandle {
    inner: BoundQueue,
}

fn queue_message(message: alien_bindings::traits::QueueMessage) -> PyResult<QueueMessage> {
    let (payload_type, payload) = match message.payload {
        MessagePayload::Json(value) => (
            "json".to_string(),
            serde_json::to_string(&value)
                .map_err(|error| PyValueError::new_err(error.to_string()))?,
        ),
        MessagePayload::Text(value) => ("text".to_string(), value),
    };
    Ok(QueueMessage {
        payload_type,
        payload,
        receipt_handle: message.receipt_handle,
        attempt: message.attempt,
    })
}

#[pymethods]
impl QueueHandle {
    fn send_json<'py>(&self, py: Python<'py>, value: String) -> PyResult<Bound<'py, PyAny>> {
        let value = serde_json::from_str(&value)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .send(MessagePayload::Json(value))
                .await
                .map_err(map_alien_error)
        })
    }
    fn send_text<'py>(&self, py: Python<'py>, value: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .send(MessagePayload::Text(value))
                .await
                .map_err(map_alien_error)
        })
    }
    #[pyo3(signature = (max_messages=1))]
    fn receive<'py>(&self, py: Python<'py>, max_messages: usize) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .receive(max_messages)
                .await
                .map_err(map_alien_error)?
                .into_iter()
                .map(queue_message)
                .collect::<PyResult<Vec<_>>>()
        })
    }
    fn ack<'py>(&self, py: Python<'py>, receipt_handle: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.ack(&receipt_handle).await.map_err(map_alien_error)
        })
    }
    fn nack<'py>(&self, py: Python<'py>, receipt_handle: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.nack(&receipt_handle).await.map_err(map_alien_error)
        })
    }
    fn purge<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(
            py,
            async move { inner.purge().await.map_err(map_alien_error) },
        )
    }
}

#[pyclass]
struct VaultHandle {
    inner: Arc<dyn Vault>,
}

#[pymethods]
impl VaultHandle {
    fn get<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.get_secret(&name).await.map_err(map_alien_error)
        })
    }
    fn put<'py>(
        &self,
        py: Python<'py>,
        name: String,
        value: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner
                .set_secret(&name, &value)
                .await
                .map_err(map_alien_error)
        })
    }
    fn delete<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.delete_secret(&name).await.map_err(map_alien_error)
        })
    }
    fn list<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.list_secrets().await.map_err(map_alien_error)
        })
    }
}

#[pyclass]
struct PostgresHandle {
    inner: Arc<dyn Postgres>,
}

#[pymethods]
impl PostgresHandle {
    fn connection(&self) -> PostgresConnection {
        let value = self.inner.connection_params();
        PostgresConnection {
            connection_string: value.connection_string(),
            host: value.host.clone(),
            port: value.port,
            database: value.database.clone(),
            username: value.username.clone(),
            password: value.password.clone(),
            sslmode: value.sslmode().as_str().to_string(),
            ca_certificates: value.ca_certificates().to_vec(),
        }
    }
}

#[pyclass]
struct ContainerHandle {
    inner: Arc<dyn Container>,
}

#[pymethods]
impl ContainerHandle {
    fn internal_url(&self) -> String {
        self.inner.get_internal_url().to_string()
    }
    fn public_url(&self) -> Option<String> {
        self.inner.get_public_url().map(str::to_string)
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add_class::<BindingsHandle>()?;
    module.add_class::<StorageHandle>()?;
    module.add_class::<KeyHandle>()?;
    module.add_class::<KvHandle>()?;
    module.add_class::<QueueHandle>()?;
    module.add_class::<VaultHandle>()?;
    module.add_class::<SandboxHandle>()?;
    module.add_class::<CommandStreamHandle>()?;
    module.add_class::<PostgresHandle>()?;
    module.add_class::<ContainerHandle>()?;
    module.add_class::<AiHandle>()?;
    module.add_class::<AiConnection>()?;
    module.add_class::<PostgresConnection>()?;
    module.add_class::<KvEntry>()?;
    module.add_class::<KvPage>()?;
    module.add_class::<QueueMessage>()?;
    module.add_class::<ObjectInfo>()?;
    module.add_class::<SignedRequest>()?;
    module.add_class::<WorkerHandle>()?;
    module.add_class::<WorkerResponse>()?;
    module.add_class::<SandboxInfo>()?;
    module.add_class::<ResolvedSandbox>()?;
    module.add_class::<CommandFrame>()?;
    module.add_class::<JobResult>()?;
    Ok(())
}
