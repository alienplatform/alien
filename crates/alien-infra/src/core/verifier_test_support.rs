//! A protected install for controller tests: a signed release of a stack, and the real
//! verifier with the install's settings and ledger in memory, its real field tables, and a
//! backend that records the writes it would send. A controller test sends the verifier request
//! it captured through `run_write`, so it proves the verifier accepts what the controller sends,
//! not only that the controller sent something.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use alien_core::release_statement::{release_body, ReleaseOptions};
use alien_core::{Platform, ReleaseStatements, Stack};
use alien_signing::envelope::Envelope;
use alien_signing::{
    fingerprint, InstallId, Kind, Ledger, Lookup, Role, SecretKey, SignedStatement, Signer,
    SignerKind, SignerListBody, SigningSettings, Workload,
};
use alien_verifier::cloud_write::{CloudWrite, Facts};
use alien_verifier::function::StillNewest;
use alien_verifier::plan::Place;
use alien_verifier::trust::InstallStore;
use alien_verifier::{Backend, Verifier};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde::Serialize;

pub const PROJECT: &str = "prj_test";
pub const CHANNEL: &str = "production";

fn key() -> SecretKey {
    let mut secret = [7u8; 32];
    secret[0] = 1;
    SecretKey::from_secret_bytes(&secret).expect("a fixed secret is a valid scalar")
}

fn statement<B: Serialize>(kind: Kind, sequence: u64, body: B) -> SignedStatement {
    statement_for(None, kind, sequence, body)
}

/// A statement signed by the CI key, for `install` when it is install-bound.
fn statement_for<B: Serialize>(
    install: Option<&InstallId>,
    kind: Kind,
    sequence: u64,
    body: B,
) -> SignedStatement {
    SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: PROJECT.to_string(),
            install: install.map(ToString::to_string),
            sequence,
            not_after: (kind == Kind::SignerList).then(|| Utc::now() + Duration::days(30)),
            signer_list_version: 1,
            body,
        }
        .to_payload()
        .expect("the envelope serializes"),
    )
    .sign_with("ci", &key())
    .expect("signing works")
}

/// The project's signer list: one CI key that administers and releases. Made once, so every
/// copy has the digest the settings pin.
pub fn signer_list() -> SignedStatement {
    static LIST: OnceLock<SignedStatement> = OnceLock::new();
    LIST.get_or_init(new_signer_list).clone()
}

fn new_signer_list() -> SignedStatement {
    statement(
        Kind::SignerList,
        1,
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: key().public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Admin, Role::Release],
                label: None,
            }],
            deny: Vec::new(),
            recovered: false,
        },
    )
}

/// Release `sequence` of `stack` for `platform`, signed, with the signer list.
pub fn signed_release(platform: Platform, stack: &Stack, sequence: u64) -> ReleaseStatements {
    signed_release_configuring(platform, stack, sequence, &[])
}

/// [`signed_release`], letting the install's signed config set the env names `configurable`.
pub fn signed_release_configuring(
    platform: Platform,
    stack: &Stack,
    sequence: u64,
    configurable: &[&str],
) -> ReleaseStatements {
    let body = release_body(
        platform,
        stack,
        &ReleaseOptions {
            channel: CHANNEL.to_string(),
            configurable_env: configurable.iter().map(|name| name.to_string()).collect(),
            ..Default::default()
        },
    )
    .expect("the stack makes a release");
    ReleaseStatements {
        release: statement(Kind::Release, sequence, body),
        signer_lists: vec![signer_list()],
    }
}

/// The signed config of `install` with environment values `env`, at `sequence`.
pub fn signed_config(install: &InstallId, env: &[(&str, &str)], sequence: u64) -> SignedStatement {
    statement_for(
        Some(install),
        Kind::Config,
        sequence,
        alien_signing::ConfigBody {
            env: env
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            ..Default::default()
        },
    )
}

/// The signed config of `install` that sends telemetry to `endpoint`, at `sequence`.
pub fn signed_telemetry_config(
    install: &InstallId,
    endpoint: &str,
    sequence: u64,
) -> SignedStatement {
    statement_for(
        Some(install),
        Kind::Config,
        sequence,
        alien_signing::ConfigBody {
            telemetry_endpoint: Some(endpoint.to_string()),
            ..Default::default()
        },
    )
}

