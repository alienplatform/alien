//! `verify --terraform` against a module extracted from the archive the
//! hosting service serves, with the install's settings as Parameter Manager
//! and Azure return them, and against a module someone changed.

use super::*;
use crate::commands::verify::tests::{fingerprint, genesis, now, signed};
use crate::commands::verify::{check_signed, render, Line, Mark};
use alien_cli_common::setup_templates::{
    render_setup_template, RenderedSetupTemplate, SetupHostingOptions, SetupRenderOptions,
};
use alien_core::{SetupPins, Stack, StackSettings, StackSigning};
use alien_signing::{Kind, ReleaseBody, SignedStatement};

const PREFIX: &str = "acme-prod";

fn stack() -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .unwrap()
}

async fn module(cloud: ModuleCloud) -> RenderedSetupTemplate {
    let signing = StackSigning {
        private_machine_api_url: None,
        fingerprint: fingerprint(),
        channel: "production".to_string(),
    };
    let settings = StackSettings {
        signing: Some(signing.clone()),
        ..StackSettings::default()
    };
    let cloudformation = alien_cloudformation::CfRegistry::built_in();
    let terraform = alien_terraform::TfRegistry::built_in();
    let helm = alien_helm::HelmRegistry::built_in();
    let hosting: SetupHostingOptions = serde_json::from_value(
        serde_json::json!({ "registration": { "mode": "outputsFallback" } }),
    )
    .unwrap();
    let options = SetupRenderOptions::builder()
        .signing(&signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(hosting)
        .pins(SetupPins {
            aws_publisher: Some("111122223333".to_string()),
            aws_verifier_image: Some(format!("verifier@sha256:{}", "5d".repeat(32))),
            gcp_verifier_image: Some(format!(
                "us-docker.pkg.dev/publisher/verifier/verifier@sha256:{}",
                "5d".repeat(32)
            )),
            azure_verifier_image: Some(format!(
                "publisher.azurecr.io/verifier@sha256:{}",
                "5d".repeat(32)
            )),
            operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
        })
        .build();
    render_setup_template(cloud.template(), &stack(), &settings, &options)
        .await
        .unwrap()
}

/// The archive the hosting service serves: the signed file map as a
/// `.tar.gz` with sorted paths and zeroed times and owners.
fn reproducible_archive(module: &RenderedSetupTemplate) -> Vec<u8> {
    let files: BTreeMap<String, String> = serde_json::from_slice(&module.bytes).unwrap();
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (path, contents) in &files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        archive
            .append_data(&mut header, path, contents.as_bytes())
            .unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

fn extract(archive: &[u8]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(dir.path())
        .unwrap();
    dir
}

fn module_release(
    platform: &str,
    cloud: ModuleCloud,
    module: &RenderedSetupTemplate,
) -> SignedStatement {
    signed(
        Kind::Release,
        30,
        1,
        None,
        ReleaseBody {
            platform: platform.to_string(),
            channel: "production".to_string(),
            workloads: Vec::new(),
            configurable: Default::default(),
            components: Default::default(),
            setup: BTreeMap::from([(cloud.template().to_string(), module.setup_template())]),
            stack: None,
        },
        &[("github-acme-app", 2)],
    )
}

fn settings_json() -> String {
    format!(
        r#"{{"fingerprint":"{}","prefix":"{PREFIX}","channel":"production"}}"#,
        fingerprint()
    )
}

fn ledger_json(install: &str) -> String {
    serde_json::json!({
        "prefix": PREFIX,
        "created": true,
        "fingerprint": fingerprint(),
        "install": install,
        "signerList": { "sequence": 1, "digest": genesis().open().unwrap().digest },
    })
    .to_string()
}

/// A Parameter Manager version as `GET …/versions/<v>?view=FULL` returns it.
fn version(name: &str, created: &str, value: &str) -> Value {
    serde_json::json!({
        "name": format!("projects/acme-data/locations/global/parameters/{name}/versions/{created}"),
        "createTime": created,
        "payload": { "data": base64::engine::general_purpose::STANDARD.encode(value) },
    })
}

fn template_line(lines: &[Line]) -> &Line {
    lines.iter().find(|line| line.label == "Template").unwrap()
}

#[tokio::test]
async fn a_gcp_module_from_the_served_archive_matches_its_signed_release() {
    let rendered = module(ModuleCloud::Gcp).await;
    let extracted = extract(&reproducible_archive(&rendered));
    let installed = installed_module(extracted.path(), ModuleCloud::Gcp).unwrap();
    assert_eq!(installed.digest, rendered.setup_template().sha256);

    // Parameter Manager lists two versions of the settings; the newest counts.
    let settings_name = names::gcp_settings(PREFIX);
    let older = version(&settings_name, "2026-11-01T09:00:00Z", "{}");
    let newer = version(&settings_name, "2026-11-02T09:00:00Z", &settings_json());
    let (newest, created) = newest_version(&settings_name, &[older, newer.clone()])
        .unwrap()
        .unwrap();
    assert_eq!(newest, newer["name"].as_str().unwrap());
    let install = "gcp:acme-data:acme-prod";
    let ledger_name = names::gcp_ledger(PREFIX);
    let ledger = version(&ledger_name, "2026-11-02T09:00:00Z", &ledger_json(install));
    let signed = Signed {
        install: InstallId::parse(install).unwrap(),
        prefix: PREFIX.to_string(),
        settings_name: settings_name.clone(),
        settings: Some((
            parameter_payload(&settings_name, &newer).unwrap(),
            Some(created),
        )),
        ledger_name: ledger_name.clone(),
        ledger: Some(parameter_payload(&ledger_name, &ledger).unwrap()),
        signer_lists: vec![genesis()],
        releases: vec![(
            "rel_30".to_string(),
            module_release("gcp", ModuleCloud::Gcp, &rendered),
        )],
        platform: "gcp",
    };
    let lines = check_signed(&signed, &installed, Vec::new(), now()).unwrap();
    assert_eq!(
        render(&lines),
        format!(
            "Install              gcp:acme-data:acme-prod\n\
             Signing fingerprint  {}   (set by setup, 2026-11-02)\n\
             Template             matches release rel_30 · production · {}  ✓\n\
             Variables            {VARIABLES}\n\
             Running              nothing yet: no release has been accepted\n\
             Trusted keys         alice@acme.com, GitHub Actions (acme/app), recovery  (list v1, expires 2027-01-24)\n",
            alien_signing::display_fingerprint(&fingerprint()),
            installed.digest
        )
    );

    // A changed file and an added file are each another module.
    let tampered = extract(&reproducible_archive(&rendered));
    let main = std::fs::read_dir(tampered.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "tf"))
        .unwrap();
    let mut text = std::fs::read_to_string(&main).unwrap();
    text.push_str("\nresource \"null_resource\" \"extra\" {}\n");
    std::fs::write(&main, text).unwrap();
    let changed = installed_module(tampered.path(), ModuleCloud::Gcp).unwrap();
    let lines = check_signed(&signed, &changed, Vec::new(), now()).unwrap();
    assert_eq!(template_line(&lines).mark, Mark::Fail);
    assert!(
        template_line(&lines)
            .text
            .contains("is not the terraform-gcp template of any release signed for production"),
        "{}",
        template_line(&lines).text
    );

    let added = extract(&reproducible_archive(&rendered));
    std::fs::write(added.path().join("override.tf"), "locals {}\n").unwrap();
    let lines = check_signed(
        &signed,
        &installed_module(added.path(), ModuleCloud::Gcp).unwrap(),
        Vec::new(),
        now(),
    )
    .unwrap();
    assert_eq!(template_line(&lines).mark, Mark::Fail);

    // The AWS module of the same release is not a GCP module.
    let aws = module(ModuleCloud::Aws).await;
    let mut other = signed.clone();
    other.releases = vec![(
        "rel_30".to_string(),
        module_release("aws", ModuleCloud::Aws, &aws),
    )];
    let lines = check_signed(&other, &installed, Vec::new(), now()).unwrap();
    assert_eq!(template_line(&lines).mark, Mark::Fail);
}

#[test]
fn a_disabled_newest_parameter_version_is_refused_not_skipped() {
    let name = names::gcp_settings(PREFIX);
    let mut newest = version(&name, "2026-11-02T09:00:00Z", "{}");
    newest["disabled"] = Value::Bool(true);
    let older = version(&name, "2026-11-01T09:00:00Z", &settings_json());
    let error = newest_version(&name, &[older, newest]).unwrap_err();
    assert!(error.message.contains("is disabled"), "{}", error.message);
    assert_eq!(newest_version(&name, &[]).unwrap(), None);
}

#[tokio::test]
async fn an_azure_module_reads_its_settings_from_the_group_tag_and_its_ledger_from_key_vault() {
    let rendered = module(ModuleCloud::Azure).await;
    let extracted = extract(&reproducible_archive(&rendered));
    let installed = installed_module(extracted.path(), ModuleCloud::Azure).unwrap();
    let install = "azure:0b1e7c2a-3d4f-4a5b-8c6d-7e8f9a0b1c2d:acme-prod-rg";
    // As `GET /subscriptions/…/resourceGroups/acme-prod-rg` and the Key Vault
    // secret return them.
    let group = serde_json::json!({
        "id": "/subscriptions/0b1e7c2a-3d4f-4a5b-8c6d-7e8f9a0b1c2d/resourceGroups/acme-prod-rg",
        "name": "acme-prod-rg",
        "location": "eastus",
        "tags": { names::AZURE_SETTINGS_TAG: settings_json(), "owner": "platform" },
    });
    let secret = serde_json::json!({
        "value": ledger_json(install),
        "id": "https://acme-prod-kv.vault.azure.net/secrets/signing-state/1",
    });
    let signed = Signed {
        install: InstallId::parse(install).unwrap(),
        prefix: PREFIX.to_string(),
        settings_name: "tag".to_string(),
        settings: resource_group_settings(&group).map(|bytes| (bytes, None)),
        ledger_name: "secret".to_string(),
        ledger: Some(key_vault_value(&secret).unwrap()),
        signer_lists: vec![genesis()],
        releases: vec![(
            "rel_30".to_string(),
            module_release("azure", ModuleCloud::Azure, &rendered),
        )],
        platform: "azure",
    };
    let lines = check_signed(&signed, &installed, Vec::new(), now()).unwrap();
    assert!(
        lines.iter().all(|line| line.mark != Mark::Fail),
        "{}",
        render(&lines)
    );
    assert_eq!(template_line(&lines).mark, Mark::Pass);

    // A group without the tag is not protected, whatever the module says.
    let untagged = serde_json::json!({ "name": "acme-prod-rg", "tags": { "owner": "platform" } });
    let mut unprotected = signed.clone();
    unprotected.settings = resource_group_settings(&untagged).map(|bytes| (bytes, None));
    let error = check_signed(&unprotected, &installed, Vec::new(), now()).unwrap_err();
    assert!(
        error.message.contains("is not protected"),
        "{}",
        error.message
    );
}

#[test]
fn a_symlink_in_the_module_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.tf"), "locals {}\n").unwrap();
    std::os::unix::fs::symlink("/etc/hostname", dir.path().join("extra.tf")).unwrap();
    let error = directory_files(dir.path()).unwrap_err();
    assert!(
        error.message.contains("is not a regular file"),
        "{}",
        error.message
    );
}
