//! What only the verifier may do on a protected install.
//!
//! A protected install runs only what the vendor signed. Every cloud API call that decides what
//! runs (function code and configuration, launch templates, instance templates, Cloud Run
//! services and jobs, Container Apps, scale sets) belongs to the verifier, which applies only
//! signed releases. The management identity keeps reads, calling the verifier, scaling where
//! the cloud enforces the signed bounds, and the infrastructure that runs no code.
//!
//! Rendering a management identity with [`PermissionContext::signing_protected`] set drops
//! these permissions from its grants. Unprotected installs never set the flag and render
//! exactly as before.
//!
//! [`PermissionContext::signing_protected`]: crate::PermissionContext::signing_protected

use alien_core::{AwsBindingSpec, AwsConditionValue};

use super::families::aws_family;

/// The AWS actions the verifier is granted for function and machine writes. Each is in the RUN
/// or REACH family (a test pins that), so management holds none of them. AWS scaling stays with the
/// management identity: `SetDesiredCapacity` can't change what boots, and the verifier sets the
/// group's min and max from the signed stack, so AWS enforces the signed bounds.
pub const AWS_VERIFIER_OWNED: &[&str] = &[
    // Function code, configuration and who may invoke it.
    "lambda:CreateFunction",
    "lambda:UpdateFunctionCode",
    "lambda:UpdateFunctionConfiguration",
    "lambda:PublishVersion",
    "lambda:CreateAlias",
    "lambda:UpdateAlias",
    "lambda:DeleteAlias",
    "lambda:PutFunctionConcurrency",
    "lambda:DeleteFunctionConcurrency",
    "lambda:PutProvisionedConcurrencyConfig",
    "lambda:DeleteProvisionedConcurrencyConfig",
    "lambda:PutFunctionEventInvokeConfig",
    "lambda:UpdateFunctionEventInvokeConfig",
    "lambda:DeleteFunctionEventInvokeConfig",
    "lambda:AddPermission",
    "lambda:RemovePermission",
    "lambda:CreateFunctionUrlConfig",
    "lambda:UpdateFunctionUrlConfig",
    "lambda:DeleteFunctionUrlConfig",
    "lambda:PutFunctionCodeSigningConfig",
    "lambda:DeleteFunctionCodeSigningConfig",
    "lambda:UpdateCodeSigningConfig",
    "lambda:PutRuntimeManagementConfig",
    // What a machine boots: its launch template, and launching one at all.
    "ec2:CreateLaunchTemplate",
    "ec2:CreateLaunchTemplateVersion",
    "ec2:ModifyLaunchTemplate",
    "ec2:DeleteLaunchTemplate",
    "ec2:RunInstances",
    "ec2:CreateFleet",
    "ec2:RequestSpotInstances",
    "ec2:RequestSpotFleet",
    "ec2:ModifyInstanceAttribute",
    "autoscaling:CreateAutoScalingGroup",
    "autoscaling:UpdateAutoScalingGroup",
    "autoscaling:StartInstanceRefresh",
    "autoscaling:CreateLaunchConfiguration",
    "autoscaling:AttachInstances",
];

/// The actions of an AWS catalog entry a protected install's management identity receives: all
/// but those in the RUN, IDENTITY and REACH families (see [`aws_family`]). `iam:PassRole` is
/// judged by the services its binding passes to.
pub fn aws_management_actions(actions: &[String], binding: &AwsBindingSpec) -> Vec<String> {
    let services = aws_passed_to_services(binding);
    actions
        .iter()
        .filter(|action| aws_family(action, services.as_deref()).is_none())
        .cloned()
        .collect()
}

/// The `iam:PassedToService` values a binding's condition restricts a role pass to, if it has one.
pub fn aws_passed_to_services(binding: &AwsBindingSpec) -> Option<Vec<String>> {
    let mut services = Vec::new();
    for values in binding.condition.as_ref()?.values() {
        for (key, value) in values {
            if key.eq_ignore_ascii_case("iam:PassedToService") {
                match value {
                    AwsConditionValue::String(service) => services.push(service.clone()),
                    AwsConditionValue::List(list) => services.extend(list.iter().cloned()),
                }
            }
        }
    }
    (!services.is_empty()).then_some(services)
}

