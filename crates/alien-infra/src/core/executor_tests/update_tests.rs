//! Tests for resource update flows and config changes.

use super::helpers::*;
use crate::core::{MockPlatformServiceProvider, StackExecutor, StackStateExt};
use crate::error::Result;
use alien_core::{
    ClientConfig, ComputeCluster, ComputePoolSelection, ComputeSettings, Container, ContainerCode,
    DeploymentConfig, KubernetesClientConfig, Platform, Resource, ResourceLifecycle, ResourceRef,
    ResourceSpec, ResourceStatus, Stack, StackResourceState, StackState,
};
use alien_error::AlienError;
use alien_k8s_clients::kubernetes::deployments::MockDeploymentApi;
use alien_preflights::{mutations::ComputeClusterMutation, runner::PreflightRunner, StackMutation};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Tests that a config change triggers an update.
#[tokio::test]
async fn test_config_change_triggers_update() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("update-test".to_owned())
        .add(func1_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_eq!(
        get_status(&state_after_v1, "func1"),
        Some(ResourceStatus::Running)
    );

    let func1_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("update-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Running)
    );
    Ok(())
}

/// Refresh observes the persisted deployment; it must not turn desired release
/// drift into an update.
#[tokio::test]
async fn test_refresh_does_not_plan_config_changes() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");
    let stack_v1 = Stack::new("refresh-test".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();
    let state = run_to_synced(&new_executor(&stack_v1)?, new_test_state()).await?;

    let stack_v2 = Stack::new("refresh-test".to_owned())
        .add(
            test_function_with_image("func1", "image-v2"),
            ResourceLifecycle::Live,
        )
        .add(test_storage("new-storage"), ResourceLifecycle::Frozen)
        .build();
    let refreshed = new_executor(&stack_v2)?.refresh(state).await?.next_state;

    assert_eq!(
        refreshed.resources.len(),
        1,
        "refresh must not create desired resources missing from persisted state"
    );
    let resource = refreshed.resources.get("func1").unwrap();
    assert_eq!(resource.status, ResourceStatus::Running);
    assert_eq!(
        resource.config,
        Resource::new(func_v1),
        "refresh must retain the deployed config instead of applying desired drift"
    );

    Ok(())
}

#[tokio::test]
async fn imported_continuation_preserves_complete_truthful_state() -> Result<()> {
    let dependency = test_storage("dependency");
    let dependent = test_storage("dependent");
    let stack = Stack::new("imported-continuation".to_owned())
        .add(dependency, ResourceLifecycle::Frozen)
        .add_with_dependencies(
            dependent,
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new("storage".into(), "dependency")],
        )
        .build();
    let executor = new_executor(&stack)?;
    let state = run_to_synced(&executor, new_test_state()).await?;

    let continued = executor.continue_imported(state).await?.next_state;

    assert_eq!(
        continued.resources["dependent"].dependencies,
        vec![ResourceRef::new("storage".into(), "dependency")]
    );
    assert!(continued
        .resources
        .values()
        .all(|resource| resource.status == ResourceStatus::Running));
    Ok(())
}

