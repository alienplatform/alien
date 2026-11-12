//! Deleting a deployment while its update to another release is unfinished, on the wire.
//!
//! A retained Live AWS bucket makes `ensure_data_protection`, and setup teardown's handoff
//! protection, send S3 GetBucketTagging through the provider the deletion picked. The current or
//! the target release carries an AWS interceptor that traps; which one fires shows which
//! release's interceptors the deletion ran.

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Platform, ReleaseInfo, Resource, ResourceLifecycle,
    ResourceStatus, RuntimeMetadata, Stack, StackResourceState, StackSettings, StackState, Storage,
};
use alien_deployment::loop_contract::LoopOperation;
use alien_deployment::runner::{DelayStrategy, RunnerPolicy};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;
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

/// `trap` names the extensions whose module traps on every AWS call it is asked about.
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

fn with_bucket(lifecycle: ResourceLifecycle) -> StackState {
    let mut state = StackState::with_resource_prefix(Platform::Aws, "owned".to_string());
    let mut resource = StackResourceState::new_pending(
        "storage".into(),
        Resource::new(Storage::new("data".into()).build()),
        None,
        vec![],
    );
    resource.lifecycle = Some(lifecycle);
    resource.status = ResourceStatus::Running;
    resource
        .set_internal_controller(Some(Box::new(AwsStorageController::default())))
        .unwrap();
    resource.internal_state.as_mut().unwrap()["bucketName"] = "owned-data".into();
    state.resources.insert("data".into(), resource);
    serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    TargetOnly,
    TargetAndCurrent,
    CurrentOnly,
    LoadedNothing,
}

fn provider(
    shape: Shape,
    from: &[(&str, &str)],
    to: &[(&str, &str)],
    trap_current: &[&str],
    trap_target: &[&str],
) -> Arc<dyn PlatformServiceProvider> {
    let (current, target) = (stack(from), stack(to));
    let p = match shape {
        Shape::TargetOnly => DefaultPlatformServiceProvider::default()
            .with_extensions(&target, loaded(&names(to), trap_target))
            .unwrap(),
        Shape::TargetAndCurrent => DefaultPlatformServiceProvider::default()
            .with_extensions(&target, loaded(&names(to), trap_target))
            .unwrap()
            .with_current_release(&current, loaded(&names(from), trap_current))
            .unwrap(),
        Shape::CurrentOnly => DefaultPlatformServiceProvider::default()
            .with_extensions(&current, loaded(&names(from), trap_current))
            .unwrap(),
        Shape::LoadedNothing => DefaultPlatformServiceProvider::default(),
    };
    Arc::new(p)
}

fn state(
    status: DeploymentStatus,
    from: Option<&[(&str, &str)]>,
    to: Option<&[(&str, &str)]>,
    stack_state: StackState,
) -> DeploymentState {
    DeploymentState {
        status,
        platform: Platform::Aws,
        current_release: from.map(|f| release("rel_1", stack(f))),
        target_release: to.map(|t| release("rel_2", stack(t))),
        stack_state: Some(stack_state),
        error: None,
        environment_info: None,
        retry_requested: status == DeploymentStatus::DeleteFailed,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        runtime_metadata: Some(RuntimeMetadata {
            prepared_stack: from.map(stack),
            pending_prepared_stack: to.map(stack),
            ..Default::default()
        }),
    }
}

struct NoopTransport;

#[async_trait::async_trait]
impl DeploymentLoopTransport for NoopTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        _state: &DeploymentState,
        _config: &DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        _heartbeats: Vec<alien_core::ResourceHeartbeat>,
        _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        Ok(StepReconcileResult {
            state: None,
            config: None,
        })
    }
}

/// Runs one deletion path; returns Ok(()) or the error rendered with its chain.
async fn run(
    path: &str,
    mut state: DeploymentState,
    client: ClientConfig,
    provider: Arc<dyn PlatformServiceProvider>,
) -> Result<(), String> {
    if path == "teardown" {
        state.status = DeploymentStatus::TeardownRequired;
        let mut config = config();
        alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &client,
            "dep_1",
            &RunnerPolicy {
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Yield,
                max_steps: 1,
            },
            &NoopTransport,
            Some(provider),
        )
        .await
        .map_err(|e| format!("{e} | code={} | {e:?}", e.code))?;
        match &state.error {
            Some(error) => Err(format!("teardown recorded: {error:?}")),
            None => Ok(()),
        }
    } else {
        alien_deployment::step(state, config(), client, Some(provider))
            .await
            .map(|_| ())
            .map_err(|e| format!("{e} | code={} | {e:?}", e.code))
    }
}

const PATHS: [(&str, DeploymentStatus); 4] = [
    ("step", DeploymentStatus::DeletePending),
    ("step", DeploymentStatus::Deleting),
    ("step", DeploymentStatus::DeleteFailed),
    ("teardown", DeploymentStatus::TeardownRequired),
];

async fn s3_server() -> httpmock::MockServer {
    httpmock::MockServer::start_async().await
}

async fn s3_404(server: &httpmock::MockServer) -> httpmock::Mock<'_> {
    server
        .mock_async(|_when, then| {
            then.status(404)
                .header("content-type", "application/xml")
                .body("<Error><Code>NoSuchBucket</Code><Message>gone</Message></Error>");
        })
        .await
}

