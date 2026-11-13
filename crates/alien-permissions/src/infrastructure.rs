use alien_core::{
    StackSettings, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};

// Core types are re-exported by the generators that need them

/// Where the management identity may place the network-facing resources it creates on AWS:
/// instances, security groups and load balancers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwsNetworkScope {
    /// The stack's own VPC, known when setup renders the policy: setup created it or the
    /// installer named it. Subnet IDs are the ones a load balancer or Auto Scaling group may use.
    Vpc {
        vpc_id: String,
        subnet_ids: Vec<String>,
    },
    /// Management creates the network itself, after the policy is written. Its VPC and subnets
    /// carry the stack's ownership tags from creation, so tags stand in for IDs.
    StackTagged,
}

/// Label prefix of statements rendered when the stack's VPC is known or unknown, but not
/// tag-scoped. Without a known VPC their `${awsVpcId}` and `${awsSubnetIds}` render as `*`.
pub const VPC_PINNED_LABEL_PREFIX: &str = "vpc-pinned-";
/// Label prefix of statements rendered only for a network management creates itself.
pub const VPC_TAGGED_LABEL_PREFIX: &str = "vpc-tagged-";
/// Condition value that renders as the list of the stack's subnet IDs.
pub const AWS_SUBNET_IDS_VARIABLE: &str = "${awsSubnetIds}";

/// Context for generating permissions with type-safe variables
#[derive(Debug, Clone)]
pub struct PermissionContext {
    // AWS variables
    pub aws_account_id: Option<String>,
    pub aws_region: Option<String>,
    /// Setup-approved boundary ARN. Absent for deployments created without a boundary.
    pub aws_permissions_boundary_arn: Option<String>,
    /// The stack's network, when setup knows it. Absent renders the network conditions with
    /// wildcards (the account's default VPC, or a caller that has no network to name).
    pub aws_network: Option<AwsNetworkScope>,

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
    pub stack_name: Option<String>,
    pub deployment_name: Option<String>,
    pub resource_id: Option<String>,
    pub resource_name: Option<String>,
    pub service_account_name: Option<String>,
    pub principal_id: Option<String>,
    pub external_id: Option<String>,
    pub managing_role_arn: Option<String>,
    pub managing_account_id: Option<String>,

    /// GCP custom-role namespace to use instead of the one derived from
    /// `stack_prefix`, for deployments whose roles already use another one.
    pub gcp_custom_role_namespace: Option<String>,

    /// AWS account that publishes the machine images a protected install's verifier may
    /// boot. Set only when rendering the verifier; it is a constant of this build.
    pub machine_image_publisher: Option<String>,

    /// The install is protected: it runs only what the vendor signed. Set only when rendering
    /// the management identity of a protected install. It never receives the grants kept for
    /// setups that recorded no boundary, it loses what only the verifier may do (see
    /// [`crate::generators::verifier_owned`]), and the catalog's `signing-` statements apply.
    pub signing_protected: bool,
}

impl PermissionContext {
    /// Create a new permission context
    pub fn new() -> Self {
        Self {
            aws_account_id: None,
            aws_region: None,
            aws_permissions_boundary_arn: None,
            aws_network: None,
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
            stack_name: None,
            deployment_name: None,
            resource_id: None,
            resource_name: None,
            service_account_name: None,
            principal_id: None,
            external_id: None,
            managing_role_arn: None,
            managing_account_id: None,
            gcp_custom_role_namespace: None,
            machine_image_publisher: None,
            signing_protected: false,
        }
    }

    /// Builder pattern for AWS account ID
    pub fn with_aws_account_id(mut self, aws_account_id: impl Into<String>) -> Self {
        self.aws_account_id = Some(aws_account_id.into());
        self
    }

    /// Use the permissions boundary recorded by setup.
    pub fn with_aws_permissions_boundary_arn(mut self, arn: impl Into<String>) -> Self {
        self.aws_permissions_boundary_arn = Some(arn.into());
        self
    }

