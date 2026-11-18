//! One key's objects in Azure Blob Storage, reached with a directory SAS for that key only.
//!
//! The agent never holds an Azure identity. The binding mints a user-delegation SAS scoped to
//! the key's directory (`sr=d` on a hierarchical-namespace account), with read, add, create and
//! write rights and nothing else, and hands it to the agent, which renews it before it expires.
//! The SAS can neither list nor delete, and cannot reach another key's directory: Azure answers
//! 403, which this store reports as a failure, never as an absent object or a lost race.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use alien_error::AlienError;
use async_trait::async_trait;
use reqwest::{header, header::HeaderMap, Method, StatusCode};

use crate::error::{ErrorData, Result};
use crate::layout::validate_tree_path;
use crate::store::{ObjectStore, ObjectVersion, Precondition, PutOutcome, StoredObject};

/// Storage service version every request is sent with.
const VERSION: &str = "2026-10-06";
/// Largest blob one `Put Blob` can write.
const MAX_PUT_BYTES: usize = 5000 * 1024 * 1024;

/// An answer read to its end.
pub struct BlobResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// Sends one request and reads the whole answer. The URL carries the SAS: an implementation must
/// never put it into an error.
#[async_trait]
pub trait BlobTransport: Send + Sync {
    async fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> std::io::Result<BlobResponse>;
}

/// The default transport: a plain reqwest client.
struct ReqwestTransport(reqwest::Client);

