pub mod aws_boundary;
mod aws_boundary_coverage;
mod aws_boundary_scope;
pub mod aws_cloudformation;
mod aws_managed_policies;
pub mod aws_runtime;
pub mod azure_runtime;
pub mod azure_setup_owned;
pub mod gcp_runtime;
pub mod gcp_setup_owned;
mod labels;

pub use aws_boundary::{
    aws_permissions_boundary, aws_stack_permissions_boundary,
    aws_stack_required_permissions_boundary,
};
pub use aws_boundary_coverage::aws_permissions_boundary_covers;
pub use aws_boundary_scope::{
    aws_boundary_scope_context, aws_longest_permission_context, validate_aws_boundary_size,
};
pub use aws_cloudformation::{
    AwsCloudFormationIamPolicy, AwsCloudFormationIamStatement,
    AwsCloudFormationPermissionsGenerator,
};
pub use aws_managed_policies::{
    merge_statement_actions, pack_managed_policy_statements, MAX_ATTACHED_MANAGEMENT_POLICIES,
    MAX_MANAGED_POLICY_BYTES,
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
pub use gcp_setup_owned::{
    gcp_project_iam_write_role, gcp_project_role_grant_conditions, gcp_runtime_roles,
    strip_gcp_management_iam_writes, GcpRuntimeRoles, GCP_PROJECT_IAM_WRITE,
    GCP_ROLES_PER_GRANT_CONDITION, GCP_ROLE_DEFINITION_WRITES,
};
