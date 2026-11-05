//! Fixtures shared by the tests and the corpus writer. Keys come from fixed
//! secrets and ECDSA signing is deterministic (RFC 6979), so every fixture
//! has the same bytes on every run.

#![allow(dead_code)]

use std::collections::BTreeMap;

use alien_signing::envelope::Envelope;
use alien_signing::release::{Bounds, Runtime, WorkloadKind};
use alien_signing::{
    fingerprint, InstallId, ReleaseBody, Role, SecretKey, SignedStatement, Signer, SignerKind,
    SignerListBody, SigningSettings, Workload,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Serialize;

pub const PROJECT: &str = "prj_test";
pub const PREFIX: &str = "acme-prod";
pub const INSTALL: &str = "aws:111122223333:us-east-1:acme-prod";
pub const CHANNEL: &str = "production";
pub const IMAGE: &str =
    "registry.example.com/app@sha256:1111111111111111111111111111111111111111111111111111111111111111";

pub fn key(seed: u8) -> SecretKey {
    let mut secret = [seed; 32];
    secret[0] = 1;
    SecretKey::from_secret_bytes(&secret).expect("fixed secrets are valid scalars")
}

pub struct Keys {
    pub admin: SecretKey,
    pub second_admin: SecretKey,
    pub ci: SecretKey,
    pub operator: SecretKey,
    pub recovery: SecretKey,
    pub outsider: SecretKey,
    pub components: SecretKey,
}

pub fn keys() -> Keys {
    Keys {
        admin: key(2),
        second_admin: key(3),
        ci: key(4),
        operator: key(5),
        recovery: key(6),
        outsider: key(8),
        components: key(9),
    }
}

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap()
}

pub fn signer(id: &str, key: &SecretKey, kind: SignerKind, roles: &[Role]) -> Signer {
    Signer {
        id: id.to_string(),
        key: key.public_key().as_str().to_string(),
        kind,
        roles: roles.to_vec(),
        label: None,
    }
}

pub fn statement<B: Serialize>(
    kind: &str,
    project: &str,
    install: Option<&str>,
    sequence: u64,
    signer_list_version: u64,
    body: B,
) -> SignedStatement {
    statement_until(
        kind,
        project,
        install,
        sequence,
        signer_list_version,
        None,
        body,
    )
}

pub fn statement_until<B: Serialize>(
    kind: &str,
    project: &str,
    install: Option<&str>,
    sequence: u64,
    signer_list_version: u64,
    not_after: Option<DateTime<Utc>>,
    body: B,
) -> SignedStatement {
    let envelope = Envelope {
        kind: kind.to_string(),
        project: project.to_string(),
        install: install.map(str::to_string),
        sequence,
        not_after,
        signer_list_version,
        body,
    };
    SignedStatement::new(&envelope.to_payload().unwrap())
}

/// A signer list version, valid for 60 days from [`now`].
pub fn list<B: Serialize>(
    project: &str,
    version: u64,
    authorized_by: u64,
    body: B,
) -> SignedStatement {
    statement_until(
        "signer-list/v1",
        project,
        None,
        version,
        authorized_by,
        Some(now() + Duration::days(60)),
        body,
    )
}

pub fn genesis_body(keys: &Keys) -> SignerListBody {
    SignerListBody {
        previous: None,
        admin_threshold: 1,
        signers: vec![
            signer("ci", &keys.ci, SignerKind::Ci, &[Role::Release]),
            signer(
                "alice-laptop",
                &keys.admin,
                SignerKind::Device,
                &[Role::Release, Role::Operate, Role::Admin],
            ),
            signer(
                "recovery",
                &keys.recovery,
                SignerKind::Recovery,
                &[Role::Admin],
            ),
        ],
        deny: Vec::new(),
        recovered: false,
    }
}

pub fn genesis(keys: &Keys) -> SignedStatement {
    list(PROJECT, 1, 1, genesis_body(keys))
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
}

/// Version 2 adds Bob's laptop with `operate` and a second admin, and raises
/// the threshold to 2.
pub fn version_2_body(keys: &Keys) -> SignerListBody {
    let mut body = genesis_body(keys);
    body.previous = Some(genesis(keys).open().unwrap().digest);
    body.admin_threshold = 2;
    body.signers.push(signer(
        "bob-laptop",
        &keys.operator,
        SignerKind::Device,
        &[Role::Operate],
    ));
    body.signers.push(signer(
        "carol-laptop",
        &keys.second_admin,
        SignerKind::Device,
        &[Role::Admin],
    ));
    body
}

pub fn version_2(keys: &Keys) -> SignedStatement {
    list(PROJECT, 2, 1, version_2_body(keys))
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
}

/// Version 3 removes Bob, signed by both admins as version 2 requires.
pub fn version_3_body(keys: &Keys) -> SignerListBody {
    let mut body = version_2_body(keys);
    body.previous = Some(version_2(keys).open().unwrap().digest);
    body.signers.retain(|signer| signer.id != "bob-laptop");
    body
}

pub fn version_3(keys: &Keys) -> SignedStatement {
    list(PROJECT, 3, 2, version_3_body(keys))
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
        .sign_with("carol-laptop", &keys.second_admin)
        .unwrap()
}

pub fn settings(keys: &Keys) -> SigningSettings {
    SigningSettings {
        private_machine_api_url: None,
        fingerprint: fingerprint(&genesis(keys).open().unwrap().digest),
        prefix: PREFIX.to_string(),
        channel: CHANNEL.to_string(),
    }
}

pub fn install() -> InstallId {
    InstallId::parse(INSTALL).unwrap()
}

pub fn web_workload() -> Workload {
    Workload {
        name: "web".to_string(),
        kind: WorkloadKind::Container,
        image: IMAGE.to_string(),
        command: Some(vec![
            "/app/server".to_string(),
            "--port".to_string(),
            "8080".to_string(),
        ]),
        env: BTreeMap::from([("LOG_LEVEL".to_string(), "info".to_string())]),
        provided_env: vec!["PORT".to_string()],
        bindings: BTreeMap::from([("uploads".to_string(), "storage".to_string())]),
        configurable_env: vec!["FEATURE_FLAGS".to_string()],
        deployer_env: BTreeMap::from([("DATABASE_URL".to_string(), "databaseUrl".to_string())]),
        secrets: vec![alien_signing::release::SecretBinding {
            name: "db-password".to_string(),
            env_name: Some("DB_PASSWORD".to_string()),
            file_path: None,
        }],
        runtime: Runtime::default(),
        volume_mount_path: None,
        identity: Some("execution".to_string()),
        bounds: Bounds {
            max_cpu: Some("2".to_string()),
            max_memory: Some("1Gi".to_string()),
            max_replicas: Some(4),
        },
        function: None,
    }
}

pub fn release_body() -> ReleaseBody {
    ReleaseBody {
        platform: "aws".to_string(),
        channel: CHANNEL.to_string(),
        workloads: vec![web_workload()],
        configurable: alien_signing::release::Configurable {
            env: vec!["FEATURE_FLAGS".to_string()],
            inputs: vec!["region-label".to_string()],
        },
        components: BTreeMap::new(),
        setup: BTreeMap::new(),
        stack: None,
    }
}

pub fn release(keys: &Keys, sequence: u64) -> SignedStatement {
    statement("release/v2", PROJECT, None, sequence, 1, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap()
}
