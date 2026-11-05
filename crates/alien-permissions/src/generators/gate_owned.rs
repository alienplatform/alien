//! What only the gate may do in a protected deployment.
//!
//! A protected deployment runs only what the developer signed. On GCP and Azure the cloud API
//! runs code directly (Cloud Run services and jobs, instance templates, Container Apps, scale
//! sets), so the gate alone writes those. The management identity keeps reads, calling the gate,
//! scaling within bounds and the infrastructure that runs no code.
//!
//! Rendering a management identity with [`PermissionContext::release_trust_protected`] set drops
//! these permissions from its grants. Nothing else is rendered with that flag.
//!
//! [`PermissionContext::release_trust_protected`]: crate::PermissionContext::release_trust_protected

/// GCP permissions only the gate holds, besides every `setIamPolicy` and the Cloud Run writes
/// [`gcp_cloud_run_write`] names.
const GCP_GATE_OWNED: &[&str] = &[
    // Machines: an instance template or an instance's metadata decides what a machine boots and
    // runs, and an instance can be created from any template.
    "compute.instanceTemplates.create",
    "compute.regionInstanceTemplates.create",
    "compute.instances.create",
    "compute.instances.setMetadata",
    "compute.instances.setServiceAccount",
    "compute.projects.setCommonInstanceMetadata",
    // Running as, or as good as, another service account.
    "iam.serviceAccounts.actAs",
    "iam.serviceAccounts.getAccessToken",
    "iam.serviceAccounts.getOpenIdToken",
    "iam.serviceAccounts.implicitDelegation",
    "iam.serviceAccounts.signBlob",
    "iam.serviceAccounts.signJwt",
    "iam.serviceAccountKeys.create",
    // The management identity's own custom roles: changing one grants it anything.
    "iam.roles.create",
    "iam.roles.update",
    "iam.roles.undelete",
    // A build runs the steps it is given. Builds the application starts use its own identity.
    "cloudbuild.builds.create",
    "cloudbuild.builds.update",
    "cloudbuild.builds.approve",
    // A reasoning engine can carry a code package.
    "aiplatform.reasoningEngines.create",
    "aiplatform.reasoningEngines.update",
];

/// GCP predefined roles a protected management identity is never bound to: each holds a
/// permission in [`GCP_GATE_OWNED`] or a Cloud Run write.
const GCP_REFUSED_PREDEFINED_ROLES: &[&str] = &[
    "roles/iam.serviceAccountTokenCreator",
    "roles/iam.serviceAccountUser",
    "roles/run.admin",
    "roles/run.developer",
    "roles/run.jobsExecutorWithOverrides",
];

/// Whether `permission` is a Cloud Run service or job write. The management identity keeps the
/// reads and running a job as it is (`run.jobs.run`, which takes no overrides).
fn gcp_cloud_run_write(permission: &str) -> bool {
    if let Some(verb) = permission.strip_prefix("run.services.") {
        return !matches!(
            verb,
            "get" | "list" | "getIamPolicy" | "listEffectiveTags" | "listTagBindings"
        );
    }
    if let Some(verb) = permission.strip_prefix("run.jobs.") {
        return !matches!(
            verb,
            "get" | "list" | "getIamPolicy" | "listEffectiveTags" | "listTagBindings" | "run"
        );
    }
    false
}

/// Whether only the gate may hold the GCP `permission` in a protected deployment.
pub fn gcp_gate_owned(permission: &str) -> bool {
    GCP_GATE_OWNED.contains(&permission)
        || permission.ends_with(".setIamPolicy")
        || gcp_cloud_run_write(permission)
}

/// Whether a protected management identity must never be bound to the GCP predefined `role`.
pub fn gcp_refused_predefined_role(role: &str) -> bool {
    GCP_REFUSED_PREDEFINED_ROLES.contains(&role)
}

/// Azure actions a protected management identity loses. Setup's deny settings refuse it every
/// write and delete in the workload resource group except those [`azure_runs_no_code`] admits,
/// but they cannot refuse an `/action`, so these go from its roles:
/// - starting a Container Apps job takes a template that replaces the job's containers;
/// - activating a revision runs an older release than the one the gate applied;
/// - assigning a user-assigned identity lets a resource outside that group run as it.
const AZURE_GATE_OWNED_ACTIONS: &[&str] = &[
    "Microsoft.App/jobs/start/action",
    "Microsoft.App/containerApps/revisions/activate/action",
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
];

/// Whether only the gate may hold the Azure `action` in a protected deployment's roles.
pub fn azure_gate_owned_action(action: &str) -> bool {
    AZURE_GATE_OWNED_ACTIONS
        .iter()
        .any(|owned| owned.eq_ignore_ascii_case(action))
}

/// Azure resource paths whose writes and deletes run code or decide who code runs as. Setup's
/// deny settings keep these from the management identity.
const AZURE_RUNS_CODE: &[&str] = &[
    "Microsoft.App/",
    "Microsoft.Authorization/",
    "Microsoft.Compute/",
    "Microsoft.ContainerInstance/",
    "Microsoft.ContainerRegistry/registries/agentPools/",
    "Microsoft.ContainerRegistry/registries/runs/",
    "Microsoft.ContainerRegistry/registries/taskRuns/",
    "Microsoft.ContainerRegistry/registries/tasks/",
    "Microsoft.ContainerService/",
    "Microsoft.ManagedIdentity/",
    "Microsoft.Resources/",
    "Microsoft.Web/",
];

/// Whether the Azure write or delete `action` changes only infrastructure that runs no code, so
/// setup's deny settings leave it to the management identity.
pub fn azure_runs_no_code(action: &str) -> bool {
    let action = action.to_ascii_lowercase();
    !AZURE_RUNS_CODE
        .iter()
        .any(|path| action.starts_with(&path.to_ascii_lowercase()))
}

/// Whether the Azure `action` is a write or a delete, the operations a `denyWriteAndDelete`
/// deny setting refuses.
pub fn azure_write_or_delete(action: &str) -> bool {
    let action = action.to_ascii_lowercase();
    action.ends_with("/write") || action.ends_with("/delete")
}
