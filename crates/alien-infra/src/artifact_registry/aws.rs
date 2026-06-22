use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info};

use crate::aws_sdk::iam_result;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    standard_resource_tags, ArtifactRegistry, ArtifactRegistryHeartbeatData,
    ArtifactRegistryHeartbeatStatus, ArtifactRegistryOutputs, AwsEcrArtifactRegistryHeartbeatData,
    AwsEcrRepositoryHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use aws_sdk_ecr::{
    types::{ReplicationConfiguration, ReplicationDestination, ReplicationRule, Repository},
    Client as EcrClient,
};
use aws_sdk_iam::{types::Tag, Client as IamClient};

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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");

        info!(
            role_name = %pull_role_name,
            "Creating pull role for artifact registry"
        );

        // Create assume role policy that allows service account roles to assume this role
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        let tags = standard_resource_tags(ctx.resource_prefix, &config.id)
            .into_iter()
            .map(|(key, value)| {
                Tag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to build IAM tag for pull role '{}'",
                            pull_role_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let response = iam_result(
            iam_client
                .create_role()
                .role_name(&pull_role_name)
                .assume_role_policy_document(assume_role_policy)
                .description(format!("Runtime ECR pull role for registry {}", config.id))
                .set_tags(Some(tags))
                .send()
                .await,
            "CreateRole",
            "IAM Role",
            &pull_role_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create pull role '{}'", pull_role_name),
            resource_id: Some(config.id.clone()),
        })?;

        let pull_role_arn = response
            .role()
            .map(|role| role.arn().to_string())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "CreateRole response did not include pull role metadata".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        self.pull_role_arn = Some(pull_role_arn);

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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");
        let policy_name = "ECRPullPolicy";

        // Create policy document for ECR pull permissions
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        let role_policy_name = format!("{pull_role_name}/{policy_name}");
        iam_result(
            iam_client
                .put_role_policy()
                .role_name(&pull_role_name)
                .policy_name(policy_name)
                .policy_document(&policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &role_policy_name,
        )
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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");

        info!(
            role_name = %push_role_name,
            "Creating push role for artifact registry"
        );

        // Create assume role policy that allows service account roles to assume this role
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        let tags = standard_resource_tags(ctx.resource_prefix, &config.id)
            .into_iter()
            .map(|(key, value)| {
                Tag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to build IAM tag for push role '{}'",
                            push_role_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let response = iam_result(
            iam_client
                .create_role()
                .role_name(&push_role_name)
                .assume_role_policy_document(assume_role_policy)
                .description(format!("Runtime ECR push role for registry {}", config.id))
                .set_tags(Some(tags))
                .send()
                .await,
            "CreateRole",
            "IAM Role",
            &push_role_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create push role '{}'", push_role_name),
            resource_id: Some(config.id.clone()),
        })?;

        let push_role_arn = response
            .role()
            .map(|role| role.arn().to_string())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "CreateRole response did not include push role metadata".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        self.push_role_arn = Some(push_role_arn);

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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");
        let policy_name = "ECRPushPolicy";

        // Create policy document for ECR push+pull permissions
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        let role_policy_name = format!("{push_role_name}/{policy_name}");
        iam_result(
            iam_client
                .put_role_policy()
                .role_name(&push_role_name)
                .policy_name(policy_name)
                .policy_document(&policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &role_policy_name,
        )
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

        if config.replication_regions.is_empty() {
            info!(registry=%config.id, "No replication regions configured, skipping");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let ecr_client = ctx.service_provider.get_aws_ecr_client(aws_cfg).await?;
        let account_id = self
            .account_id
            .as_deref()
            .unwrap_or(&aws_cfg.account_id.to_string())
            .to_string();

        self.apply_replication_config(
            &ecr_client,
            &account_id,
            &aws_cfg.region,
            &config.replication_regions,
            &config.id,
        )
        .await?;

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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        iam_result(
            iam_client
                .update_assume_role_policy()
                .role_name(&pull_role_name)
                .policy_document(assume_role_policy)
                .send()
                .await,
            "UpdateAssumeRolePolicy",
            "IAM Role",
            &pull_role_name,
        )
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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "pull");
        let policy_name = "ECRPullPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        let role_policy_name = format!("{pull_role_name}/{policy_name}");
        iam_result(
            iam_client
                .put_role_policy()
                .role_name(&pull_role_name)
                .policy_name(policy_name)
                .policy_document(&policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &role_policy_name,
        )
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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        iam_result(
            iam_client
                .update_assume_role_policy()
                .role_name(&push_role_name)
                .policy_document(assume_role_policy)
                .send()
                .await,
            "UpdateAssumeRolePolicy",
            "IAM Role",
            &push_role_name,
        )
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
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = fallback_role_name(ctx.resource_prefix, &config.id, "push");
        let policy_name = "ECRPushPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        let role_policy_name = format!("{push_role_name}/{policy_name}");
        iam_result(
            iam_client
                .put_role_policy()
                .role_name(&push_role_name)
                .policy_name(policy_name)
                .policy_document(&policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &role_policy_name,
        )
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

        if config.replication_regions.is_empty() {
            info!(registry=%config.id, "No replication regions configured, skipping");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let ecr_client = ctx.service_provider.get_aws_ecr_client(aws_cfg).await?;
        let account_id = self
            .account_id
            .as_deref()
            .unwrap_or(&aws_cfg.account_id.to_string())
            .to_string();

        self.apply_replication_config(
            &ecr_client,
            &account_id,
            &aws_cfg.region,
            &config.replication_regions,
            &config.id,
        )
        .await?;

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

        let pull_role_name = self
            .pull_role_arn
            .as_deref()
            .and_then(role_name_from_arn)
            .map(str::to_string)
            .unwrap_or_else(|| fallback_role_name(ctx.resource_prefix, &config.id, "pull"));

        self.cleanup_role_policies(&iam_client, &pull_role_name, "pull", &config.id)
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

        let pull_role_name = self
            .pull_role_arn
            .as_deref()
            .and_then(role_name_from_arn)
            .map(str::to_string)
            .unwrap_or_else(|| fallback_role_name(ctx.resource_prefix, &config.id, "pull"));

        // Delete pull role - treat NotFound as success for idempotent deletion
        match iam_result(
            iam_client
                .delete_role()
                .role_name(&pull_role_name)
                .send()
                .await,
            "DeleteRole",
            "IAM Role",
            &pull_role_name,
        ) {
            Ok(_) => {
                info!(role_name = %pull_role_name, "Pull role deleted successfully");
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(role_name = %pull_role_name, "Pull role already deleted");
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceConflict { .. })) => {
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

        let push_role_name = self
            .push_role_arn
            .as_deref()
            .and_then(role_name_from_arn)
            .map(str::to_string)
            .unwrap_or_else(|| fallback_role_name(ctx.resource_prefix, &config.id, "push"));

        self.cleanup_role_policies(&iam_client, &push_role_name, "push", &config.id)
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

        let push_role_name = self
            .push_role_arn
            .as_deref()
            .and_then(role_name_from_arn)
            .map(str::to_string)
            .unwrap_or_else(|| fallback_role_name(ctx.resource_prefix, &config.id, "push"));

        // Delete push role - treat NotFound as success for idempotent deletion
        match iam_result(
            iam_client
                .delete_role()
                .role_name(&push_role_name)
                .send()
                .await,
            "DeleteRole",
            "IAM Role",
            &push_role_name,
        ) {
            Ok(_) => {
                info!(role_name = %push_role_name, "Push role deleted successfully");
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(role_name = %push_role_name, "Push role already deleted");
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceConflict { .. })) => {
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

        self.push_role_arn = None;

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
                .describe_repositories()
                .registry_id(stored_account_id)
                .max_results(100)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to describe ECR repositories during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let mut repositories = Vec::new();
            for repository in repositories_response.repositories() {
                let repository_name = ecr_repository_required_string(
                    repository,
                    "repository name",
                    repository.repository_name(),
                )?;
                if repository_name.starts_with(&repository_prefix) {
                    repositories.push(repository.clone());
                }
            }

            emit_aws_artifact_registry_heartbeat(
                ctx,
                &config.id,
                stored_account_id,
                stored_region,
                &repository_prefix,
                self.pull_role_arn.clone(),
                self.push_role_arn.clone(),
                repositories_response.next_token().is_some(),
                repositories,
            )?;
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

impl AwsArtifactRegistryController {
    async fn cleanup_role_policies(
        &self,
        iam_client: &IamClient,
        role_name: &str,
        role_label: &str,
        resource_id: &str,
    ) -> Result<()> {
        match iam_result(
            iam_client
                .list_attached_role_policies()
                .role_name(role_name)
                .send()
                .await,
            "ListAttachedRolePolicies",
            "IAM Role",
            role_name,
        ) {
            Ok(response) => {
                for policy in response.attached_policies() {
                    let policy_arn = policy.policy_arn().ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Attached policy for '{}' did not include an ARN",
                                role_name
                            ),
                            resource_id: Some(resource_id.to_string()),
                        })
                    })?;
                    let policy_arn = policy_arn.to_string();
                    let resource_name = format!("{role_name}/{policy_arn}");
                    match iam_result(
                        iam_client
                            .detach_role_policy()
                            .role_name(role_name)
                            .policy_arn(&policy_arn)
                            .send()
                            .await,
                        "DetachRolePolicy",
                        "IAM RolePolicyAttachment",
                        &resource_name,
                    ) {
                        Ok(_) => {
                            info!(
                                role_name = %role_name,
                                policy_arn = %policy_arn,
                                role_label = %role_label,
                                "Artifact registry role managed policy detached successfully"
                            );
                        }
                        Err(e)
                            if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) =>
                        {
                            info!(
                                role_name = %role_name,
                                policy_arn = %policy_arn,
                                role_label = %role_label,
                                "Artifact registry role managed policy already detached"
                            );
                        }
                        Err(e) => {
                            return Err(e.into_alien_error().context(
                                ErrorData::CloudPlatformError {
                                    message: format!(
                                        "Failed to detach {} role policy '{}' from '{}'",
                                        role_label, policy_arn, role_name
                                    ),
                                    resource_id: Some(resource_id.to_string()),
                                },
                            ));
                        }
                    }
                }
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

        match iam_result(
            iam_client
                .list_role_policies()
                .role_name(role_name)
                .send()
                .await,
            "ListRolePolicies",
            "IAM Role",
            role_name,
        ) {
            Ok(response) => {
                for policy_name in response.policy_names() {
                    let resource_name = format!("{role_name}/{policy_name}");
                    match iam_result(
                        iam_client
                            .delete_role_policy()
                            .role_name(role_name)
                            .policy_name(policy_name)
                            .send()
                            .await,
                        "DeleteRolePolicy",
                        "IAM RolePolicy",
                        &resource_name,
                    ) {
                        Ok(_) => {
                            info!(
                                role_name = %role_name,
                                policy_name = %policy_name,
                                role_label = %role_label,
                                "Artifact registry role inline policy deleted successfully"
                            );
                        }
                        Err(e)
                            if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) =>
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

    /// Configure ECR private image replication to the specified destination regions.
    ///
    /// ECR replication is configured at the registry level (per account). This method
    /// reads the current replication configuration, merges the desired destination
    /// regions, and writes back the updated configuration.
    async fn apply_replication_config(
        &self,
        ecr_client: &EcrClient,
        account_id: &str,
        home_region: &str,
        replication_regions: &[String],
        registry_id: &str,
    ) -> Result<()> {
        // Build the set of desired destinations (same account, different regions)
        let desired_destinations: Vec<ReplicationDestination> = replication_regions
            .iter()
            .filter(|r| r.as_str() != home_region)
            .map(|region| {
                ReplicationDestination::builder()
                    .region(region)
                    .registry_id(account_id)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build ECR replication destination".to_string(),
                        resource_id: Some(registry_id.to_string()),
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        if desired_destinations.is_empty() {
            info!(
                registry = %registry_id,
                "All replication regions match the home region, skipping"
            );
            return Ok(());
        }

        // Read the current replication configuration so we don't clobber existing rules
        let current_response = ecr_client
            .describe_registry()
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe ECR registry for replication config".to_string(),
                resource_id: Some(registry_id.to_string()),
            })?;
        let current = match current_response.replication_configuration {
            Some(replication_configuration) => replication_configuration,
            None => ReplicationConfiguration::builder()
                .set_rules(Some(vec![]))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build empty ECR replication configuration".to_string(),
                    resource_id: Some(registry_id.to_string()),
                })?,
        };

        // Merge: find or create a rule whose destinations include ours
        let mut rules = current.rules;
        if rules.is_empty() {
            rules.push(
                ReplicationRule::builder()
                    .set_destinations(Some(desired_destinations.clone()))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build ECR replication rule".to_string(),
                        resource_id: Some(registry_id.to_string()),
                    })?,
            );
        } else {
            // Merge into the first rule's destinations
            let first_rule = &mut rules[0];
            for dest in &desired_destinations {
                if !first_rule.destinations.contains(dest) {
                    first_rule.destinations.push(dest.clone());
                }
            }
        }

        let replication_configuration = ReplicationConfiguration::builder()
            .set_rules(Some(rules))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build ECR replication configuration".to_string(),
                resource_id: Some(registry_id.to_string()),
            })?;

        let response = ecr_client
            .put_replication_configuration()
            .replication_configuration(replication_configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to configure ECR replication".to_string(),
                resource_id: Some(registry_id.to_string()),
            })?;
        response.replication_configuration().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message:
                    "PutReplicationConfiguration response did not include replication configuration"
                        .to_string(),
                resource_id: Some(registry_id.to_string()),
            })
        })?;

        info!(
            registry = %registry_id,
            destinations = ?desired_destinations,
            "ECR replication configured successfully"
        );

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
) -> Result<()> {
    let repository_data = repositories
        .iter()
        .map(ecr_repository_heartbeat_data)
        .collect::<Result<Vec<_>>>()?;
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

    Ok(())
}

