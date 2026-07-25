//! Storage binding handle. Thin argument/error translation over the
//! `ObjectStore` supertrait plus the inherent presigned-request methods.

use crate::error::{map_alien_error, map_object_store_error};
use alien_bindings::error::ErrorData;
use alien_bindings::presigned::PresignedRequest;
use alien_bindings::Storage;
use alien_error::AlienError;
use futures::StreamExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use object_store::path::Path;
use object_store::{ObjectMeta, PutPayload};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Metadata for a single stored object.
#[napi(object)]
pub struct ObjectMetaJs {
    /// Object location (path) within the store.
    pub location: String,
    /// Object size in bytes.
    pub size: f64,
    /// Last-modified timestamp as an RFC 3339 string.
    pub last_modified: String,
}

/// A presigned request: a URL plus the method and headers to replay it with.
#[napi(object)]
pub struct PresignedRequestJs {
    /// The (possibly `local://`) URL to send the request to.
    pub url: String,
    /// HTTP method (`GET` | `PUT` | `DELETE`).
    pub method: String,
    /// Headers to include with the request.
    pub headers: HashMap<String, String>,
}

/// Translate an `object_store::ObjectMeta` into its JS shape.
fn object_meta_to_js(meta: &ObjectMeta) -> ObjectMetaJs {
    ObjectMetaJs {
        location: meta.location.to_string(),
        size: meta.size as f64,
        last_modified: meta.last_modified.to_rfc3339(),
    }
}

/// Translate a `PresignedRequest` into its JS shape.
fn presigned_to_js(request: &PresignedRequest) -> PresignedRequestJs {
    PresignedRequestJs {
        url: request.url(),
        method: request.method().to_string(),
        headers: request.headers(),
    }
}

fn parse_path(path: String, field_name: &str, operation_context: &str) -> napi::Result<Path> {
    Path::parse(path).map_err(|error| {
        map_alien_error(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation_context.to_string(),
            details: error.to_string(),
            field_name: Some(field_name.to_string()),
        }))
    })
}

/// Handle to a resolved storage binding.
#[napi]
pub struct StorageHandle {
    inner: Arc<dyn Storage>,
    binding: String,
}

impl StorageHandle {
    /// Construct a handle. Called by `BindingsHandle::storage`; the binding name
    /// is retained so `object_store` errors can name it.
    pub(crate) fn new(inner: Arc<dyn Storage>, binding: String) -> Self {
        Self { inner, binding }
    }
}