    /// Keep network-facing creates inside the stack's own network.
    pub fn with_aws_network(mut self, network: AwsNetworkScope) -> Self {
        self.aws_network = Some(network);
        self
    }

    /// The stack's VPC ID, or `*` when it is unknown or tag-scoped.
    pub fn aws_vpc_id(&self) -> &str {
        match &self.aws_network {
            Some(AwsNetworkScope::Vpc { vpc_id, .. }) => vpc_id,
            _ => "*",
        }
    }

    /// The stack's subnet IDs, or `*` when they are unknown or tag-scoped.
    pub fn aws_subnet_ids(&self) -> Vec<String> {
        match &self.aws_network {
            Some(AwsNetworkScope::Vpc { subnet_ids, .. }) if !subnet_ids.is_empty() => {
                subnet_ids.clone()
            }
            _ => vec!["*".to_string()],
        }
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

    /// Builder pattern for the user-facing stack name
    pub fn with_stack_name(mut self, stack_name: impl Into<String>) -> Self {
        self.stack_name = Some(stack_name.into());
        self
    }

    /// Builder pattern for the human-facing deployment name.
    pub fn with_deployment_name(mut self, deployment_name: impl Into<String>) -> Self {
        self.deployment_name = Some(deployment_name.into());
        self
    }

    /// Builder pattern for the Alien logical resource ID.
    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.resource_id = Some(resource_id.into());
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

    /// Builder pattern for the GCP custom-role namespace override
    pub fn with_gcp_custom_role_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.gcp_custom_role_namespace = Some(namespace.into());
        self
    }

    /// Builder pattern for the AWS account that publishes bootable machine images
    pub fn with_machine_image_publisher(mut self, account_id: impl Into<String>) -> Self {
        self.machine_image_publisher = Some(account_id.into());
        self
    }

    /// Builder pattern for a protected install's management identity
    pub fn with_signing_protected(mut self, protected: bool) -> Self {
        self.signing_protected = protected;
        self
    }

    /// Mark the management identity protected when the stack settings pin signing.
    pub fn with_signing(self, settings: &StackSettings) -> Self {
        self.with_signing_protected(settings.signing.is_some())
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
            "awsPermissionsBoundaryArn" => self.aws_permissions_boundary_arn.as_deref(),
            "awsVpcId" => Some(self.aws_vpc_id()),
            "projectName" => self.project_name.as_deref(),
            "projectNumber" => self.project_number.as_deref(),
            "region" => self.region.as_deref(),
            "subscriptionId" => self.subscription_id.as_deref(),
            "resourceGroup" => self.resource_group.as_deref(),
            "storageAccountName" => self.storage_account_name.as_deref(),
            "stackPrefix" => self.stack_prefix.as_deref(),
            "stackName" => self.stack_name.as_deref(),
            "deploymentName" => self.deployment_name.as_deref(),
            "resourceId" => self.resource_id.as_deref(),
            "resourceName" => self.resource_name.as_deref(),
            "serviceAccountName" => self.service_account_name.as_deref(),
            "principalId" => self.principal_id.as_deref(),
            "externalId" => self.external_id.as_deref(),
            "managingRoleArn" => self.managing_role_arn.as_deref(),
            "managingAccountId" => self.managing_account_id.as_deref(),
            "managingProjectId" => self.managing_project_id.as_deref(),
            "managingSubscriptionId" => self.managing_subscription_id.as_deref(),
            "managingResourceGroup" => self.managing_resource_group.as_deref(),
            "machineImagePublisher" => self.machine_image_publisher.as_deref(),
            "stackTag" => Some(ALIEN_STACK_TAG_KEY),
            "resourceTag" => Some(ALIEN_RESOURCE_TAG_KEY),
            "managedByTag" => Some(ALIEN_MANAGED_BY_TAG_KEY),
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
