//! `step` while a deployment moves between releases that list different extensions. Each
//! release is checked against the provider that loaded it: the target release against the
//! provider itself, the current release against the provider loaded for it with
//! `with_current_release`. Updates and deletions go through with that provider, and are refused,
//! naming the release, when a provider is missing.

mod support;

use alien_aws_clients::AwsClientConfigExt as _;
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Platform, ReleaseInfo, RuntimeMetadata, Stack, StackSettings,
    StackState,
};
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use std::sync::Arc;

fn ext(name: &str, version: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name, "version": version, "digest": format!("sha256:{name}{version}"), "resourceTypes": {}
    })
}

fn stack(extensions: &[(&str, &str)]) -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "extensions": extensions.iter().map(|(n, v)| ext(n, v)).collect::<Vec<_>>()
    }))
    .unwrap()
}

fn loaded(names: &[&str]) -> Arc<LoadedExtensions> {
    Arc::new(names.iter().fold(LoadedExtensions::default(), |l, n| {
        l.with_hooks(*n, alien_deployment::testing::no_hooks(n))
    }))
}

fn names<'a>(e: &'a [(&'a str, &'a str)]) -> Vec<&'a str> {
    e.iter().map(|(n, _)| *n).collect()
}

fn release(id: &str, stack: Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(id.to_string()),
        version: None,
        description: None,
        stack,
    }
}

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build()
}

enum Shape {
    TargetOnly,
    TargetAndCurrent,
    CurrentOnly,
    LoadedNothing,
}

async fn step(
    status: DeploymentStatus,
    from: &[(&str, &str)],
    to: &[(&str, &str)],
    shape: Shape,
) -> Result<DeploymentStatus, String> {
    let (current, target) = (stack(from), stack(to));
    let provider = match shape {
        Shape::TargetOnly => DefaultPlatformServiceProvider::default()
            .with_extensions(&target, loaded(&names(to)))
            .unwrap(),
        Shape::TargetAndCurrent => DefaultPlatformServiceProvider::default()
            .with_extensions(&target, loaded(&names(to)))
            .unwrap()
            .with_current_release(&current, loaded(&names(from)))
            .unwrap(),
        Shape::CurrentOnly => DefaultPlatformServiceProvider::default()
            .with_extensions(&current, loaded(&names(from)))
            .unwrap(),
        Shape::LoadedNothing => DefaultPlatformServiceProvider::default(),
    };
    let provider: Arc<dyn PlatformServiceProvider> = Arc::new(provider);
    let state = DeploymentState {
        status,
        platform: Platform::Aws,
        current_release: Some(release("rel_1", current.clone())),
        target_release: Some(release("rel_2", target.clone())),
        stack_state: Some(StackState::with_resource_prefix(
            Platform::Aws,
            "test".to_string(),
        )),
        error: None,
        environment_info: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        runtime_metadata: Some(RuntimeMetadata {
            prepared_stack: Some(current),
            pending_prepared_stack: Some(target),
            ..Default::default()
        }),
    };
    crate::support::step(
        state,
        config(),
        ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
        Some(provider),
    )
    .await
    .map(|r| r.state.status)
    .map_err(|e| format!("{e} | code={} | {e:?}", e.code))
}

/// A release as the (extension, version) pairs its stack lists.
type Release = &'static [(&'static str, &'static str)];

/// Release pairs an update can move between.
const PAIRS: [(Release, Release); 6] = [
    (&[("acme", "1")], &[("acme", "1")]),
    (&[("acme", "1")], &[("backups", "1")]),
    (&[], &[("acme", "1")]),
    (&[("acme", "1")], &[]),
    (&[("acme", "1")], &[("acme", "1"), ("backups", "1")]),
    (&[("acme", "1")], &[("acme", "2")]),
];

