pub mod aws_cloudformation;
pub mod aws_runtime;
pub mod azure_runtime;
pub mod azure_setup_owned;
pub mod gcp_runtime;
mod labels;

pub use aws_cloudformation::{
    AwsCloudFormationIamPolicy, AwsCloudFormationIamStatement,
    AwsCloudFormationPermissionsGenerator,
};
pub use aws_runtime::{
    ensure_unique_statement_sids, AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator,
};
pub use azure_runtime::{
    azure_predefined_role_id, custom_roles_by_scope,
    dedupe_azure_role_bindings, management_roles_by_scope, protected_management_assignable_scopes,
    AzureCustomRole, AzureGrantPlan, AzureManagementRoles, AzureManagementScopeKind,
    AzureRoleBinding, AzureRoleDefinition, AzureRoleDefinitionRef,
    AzureRuntimePermissionsGenerator, AzureScopedCustomRole,
};
pub use azure_setup_owned::{
    azure_role_assignment_allowlist_conditions, azure_role_key_segment, azure_runtime_role_definition_uuid,
    azure_runtime_roles, role_definition_guid, AzureRuntimeCustomRole, AzureRuntimeRoleHolder,
    AzureRuntimeRoles, AZURE_CONDITION_SHARD_LIMIT, AZURE_REMOTE_BINDINGS_PROFILE,
    AZURE_SERVICE_BUS_DATA_RECEIVER_ROLE_ID, AZURE_SERVICE_BUS_DATA_SENDER_ROLE_ID,
};
pub use gcp_runtime::{
    custom_role_description_names_prefix, custom_role_namespace_for_prefix,
    custom_role_permission_set_prefix, custom_role_prefix, legacy_custom_role_namespace_for_prefix,
    GcpBindingResourceKind, GcpBindingTargetScope, GcpCustomRole, GcpGrantPlan, GcpIamBinding,
    GcpIamBindings, GcpIamCondition, GcpRuntimePermissionsGenerator,
};
