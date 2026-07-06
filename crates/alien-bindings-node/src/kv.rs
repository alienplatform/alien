//! Key-value binding handle. Thin argument/error translation over the `Kv`
//! trait.

use crate::error::map_alien_error;
use alien_bindings::traits::{Kv, PutOptions, ScanResult};
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
            .map(|(key, value)| KvItemJs {
                key,
                value: Buffer::from(value),
            })
            .collect(),
        next_cursor: result.next_cursor,
    }
}

/// Build `PutOptions` from the optional JS arguments, or `None` when neither is
/// set (so the provider takes its default unconditional-put path).
fn put_options(ttl_secs: Option<u32>, if_not_exists: Option<bool>) -> Option<PutOptions> {
    let if_not_exists = if_not_exists.unwrap_or(false);
    if ttl_secs.is_none() && !if_not_exists {
        return None;
    }
    Some(PutOptions {
        ttl: ttl_secs.map(|secs| Duration::from_secs(u64::from(secs))),
        if_not_exists,
    })
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
    /// Get the value for `key`, or `None` if absent/expired.
    #[napi]
    pub async fn get(&self, key: String) -> napi::Result<Option<Buffer>> {
        let kv = self.inner.clone();
        let value = kv.get(&key).await.map_err(map_alien_error)?;
        Ok(value.map(Buffer::from))
    }

    /// Put `value` at `key`.
    ///
    /// With `if_not_exists`, returns `true` when created and `false` when the
    /// key already existed; otherwise always returns `true`.
    #[napi]
    pub async fn put(
        &self,
        key: String,
        value: Buffer,
        ttl_secs: Option<u32>,
        if_not_exists: Option<bool>,
    ) -> napi::Result<bool> {
        let kv = self.inner.clone();
        let options = put_options(ttl_secs, if_not_exists);
        kv.put(&key, value.to_vec(), options)
            .await
            .map_err(map_alien_error)
    }

    /// Delete `key` (no error if absent).
    #[napi]
    pub async fn delete(&self, key: String) -> napi::Result<()> {
        let kv = self.inner.clone();
        kv.delete(&key).await.map_err(map_alien_error)
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
    fn put_options_none_when_no_ttl_and_no_if_not_exists() {
        assert!(put_options(None, None).is_none());
        assert!(put_options(None, Some(false)).is_none());
    }

    #[test]
    fn put_options_sets_ttl_and_flag() {
        let opts = put_options(Some(30), Some(true)).expect("options should be present");
        assert_eq!(opts.ttl, Some(Duration::from_secs(30)));
        assert!(opts.if_not_exists);

        let ttl_only = put_options(Some(5), None).expect("options should be present");
        assert_eq!(ttl_only.ttl, Some(Duration::from_secs(5)));
        assert!(!ttl_only.if_not_exists);

        let flag_only = put_options(None, Some(true)).expect("options should be present");
        assert_eq!(flag_only.ttl, None);
        assert!(flag_only.if_not_exists);
    }

    #[test]
    fn scan_to_js_maps_items_and_cursor() {
        let result = ScanResult {
            items: vec![
                ("a".to_string(), b"one".to_vec()),
                ("b".to_string(), b"two".to_vec()),
            ],
            next_cursor: Some("next".to_string()),
        };

        let js = scan_to_js(result);

        assert_eq!(js.items.len(), 2);
        assert_eq!(js.items[0].key, "a");
        assert_eq!(js.items[0].value.as_ref(), b"one");
        assert_eq!(js.items[1].key, "b");
        assert_eq!(js.items[1].value.as_ref(), b"two");
        assert_eq!(js.next_cursor, Some("next".to_string()));
    }
}