fn ecr_repository_heartbeat_data(repository: &Repository) -> Result<AwsEcrRepositoryHeartbeatData> {
    let repository_name = ecr_repository_required_string(
        repository,
        "repository name",
        repository.repository_name(),
    )?;

    Ok(AwsEcrRepositoryHeartbeatData {
        repository_arn: ecr_repository_required_string(
            repository,
            "repository ARN",
            repository.repository_arn(),
        )?,
        registry_id: ecr_repository_required_string(
            repository,
            "registry ID",
            repository.registry_id(),
        )?,
        repository_name: repository_name.clone(),
        repository_uri: ecr_repository_required_string(
            repository,
            "repository URI",
            repository.repository_uri(),
        )?,
        created_at: repository
            .created_at()
            .map(|created_at| created_at.secs() as f64)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "ECR DescribeRepositories response for '{repository_name}' did not include creation time"
                    ),
                    resource_id: Some(repository_name.clone()),
                })
            })?,
        image_tag_mutability: repository
            .image_tag_mutability()
            .map(|mutability| mutability.as_str().to_string()),
        scan_on_push: repository
            .image_scanning_configuration()
            .map(|config| config.scan_on_push()),
        encryption_type: repository
            .encryption_configuration()
            .map(|config| config.encryption_type().as_str().to_string()),
        kms_key_present: repository
            .encryption_configuration()
            .and_then(|config| config.kms_key())
            .is_some(),
    })
}

