use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{aws_ownership::is_tagged_as_ours, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag, IamApi, IamRoleWriter};
use alien_core::{
    standard_resource_tags, ArtifactRegistry, ArtifactRegistryHeartbeatData,
    ArtifactRegistryHeartbeatStatus, ArtifactRegistryOutputs, AwsEcrArtifactRegistryHeartbeatData,
    AwsEcrRepositoryHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};

use alien_aws_clients::aws::ecr::{DescribeRepositoriesRequest, Repository};
use chrono::Utc;

fn role_name_from_arn(arn: &str) -> Option<&str> {
    arn.rsplit_once(':')?
        .1
        .strip_prefix("role/")
        .filter(|role_name| !role_name.is_empty())
}

fn fallback_role_name(resource_prefix: &str, resource_id: &str, suffix: &str) -> String {
    format!("{}-{}-{}", resource_prefix, resource_id, suffix)
}

fn is_conflict(error: &AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
    )
}

fn is_not_found(error: &AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    )
}

/// The name of a role recorded by its ARN. Roles this controller creates have no path.
fn recorded_role_name(arn: &str, resource_id: &str) -> Result<String> {
    role_name_from_arn(arn).map(str::to_string).ok_or_else(|| {
        AlienError::new(ErrorData::ResourceStateSerializationFailed {
            resource_id: resource_id.to_string(),
            message: format!("Recorded IAM role ARN '{}' has no role name", arn),
        })
    })
}

/// What holds an IAM role name, as far as this deployment is concerned.
enum ExistingRole {
    Missing,
    /// Carries this deployment's tags for the resource.
    Ours {
        arn: String,
    },
    /// Exists without them: someone else made it.
    Foreign,
}

/// Finds the role named `role_name` and tells whether this deployment created it. The name is
/// derived from the deployment prefix and resource ID, so the account can already hold a role
/// with it that someone else made; only our tags show it is ours. GetRole returns the tags.
async fn inspect_role(
    iam_client: &dyn IamApi,
    role_name: &str,
    our_tags: &HashMap<String, String>,
    resource_id: &str,
) -> Result<ExistingRole> {
    let role = match iam_client.get_role(role_name).await {
        Ok(response) => response.get_role_result.role,
        Err(error) if is_not_found(&error) => return Ok(ExistingRole::Missing),
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to read IAM role '{}'", role_name),
                resource_id: Some(resource_id.to_string()),
            }));
        }
    };
    let tags: HashMap<String, String> = role
        .tags
        .map(|tags| tags.member)
        .unwrap_or_default()
        .into_iter()
        .map(|tag| (tag.key, tag.value))
        .collect();
    if is_tagged_as_ours(&tags, our_tags) {
        Ok(ExistingRole::Ours { arn: role.arn })
    } else {
        Ok(ExistingRole::Foreign)
    }
}

/// AWS Artifact Registry controller.
///
/// AWS ECR implicitly exists in every AWS account and region, but this controller
/// creates two IAM roles to manage access: one for pull permissions and one for push+pull permissions.
#[controller]
pub struct AwsArtifactRegistryController {
    /// AWS account ID for generating the ECR registry URL
    pub(crate) account_id: Option<String>,
    /// The AWS region for this registry
    pub(crate) region: Option<String>,
    /// The ARN of the pull role
    pub(crate) pull_role_arn: Option<String>,
    /// The ARN of the push+pull role
    pub(crate) push_role_arn: Option<String>,
    /// The repository prefix (resource id)
    pub(crate) repository_prefix: Option<String>,
}

