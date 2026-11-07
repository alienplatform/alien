//! Refresh bounded runtime credentials without retaining expired authority.

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use alien_error::AlienError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{io::AsyncReadExt, sync::Mutex};

use crate::{ErrorData, Result};

/// How far apart this process's clock and the manager's clock may be when we
/// check the lifetime of a credential the manager issued. Without it, a
/// manager clock one second ahead makes a one hour credential look too long
/// and locks the caller out. AWS uses the same 5 minute tolerance for SigV4.
pub const ALLOWED_CLOCK_SKEW: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// Longest lifetime of a registry pull credential. Pods pull images with it
/// from a Kubernetes Secret that the operator refreshes, so it may last as
/// long as the operator's daily key. Pods then still start while the operator
/// is down. The manager caps it at the key's own expiry.
pub const REGISTRY_PULL_LIFETIME: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// Every purpose other than registry pull lasts at most one hour.
fn max_lifetime(purpose: &Value) -> chrono::TimeDelta {
    if purpose.get("type").and_then(Value::as_str) == Some("registryPull") {
        REGISTRY_PULL_LIFETIME
    } else {
        chrono::TimeDelta::hours(1)
    }
}

/// Supplies fresh identity evidence. Implementations must not return a runtime
/// capability as proof or fall back to an unrelated stored credential.
#[async_trait]
pub trait RuntimeProofProvider: Send + Sync {
    async fn proof(&self, audience: &str) -> Result<Value>;
}

/// A bounded credential suitable for an atomically replaced private file.
/// It deliberately has no Debug implementation.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCapability {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

enum CredentialSource {
    Exchange(Arc<dyn RuntimeProofProvider>),
    File(PathBuf),
}

#[derive(Default)]
struct CredentialState {
    cached: Option<CachedCredential>,
    rejected: Option<String>,
}

struct CachedCredential {
    credential: RuntimeCapability,
    refresh_at: DateTime<Utc>,
}

/// Nonsecret configuration supplied by deployment provisioning.
pub struct RuntimeCredentialConfig {
    pub manager_url: String,
    /// Exact configured manager identity, independent of its transport URL.
    pub audience: String,
    pub deployment_id: String,
    pub purpose: Value,
}

/// A single-purpose credential source shared by concurrent requests.
/// Refresh errors propagate; expired credentials are never returned.
pub struct RuntimeCredentials {
    client: Client,
    endpoint: Url,
    receiver: Url,
    audience: String,
    deployment_id: String,
    purpose: Value,
    source: CredentialSource,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    state: Mutex<CredentialState>,
}

impl fmt::Debug for RuntimeCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeCredentials")
            .finish_non_exhaustive()
    }
}

impl RuntimeCredentials {
    /// Connect to a manager using a fixed deployment and purpose. The manager
    /// independently verifies membership and never trusts these selectors.
    pub fn new(
        config: RuntimeCredentialConfig,
        proof: Arc<dyn RuntimeProofProvider>,
    ) -> Result<Self> {
        Self::with_source(config, CredentialSource::Exchange(proof))
    }

