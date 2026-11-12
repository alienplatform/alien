//! A controller binary replaced on disk never runs: the Operator refuses it before a pass, and
//! a process checked before the swap keeps running the bytes it was checked against.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, RuntimeMetadata,
    StackSettings, StackState,
};
use alien_infra::extension_bundle::{host_target, testing::TestBundle};
use alien_operator::extension_bundles::{cache, provider_for, Controllers};
use serde_json::json;

const DNS_ZONE: &str = "acme/dns-zone";

fn script(marker: &Path, tag: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho {tag} >> '{}'\nexit 3\n", marker.display()).into_bytes()
}

fn binary_path(data_dir: &Path, bundle: &TestBundle) -> std::path::PathBuf {
    cache(data_dir.to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap()
        .join(format!("controller-{}", host_target().unwrap()))
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

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: "hash".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

/// A swap before the pass starts: the cache checks the file again and refuses it.
#[tokio::test]
async fn a_binary_swapped_before_the_pass_is_refused() {
    let data_dir = tempfile::tempdir().unwrap();
    let marker = data_dir.path().join("ran");
    let bundle = TestBundle::new(
        "acme",
        "1.0.0",
        &[(
            host_target().unwrap(),
            script(&marker, "ORIGINAL").as_slice(),
        )],
    );
    cache_bundle(data_dir.path(), &bundle);
    let state = deployment(&bundle);
    let processes = Controllers::default();
    let c = cache(data_dir.path().to_str().unwrap());
    provider_for(None, &c, &state, "dep", &ClientConfig::Test, &processes)
        .await
        .unwrap();
    write_exec(
        &binary_path(data_dir.path(), &bundle),
        &script(&marker, "SWAPPED"),
    );
    let refused = provider_for(None, &c, &state, "dep", &ClientConfig::Test, &processes).await;
    assert!(refused.is_err(), "a swapped binary must be refused");
    assert!(!marker.exists());
}

/// A swap after the pass verified the binary, before its first step, runs nothing new.
#[tokio::test(flavor = "multi_thread")]
async fn a_binary_swapped_after_the_check_is_not_executed() {
    let data_dir = tempfile::tempdir().unwrap();
    let marker = data_dir.path().join("ran");
    let bundle = TestBundle::new(
        "acme",
        "1.0.0",
        &[(
            host_target().unwrap(),
            script(&marker, "ORIGINAL").as_slice(),
        )],
    );
    cache_bundle(data_dir.path(), &bundle);
    let state = deployment(&bundle);
    let processes = Controllers::default();
    let c = cache(data_dir.path().to_str().unwrap());
    let provider = provider_for(None, &c, &state, "dep", &ClientConfig::Test, &processes)
        .await
        .unwrap()
        .unwrap();

    write_exec(
        &binary_path(data_dir.path(), &bundle),
        &script(&marker, "SWAPPED"),
    );

    let mut current = state;
    for _ in 0..3 {
        alien_deployment::set_ephemeral_secrets_digest_key();
        current = alien_deployment::step(
            current,
            config(),
            ClientConfig::Test,
            Some(provider.clone()),
        )
        .await
        .unwrap()
        .state;
    }
    let ran = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        ran.contains("ORIGINAL"),
        "the verified binary ran, so the check is not passing for want of a run: {ran:?}"
    );
    assert!(
        !ran.contains("SWAPPED"),
        "the swapped binary ran after the check passed: {ran:?}"
    );
}

/// A controller that exits after a verified start is started again from the verified bytes, not
/// from a binary swapped in meanwhile.
#[tokio::test(flavor = "multi_thread")]
async fn a_respawn_after_an_exit_runs_only_the_verified_binary() {
    let data_dir = tempfile::tempdir().unwrap();
    let marker = data_dir.path().join("ran");
    let bundle = TestBundle::new(
        "acme",
        "1.0.0",
        &[(
            host_target().unwrap(),
            script(&marker, "ORIGINAL").as_slice(),
        )],
    );
    cache_bundle(data_dir.path(), &bundle);
    let state = deployment(&bundle);
    let processes = Controllers::default();
    let c = cache(data_dir.path().to_str().unwrap());
    let provider = provider_for(None, &c, &state, "dep", &ClientConfig::Test, &processes)
        .await
        .unwrap()
        .unwrap();
    let mut current = state;
    for _ in 0..5 {
        if std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .contains("ORIGINAL")
        {
            break;
        }
        alien_deployment::set_ephemeral_secrets_digest_key();
        current = alien_deployment::step(
            current,
            config(),
            ClientConfig::Test,
            Some(provider.clone()),
        )
        .await
        .unwrap()
        .state;
    }
    let before = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(before.contains("ORIGINAL"), "{before:?}");

    write_exec(
        &binary_path(data_dir.path(), &bundle),
        &script(&marker, "SWAPPED"),
    );
    for _ in 0..2 {
        alien_deployment::set_ephemeral_secrets_digest_key();
        current = alien_deployment::step(
            current,
            config(),
            ClientConfig::Test,
            Some(provider.clone()),
        )
        .await
        .unwrap()
        .state;
    }
    let ran = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        ran.matches("ORIGINAL").count() >= 2,
        "the verified binary ran again after the exit: {ran:?}"
    );
    assert!(
        !ran.contains("SWAPPED"),
        "the respawn ran the swapped binary: {ran:?}"
    );
}
