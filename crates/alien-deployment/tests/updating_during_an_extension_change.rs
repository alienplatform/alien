//! `step` in Updating deletes a resource the target release removes through the current
//! release's interceptors, on the wire.
//!
//! The installed stack state holds a Live AWS bucket that the target's prepared stack omits, so
//! the update deletes it. The current release's extension traps on every AWS call; the
//! target's has no hooks.

mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Platform, ReleaseInfo, Resource, ResourceLifecycle,
    ResourceStatus, RuntimeMetadata, Stack, StackResourceState, StackSettings, StackState, Storage,
};
use alien_infra::{
    AwsStorageController, DefaultPlatformServiceProvider, LoadedExtensions,
    PlatformServiceProvider, StackResourceStateExt as _,
};
use std::collections::HashMap;
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

fn loaded(names: &[&str], trap: &[&str]) -> Arc<LoadedExtensions> {
    Arc::new(names.iter().fold(LoadedExtensions::default(), |l, n| {
        let module = if trap.contains(n) {
            alien_deployment::testing::hooks_module(
                n,
                r#"[{"name":"all","kind":"awsInterceptor"}]"#,
                "unreachable",
            )
        } else {
            alien_deployment::testing::no_hooks(n)
        };
        l.with_hooks(*n, module)
    }))
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

fn aws(base: &str) -> ClientConfig {
    ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "test-access".into(),
            secret_access_key: "test-secret".into(),
            session_token: None,
        },
        service_overrides: Some(alien_aws_clients::ServiceOverrides {
            endpoints: HashMap::from([("s3".to_string(), base.to_string())]),
        }),
    }))
}

fn with_bucket() -> StackState {
    let mut state = StackState::with_resource_prefix(Platform::Aws, "owned".to_string());
    let mut resource = StackResourceState::new_pending(
        "storage".into(),
        Resource::new(Storage::new("data".into()).build()),
        None,
        vec![],
    );
    resource.lifecycle = Some(ResourceLifecycle::Live);
    resource.status = ResourceStatus::Running;
    resource
        .set_internal_controller(Some(Box::new(AwsStorageController::default())))
        .unwrap();
    resource.internal_state.as_mut().unwrap()["bucketName"] = "owned-data".into();
    state.resources.insert("data".into(), resource);
    serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
}

async fn update_step(
    from: &[(&str, &str)],
    to: &[(&str, &str)],
    trap_current: &[&str],
    trap_target: &[&str],
) -> (Result<DeploymentState, String>, usize) {
    let server = httpmock::MockServer::start_async().await;
    let any = server
        .mock_async(|_when, then| {
            then.status(404)
                .header("content-type", "application/xml")
                .body("<Error><Code>NoSuchBucket</Code><Message>gone</Message></Error>");
        })
        .await;
    let (current, target) = (stack(from), stack(to));
    let names = |e: &[(&str, &str)]| e.iter().map(|(n, _)| n.to_string()).collect::<Vec<_>>();
    let (from_names, to_names) = (names(from), names(to));
    let from_names: Vec<&str> = from_names.iter().map(String::as_str).collect();
    let to_names: Vec<&str> = to_names.iter().map(String::as_str).collect();
    let provider: Arc<dyn PlatformServiceProvider> = Arc::new(
        DefaultPlatformServiceProvider::default()
            .with_extensions(&target, loaded(&to_names, trap_target))
            .unwrap()
            .with_current_release(&current, loaded(&from_names, trap_current))
            .unwrap(),
    );
    let state = DeploymentState {
        status: DeploymentStatus::Updating,
        platform: Platform::Aws,
        current_release: Some(release("rel_1", current.clone())),
        target_release: Some(release("rel_2", target.clone())),
        stack_state: Some(with_bucket()),
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
    let mut out = Ok(state);
    // A few steps: the bucket's delete flow may take more than one.
    for _ in 0..4 {
        let Ok(state) = out else { break };
        if state.status != DeploymentStatus::Updating {
            out = Ok(state);
            break;
        }
        out = crate::support::step(
            state,
            config(),
            aws(&server.base_url()),
            Some(provider.clone()),
        )
        .await
        .map(|r| r.state)
        .map_err(|e| format!("{e} | code={} | {e:?}", e.code));
        if let Ok(state) = &out {
            if let Some(error) = &state.error {
                out = Err(format!("recorded: {error:?}"));
            } else if state
                .stack_state
                .as_ref()
                .is_some_and(|s| s.resources.get("data").is_some_and(|r| r.error.is_some()))
            {
                out = Err(format!(
                    "resource error: {:?}",
                    state.stack_state.as_ref().unwrap().resources["data"].error
                ));
            }
        }
    }
    (out, any.hits_async().await)
}

/// acme (trapping) -> backups: deleting the bucket the target removes runs acme's interceptor,
/// and the S3 call is never sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_deletes_what_the_target_removes_with_the_current_releases_interceptors() {
    let (out, hits) = update_step(&[("acme", "1")], &[("backups", "1")], &["acme"], &[]).await;
    let err = out.expect_err("the current release's interceptor must fire");
    assert!(
        err.contains("INTERCEPTOR_FAILED") && err.contains("acme"),
        "{err}"
    );
    assert_eq!(hits, 0);
}

/// backups -> acme (trapping): the target's interceptor never runs on the removed bucket.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_never_deletes_what_the_target_removes_with_the_targets_interceptors() {
    let (out, hits) = update_step(&[("backups", "1")], &[("acme", "1")], &[], &["acme"]).await;
    if let Err(err) = &out {
        assert!(!err.contains("INTERCEPTOR_FAILED"), "{err}");
    }
    assert!(hits >= 1, "{out:?}");
}
