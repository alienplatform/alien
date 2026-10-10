//! Built-in permission sets are stored by name, never by body.
//!
//! Update checks compare what an installation recorded at setup (the prepared stack: permission
//! profiles, the management profile, and Frozen resources such as service accounts) with what a
//! new release prepares. If a built-in set's body were recorded there, adding an action to it
//! (for example `ec2:DescribeInstanceTypeOfferings` in `compute-cluster/management`) would make
//! every existing installation's next update fail with "Management permissions configuration was
//! modified" or a Frozen-resource change, and the update would need a setup re-run.

use alien_core::{
    compute_planner::plan_compute,
    permissions::{
        ManagementPermissions, PermissionProfile, PermissionSetReference, PermissionsConfig,
    },
    AwsManagementConfig, ComputeSettings, Container, ContainerCode, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig, PersistentStorage, Platform,
    ResourceLifecycle, ResourceSpec, Stack, StackSettings, StackState, VolumeBackups,
};
use alien_preflights::runner::PreflightRunner;

fn container(id: &str, persistent: bool) -> Container {
    let builder = Container::new(id.to_string())
        .code(ContainerCode::Image {
            image: format!("{id}:latest"),
        })
        .cpu(ResourceSpec {
            min: "1".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "1Gi".to_string(),
            desired: "1Gi".to_string(),
        })
        .port(8080)
        .permissions("app".to_string());
    if persistent {
        builder
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups: VolumeBackups::default(),
            })
            .stateful(true)
            .replicas(1)
            .build()
    } else {
        builder.build()
    }
}

#[tokio::test]
async fn prepared_aws_compute_stack_records_compute_permission_sets_by_name_only() {
    let stack = Stack::new("test-stack".to_string())
        .add(container("api", false), ResourceLifecycle::Live)
        .add(container("database", true), ResourceLifecycle::Live)
        .permissions(PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .build();
    let plan = plan_compute(&stack, Platform::Aws, None).expect("compute plan should build");
    let compute = ComputeSettings {
        containers: Default::default(),
        pools: plan
            .pools
            .iter()
            .map(|pool| (pool.pool_id.clone(), pool.recommended.clone()))
            .collect(),
    };
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings {
            compute: Some(compute),
            ..StackSettings::default()
        })
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/alien-management".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();

    let prepared = PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("stack should prepare");

    let (ManagementPermissions::Extend(management) | ManagementPermissions::Override(management)) =
        prepared.management()
    else {
        panic!("AWS management profile should be materialized");
    };
    let compute_refs: Vec<&PermissionSetReference> = management
        .0
        .values()
        .flatten()
        .filter(|reference| reference.id().starts_with("compute-cluster/"))
        .collect();
    assert!(
        !compute_refs.is_empty(),
        "management should grant the compute-cluster sets"
    );
    for reference in compute_refs {
        assert!(
            matches!(reference, PermissionSetReference::Name(_)),
            "{} must be referenced by name",
            reference.id()
        );
    }

    // Nothing that is persisted and compared on update (profiles, management, Frozen resources
    // and therefore the setup-owned digest) carries the body of a compute-cluster set. Actions
    // that have always been in those sets are as absent as the newly added one.
    let persisted = serde_json::to_string(&prepared).expect("prepared stack serializes");
    // Control: a set granted to an application profile is resolved into its Frozen service
    // account, so body changes to such sets do show up on update. Compute-cluster sets are
    // management-only and stay out.
    assert!(
        persisted.contains("s3:GetObject"),
        "the app service account should record its resolved storage/data-read body"
    );
    for action in [
        "autoscaling:CreateAutoScalingGroup",
        "autoscaling:DescribeScalingActivities",
        "ec2:DescribeInstanceTypeOfferings",
    ] {
        assert!(
            !persisted.contains(action),
            "prepared stack must not record '{action}'"
        );
    }
}
