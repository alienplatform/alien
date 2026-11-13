//! Signed sync responses for the protected Operator's kind test.
//!
//! `protected-kind-fixtures <out-dir> <release> <install-id> <image-a> <image-b> <image-evil>`
//! writes, under fresh keys, the signing settings and empty ledger setup
//! writes for Helm release `<release>`, and one `/v1/sync` response body per
//! case in `<out-dir>/sync/`. See `scripts/signed-release-e2e/operator_kind.py`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use alien_core::release_statement::{release_body, ReleaseOptions};
use alien_core::sync::{SyncResponse, TargetDeployment};
use alien_core::{
    DeploymentConfig, EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot,
    ExternalBindings, Platform, ReleaseInfo, SignedStatement, Stack, StackSettings,
};
use alien_signing::envelope::Envelope;
use alien_signing::{
    fingerprint, ConfigBody, Kind, Ledger, Role, SecretKey, Signer, SignerKind, SignerListBody,
    SigningSettings,
};
use base64::Engine as _;
use chrono::{Duration, Utc};
use serde::Serialize;

type Error = Box<dyn std::error::Error>;

const PROJECT: &str = "prj_kind";

fn statement<B: Serialize>(
    kind: Kind,
    install: Option<&str>,
    sequence: u64,
    list_version: u64,
    body: B,
) -> Result<SignedStatement, Error> {
    Ok(SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: PROJECT.to_string(),
            install: install.map(str::to_string),
            sequence,
            not_after: (kind == Kind::SignerList).then(|| Utc::now() + Duration::days(60)),
            signer_list_version: list_version,
            body,
        }
        .to_payload()?,
    ))
}

/// The app: one container printing `message`, with a developer input
/// (`LOG_LEVEL`) and a deployer input (`DATABASE_URL`). `deployer_env`
/// names the env the deployer input lands in.
fn stack(image: &str, message: &str, deployer_env: &str) -> Result<Stack, Error> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": "signed-e2e",
        "permissions": {"profiles": {"execution": {"*": []}}},
        "inputs": [
            {"id": "logLevel", "kind": "string", "providedBy": ["developer"], "required": false,
             "label": "Log level", "description": "Set by the developer.",
             "env": [{"name": "LOG_LEVEL", "targetResources": ["app"]}]},
            {"id": "databaseUrl", "kind": "string", "providedBy": ["deployer"], "required": false,
             "label": "Database URL", "description": "Set by the installer.",
             "env": [{"name": deployer_env, "targetResources": ["app"]}]},
        ],
        "resources": {"app": {
            "config": {
                "id": "app", "type": "container",
                "code": {"type": "image", "image": image},
                "command": ["sh", "-c", "echo started: $MESSAGE; env | sort; exec sleep 2147483647"],
                "cpu": {"min": "0.05", "desired": "0.05"},
                "memory": {"min": "32Mi", "desired": "32Mi"},
                "environment": {"MESSAGE": message},
                "links": [], "permissions": "execution", "ports": [], "stateful": false,
            },
            "dependencies": [], "lifecycle": "live",
        }},
    }))?)
}

/// The app linked to a storage resource `data` outside the cluster, whose
/// binding comes from signed config.
fn stack_with_data(image: &str, message: &str) -> Result<Stack, Error> {
    let mut stack = serde_json::to_value(stack(image, message, "DATABASE_URL")?)?;
    let mut data = serde_json::to_value(alien_core::Storage::new("data".to_string()).build())?;
    data["type"] = serde_json::json!(alien_core::Storage::RESOURCE_TYPE.to_string());
    stack["resources"]["data"] = serde_json::json!({
        "config": data,
        "dependencies": [],
        "lifecycle": "frozen",
    });
    stack["resources"]["app"]["config"]["links"] =
        serde_json::json!([alien_core::ResourceRef::new(
            alien_core::Storage::RESOURCE_TYPE,
            "data"
        )]);
    Ok(serde_json::from_value(stack)?)
}

fn release_options(channel: &str) -> ReleaseOptions {
    ReleaseOptions {
        channel: channel.to_string(),
        ..ReleaseOptions::default()
    }
}

/// A target the manager sends: `stack` as `release`, with the manager's
/// own environment variables and input values.
fn target(
    id: &str,
    release: Option<SignedStatement>,
    stack: Stack,
    lists: &[SignedStatement],
    config: Option<&SignedStatement>,
    variables: &[(&str, &str)],
    inputs: &[(&str, &str)],
) -> Result<TargetDeployment, Error> {
    Ok(TargetDeployment {
        signed_config: config.cloned(),
        release_info: ReleaseInfo {
            release_id: Some(format!("rel_{id}")),
            version: None,
            description: None,
            stack,
            signed: release,
            signer_lists: lists.to_vec(),
        },
        config: DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: variables
                    .iter()
                    .map(|(name, value)| EnvironmentVariable {
                        name: name.to_string(),
                        value: value.to_string(),
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: Some(vec!["app".to_string()]),
                    })
                    .collect(),
                hash: String::new(),
                created_at: String::new(),
            })
            .input_values(
                inputs
                    .iter()
                    .map(|(id, value)| (id.to_string(), serde_json::json!(value)))
                    .collect::<HashMap<_, _>>(),
            )
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build(),
    })
}

