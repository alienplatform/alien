//! `alien dev`'s manager runs a local controller only once the binary on disk has the sha256 the
//! verified bundle pins.

use std::path::PathBuf;
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

/// The manager with `bundle` stored and local controllers found at `binary`, recording each
/// lookup's (name, sha256).
async fn bundles(
    bundle: &TestBundle,
    store_dir: &std::path::Path,
    binary: Option<PathBuf>,
    lookups: Arc<Mutex<Vec<(String, String)>>>,
) -> ExtensionBundles {
    let store: Arc<dyn ExtensionBundleStore> = Arc::new(StateDirExtensionBundles::new(store_dir));
    let hex = format!("{:x}", Sha256::digest(&bundle.zip));
    store
        .put(&Subject::system(), &hex, bundle.zip.clone())
        .await
        .unwrap();
    ExtensionBundles::new(store).with_local_controllers(LocalControllers::new(
        Arc::new(move |name: &str, sha: &str| {
            lookups
                .lock()
                .unwrap()
                .push((name.to_string(), sha.to_string()));
            Ok(binary.clone())
        }),
        Vec::new(),
        store_dir.join("copies"),
    ))
}

#[tokio::test]
async fn a_local_controller_with_the_pinned_sha256_is_accepted() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("acme-controller");
    std::fs::write(&binary, PINNED).unwrap();
    let lookups = Arc::default();
    let bundles = bundles(
        &bundle,
        &dir.path().join("store"),
        Some(binary),
        Arc::clone(&lookups),
    )
    .await;

    // Each tick builds the deployment's provider again; the process from the first is reused.
    for _ in 0..2 {
        bundles
            .provider_for(
                alien_infra::DefaultPlatformServiceProvider::default(),
                "default",
                "dep_1",
                &deployment_on(&bundle),
                |_, _| None,
            )
            .await
            .unwrap();
    }
    let lookups = lookups.lock().unwrap().clone();
    assert_eq!(lookups.len(), 1, "one lookup for the binary across ticks");
    assert_eq!(lookups[0].0, "acme");
    assert_eq!(lookups[0].1, format!("{:x}", Sha256::digest(PINNED)));
}

#[tokio::test]
async fn a_local_controller_whose_bytes_differ_from_the_pin_is_refused() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("acme-controller");
    std::fs::write(&binary, b"#!/bin/sh\necho swapped\n").unwrap();
    let bundles = bundles(
        &bundle,
        &dir.path().join("store"),
        Some(binary),
        Arc::default(),
    )
    .await;

    let refused = bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&bundle),
            |_, _| None,
        )
        .await;
    let error = refused
        .err()
        .expect("a binary that isn't the pinned one is refused");
    assert!(error.to_string().contains("acme"), "{error}");
}

#[tokio::test]
async fn a_missing_local_controller_is_refused_naming_the_extension() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let dir = tempfile::tempdir().unwrap();
    let bundles = bundles(&bundle, &dir.path().join("store"), None, Arc::default()).await;

    let error = bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&bundle),
            |_, _| None,
        )
        .await
        .err()
        .expect("no binary on this machine is refused");
    assert!(error.to_string().contains("Extension 'acme'"), "{error}");
}

/// A rebuilt controller, with new bytes, gets its own lookup and process; the process of the old
/// sha256 is never reused for it.
#[tokio::test]
async fn a_new_controller_sha256_gets_a_new_lookup() {
    const V2: &[u8] = b"#!/bin/sh\nexit 1\n";
    let v1 = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let v2 = TestBundle::new("acme", "1.0.1", &[(host_target().unwrap(), V2)]);
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn ExtensionBundleStore> =
        Arc::new(StateDirExtensionBundles::new(dir.path().join("store")));
    for bundle in [&v1, &v2] {
        let hex = format!("{:x}", Sha256::digest(&bundle.zip));
        store
            .put(&Subject::system(), &hex, bundle.zip.clone())
            .await
            .unwrap();
    }
    for (folder, bytes) in [("v1", PINNED), ("v2", V2)] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        std::fs::write(dir.path().join(folder).join("acme-controller"), bytes).unwrap();
    }
    let s1 = format!("{:x}", Sha256::digest(PINNED));
    let s2 = format!("{:x}", Sha256::digest(V2));
    let lookups: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = lookups.clone();
    let (root, first) = (dir.path().to_path_buf(), s1.clone());
    let bundles = ExtensionBundles::new(store).with_local_controllers(LocalControllers::new(
        Arc::new(move |_name: &str, sha: &str| {
            seen.lock().unwrap().push(sha.to_string());
            let folder = if sha == first { "v1" } else { "v2" };
            Ok(Some(root.join(folder).join("acme-controller")))
        }),
        Vec::new(),
        dir.path().join("copies"),
    ));

    for bundle in [&v1, &v1, &v2, &v2] {
        bundles
            .provider_for(
                alien_infra::DefaultPlatformServiceProvider::default(),
                "default",
                "dep_1",
                &deployment_on(bundle),
                |_, _| None,
            )
            .await
            .unwrap();
    }

    let lookups = lookups.lock().unwrap().clone();
    assert_eq!(lookups, vec![s1, s2], "one lookup per controller sha256");
}

/// The private copy goes under the folder the manager is given, not beside the binary, so a
/// rebuild that empties the binary's folder leaves it.
#[tokio::test]
async fn a_local_controllers_copy_goes_outside_the_binarys_folder() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host_target().unwrap(), PINNED)]);
    let dir = tempfile::tempdir().unwrap();
    let built = dir.path().join("target/alien-extension");
    std::fs::create_dir_all(&built).unwrap();
    let binary = built.join("acme-controller");
    std::fs::write(&binary, PINNED).unwrap();
    let bundles = bundles(
        &bundle,
        &dir.path().join("store"),
        Some(binary),
        Arc::default(),
    )
    .await;

    bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&bundle),
            |_, _| None,
        )
        .await
        .unwrap();

    let names = |folder: &std::path::Path| -> Vec<String> {
        std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(names(&built), ["acme-controller"]);
    let copies = names(&dir.path().join("store/copies"));
    assert!(
        copies.len() == 1 && copies[0].starts_with(".run-"),
        "{copies:?}"
    );
}
