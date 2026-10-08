use crate::error::{map_alien_error, map_object_store_error};
use crate::future_into_py;
use alien_bindings::error::ErrorData;
use alien_bindings::Storage;
use alien_error::AlienError;
use futures::TryStreamExt;
use object_store::path::Path;
use object_store::{PutMode, PutOptions, PutPayload};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct ObjectInfo {
    location: String,
    size: u64,
    last_modified: String,
    e_tag: Option<String>,
    version: Option<String>,
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct SignedRequest {
    url: String,
    method: String,
    headers: HashMap<String, String>,
}

#[pyclass]
pub(crate) struct StorageHandle {
    pub(crate) inner: Arc<dyn Storage>,
    pub(crate) binding: String,
}

fn parse_path(path: String) -> PyResult<Path> {
    Path::parse(path).map_err(|error| PyValueError::new_err(error.to_string()))
}

#[pymethods]
impl StorageHandle {
    fn get<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let binding = self.binding.clone();
        future_into_py(py, async move {
            let location = parse_path(path)?;
            let result = inner
                .get(&location)
                .await
                .map_err(|error| map_object_store_error(error, &binding, "get"))?;
            Ok(result
                .bytes()
                .await
                .map_err(|error| map_object_store_error(error, &binding, "get"))?
                .to_vec())
        })
    }

    fn get_optional<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let binding = self.binding.clone();
        future_into_py(py, async move {
            let location = parse_path(path)?;
            let result = match inner.get(&location).await {
                Ok(result) => result,
                Err(object_store::Error::NotFound { .. }) => return Ok(None),
                Err(error) => return Err(map_object_store_error(error, &binding, "get")),
            };
            let data = result
                .bytes()
                .await
                .map_err(|error| map_object_store_error(error, &binding, "get"))?;
            Ok(Some(data.to_vec()))
        })
    }

    #[pyo3(signature = (path, data, condition=None))]
    fn put<'py>(
        &self,
        py: Python<'py>,
        path: String,
        data: Vec<u8>,
        condition: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let binding = self.binding.clone();
        future_into_py(py, async move {
            let mode = match condition.as_deref() {
                None => PutMode::Overwrite,
                Some("absent") => PutMode::Create,
                Some(other) => {
                    return Err(map_alien_error(AlienError::new(ErrorData::InvalidInput {
                        operation_context: "storage.put".to_string(),
                        details: format!("unsupported condition '{other}', expected 'absent'"),
                        field_name: Some("condition".to_string()),
                    })));
                }
            };
            inner
                .put_opts(
                    &parse_path(path)?,
                    PutPayload::from(data),
                    PutOptions {
                        mode,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| map_object_store_error(error, &binding, "put"))?;
            Ok(())
        })
    }

    fn delete<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let binding = self.binding.clone();
        future_into_py(py, async move {
            inner
                .delete(&parse_path(path)?)
                .await
                .map_err(|error| map_object_store_error(error, &binding, "delete"))
        })
    }

    fn list<'py>(&self, py: Python<'py>, prefix: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let binding = self.binding.clone();
        future_into_py(py, async move {
            let prefix = prefix.map(parse_path).transpose()?;
            inner
                .list(prefix.as_ref())
                .map_ok(|meta| ObjectInfo {
                    location: meta.location.to_string(),
                    size: meta.size,
                    last_modified: meta.last_modified.to_rfc3339(),
                    e_tag: meta.e_tag,
                    version: meta.version,
                })
                .try_collect::<Vec<ObjectInfo>>()
                .await
                .map_err(|error| map_object_store_error(error, &binding, "list"))
        })
    }

    fn signed_url<'py>(
        &self,
        py: Python<'py>,
        method: String,
        path: String,
        expires_in: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let path = parse_path(path)?;
            let expires = Duration::from_secs(expires_in);
            let request = match method.as_str() {
                "GET" => inner.presigned_get(&path, expires).await,
                "PUT" => inner.presigned_put(&path, expires).await,
                "DELETE" => inner.presigned_delete(&path, expires).await,
                _ => return Err(PyValueError::new_err("method must be GET, PUT, or DELETE")),
            }
            .map_err(map_alien_error)?;
            Ok(SignedRequest {
                url: request.url(),
                method: request.method().to_string(),
                headers: request.headers(),
            })
        })
    }
}
