//! The management identity writes the deployment `secrets` vault but must not read it, and it must
//! not be able to give itself that read through an IAM change either.
//!
//! These tests prepare a stack the way a Terraform setup is prepared (no automatic setup) with a
//! Live resource of every runtime kind, then resolve every permission set the management
//! identity is granted and check that none of them can change IAM in a way that reaches the
//! vault.

use alien_core::{
    compute_planner::plan_compute,
    permissions::{
        ManagementPermissions, PermissionProfile, PermissionSet, PermissionSetReference,
    },
    Ai, ComputeSettings, Container, ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, GcpManagementConfig, Kv, ManagementConfig, Platform, Queue,
    ResourceLifecycle, ResourceSpec, Stack, StackSettings, StackState, Storage, Vault, Worker,
    WorkerCode,
};

use alien_preflights::runner::PreflightRunner;

/// A stack with a Live resource of every kind the runtime provisions, plus a container so
/// preparation adds the compute cluster.
fn stack() -> Stack {
    Stack::new("acme".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("app-secrets", ["vault/data-read"])
                .resource("llm", ["ai/invoke"]),
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "registry.example.com/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            Container::new("web".to_string())
                .code(ContainerCode::Image {
                    image: "registry.example.com/web:1".to_string(),
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
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            Vault::new("app-secrets".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(Ai::new("llm".to_string()).build(), ResourceLifecycle::Live)
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build()
}

async fn prepared_management(
    platform: Platform,
    management: ManagementConfig,
) -> Vec<PermissionSet> {
    let stack = stack();
    let plan = plan_compute(&stack, platform, None).expect("compute plan builds");
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings {
            compute: Some(ComputeSettings {
                containers: Default::default(),
                pools: plan
                    .pools
                    .iter()
                    .map(|pool| (pool.pool_id.clone(), pool.recommended.clone()))
                    .collect(),
            }),
            ..StackSettings::default()
        })
        .management_config(management)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let prepared = PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(platform), &config)
        .await
        .expect("stack prepares");
    let (ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile)) =
        prepared.management()
    else {
        panic!("management profile is materialized");
    };
    let mut sets: Vec<PermissionSet> = profile
        .0
        .values()
        .flatten()
        .map(|reference: &PermissionSetReference| {
            reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .unwrap_or_else(|| panic!("permission set '{}' resolves", reference.id()))
        })
        .collect();
    sets.sort_by(|a, b| a.id.cmp(&b.id));
    sets.dedup_by(|a, b| a.id == b.id);
    sets
}

/// GCP permissions that change an IAM policy or a role, which would let the holder grant
/// itself `secretmanager.versions.access`, directly or by impersonating a workload.
const GCP_IAM_WRITES: &[&str] = &[
    "resourcemanager.projects.setIamPolicy",
    "iam.serviceAccounts.setIamPolicy",
    "secretmanager.secrets.setIamPolicy",
    "iam.roles.create",
    "iam.roles.update",
    "iam.serviceAccounts.getAccessToken",
    "iam.serviceAccounts.signJwt",
    "iam.serviceAccountKeys.create",
];

/// Predefined roles that include one of `GCP_IAM_WRITES` or secret reads.
const GCP_IAM_WRITE_ROLES: &[&str] = &[
    "roles/owner",
    "roles/editor",
    "roles/resourcemanager.projectIamAdmin",
    "roles/iam.securityAdmin",
    "roles/iam.serviceAccountAdmin",
    "roles/iam.serviceAccountTokenCreator",
    "roles/iam.serviceAccountKeyAdmin",
    "roles/iam.roleAdmin",
    "roles/secretmanager.admin",
    "roles/secretmanager.secretAccessor",
];

#[tokio::test]
async fn gcp_management_holds_no_iam_write_that_reaches_the_secrets_vault() {
    let sets = prepared_management(
        Platform::Gcp,
        ManagementConfig::Gcp(GcpManagementConfig {
            federation_trust: None,
            service_account_email: "manager@vendor.iam.gserviceaccount.com".to_string(),
        }),
    )
    .await;
    let ids: Vec<&str> = sets.iter().map(|set| set.id.as_str()).collect();
    for expected in [
        "worker/provision",
        "vault/provision",
        "ai/provision",
        "compute-cluster/management",
    ] {
        assert!(ids.contains(&expected), "{expected} is granted: {ids:?}");
    }

    let mut found = Vec::new();
    for set in &sets {
        for grant in set.platforms.gcp.iter().flatten() {
            let permissions = grant
                .grant
                .permissions
                .iter()
                .chain(grant.grant.residual_permissions.iter())
                .flatten();
            for permission in permissions {
                if GCP_IAM_WRITES.contains(&permission.as_str()) {
                    found.push(format!("{}: {permission}", set.id));
                }
            }
            for role in grant.grant.predefined_roles.iter().flatten() {
                if GCP_IAM_WRITE_ROLES.contains(&role.as_str()) {
                    found.push(format!("{}: {role}", set.id));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "management can change IAM and grant itself secret reads: {found:#?}"
    );
}
