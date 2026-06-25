pub mod aws_cloudformation;
pub mod aws_runtime;
pub mod azure_runtime;
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
    azure_predefined_role_id, dedupe_azure_role_bindings, AzureCustomRole, AzureGrantPlan,
    AzureRoleBinding, AzureRoleDefinition, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator,
};
pub use gcp_runtime::{
    custom_role_permission_set_prefix, custom_role_prefix, GcpBindingResourceKind,
    GcpBindingTargetScope, GcpCustomRole, GcpGrantPlan, GcpIamBinding, GcpIamBindings,
    GcpIamCondition, GcpRuntimePermissionsGenerator,
};
