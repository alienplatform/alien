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
    use crate::generators::families::{aws_family, azure_family};

    /// Actions that start a job, pick a revision, assign an identity or run a script were once a
    /// hand-kept list; the family predicate alone must cover them.
    #[test]
    fn azure_actions_that_run_code_or_assign_identities_are_family_actions() {
        for action in [
            "Microsoft.App/jobs/start/action",
            "Microsoft.App/containerApps/revisions/activate/action",
            "Microsoft.App/containerApps/revisions/deactivate/action",
            "Microsoft.App/containerApps/revisions/restart/action",
            "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
            "Microsoft.Compute/virtualMachines/runCommand/action",
            "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommand/action",
        ] {
            assert!(azure_family(action).is_some(), "{action}");
        }
    }

    #[test]
    fn every_aws_action_the_verifier_is_granted_is_in_a_family() {
        for action in AWS_VERIFIER_OWNED {
            // The verifier also deletes what it wrote; deletes are in no family.
            if action.contains(":Delete") && aws_family(action, None).is_none() {
                continue;
            }
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
