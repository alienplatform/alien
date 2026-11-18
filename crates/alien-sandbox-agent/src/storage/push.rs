//! The push transport: the agent writes its key's store directly with the grant the binding
//! minted (an S3 POST policy for `<key>/`, a presigned PUT for `<key>/HEAD`, presigned GETs for
//! the restore). The agent holds no cloud identity, and the grant reaches nothing but one key.
//!
//! Its sockets are the agent's own: commands run in the guest cgroup, whose sockets carry the
//! guest mark and are dropped on the way to the store.

use std::sync::RwLock;

use alien_error::AlienError;
use alien_sandbox_storage::{
    ErrorData as StorageError, ObjectStore, ObjectVersion, Precondition, PutOutcome,
    Result as StorageResult, StorageGrant, StoredObject, HEAD_OBJECT,
};
use async_trait::async_trait;

fn failed(operation: &str, object: &str, reason: impl Into<String>) -> AlienError<StorageError> {
    AlienError::new(StorageError::StoreFailed {
        operation: operation.to_string(),
        object: object.to_string(),
        reason: reason.into(),
    })
}

/// A key's store as a grant reaches it. The grant is replaced on renewal.
pub struct GrantStore {
    grant: RwLock<StorageGrant>,
    http: reqwest::Client,
    /// Set when the store refused the grant (403) and cleared by a renewal.
    denied: std::sync::atomic::AtomicBool,
}

impl GrantStore {
    /// A store using `grant`.
    pub fn new(grant: StorageGrant) -> Self {
        Self {
            grant: RwLock::new(grant),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("a client with rustls builds"),
            denied: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Whether the grant is expired or the store refused it: nothing more can be written until
    /// the binding renews it.
    pub fn denied(&self) -> bool {
        self.denied.load(std::sync::atomic::Ordering::SeqCst)
            || self.grant().expires_at() <= chrono::Utc::now()
    }

    fn observe(&self, status: reqwest::StatusCode) {
        if status == reqwest::StatusCode::FORBIDDEN {
            self.denied.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Replaces the grant (renewal before expiry).
    pub fn renew(&self, grant: StorageGrant) {
        *self.grant.write().expect("not poisoned") = grant;
        self.denied
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    fn grant(&self) -> StorageGrant {
        self.grant.read().expect("not poisoned").clone()
    }

    fn read_url(&self, object: &str) -> Option<String> {
        match self.grant() {
            StorageGrant::S3 { reads, .. } => reads.get(object).cloned(),
        }
    }
}

fn version_of(response: &reqwest::Response) -> ObjectVersion {
    ObjectVersion::etag(
        response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    )
}

/// A multipart/form-data body with the fields in order and the file last, as S3 requires.
fn form(fields: &[(&str, &str)], file: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("alien-{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(file.len() + 4096);
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"object\"\r\nContent-Type: application/octet-stream\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[async_trait]
impl ObjectStore for GrantStore {
    async fn get(&self, object: &str) -> StorageResult<Option<StoredObject>> {
        let url = self
            .read_url(object)
            .ok_or_else(|| failed("get", object, "the grant does not cover this object"))?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(failed("get", object, format!("HTTP {}", response.status())));
        }
        let version = version_of(&response);
        let bytes = response
            .bytes()
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        Ok(Some(StoredObject {
            bytes: bytes.to_vec(),
            version,
        }))
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> StorageResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let url = self
            .read_url(object)
            .ok_or_else(|| failed("get", object, "the grant does not cover this object"))?;
        let response = self
            .http
            .get(url)
            .header(
                reqwest::header::RANGE,
                format!("bytes={offset}-{}", offset + len - 1),
            )
            .send()
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        if !response.status().is_success() {
            return Err(failed("get", object, format!("HTTP {}", response.status())));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        Ok(bytes.to_vec())
    }

    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: Precondition,
    ) -> StorageResult<PutOutcome> {
        let grant = self.grant();
        if grant.expires_at() <= chrono::Utc::now() {
            // Sent anyway it would fail with 403 later and cost an upload; say why now.
            self.denied.store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(failed("put", object, "the storage grant expired"));
        }
        let StorageGrant::S3 {
            post, head_put_url, ..
        } = grant;
        let response = if object == HEAD_OBJECT {
            let request = self.http.put(head_put_url).body(bytes);
            let request = match &precondition {
                Precondition::Matches(version) => {
                    request.header(reqwest::header::IF_MATCH, version.etag.clone())
                }
                Precondition::Absent => request.header(reqwest::header::IF_NONE_MATCH, "*"),
            };
            request.send().await
        } else {
            // Packs and manifests are named by their hash, so an existing name holds the same
            // bytes; a POST upload cannot be made create-only and does not need to be.
            let key = format!("{}{object}", post.key_prefix);
            let mut fields: Vec<(&str, &str)> = vec![("key", key.as_str())];
            fields.extend(
                post.fields
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            );
            let (content_type, body) = form(&fields, &bytes);
            self.http
                .post(&post.url)
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(body)
                .send()
                .await
        }
        .map_err(|e| failed("put", object, e.to_string()))?;
        self.observe(response.status());
        match response.status() {
            reqwest::StatusCode::PRECONDITION_FAILED | reqwest::StatusCode::CONFLICT => {
                Ok(PutOutcome::Conflict)
            }
            status if status.is_success() => Ok(PutOutcome::Written {
                version: version_of(&response),
            }),
            status => {
                let text = response.text().await.unwrap_or_default();
                Err(failed(
                    "put",
                    object,
                    format!(
                        "HTTP {status}: {}",
                        text.chars().take(300).collect::<String>()
                    ),
                ))
            }
        }
    }

    async fn list(&self, _prefix: &str) -> StorageResult<Vec<String>> {
        Err(failed("list", "", "a writer's grant cannot list the store"))
    }

    async fn delete(&self, object: &str) -> StorageResult<()> {
        Err(failed("delete", object, "a writer's grant cannot delete"))
    }
}
