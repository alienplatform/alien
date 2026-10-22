use crate::{
    ownership_policy_for_resource_type, ErrorData, Key, ManagementPermissions, Platform,
    ResourceEntry, Result, Sandbox, SetupManagement, Stack, Storage,
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
    // Only the CloudFormation template applies S3 CORS rules. The runtime storage
    // controller would create the bucket without them, so browser reads would fail.
    if platform == Platform::Aws
        && entry
            .config
            .downcast_ref::<Storage>()
            .is_some_and(|storage| !storage.cors_allowed_origins.is_empty())
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

/// Automatic packages install access and secret storage, then let the runtime
/// create application infrastructure. Cloud API activation and Azure's containing
/// resource group must exist before their management identity can be installed.
/// Azure also installs application identities so management can name their
/// principals in the role-assignment conditions.
pub fn emits_package_scaffolding(
    entry: &ResourceEntry,
    management: SetupManagement,
    platform: Platform,
) -> bool {
    let resource_type = entry.config.resource_type();
    if management == SetupManagement::Automatic {
        is_automatic_setup_package_resource_type(resource_type.as_ref(), platform)
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
