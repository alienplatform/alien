pub mod error;
pub mod generators;
pub mod registry;
pub mod variables;

pub use error::*;
pub use registry::{get_permission_set, has_permission_set, list_permission_set_ids};
pub use variables::VariableInterpolator;

// Core types are re-exported by the generators that need them

/// Context for generating permissions with type-safe variables
#[derive(Debug, Clone)]
pub struct PermissionContext {
    // AWS variables
    pub aws_account_id: Option<String>,
    pub aws_region: Option<String>,

    // GCP variables
    pub project_name: Option<String>,
    pub project_number: Option<String>,
    pub region: Option<String>,

    // Azure variables
    pub subscription_id: Option<String>,
    pub resource_group: Option<String>,
    pub storage_account_name: Option<String>,

    // GCP cross-project management variables
    pub managing_project_id: Option<String>,

    // Azure cross-subscription management variables
    pub managing_subscription_id: Option<String>,
    pub managing_resource_group: Option<String>,

    // Common variables
    pub stack_prefix: Option<String>,
    pub resource_name: Option<String>,
    pub service_account_name: Option<String>,
    pub principal_id: Option<String>,
    pub external_id: Option<String>,
    pub managing_role_arn: Option<String>,
    pub managing_account_id: Option<String>,
}

impl PermissionContext {
    /// Create a new permission context
    pub fn new() -> Self {
        Self {
            aws_account_id: None,
            aws_region: None,
            project_name: None,
            project_number: None,
            region: None,
            subscription_id: None,
            resource_group: None,
            storage_account_name: None,
            managing_project_id: None,
            managing_subscription_id: None,
            managing_resource_group: None,
            stack_prefix: None,
            resource_name: None,
            service_account_name: None,
            principal_id: None,
            external_id: None,
            managing_role_arn: None,
            managing_account_id: None,
        }
    }

    /// Builder pattern for AWS account ID
    pub fn with_aws_account_id(mut self, aws_account_id: impl Into<String>) -> Self {
        self.aws_account_id = Some(aws_account_id.into());
        self
    }

    /// Builder pattern for AWS region
    pub fn with_aws_region(mut self, aws_region: impl Into<String>) -> Self {
        self.aws_region = Some(aws_region.into());
        self
    }

    /// Builder pattern for GCP project name
    pub fn with_project_name(mut self, project_name: impl Into<String>) -> Self {
        self.project_name = Some(project_name.into());
        self
    }

    /// Builder pattern for GCP project number (numeric, used in IAM condition expressions)
    pub fn with_project_number(mut self, project_number: impl Into<String>) -> Self {
        self.project_number = Some(project_number.into());
        self
    }

    /// Builder pattern for GCP region (used in artifact-registry, function, network permission sets)
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Builder pattern for Azure subscription ID
    pub fn with_subscription_id(mut self, subscription_id: impl Into<String>) -> Self {
        self.subscription_id = Some(subscription_id.into());
        self
    }

    /// Builder pattern for Azure resource group
    pub fn with_resource_group(mut self, resource_group: impl Into<String>) -> Self {
        self.resource_group = Some(resource_group.into());
        self
    }

    /// Builder pattern for Azure storage account name
    pub fn with_storage_account_name(mut self, storage_account_name: impl Into<String>) -> Self {
        self.storage_account_name = Some(storage_account_name.into());
        self
    }

    /// Builder pattern for GCP managing project ID (cross-project management)
    pub fn with_managing_project_id(mut self, id: impl Into<String>) -> Self {
        self.managing_project_id = Some(id.into());
        self
    }

    /// Builder pattern for Azure managing subscription ID (cross-subscription management)
    pub fn with_managing_subscription_id(mut self, id: impl Into<String>) -> Self {
        self.managing_subscription_id = Some(id.into());
        self
    }

    /// Builder pattern for Azure managing resource group (cross-subscription management)
    pub fn with_managing_resource_group(mut self, rg: impl Into<String>) -> Self {
        self.managing_resource_group = Some(rg.into());
        self
    }

    /// Builder pattern for stack prefix
    pub fn with_stack_prefix(mut self, stack_prefix: impl Into<String>) -> Self {
        self.stack_prefix = Some(stack_prefix.into());
        self
    }

    /// Builder pattern for resource name
    pub fn with_resource_name(mut self, resource_name: impl Into<String>) -> Self {
        self.resource_name = Some(resource_name.into());
        self
    }

    /// Builder pattern for service account name
    pub fn with_service_account_name(mut self, service_account_name: impl Into<String>) -> Self {
        self.service_account_name = Some(service_account_name.into());
        self
    }

    /// Builder pattern for principal ID
    pub fn with_principal_id(mut self, principal_id: impl Into<String>) -> Self {
        self.principal_id = Some(principal_id.into());
        self
    }

    /// Builder pattern for external ID
    pub fn with_external_id(mut self, external_id: impl Into<String>) -> Self {
        self.external_id = Some(external_id.into());
        self
    }

    /// Builder pattern for managing role ARN
    pub fn with_managing_role_arn(mut self, managing_role_arn: impl Into<String>) -> Self {
        self.managing_role_arn = Some(managing_role_arn.into());
        self
    }

    /// Builder pattern for managing account ID
    pub fn with_managing_account_id(mut self, managing_account_id: impl Into<String>) -> Self {
        self.managing_account_id = Some(managing_account_id.into());
        self
    }

    /// Extract AWS account ID from an IAM role ARN
    /// Format: arn:aws:iam::ACCOUNT_ID:role/ROLE_NAME
    pub fn extract_account_id_from_role_arn(role_arn: &str) -> Option<String> {
        let parts: Vec<&str> = role_arn.split(':').collect();
        if parts.len() >= 5 && parts[0] == "arn" && parts[2] == "iam" {
            Some(parts[4].to_string())
        } else {
            None
        }
    }

    /// Get a variable by name (for backward compatibility with interpolation)
    pub fn get_variable(&self, key: &str) -> Option<&str> {
        match key {
            "awsAccountId" => self.aws_account_id.as_deref(),
            "awsRegion" => self.aws_region.as_deref(),
            "projectName" => self.project_name.as_deref(),
            "projectNumber" => self.project_number.as_deref(),
            "region" => self.region.as_deref(),
            "subscriptionId" => self.subscription_id.as_deref(),
            "resourceGroup" => self.resource_group.as_deref(),
            "storageAccountName" => self.storage_account_name.as_deref(),
            "stackPrefix" => self.stack_prefix.as_deref(),
            "resourceName" => self.resource_name.as_deref(),
            "serviceAccountName" => self.service_account_name.as_deref(),
            "principalId" => self.principal_id.as_deref(),
            "externalId" => self.external_id.as_deref(),
            "managingRoleArn" => self.managing_role_arn.as_deref(),
            "managingAccountId" => self.managing_account_id.as_deref(),
            "managingProjectId" => self.managing_project_id.as_deref(),
            "managingSubscriptionId" => self.managing_subscription_id.as_deref(),
            "managingResourceGroup" => self.managing_resource_group.as_deref(),
            _ => None,
        }
    }
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Binding target type for permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTarget {
    /// Stack-level binding
    Stack,
    /// Resource-level binding  
    Resource,
}

impl std::fmt::Display for BindingTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindingTarget::Stack => write!(f, "stack"),
            BindingTarget::Resource => write!(f, "resource"),
        }
    }
}