/// GCP permissions only the verifier holds, besides every `setIamPolicy`, the Cloud Run writes
/// [`gcp_cloud_run_write`] names and the OS Login and OS Config writes [`gcp_machine_access_write`]
/// names.
const GCP_VERIFIER_OWNED: &[&str] = &[
    // Machines: an instance template, an instance's metadata or the group's configuration
    // decides what a machine boots and runs. `instanceGroupManagers.update` also sets per-instance
    // metadata and is the same permission a resize needs, so the verifier scales on GCP.
    // `useReadOnly` would let a group point at an older or another install's template.
    "compute.instanceTemplates.create",
    "compute.instanceTemplates.useReadOnly",
    "compute.instanceGroupManagers.create",
    "compute.instanceGroupManagers.update",
    "compute.instances.create",
    "compute.instances.setMetadata",
    "compute.instances.setServiceAccount",
    "compute.projects.setCommonInstanceMetadata",
    // Network tags decide which firewall rules apply to a machine.
    "compute.instances.setTags",
    // An HMAC key reads the install's buckets as the service account it belongs to.
    "storage.hmacKeys.create",
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
    // A sandbox template decides what a sandbox runs, and a sandbox is created from one. The AWS
    // equivalent, `AWS_MICROVM_RUNS`, is the verifier's too.
    "aiplatform.sandboxEnvironmentTemplates.create",
    "aiplatform.sandboxEnvironmentTemplates.delete",
    "aiplatform.sandboxEnvironments.create",
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

/// Whether only the verifier may hold the GCP `permission` in a protected install.
pub fn gcp_verifier_owned(permission: &str) -> bool {
    GCP_VERIFIER_OWNED.contains(&permission)
        || permission.ends_with(".setIamPolicy")
        || gcp_cloud_run_write(permission)
        || gcp_machine_access_write(permission)
}

/// Whether `permission` writes OS Login or OS Config state, which gives a login or runs a script
/// on a machine: `osconfig` and `oslogin` permissions other than reads, and the two instance
/// permissions that open an OS Login session.
fn gcp_machine_access_write(permission: &str) -> bool {
    if matches!(
        permission,
        "compute.instances.osLogin" | "compute.instances.osAdminLogin"
    ) {
        return true;
    }
    if permission.starts_with("osconfig.") || permission.starts_with("oslogin.") {
        let verb = permission.rsplit('.').next().unwrap_or_default();
        return !(verb.starts_with("get") || verb.starts_with("list"));
    }
    false
}

/// Azure actions a protected management identity loses. Setup's deny settings refuse it every
/// write and delete in the workload resource group except those [`azure_runs_no_code`] admits,
/// but they cannot refuse an `/action`, so these go from its roles:
/// - starting a Container Apps job takes a template that replaces the job's containers;
/// - activating a revision runs an older release than the one the verifier applied;
/// - assigning a user-assigned identity lets a resource outside that group run as it;
/// - deactivating or restarting a revision decides which signed revision serves;
/// - a run command runs a script on a machine.
const AZURE_VERIFIER_OWNED_ACTIONS: &[&str] = &[
    "Microsoft.App/jobs/start/action",
    "Microsoft.App/containerApps/revisions/activate/action",
    "Microsoft.App/containerApps/revisions/deactivate/action",
    "Microsoft.App/containerApps/revisions/restart/action",
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
    "Microsoft.Compute/virtualMachines/runCommand/action",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommand/action",
];

/// Whether only the verifier may hold the Azure `action` in a protected install's roles.
pub fn azure_verifier_owned_action(action: &str) -> bool {
    AZURE_VERIFIER_OWNED_ACTIONS
        .iter()
        .any(|owned| owned.eq_ignore_ascii_case(action))
}

/// Azure data actions only the verifier holds in a protected install's roles: a sandbox disk
/// image or a sandbox is what a sandbox runs, and setup's deny settings don't reach the data
/// plane. The verifier-owned AWS and GCP sandbox powers are the same.
const AZURE_VERIFIER_OWNED_DATA_ACTIONS: &[&str] = &[
    "Microsoft.App/sandboxGroups/diskimages/write",
    "Microsoft.App/sandboxGroups/sandboxes/write",
];

/// Whether only the verifier may hold the Azure data `action` in a protected install's roles.
pub fn azure_verifier_owned_data_action(action: &str) -> bool {
    AZURE_VERIFIER_OWNED_DATA_ACTIONS
        .iter()
        .any(|owned| owned.eq_ignore_ascii_case(action))
}

/// What a protected install's management identity receives instead of a predefined role that a
/// built-in set it holds binds at the resource group, and that reaches the verifier's identity or
/// runs a template:
/// - `Managed Identity Contributor` writes every user-assigned identity in the group, including
///   the verifier's, and adds a federated credential to it, which makes management the verifier.
///   Management keeps reading identities.
/// - `Network Contributor` also holds `Microsoft.Resources/deployments/*`, which deploys any
///   template into the group. Management keeps the network actions.
const AZURE_PROTECTED_ROLE_SUBSTITUTES: &[(&str, &[&str])] = &[
    (
        "Managed Identity Contributor",
        &["Microsoft.ManagedIdentity/userAssignedIdentities/read"],
    ),
    ("Network Contributor", &["Microsoft.Network/*"]),
];

/// The actions that replace the predefined `role` on a protected install, if it is replaced.
pub fn azure_protected_role_substitute(role: &str) -> Option<&'static [&'static str]> {
    AZURE_PROTECTED_ROLE_SUBSTITUTES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(role))
        .map(|(_, actions)| *actions)
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

