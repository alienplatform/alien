use crate::{ownership_policy_for_resource_type, ResourceEntry, SetupManagement};

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
        matches!(
            resource_type.as_ref(),
            "remote-stack-management" | "vault" | "azure_resource_group" | "service_activation"
        )
    } else {
        ownership_policy_for_resource_type(resource_type.as_ref())
            .emits_setup_scaffolding(entry.lifecycle)
    }
}