fn ecr_repository_required_string(
    repository: &Repository,
    field_name: &str,
    value: Option<&str>,
) -> Result<String> {
    let resource_id = repository.repository_name().map(ToString::to_string);
    value.map(ToString::to_string).ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("ECR DescribeRepositories response did not include {field_name}"),
            resource_id,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use aws_sdk_ecr::{
        operation::describe_repositories::DescribeRepositoriesOutput, primitives::DateTime,
        types::ImageTagMutability, Client as EcrClient,
    };
    use aws_sdk_iam::{
        operation::{
            create_role::CreateRoleOutput, delete_role::DeleteRoleOutput,
            delete_role_policy::DeleteRolePolicyOutput,
            list_attached_role_policies::ListAttachedRolePoliciesOutput,
            list_role_policies::ListRolePoliciesOutput, put_role_policy::PutRolePolicyOutput,
            update_assume_role_policy::UpdateAssumeRolePolicyOutput,
        },
        types::Role,
        Client as IamClient,
    };
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use aws_smithy_mocks::{mock, mock_client, MockResponse, RuleMode};
    use std::sync::Arc;

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn create_successful_role_response(role_name: &str) -> CreateRoleOutput {
        CreateRoleOutput::builder()
            .role(
                Role::builder()
                    .path("/")
                    .role_name(role_name)
                    .role_id("AROAEXAMPLE123")
                    .arn(format!("arn:aws:iam::123456789012:role/{}", role_name))
                    .create_date(aws_sdk_iam::primitives::DateTime::from_secs(0))
                    .build()
                    .expect("test role should build"),
            )
            .build()
    }

    fn setup_mock_client_for_creation_and_deletion() -> IamClient {
        let create_role_rule = mock!(IamClient::create_role)
            .match_requests(|request| request.role_name().is_some())
            .then_compute_response(|request| {
                MockResponse::Output(create_successful_role_response(
                    request
                        .role_name()
                        .expect("create role request should include role name"),
                ))
            });
        let put_role_policy_rule = mock!(IamClient::put_role_policy)
            .match_requests(|request| {
                request.role_name().is_some() && request.policy_name().is_some()
            })
            .then_output(|| PutRolePolicyOutput::builder().build());
        let list_attached_rule = mock!(IamClient::list_attached_role_policies)
            .match_requests(|request| {
                let role_name = request
                    .role_name()
                    .expect("list attached policies request should include role name");
                assert!(
                    matches!(role_name, "test-my-registry-pull" | "test-my-registry-push"),
                    "expected artifact registry role name, got {role_name}"
                );
                true
            })
            .then_output(empty_attached_role_policies_response);
        let list_role_policies_rule = mock!(IamClient::list_role_policies)
            .match_requests(|request| request.role_name().is_some())
            .then_compute_response(|request| {
                let role_name = request
                    .role_name()
                    .expect("list role policies request should include role name");
                let policy_name = if role_name.ends_with("-pull") {
                    "ECRPullPolicy"
                } else {
                    "ECRPushPolicy"
                };
                MockResponse::Output(
                    ListRolePoliciesOutput::builder()
                        .policy_names(policy_name)
                        .is_truncated(false)
                        .build()
                        .expect("test list role policies response should build"),
                )
            });
        let delete_role_policy_rule = mock!(IamClient::delete_role_policy)
            .match_requests(|request| {
                request.role_name().is_some() && request.policy_name().is_some()
            })
            .then_output(|| DeleteRolePolicyOutput::builder().build());
        let delete_role_rule = mock!(IamClient::delete_role)
            .match_requests(|request| request.role_name().is_some())
            .then_output(|| DeleteRoleOutput::builder().build());

        mock_client!(
            aws_sdk_iam,
            RuleMode::MatchAny,
            [
                &create_role_rule,
                &put_role_policy_rule,
                &list_attached_rule,
                &list_role_policies_rule,
                &delete_role_policy_rule,
                &delete_role_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_client_for_creation_and_update() -> IamClient {
        let create_role_rule = mock!(IamClient::create_role)
            .match_requests(|request| request.role_name().is_some())
            .then_compute_response(|request| {
                MockResponse::Output(create_successful_role_response(
                    request
                        .role_name()
                        .expect("create role request should include role name"),
                ))
            });
        let put_role_policy_rule = mock!(IamClient::put_role_policy)
            .match_requests(|request| {
                request.role_name().is_some() && request.policy_name().is_some()
            })
            .then_output(|| PutRolePolicyOutput::builder().build());
        let update_assume_role_policy_rule = mock!(IamClient::update_assume_role_policy)
            .match_requests(|request| {
                request.role_name().is_some() && request.policy_document().is_some()
            })
            .then_output(|| UpdateAssumeRolePolicyOutput::builder().build());

        mock_client!(
            aws_sdk_iam,
            RuleMode::MatchAny,
            [
                &create_role_rule,
                &put_role_policy_rule,
                &update_assume_role_policy_rule
            ],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_service_provider(mock_iam: IamClient) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        Arc::new(mock_provider)
    }

    fn setup_mock_service_provider_with_ecr(
        ecr_client: EcrClient,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_ecr_client()
            .returning(move |_| Ok(ecr_client.clone()));

        Arc::new(mock_provider)
    }

    fn empty_attached_role_policies_response() -> ListAttachedRolePoliciesOutput {
        ListAttachedRolePoliciesOutput::builder()
            .is_truncated(false)
            .build()
    }

    fn empty_inline_role_policies_response() -> ListRolePoliciesOutput {
        ListRolePoliciesOutput::builder()
            .set_policy_names(Some(vec![]))
            .is_truncated(false)
            .build()
            .expect("test list role policies response should build")
    }

    fn ecr_repository(repository_name: &str) -> Repository {
        Repository::builder()
            .repository_arn(format!(
                "arn:aws:ecr:us-east-1:123456789012:repository/{}",
                repository_name
            ))
            .registry_id("123456789012")
            .repository_name(repository_name)
            .repository_uri(format!(
                "123456789012.dkr.ecr.us-east-1.amazonaws.com/{}",
                repository_name
            ))
            .created_at(DateTime::from_secs(0))
            .image_tag_mutability(ImageTagMutability::Mutable)
            .build()
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

        let list_attached_rule = mock!(IamClient::list_attached_role_policies)
            .match_requests(|request| {
                let role_name = request
                    .role_name()
                    .expect("list attached policies request should include role name");
                assert!(
                    matches!(role_name, "short-registry-pull" | "short-registry-push"),
                    "expected ARN-derived role name, got {role_name}"
                );
                true
            })
            .then_output(empty_attached_role_policies_response);
        let list_inline_rule = mock!(IamClient::list_role_policies)
            .match_requests(|request| {
                let role_name = request
                    .role_name()
                    .expect("list inline policies request should include role name");
                assert!(
                    matches!(role_name, "short-registry-pull" | "short-registry-push"),
                    "expected ARN-derived role name, got {role_name}"
                );
                true
            })
            .then_output(empty_inline_role_policies_response);
        let delete_role_rule = mock!(IamClient::delete_role)
            .match_requests(|request| {
                let role_name = request
                    .role_name()
                    .expect("delete role request should include role name");
                assert!(
                    matches!(role_name, "short-registry-pull" | "short-registry-push"),
                    "expected ARN-derived role name, got {role_name}"
                );
                true
            })
            .then_output(|| DeleteRoleOutput::builder().build());
        let mock_iam = mock_client!(
            aws_sdk_iam,
            RuleMode::MatchAny,
            [&list_attached_rule, &list_inline_rule, &delete_role_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );

        let mock_provider = setup_mock_service_provider(mock_iam);
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

    #[tokio::test]
    async fn ready_uses_sdk_native_ecr_mock_for_repository_heartbeat() {
        let registry = basic_artifact_registry();
        let account_id = "123456789012";
        let region = "us-east-1";
        let repository_prefix = "test-artifact-registry";
        let expected_registry_id = account_id.to_string();
        let describe_rule = mock!(EcrClient::describe_repositories)
            .match_requests(move |request| {
                request.registry_id() == Some(expected_registry_id.as_str())
                    && request.max_results() == Some(100)
            })
            .then_output(move || {
                DescribeRepositoriesOutput::builder()
                    .repositories(ecr_repository(&format!("{repository_prefix}/app")))
                    .build()
            });
        let ecr_client = mock_client!(
            aws_sdk_ecr,
            RuleMode::Sequential,
            [&describe_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider_with_ecr(ecr_client);
        let controller = AwsArtifactRegistryController::mock_ready(account_id, region);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(describe_rule.num_calls(), 1);
    }
}
