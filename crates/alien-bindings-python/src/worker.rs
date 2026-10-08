use crate::error::map_alien_error;
use crate::future_into_py;
use alien_bindings::traits::{Worker, WorkerInvokeRequest};
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct WorkerResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

#[pyclass]
pub(crate) struct WorkerHandle {
    pub(crate) inner: Arc<dyn Worker>,
}

#[pymethods]
impl WorkerHandle {
    #[pyo3(signature = (method, path, headers, body, timeout_ms=None))]
    fn invoke<'py>(
        &self,
        py: Python<'py>,
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
        timeout_ms: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let response = inner
                .invoke(WorkerInvokeRequest {
                    target_worker: String::new(),
                    method,
                    path,
                    headers,
                    body,
                    timeout: timeout_ms.map(Duration::from_millis),
                })
                .await
                .map_err(map_alien_error)?;
            Ok(WorkerResponse {
                status: response.status,
                headers: response.headers,
                body: response.body,
            })
        })
    }

    fn public_url<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            inner.get_worker_url().await.map_err(map_alien_error)
        })
    }
}
