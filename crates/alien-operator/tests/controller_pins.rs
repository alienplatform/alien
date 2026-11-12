//! A bundle that pins its controller's sha256 in uppercase hex runs in the Operator, as the cache
//! accepts it.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use alien_core::{
    ClientConfig, DeploymentState, DeploymentStatus, ExtensionBundleManifest, ExtensionManifest,
    InitialSetupAuthority, Platform, ReleaseInfo, RuntimeMetadata, StackState,
};
use alien_infra::extension_bundle::{
    host_target,
    testing::{core_zip, hooks_module, TestBundle},
};
use alien_operator::extension_bundles::{cache, provider_for, Controllers};
use serde_json::json;
use sha2::{Digest, Sha256};

const DNS_ZONE: &str = "acme/dns-zone";

fn script(marker: &Path, tag: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho {tag} >> '{}'\nexit 3\n", marker.display()).into_bytes()
}

fn write_exec(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn cache_bundle(data_dir: &Path, bundle: &TestBundle) {
    let dir = cache(data_dir.to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("core.zip"), &bundle.zip).unwrap();
    for (target, bytes) in &bundle.controllers {
        write_exec(&dir.join(format!("controller-{target}")), bytes);
    }
}

fn deployment(bundle: &TestBundle) -> DeploymentState {
    let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
    manifest["resourceTypes"] = json!({ DNS_ZONE: {
        "lifecycles": ["live"], "holdsData": false,
        "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
        "controllerPlatforms": ["test"], "permissionSets": [], "link": { "permissionSets": [] },
        "schemas": { "config": {}, "outputs": {} }
    }});
    let stack: alien_core::Stack = serde_json::from_value(json!({
        "id": "acme",
        "resources": { "zone": {
            "config": { "type": DNS_ZONE, "id": "zone", "domain": "a.acme.dev" },
            "lifecycle": "live", "dependencies": []
        }},
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [manifest]
    }))
    .unwrap();
    DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform: Platform::Test,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_1".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: stack.clone(),
        }),
        stack_state: Some(StackState::new(Platform::Test)),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            initial_setup_authority: InitialSetupAuthority::DirectSetup,
            prepared_stack: Some(stack),
            pending_prepared_release_id: Some("rel_1".to_string()),
            ..RuntimeMetadata::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

#[tokio::test]
async fn an_uppercase_controller_pin_is_accepted_by_the_operator() {
    let data_dir = tempfile::tempdir().unwrap();
    let marker = data_dir.path().join("ran");
    let bytes = script(&marker, "ORIGINAL");
    let target = host_target().unwrap();
    let hooks = hooks_module("acme");
    let upper = format!("{:X}", Sha256::digest(&bytes));
    let manifest: ExtensionBundleManifest = serde_json::from_value(json!({
        "name": "acme", "version": "1.0.0",
        "protocol": { "hooks": 1, "controller": 1 },
        "controllerUrl": null,
        "hooksSha256": format!("{:x}", Sha256::digest(&hooks)),
        "controllers": { serde_json::to_value(target).unwrap().as_str().unwrap():
            { "sha256": upper, "size": bytes.len() } },
        "resourceTypes": {}
    }))
    .unwrap();
    let zip = core_zip(&manifest, &hooks);
    let bundle = TestBundle {
        manifest: ExtensionManifest {
            name: "acme".to_string(),
            version: "1.0.0".to_string(),
            digest: format!("sha256:{:x}", Sha256::digest(&zip)),
            resource_types: Default::default(),
        },
        zip,
        controllers: [(target, bytes.clone())].into_iter().collect(),
    };
    cache_bundle(data_dir.path(), &bundle);
    let state = deployment(&bundle);
    let processes = Controllers::default();
    let c = cache(data_dir.path().to_str().unwrap());
    let result = provider_for(None, &c, &state, "dep", &ClientConfig::Test, &processes).await;
    assert!(
        result.is_ok(),
        "the cache accepted the pin, the Operator refused it: {:?}",
        result.err()
    );
}
