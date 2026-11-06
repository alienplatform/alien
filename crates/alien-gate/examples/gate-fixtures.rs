//! Signed statements for the verifier's end-to-end test.
//!
//! `gate-fixtures <out-dir> <prefix> <install-id> <image-a> <image-b>`
//! writes, under fresh keys: signer list version 1, the signing settings and
//! the empty ledger setup writes for `prefix`, releases of one function
//! workload `api`, and config statements for `install-id`. See
//! `scripts/gate-e2e/aws.py`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use alien_core::release_statement::{self, ReleaseOptions};
use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
use alien_signing::envelope::Envelope;
use alien_signing::release::{
    Bounds, Configurable, FunctionSettings, Runtime, SecretBinding, WorkloadKind,
};
use alien_signing::{
    fingerprint, ConfigBody, Kind, Ledger, ReleaseBody, Role, SecretKey, SignedStatement, Signer,
    SignerKind, SignerListBody, SigningSettings, Workload,
};
use chrono::{Duration, Utc};
use serde::Serialize;

const PROJECT: &str = "prj_e2e";

fn statement<B: Serialize>(
    kind: Kind,
    install: Option<&str>,
    sequence: u64,
    body: B,
) -> Result<SignedStatement, Box<dyn std::error::Error>> {
    Ok(SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: PROJECT.to_string(),
            install: install.map(str::to_string),
            sequence,
            not_after: (kind == Kind::SignerList).then(|| Utc::now() + Duration::days(60)),
            signer_list_version: 1,
            body,
        }
        .to_payload()?,
    ))
}

