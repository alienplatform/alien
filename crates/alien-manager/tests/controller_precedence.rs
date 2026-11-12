//! With local controllers (`alien dev`), the manager runs each extension's controller as a
//! process on this machine and never asks for a controller server.

use std::sync::{Arc, Mutex};

use alien_core::DeploymentState;
use alien_infra::extension_bundle::{host_target, testing::TestBundle};
use alien_manager::auth::Subject;
use alien_manager::extension_bundles::{ExtensionBundles, LocalControllers};
use alien_manager::providers::StateDirExtensionBundles;
use alien_manager::traits::ExtensionBundleStore;
use sha2::{Digest, Sha256};

const PINNED: &[u8] = b"#!/bin/sh\nexit 0\n";

fn deployment_on(bundle: &TestBundle) -> DeploymentState {
    serde_json::from_value(serde_json::json!({
        "platform": "local", "status": "running",
        "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "currentRelease": {"releaseId": "rel", "stack": {"id": "app", "resources": {}, "extensions": [bundle.manifest]}}
    }))
    .unwrap()
}

async fn local_bundles(bundle: &TestBundle, dir: &std::path::Path) -> ExtensionBundles {
    let store: Arc<dyn ExtensionBundleStore> =
        Arc::new(StateDirExtensionBundles::new(dir.join("store")));
    let hex = format!("{:x}", Sha256::digest(&bundle.zip));
    store
        .put(&Subject::system(), &hex, bundle.zip.clone())
        .await
        .unwrap();
    let binary = dir.join("acme-controller");
    std::fs::write(&binary, PINNED).unwrap();
    ExtensionBundles::new(store).with_local_controllers(LocalControllers::new(
        Arc::new(move |_: &str, _: &str| Ok(Some(binary.clone()))),
        Vec::new(),
        dir.join("copies"),
    ))
}

#[tokio::test]
async fn a_local_process_wins_over_the_controller_server() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let dir = tempfile::tempdir().unwrap();
    let bundles = local_bundles(&bundle, dir.path()).await;
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let record = asked.clone();
    bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&bundle),
            move |extension, _| {
                record.lock().unwrap().push(extension.name.clone());
                None
            },
        )
        .await
        .unwrap();
    assert!(
        asked.lock().unwrap().is_empty(),
        "the push controller was asked for an extension with a local process: {:?}",
        asked.lock().unwrap()
    );
}

#[tokio::test]
async fn a_bundle_without_a_local_controller_asks_for_no_server() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let dir = tempfile::tempdir().unwrap();
    let bundles = local_bundles(&bundle, dir.path()).await;
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let record = asked.clone();
    bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&bundle),
            move |extension, _| {
                record.lock().unwrap().push(extension.name.clone());
                None
            },
        )
        .await
        .unwrap();
    assert!(
        asked.lock().unwrap().is_empty(),
        "alien dev asked for a controller server: {:?}",
        asked.lock().unwrap()
    );
}
