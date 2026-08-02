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
use object_store::{
    Attribute, Attributes, GetOptions, ObjectMeta, PutOptions, PutPayload, PutResult,
};
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
    /// Provider entity tag, when available.
    pub e_tag: Option<String>,
    /// Provider object version, when available.
    pub version: Option<String>,
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

/// Provider-neutral object attributes accepted on a storage write.
#[napi(object)]
pub struct StoragePutAttributesJs {
    /// MIME type stored with the object.
    pub content_type: Option<String>,
    /// Browser content-disposition behavior stored with the object.
    pub content_disposition: Option<String>,
    /// Content encoding stored with the object.
    pub content_encoding: Option<String>,
    /// Content language stored with the object.
    pub content_language: Option<String>,
    /// Cache-control policy stored with the object.
    pub cache_control: Option<String>,
    /// User-defined object metadata.
    pub metadata: Option<HashMap<String, String>>,
}

/// Options for a storage write.
#[napi(object)]
pub struct StoragePutOptionsJs {
    /// Object attributes to persist with the payload.
    pub attributes: Option<StoragePutAttributesJs>,
}

/// Provider-neutral attributes returned with a stored object.
#[napi(object)]
pub struct StorageObjectAttributesJs {
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub cache_control: Option<String>,
    pub storage_class: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Result of reading a stored object.
#[napi(object)]
pub struct StorageGetResultJs {
    pub data: Buffer,
    pub meta: ObjectMetaJs,
    pub attributes: StorageObjectAttributesJs,
}

/// Result of reading object metadata without its payload.
#[napi(object)]
pub struct StorageHeadResultJs {
    pub meta: ObjectMetaJs,
    pub attributes: StorageObjectAttributesJs,
}

/// Provider identifiers returned after a successful storage write.
#[napi(object)]
pub struct StoragePutResultJs {
    pub e_tag: Option<String>,
    pub version: Option<String>,
}

pub(crate) fn object_store_put_options(options: StoragePutOptionsJs) -> PutOptions {
    let Some(options) = options.attributes else {
        return PutOptions::default();
    };
    let mut attributes = Attributes::new();
    for (attribute, value) in [
        (Attribute::ContentType, options.content_type),
        (Attribute::ContentDisposition, options.content_disposition),
        (Attribute::ContentEncoding, options.content_encoding),
        (Attribute::ContentLanguage, options.content_language),
        (Attribute::CacheControl, options.cache_control),
    ] {
        if let Some(value) = value {
            attributes.insert(attribute, value.into());
        }
    }
    for (key, value) in options.metadata.unwrap_or_default() {
        attributes.insert(Attribute::Metadata(key.into()), value.into());
    }
    PutOptions {
        attributes,
        ..Default::default()
    }
}

pub(crate) fn object_attributes_to_js(attributes: &Attributes) -> StorageObjectAttributesJs {
    let value = |attribute| {
        attributes
            .get(&attribute)
            .map(|value| value.as_ref().to_string())
    };
    let metadata = attributes
        .iter()
        .filter_map(|(attribute, value)| match attribute {
            Attribute::Metadata(key) => Some((key.to_string(), value.as_ref().to_string())),
            _ => None,
        })
        .collect();

    StorageObjectAttributesJs {
        content_type: value(Attribute::ContentType),
        content_disposition: value(Attribute::ContentDisposition),
        content_encoding: value(Attribute::ContentEncoding),
        content_language: value(Attribute::ContentLanguage),
        cache_control: value(Attribute::CacheControl),
        storage_class: value(Attribute::StorageClass),
        metadata,
    }
}

/// Translate an `object_store::ObjectMeta` into its JS shape.
pub(crate) fn object_meta_to_js(meta: &ObjectMeta) -> ObjectMetaJs {
    ObjectMetaJs {
        location: meta.location.to_string(),
        size: meta.size as f64,
        last_modified: meta.last_modified.to_rfc3339(),
        e_tag: meta.e_tag.clone(),
        version: meta.version.clone(),
    }
}

pub(crate) fn put_result_to_js(result: PutResult) -> StoragePutResultJs {
    StoragePutResultJs {
        e_tag: result.e_tag,
        version: result.version,
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
    pub async fn get(&self, path: String) -> napi::Result<StorageGetResultJs> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "get")?;
        let result = store
            .get(&location)
            .await
            .map_err(|e| map_object_store_error(e, &binding, "get"))?;
        let meta = object_meta_to_js(&result.meta);
        let attributes = object_attributes_to_js(&result.attributes);
        let data = result
            .bytes()
            .await
            .map_err(|e| map_object_store_error(e, &binding, "get"))?;
        Ok(StorageGetResultJs {
            data: Buffer::from(data.to_vec()),
            meta,
            attributes,
        })
    }

