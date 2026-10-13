use crate::{
    ownership_policy_for_resource_type, ErrorData, Platform, ResourceEntry, Result, Sandbox,
    SetupManagement, Stack,
};
use alien_error::AlienError;

/// Refuse resources whose setup-owned scaffolding cannot yet be created by runtime.
/// Check both package generation and deployment planning, including later additions.
pub fn validate_automatic_setup_resources(
    stack: &Stack,
    platform: Platform,
    management: SetupManagement,
) -> Result<()> {
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
pub fn emits_package_scaffolding(entry: &ResourceEntry, management: SetupManagement) -> bool {
    let resource_type = entry.config.resource_type();
    if management == SetupManagement::Automatic {
        is_automatic_setup_package_resource_type(resource_type.as_ref())
    } else {
        ownership_policy_for_resource_type(resource_type.as_ref())
            .emits_setup_scaffolding(entry.lifecycle)
    }
}

/// Resources whose state is replaced by an automatic setup package import.
/// Application resources are managed through the installed identity and must
/// retain their controller state when that package is reimported.
pub fn is_automatic_setup_package_resource_type(resource_type: &str) -> bool {
    matches!(
        resource_type,
        "remote-stack-management" | "vault" | "azure_resource_group" | "service_activation"
    )
}
