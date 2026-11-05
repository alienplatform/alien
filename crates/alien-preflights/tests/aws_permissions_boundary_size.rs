//! A new AWS release fails when its setup permissions boundary cannot fit IAM's managed-policy
//! limit, instead of failing later when its setup packages are built.

use alien_core::{
    AwsManagementConfig, ComputeCluster, Container, ContainerCode, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig, PermissionProfile,
    PermissionSetReference, Platform, ResourceLifecycle, ResourceSpec, Stack, StackInputDefinition,
    StackSettings, StackState, Storage, ToolchainConfig, Worker, WorkerCode,
};
use alien_permissions::{
    generators::{aws_boundary_scope_context, aws_stack_permissions_boundary},
    PermissionContext,
};
use alien_preflights::{
    compile_time::aws_permissions_boundary_size::prepare_as_setup_package, error::ErrorData,
    runner::PreflightRunner,
};
use serde_json::json;

const EXTERNAL_BUCKETS: usize = 60;

/// Reads of many buckets outside the stack's prefix. The boundary lists each of them, so
/// together they exceed the limit.
fn external_reads_stack() -> Stack {
    let references = (0..EXTERNAL_BUCKETS).map(|index| {
        PermissionSetReference::Inline(
            serde_json::from_value(json!({
                "id": "custom/partner-read",
                "description": "Read a partner's exports",
                "platforms": {"aws": [{
                    "grant": {"actions": ["s3:GetObject"]},
                    "binding": {"stack": {"resources": [
                        format!("arn:aws:s3:::partner-exports-{index}/*")
                    ]}}
                }]}
            }))
            .expect("inline permission set"),
        )
    });
    Stack::new("partner-sync".to_string())
        .permission("application", PermissionProfile::new().global(references))
        .build()
}

fn assert_boundary_too_large(error: alien_error::AlienError<ErrorData>) {
    assert_eq!(error.code, "VALIDATION_FAILED", "{error}");
    let source = error
        .source
        .as_ref()
        .expect("the boundary error is the source");
    assert_eq!(source.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
    assert!(!source.retryable);
    let Some(ErrorData::ValidationFailed {
        error_count,
        warning_count,
        results,
    }) = &error.error
    else {
        panic!("unexpected error {error:?}");
    };
    assert_eq!((*error_count, *warning_count), (1, 0));
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert!(!result.success);
    assert_eq!(
        result.code.as_deref(),
        Some("AWS_PERMISSIONS_BOUNDARY_SIZE")
    );
    assert_eq!(result.errors.len(), 1);
    let message = &result.errors[0];
    assert!(
        message.starts_with("PERMISSIONS_BOUNDARY_TOO_LARGE: AWS permissions boundary is "),
        "{message}"
    );
    // The largest contributor is the grant, with an example resource to find it by.
    assert!(
        message.contains(&format!(
            "Largest contributors: profile 'application' stack-wide (custom/partner-read): "
        )),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "in {EXTERNAL_BUCKETS} statements, e.g. 'arn:aws:s3:::partner-exports-0/*'"
        )),
        "{message}"
    );
}

#[tokio::test]
async fn new_releases_fail_when_the_setup_boundary_does_not_fit() {
    let stack = external_reads_stack();
    // The management identity, and with it the boundary, only exists after setup preparation,
    // so failing here shows the check sized the prepared stack.
    assert!(stack.resources.is_empty());
    let runner = PreflightRunner::new();

    // `alien build`, which `alien release` runs for each platform.
    assert_boundary_too_large(
        runner
            .run_build_time_preflights(&stack, Platform::Aws)
            .await
            .expect_err("the boundary does not fit"),
    );
    // `alien release --prebuilt`.
    assert_boundary_too_large(
        runner
            .run_prebuilt_release_checks(&stack, Platform::Aws)
            .await
            .expect_err("the boundary does not fit"),
    );
}

#[tokio::test]
async fn setup_package_preparation_and_other_platforms_do_not_run_the_release_check() {
    let stack = external_reads_stack();
    let runner = PreflightRunner::new();
    // Packages of already-released stacks keep rendering; their own renderer reports the size.
    let summary = runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    assert!(summary.success);
    assert!(summary
        .results
        .iter()
        .all(|result| result.code.as_deref() != Some("AWS_PERMISSIONS_BOUNDARY_SIZE")));
    // Only AWS setup renders this boundary.
    for platform in [Platform::Gcp, Platform::Azure] {
        let summary = runner
            .run_build_time_preflights(&stack, platform)
            .await
            .unwrap_or_else(|error| panic!("{platform}: {error}"));
        assert!(summary.success, "{platform}");
    }
}

