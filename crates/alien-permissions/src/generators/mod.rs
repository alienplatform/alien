pub mod aws_cloudformation;
pub mod aws_runtime;
pub mod azure_runtime;
pub mod gcp_runtime;

pub use aws_cloudformation::{
    AwsCloudFormationIamPolicy, AwsCloudFormationIamStatement,
    AwsCloudFormationPermissionsGenerator,
};
pub use aws_runtime::{AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator};
pub use azure_runtime::{
    AzureRoleAssignment, AzureRoleAssignmentProperties, AzureRoleDefinition,
    AzureRuntimePermissionsGenerator,
};
pub use gcp_runtime::{
    GcpCustomRole, GcpIamBinding, GcpIamBindings, GcpIamCondition, GcpRuntimePermissionsGenerator,
};
