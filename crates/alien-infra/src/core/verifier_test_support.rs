//! A protected install for controller tests: a signed release of a stack, and the real
//! verifier with the install's settings and ledger in memory and a backend that records the
//! functions it would write. A controller test sends the verifier request it captured through
//! `run_verifier`, so it proves the verifier accepts what the controller sends, not only that
//! the controller sent something.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use alien_core::release_statement::{release_body, ReleaseOptions};
use alien_core::{Platform, ReleaseStatements, Stack};
use alien_signing::envelope::Envelope;
use alien_signing::{
    fingerprint, InstallId, Kind, Ledger, Lookup, Role, SecretKey, SignedStatement, Signer,
    SignerKind, SignerListBody, SigningSettings, Workload,
};
use alien_verifier::function::StillNewest;
use alien_verifier::plan::Place;
use alien_verifier::trust::InstallStore;
use alien_verifier::{Backend, FunctionSpec, Verifier};
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
    SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: PROJECT.to_string(),
            install: None,
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
    let body = release_body(
        platform,
        stack,
        &ReleaseOptions {
            channel: CHANNEL.to_string(),
            ..Default::default()
        },
    )
    .expect("the stack makes a release");
    ReleaseStatements {
        release: statement(Kind::Release, sequence, body),
        signer_lists: vec![signer_list()],
    }
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

/// A backend that records each function the verifier writes.
#[derive(Default)]
struct Recorded(Mutex<Vec<FunctionSpec>>);

#[async_trait]
impl Backend for Recorded {
    async fn binding_reads(
        &self,
        _place: &Place,
        _workload: &Workload,
    ) -> alien_verifier::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    async fn apply(
        &self,
        spec: &FunctionSpec,
        newest: &dyn StillNewest,
    ) -> alien_verifier::Result<()> {
        newest.check().await?;
        self.0.lock().unwrap().push(spec.clone());
        Ok(())
    }

    async fn check(&self, _spec: &FunctionSpec) -> alien_verifier::Result<()> {
        unreachable!("applyRelease writes; it reads back through apply")
    }
}

/// Run the verifier of the install `install` with prefix `prefix` on the request `payload`,
/// as the cloud would deliver it. Its answer, or its reason for refusing, and the functions
/// it wrote.
pub async fn run_verifier(
    install: InstallId,
    prefix: &str,
    payload: &[u8],
) -> (Result<serde_json::Value, String>, Vec<FunctionSpec>) {
    let backend = Arc::new(Recorded::default());
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
    let written = backend.0.lock().unwrap().clone();
    (answer, written)
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
