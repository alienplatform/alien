//! Key-value binding handle. Thin argument/error translation over the `Kv`
//! trait.

use crate::error::map_alien_error;
#[cfg(test)]
use alien_bindings::traits::KvEntry;
use alien_bindings::traits::{Kv, PutCondition, PutOptions, ScanResult};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use std::sync::Arc;
use std::time::Duration;

/// A single key-value pair returned by a scan.
#[napi(object)]
pub struct KvItemJs {
    /// The key.
    pub key: String,
    /// The value bytes.
    pub value: Buffer,
    /// Opaque version for a later conditional set or delete.
    pub version: String,
}

/// A page of scan results.
#[napi(object)]
pub struct ScanResultJs {
    /// Items found on this page (may be fewer than the requested limit).
    pub items: Vec<KvItemJs>,
    /// Opaque cursor for the next page, or `None` when exhausted.
    pub next_cursor: Option<String>,
}

/// Translate a `ScanResult` into its JS shape.
fn scan_to_js(result: ScanResult) -> ScanResultJs {
    ScanResultJs {
        items: result
            .items
            .into_iter()
            .map(|entry| KvItemJs {
                key: entry.key,
                value: Buffer::from(entry.value),
                version: entry.version,
            })
            .collect(),
        next_cursor: result.next_cursor,
    }
}

fn put_options(
    ttl_secs: Option<u32>,
    condition: Option<String>,
    version: Option<String>,
) -> napi::Result<Option<PutOptions>> {
    let condition =
        match condition.as_deref() {
            None => PutCondition::None,
            Some("absent") => PutCondition::Absent,
            Some("version") => PutCondition::Version(version.ok_or_else(|| {
                napi::Error::from_reason("a version condition requires a version")
            })?),
            Some(other) => {
                return Err(napi::Error::from_reason(format!(
                    "unsupported KV put condition '{other}'"
                )));
            }
        };
    if ttl_secs.is_none() && matches!(condition, PutCondition::None) {
        return Ok(None);
    }
    Ok(Some(PutOptions {
        ttl: ttl_secs.map(|secs| Duration::from_secs(u64::from(secs))),
        condition,
    }))
}

/// Handle to a resolved key-value binding.
#[napi]
pub struct KvHandle {
    inner: Arc<dyn Kv>,
}

impl KvHandle {
    pub(crate) fn new(inner: Arc<dyn Kv>) -> Self {
        Self { inner }
    }
}

#[napi]
impl KvHandle {
    /// Get the entry for `key`, or `None` if absent/expired.
    #[napi]
    pub async fn get(&self, key: String) -> napi::Result<Option<KvItemJs>> {
        let kv = self.inner.clone();
        let entry = kv.get(&key).await.map_err(map_alien_error)?;
        Ok(entry.map(|entry| KvItemJs {
            key: entry.key,
            value: Buffer::from(entry.value),
            version: entry.version,
        }))
    }

    /// Put `value` at `key`.
    ///
    /// Returns `false` when a supplied precondition does not match.
    #[napi]
    pub async fn put(
        &self,
        key: String,
        value: Buffer,
        ttl_secs: Option<u32>,
        condition: Option<String>,
        version: Option<String>,
    ) -> napi::Result<bool> {
        let kv = self.inner.clone();
        let options = put_options(ttl_secs, condition, version)?;
        kv.put(&key, value.to_vec(), options)
            .await
            .map_err(map_alien_error)
    }

    /// Delete `key`, optionally only at the supplied version.
    #[napi]
    pub async fn delete(&self, key: String, if_version: Option<String>) -> napi::Result<bool> {
        let kv = self.inner.clone();
        kv.delete(&key, if_version.as_deref())
            .await
            .map_err(map_alien_error)
    }

    /// Check whether `key` exists.
    #[napi]
    pub async fn exists(&self, key: String) -> napi::Result<bool> {
        let kv = self.inner.clone();
        kv.exists(&key).await.map_err(map_alien_error)
    }

    /// Scan keys under `prefix` with optional pagination.
    #[napi]
    pub async fn scan(
        &self,
        prefix: String,
        limit: Option<u32>,
        cursor: Option<String>,
    ) -> napi::Result<ScanResultJs> {
        let kv = self.inner.clone();
        let result = kv
            .scan_prefix(&prefix, limit.map(|l| l as usize), cursor)
            .await
            .map_err(map_alien_error)?;
        Ok(scan_to_js(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_options_none_when_unconditional_and_without_ttl() {
        assert!(put_options(None, None, None).unwrap().is_none());
    }

    #[test]
    fn put_options_sets_ttl_and_condition() {
        let opts = put_options(Some(30), Some("absent".to_string()), None)
            .unwrap()
            .expect("options should be present");
        assert_eq!(opts.ttl, Some(Duration::from_secs(30)));
        assert_eq!(opts.condition, PutCondition::Absent);

        let ttl_only = put_options(Some(5), None, None)
            .unwrap()
            .expect("options should be present");
        assert_eq!(ttl_only.ttl, Some(Duration::from_secs(5)));
        assert_eq!(ttl_only.condition, PutCondition::None);

        let version = put_options(
            None,
            Some("version".to_string()),
            Some("opaque".to_string()),
        )
        .unwrap()
        .expect("options should be present");
        assert_eq!(
            version.condition,
            PutCondition::Version("opaque".to_string())
        );
    }

    #[test]
    fn scan_to_js_maps_items_and_cursor() {
        let result = ScanResult {
            items: vec![
                KvEntry {
                    key: "a".to_string(),
                    value: b"one".to_vec(),
                    version: "v1".to_string(),
                },
                KvEntry {
                    key: "b".to_string(),
                    value: b"two".to_vec(),
                    version: "v2".to_string(),
                },
            ],
            next_cursor: Some("next".to_string()),
        };

        let js = scan_to_js(result);

        assert_eq!(js.items.len(), 2);
        assert_eq!(js.items[0].key, "a");
        assert_eq!(js.items[0].value.as_ref(), b"one");
        assert_eq!(js.items[0].version, "v1");
        assert_eq!(js.items[1].key, "b");
        assert_eq!(js.items[1].value.as_ref(), b"two");
        assert_eq!(js.next_cursor, Some("next".to_string()));
    }
}