/// Current release acme (trapping) -> target backups: every deletion path runs acme's
/// interceptor on the S3 call, and the call is never sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_during_an_unfinished_update_runs_the_current_releases_interceptors() {
    let from: &[(&str, &str)] = &[("acme", "1")];
    for to in [
        &[("backups", "1")][..],
        &[("acme", "2")][..],
        &[("acme", "1")][..],
    ] {
        for (path, status) in PATHS {
            let server = s3_server().await;
            let any = s3_404(&server).await;
            let lifecycle = ResourceLifecycle::Live;
            let out = run(
                path,
                state(status, Some(from), Some(to), with_bucket(lifecycle)),
                aws(&server.base_url()),
                provider(
                    Shape::TargetAndCurrent,
                    from,
                    to,
                    &["acme"],
                    if to == [("acme", "1")] {
                        &["acme"]
                    } else {
                        &[]
                    },
                ),
            )
            .await;
            let hits = any.hits_async().await;
            let err = out.expect_err(&format!(
                "{path} {status:?} -> {to:?}: interceptor must fire"
            ));
            assert!(
                err.contains("INTERCEPTOR_FAILED") && err.contains("acme"),
                "{path} {status:?} -> {to:?}: {err}"
            );
            assert_eq!(hits, 0, "{path} {status:?} -> {to:?}");
        }
    }
}

/// Current release backups (no hooks) or none -> target acme (trapping): deletion never runs
/// the target's interceptor; the S3 call is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_during_an_unfinished_update_never_runs_the_target_releases_interceptors() {
    let to: &[(&str, &str)] = &[("acme", "1")];
    for from in [&[("backups", "1")][..], &[][..]] {
        for (path, status) in PATHS {
            let server = s3_server().await;
            let any = s3_404(&server).await;
            let out = run(
                path,
                state(
                    status,
                    Some(from),
                    Some(to),
                    with_bucket(ResourceLifecycle::Live),
                ),
                aws(&server.base_url()),
                provider(Shape::TargetAndCurrent, from, to, &[], &["acme"]),
            )
            .await;
            let hits = any.hits_async().await;
            if let Err(err) = &out {
                assert!(
                    !err.contains("INTERCEPTOR_FAILED"),
                    "{path} {status:?} {from:?}: {err}"
                );
            }
            assert!(hits >= 1, "{path} {status:?} {from:?}: {out:?}");
        }
    }
}

/// A release as the (extension, version) pairs its stack lists.
type Release = &'static [(&'static str, &'static str)];

/// Release pairs a deployment can be deleted between.
const PAIRS: [(Release, Release); 6] = [
    (&[("acme", "1")], &[("acme", "1")]),
    (&[("acme", "1")], &[("backups", "1")]),
    (&[], &[("acme", "1")]),
    (&[("acme", "1")], &[]),
    (&[("acme", "1")], &[("acme", "1"), ("backups", "1")]),
    (&[("acme", "1")], &[("acme", "2")]),
];

/// The extension check and the deletion agree: over every provider shape, release pair and
/// deletion path (on an empty stack state), a deletion is either let through or refused by the
/// check, naming a release; never refused later by the executor's own load ("Failed to load
/// the deployed stack's extensions").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_deletion_the_check_lets_through_runs() {
    let mut table = Vec::new();
    for shape in [
        Shape::TargetOnly,
        Shape::TargetAndCurrent,
        Shape::CurrentOnly,
        Shape::LoadedNothing,
    ] {
        for (from, to) in PAIRS {
            for releases in ["both", "current-only", "target-only"] {
                let (f, t) = match releases {
                    "both" => (Some(from), Some(to)),
                    "current-only" => (Some(from), None),
                    _ => (None, Some(to)),
                };
                for (path, status) in PATHS {
                    let out = run(
                        path,
                        state(
                            status,
                            f,
                            t,
                            StackState::with_resource_prefix(Platform::Aws, "test".into()),
                        ),
                        ClientConfig::Aws(Box::new(
                            <alien_aws_clients::AwsClientConfig as alien_aws_clients::AwsClientConfigExt>::mock(),
                        )),
                        provider(shape, from, to, &[], &[]),
                    )
                    .await;
                    if let Err(err) = &out {
                        assert!(
                            !err.contains("deployed stack's extensions"),
                            "the deletion refused what the check let through: {shape:?} {releases} {from:?}->{to:?} {path} {status:?}: {err}"
                        );
                        assert!(
                            err.contains("MISSING_CONFIGURATION") && err.contains("release"),
                            "{shape:?} {releases} {from:?}->{to:?} {path} {status:?}: {err}"
                        );
                    }
                    // A deletion goes through exactly when each release's extensions are among
                    // those of the provider that serves it: the provider passed in for the target
                    // release, and for the current one too when both list the same extensions
                    // (or the current one lists none); otherwise the current release needs a
                    // provider of its own.
                    let main: &[(&str, &str)] = match shape {
                        Shape::TargetOnly | Shape::TargetAndCurrent => to,
                        Shape::CurrentOnly => from,
                        Shape::LoadedNothing => &[],
                    };
                    // A provider's hooks are found by extension name.
                    let within = |listed: &[(&str, &str)], loaded: &[(&str, &str)]| {
                        listed
                            .iter()
                            .all(|(name, _)| loaded.iter().any(|(loaded, _)| loaded == name))
                    };
                    let target_served = t.is_none_or(|t| within(t, main));
                    let current_served = f.is_none_or(|f| {
                        let provider = if t.is_none_or(|t| t == f) || f.is_empty() {
                            Some(main)
                        } else if matches!(shape, Shape::TargetAndCurrent) {
                            Some(from)
                        } else {
                            None
                        };
                        provider.is_some_and(|loaded| within(f, loaded))
                    });
                    let expected = target_served && current_served;
                    assert_eq!(
                        out.is_ok(),
                        expected,
                        "{shape:?} {releases} {from:?}->{to:?} {path} {status:?}: {out:?}"
                    );
                    table.push(format!(
                        "{shape:?} {releases} {from:?}->{to:?} {path} {status:?}: {}",
                        if out.is_ok() { "ok" } else { "refused" }
                    ));
                }
            }
        }
    }
    assert_eq!(table.len(), 288);
}