fn write(dir: &Path, name: &str, response: &SyncResponse) -> Result<(), Error> {
    std::fs::write(
        dir.join(format!("{name}.json")),
        serde_json::to_string_pretty(response)?,
    )?;
    Ok(())
}

fn sync(target: TargetDeployment) -> SyncResponse {
    SyncResponse {
        execution_claim: None,
        current_state: None,
        signed_config: target.signed_config.clone(),
        target: Some(target),
        commands_url: None,
        target_operations_bundle_set: None,
        target_dynamic_containers: None,
        target_operator_image: None,
    }
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    let [_, out, release_name, install, image_a, image_b, evil] = args.as_slice() else {
        return Err("usage: protected-kind-fixtures <out-dir> <release> <install-id> <image-a> <image-b> <image-evil>".into());
    };
    let out = PathBuf::from(out);
    let dir = out.join("sync");
    std::fs::create_dir_all(&dir)?;

    let ci = SecretKey::generate()?;
    let laptop = SecretKey::generate()?;
    let attacker = SecretKey::generate()?;
    let signer = |id: &str, key: &SecretKey, kind: SignerKind, roles: Vec<Role>| Signer {
        id: id.to_string(),
        key: key.public_key().as_str().to_string(),
        kind,
        roles,
        label: None,
    };
    let laptop_signer = signer(
        "alice-laptop",
        &laptop,
        SignerKind::Device,
        vec![Role::Admin, Role::Release],
    );
    let v1 = statement(
        Kind::SignerList,
        None,
        1,
        1,
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![
                laptop_signer.clone(),
                signer("ci", &ci, SignerKind::Ci, vec![Role::Release]),
            ],
            deny: Vec::new(),
            recovered: false,
        },
    )?
    .sign_with("alice-laptop", &laptop)?;
    // Version 2 removes the CI key.
    let v2 = statement(
        Kind::SignerList,
        None,
        2,
        1,
        SignerListBody {
            previous: Some(v1.open()?.digest),
            admin_threshold: 1,
            signers: vec![laptop_signer],
            deny: Vec::new(),
            recovered: false,
        },
    )?
    .sign_with("alice-laptop", &laptop)?;
    let settings = SigningSettings {
        fingerprint: fingerprint(&v1.open()?.digest),
        prefix: release_name.clone(),
        channel: "production".to_string(),
    };
    std::fs::write(out.join("settings.json"), settings.to_json())?;
    std::fs::write(out.join("ledger.json"), Ledger::new(release_name).to_json())?;

    let signed = |stack: &Stack,
                  channel: &str,
                  sequence: u64,
                  list_version: u64,
                  id: &str,
                  key: &SecretKey|
     -> Result<SignedStatement, Error> {
        let body = release_body(Platform::Kubernetes, stack, &release_options(channel))?;
        Ok(statement(Kind::Release, None, sequence, list_version, body)?.sign_with(id, key)?)
    };
    let config = |sequence: u64,
                  list_version: u64,
                  id: &str,
                  key: &SecretKey,
                  body: serde_json::Value|
     -> Result<SignedStatement, Error> {
        let body: ConfigBody = serde_json::from_value(body)?;
        Ok(
            statement(Kind::Config, Some(install), sequence, list_version, body)?
                .sign_with(id, key)?,
        )
    };
    let one = stack(image_a, "release-1", "DATABASE_URL")?;
    let two = stack(image_b, "release-2", "DATABASE_URL")?;
    let r1 = signed(&one, "production", 1, 1, "ci", &ci)?;
    let r2 = signed(&two, "production", 2, 1, "ci", &ci)?;
    let c1 = config(
        1,
        1,
        "ci",
        &ci,
        serde_json::json!({"inputs": {"logLevel": "debug"}}),
    )?;
    // After CI leaves the list, the laptop signs the next config.
    let c2 = config(
        2,
        2,
        "alice-laptop",
        &laptop,
        serde_json::json!({"inputs": {"logLevel": "warn"}}),
    )?;
    let v1_only = [v1.clone()];
    let both = [v1.clone(), v2.clone()];

    write(
        &dir,
        "release-1",
        &sync(target(
            "release-1",
            Some(r1.clone()),
            one.clone(),
            &v1_only,
            None,
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "release-2",
        &sync(target(
            "release-2",
            Some(r2.clone()),
            two.clone(),
            &v1_only,
            None,
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "release-2-config-1",
        &sync(target(
            "release-2-config-1",
            Some(r2.clone()),
            two.clone(),
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;

    // Attacks on top of release 2 with config 1.
    let evil_stack = stack(evil, "evil", "DATABASE_URL")?;
    write(
        &dir,
        "unsigned",
        &sync(target(
            "unsigned",
            None,
            evil_stack.clone(),
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "modified-stack-image",
        &sync(target(
            "modified-stack-image",
            Some(r2.clone()),
            evil_stack.clone(),
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    let mut tampered = r2.clone();
    let mut payload: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD.decode(&tampered.payload)?,
    )?;
    payload["body"]["workloads"][0]["image"] = serde_json::json!(evil);
    tampered.payload =
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&payload)?);
    write(
        &dir,
        "tampered-signed-bytes",
        &sync(target(
            "tampered-signed-bytes",
            Some(tampered),
            evil_stack.clone(),
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "older-signed-release",
        &sync(target(
            "older-signed-release",
            Some(r1),
            one,
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    let dev = stack(evil, "dev", "DATABASE_URL")?;
    write(
        &dir,
        "dev-release",
        &sync(target(
            "dev-release",
            Some(signed(&dev, "dev", 3, 1, "ci", &ci)?),
            dev,
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "attacker-key",
        &sync(target(
            "attacker-key",
            Some(signed(&evil_stack, "production", 4, 1, "ci", &attacker)?),
            evil_stack.clone(),
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    // A signed release whose deployer input lands in LD_PRELOAD: the value
    // would come from the install's vault.
    let loader = stack(image_b, "release-5", "LD_PRELOAD")?;
    let loader_release = match signed(&loader, "production", 5, 1, "ci", &ci) {
        Ok(statement) => statement,
        // The release tooling refuses it; sign the body with the stack swapped in.
        Err(_) => {
            let mut body =
                release_body(Platform::Kubernetes, &two, &release_options("production"))?;
            body.stack = Some(serde_json::to_value(&loader)?);
            statement(Kind::Release, None, 5, 1, body)?.sign_with("ci", &ci)?
        }
    };
    write(
        &dir,
        "ld-preload-from-vault",
        &sync(target(
            "ld-preload-from-vault",
            Some(loader_release),
            loader,
            &v1_only,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    write(
        &dir,
        "manager-env-vars",
        &sync(target(
            "manager-env-vars",
            Some(r2.clone()),
            two.clone(),
            &v1_only,
            Some(&c1),
            &[
                ("LD_PRELOAD", "/tmp/evil.so"),
                ("NODE_OPTIONS", "--require /tmp/evil.js"),
            ],
            &[],
        )?),
    )?;
    write(
        &dir,
        "redirected-deployer-input",
        &sync(target(
            "redirected-deployer-input",
            Some(r2.clone()),
            two.clone(),
            &v1_only,
            Some(&c1),
            &[("DATABASE_URL", "postgres://attacker.example/steal")],
            &[("databaseUrl", "postgres://attacker.example/steal")],
        )?),
    )?;
    let mut operator_image = sync(target(
        "manager-operator-image",
        Some(r2.clone()),
        two.clone(),
        &v1_only,
        Some(&c1),
        &[],
        &[],
    )?);
    operator_image.target_operator_image = Some(evil.clone());
    write(&dir, "manager-operator-image", &operator_image)?;

    // The CI key leaves the list: the accepted release keeps running, the
    // next one signed by the laptop lands, and CI can sign nothing more.
    write(
        &dir,
        "ci-removed-accepted-release",
        &sync(target(
            "ci-removed-accepted-release",
            Some(r2),
            two,
            &both,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    let ten = stack(image_a, "release-10", "DATABASE_URL")?;
    let r10 = signed(&ten, "production", 10, 2, "alice-laptop", &laptop)?;
    write(
        &dir,
        "ci-removed-next-release",
        &sync(target(
            "release-10",
            Some(r10.clone()),
            ten.clone(),
            &both,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    let eleven = stack(evil, "release-11", "DATABASE_URL")?;
    write(
        &dir,
        "ci-removed-ci-release",
        &sync(target(
            "ci-removed-ci-release",
            Some(signed(&eleven, "production", 11, 2, "ci", &ci)?),
            eleven,
            &both,
            Some(&c1),
            &[],
            &[],
        )?),
    )?;
    // Newer signed config: the developer value changes without a release.
    write(
        &dir,
        "config-2",
        &sync(target(
            "release-10",
            Some(r10),
            ten,
            &both,
            Some(&c2),
            &[],
            &[],
        )?),
    )?;
    // Bindings: the signed config binds `data`; the manager points it elsewhere.
    let twelve = stack_with_data(image_b, "release-12")?;
    let r12 = signed(&twelve, "production", 12, 2, "alice-laptop", &laptop)?;
    let c3 = config(
        3,
        2,
        "alice-laptop",
        &laptop,
        serde_json::json!({"inputs": {"logLevel": "warn"},
                           "bindings": {"data": {"service": "s3", "bucketName": "shop-data"}}}),
    )?;
    write(
        &dir,
        "binding-signed",
        &sync(target(
            "release-12",
            Some(r12.clone()),
            twelve.clone(),
            &both,
            Some(&c3),
            &[],
            &[],
        )?),
    )?;
    let mut redirect = target(
        "binding-redirect",
        Some(r12),
        twelve,
        &both,
        Some(&c3),
        &[],
        &[],
    )?;
    redirect.config.external_bindings.insert(
        "data",
        alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("attacker-bucket")),
    );
    write(&dir, "binding-redirect", &sync(redirect))?;
    println!("{}", out.display());
    Ok(())
}