#[napi]
impl StorageHandle {
    /// Fetch the object at `path`.
    #[napi]
    pub async fn get(&self, path: String) -> napi::Result<Buffer> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "get")?;
        let result = store
            .get(&location)
            .await
            .map_err(|e| map_object_store_error(e, &binding, "get"))?;
        let bytes = result
            .bytes()
            .await
            .map_err(|e| map_object_store_error(e, &binding, "get"))?;
        Ok(Buffer::from(bytes.to_vec()))
    }

    /// Store `data` at `path`.
    #[napi]
    pub async fn put(&self, path: String, data: Buffer) -> napi::Result<()> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "put")?;
        store
            .put(&location, PutPayload::from(data.to_vec()))
            .await
            .map_err(|e| map_object_store_error(e, &binding, "put"))?;
        Ok(())
    }

    /// Delete the object at `path`.
    #[napi]
    pub async fn delete(&self, path: String) -> napi::Result<()> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "delete")?;
        store
            .delete(&location)
            .await
            .map_err(|e| map_object_store_error(e, &binding, "delete"))?;
        Ok(())
    }

    /// List objects, optionally filtered by `prefix`.
    #[napi]
    pub async fn list(&self, prefix: Option<String>) -> napi::Result<Vec<ObjectMetaJs>> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let prefix = prefix
            .map(|prefix| parse_path(prefix, "prefix", "list"))
            .transpose()?;
        let mut stream = store.list(prefix.as_ref());
        let mut metas = Vec::new();
        while let Some(item) = stream.next().await {
            let meta = item.map_err(|e| map_object_store_error(e, &binding, "list"))?;
            metas.push(object_meta_to_js(&meta));
        }
        Ok(metas)
    }

    /// Fetch metadata for the object at `path`.
    #[napi]
    pub async fn head(&self, path: String) -> napi::Result<ObjectMetaJs> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "head")?;
        let meta = store
            .head(&location)
            .await
            .map_err(|e| map_object_store_error(e, &binding, "head"))?;
        Ok(object_meta_to_js(&meta))
    }

    /// Copy the object at `from` to `to`.
    #[napi]
    pub async fn copy(&self, from: String, to: String) -> napi::Result<()> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let from = parse_path(from, "from", "copy")?;
        let to = parse_path(to, "to", "copy")?;
        store
            .copy(&from, &to)
            .await
            .map_err(|e| map_object_store_error(e, &binding, "copy"))?;
        Ok(())
    }

    /// Create a presigned request for `path`.
    ///
    /// `method` must be `GET`, `PUT`, or `DELETE`; `expires_in_secs` is the
    /// request's validity window.
    #[napi]
    pub async fn signed_url(
        &self,
        method: String,
        path: String,
        expires_in_secs: u32,
    ) -> napi::Result<PresignedRequestJs> {
        let store = self.inner.clone();
        let location = parse_path(path, "path", "signed_url")?;
        let expires_in = Duration::from_secs(u64::from(expires_in_secs));
        let request = match method.as_str() {
            "GET" => store.presigned_get(&location, expires_in).await,
            "PUT" => store.presigned_put(&location, expires_in).await,
            "DELETE" => store.presigned_delete(&location, expires_in).await,
            other => {
                return Err(map_alien_error(AlienError::new(ErrorData::InvalidInput {
                    operation_context: "signed_url".to_string(),
                    details: format!("unsupported method '{other}', expected GET, PUT, or DELETE"),
                    field_name: Some("method".to_string()),
                })));
            }
        }
        .map_err(map_alien_error)?;
        Ok(presigned_to_js(&request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::presigned::PresignedOperation;
    use chrono::{TimeZone, Utc};

    #[test]
    fn parse_path_preserves_rfc_message_id_characters() {
        let raw = "<0100019f@example.com>/message.eml";

        let path = parse_path(raw.to_string(), "path", "signed_url")
            .expect("RFC Message-ID characters should form a valid object path");

        assert_eq!(path.as_ref(), raw);
    }

    #[test]
    fn parse_path_rejects_invalid_segments_with_structured_input_error() {
        let error = parse_path("messages//raw.eml".to_string(), "path", "signed_url")
            .expect_err("empty path segments should be rejected");

        assert!(
            error.reason.contains("\"code\":\"INVALID_INPUT\"")
                && error.reason.contains("\"field_name\":\"path\"")
                && error.reason.contains("signed_url"),
            "unexpected error envelope: {}",
            error.reason
        );
    }

    #[test]
    fn object_meta_to_js_maps_location_size_and_timestamp() {
        let meta = ObjectMeta {
            location: Path::from("dir/file.txt"),
            last_modified: Utc.with_ymd_and_hms(2026, 7, 6, 12, 0, 0).unwrap(),
            size: 1234,
            e_tag: None,
            version: None,
        };

        let js = object_meta_to_js(&meta);

        assert_eq!(js.location, "dir/file.txt");
        assert_eq!(js.size, 1234.0);
        assert_eq!(js.last_modified, "2026-07-06T12:00:00+00:00");
    }

    #[test]
    fn presigned_to_js_maps_http_request_fields() {
        let mut headers = HashMap::new();
        headers.insert("x-test".to_string(), "1".to_string());
        let request = PresignedRequest::new_http(
            "https://example.com/obj?sig=abc".to_string(),
            "PUT".to_string(),
            headers,
            PresignedOperation::Put,
            "obj".to_string(),
            Utc.with_ymd_and_hms(2026, 7, 6, 12, 0, 0).unwrap(),
        );

        let js = presigned_to_js(&request);

        assert_eq!(js.url, "https://example.com/obj?sig=abc");
        assert_eq!(js.method, "PUT");
        assert_eq!(js.headers.get("x-test"), Some(&"1".to_string()));
    }

    #[test]
    fn presigned_to_js_maps_local_request_to_local_url() {
        let request = PresignedRequest::new_local(
            "/tmp/data/obj".to_string(),
            PresignedOperation::Get,
            "obj".to_string(),
            Utc.with_ymd_and_hms(2026, 7, 6, 12, 0, 0).unwrap(),
        );

        let js = presigned_to_js(&request);

        assert_eq!(js.url, "local:///tmp/data/obj");
        assert_eq!(js.method, "GET");
        assert!(js.headers.is_empty());
    }
}
