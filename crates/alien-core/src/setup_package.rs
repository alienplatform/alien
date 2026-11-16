use crate::{
    ownership_policy_for_resource_type, ComputeCluster, ErrorData, Key, ManagementPermissions,
    Platform, ResourceEntry, ResourceLifecycle, Result, Sandbox, SetupManagement, Stack, Storage,
};
use alien_error::AlienError;

/// Refuse resources whose setup-owned scaffolding cannot yet be created by runtime.
/// Check both package generation and deployment planning, including later additions.
pub fn validate_automatic_setup_resources(
    stack: &Stack,
    platform: Platform,
    management: SetupManagement,
) -> Result<()> {
    if management == SetupManagement::Automatic
        && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
    {
        let unsupported = |resource_type: &str, resource_id: &str| {
            AlienError::new(ErrorData::AutomaticSetupResourceUnsupported {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                platform,
            })
        };
        match stack.management() {
            // Setup must be able to grant whatever a later release adds.
            ManagementPermissions::Override(_) => {
                return Err(unsupported(
                    "management permission override",
                    "permissions.management",
                ));
            }
            // The AWS management role may not change its own policies after setup, so a
            // grant scoped to one resource could never be attached by the runtime.
            ManagementPermissions::Extend(profile) if platform == Platform::Aws => {
                if let Some(scope) = profile.0.keys().find(|scope| scope.as_str() != "*") {
                    return Err(unsupported("resource-scoped management permission", scope));
                }
            }
            _ => {}
        }
        for (id, entry) in stack.resources() {
            if let Some(resource_type) = automatic_setup_unsupported_resource(entry, platform) {
                return Err(unsupported(&resource_type, id));
            }
        }
    }
    if management == SetupManagement::Automatic && platform == Platform::Azure {
        for (id, entry) in stack.resources() {
            if entry.config.downcast_ref::<Sandbox>().is_some() {
                return Err(AlienError::new(ErrorData::SandboxCapabilityUnsupported {
                    capability: format!(
                        "automatic infrastructure management for sandbox '{id}'; use manual setup to create and import its sandbox group"
                    ),
                    platform: platform.to_string(),
                }));
            }
        }
    }
    Ok(())
}

/// Return resources whose declarations cannot be created by their runtime
/// controllers when omitted from automatic setup.
fn automatic_setup_unsupported_resource(
    entry: &ResourceEntry,
    platform: Platform,
) -> Option<String> {
    if entry.config.downcast_ref::<Key>().is_some() {
        return Some("key".to_string());
    }
    let resource_type = entry.config.resource_type();
    // These controllers refuse runtime creation: only a setup package can provision them.
    if matches!(
        resource_type.as_ref(),
        "email" | "experimental/aws-opensearch" | "kubernetes-cluster"
    ) {
        return Some(resource_type.as_ref().to_string());
    }
    // Azure CORS belongs to the shared account blob service; the container
    // controller cannot apply it independently.
    if platform == Platform::Azure
        && entry
            .config
            .downcast_ref::<Storage>()
            .is_some_and(|storage| storage.has_cors())
    {
        return Some("storage with CORS origins".to_string());
    }
    None
}

/// Controls configuration exposure, independently of installation authority.
/// Existing generator entry points retain the complete Advanced interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupConfigurationProfile {
    Standard,
    Advanced,
}

/// Where a setup package installs: the platform the deployment runs on and the cloud its
/// infrastructure lives in. They differ for managed Kubernetes (an EKS, GKE or AKS cluster runs
/// on `Kubernetes`, in the `Aws`, `Gcp` or `Azure` cloud).
///
/// Every question about what a setup package emits and registers is answered from this, so the
/// generators, the emitters and the manager's registration check cannot disagree about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupTarget {
    /// The platform the deployment runs on.
    pub deployment_platform: Platform,
    /// The cloud the setup's infrastructure lives in.
    pub cloud_platform: Platform,
}

impl SetupTarget {
    /// A target from a registration's `platform` and `basePlatform`.
    pub fn new(deployment_platform: Platform, base_platform: Option<Platform>) -> Self {
        Self {
            deployment_platform,
            cloud_platform: base_platform.unwrap_or(deployment_platform),
        }
    }

    /// A target from the cloud and whether the setup installs onto a Kubernetes cluster in it.
    pub fn from_cloud(cloud_platform: Platform, targets_kubernetes: bool) -> Self {
        Self {
            deployment_platform: if targets_kubernetes {
                Platform::Kubernetes
            } else {
                cloud_platform
            },
            cloud_platform,
        }
    }

