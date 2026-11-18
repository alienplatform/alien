//! One key's objects in Azure Blob Storage, reached with a directory SAS for that key only.
//!
//! The agent never holds an Azure identity. The binding mints a user-delegation SAS scoped to
//! the key's directory (`sr=d` on a hierarchical-namespace account), with read, add, create and
//! write rights and nothing else, and hands it to the agent, which renews it before it expires.
//! The SAS can neither list nor delete, and cannot reach another key's directory: Azure answers
//! 403, which this store reports as a failure, never as an absent object or a lost race.

use std::sync::RwLock;
use std::time::Duration;

use alien_error::AlienError;
use async_trait::async_trait;
use reqwest::{header, Client, StatusCode};

use crate::error::{ErrorData, Result};
use crate::layout::validate_tree_path;
use crate::store::{ObjectStore, Precondition, PutOutcome, StoredObject};

/// Storage service version every request is sent with.
const VERSION: &str = "2026-10-06";
/// Largest blob one `Put Blob` can write.
const MAX_PUT_BYTES: usize = 5000 * 1024 * 1024;

/// A key's directory in a container, with the SAS that reaches it.
pub struct AzureBlobStore {
    client: Client,
    /// `https://<account>.blob.core.windows.net/<container>/<key root>/`, ending in `/`.
    key_url: String,
    sas: RwLock<String>,
}

impl std::fmt::Debug for AzureBlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The SAS is a credential: never printed.
        f.debug_struct("AzureBlobStore")
            .field("key_url", &self.key_url)
            .finish_non_exhaustive()
    }
}