    /// Store `data` at `path`.
    #[napi]
    pub async fn put(
        &self,
        path: String,
        data: Buffer,
        options: Option<StoragePutOptionsJs>,
    ) -> napi::Result<StoragePutResultJs> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "put")?;
        let payload = PutPayload::from(data.to_vec());
        let result = match options {
            Some(options) => {
                store
                    .put_opts(&location, payload, object_store_put_options(options))
                    .await
            }
            None => store.put(&location, payload).await,
        }
        .map_err(|e| map_object_store_error(e, &binding, "put"))?;
        Ok(put_result_to_js(result))
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
    pub async fn head(&self, path: String) -> napi::Result<StorageHeadResultJs> {
        let store = self.inner.clone();
        let binding = self.binding.clone();
        let location = parse_path(path, "path", "head")?;
        let result = store
            .get_opts(
                &location,
                GetOptions {
                    head: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| map_object_store_error(e, &binding, "head"))?;
        Ok(StorageHeadResultJs {
            meta: object_meta_to_js(&result.meta),
            attributes: object_attributes_to_js(&result.attributes),
        })
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
    use object_store::AttributeValue;

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
            e_tag: Some("etag-123".to_string()),
            version: Some("version-456".to_string()),
        };

        let js = object_meta_to_js(&meta);

        assert_eq!(js.location, "dir/file.txt");
        assert_eq!(js.size, 1234.0);
        assert_eq!(js.last_modified, "2026-07-06T12:00:00+00:00");
        assert_eq!(js.e_tag.as_deref(), Some("etag-123"));
        assert_eq!(js.version.as_deref(), Some("version-456"));
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

    #[test]
    fn storage_put_options_map_every_object_attribute() {
        let options = object_store_put_options(StoragePutOptionsJs {
            attributes: Some(StoragePutAttributesJs {
                content_type: Some("message/rfc822".to_string()),
                content_disposition: Some("attachment; filename=message.eml".to_string()),
                content_encoding: Some("gzip".to_string()),
                content_language: Some("en-US".to_string()),
                cache_control: Some("private, max-age=60".to_string()),
                metadata: Some(HashMap::from([
                    ("message-id".to_string(), "msg_123".to_string()),
                    ("source".to_string(), "inbound".to_string()),
                ])),
            }),
        });

        let expected = Attributes::from_iter([
            (
                Attribute::ContentType,
                AttributeValue::from("message/rfc822"),
            ),
            (
                Attribute::ContentDisposition,
                AttributeValue::from("attachment; filename=message.eml"),
            ),
            (Attribute::ContentEncoding, AttributeValue::from("gzip")),
            (Attribute::ContentLanguage, AttributeValue::from("en-US")),
            (
                Attribute::CacheControl,
                AttributeValue::from("private, max-age=60"),
            ),
            (
                Attribute::Metadata("message-id".into()),
                AttributeValue::from("msg_123"),
            ),
            (
                Attribute::Metadata("source".into()),
                AttributeValue::from("inbound"),
            ),
        ]);

        assert_eq!(options.attributes, expected);
    }

    #[test]
    fn object_attributes_to_js_maps_headers_storage_class_and_metadata() {
        let attributes = Attributes::from_iter([
            (Attribute::ContentType, AttributeValue::from("text/plain")),
            (
                Attribute::ContentDisposition,
                AttributeValue::from("inline"),
            ),
            (Attribute::ContentEncoding, AttributeValue::from("gzip")),
            (Attribute::ContentLanguage, AttributeValue::from("en-US")),
            (
                Attribute::CacheControl,
                AttributeValue::from("private, max-age=60"),
            ),
            (Attribute::StorageClass, AttributeValue::from("STANDARD")),
            (
                Attribute::Metadata("source".into()),
                AttributeValue::from("upload"),
            ),
        ]);

        let js = object_attributes_to_js(&attributes);

        assert_eq!(js.content_type.as_deref(), Some("text/plain"));
        assert_eq!(js.content_disposition.as_deref(), Some("inline"));
        assert_eq!(js.content_encoding.as_deref(), Some("gzip"));
        assert_eq!(js.content_language.as_deref(), Some("en-US"));
        assert_eq!(js.cache_control.as_deref(), Some("private, max-age=60"));
        assert_eq!(js.storage_class.as_deref(), Some("STANDARD"));
        assert_eq!(
            js.metadata.get("source").map(String::as_str),
            Some("upload")
        );
    }

    #[test]
    fn put_result_to_js_preserves_provider_identifiers() {
        let js = put_result_to_js(PutResult {
            e_tag: Some("etag-123".to_string()),
            version: Some("version-456".to_string()),
        });

        assert_eq!(js.e_tag.as_deref(), Some("etag-123"));
        assert_eq!(js.version.as_deref(), Some("version-456"));
    }
}