#[async_trait]
impl BlobTransport for ReqwestTransport {
    async fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> std::io::Result<BlobResponse> {
        let mut request = self.0.request(method, url).body(body);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|e| std::io::Error::other(e.without_url().to_string()))?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(|e| std::io::Error::other(e.without_url().to_string()))?;
        Ok(BlobResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

/// A key's directory in a container, with the SAS that reaches it.
pub struct AzureBlobStore {
    transport: Arc<dyn BlobTransport>,
    /// `https://<account>.blob.core.windows.net/<container>/<key root>/`, ending in `/`.
    key_url: String,
    sas: RwLock<String>,
    /// Set when Azure refused the SAS (403) and cleared by a renewal.
    denied: std::sync::atomic::AtomicBool,
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
    /// string without the leading `?`), reached with a plain HTTP client.
    pub fn new(key_url: &str, sas: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| invalid(key_url, &e.without_url().to_string()))?;
        Self::with_transport(key_url, sas, Arc::new(ReqwestTransport(client)))
    }

    /// As [`AzureBlobStore::new`], with every request sent through `transport` (the agent's
    /// own marked sockets, which the guest drop rule lets through).
    pub fn with_transport(
        key_url: &str,
        sas: &str,
        transport: Arc<dyn BlobTransport>,
    ) -> Result<Self> {
        let parsed = url::Url::parse(key_url).map_err(|_| invalid(key_url, "not a URL"))?;
        if parsed.scheme() != "https" {
            return Err(invalid(key_url, "the key URL must be HTTPS"));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() || !key_url.ends_with('/') {
            return Err(invalid(
                key_url,
                "the key URL must end in '/' and carry no query; the SAS is passed apart",
            ));
        }
        Ok(Self {
            transport,
            key_url: key_url.to_string(),
            sas: RwLock::new(sas.trim_start_matches('?').to_string()),
            denied: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Replaces the SAS with a renewed one. Requests already sent keep the old one.
    pub fn renew(&self, sas: &str) {
        *self.sas.write().expect("not poisoned") = sas.trim_start_matches('?').to_string();
        self.denied
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether Azure refused the SAS since it was last renewed.
    pub fn denied(&self) -> bool {
        self.denied.load(std::sync::atomic::Ordering::SeqCst)
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
        method: Method,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> Result<BlobResponse> {
        let url = self.url(object)?;
        let mut all = vec![("x-ms-version", VERSION.to_string())];
        all.extend_from_slice(headers);
        let response = self
            .transport
            .send(method, &url, &all, body)
            .await
            .map_err(|e| failed(operation, object, &redact(&e.to_string(), &url)))?;
        if response.status == StatusCode::FORBIDDEN {
            self.denied
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(response)
    }
}

fn invalid(key_url: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StoreFailed {
        operation: "configure".to_string(),
        object: key_url.to_string(),
        reason: reason.to_string(),
    })
}

/// A transport error with the request's query (the SAS) cut out, should it carry the URL.
fn redact(reason: &str, url: &str) -> String {
    match url.split_once('?') {
        Some((_, query)) if !query.is_empty() => reason.replace(query, "<sas>"),
        _ => reason.to_string(),
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
fn refused(operation: &str, object: &str, response: &BlobResponse) -> AlienError<ErrorData> {
    let code = response
        .headers
        .get("x-ms-error-code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("no error code");
    failed(
        operation,
        object,
        &format!("Azure answered {} ({code})", response.status),
    )
}

fn etag(response: &BlobResponse, operation: &str, object: &str) -> Result<String> {
    response
        .headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| failed(operation, object, "Azure answered without an ETag"))
}

#[async_trait]
impl ObjectStore for AzureBlobStore {
    async fn get(&self, object: &str) -> Result<Option<StoredObject>> {
        let response = self
            .send("get", object, Method::GET, &[], Vec::new())
            .await?;
        match response.status {
            StatusCode::OK => {
                let etag = etag(&response, "get", object)?;
                Ok(Some(StoredObject {
                    bytes: response.body,
                    version: ObjectVersion::etag(etag),
                }))
            }
            StatusCode::NOT_FOUND => Ok(None),
            _ => Err(refused("get", object, &response)),
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
                Method::GET,
                &[("range", format!("bytes={offset}-{last}"))],
                Vec::new(),
            )
            .await?;
        match response.status {
            StatusCode::PARTIAL_CONTENT | StatusCode::OK => Ok(response.body),
            _ => Err(refused("get", object, &response)),
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
        let condition = match &precondition {
            Precondition::Absent => ("if-none-match", "*".to_string()),
            // Azure has no version beside the ETag, so the ETag is the whole precondition.
            Precondition::Matches(version) => ("if-match", version.etag.clone()),
        };
        let response = self
            .send(
                "put",
                object,
                Method::PUT,
                &[
                    ("x-ms-blob-type", "BlockBlob".to_string()),
                    ("content-length", bytes.len().to_string()),
                    condition,
                ],
                bytes,
            )
            .await?;
        match response.status {
            StatusCode::CREATED => Ok(PutOutcome::Written {
                version: ObjectVersion::etag(etag(&response, "put", object)?),
            }),
            // `If-None-Match: *` on an existing blob is 409 BlobAlreadyExists; `If-Match` on a
            // changed blob is 412, and on a deleted one (an erased key) 404 or 412: in every case
            // nothing was written because the object is not what the writer expected.
            StatusCode::CONFLICT | StatusCode::PRECONDITION_FAILED => Ok(PutOutcome::Conflict),
            StatusCode::NOT_FOUND if matches!(precondition, Precondition::Matches(_)) => {
                Ok(PutOutcome::Conflict)
            }
            _ => Err(refused("put", object, &response)),
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

    /// Records requests and answers with a fixed status, or fails quoting the URL.
    struct Fake {
        status: Option<StatusCode>,
        seen: std::sync::Mutex<Vec<(Method, String, Vec<(String, String)>)>>,
    }

    #[async_trait]
    impl BlobTransport for Fake {
        async fn send(
            &self,
            method: Method,
            url: &str,
            headers: &[(&str, String)],
            _body: Vec<u8>,
        ) -> std::io::Result<BlobResponse> {
            self.seen.lock().unwrap().push((
                method,
                url.to_string(),
                headers
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.clone()))
                    .collect(),
            ));
            let Some(status) = self.status else {
                return Err(std::io::Error::other(format!("connect to {url} failed")));
            };
            let mut headers = HeaderMap::new();
            headers.insert(header::ETAG, "\"0x1\"".parse().unwrap());
            Ok(BlobResponse {
                status,
                headers,
                body: Vec::new(),
            })
        }
    }

    const KEY: &str = "https://a.blob.core.windows.net/c/k/";

    #[tokio::test]
    async fn sends_through_its_transport_with_the_precondition() {
        let fake = Arc::new(Fake {
            status: Some(StatusCode::CREATED),
            seen: Default::default(),
        });
        let store = AzureBlobStore::with_transport(KEY, "sig=x", fake.clone()).unwrap();
        let outcome = store
            .put_if("HEAD", b"h".to_vec(), Precondition::Absent)
            .await
            .unwrap();
        assert!(matches!(outcome, PutOutcome::Written { .. }));
        let seen = fake.seen.lock().unwrap();
        let (method, url, headers) = &seen[0];
        assert_eq!(*method, Method::PUT);
        assert_eq!(url, "https://a.blob.core.windows.net/c/k/HEAD?sig=x");
        for expected in [
            ("x-ms-version", VERSION),
            ("x-ms-blob-type", "BlockBlob"),
            ("if-none-match", "*"),
            ("content-length", "1"),
        ] {
            assert!(
                headers
                    .iter()
                    .any(|(n, v)| n == expected.0 && v == expected.1),
                "{expected:?} in {headers:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_transport_error_never_carries_the_sas() {
        let fake = Arc::new(Fake {
            status: None,
            seen: Default::default(),
        });
        let store = AzureBlobStore::with_transport(KEY, "sig=SECRET", fake).unwrap();
        let error = store.get("HEAD").await.unwrap_err();
        let printed = format!("{error:?} {error}");
        assert!(!printed.contains("SECRET"), "{printed}");
    }

    #[tokio::test]
    async fn a_refusal_marks_the_store_denied_until_renewed() {
        let fake = Arc::new(Fake {
            status: Some(StatusCode::FORBIDDEN),
            seen: Default::default(),
        });
        let store = AzureBlobStore::with_transport(KEY, "sig=x", fake).unwrap();
        assert!(store.get("HEAD").await.is_err());
        assert!(store.denied());
        store.renew("sig=y");
        assert!(!store.denied());
    }
}