    pub fn is_kubernetes(&self) -> bool {
        self.deployment_platform == Platform::Kubernetes
    }
}

/// Whether the resource type is one a Kubernetes target runs on the cluster itself: a
/// `ComputeCluster` is a set of logical pools the Operator validates and a `Sandbox` is a pod.
/// Setup creates no cloud infrastructure for either, and runtime provisioning verifies them
/// through the Kubernetes controllers rather than a cloud controller.
///
/// Setup's emit rule ([`emits_package_scaffolding`]) and the executor's choice of controller
/// both ask this, so they cannot disagree.
pub fn is_kubernetes_native_resource_type(resource_type: &crate::ResourceType) -> bool {
    *resource_type == Sandbox::RESOURCE_TYPE || *resource_type == ComputeCluster::RESOURCE_TYPE
}

/// Whether setup emits this resource and registers it with the manager.
///
/// This is the one rule. Both setup generators render exactly the resources it accepts, and the
/// manager expects a registration to hold exactly those, so a change here moves all three
/// together.
///
/// - On Kubernetes the cluster's own nodes and pods stand in for the cloud machines: a
///   `ComputeCluster` is a set of logical pools the Operator validates, and a `Sandbox` is a pod.
///   Setup creates no cloud infrastructure for either, whatever the cloud or whether the cluster
///   is new or existing.
/// - Automatic packages install access and secret storage, then let the runtime create
///   application infrastructure. Cloud API activation and Azure's containing resource group must
///   exist before their management identity can be installed. Azure also installs application
///   identities so management can name their principals in the role-assignment conditions.
/// - Manual packages emit what the resource's ownership policy says.
///
/// Whether a Kubernetes cluster is new or existing decides which resources the stack declares
/// (the cluster and network mutations add or omit them), not which of them setup emits.
pub fn emits_package_scaffolding(
    entry: &ResourceEntry,
    management: SetupManagement,
    target: SetupTarget,
) -> bool {
    let resource_type = entry.config.resource_type();
    if target.is_kubernetes() && is_kubernetes_native_resource_type(&resource_type) {
        return false;
    }
    if management == SetupManagement::Automatic {
        is_automatic_setup_package_resource_type(resource_type.as_ref(), target.cloud_platform)
    } else {
        ownership_policy_for_resource_type(resource_type.as_ref())
            .emits_setup_scaffolding(entry.lifecycle)
    }
}

/// Resources whose state is replaced by an automatic setup package import.
/// Application resources are managed through the installed identity and must
/// retain their controller state when that package is reimported.
pub fn is_automatic_setup_package_resource_type(resource_type: &str, platform: Platform) -> bool {
    matches!(
        resource_type,
        "remote-stack-management" | "vault" | "azure_resource_group" | "service_activation"
    ) || (platform == Platform::Azure
        && matches!(resource_type, "service-account" | "resource-access"))
}

/// Whether deleting an installation through runtime cleanup removes this resource before the
/// setup package's own teardown runs.
///
/// An automatic package creates only its scaffolding types (see
/// [`is_automatic_setup_package_resource_type`]). The runtime created everything else, Frozen
/// resources included, and the package's teardown cannot see those, so runtime cleanup removes
/// them. It leaves the package's resources, Live vaults included, to that teardown. Any other
/// package owns its Frozen resources, apart from types that need runtime cleanup first.
pub fn removed_by_runtime_cleanup(
    resource_type: &str,
    lifecycle: ResourceLifecycle,
    management: SetupManagement,
    platform: Platform,
) -> bool {
    if management == SetupManagement::Automatic {
        return !is_automatic_setup_package_resource_type(resource_type, platform);
    }
    lifecycle == ResourceLifecycle::Live
        || ownership_policy_for_resource_type(resource_type).has_runtime_cleanup_before_teardown()
}

#[cfg(test)]
mod cors_tests {
    use super::*;

    #[test]
    fn automatic_cors_requires_an_implementing_controller() {
        let stack = Stack::new("browser-storage".into())
            .add(
                Storage::new("assets".into())
                    .cors_allowed_origins(vec!["https://app.example.com".into()])
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        for platform in [Platform::Aws, Platform::Gcp] {
            validate_automatic_setup_resources(&stack, platform, SetupManagement::Automatic)
                .unwrap();
        }
        let error =
            validate_automatic_setup_resources(&stack, Platform::Azure, SetupManagement::Automatic)
                .unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::AutomaticSetupResourceUnsupported { .. })
        ));
    }
}