#[tokio::test]
async fn imported_continuation_refuses_to_initialize_pending_resource() -> Result<()> {
    let storage = test_storage("assets");
    let stack = Stack::new("imported-pending".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let executor = new_executor(&stack)?;
    let mut state = new_test_state();
    state.resources.insert(
        "assets".to_string(),
        StackResourceState::new_pending(
            "storage".to_string(),
            Resource::new(storage),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        ),
    );

    let error = executor
        .continue_imported(state)
        .await
        .expect_err("imported Pending state must not initialize a controller");

    assert!(error
        .message
        .contains("cannot continue from status Pending"));
    assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
    assert!(!error.retryable);
    assert!(!error.internal);
    Ok(())
}

#[tokio::test]
async fn imported_continuation_refuses_missing_setup_resource() -> Result<()> {
    let stack = Stack::new("imported-missing".to_owned())
        .add(test_storage("assets"), ResourceLifecycle::Frozen)
        .build();

    let error = new_executor(&stack)?
        .continue_imported(new_test_state())
        .await
        .expect_err("an incomplete setup handoff must fail");

    assert!(error.message.contains("missing resource 'assets'"));
    assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
    assert!(!error.retryable);
    assert!(!error.internal);
    Ok(())
}

/// Built the way `initial_setup.rs` builds it for an imported handoff.
fn new_imported_setup_executor(stack: &Stack) -> Result<StackExecutor> {
    StackExecutor::builder(stack, ClientConfig::Test)
        .deployment_config(&default_deployment_config())
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .step_running_resources(false)
        .step_out_of_scope_resources(false)
        .build()
}

fn entry(resource: Resource, lifecycle: ResourceLifecycle) -> alien_core::ResourceEntry {
    alien_core::ResourceEntry {
        config: resource,
        lifecycle,
        dependencies: Vec::new(),
        remote_access: false,
        enabled_when: None,
    }
}

/// A stack of one Frozen storage plus `extra`, and imported state that holds the synced storage
/// and `imported` for `extra`.
async fn imported_beside_storage(
    extra: (&str, alien_core::ResourceEntry),
    imported: StackResourceState,
) -> Result<(Stack, StackState)> {
    let mut stack = Stack::new("imported".to_owned())
        .add(test_storage("assets"), ResourceLifecycle::Frozen)
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    state.resources.insert(extra.0.to_string(), imported);
    stack.resources.insert(extra.0.to_string(), extra.1);
    Ok((stack, state))
}

#[cfg(feature = "aws")]
fn live_sandbox_entry() -> alien_core::ResourceEntry {
    let sandbox = alien_core::Sandbox::new("agents".to_string())
        .code(alien_core::SandboxCode::Image {
            image: "s3://example-artifacts/agents/bundle.zip".to_string(),
        })
        .egress(alien_core::SandboxEgress::Allow)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    entry(Resource::new(sandbox), ResourceLifecycle::Live)
}

/// The record the AWS importer builds from a sandbox registration, so these tests follow the
/// importer if what it produces changes.
#[cfg(feature = "aws")]
fn import_aws_sandbox(
    entry: &alien_core::ResourceEntry,
    image: Option<(&str, &str, &str)>,
) -> StackResourceState {
    use crate::import::ResourceImporter;

    let settings = alien_core::StackSettings::default();
    let ctx = alien_core::import::ImportContext {
        resource_id: "agents",
        platform: Platform::Aws,
        region: "us-east-2",
        stack_settings: &settings,
        management_config: None,
        resource: entry,
    };
    let data = alien_core::import::data::AwsSandboxImportData {
        image_identifier: image.map(|(identifier, _, _)| identifier.to_string()),
        image_arn: image.map(|(_, arn, _)| arn.to_string()),
        image_version: image.map(|(_, _, version)| version.to_string()),
        build_role_arn: Some("arn:aws:iam::123456789012:role/agents-build".to_string()),
        bundle_uri: Some("s3://example-artifacts/agents/bundle.zip".to_string()),
        egress_connector_arns: Vec::new(),
        preview_ports: Vec::new(),
        allow_egress: true,
    };
    crate::sandbox::AwsSandboxImporter
        .import(data, &ctx)
        .expect("the AWS sandbox registration imports")
}

/// A Live sandbox's build role is rendered by setup, so the import carries the sandbox outside
/// the Frozen filter. Setup must leave it for the runtime controller rather than refuse the
/// whole handoff.
#[cfg(feature = "aws")]
#[tokio::test]
async fn imported_continuation_leaves_a_registered_live_sandbox_to_runtime() -> Result<()> {
    let sandbox = live_sandbox_entry();
    let imported = import_aws_sandbox(&sandbox, None);
    let (stack, state) = imported_beside_storage(("agents", sandbox), imported).await?;

    let continued = new_imported_setup_executor(&stack)?
        .continue_imported(state)
        .await?
        .next_state;

    assert_eq!(
        continued.resources["assets"].status,
        ResourceStatus::Running
    );
    assert_eq!(
        continued.resources["agents"].status,
        ResourceStatus::Provisioning,
        "initial setup must neither refuse nor advance the Live sandbox"
    );
    Ok(())
}

/// A Live sandbox registered with its image already built claims an image the setup stack owns;
/// its runtime controller would then update or delete it.
#[cfg(feature = "aws")]
#[tokio::test]
async fn imported_continuation_refuses_a_live_sandbox_registered_as_built() -> Result<()> {
    let sandbox = live_sandbox_entry();
    let imported = import_aws_sandbox(
        &sandbox,
        Some((
            "agents",
            "arn:aws:lambda:us-east-2:123456789012:microvm-image:agents",
            "1",
        )),
    );
    let (stack, state) = imported_beside_storage(("agents", sandbox), imported).await?;

    let error = new_imported_setup_executor(&stack)?
        .continue_imported(state)
        .await
        .expect_err("a Live sandbox setup claims to have built must fail the handoff");

    assert!(error.message.contains("unexpected resource 'agents'"));
    assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
    Ok(())
}

/// Setup never renders a Worker, so a registered one is not something setup did, even in the
/// shape a registered Live sandbox has.
#[tokio::test]
async fn imported_continuation_refuses_a_live_resource_setup_does_not_render() -> Result<()> {
    let worker = Stack::new("worker".to_owned())
        .add(test_function("func"), ResourceLifecycle::Live)
        .build();
    let synced = run_to_synced(&new_executor(&worker)?, new_test_state()).await?;
    let mut imported = synced.resources["func"].clone();
    imported.status = ResourceStatus::Provisioning;
    let (stack, state) = imported_beside_storage(
        (
            "func",
            entry(
                Resource::new(test_function("func")),
                ResourceLifecycle::Live,
            ),
        ),
        imported,
    )
    .await?;

    let error = new_imported_setup_executor(&stack)?
        .continue_imported(state)
        .await
        .expect_err("a registered Worker must fail the handoff");

    assert!(error.message.contains("unexpected resource 'func'"));
    assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
    Ok(())
}

#[tokio::test]
async fn imported_continuation_refuses_a_resource_outside_the_stack() -> Result<()> {
    let stack = Stack::new("imported-stray".to_owned())
        .add(test_storage("assets"), ResourceLifecycle::Frozen)
        .build();
    let mut state = run_to_synced(&new_executor(&stack)?, new_test_state()).await?;
    state.resources.insert(
        "stray".to_string(),
        create_running_function_state("stray", "image"),
    );

    let error = new_imported_setup_executor(&stack)?
        .continue_imported(state)
        .await
        .expect_err("a resource the stack does not declare must fail the handoff");

    assert!(error.message.contains("unexpected resource 'stray'"));
    assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
    Ok(())
}

/// Tests that config changes while a resource is still provisioning do not
/// interrupt the in-flight create. The update should happen after create reaches
/// a stable state.
#[tokio::test]
async fn test_config_change_during_provisioning_waits_for_stable_state() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("provisioning-config-change-test".to_owned())
        .add(func_v1, ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    for _ in 0..3 {
        state = executor_v1.step(state).await?.next_state;
        if get_status(&state, "func1") == Some(ResourceStatus::Provisioning) {
            break;
        }
    }

    assert_eq!(
        get_status(&state, "func1"),
        Some(ResourceStatus::Provisioning)
    );

    let func_v2 = test_function_with_image("func1", "image-v2");
    let stack_v2 = Stack::new("provisioning-config-change-test".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let next_state = executor_v2.step(state).await?.next_state;

    assert_eq!(
        get_status(&next_state, "func1"),
        Some(ResourceStatus::Provisioning),
        "config drift during provisioning must not transition to delete"
    );

    let final_state = run_to_synced(&executor_v2, next_state).await?;
    let final_resource = final_state.resources.get("func1").unwrap();

    assert_eq!(
        final_resource.status,
        ResourceStatus::Running,
        "resource should finish create and reconcile to running"
    );
    assert_eq!(
        final_resource.config,
        Resource::new(func_v2),
        "stable resource should reconcile to the latest desired config"
    );

    Ok(())
}

/// Tests adding a new resource to an existing stack.
#[tokio::test]
async fn test_add_resource_to_existing_stack() -> Result<()> {
    let func_a = test_function("func-a");

    let stack_v1 = Stack::new("add-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a"]);

    let func_b = test_function("func-b");

    let stack_v2 = Stack::new("add-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_all_running(&final_state, &["func-a", "func-b"]);
    Ok(())
}

/// Tests removing a resource from an existing stack.
#[tokio::test]
async fn test_remove_resource_from_existing_stack() -> Result<()> {
    let func_a = test_function("func-a");
    let func_b = test_function("func-b");

    let stack_v1 = Stack::new("remove-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a", "func-b"]);

    let stack_v2 = Stack::new("remove-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func-a"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func-b"),
        Some(ResourceStatus::Deleted)
    );
    Ok(())
}

/// Tests combined add, update, and remove in single stack change.
#[tokio::test]
async fn test_combined_add_update_remove() -> Result<()> {
    let func_a_v1 = test_function_with_image("func-a", "image-a-v1");
    let func_b = test_function("func-b");

    let stack_v1 = Stack::new("combined-test".to_owned())
        .add(func_a_v1.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a", "func-b"]);

    let func_a_v2 = test_function_with_image("func-a", "image-a-v2");
    let func_c = test_function("func-c");

    let stack_v2 = Stack::new("combined-test".to_owned())
        .add(func_a_v2.clone(), ResourceLifecycle::Live)
        .add(func_c.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func-a"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func-b"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&final_state, "func-c"),
        Some(ResourceStatus::Running)
    );
    Ok(())
}

/// Cloud setup omits logical pools; continuation must verify the namespace
/// through their controller instead of inventing imported cloud fleet state.
#[tokio::test]
async fn imported_kubernetes_compute_retries_namespace_verification() -> Result<()> {
    let mut deployments = MockDeploymentApi::new();
    let allowed = Arc::new(AtomicBool::new(false));
    let access = allowed.clone();
    deployments
        .expect_list_deployments()
        .withf(|namespace, labels, fields| {
            namespace == "application"
                && labels.is_none()
                && fields.as_deref() == Some("metadata.name=alien-compute-access-check")
        })
        .times(2..)
        .returning(move |_, _, _| {
            if access.load(Ordering::SeqCst) {
                Ok(Default::default())
            } else {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteAccessDenied {
                        resource_type: "Deployment".to_string(),
                        resource_name: "application".to_string(),
                    },
                ))
            }
        });
    let deployments = Arc::new(deployments);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_kubernetes_deployment_client()
        .returning(move |_| Ok(deployments.clone()));
    let stack = Stack::new("imported-pools".to_string())
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(alien_core::CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: None,
                    profile: None,
                    min_size: 1,
                    max_size: 1,
                    scale_policy: None,
                    nested_virtualization: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let executor = StackExecutor::builder(
        &stack,
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::InCluster {
            namespace: Some("application".to_string()),
            additional_headers: None,
        })),
    )
    .deployment_config(&default_deployment_config())
    .service_provider(Arc::new(provider))
    .build()?;
    let mut state = StackState::new(Platform::Kubernetes);
    for _ in 0..24 {
        state = executor.continue_imported(state).await?.next_state;
        if state.resources["compute"].status == ResourceStatus::ProvisionFailed {
            break;
        }
    }
    assert_eq!(
        state.resources["compute"].status,
        ResourceStatus::ProvisionFailed
    );
    // Initial setup retries only after the caller requests a retry. Restore the
    // saved checkpoint exactly as the deployment engine does before continuation.
    assert!(executor.continue_imported(state.clone()).await.is_err());
    allowed.store(true, Ordering::SeqCst);
    let retried = state.retry_failed_with_lifecycle_filter(&[ResourceLifecycle::Frozen])?;
    assert_eq!(retried, vec!["compute".to_string()]);
    for _ in 0..8 {
        state = executor.continue_imported(state).await?.next_state;
        if state.resources["compute"].status == ResourceStatus::Running {
            break;
        }
    }
    assert_eq!(state.resources["compute"].status, ResourceStatus::Running);
    assert_eq!(
        state.resources["compute"].controller_platform,
        Some(Platform::Kubernetes)
    );
    assert!(state.resources["compute"].outputs.is_none());
    Ok(())
}