    /// Exchange proof at a configured manager path for a credential restricted
    /// to an independently configured receiver. Neither destination is selected
    /// by the proof provider or an exchange response.
    pub fn new_for_receiver(
        config: RuntimeCredentialConfig,
        proof: Arc<dyn RuntimeProofProvider>,
        exchange_path: &str,
        receiver_url: &str,
    ) -> Result<Self> {
        let mut credentials = Self::new(config, proof)?;
        if !exchange_path.starts_with('/') || exchange_path.starts_with("//") {
            return Err(invalid("exchange path must be an absolute manager path"));
        }
        let endpoint = credentials
            .endpoint
            .join(exchange_path)
            .map_err(|_| invalid("invalid exchange path"))?;
        if endpoint.origin() != credentials.endpoint.origin()
            || endpoint.path() != exchange_path
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err(invalid(
                "exchange path must remain on the configured manager",
            ));
        }
        credentials.receiver = secure_endpoint(receiver_url)?;
        credentials.endpoint = endpoint;
        Ok(credentials)
    }

    /// Read an externally refreshed capability on each request. This source
    /// never exchanges or renews the supplied credential.
    pub fn from_capability_file(config: RuntimeCredentialConfig, path: PathBuf) -> Result<Self> {
        Self::with_source(config, CredentialSource::File(path))
    }

    fn with_source(config: RuntimeCredentialConfig, source: CredentialSource) -> Result<Self> {
        let manager_url = &config.manager_url;
        let base = secure_endpoint(manager_url)?;
        let endpoint = Url::parse(&format!(
            "{}/v1/identity/exchange",
            manager_url.trim_end_matches('/')
        ))
        .map_err(|_| invalid("invalid exchange URL"))?;
        let client = Client::builder()
            .redirect(Policy::none())
            .http1_only()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| invalid("cannot initialize credential transport"))?;
        Ok(Self {
            client,
            endpoint,
            receiver: base,
            audience: config.audience,
            deployment_id: config.deployment_id,
            purpose: config.purpose,
            source,
            clock: Arc::new(Utc::now),
            state: Mutex::new(CredentialState::default()),
        })
    }

    /// Credentials may only be attached to their configured receiver origin.
    pub fn allows_endpoint(&self, endpoint: &str) -> bool {
        Url::parse(endpoint)
            .map(|url| {
                url.username().is_empty()
                    && url.password().is_none()
                    && url.origin() == self.receiver.origin()
            })
            .unwrap_or(false)
    }

    /// Inject a clock for precise expiry validation.
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }

    /// Return a current credential, refreshing halfway through its remaining
    /// lifetime. A scheduled refresh failure may retain unexpired authority.
    pub async fn token(&self) -> Result<String> {
        Ok(self.capability().await?.token)
    }

    /// Invalidate only the credential rejected by a receiver. A delayed response
    /// for an older token must not evict a newer cached credential or replace
    /// the rejection recorded while that newer generation is being refreshed.
    pub async fn reject(&self, rejected: &str) {
        let mut state = self.state.lock().await;
        if state
            .cached
            .as_ref()
            .is_some_and(|current| current.credential.token == rejected)
        {
            state.cached = None;
            state.rejected = Some(rejected.to_owned());
        }
    }

    /// Retry callers share a replacement when they rejected the same token.
    pub async fn token_after_rejection(&self, rejected: &str) -> Result<String> {
        self.reject(rejected).await;
        self.token().await
    }

    /// The deadline accompanies tokens written to child credential files.
    pub async fn capability(&self) -> Result<RuntimeCapability> {
        let mut state = self.state.lock().await;
        if let CredentialSource::File(path) = &self.source {
            let file = tokio::fs::File::open(path)
                .await
                .map_err(|_| invalid("cannot open capability file"))?;
            let metadata = file
                .metadata()
                .await
                .map_err(|_| invalid("cannot inspect capability file"))?;
            if !metadata.is_file() || metadata.len() > 16 * 1024 {
                return Err(invalid("invalid capability file"));
            }
            #[cfg(unix)]
            {
                let mode = metadata.permissions().mode();
                // Projected Secrets are root-owned and readable by the pod's
                // fsGroup. That group shares the credential trust boundary.
                if mode & 0o037 != 0 || (mode & 0o040 != 0 && metadata.uid() != 0) {
                    return Err(invalid(
                        "capability file must be private or root-owned group-read-only",
                    ));
                }
            }
            let mut bytes = Vec::new();
            file.take(16 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| invalid("cannot read capability file"))?;
            if bytes.len() > 16 * 1024 {
                return Err(invalid("capability file exceeds limit"));
            }
            let capability: RuntimeCapability =
                serde_json::from_slice(&bytes).map_err(|_| invalid("invalid capability file"))?;
            let remaining = capability.expires_at - (self.clock)();
            if !capability.token.starts_with("ax_cap_v1_")
                || remaining <= chrono::Duration::zero()
                || remaining > max_lifetime(&self.purpose) + ALLOWED_CLOCK_SKEW
            {
                return Err(invalid("invalid capability file lifetime"));
            }
            if state.rejected.as_deref() == Some(capability.token.as_str()) {
                return Err(invalid(
                    "capability file still contains a rejected credential",
                ));
            }
            state.cached = Some(CachedCredential {
                credential: capability.clone(),
                refresh_at: capability.expires_at,
            });
            return Ok(capability);
        }
        let now = (self.clock)();
        if let Some(current) = state.cached.as_ref() {
            if now < current.refresh_at && now < current.credential.expires_at {
                return Ok(current.credential.clone());
            }
        }
        match self.exchange().await {
            Ok(credential) => {
                if state.rejected.as_deref() == Some(credential.token.as_str()) {
                    return Err(invalid("identity exchange returned a rejected credential"));
                }
                let now = (self.clock)();
                if now >= credential.expires_at {
                    return Err(invalid("identity exchange credential expired before use"));
                }
                state.cached = Some(CachedCredential {
                    refresh_at: now + (credential.expires_at - now) / 2,
                    credential: credential.clone(),
                });
                Ok(credential)
            }
            Err(error) => {
                // Only scheduled refresh can retain authority. Explicit rejection
                // removed its exact cached token before reaching this branch.
                if let Some(current) = state.cached.as_ref() {
                    if (self.clock)() < current.credential.expires_at {
                        return Ok(current.credential.clone());
                    }
                }
                state.cached = None;
                Err(error)
            }
        }
    }

    async fn exchange(&self) -> Result<RuntimeCapability> {
        let CredentialSource::Exchange(proof) = &self.source else {
            unreachable!()
        };
        let proof = proof.proof(&self.audience).await?;
        let mut response = self.client.post(self.endpoint.clone())
            .json(&json!({"deploymentId": self.deployment_id, "purpose": self.purpose, "proof": proof}))
            .send().await.map_err(|_| invalid("identity exchange transport failed"))?;
        if !response.status().is_success() {
            return Err(invalid(&format!(
                "identity exchange returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| invalid("cannot read identity exchange response"))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 {
                return Err(invalid("identity exchange response exceeds limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        // Never preserve provider/decoder errors which may echo credential data.
        let credential: RuntimeCapability = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid identity exchange response"))?;
        let now = (self.clock)();
        let remaining = credential.expires_at - now;
        if credential.token.is_empty()
            || remaining <= chrono::Duration::zero()
            || remaining > max_lifetime(&self.purpose) + ALLOWED_CLOCK_SKEW
        {
            return Err(invalid(
                "identity exchange returned an invalid credential lifetime",
            ));
        }
        Ok(credential)
    }
}

fn secure_endpoint(endpoint: &str) -> Result<Url> {
    let url = Url::parse(endpoint).map_err(|_| invalid("invalid credential endpoint"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err(invalid(
            "credential endpoint must use HTTPS (HTTP is allowed only on loopback)",
        ));
    }
    Ok(url)
}

fn invalid(message: &str) -> crate::Error {
    AlienError::new(ErrorData::AuthenticationError {
        message: message.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::{AtomicI64, AtomicUsize, Ordering},
        thread,
    };

    struct Proof(AtomicUsize);
    #[async_trait]
    impl RuntimeProofProvider for Proof {
        async fn proof(&self, audience: &str) -> Result<Value> {
            assert_eq!(audience, "manager-identity");
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"type":"session", "token":"fixture-proof"}))
        }
    }

    fn fixture(
        replies: Vec<(u16, Value)>,
    ) -> (
        RuntimeCredentials,
        Arc<AtomicI64>,
        Arc<Proof>,
        thread::JoinHandle<()>,
    ) {
        fixture_for(replies, None, json!({"type":"telemetry"}))
    }

    fn fixture_for_receiver(
        replies: Vec<(u16, Value)>,
        receiver: Option<&str>,
    ) -> (
        RuntimeCredentials,
        Arc<AtomicI64>,
        Arc<Proof>,
        thread::JoinHandle<()>,
    ) {
        fixture_for(replies, receiver, json!({"type":"telemetry"}))
    }

    fn fixture_for(
        replies: Vec<(u16, Value)>,
        receiver: Option<&str>,
        purpose: Value,
    ) -> (
        RuntimeCredentials,
        Arc<AtomicI64>,
        Arc<Proof>,
        thread::JoinHandle<()>,
    ) {
        let expected_purpose = purpose.clone();
        let exchange_path = if receiver.is_some() {
            "/v1/identity/scoped"
        } else {
            "/v1/identity/exchange"
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(header_end) =
                        request.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if request.len() >= header_end + 4 + length {
                            assert!(headers.starts_with(&format!("POST {exchange_path} HTTP/1.1")));
                            let request: Value =
                                serde_json::from_slice(&request[header_end + 4..]).unwrap();
                            assert_eq!(request["deploymentId"], "deployment-one");
                            assert_eq!(request["purpose"], expected_purpose);
                            assert_eq!(request["proof"]["token"], "fixture-proof");
                            break;
                        }
                    }
                }
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let clock = Arc::new(AtomicI64::new(1_800_000_000));
        let proof = Arc::new(Proof(AtomicUsize::new(0)));
        let observed_clock = clock.clone();
        let config = RuntimeCredentialConfig {
            manager_url: url,
            audience: "manager-identity".into(),
            deployment_id: "deployment-one".into(),
            purpose,
        };
        let client = match receiver {
            Some(receiver) => {
                RuntimeCredentials::new_for_receiver(config, proof.clone(), exchange_path, receiver)
            }
            None => RuntimeCredentials::new(config, proof.clone()),
        }
        .unwrap()
        .with_clock(Arc::new(move || {
            DateTime::from_timestamp(observed_clock.load(Ordering::SeqCst), 0).unwrap()
        }));
        (client, clock, proof, server)
    }

    #[tokio::test]
    async fn explicit_receiver_keeps_exchange_and_delivery_origins_separate() {
        let receiver = TcpListener::bind("127.0.0.1:0").unwrap();
        let receiver_url = format!("http://{}", receiver.local_addr().unwrap());
        let unauthorized = TcpListener::bind("127.0.0.1:0").unwrap();
        unauthorized.set_nonblocking(true).unwrap();
        let wrong_url = format!("http://{}", unauthorized.local_addr().unwrap());
        let (client, _, _, exchange) = fixture_for_receiver(
            vec![(200, credential("scoped-child", 100))],
            Some(&receiver_url),
        );
        assert!(!client.allows_endpoint(client.endpoint.as_str()));
        assert!(!client.allows_endpoint(&wrong_url));
        assert!(client.allows_endpoint(&receiver_url));
        let delivery = thread::spawn(move || {
            let (mut stream, _) = receiver.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let count = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]);
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer scoped-child"));
            write!(
                stream,
                "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        let token = client.token().await.unwrap();
        let response = Client::builder()
            .redirect(Policy::none())
            .build()
            .unwrap()
            .get(&receiver_url)
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
        assert_eq!(
            unauthorized.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        exchange.join().unwrap();
        delivery.join().unwrap();
    }

    #[tokio::test]
    async fn explicit_exchange_refuses_redirect_to_another_listener() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let destination_url = format!("http://{}", destination.local_addr().unwrap());
        let manager = TcpListener::bind("127.0.0.1:0").unwrap();
        let manager_url = format!("http://{}", manager.local_addr().unwrap());
        let redirect = destination_url.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = manager.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = RuntimeCredentials::new_for_receiver(
            RuntimeCredentialConfig {
                manager_url,
                audience: "manager-identity".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            Arc::new(Proof(AtomicUsize::new(0))),
            "/v1/identity/scoped",
            &destination_url,
        )
        .unwrap();
        assert!(client.token().await.is_err());
        server.join().unwrap();
        assert_eq!(
            destination.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn explicit_destinations_refuse_ambiguous_or_insecure_configuration() {
        for (path, receiver) in [
            ("//other.example/exchange", "https://receiver.example"),
            ("https://other.example/exchange", "https://receiver.example"),
            ("/exchange?next=other", "https://receiver.example"),
            ("/exchange#other", "https://receiver.example"),
            ("/a/../exchange", "https://receiver.example"),
            ("/exchange", "http://receiver.example"),
            ("/exchange", "https://user:pass@receiver.example"),
            ("/exchange", "https://receiver.example?secret=value"),
        ] {
            let result = RuntimeCredentials::new_for_receiver(
                RuntimeCredentialConfig {
                    manager_url: "https://manager.example".into(),
                    audience: "manager-identity".into(),
                    deployment_id: "deployment-one".into(),
                    purpose: json!({"type":"telemetry"}),
                },
                Arc::new(Proof(AtomicUsize::new(0))),
                path,
                receiver,
            );
            assert!(result.is_err(), "unexpected accepted destination");
        }
    }

    fn credential(token: &str, seconds: i64) -> Value {
        json!({"token": token, "expiresAt": DateTime::from_timestamp(1_800_000_000 + seconds, 0).unwrap()})
    }

    #[tokio::test]
    async fn concurrent_requests_share_refresh_and_refresh_before_expiry() {
        let (client, clock, proof, server) = fixture(vec![
            (200, credential("first", 100)),
            (200, credential("second", 200)),
        ]);
        let (one, two) = tokio::join!(client.token(), client.token());
        assert_eq!(one.unwrap(), "first");
        assert_eq!(two.unwrap(), "first");
        assert_eq!(proof.0.load(Ordering::SeqCst), 1);
        clock.store(1_800_000_049, Ordering::SeqCst);
        assert_eq!(client.token().await.unwrap(), "first");
        clock.store(1_800_000_050, Ordering::SeqCst);
        assert_eq!(client.token().await.unwrap(), "second");
        assert_eq!(proof.0.load(Ordering::SeqCst), 2);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn early_refresh_outage_retains_only_unexpired_nonrejected_token() {
        let (client, clock, _, server) = fixture(vec![
            (200, credential("first", 100)),
            (503, json!({})),
            (503, json!({})),
        ]);
        assert_eq!(client.token().await.unwrap(), "first");
        clock.store(1_800_000_050, Ordering::SeqCst);
        assert_eq!(client.token().await.unwrap(), "first");
        clock.store(1_800_000_100, Ordering::SeqCst);
        assert!(client.token().await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn refresh_failure_rechecks_expiry_after_io() {
        struct FailingProof(Arc<AtomicI64>);
        #[async_trait]
        impl RuntimeProofProvider for FailingProof {
            async fn proof(&self, _: &str) -> Result<Value> {
                self.0.store(1_800_000_100, Ordering::SeqCst);
                Err(invalid("fixture provider unavailable"))
            }
        }
        let (mut client, clock, _, server) = fixture(vec![(200, credential("first", 100))]);
        assert_eq!(client.token().await.unwrap(), "first");
        server.join().unwrap();
        clock.store(1_800_000_050, Ordering::SeqCst);
        client.source = CredentialSource::Exchange(Arc::new(FailingProof(clock)));
        assert!(client.token().await.is_err());
    }

    #[tokio::test]
    async fn concurrent_rejections_preserve_the_replacement() {
        let (client, _, proof, server) = fixture(vec![
            (200, credential("first", 100)),
            (200, credential("second", 100)),
        ]);
        assert_eq!(client.token().await.unwrap(), "first");
        let (one, two) = tokio::join!(
            client.token_after_rejection("first"),
            client.token_after_rejection("first")
        );
        assert_eq!(one.unwrap(), "second");
        assert_eq!(two.unwrap(), "second");
        assert_eq!(proof.0.load(Ordering::SeqCst), 2);
        client.reject("first").await;
        assert_eq!(client.token().await.unwrap(), "second");
        assert_eq!(proof.0.load(Ordering::SeqCst), 2);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn stale_rejection_cannot_replace_newer_rejection_during_refresh() {
        let (client, _, _, server) = fixture(vec![
            (200, credential("first", 100)),
            (200, credential("second", 100)),
            (200, credential("second", 100)),
            (200, credential("third", 100)),
        ]);
        assert_eq!(client.token().await.unwrap(), "first");
        assert_eq!(
            client.token_after_rejection("first").await.unwrap(),
            "second"
        );
        client.reject("second").await;
        client.reject("first").await;
        assert!(
            client.token().await.is_err(),
            "late first rejection restored rejected second credential"
        );
        assert_eq!(client.token().await.unwrap(), "third");
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_rejection_cannot_restore_rejected_projected_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credential");
        let write = |token: &str| {
            std::fs::write(
                &path,
                serde_json::to_vec(&RuntimeCapability {
                    token: token.into(),
                    expires_at: Utc::now() + chrono::Duration::minutes(30),
                })
                .unwrap(),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        };
        let client = RuntimeCredentials::from_capability_file(
            RuntimeCredentialConfig {
                manager_url: "https://manager.example".into(),
                audience: "manager-one".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            path.clone(),
        )
        .unwrap();
        write("ax_cap_v1_first");
        assert_eq!(client.token().await.unwrap(), "ax_cap_v1_first");
        client.reject("ax_cap_v1_first").await;
        write("ax_cap_v1_second");
        assert_eq!(client.token().await.unwrap(), "ax_cap_v1_second");
        client.reject("ax_cap_v1_second").await;
        client.reject("ax_cap_v1_first").await;
        assert!(
            client.token().await.is_err(),
            "late first rejection restored rejected file bytes"
        );
        write("ax_cap_v1_third");
        assert_eq!(client.token().await.unwrap(), "ax_cap_v1_third");
    }

    #[tokio::test]
    async fn rejected_refresh_does_not_restore_cached_or_expired_authority() {
        let (client, _, _, server) = fixture(vec![
            (200, credential("first", 100)),
            (401, json!({"secret":"must-not-appear"})),
            (200, credential("replacement", 200)),
        ]);
        assert_eq!(client.token().await.unwrap(), "first");
        let error = client.token_after_rejection("first").await.unwrap_err();
        assert!(!format!("{error:?}").contains("must-not-appear"));
        assert_eq!(client.token().await.unwrap(), "replacement");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn exchange_cannot_restore_the_rejected_token() {
        let (client, _, _, server) = fixture(vec![
            (200, credential("first", 100)),
            (200, credential("first", 100)),
            (200, credential("replacement", 100)),
        ]);
        assert_eq!(client.token().await.unwrap(), "first");
        assert!(client.token_after_rejection("first").await.is_err());
        assert_eq!(client.token().await.unwrap(), "replacement");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn refuses_expired_and_overlong_responses() {
        let (client, _, _, server) = fixture(vec![
            (200, credential("expired", 0)),
            (200, credential("overlong", 3600 + 301)),
        ]);
        assert!(client.token().await.is_err());
        assert!(client.token().await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn credential_from_a_manager_clock_two_minutes_ahead_is_accepted() {
        // The manager issued a one hour credential by its own clock, which
        // runs two minutes ahead of ours.
        let (client, _, _, server) = fixture(vec![(200, credential("ahead", 3600 + 120))]);
        assert_eq!(client.token().await.unwrap(), "ahead");
        server.join().unwrap();
    }
    /// The operator writes the pull credential into a Secret that pods read
    /// while the operator may be down. It accepts one that lasts as long as
    /// the daily key, and still refuses longer ones or long credentials for
    /// any other purpose.
    #[tokio::test]
    async fn registry_pull_credential_may_last_as_long_as_the_daily_key() {
        let day = 24 * 3600;
        let (client, _, _, server) = fixture_for(
            vec![
                (200, credential("pull-day", day)),
                (200, credential("pull-overlong", day + 301)),
            ],
            None,
            json!({"type":"registryPull"}),
        );
        assert_eq!(client.token().await.unwrap(), "pull-day");
        client.reject("pull-day").await;
        assert!(client.token().await.is_err());
        server.join().unwrap();

        let (client, _, _, server) = fixture(vec![(200, credential("telemetry-day", day))]);
        assert!(client.token().await.is_err());
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn capability_file_reopens_replacement_and_refuses_exact_expiry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("telemetry");
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(1_800_000_000));
        let write = |name: &str, token: &str, expires: i64| {
            let file = directory.path().join(name);
            let capability = RuntimeCapability {
                token: token.into(),
                expires_at: DateTime::from_timestamp(expires, 0).unwrap(),
            };
            std::fs::write(&file, serde_json::to_vec(&capability).unwrap()).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            file
        };
        write("telemetry", "legacy-deployment-token", 1_800_000_100);
        let observed = clock.clone();
        let credentials = RuntimeCredentials::from_capability_file(
            RuntimeCredentialConfig {
                manager_url: "https://manager.example".into(),
                audience: "manager-one".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type":"telemetry"}),
            },
            path.clone(),
        )
        .unwrap()
        .with_clock(Arc::new(move || {
            DateTime::from_timestamp(observed.load(std::sync::atomic::Ordering::SeqCst), 0).unwrap()
        }));
        assert!(
            credentials.token().await.is_err(),
            "legacy token must not enter capability-file mode"
        );
        write("telemetry", "ax_cap_v1_first", 1_800_000_100);
        assert_eq!(credentials.token().await.unwrap(), "ax_cap_v1_first");
        credentials.reject("ax_cap_v1_first").await;
        assert!(credentials.token().await.is_err());
        clock.store(1_800_000_100, std::sync::atomic::Ordering::SeqCst);
        assert!(credentials.token().await.is_err());
        let next = write("next", "ax_cap_v1_second", 1_800_000_200);
        std::fs::rename(next, &path).unwrap();
        assert_eq!(credentials.token().await.unwrap(), "ax_cap_v1_second");
        credentials.reject("ax_cap_v1_first").await;
        assert_eq!(credentials.token().await.unwrap(), "ax_cap_v1_second");
        for mode in [0o644, 0o620, 0o610, 0o601] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(credentials.token().await.is_err());
        }
        if std::fs::metadata(&path).unwrap().uid() != 0 {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
            assert!(credentials.token().await.is_err());
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(credentials.token().await.unwrap(), "ax_cap_v1_second");
        std::fs::remove_file(&path).unwrap();
        assert!(credentials.token().await.is_err());
    }
}
