//! Signed statements for the verifier's tests and its end-to-end fixtures.

#![allow(missing_docs)]

use std::collections::BTreeMap;

use alien_signing::envelope::Envelope;
use alien_signing::release::{Bounds, Configurable, FunctionSettings, Runtime, WorkloadKind};
use alien_signing::{
    fingerprint, ConfigBody, Kind, ReleaseBody, Role, SecretKey, SignedStatement, Signer,
    SignerKind, SignerListBody, SigningSettings, Workload,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Serialize;

pub const PROJECT: &str = "prj_shop";
pub const PREFIX: &str = "acme-prod";
pub const ACCOUNT: &str = "111122223333";
pub const REGION: &str = "us-east-1";
pub const CHANNEL: &str = "production";
pub const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
pub const REGISTRY: &str = "111122223333.dkr.ecr.us-east-1.amazonaws.com/acme-prod";

pub fn key(seed: u8) -> SecretKey {
    let mut secret = [seed; 32];
    secret[0] = 1;
    SecretKey::from_secret_bytes(&secret).expect("fixed secrets are valid scalars")
}

/// The release key of the test project.
pub fn ci() -> SecretKey {
    key(4)
}

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap()
}

/// A statement of `kind`, unsigned.
pub fn statement<B: Serialize>(
    project: &str,
    kind: Kind,
    install: Option<&str>,
    sequence: u64,
    not_after: Option<DateTime<Utc>>,
    body: B,
) -> SignedStatement {
    statement_under(project, kind, install, sequence, not_after, 1, body)
}

/// A statement of `kind` under signer list version `list_version`, unsigned.
pub fn statement_under<B: Serialize>(
    project: &str,
    kind: Kind,
    install: Option<&str>,
    sequence: u64,
    not_after: Option<DateTime<Utc>>,
    list_version: u64,
    body: B,
) -> SignedStatement {
    SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: project.to_string(),
            install: install.map(str::to_string),
            sequence,
            not_after,
            signer_list_version: list_version,
            body,
        }
        .to_payload()
        .expect("the envelope serializes"),
    )
}

/// Signer list version 1 of `project` with `key` as its one admin and
/// releaser, valid for 60 days from `now`.
pub fn signer_list_of(project: &str, key: &SecretKey, now: DateTime<Utc>) -> SignedStatement {
    statement(
        project,
        Kind::SignerList,
        None,
        1,
        Some(now + Duration::days(60)),
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: key.public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Admin, Role::Release],
                label: None,
            }],
            deny: Vec::new(),
            recovered: false,
        },
    )
    .sign_with("ci", key)
    .expect("signing works")
}

pub fn signer_list() -> SignedStatement {
    signer_list_of(PROJECT, &ci(), now())
}

pub fn settings_for(list: &SignedStatement, prefix: &str, channel: &str) -> SigningSettings {
    SigningSettings {
        private_input_custody: false,
        private_machine_api_url: None,
        fingerprint: fingerprint(&list.open().expect("the list opens").digest),
        prefix: prefix.to_string(),
        channel: channel.to_string(),
    }
}

pub fn settings() -> SigningSettings {
    settings_for(&signer_list(), PREFIX, CHANNEL)
}

/// A function workload named `api` running `image`.
pub fn function_workload_with(image: &str) -> Workload {
    Workload {
        name: "api".to_string(),
        kind: WorkloadKind::Function,
        image: image.to_string(),
        command: None,
        env: BTreeMap::from([("MODE".to_string(), "signed".to_string())]),
        provided_env: vec!["ALIEN_DEPLOYMENT_ID".to_string()],
        bindings: BTreeMap::new(),
        configurable_env: vec!["LOG_LEVEL".to_string()],
        deployer_env: BTreeMap::new(),
        secrets: Vec::new(),
        runtime: Runtime::default(),
        volume_mount_path: None,
        identity: Some("execution".to_string()),
        bounds: Bounds::default(),
        function: Some(FunctionSettings {
            memory_mb: 512,
            timeout_seconds: 30,
            architecture: None,
        }),
    }
}

pub fn function_workload() -> Workload {
    function_workload_with(&format!("ghcr.io/shop/api@{DIGEST}"))
}

pub fn release_body(workloads: Vec<Workload>) -> ReleaseBody {
    ReleaseBody {
        platform: "aws".to_string(),
        channel: CHANNEL.to_string(),
        workloads,
        configurable: Configurable {
            env: vec!["LOG_LEVEL".to_string()],
            inputs: Vec::new(),
        },
        components: BTreeMap::new(),
        setup: BTreeMap::new(),
        stack: None,
    }
}

pub fn signed_release_by(
    project: &str,
    sequence: u64,
    body: &ReleaseBody,
    key: &SecretKey,
) -> SignedStatement {
    statement(project, Kind::Release, None, sequence, None, body)
        .sign_with("ci", key)
        .expect("signing works")
}

pub fn signed_release(sequence: u64, body: &ReleaseBody) -> SignedStatement {
    signed_release_by(PROJECT, sequence, body, &ci())
}

pub fn release(sequence: u64, workload: Workload) -> SignedStatement {
    signed_release(sequence, &release_body(vec![workload]))
}

pub fn install_id() -> String {
    format!("aws:{ACCOUNT}:{REGION}:{PREFIX}")
}

pub fn config_for(
    project: &str,
    install: &str,
    sequence: u64,
    body: &ConfigBody,
    key: &SecretKey,
) -> SignedStatement {
    statement(project, Kind::Config, Some(install), sequence, None, body)
        .sign_with("ci", key)
        .expect("signing works")
}

pub fn config(sequence: u64, body: ConfigBody) -> SignedStatement {
    config_for(PROJECT, &install_id(), sequence, &body, &ci())
}