/// Exercise derived workload profiles through compatibility and the executor's real planner.
#[tokio::test]
async fn mutated_machine_change_passes_compatibility_and_plans_update() {
    let workload = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "test:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.25".to_string(),
            desired: "0.25".to_string(),
        })
        .memory(ResourceSpec {
            min: "256Mi".to_string(),
            desired: "256Mi".to_string(),
        })
        .ephemeral_storage("40Gi".to_string())
        .port(8080)
        .permissions("execution".to_string())
        .build();
    let release = Stack::new("machine-change".to_string())
        .add(workload, ResourceLifecycle::Live)
        .build();
    let installed = materialize_machine(release.clone(), "t4g.micro").await;
    let resized = materialize_machine(release.clone(), "t4g.small").await;
    let config = default_deployment_config();
    assert!(!config.allow_frozen_changes);
    let compatibility = PreflightRunner::new()
        .run_compatibility_checks(&installed, &resized, &config, Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(compatibility.success, "{compatibility:?}");

    let cluster = |stack: &Stack| {
        stack
            .resources
            .values()
            .find_map(|entry| entry.config.downcast_ref::<ComputeCluster>().cloned())
            .expect("mutation should produce a compute cluster")
    };
    let old_cluster = cluster(&installed);
    let new_cluster = cluster(&resized);
    assert_eq!(
        old_cluster.capacity_groups[0].instance_type.as_deref(),
        Some("t4g.micro")
    );
    assert_eq!(
        new_cluster.capacity_groups[0].instance_type.as_deref(),
        Some("t4g.small")
    );
    assert_eq!(
        new_cluster.capacity_groups[0]
            .profile
            .as_ref()
            .unwrap()
            .ephemeral_storage_bytes,
        40 * 1024 * 1024 * 1024
    );

    // The planner invokes ComputeCluster::validate_update independently of its controller.
    // Use the built-in Kubernetes controller for planning only; no provider is contacted.
    let mut state = StackState::new(Platform::Kubernetes);
    let mut resource = StackResourceState::new_pending(
        ComputeCluster::RESOURCE_TYPE.to_string(),
        Resource::new(old_cluster.clone()),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource.status = ResourceStatus::Running;
    state.resources.insert(old_cluster.id.clone(), resource);
    let plan = plan_machine_cluster(new_cluster, &state, &config)
        .expect("same-architecture machine should plan");
    assert_eq!(plan.updates.len(), 1);
    assert!(plan.updates.contains_key(&old_cluster.id));
    assert!(plan.creates.is_empty());
    assert!(plan.deletes.is_empty());

    let cross_arch = materialize_machine(release, "m7i.large").await;
    let compatibility = PreflightRunner::new()
        .run_compatibility_checks(&installed, &cross_arch, &config, Platform::Aws)
        .await
        .expect("compatibility checks should run");
    assert!(!compatibility.success);
    assert!(compatibility
        .results
        .iter()
        .flat_map(|result| &result.errors)
        .any(|error| error.contains("same CPU architecture")));
    let error = plan_machine_cluster(cluster(&cross_arch), &state, &config)
        .expect_err("planner must also refuse a cross-architecture machine");
    assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
}

async fn materialize_machine(stack: Stack, machine: &str) -> Stack {
    let mut config = default_deployment_config();
    config.stack_settings.compute = Some(ComputeSettings {
        containers: Default::default(),
        pools: [(
            "general".to_string(),
            ComputePoolSelection::Fixed {
                machines: 1,
                machine: Some(machine.to_string()),
                failure_domains: None,
            },
        )]
        .into_iter()
        .collect(),
    });
    ComputeClusterMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("compute selection should materialize")
}

fn plan_machine_cluster(
    cluster: ComputeCluster,
    state: &StackState,
    config: &DeploymentConfig,
) -> Result<crate::core::PlanResult> {
    let stack = Stack::new("machine-change".to_string())
        .add(cluster, ResourceLifecycle::Frozen)
        .build();
    StackExecutor::builder(
        &stack,
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::InCluster {
            namespace: Some("application".to_string()),
            additional_headers: None,
        })),
    )
    .deployment_config(config)
    .service_provider(Arc::new(MockPlatformServiceProvider::new()))
    .build()?
    .plan(state)
}