/// A release of `api` running `image`, on `channel`, whose one secret is
/// named `secret`.
fn release(image: &str, marker: &str, channel: &str, secret: &str) -> ReleaseBody {
    ReleaseBody {
        platform: "aws".to_string(),
        channel: channel.to_string(),
        workloads: vec![Workload {
            name: "api".to_string(),
            kind: WorkloadKind::Function,
            image: image.to_string(),
            command: None,
            env: BTreeMap::from([("WORKLOAD_MARKER".to_string(), marker.to_string())]),
            provided_env: vec!["ALIEN_DEPLOYMENT_ID".to_string()],
            bindings: BTreeMap::from([("data".to_string(), "storage".to_string())]),
            configurable_env: vec!["LOG_LEVEL".to_string()],
            deployer_env: BTreeMap::new(),
            secrets: vec![SecretBinding {
                name: secret.to_string(),
                env_name: Some(secret.to_string()),
                file_path: None,
            }],
            runtime: Runtime::default(),
            volume_mount_path: None,
            identity: Some("execution".to_string()),
            bounds: Bounds::default(),
            function: Some(FunctionSettings {
                memory_mb: 256,
                timeout_seconds: 20,
                architecture: Some("x86_64".to_string()),
            }),
        }],
        configurable: Configurable {
            env: vec!["LOG_LEVEL".to_string()],
            inputs: Vec::new(),
        },
        components: BTreeMap::new(),
        setup: BTreeMap::new(),
        stack: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, out, prefix, install, image_a, image_b] = args.as_slice() else {
        return Err(
            "usage: gate-fixtures <out-dir> <prefix> <install-id> <image-a> <image-b>".into(),
        );
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let write = |name: &str, json: String| std::fs::write(out.join(name), json);

    let ci = SecretKey::generate()?;
    let attacker = SecretKey::generate()?;
    let list = statement(
        Kind::SignerList,
        None,
        1,
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: ci.public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Admin, Role::Release],
                label: None,
            }],
            deny: Vec::new(),
            recovered: false,
        },
    )?
    .sign_with("ci", &ci)?;
    let settings = SigningSettings {
        private_input_custody: false,
        private_machine_api_url: None,
        fingerprint: fingerprint(&list.open()?.digest),
        prefix: prefix.clone(),
        channel: "production".to_string(),
    };
    write("signer-list.json", serde_json::to_string_pretty(&list)?)?;
    write("settings.json", settings.to_json())?;
    write("ledger.json", Ledger::new(prefix).to_json())?;

    let signed = |name: &str, sequence: u64, body: &ReleaseBody, key: &SecretKey| {
        let statement = statement(Kind::Release, None, sequence, body)?.sign_with("ci", key)?;
        write(
            &format!("{name}.json"),
            serde_json::to_string_pretty(&statement)?,
        )?;
        Ok::<_, Box<dyn std::error::Error>>(statement)
    };
    signed(
        "release-10",
        10,
        &release(image_a, "release-10", "production", "DB_PASSWORD"),
        &ci,
    )?;
    let twenty = release(image_b, "release-20", "production", "DB_PASSWORD");
    signed("release-20", 20, &twenty, &ci)?;
    signed(
        "release-30-dev",
        30,
        &release(image_a, "release-30-dev", "dev", "DB_PASSWORD"),
        &ci,
    )?;
    signed(
        "release-40-attacker",
        40,
        &release(image_a, "release-40-attacker", "production", "DB_PASSWORD"),
        &attacker,
    )?;
    signed(
        "release-60-ld-preload",
        60,
        &release(image_b, "release-60-ld-preload", "production", "LD_PRELOAD"),
        &ci,
    )?;
    // A machine cluster: one group of one t3.micro, booting the machine
    // agent build `agent`.
    let agent = format!("sha256:{}", "a".repeat(64));
    let machines = |platform: Platform, instance_type: &str| {
        let stack = Stack::new("machines".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .capacity_group(CapacityGroup {
                        group_id: "general".to_string(),
                        instance_type: Some(instance_type.to_string()),
                        profile: None,
                        min_size: 1,
                        max_size: 1,
                        scale_policy: None,
                        nested_virtualization: None,
                        data_disk_gib_range: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        release_statement::release_body(
            platform,
            &stack,
            &ReleaseOptions {
                channel: "production".to_string(),
                components: BTreeMap::from([(
                    "machine-agent/linux-x86_64".to_string(),
                    agent.clone(),
                )]),
                ..ReleaseOptions::default()
            },
        )
    };
    signed(
        "release-70-machines",
        70,
        &machines(Platform::Aws, "t3.micro")?,
        &ci,
    )?;
    signed(
        "release-71-machines-gcp",
        71,
        &machines(Platform::Gcp, "e2-small")?,
        &ci,
    )?;
    signed(
        "release-72-machines-azure",
        72,
        &machines(Platform::Azure, "Standard_B2s")?,
        &ci,
    )?;

    // Release 20's body at sequence 50 with no signature at all.
    let unsigned = statement(Kind::Release, None, 50, &twenty)?;
    write(
        "release-50-unsigned.json",
        serde_json::to_string_pretty(&unsigned)?,
    )?;

    let config = |name: &str, sequence: u64, body: ConfigBody| {
        let statement =
            statement(Kind::Config, Some(install), sequence, body)?.sign_with("ci", &ci)?;
        write(
            &format!("{name}.json"),
            serde_json::to_string_pretty(&statement)?,
        )?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    config(
        "config-1",
        1,
        ConfigBody {
            env: BTreeMap::from([("LOG_LEVEL".to_string(), "info".to_string())]),
            ..ConfigBody::default()
        },
    )?;
    // A signed config may name a binding the install derives; the derived
    // value still wins.
    config(
        "config-2-binding",
        2,
        ConfigBody {
            env: BTreeMap::from([("LOG_LEVEL".to_string(), "warn".to_string())]),
            bindings: BTreeMap::from([(
                "data".to_string(),
                serde_json::json!({"service": "s3", "bucketName": "attacker-bucket"}),
            )]),
            ..ConfigBody::default()
        },
    )?;
    println!("{}", out.display());
    Ok(())
}