impl AzureBlobStore {
    /// A store for the key at `key_url` (HTTPS, ending in `/`), authorized by `sas` (a query
    /// string without the leading `?`).
    pub fn new(key_url: &str, sas: &str) -> Result<Self> {
        let invalid = |reason: &str| {
            AlienError::new(ErrorData::StoreFailed {
                operation: "configure".to_string(),
                object: key_url.to_string(),
                reason: reason.to_string(),
            })
        };
        let parsed = url::Url::parse(key_url).map_err(|_| invalid("not a URL"))?;
        if parsed.scheme() != "https" {
            return Err(invalid("the key URL must be HTTPS"));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() || !key_url.ends_with('/') {
            return Err(invalid(
                "the key URL must end in '/' and carry no query; the SAS is passed apart",
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| invalid(&e.without_url().to_string()))?;
        Ok(Self {
            client,
            key_url: key_url.to_string(),
            sas: RwLock::new(sas.trim_start_matches('?').to_string()),
        })
    }

    /// Replaces the SAS with a renewed one. Requests already sent keep the old one.
    pub fn renew(&self, sas: &str) {
        *self.sas.write().expect("not poisoned") = sas.trim_start_matches('?').to_string();
    }

    fn url(&self, object: &str) -> Result<String> {
        // Object names come from the layout (`HEAD`, `packs/…`, `manifests/…`); refusing
        // anything else keeps a request from addressing outside the key, even before Azure would.
        validate_tree_path(object)?;
        let sas = self.sas.read().expect("not poisoned");
        Ok(format!("{}{}?{}", self.key_url, object, sas))
    }

    async fn send(
        &self,
        operation: &str,
        object: &str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        request
            .header("x-ms-version", VERSION)
            .send()
            .await
            // The URL carries the SAS, so it never goes into an error.
            .map_err(|e| failed(operation, object, &e.without_url().to_string()))
    }
}

fn failed(operation: &str, object: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreFailed {
        operation: operation.to_string(),
        object: object.to_string(),
        reason: reason.to_string(),
    })
}

/// A refusal Azure answered, with its error code and never the request URL.
async fn refused(
    operation: &str,
    object: &str,
    response: reqwest::Response,
) -> AlienError<ErrorData> {
    let status = response.status();
    let code = response
        .headers()
        .get("x-ms-error-code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("no error code")
        .to_string();
    failed(
        operation,
        object,
        &format!("Azure answered {status} ({code})"),
    )
}

fn etag(response: &reqwest::Response, operation: &str, object: &str) -> Result<String> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| failed(operation, object, "Azure answered without an ETag"))
}

#[async_trait]
impl ObjectStore for AzureBlobStore {
    async fn get(&self, object: &str) -> Result<Option<StoredObject>> {
        let response = self
            .send("get", object, self.client.get(self.url(object)?))
            .await?;
        match response.status() {
            StatusCode::OK => {
                let etag = etag(&response, "get", object)?;
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| failed("get", object, &e.without_url().to_string()))?;
                Ok(Some(StoredObject {
                    bytes: bytes.to_vec(),
                    etag,
                }))
            }
            StatusCode::NOT_FOUND => Ok(None),
            _ => Err(refused("get", object, response).await),
        }
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let last = offset.saturating_add(len - 1);
        let response = self
            .send(
                "get",
                object,
                self.client
                    .get(self.url(object)?)
                    .header(header::RANGE, format!("bytes={offset}-{last}")),
            )
            .await?;
        match response.status() {
            StatusCode::PARTIAL_CONTENT | StatusCode::OK => response
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| failed("get", object, &e.without_url().to_string())),
            _ => Err(refused("get", object, response).await),
        }
    }

    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> Result<PutOutcome> {
        if bytes.len() > MAX_PUT_BYTES {
            return Err(failed(
                "put",
                object,
                "larger than the 5000 MiB one Azure Put Blob can write",
            ));
        }
        let (name, value) = match &precondition {
            Precondition::Absent => (header::IF_NONE_MATCH, "*".to_string()),
            Precondition::Matches(etag) => (header::IF_MATCH, etag.clone()),
        };
        let response = self
            .send(
                "put",
                object,
                self.client
                    .put(self.url(object)?)
                    .header("x-ms-blob-type", "BlockBlob")
                    .header(header::CONTENT_LENGTH, bytes.len())
                    .header(name, value)
                    .body(bytes),
            )
            .await?;
        match response.status() {
            StatusCode::CREATED => Ok(PutOutcome::Written {
                etag: etag(&response, "put", object)?,
            }),
            // `If-None-Match: *` on an existing blob is 409 BlobAlreadyExists; `If-Match` on a
            // changed blob is 412, and on a deleted one (an erased key) 404 or 412: in every case
            // nothing was written because the object is not what the writer expected.
            StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => Ok(PutOutcome::Conflict),
            StatusCode::NOT_FOUND if matches!(precondition, Precondition::Matches(_)) => {
                Ok(PutOutcome::Conflict)
            }
            _ => Err(refused("put", object, response).await),
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        Err(failed(
            "list",
            prefix,
            "a key's SAS cannot list; listing is the binding's, with its own identity",
        ))
    }

    async fn delete(&self, object: &str) -> Result<()> {
        Err(failed(
            "delete",
            object,
            "a key's SAS cannot delete; deletion is the binding's, with its own identity",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_key_url_that_is_not_https_or_carries_a_query() {
        for url in [
            "http://a.blob.core.windows.net/c/k/",
            "https://a.blob.core.windows.net/c/k",
            "https://a.blob.core.windows.net/c/k/?sig=x",
            "not a url",
        ] {
            assert!(AzureBlobStore::new(url, "sig=x").is_err(), "{url}");
        }
        AzureBlobStore::new(
            "https://a.blob.core.windows.net/c/sandbox-storage/s/k/",
            "?sig=x",
        )
        .unwrap();
    }

    #[test]
    fn addresses_only_names_inside_the_key() {
        let store = AzureBlobStore::new(
            "https://a.blob.core.windows.net/c/sandbox-storage/s/k/",
            "sig=x",
        )
        .unwrap();
        assert_eq!(
            store.url("packs/1.tar").unwrap(),
            "https://a.blob.core.windows.net/c/sandbox-storage/s/k/packs/1.tar?sig=x"
        );
        for object in ["../other/HEAD", "/HEAD", "a//b", ""] {
            assert!(store.url(object).is_err(), "{object}");
        }
        store.renew("?sig=y");
        assert!(store.url("HEAD").unwrap().ends_with("HEAD?sig=y"));
    }

    #[test]
    fn never_prints_the_sas() {
        let store = AzureBlobStore::new(
            "https://a.blob.core.windows.net/c/sandbox-storage/s/k/",
            "sig=SECRET",
        )
        .unwrap();
        assert!(!format!("{store:?}").contains("SECRET"));
    }
}
