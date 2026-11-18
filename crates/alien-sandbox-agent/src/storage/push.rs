//! The push transport: the agent writes its key's store directly with the grant the binding
//! minted (S3: a POST policy for `<key>/`, a presigned PUT for `<key>/HEAD`, presigned GETs for
//! the restore; Azure: a SAS for the key's directory). The agent holds no cloud identity, and the
//! grant reaches nothing but one key.
//!
//! Its sockets are the agent's own: commands run in the guest cgroup, whose sockets carry the
//! guest mark and are dropped on the way to the store.

use std::sync::RwLock;

use alien_error::AlienError;
use alien_sandbox_storage::{
    AzureBlobStore, BlobResponse, BlobTransport, ErrorData as StorageError, ObjectStore, ObjectVersion, Precondition,
    PutOutcome, Result as StorageResult, StorageGrant, StoredObject, HEAD_OBJECT,
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
    http: super::http::TrustedHttp,
    /// Set when the grant is an Azure SAS: every request goes through it (on the same trusted
    /// sockets), and a renewal swaps its SAS.
    azure: Option<AzureBlobStore>,
    /// Set when the store refused the grant (403) and cleared by a renewal.
    denied: std::sync::atomic::AtomicBool,
}

impl GrantStore {
    /// A store using `grant`. Refused when an Azure grant's key URL is not an HTTPS directory URL.
    pub fn new(grant: StorageGrant) -> StorageResult<Self> {
        let http = super::http::TrustedHttp::new(std::time::Duration::from_secs(60));
        let azure = match &grant {
            StorageGrant::S3 { .. } => None,
            StorageGrant::AzureSas { key_url, sas, .. } => Some(AzureBlobStore::with_transport(
                key_url,
                &sas.0,
                std::sync::Arc::new(TrustedBlobTransport(super::http::TrustedHttp::new(
                    // A pack is up to MAX_PACK_BYTES; give its upload more than a HEAD's minute.
                    std::time::Duration::from_secs(300),
                ))),
            )?),
        };
        Ok(Self {
            grant: RwLock::new(grant),
            http,
            azure,
            denied: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Whether the grant is expired or the store refused it: nothing more can be written until
    /// the binding renews it.
    pub fn denied(&self) -> bool {
        self.denied.load(std::sync::atomic::Ordering::SeqCst)
            || self.azure.as_ref().is_some_and(AzureBlobStore::denied)
            || self.grant().expires_at() <= chrono::Utc::now()
    }

    /// Marks the grant denied when the store refused it: 403, or the 400 S3 gives a POST whose
    /// policy expired (a clock behind the store's can let one through the local check).
    fn observe(&self, status: hyper::StatusCode, body: &[u8]) {
        if status == hyper::StatusCode::FORBIDDEN
            || (status == hyper::StatusCode::BAD_REQUEST
                && String::from_utf8_lossy(body).contains("Policy expired"))
        {
            self.denied.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Replaces the grant (renewal before expiry). A renewal must reach the same key the same
    /// way: one for another cloud or another Azure directory is refused and the old grant kept.
    pub fn renew(&self, grant: StorageGrant) -> StorageResult<()> {
        let mut current = self.grant.write().expect("not poisoned");
        match (&*current, &grant, &self.azure) {
            (StorageGrant::S3 { .. }, StorageGrant::S3 { .. }, None) => {}
            (
                StorageGrant::AzureSas { key_url: held, .. },
                StorageGrant::AzureSas { key_url, sas, .. },
                Some(azure),
            ) if held == key_url => azure.renew(&sas.0),
            _ => {
                return Err(failed(
                    "renew",
                    "",
                    "the renewed grant does not reach the attached key the same way",
                ))
            }
        }
        *current = grant;
        self.denied.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn grant(&self) -> StorageGrant {
        self.grant.read().expect("not poisoned").clone()
    }

    fn read_url(&self, object: &str) -> Option<String> {
        match self.grant() {
            StorageGrant::S3 { reads, .. } => reads.get(object).cloned(),
            StorageGrant::AzureSas { .. } => None,
        }
    }
}

/// Azure requests on the agent's trusted sockets, which the guest drop rule lets through.
struct TrustedBlobTransport(super::http::TrustedHttp);

#[async_trait]
impl BlobTransport for TrustedBlobTransport {
    async fn send(
        &self,
        method: hyper::Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> std::io::Result<BlobResponse> {
        let response = self.0.send(method, url, headers, body).await?;
        Ok(BlobResponse {
            status: response.status,
            headers: response.headers,
            body: response.body.to_vec(),
        })
    }
}

fn version_of(response: &super::http::Response) -> ObjectVersion {
    ObjectVersion::etag(
        response
            .headers
            .get(hyper::header::ETAG)
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
        if let Some(azure) = &self.azure {
            return azure.get(object).await;
        }
        let url = self
            .read_url(object)
            .ok_or_else(|| failed("get", object, "the grant does not cover this object"))?;
        let response = self
            .http
            .send(hyper::Method::GET, &url, &[], Vec::new())
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        if response.status == hyper::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status.is_success() {
            return Err(failed("get", object, format!("HTTP {}", response.status)));
        }
        let version = version_of(&response);
        Ok(Some(StoredObject {
            bytes: response.body.to_vec(),
            version,
        }))
    }

    async fn get_range(&self, object: &str, offset: u64, len: u64) -> StorageResult<Vec<u8>> {
        if let Some(azure) = &self.azure {
            return azure.get_range(object, offset, len).await;
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        let url = self
            .read_url(object)
            .ok_or_else(|| failed("get", object, "the grant does not cover this object"))?;
        let range = format!("bytes={offset}-{}", offset + len - 1);
        let response = self
            .http
            .send(hyper::Method::GET, &url, &[("range", range)], Vec::new())
            .await
            .map_err(|e| failed("get", object, e.to_string()))?;
        if !response.status.is_success() {
            return Err(failed("get", object, format!("HTTP {}", response.status)));
        }
        Ok(response.body.to_vec())
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
        if let Some(azure) = &self.azure {
            return azure.put_if(object, bytes, precondition).await;
        }
        let StorageGrant::S3 {
            post, head_put_url, ..
        } = grant
        else {
            return Err(failed("put", object, "the grant is not an S3 grant"));
        };
        let response = if object == HEAD_OBJECT {
            let condition = match &precondition {
                Precondition::Matches(version) => ("if-match", version.etag.clone()),
                Precondition::Absent => ("if-none-match", "*".to_string()),
            };
            self.http
                .send(hyper::Method::PUT, &head_put_url, &[condition], bytes)
                .await
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
                .send(
                    hyper::Method::POST,
                    &post.url,
                    &[("content-type", content_type)],
                    body,
                )
                .await
        }
        .map_err(|e| failed("put", object, e.to_string()))?;
        self.observe(response.status, &response.body);
        match response.status {
            hyper::StatusCode::PRECONDITION_FAILED | hyper::StatusCode::CONFLICT => {
                Ok(PutOutcome::Conflict)
            }
            status if status.is_success() => Ok(PutOutcome::Written {
                version: version_of(&response),
            }),
            status => {
                let text = String::from_utf8_lossy(&response.body);
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

#[cfg(test)]
mod azure_grant_tests {
    use super::*;
    use alien_sandbox_storage::{S3PostPolicy, SasToken};
    use chrono::{TimeZone, Utc};

    const KEY_URL: &str = "https://acmeagents1a2b3c4d.blob.core.windows.net/keys/engine-42/";

    fn azure(key_url: &str, sas: &str) -> StorageGrant {
        StorageGrant::AzureSas {
            key_url: key_url.to_string(),
            sas: SasToken(sas.to_string()),
            expires_at: Utc.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap(),
        }
    }

    /// The grant as the binding sends it on the agent channel.
    #[test]
    fn an_azure_grant_reads_from_the_wire() {
        let grant: StorageGrant = serde_json::from_value(serde_json::json!({
            "type": "azureSas",
            "keyUrl": KEY_URL,
            "sas": "sv=2026-10-06&sr=d&sig=secret",
            "expiresAt": "2026-11-18T12:00:00Z",
        }))
        .expect("the grant reads");
        assert_eq!(grant, azure(KEY_URL, "sv=2026-10-06&sr=d&sig=secret"));
    }

    /// The SAS is a credential: a grant printed for a log or an error shows no part of it.
    #[test]
    fn the_sas_is_never_printed() {
        let printed = format!("{:?}", azure(KEY_URL, "sv=2026-10-06&sr=d&sig=secret"));
        assert!(
            !printed.contains("secret") && !printed.contains("sr=d"),
            "{printed}"
        );
        let store = GrantStore::new(azure(KEY_URL, "sig=secret")).expect("the store builds");
        assert!(!format!("{:?}", store.azure).contains("secret"));
    }

    /// A key URL the transport cannot address safely is refused at attach, not at the first push.
    #[test]
    fn an_azure_grant_must_name_an_https_directory() {
        for key_url in [
            "http://acme.blob.core.windows.net/keys/engine-42/",
            "https://acme.blob.core.windows.net/keys/engine-42",
            "https://acme.blob.core.windows.net/keys/engine-42/?sig=x",
        ] {
            assert!(
                GrantStore::new(azure(key_url, "sig=x")).is_err(),
                "{key_url}"
            );
        }
    }

    /// A renewal reaches the same key the same way, or it is refused and the old grant stays.
    #[test]
    fn a_renewal_cannot_move_the_store() {
        let store = GrantStore::new(azure(KEY_URL, "sig=first")).expect("the store builds");
        store
            .renew(azure(KEY_URL, "sig=second"))
            .expect("same directory renews");
        assert_eq!(store.grant(), azure(KEY_URL, "sig=second"));

        let other_key = "https://acmeagents1a2b3c4d.blob.core.windows.net/keys/engine-43/";
        assert!(store.renew(azure(other_key, "sig=third")).is_err());
        let s3 = StorageGrant::S3 {
            post: S3PostPolicy {
                url: "https://bucket.s3.amazonaws.com/".to_string(),
                fields: Default::default(),
                key_prefix: "engine-42/".to_string(),
            },
            head_put_url: "https://bucket.s3.amazonaws.com/engine-42/HEAD".to_string(),
            reads: Default::default(),
            expires_at: Utc.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap(),
        };
        assert!(store.renew(s3).is_err());
        assert_eq!(store.grant(), azure(KEY_URL, "sig=second"));
    }
}