/// Source-built workloads that each read many installer-provided secrets. Setup preparation
/// grants each consumer reads of exactly its secrets; they name prefixed resources, so they
/// don't grow the boundary.
fn secret_consumers_stack() -> Stack {
    const CONSUMERS: usize = 3;
    let consumer = |index: usize| format!("api-{index}");
    let mut stack = Stack::new("orders".to_string()).add(
        Storage::new("uploads".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    for index in 0..CONSUMERS {
        let container = Container::new(consumer(index))
            .code(ContainerCode::Source {
                src: "./api".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "api".to_string(),
                },
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.25".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .replicas(1)
            .permissions(consumer(index))
            .port(8080)
            .build();
        stack = stack
            .permission(
                consumer(index),
                PermissionProfile::new().resource("uploads", ["storage/data-read"]),
            )
            .add(container, ResourceLifecycle::Live);
    }
    let worker = Worker::new("jobs".to_string())
        .code(WorkerCode::Source {
            src: "./jobs".to_string(),
            toolchain: ToolchainConfig::TypeScript { binary_name: None },
        })
        .permissions("jobs".to_string())
        .build();
    let mut stack = stack
        .permission("jobs", PermissionProfile::new())
        .add(worker, ResourceLifecycle::Live)
        .build();
    for input in 0..40 {
        let targets: Vec<_> = (0..CONSUMERS)
            .filter(|index| input % CONSUMERS >= *index)
            .map(consumer)
            .chain(["jobs".to_string()])
            .collect();
        let definition: StackInputDefinition = serde_json::from_value(json!({
            "id": format!("applicationSetting{input}"),
            "kind": "secret",
            "providedBy": ["deployer"],
            "required": true,
            "label": format!("Application setting {input}"),
            "description": "A credential the installer provides.",
            "env": [{"name": format!("APPLICATION_SETTING_{input}"), "targetResources": targets}],
        }))
        .expect("secret input definition");
        stack.inputs.push(definition);
    }
    stack
}

#[tokio::test]
async fn new_releases_with_many_secret_consumers_pass() {
    let stack = secret_consumers_stack();
    // Preparation grants each consumer reads of its secrets.
    let prepared = prepare_as_setup_package(&stack).await.expect("preparation");
    for consumer in ["api-0", "api-1", "api-2", "jobs"] {
        let profile = &prepared.permissions.profiles[consumer];
        assert!(
            profile
                .0
                .values()
                .flatten()
                .any(|grant| grant.id() == "vault/deployer-secrets-read"),
            "{consumer}: {profile:?}"
        );
    }
    let runner = PreflightRunner::new();
    let summary = runner
        .run_build_time_preflights(&stack, Platform::Aws)
        .await
        .expect("the boundary fits");
    assert!(summary.success);
}

#[tokio::test]
async fn new_releases_whose_setup_boundary_fits_pass() {
    // A fifth of the reads fit.
    let mut stack = external_reads_stack();
    stack.permissions.profiles["application"].0["*"].truncate(EXTERNAL_BUCKETS / 5);
    let runner = PreflightRunner::new();
    let summary = runner
        .run_build_time_preflights(&stack, Platform::Aws)
        .await
        .expect("the boundary fits");
    assert!(summary.success);
    runner
        .run_prebuilt_release_checks(&stack, Platform::Aws)
        .await
        .expect("the boundary fits");
}

/// Machines are chosen per deployment, so a release must not be rejected because compute
/// planning with default settings finds no machine for a large workload.
#[tokio::test]
async fn new_releases_are_not_held_to_default_compute_choices() {
    let container = Container::new("trainer".to_string())
        .code(ContainerCode::Source {
            src: "./trainer".to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: "trainer".to_string(),
            },
        })
        .cpu(ResourceSpec {
            min: "90".to_string(),
            desired: "90".to_string(),
        })
        .memory(ResourceSpec {
            min: "700Gi".to_string(),
            desired: "700Gi".to_string(),
        })
        .replicas(1)
        .permissions("trainer".to_string())
        .port(8080)
        .build();
    let stack = Stack::new("training".to_string())
        .permission("trainer", PermissionProfile::new())
        .add(container, ResourceLifecycle::Live)
        .build();
    // Planning compute with default settings rejects this workload.
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let error = PreflightRunner::new()
        .apply_mutations(stack.clone(), &StackState::new(Platform::Aws), &config)
        .await
        .expect_err("no default machine fits");
    assert_eq!(error.code, "STACK_MUTATION_FAILED", "{error}");

    let prepared = prepare_as_setup_package(&stack)
        .await
        .expect("preparation leaves compute choices to the deployment");
    assert!(prepared.resources.contains_key("management"));
    PreflightRunner::new()
        .run_build_time_preflights(&stack, Platform::Aws)
        .await
        .expect("the release check passes");
}

/// Leaving compute planning out of the preparation does not change the boundary: for a stack
/// default planning accepts, it renders the same document as the full setup preparation.
#[tokio::test]
async fn compute_planning_does_not_change_the_boundary() {
    let mut stack = secret_consumers_stack();
    // A workload grant on compute clusters, which only planning generates here.
    stack.permissions.profiles["jobs"] =
        PermissionProfile::new().global(["compute-cluster/execute"]);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let planned = PreflightRunner::new()
        .apply_mutations(stack.clone(), &StackState::new(Platform::Aws), &config)
        .await
        .expect("default planning fits these workloads");
    assert!(
        planned
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == ComputeCluster::RESOURCE_TYPE),
        "planning generated a compute cluster"
    );
    let prepared = prepare_as_setup_package(&stack).await.expect("preparation");
    assert!(
        prepared
            .resources
            .values()
            .all(|entry| entry.config.resource_type() != ComputeCluster::RESOURCE_TYPE),
        "preparation leaves compute to the deployment"
    );

    let context = PermissionContext::new()
        .with_stack_prefix("orders-production")
        .with_aws_region("eu-west-1")
        .with_aws_account_id("123456789012")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
        .with_managing_account_id("210987654321");
    let boundary = |stack: &Stack| {
        aws_stack_permissions_boundary(stack, &context, |_, scope| {
            aws_boundary_scope_context(stack, &context, scope, |_| Ok(None))
        })
        .expect("the boundary renders")
    };
    assert_eq!(boundary(&prepared), boundary(&planned));
}