/// The settings setup writes for `prefix`, pinning the signer list's fingerprint.
pub fn settings(prefix: &str) -> SigningSettings {
    SigningSettings {
        fingerprint: fingerprint(&signer_list().open().expect("the list opens").digest),
        prefix: prefix.to_string(),
        channel: CHANNEL.to_string(),
    }
}

/// An install's settings and ledger, in memory.
struct MemoryInstall {
    settings: SigningSettings,
    ledger: Mutex<Ledger>,
}

#[async_trait]
impl InstallStore for MemoryInstall {
    async fn settings(&self, prefix: &str) -> alien_verifier::Result<Lookup> {
        Ok(if prefix == self.settings.prefix {
            Lookup::Found(self.settings.to_json().into_bytes())
        } else {
            Lookup::Absent
        })
    }

    async fn ledger(&self, prefix: &str) -> alien_verifier::Result<Lookup> {
        Ok(if prefix == self.settings.prefix {
            Lookup::Found(self.ledger.lock().unwrap().to_json().into_bytes())
        } else {
            Lookup::Absent
        })
    }

    async fn store_ledger(&self, _prefix: &str, ledger: &Ledger) -> alien_verifier::Result<()> {
        *self.ledger.lock().unwrap() = ledger.clone();
        Ok(())
    }

    async fn input(&self, _prefix: &str, _name: &str) -> alien_verifier::Result<Option<String>> {
        Ok(None)
    }
}

/// A backend that checks each write with its cloud's table and records what it would send.
struct Forwarded {
    install: InstallId,
    /// The Container Apps environment setup names (Azure).
    environment: Option<String>,
    writes: Mutex<Vec<CloudWrite>>,
}

#[async_trait]
impl Backend for Forwarded {
    async fn binding_reads(
        &self,
        _place: &Place,
        _workload: &Workload,
    ) -> alien_verifier::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> alien_verifier::Result<()> {
        match self.install {
            InstallId::Aws { .. } => alien_verifier::cloud_write::lambda::check_write(facts, write),
            InstallId::Gcp { .. } => {
                alien_verifier::cloud_write::cloud_run::check_write(facts, write)
            }
            _ => alien_verifier::cloud_write::container_apps::check_write(
                facts,
                write,
                self.environment.as_deref(),
            ),
        }
    }

    async fn forward(
        &self,
        _facts: &Facts<'_>,
        write: &CloudWrite,
        newest: &dyn StillNewest,
    ) -> alien_verifier::Result<serde_json::Value> {
        newest.check().await?;
        self.writes.lock().unwrap().push(write.clone());
        Ok(serde_json::Value::Null)
    }
}

/// Run the verifier of the install `install` with prefix `prefix` on the `applyWrite` request
/// `payload`, as the cloud would deliver it, with the real field tables. Its answer, or its
/// reason for refusing, and the writes it would send. `environment` is the Container Apps
/// environment setup names (Azure).
pub async fn run_write(
    install: InstallId,
    prefix: &str,
    environment: Option<&str>,
    payload: &[u8],
) -> (Result<serde_json::Value, String>, Vec<CloudWrite>) {
    let backend = Arc::new(Forwarded {
        install: install.clone(),
        environment: environment.map(str::to_string),
        writes: Mutex::new(Vec::new()),
    });
    let verifier = Verifier {
        own_name: format!("{prefix}-verifier"),
        install_id: Arc::new(move |_| install.clone()),
        store: Arc::new(MemoryInstall {
            settings: settings(prefix),
            ledger: Mutex::new(Ledger::new(prefix)),
        }),
        backend: backend.clone(),
        vpc: None,
        machine_network: None,
    };
    let request = serde_json::from_slice(payload).expect("the request is JSON");
    let answer = verifier
        .handle(request)
        .await
        .map_err(|error| alien_verifier::describe(&error));
    let writes = backend.writes.lock().unwrap().clone();
    (answer, writes)
}

/// What an HTTP verifier stub received: each request's bearer token and body.
pub type HttpRequests = Arc<Mutex<Vec<(Option<String>, Vec<u8>)>>>;

/// An HTTP verifier (Cloud Run, Container Apps) that records each `POST /` and answers with
/// `status` and `answer`. Its base URL and what it received.
pub async fn http_verifier_stub(
    status: axum::http::StatusCode,
    answer: serde_json::Value,
) -> (String, HttpRequests) {
    let requests: HttpRequests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let recorded = recorded.clone();
                let answer = answer.clone();
                async move {
                    let bearer = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    recorded.lock().unwrap().push((bearer, body.to_vec()));
                    (status, axum::Json(answer))
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, requests)
}