/// Whether the Azure `action` writes or deletes what runs code or a Key Vault, or is a wildcard
/// reaching such writes: a protected install's management identity holds none of these.
pub fn azure_changes_what_runs(action: &str) -> bool {
    // A pattern that starts with a wildcard (`*/write`) names no provider, so it reaches every
    // provider's writes unless it only reads.
    if action.starts_with('*') {
        return !action.to_ascii_lowercase().ends_with("/read");
    }
    // A vault write could turn the signing vault to access policies and open the ledger.
    let vault = action
        .to_ascii_lowercase()
        .starts_with("microsoft.keyvault/vaults");
    if azure_write_or_delete(action) {
        return vault || !azure_runs_no_code(action);
    }
    let Some(prefix) = action.strip_suffix('*') else {
        return false;
    };
    let prefix = prefix.to_ascii_lowercase();
    prefix.is_empty()
        || AZURE_RUNS_CODE.iter().any(|path| {
            let path = path.to_ascii_lowercase();
            path.starts_with(&prefix) || prefix.starts_with(&path)
        })
}

#[cfg(test)]
mod tests {
    use super::{azure_changes_what_runs, AWS_VERIFIER_OWNED};
    use crate::generators::families::aws_family;

    #[test]
    fn every_aws_action_the_verifier_is_granted_is_in_a_family() {
        for action in AWS_VERIFIER_OWNED {
            assert!(aws_family(action, None).is_some(), "{action}");
        }
    }


    #[test]
    fn azure_patterns_that_reach_what_runs_are_dropped() {
        for action in [
            "*",
            "*/write",
            "*/delete",
            "Microsoft.Compute/*",
            "Microsoft.Compute/virtualMachineScaleSets/write",
            "Microsoft.Resources/tags/write",
            "Microsoft.KeyVault/vaults/write",
        ] {
            assert!(azure_changes_what_runs(action), "{action}");
        }
        for action in [
            "Microsoft.Storage/storageAccounts/write",
            "Microsoft.Compute/virtualMachineScaleSets/read",
            "Microsoft.Network/virtualNetworks/write",
            "*/read",
        ] {
            assert!(!azure_changes_what_runs(action), "{action}");
        }
    }
}
