//! Compile-time checks that validate stack configuration without requiring cloud access.
//! These checks can run during build time for early error detection.

pub mod allowed_user_resources;
pub mod capacity_group_profile;
pub mod container_lifecycle;
pub mod external_bindings_required;
pub mod frozen_resource_lifecycle;
pub mod network_required;
pub mod permission_profiles_exist;
pub mod public_function_lifecycle;
pub mod resource_id_pattern;
pub mod resource_name_length;
pub mod resource_references_exist;
pub mod infrastructure_requirements;
pub mod service_account_impersonate_validation;
pub mod single_exposed_port_check;
pub mod single_queue_trigger;
pub mod unique_resources;
pub mod valid_resource_dependencies;

pub use allowed_user_resources::AllowedUserResourcesCheck;
pub use capacity_group_profile::CapacityGroupProfileCheck;
pub use container_lifecycle::ContainerLifecycleCheck;
pub use external_bindings_required::ExternalBindingsRequiredCheck;
pub use frozen_resource_lifecycle::FrozenResourceLifecycleCheck;
pub use network_required::{
    stack_requires_network, NetworkSettingsPlatformCheck, PublicSubnetsRequiredCheck,
};
pub use permission_profiles_exist::PermissionProfilesExistCheck;
pub use public_function_lifecycle::PublicFunctionLifecycleCheck;
pub use resource_id_pattern::ResourceIdPatternCheck;
pub use resource_name_length::ResourceNameLengthCheck;
pub use resource_references_exist::ResourceReferencesExistCheck;
pub use infrastructure_requirements::{DnsTlsRequiredCheck, HorizonRequiredCheck};
pub use service_account_impersonate_validation::ServiceAccountImpersonateValidationCheck;
pub use single_exposed_port_check::SingleExposedPortCheck;
pub use single_queue_trigger::SingleQueueTriggerCheck;
pub use unique_resources::UniqueResourcesCheck;
pub use valid_resource_dependencies::ValidResourceDependenciesCheck;