#[controller]
impl AwsArtifactRegistryController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;

        info!(
            registry_id = %config.id,
            region = %aws_cfg.region,
            "Setting up AWS ECR registry with roles"
        );

        let account_id = aws_cfg.account_id.to_string();

        // Store the repository prefix using resource_prefix-config.id pattern
        self.repository_prefix = Some(format!("{}-{}", ctx.resource_prefix, config.id));

        info!(
            registry_id = %config.id,
            account_id = %account_id,
            region = %aws_cfg.region,
            repository_prefix = %self.repository_prefix.as_deref().unwrap_or("unknown"),
            "AWS ECR registry is ready (implicitly exists)"
        );

        self.account_id = Some(account_id);
        self.region = Some(aws_cfg.region.clone());

        Ok(HandlerAction::Continue {
            state: CreatingPullRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPullRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_pull_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");

        info!(
            role_name = %pull_role_name,
            "Creating pull role for artifact registry"
        );

        let description = format!("Runtime ECR pull role for registry {}", config.id);
        self.pull_role_arn = Some(
            self.create_or_resume_role(ctx, &*iam_client, &pull_role_name, description, &config.id)
                .await?,
        );

        info!(
            role_name = %pull_role_name,
            role_arn = %self.pull_role_arn.as_deref().unwrap_or("unknown"),
            "Pull role created successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPullRolePolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");
        let policy_name = "ECRPullPolicy";

        // Create policy document for ECR pull permissions
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&pull_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{pull_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .put_role_policy(&pull_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to attach pull policy to role '{}'", pull_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role policy attached successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPushRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPushRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");

        info!(
            role_name = %push_role_name,
            "Creating push role for artifact registry"
        );

        let description = format!("Runtime ECR push role for registry {}", config.id);
        self.push_role_arn = Some(
            self.create_or_resume_role(ctx, &*iam_client, &push_role_name, description, &config.id)
                .await?,
        );

        info!(
            role_name = %push_role_name,
            role_arn = %self.push_role_arn.as_deref().unwrap_or("unknown"),
            "Push role created successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPushRolePolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");
        let policy_name = "ECRPushPolicy";

        // Create policy document for ECR push+pull permissions
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&push_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{push_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .put_role_policy(&push_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to attach push policy to role '{}'", push_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role policy attached successfully"
        );

        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;

        info!(registry=%config.id, "Applying resource-scoped permissions for ECR repository");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "artifact-registry",
            )
            .await?;
        }

        info!(registry=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: ConfiguringReplication,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringReplication,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_replication(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;

        info!(
            registry_id = %config.id,
            "Starting AWS ECR registry update (checking if policies need updating)"
        );

        // Always go through the policy update flow for consistency
        // Even if no updates are needed, we verify the policies are correct
        Ok(HandlerAction::Continue {
            state: UpdatingPullRoleTrustPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPullRoleTrustPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_pull_role_trust_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&pull_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{pull_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .update_assume_role_policy(&pull_role_name, &assume_role_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to update pull role trust policy for '{}'",
                    pull_role_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role trust policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPullRolePolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");
        let policy_name = "ECRPullPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&pull_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{pull_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .put_role_policy(&pull_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update pull role policy for '{}'", pull_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPushRoleTrustPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPushRoleTrustPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_push_role_trust_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&push_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{push_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .update_assume_role_policy(&push_role_name, &assume_role_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to update push role trust policy for '{}'",
                    push_role_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role trust policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPushRolePolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");
        let policy_name = "ECRPushPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        IamRoleWriter::new(iam_client.as_ref(), ctx.aws_permissions_boundary_arn())
            .prepare_existing(&push_role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare IAM role '{push_role_name}' for writing"),
                resource_id: Some(config.id.clone()),
            })?;
        iam_client
            .put_role_policy(&push_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update push role policy for '{}'", push_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingReplication,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingReplication,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_replication(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        Self::validate_replication(config)?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Deleting AWS ECR registry roles"
        );

        Ok(HandlerAction::Continue {
            state: DeletingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPullRolePolicy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        self.pull_role_arn = owned_role_arn(
            &*iam_client,
            self.pull_role_arn.clone(),
            ctx.resource_prefix,
            &config.id,
            "pull",
        )
        .await?;
        let Some(pull_role_arn) = self.pull_role_arn.as_deref() else {
            return Ok(HandlerAction::Continue {
                state: DeletingPushRolePolicy,
                suggested_delay: None,
            });
        };
        let pull_role_name = recorded_role_name(pull_role_arn, &config.id)?;

        self.cleanup_role_policies(ctx, &*iam_client, &pull_role_name, "pull", &config.id)
            .await?;

        Ok(HandlerAction::Continue {
            state: DeletingPullRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPullRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_pull_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        self.pull_role_arn = owned_role_arn(
            &*iam_client,
            self.pull_role_arn.clone(),
            ctx.resource_prefix,
            &config.id,
            "pull",
        )
        .await?;
        let Some(pull_role_arn) = self.pull_role_arn.as_deref() else {
            return Ok(HandlerAction::Continue {
                state: DeletingPushRolePolicy,
                suggested_delay: None,
            });
        };
        let pull_role_name = recorded_role_name(pull_role_arn, &config.id)?;

        // Delete pull role - treat NotFound as success for idempotent deletion
        match iam_client.delete_role(&pull_role_name).await {
            Ok(_) => {
                info!(role_name = %pull_role_name, "Pull role deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %pull_role_name, "Pull role already deleted");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                info!(
                    role_name = %pull_role_name,
                    "Pull role still has policies attached; retrying policy cleanup"
                );
                return Ok(HandlerAction::Continue {
                    state: DeletingPullRolePolicy,
                    suggested_delay: Some(Duration::from_secs(5)),
                });
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete pull role '{}'", pull_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.pull_role_arn = None;

        Ok(HandlerAction::Continue {
            state: DeletingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPushRolePolicy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let owned_push_role_arn = owned_role_arn(
            &*iam_client,
            self.push_role_arn.clone(),
            ctx.resource_prefix,
            &config.id,
            "push",
        )
        .await?;
        if owned_push_role_arn.is_some() {
            self.push_role_arn = owned_push_role_arn.clone();
        }
        let Some(push_role_arn) = owned_push_role_arn.as_deref() else {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };
        let push_role_name = recorded_role_name(push_role_arn, &config.id)?;

        self.cleanup_role_policies(ctx, &*iam_client, &push_role_name, "push", &config.id)
            .await?;

        Ok(HandlerAction::Continue {
            state: DeletingPushRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPushRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let owned_push_role_arn = owned_role_arn(
            &*iam_client,
            self.push_role_arn.clone(),
            ctx.resource_prefix,
            &config.id,
            "push",
        )
        .await?;
        if owned_push_role_arn.is_some() {
            self.push_role_arn = owned_push_role_arn.clone();
        }
        let Some(push_role_arn) = owned_push_role_arn.as_deref() else {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };
        let push_role_name = recorded_role_name(push_role_arn, &config.id)?;

        // Delete push role - treat NotFound as success for idempotent deletion
        match iam_client.delete_role(&push_role_name).await {
            Ok(_) => {
                info!(role_name = %push_role_name, "Push role deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %push_role_name, "Push role already deleted");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                info!(
                    role_name = %push_role_name,
                    "Push role still has policies attached; retrying policy cleanup"
                );
                return Ok(HandlerAction::Continue {
                    state: DeletingPushRolePolicy,
                    suggested_delay: Some(Duration::from_secs(5)),
                });
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete push role '{}'", push_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        // Keep the authenticated creation-owner identity as recovery metadata. Deleted
        // state proves the role is gone; erasing retained repositories still needs its ARN.
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Heartbeat check: verify stored account/region haven't drifted
        if let (Some(stored_account_id), Some(stored_region)) = (&self.account_id, &self.region) {
            // Check for configuration drift
            if stored_account_id != &aws_cfg.account_id.to_string() {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS account ID changed from {} to {}",
                        stored_account_id, aws_cfg.account_id
                    ),
                }));
            }

            if stored_region != &aws_cfg.region {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS region changed from {} to {}",
                        stored_region, aws_cfg.region
                    ),
                }));
            }

            debug!(account_id=%stored_account_id, region=%stored_region, "AWS ECR registry heartbeat check passed");

            let ecr_client = ctx.service_provider.get_aws_ecr_client(aws_cfg).await?;
            let repository_prefix = self
                .repository_prefix
                .clone()
                .unwrap_or_else(|| format!("{}-{}", ctx.resource_prefix, config.id));
            let repositories_response = ecr_client
                .describe_repositories(DescribeRepositoriesRequest {
                    registry_id: Some(stored_account_id.clone()),
                    repository_names: None,
                    next_token: None,
                    max_results: Some(100),
                })
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to describe ECR repositories during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let repositories = repositories_response
                .repositories
                .into_iter()
                .filter(|repository| repository.repository_name.starts_with(&repository_prefix))
                .collect::<Vec<_>>();

            emit_aws_artifact_registry_heartbeat(
                ctx,
                &config.id,
                stored_account_id,
                stored_region,
                &repository_prefix,
                self.pull_role_arn.clone(),
                self.push_role_arn.clone(),
                repositories_response.next_token.is_some(),
                repositories,
            );
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(account_id), Some(region)) = (&self.account_id, &self.region) {
            let registry_endpoint = format!("{}.dkr.ecr.{}.amazonaws.com", account_id, region);
            let registry_id = format!("{}:{}", account_id, region);
            Some(ResourceOutputs::new(ArtifactRegistryOutputs {
                registry_id,
                registry_endpoint,
                pull_role: self.pull_role_arn.clone(),
                push_role: self.push_role_arn.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::ArtifactRegistryBinding;

        if let Some(repository_prefix) = &self.repository_prefix {
            let binding = ArtifactRegistryBinding::ecr(
                repository_prefix.clone(),
                self.pull_role_arn.clone(),
                self.push_role_arn.clone(),
            );

            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

/// The ARN of the role to delete, or `None` when there is nothing this deployment may delete.
///
/// A role is recorded only once CreateRole succeeded, in this and every earlier version (or
/// when an import recorded what setup created). Without one, the role with the derived name is
/// deleted only if it carries our tags (a create whose response was lost); a role someone else
/// made is kept, and its policies are left alone.
async fn owned_role_arn(
    iam_client: &dyn IamApi,
    recorded_arn: Option<String>,
    resource_prefix: &str,
    resource_id: &str,
    role_label: &str,
) -> Result<Option<String>> {
    if recorded_arn.is_some() {
        return Ok(recorded_arn);
    }
    let role_name = fallback_role_name(resource_prefix, resource_id, role_label);
    let our_tags = standard_resource_tags(resource_prefix, resource_id);
    match inspect_role(iam_client, &role_name, &our_tags, resource_id).await? {
        ExistingRole::Ours { arn } => Ok(Some(arn)),
        ExistingRole::Missing => {
            info!(role_name = %role_name, role_label = %role_label, "No artifact registry role to delete");
            Ok(None)
        }
        ExistingRole::Foreign => {
            warn!(
                role_name = %role_name,
                role_label = %role_label,
                "Keeping IAM role and its policies: it is not tagged as created by this deployment"
            );
            Ok(None)
        }
    }
}

impl AwsArtifactRegistryController {
    /// Creates a role with this deployment's tags and returns its ARN. A role that already has
    /// the name is used only if it carries those tags: an earlier attempt created it and its
    /// response was lost. Any other role is refused and left untouched.
    fn validate_replication(config: &ArtifactRegistry) -> Result<()> {
        config.validate_aws_replication().context(ErrorData::ResourceConfigInvalid {
            resource_id: Some(config.id.clone()),
            message: "Per-deployment ECR replicationRegions is unsupported; shared account rules are left unchanged".to_string(),
        })
    }

    async fn create_or_resume_role(
        &self,
        ctx: &ResourceControllerContext<'_>,
        iam_client: &dyn IamApi,
        role_name: &str,
        description: String,
        resource_id: &str,
    ) -> Result<String> {
        let our_tags = standard_resource_tags(ctx.resource_prefix, resource_id);
        let request = CreateRoleRequest::builder()
            .role_name(role_name.to_string())
            .assume_role_policy_document(self.generate_service_account_assume_role_policy(ctx)?)
            .description(description)
            .tags(
                our_tags
                    .iter()
                    .map(|(key, value)| CreateRoleTag {
                        key: key.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            )
            .build();

        let error = match IamRoleWriter::new(iam_client, ctx.aws_permissions_boundary_arn())
            .create(request)
            .await
        {
            Ok(response) => return Ok(response.create_role_result.role.arn),
            Err(error) => error,
        };
        let create_failed = |error: AlienError<alien_client_core::ErrorData>| {
            error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create role '{}'", role_name),
                resource_id: Some(resource_id.to_string()),
            })
        };
        if !is_conflict(&error) {
            return Err(create_failed(error));
        }
        match inspect_role(iam_client, role_name, &our_tags, resource_id).await? {
            ExistingRole::Ours { arn } => {
                info!(role_name = %role_name, "Role was created by an earlier attempt; resuming");
                Ok(arn)
            }
            ExistingRole::Foreign => {
                Err(AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
                    resource_id: resource_id.to_string(),
                    object: format!("IAM role '{}'", role_name),
                }))
            }
            // Deleted since CreateRole answered; the retry creates it.
            ExistingRole::Missing => Err(create_failed(error)),
        }
    }

    async fn cleanup_role_policies(
        &self,
        ctx: &ResourceControllerContext<'_>,
        iam_client: &dyn IamApi,
        role_name: &str,
        role_label: &str,
        resource_id: &str,
    ) -> Result<()> {
        match IamRoleWriter::new(iam_client, ctx.aws_permissions_boundary_arn())
            .prepare_existing(role_name)
            .await
        {
            Ok(()) => {}
            Err(error) if is_not_found(&error) => return Ok(()),
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to prepare IAM role '{role_name}' for policy cleanup"),
                    resource_id: Some(resource_id.to_string()),
                }))
            }
        }
        match iam_client.list_attached_role_policies(role_name).await {
            Ok(response) => {
                if let Some(attached_policies) = response
                    .list_attached_role_policies_result
                    .attached_policies
                {
                    for policy in attached_policies.member {
                        match iam_client
                            .detach_role_policy(role_name, &policy.policy_arn)
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_arn = %policy.policy_arn,
                                    role_label = %role_label,
                                    "Artifact registry role managed policy detached successfully"
                                );
                            }
                            Err(e)
                                if matches!(
                                    e.error,
                                    Some(
                                        alien_client_core::ErrorData::RemoteResourceNotFound { .. }
                                    )
                                ) =>
                            {
                                info!(
                                    role_name = %role_name,
                                    policy_arn = %policy.policy_arn,
                                    role_label = %role_label,
                                    "Artifact registry role managed policy already detached"
                                );
                            }
                            Err(e) => {
                                return Err(e.into_alien_error().context(
                                    ErrorData::CloudPlatformError {
                                        message: format!(
                                            "Failed to detach {} role policy '{}' from '{}'",
                                            role_label, policy.policy_arn, role_name
                                        ),
                                        resource_id: Some(resource_id.to_string()),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %role_name, role_label = %role_label, "Artifact registry role already deleted");
                return Ok(());
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to list attached policies for '{}'", role_name),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
        }

        match iam_client.list_role_policies(role_name).await {
            Ok(response) => {
                if let Some(policy_names) = response.list_role_policies_result.policy_names {
                    for policy_name in policy_names.member {
                        match iam_client.delete_role_policy(role_name, &policy_name).await {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_name = %policy_name,
                                    role_label = %role_label,
                                    "Artifact registry role inline policy deleted successfully"
                                );
                            }
                            Err(e)
                                if matches!(
                                    e.error,
                                    Some(
                                        alien_client_core::ErrorData::RemoteResourceNotFound { .. }
                                    )
                                ) =>
                            {
                                info!(
                                    role_name = %role_name,
                                    policy_name = %policy_name,
                                    role_label = %role_label,
                                    "Artifact registry role inline policy already deleted"
                                );
                            }
                            Err(e) => {
                                return Err(e.into_alien_error().context(
                                    ErrorData::CloudPlatformError {
                                        message: format!(
                                            "Failed to delete {} role policy '{}' from '{}'",
                                            role_label, policy_name, role_name
                                        ),
                                        resource_id: Some(resource_id.to_string()),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %role_name, role_label = %role_label, "Artifact registry role already deleted");
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to list inline policies for '{}'", role_name),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
        }

        Ok(())
    }

    /// Generates an assume role policy that allows service account roles in the stack to assume this role
    /// In the new permission system, service accounts are created from permission profiles,
    /// and the artifact registry roles can be assumed by any service account role in the stack
    fn generate_service_account_assume_role_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;
        let mut service_account_role_arns = Vec::new();

        // Find all service account roles in the stack that could assume this role
        for (resource_id, resource_entry) in ctx.desired_stack.resources() {
            if let Some(_service_account) = resource_entry
                .config
                .downcast_ref::<alien_core::ServiceAccount>()
            {
                // Generate the role ARN for this service account
                let role_name = format!("{}-{}", ctx.resource_prefix, resource_id);
                let role_arn = format!("arn:aws:iam::{}:role/{}", aws_cfg.account_id, role_name);
                service_account_role_arns.push(role_arn);
            }
        }

        // If no service accounts found, create a minimal policy (shouldn't happen in practice)
        if service_account_role_arns.is_empty() {
            let policy = serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Deny",
                        "Principal": "*",
                        "Action": "*"
                    }
                ]
            });
            return Ok(policy.to_string());
        }

        // Create the principal object with service account role ARNs
        let principal = if service_account_role_arns.len() == 1 {
            serde_json::json!({
                "AWS": service_account_role_arns[0]
            })
        } else {
            serde_json::json!({
                "AWS": service_account_role_arns
            })
        };

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": principal,
                    "Action": "sts:AssumeRole"
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Generates ECR pull policy for the pull role
    fn generate_ecr_pull_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        registry_id: &str,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:GetAuthorizationToken"
                    ],
                    "Resource": "*"
                },
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:BatchCheckLayerAvailability",
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage",
                        "ecr:DescribeRepositories",
                        "ecr:DescribeImages",
                        "ecr:ListImages"
                    ],
                    "Resource": format!("arn:aws:ecr:{}:{}:repository/{}-{}-*", aws_cfg.region, aws_cfg.account_id, ctx.resource_prefix, registry_id)
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Generates ECR push+pull policy for the push role
    fn generate_ecr_push_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        registry_id: &str,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:GetAuthorizationToken"
                    ],
                    "Resource": "*"
                },
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:BatchCheckLayerAvailability",
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage",
                        "ecr:CompleteLayerUpload",
                        "ecr:UploadLayerPart",
                        "ecr:InitiateLayerUpload",
                        "ecr:PutImage",
                        "ecr:DescribeRepositories",
                        "ecr:DescribeImages",
                        "ecr:ListImages",
                        "ecr:CreateRepository",
                        "ecr:DeleteRepository"
                    ],
                    "Resource": format!("arn:aws:ecr:{}:{}:repository/{}-{}-*", aws_cfg.region, aws_cfg.account_id, ctx.resource_prefix, registry_id)
                },
                {
                    "Sid": "RegionalRepositoryPolicies",
                    "Effect": "Allow",
                    "Action": ["ecr:GetRepositoryPolicy", "ecr:DeleteRepositoryPolicy", "ecr:SetRepositoryPolicy", "ecr:DescribeRepositories"],
                    "Resource": format!("arn:aws:ecr:*:{}:repository/{}-{}-*", aws_cfg.account_id, ctx.resource_prefix, registry_id)
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Create a mock controller for testing
    #[cfg(test)]
    pub fn mock_ready(account_id: &str, region: &str) -> Self {
        Self {
            state: AwsArtifactRegistryState::Ready,
            account_id: Some(account_id.to_string()),
            region: Some(region.to_string()),
            pull_role_arn: Some(format!("arn:aws:iam::{}:role/test-pull-role", account_id)),
            push_role_arn: Some(format!("arn:aws:iam::{}:role/test-push-role", account_id)),
            repository_prefix: Some("test-artifact-registry".to_string()),
            _internal_stay_count: None,
        }
    }
}

fn emit_aws_artifact_registry_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    registry_id: &str,
    region: &str,
    repository_prefix: &str,
    pull_role_arn: Option<String>,
    push_role_arn: Option<String>,
    repositories_truncated: bool,
    repositories: Vec<Repository>,
) {
    let repository_data = repositories
        .into_iter()
        .map(|repository| AwsEcrRepositoryHeartbeatData {
            repository_arn: repository.repository_arn,
            registry_id: repository.registry_id,
            repository_name: repository.repository_name,
            repository_uri: repository.repository_uri,
            created_at: repository.created_at,
            image_tag_mutability: repository.image_tag_mutability,
            scan_on_push: repository
                .image_scanning_configuration
                .and_then(|config| config.scan_on_push),
            encryption_type: repository
                .encryption_configuration
                .as_ref()
                .map(|config| config.encryption_type.clone()),
            kms_key_present: repository
                .encryption_configuration
                .as_ref()
                .and_then(|config| config.kms_key.as_ref())
                .is_some(),
        })
        .collect::<Vec<_>>();
    let repository_count = repository_data.len() as u32;

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ArtifactRegistry::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ArtifactRegistry(ArtifactRegistryHeartbeatData::AwsEcr(
            AwsEcrArtifactRegistryHeartbeatData {
                status: ArtifactRegistryHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "AWS ECR registry '{}' in '{}' is reachable",
                        registry_id, region
                    )),
                    stale: false,
                    partial: repositories_truncated,
                    collection_issues: vec![],
                },
                registry_id: registry_id.to_string(),
                region: region.to_string(),
                registry_uri: format!("{registry_id}.dkr.ecr.{region}.amazonaws.com"),
                repository_prefix: repository_prefix.to_string(),
                pull_role_arn,
                push_role_arn,
                repository_count,
                repositories_truncated,
                repositories: repository_data,
            },
        )),
        raw: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_aws_clients::iam::{
        AttachedPolicies, AttachedPolicy, CreateRoleResponse, CreateRoleResult, GetRoleResponse,
        GetRoleResult, ListAttachedRolePoliciesResponse, ListAttachedRolePoliciesResult,
        ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, PolicyNames, Role, Tag, Tags,
    };
    use alien_core::Platform;
    use rstest::rstest;
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn create_successful_role_response(role_name: &str) -> CreateRoleResponse {
        CreateRoleResponse {
            create_role_result: CreateRoleResult {
                role: Role {
                    path: "/".to_string(),
                    role_name: role_name.to_string(),
                    role_id: "AROAEXAMPLE123".to_string(),
                    arn: format!("arn:aws:iam::123456789012:role/{}", role_name),
                    create_date: "2023-01-01T00:00:00Z".to_string(),
                    assume_role_policy_document: None,
                    description: None,
                    max_session_duration: None,
                    permissions_boundary: None,
                    tags: None,
                    role_last_used: None,
                },
            },
        }
    }

    fn setup_mock_client_for_creation_and_deletion() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful pull role creation
        mock_iam
            .expect_create_role()
            .returning(|request| Ok(create_successful_role_response(&request.role_name)));

        // Mock successful policy attachment
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));

        // Mock successful policy deletion (for both roles)
        mock_iam
            .expect_list_attached_role_policies()
            .returning(|_| Ok(empty_attached_role_policies_response()));
        mock_iam.expect_list_role_policies().returning(|role_name| {
            let policy_name = if role_name.ends_with("-pull") {
                "ECRPullPolicy"
            } else {
                "ECRPushPolicy"
            };
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec![policy_name.to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        mock_iam
            .expect_delete_role_policy()
            .returning(|_, _| Ok(()));

        // Mock successful role deletion (for both roles)
        mock_iam.expect_delete_role().returning(|_| Ok(()));

        Arc::new(mock_iam)
    }

    fn setup_mock_client_for_creation_and_update() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful pull role creation
        mock_iam
            .expect_create_role()
            .returning(|request| Ok(create_successful_role_response(&request.role_name)));

        // Mock successful policy attachment (for both create and update)
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));

        // Mock successful trust policy update (for updates)
        mock_iam
            .expect_update_assume_role_policy()
            .returning(|_, _| Ok(()));

        Arc::new(mock_iam)
    }

    fn setup_mock_service_provider(mock_iam: Arc<MockIamApi>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        Arc::new(mock_provider)
    }

    fn empty_attached_role_policies_response() -> ListAttachedRolePoliciesResponse {
        ListAttachedRolePoliciesResponse {
            list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                attached_policies: Some(AttachedPolicies { member: vec![] }),
                is_truncated: Some(false),
                marker: None,
            },
        }
    }

    fn empty_inline_role_policies_response() -> ListRolePoliciesResponse {
        ListRolePoliciesResponse {
            list_role_policies_result: ListRolePoliciesResult {
                policy_names: Some(PolicyNames { member: vec![] }),
                is_truncated: Some(false),
                marker: None,
            },
        }
    }

    const TEST_BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-permissions-boundary";

    fn bounded_management() -> crate::remote_stack_management::AwsRemoteStackManagementController {
        let mut controller =
            crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                "test-management",
            );
        controller.aws_permissions_boundary_arn = Some(TEST_BOUNDARY.to_string());
        controller
    }

    #[tokio::test]
    async fn bounded_registry_creation_passes_boundary_to_both_roles() {
        for state in [
            AwsArtifactRegistryState::CreatingPullRole,
            AwsArtifactRegistryState::CreatingPushRole,
        ] {
            let mut iam = MockIamApi::new();
            iam.expect_create_role().times(1).returning(|request| {
                assert_eq!(request.permissions_boundary.as_deref(), Some(TEST_BOUNDARY));
                Ok(create_successful_role_response(&request.role_name))
            });
            let mut executor = SingleControllerExecutor::builder()
                .resource(basic_artifact_registry())
                .controller(AwsArtifactRegistryController {
                    state,
                    ..Default::default()
                })
                .platform(Platform::Aws)
                .with_dependency(
                    alien_core::RemoteStackManagement::new("management".to_string()).build(),
                    bounded_management(),
                )
                .service_provider(setup_mock_service_provider(Arc::new(iam)))
                .build()
                .await
                .unwrap();
            executor.step().await.unwrap();
            let controller = executor
                .internal_state::<AwsArtifactRegistryController>()
                .unwrap();
            assert!(controller.pull_role_arn.is_some() || controller.push_role_arn.is_some());
        }
    }

    #[tokio::test]
    async fn bounded_registry_adopts_before_each_policy_or_trust_write() {
        for (state, trust) in [
            (AwsArtifactRegistryState::CreatingPullRolePolicy, false),
            (AwsArtifactRegistryState::CreatingPushRolePolicy, false),
            (AwsArtifactRegistryState::UpdatingPullRolePolicy, false),
            (AwsArtifactRegistryState::UpdatingPushRolePolicy, false),
            (AwsArtifactRegistryState::UpdatingPullRoleTrustPolicy, true),
            (AwsArtifactRegistryState::UpdatingPushRoleTrustPolicy, true),
        ] {
            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            iam.expect_get_role()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|name| {
                    Ok(GetRoleResponse {
                        get_role_result: GetRoleResult {
                            role: create_successful_role_response(name)
                                .create_role_result
                                .role,
                        },
                    })
                });
            iam.expect_list_attached_role_policies()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|_| Ok(empty_attached_role_policies_response()));
            iam.expect_put_role_permissions_boundary()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|_, arn| {
                    assert_eq!(arn, TEST_BOUNDARY);
                    Ok(())
                });
            if trust {
                iam.expect_update_assume_role_policy()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(|_, _| Ok(()));
            } else {
                iam.expect_put_role_policy()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(|_, _, _| Ok(()));
            }
            let mut executor = SingleControllerExecutor::builder()
                .resource(basic_artifact_registry())
                .controller(AwsArtifactRegistryController {
                    state,
                    ..Default::default()
                })
                .platform(Platform::Aws)
                .with_dependency(
                    alien_core::RemoteStackManagement::new("management".to_string()).build(),
                    bounded_management(),
                )
                .service_provider(setup_mock_service_provider(Arc::new(iam)))
                .build()
                .await
                .unwrap();
            executor.step().await.expect("adoption precedes the write");
        }
    }

    #[tokio::test]
    async fn bounded_registry_cleanup_adopts_before_removing_policies_and_tolerates_missing_roles()
    {
        for missing in [false, true] {
            for pull in [false, true] {
                let mut iam = MockIamApi::new();
                let mut sequence = mockall::Sequence::new();
                iam.expect_get_role()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(move |name| {
                        if missing {
                            Err(AlienError::new(
                                alien_client_core::ErrorData::RemoteResourceNotFound {
                                    resource_type: "IAM role".to_string(),
                                    resource_name: name.to_string(),
                                },
                            ))
                        } else {
                            Ok(GetRoleResponse {
                                get_role_result: GetRoleResult {
                                    role: create_successful_role_response(name)
                                        .create_role_result
                                        .role,
                                },
                            })
                        }
                    });
                if !missing {
                    iam.expect_list_attached_role_policies()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(|_| Ok(empty_attached_role_policies_response()));
                    iam.expect_put_role_permissions_boundary()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(|_, arn| {
                            assert_eq!(arn, TEST_BOUNDARY);
                            Ok(())
                        });
                    iam.expect_list_attached_role_policies()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(|_| Ok(empty_attached_role_policies_response()));
                    iam.expect_list_role_policies()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(|_| {
                            Ok(ListRolePoliciesResponse {
                                list_role_policies_result: ListRolePoliciesResult {
                                    policy_names: Some(PolicyNames {
                                        member: vec!["old-policy".into()],
                                    }),
                                    is_truncated: Some(false),
                                    marker: None,
                                },
                            })
                        });
                    iam.expect_delete_role_policy()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(|_, name| {
                            assert_eq!(name, "old-policy");
                            Ok(())
                        });
                }
                let mut executor = SingleControllerExecutor::builder()
                    .resource(basic_artifact_registry())
                    .controller(AwsArtifactRegistryController {
                        state: if pull {
                            AwsArtifactRegistryState::DeletingPullRolePolicy
                        } else {
                            AwsArtifactRegistryState::DeletingPushRolePolicy
                        },
                        pull_role_arn: Some("arn:aws:iam::123456789012:role/recorded-pull".into()),
                        push_role_arn: Some("arn:aws:iam::123456789012:role/recorded-push".into()),
                        ..Default::default()
                    })
                    .platform(Platform::Aws)
                    .with_dependency(
                        alien_core::RemoteStackManagement::new("management".to_string()).build(),
                        bounded_management(),
                    )
                    .service_provider(setup_mock_service_provider(Arc::new(iam)))
                    .build()
                    .await
                    .unwrap();
                executor.step().await.expect("cleanup can advance");
            }
        }
    }

    #[tokio::test]
    async fn bounded_registry_refuses_unsupported_adoption_before_writing() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role().times(1).returning(|name| {
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: create_successful_role_response(name)
                        .create_role_result
                        .role,
                },
            })
        });
        iam.expect_list_attached_role_policies()
            .times(1)
            .returning(|_| {
                Ok(ListAttachedRolePoliciesResponse {
                    list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                        attached_policies: Some(AttachedPolicies {
                            member: vec![AttachedPolicy {
                                policy_name: "AmazonEC2ReadOnlyAccess".into(),
                                policy_arn: "arn:aws:iam::aws:policy/AmazonEC2ReadOnlyAccess"
                                    .into(),
                            }],
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });
        // Neither adoption nor the trust write is allowed after an unsupported-policy audit.
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_artifact_registry())
            .controller(AwsArtifactRegistryController {
                state: AwsArtifactRegistryState::UpdatingPullRoleTrustPolicy,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
                bounded_management(),
            )
            .service_provider(setup_mock_service_provider(Arc::new(iam)))
            .build()
            .await
            .unwrap();
        let error = executor.step().await.expect_err("unsafe adoption fails");
        assert!(format!("{error:?}").contains("AmazonEC2ReadOnlyAccess"));
        assert!(matches!(
            executor
                .internal_state::<AwsArtifactRegistryController>()
                .unwrap()
                .state,
            AwsArtifactRegistryState::UpdatingPullRoleTrustPolicy
        ));
    }

    #[tokio::test]
    async fn replication_settings_leave_account_configuration_unchanged() {
        for state in [
            AwsArtifactRegistryState::ConfiguringReplication,
            AwsArtifactRegistryState::UpdatingReplication,
        ] {
            let mut registry = ArtifactRegistry::new("images".to_string()).build();
            registry.replication_regions = vec!["us-west-2".to_string()];
            let saved = AwsArtifactRegistryController {
                state: state.clone(),
                account_id: Some("123456789012".to_string()),
                region: Some("us-east-1".to_string()),
                repository_prefix: Some("test-images".to_string()),
                pull_role_arn: Some("arn:aws:iam::123456789012:role/test-images-pull".to_string()),
                push_role_arn: Some("arn:aws:iam::123456789012:role/test-images-push".to_string()),
                ..Default::default()
            };
            let controller: AwsArtifactRegistryController =
                serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
            // No client expectations: the compatibility transition never writes account replication.
            let mut executor = SingleControllerExecutor::builder()
                .resource(registry)
                .controller(controller)
                .platform(Platform::Aws)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .build()
                .await
                .unwrap();
            executor
                .step()
                .await
                .expect("legacy replication settings warn and continue without account writes");
            let saved = executor
                .internal_state::<AwsArtifactRegistryController>()
                .unwrap();
            assert_eq!(saved.repository_prefix.as_deref(), Some("test-images"));
            assert!(saved.pull_role_arn.is_some());
            assert!(saved.push_role_arn.is_some());
        }
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as AwsClientConfig::mock()
        let account_id = "123456789012";
        let region = "us-east-1";

        let mock_iam = setup_mock_client_for_creation_and_deletion();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(AwsArtifactRegistryController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Test create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs
        let outputs = executor.outputs().unwrap();
        let registry_outputs = outputs.downcast_ref::<ArtifactRegistryOutputs>().unwrap();

        assert_eq!(
            registry_outputs.registry_id,
            format!("{}:{}", account_id, region)
        );
        assert_eq!(
            registry_outputs.registry_endpoint,
            format!("{}.dkr.ecr.{}.amazonaws.com", account_id, region)
        );
        assert!(registry_outputs.pull_role.is_some());
        assert!(registry_outputs.push_role.is_some());

        // Test delete flow
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_imported_delete_uses_persisted_role_arn_names() {
        let registry = ArtifactRegistry::new(
            "test-alien-artifact-registry-with-long-enough-name-to-overflow-iam-role-limit"
                .to_string(),
        )
        .build();

        let mut mock_iam = MockIamApi::new();
        mock_iam
            .expect_list_attached_role_policies()
            .returning(|role_name| {
                assert!(
                    matches!(role_name, "short-registry-pull" | "short-registry-push"),
                    "expected ARN-derived role name, got {role_name}"
                );
                Ok(empty_attached_role_policies_response())
            });
        mock_iam.expect_list_role_policies().returning(|role_name| {
            assert!(
                matches!(role_name, "short-registry-pull" | "short-registry-push"),
                "expected ARN-derived role name, got {role_name}"
            );
            Ok(empty_inline_role_policies_response())
        });
        mock_iam.expect_delete_role().returning(|role_name| {
            assert!(
                matches!(role_name, "short-registry-pull" | "short-registry-push"),
                "expected ARN-derived role name, got {role_name}"
            );
            Ok(())
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_iam));
        let controller = AwsArtifactRegistryController {
            state: AwsArtifactRegistryState::Ready,
            account_id: Some("123456789012".to_string()),
            region: Some("us-east-1".to_string()),
            pull_role_arn: Some("arn:aws:iam::123456789012:role/short-registry-pull".to_string()),
            push_role_arn: Some("arn:aws:iam::123456789012:role/short-registry-push".to_string()),
            repository_prefix: Some("test-registry".to_string()),
            _internal_stay_count: None,
        };

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as AwsClientConfig::mock()
        let _account_id = "123456789012";
        let _region = "us-east-1";

        let mock_iam = setup_mock_client_for_creation_and_update();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry.clone())
            .controller(AwsArtifactRegistryController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Initial creation
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Test update flow (should be no-op)
        executor.update(registry).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── OWNERSHIP TESTS ──────────────────────────────────

    /// One IAM role as the account sees it.
    #[derive(Debug, Clone, PartialEq)]
    struct FakeRole {
        tags: HashMap<String, String>,
        inline_policies: BTreeSet<String>,
        attached_policies: BTreeSet<String>,
    }

    /// The IAM roles in the account, by name.
    type FakeAccount = Arc<Mutex<HashMap<String, FakeRole>>>;

    fn no_such_role(role_name: &str) -> AlienError<alien_client_core::ErrorData> {
        AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
            resource_type: "IAM Resource".to_string(),
            resource_name: role_name.to_string(),
        })
    }

    fn role_named(role_name: &str, tags: &HashMap<String, String>) -> Role {
        let mut role = create_successful_role_response(role_name)
            .create_role_result
            .role;
        role.tags = Some(Tags {
            member: tags
                .iter()
                .map(|(key, value)| Tag {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
        });
        role
    }

    /// An IAM mock that keeps the account's roles, their tags and policies. CreateRole fails
    /// with EntityAlreadyExists on a taken name and DeleteRole with DeleteConflict while
    /// policies remain, like AWS. With `lose_create_response` the first CreateRole creates the
    /// role but its response is lost.
    fn fake_iam(account: FakeAccount, lose_create_response: bool) -> MockIamApi {
        let mut mock = MockIamApi::new();

        let state = account.clone();
        let lose = Arc::new(Mutex::new(lose_create_response));
        mock.expect_create_role().returning(move |request| {
            let mut roles = state.lock().unwrap();
            if roles.contains_key(&request.role_name) {
                return Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceConflict {
                        message: format!("Role with name {} already exists.", request.role_name),
                        resource_type: "IAM Resource".to_string(),
                        resource_name: request.role_name.clone(),
                    },
                ));
            }
            let tags = request
                .tags
                .unwrap_or_default()
                .into_iter()
                .map(|tag| (tag.key, tag.value))
                .collect();
            roles.insert(
                request.role_name.clone(),
                FakeRole {
                    tags,
                    inline_policies: BTreeSet::new(),
                    attached_policies: BTreeSet::new(),
                },
            );
            if std::mem::take(&mut *lose.lock().unwrap()) {
                return Err(AlienError::new(
                    alien_client_core::ErrorData::HttpRequestFailed {
                        message: "connection reset before the response arrived".to_string(),
                    },
                ));
            }
            Ok(create_successful_role_response(&request.role_name))
        });

        let state = account.clone();
        mock.expect_get_role().returning(move |role_name| {
            let roles = state.lock().unwrap();
            let role = roles
                .get(role_name)
                .ok_or_else(|| no_such_role(role_name))?;
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: role_named(role_name, &role.tags),
                },
            })
        });

        let state = account.clone();
        mock.expect_put_role_policy()
            .returning(move |role_name, policy_name, _| {
                let mut roles = state.lock().unwrap();
                let role = roles
                    .get_mut(role_name)
                    .ok_or_else(|| no_such_role(role_name))?;
                role.inline_policies.insert(policy_name.to_string());
                Ok(())
            });

        let state = account.clone();
        mock.expect_list_attached_role_policies()
            .returning(move |role_name| {
                let roles = state.lock().unwrap();
                let role = roles
                    .get(role_name)
                    .ok_or_else(|| no_such_role(role_name))?;
                Ok(ListAttachedRolePoliciesResponse {
                    list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                        attached_policies: Some(AttachedPolicies {
                            member: role
                                .attached_policies
                                .iter()
                                .map(|arn| AttachedPolicy {
                                    policy_name: arn.rsplit('/').next().unwrap().to_string(),
                                    policy_arn: arn.clone(),
                                })
                                .collect(),
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });

        let state = account.clone();
        mock.expect_detach_role_policy()
            .returning(move |role_name, policy_arn| {
                let mut roles = state.lock().unwrap();
                let role = roles
                    .get_mut(role_name)
                    .ok_or_else(|| no_such_role(role_name))?;
                role.attached_policies.remove(policy_arn);
                Ok(())
            });

        let state = account.clone();
        mock.expect_list_role_policies()
            .returning(move |role_name| {
                let roles = state.lock().unwrap();
                let role = roles
                    .get(role_name)
                    .ok_or_else(|| no_such_role(role_name))?;
                Ok(ListRolePoliciesResponse {
                    list_role_policies_result: ListRolePoliciesResult {
                        policy_names: Some(PolicyNames {
                            member: role.inline_policies.iter().cloned().collect(),
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });

        let state = account.clone();
        mock.expect_delete_role_policy()
            .returning(move |role_name, policy_name| {
                let mut roles = state.lock().unwrap();
                let role = roles
                    .get_mut(role_name)
                    .ok_or_else(|| no_such_role(role_name))?;
                role.inline_policies.remove(policy_name);
                Ok(())
            });

        let state = account.clone();
        mock.expect_delete_role().returning(move |role_name| {
            let mut roles = state.lock().unwrap();
            let role = roles
                .get(role_name)
                .ok_or_else(|| no_such_role(role_name))?;
            if !role.inline_policies.is_empty() || !role.attached_policies.is_empty() {
                return Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceConflict {
                        message: "Cannot delete entity, must delete policies first.".to_string(),
                        resource_type: "IAM Resource".to_string(),
                        resource_name: role_name.to_string(),
                    },
                ));
            }
            roles.remove(role_name);
            Ok(())
        });

        mock
    }

    /// A role someone made by hand in the same account, with its own policies.
    fn handmade_role() -> FakeRole {
        FakeRole {
            tags: HashMap::from([("owner".to_string(), "handmade".to_string())]),
            inline_policies: BTreeSet::from(["HandmadeInline".to_string()]),
            attached_policies: BTreeSet::from([
                "arn:aws:iam::aws:policy/ReadOnlyAccess".to_string()
            ]),
        }
    }

    fn our_role() -> FakeRole {
        FakeRole {
            tags: standard_resource_tags("test", "my-registry"),
            inline_policies: BTreeSet::from(["ECRPullPolicy".to_string()]),
            attached_policies: BTreeSet::new(),
        }
    }

    fn account_with(roles: &[(&str, FakeRole)]) -> FakeAccount {
        Arc::new(Mutex::new(
            roles
                .iter()
                .map(|(name, role)| (name.to_string(), role.clone()))
                .collect(),
        ))
    }

    async fn registry_executor(
        controller: AwsArtifactRegistryController,
        account: &FakeAccount,
        lose_create_response: bool,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(basic_artifact_registry())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(fake_iam(
                account.clone(),
                lose_create_response,
            ))))
            .with_test_dependencies()
            .build()
            .await
            .unwrap()
    }

    /// State an earlier version left after creation failed before recording a role.
    fn failed_create_state() -> AwsArtifactRegistryController {
        AwsArtifactRegistryController {
            state: AwsArtifactRegistryState::CreateFailed,
            account_id: Some("123456789012".to_string()),
            region: Some("us-east-1".to_string()),
            pull_role_arn: None,
            push_role_arn: None,
            repository_prefix: Some("test-my-registry".to_string()),
            _internal_stay_count: None,
        }
    }

    /// Normal create and delete: both roles are created with our tags and policies, then
    /// deleted with their policies.
    #[tokio::test]
    async fn create_then_delete_removes_our_roles() {
        let account = account_with(&[]);
        let mut executor =
            registry_executor(AwsArtifactRegistryController::default(), &account, false).await;

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        {
            let roles = account.lock().unwrap();
            assert_eq!(
                roles.keys().cloned().collect::<BTreeSet<_>>(),
                BTreeSet::from([
                    "test-my-registry-pull".to_string(),
                    "test-my-registry-push".to_string()
                ])
            );
            for role in roles.values() {
                assert_eq!(role.tags, standard_resource_tags("test", "my-registry"));
                assert_eq!(role.inline_policies.len(), 1);
            }
        }

        let owner = executor
            .internal_state::<AwsArtifactRegistryController>()
            .unwrap()
            .push_role_arn
            .clone()
            .unwrap();
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(account.lock().unwrap().is_empty());
        let saved: AwsArtifactRegistryController = serde_json::from_value(
            serde_json::to_value(
                executor
                    .internal_state::<AwsArtifactRegistryController>()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved.push_role_arn.as_deref(),
            Some(owner.as_str()),
            "retain must keep repository creation ownership after role deletion"
        );
    }

    #[tokio::test]
    async fn a_saved_final_delete_keeps_repository_ownership_when_the_role_is_already_gone() {
        let owner = "arn:aws:iam::123456789012:role/test-my-registry-push";
        let controller = AwsArtifactRegistryController {
            state: AwsArtifactRegistryState::DeletingPushRole,
            account_id: Some("123456789012".into()),
            region: Some("us-east-1".into()),
            repository_prefix: Some("test-my-registry".into()),
            push_role_arn: Some(owner.into()),
            ..Default::default()
        };
        let controller = serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        let account = account_with(&[]);
        let mut executor = registry_executor(controller, &account, false).await;
        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            executor
                .internal_state::<AwsArtifactRegistryController>()
                .unwrap()
                .push_role_arn
                .as_deref(),
            Some(owner)
        );
        assert!(account.lock().unwrap().is_empty());
    }

    /// A same-account role that already has the pull or push role's name was made by someone
    /// else. Creating refuses it, and removing the resource afterwards leaves it and its
    /// policies in place; only the role this deployment did create is deleted.
    #[rstest]
    #[case::pull("test-my-registry-pull", None)]
    #[case::push("test-my-registry-push", Some("test-my-registry-pull"))]
    #[tokio::test]
    async fn existing_role_without_our_tags_is_never_adopted_or_deleted(
        #[case] taken_name: &str,
        #[case] created_before_refusal: Option<&str>,
    ) {
        let account = account_with(&[(taken_name, handmade_role())]);
        let mut executor =
            registry_executor(AwsArtifactRegistryController::default(), &account, false).await;

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a role this deployment did not create is refused");
        let expected_object = format!("IAM role '{taken_name}'");
        assert!(
            matches!(
                &error.error,
                Some(crate::error::ErrorData::ExistingCloudResourceNotOwned { resource_id, object })
                    if resource_id == "my-registry" && *object == expected_object
            ),
            "{error:?}"
        );
        assert!(!error.retryable, "retrying cannot make the role ours");
        assert_eq!(
            account.lock().unwrap().get(taken_name),
            Some(&handmade_role())
        );
        if let Some(created) = created_before_refusal {
            assert_eq!(
                account
                    .lock()
                    .unwrap()
                    .get(created)
                    .map(|role| role.tags.clone()),
                Some(standard_resource_tags("test", "my-registry"))
            );
        }

        // The release that drops the resource deletes only what this deployment created.
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *account.lock().unwrap(),
            HashMap::from([(taken_name.to_string(), handmade_role())])
        );
    }

    /// State written by earlier versions after CreateRole failed on the name: no role
    /// recorded. Deleting it must neither get stuck nor touch the foreign roles.
    #[tokio::test]
    async fn delete_after_a_failed_create_keeps_foreign_roles() {
        let account = account_with(&[
            ("test-my-registry-pull", handmade_role()),
            ("test-my-registry-push", handmade_role()),
        ]);
        let before = account.lock().unwrap().clone();
        let mut executor = registry_executor(failed_create_state(), &account, false).await;

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), before);
    }

    /// A create whose response was lost left an unrecorded role carrying our tags. A delete
    /// removes it with its policies.
    #[tokio::test]
    async fn delete_after_an_interrupted_create_removes_our_tagged_role() {
        let account = account_with(&[("test-my-registry-pull", our_role())]);
        let mut executor = registry_executor(failed_create_state(), &account, false).await;

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(account.lock().unwrap().is_empty());
    }

    /// A retry after a lost CreateRole response resumes the role it created.
    #[tokio::test]
    async fn retry_after_a_lost_create_response_resumes_our_role() {
        let account = account_with(&[]);
        let mut executor =
            registry_executor(AwsArtifactRegistryController::default(), &account, true).await;

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the first attempt loses its response");
        assert!(error.retryable, "{error:?}");

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let controller = executor
            .internal_state::<AwsArtifactRegistryController>()
            .unwrap();
        assert_eq!(
            controller.pull_role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/test-my-registry-pull")
        );
        assert_eq!(
            controller.push_role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/test-my-registry-push")
        );
    }
}