/// With both releases loaded, an update starts from Running and UpdatePending and finishes from
/// Updating, whichever extensions the two releases list.
#[tokio::test]
async fn an_update_between_any_two_releases_goes_through() {
    for (status, next) in [
        (DeploymentStatus::Running, DeploymentStatus::Updating),
        (DeploymentStatus::UpdatePending, DeploymentStatus::Updating),
        (DeploymentStatus::Updating, DeploymentStatus::Running),
    ] {
        for (from, to) in PAIRS {
            let out = step(status, from, to, Shape::TargetAndCurrent).await;
            assert_eq!(out, Ok(next), "{status:?} {from:?} -> {to:?}");
        }
    }
}

/// With only the target release loaded, an update whose current release lists other extensions
/// is refused, naming the current release; one whose current release lists the same or none
/// goes through.
#[tokio::test]
async fn an_update_without_the_current_releases_provider_is_refused_when_it_needs_one() {
    for status in [
        DeploymentStatus::Running,
        DeploymentStatus::UpdatePending,
        DeploymentStatus::Updating,
    ] {
        for (from, to) in PAIRS {
            let out = step(status, from, to, Shape::TargetOnly).await;
            if from == to || from.is_empty() {
                assert!(out.is_ok(), "{status:?} {from:?} -> {to:?}: {out:?}");
            } else {
                let error = out.expect_err("no provider for the current release");
                assert!(
                    error.contains("current release"),
                    "{status:?} {from:?} -> {to:?}: {error}"
                );
            }
        }
    }
}

/// acme v1 -> acme v2 (same name, new digest): the provider that loaded only the target release
/// must not serve the current release.
#[tokio::test]
async fn same_extension_new_digest_needs_the_current_release() {
    let err = step(
        DeploymentStatus::Updating,
        &[("acme", "1")],
        &[("acme", "2")],
        Shape::TargetOnly,
    )
    .await
    .expect_err("v1 -> v2 without the current release's provider");
    assert!(
        err.contains("The current release rel_1 uses extensions [\"acme 1 (sha256:acme1)\"]"),
        "{err}"
    );
    let ok = step(
        DeploymentStatus::Updating,
        &[("acme", "1")],
        &[("acme", "2")],
        Shape::TargetAndCurrent,
    )
    .await;
    assert_eq!(ok, Ok(DeploymentStatus::Running));
}

/// A provider that loaded nothing is refused in every status.
#[tokio::test]
async fn a_provider_that_loaded_nothing_is_refused_in_every_status() {
    use DeploymentStatus::*;
    for status in [
        Pending,
        InitialSetup,
        Provisioning,
        Running,
        UpdatePending,
        Updating,
        DeletePending,
        Deleting,
        DeleteFailed,
        UpdateFailed,
        ProvisioningFailed,
        PreflightsFailed,
    ] {
        let err = step(
            status,
            &[("acme", "1")],
            &[("acme", "1")],
            Shape::LoadedNothing,
        )
        .await
        .expect_err("refused");
        assert!(err.contains("EXTENSIONS_NOT_LOADED"), "{status:?}: {err}");
    }
}

/// Deleting a deployment whose update to a release with other extensions did not finish: its
/// resources belong to the current release, so the deletion runs with the provider loaded for it,
/// and is refused when either release's provider is missing.
#[tokio::test]
async fn delete_during_an_extension_swap() {
    for status in [DeploymentStatus::DeletePending, DeploymentStatus::Deleting] {
        let out = step(
            status,
            &[("acme", "1")],
            &[("backups", "1")],
            Shape::TargetAndCurrent,
        )
        .await;
        assert!(out.is_ok(), "{status:?}: {out:?}");
        for (shape, release) in [
            (Shape::TargetOnly, "current release"),
            (Shape::CurrentOnly, "target release"),
        ] {
            let error = step(status, &[("acme", "1")], &[("backups", "1")], shape)
                .await
                .expect_err("a release without its provider");
            assert!(error.contains(release), "{status:?}: {error}");
        }
    }
}
