use std::collections::HashSet;
use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::{
    CreateRoleRequest, CreateRoleTag, IamRoleWriter, TrustPolicyDocument, TrustPolicyPrincipal,
    TrustPolicyPrincipalValue, TrustPolicyStatement,
};
use alien_core::{
    standard_resource_tags, AwsIamRoleServiceAccountHeartbeatData, Build, ComputeCluster,
    Container, HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, ServiceAccount,
    ServiceAccountHeartbeatData, ServiceAccountHeartbeatStatus, ServiceAccountOutputs, Worker,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::{
    aws_resource_policies::{
        aws_resource_policy_name, aws_service_account_injected_statements, legacy_policy_name,
        names_aws_resource_grant,
    },
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use chrono::Utc;

/// Generates the AWS IAM role name for a ServiceAccount.
fn get_aws_role_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

// Define the inline policy name we will manage
const MANAGED_POLICY_NAME: &str = "deployment-permissions";

#[controller]
pub struct AwsServiceAccountController {
    /// The ARN of the created IAM role.
    pub role_arn: Option<String>,
    /// The name of the created IAM role.
    pub(crate) role_name: Option<String>,
    /// Set only after this controller creates a role during automatic setup.
    #[serde(default)]
    pub(crate) runtime_created: bool,
    /// Whether stack-level permissions have been applied
    pub(crate) stack_permissions_applied: bool,
    /// Last trust policy installed by privileged setup, with its `sts:ExternalId` as the
    /// value's SHA-256 (see [`redacted_trust`]); absent in older checkpoints.
    #[serde(default)]
    pub(crate) assume_role_policy: Option<String>,
}

#[controller]
impl AwsServiceAccountController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        // Validate the complete stack policy, including injected grants, before
        // creating a role that could otherwise be left behind by a size failure.
        self.generate_stack_policy_document(config, ctx)?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        let role_name = match &config.extension {
            Some(_) => {
                alien_core::extension_identity_aws_role_name(ctx.resource_prefix, &config.id)
            }
            None => get_aws_role_name(ctx.resource_prefix, &config.id),
        };
        let assume_role_policy =
            Self::generate_assume_role_policy_for_service_account(config, ctx, None)?;

        info!(
            role_name = %role_name,
            service_account_id = %config.id,
            "Creating IAM role for ServiceAccount"
        );

        let role_request = CreateRoleRequest::builder()
            .role_name(role_name.clone())
            .assume_role_policy_document(assume_role_policy.clone())
            .description(
                match (&config.extension, ctx.deployment_name_for_metadata()) {
                    (Some(extension), _) => format!(
                        "{}. Resource prefix: {}. Resource: {}.",
                        alien_core::extension_identity_display_name(extension),
                        ctx.resource_prefix,
                        config.id
                    ),
                    (None, Some(deployment_name)) => format!(
                        "Runtime IAM role for {deployment_name}. Resource prefix: {}. Resource: {}.",
                        ctx.resource_prefix, config.id
                    ),
                    (None, None) => format!(
                        "Runtime IAM role. Resource prefix: {}. Resource: {}.",
                        ctx.resource_prefix, config.id
                    ),
                },
            )
            .tags(
                standard_resource_tags(ctx.resource_prefix, &config.id)
                    .into_iter()
                    .map(|(key, value)| CreateRoleTag { key, value })
                    .collect(),
            )
            .build();

        let created_role = IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .create(role_request)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create IAM role '{}'", role_name),
            resource_id: Some(config.id.clone()),
        })?;

        let role_arn = created_role.create_role_result.role.arn;

        info!(
            role_name = %role_name,
            role_arn = %role_arn,
            "IAM role created successfully"
        );

        self.runtime_created = ctx.deployment_config.stack_settings.setup_management
            == alien_core::SetupManagement::Automatic;
        self.assume_role_policy = Some(redacted_trust(&assume_role_policy)?);
        self.role_name = Some(role_name);
        self.role_arn = Some(role_arn);

        Ok(HandlerAction::Continue {
            state: ApplyingStackPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingStackPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_stack_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_stack_permissions(ctx).await?;
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
        Self::apply_resource_permissions(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        // Heartbeat check: verify role still exists
        let role = client
            .get_role(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get IAM role during heartbeat check".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Check if role ARN matches what we expect
        if let Some(expected_arn) = &self.role_arn {
            if role.get_role_result.role.arn != *expected_arn {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "Role ARN changed from {} to {}",
                        expected_arn, role.get_role_result.role.arn
                    ),
                }));
            }
        }

        let attached_policies = client
            .list_attached_role_policies(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list attached IAM role policies during heartbeat check"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?
            .list_attached_role_policies_result
            .attached_policies
            .map(|policies| policies.member)
            .unwrap_or_default();

        let inline_policy_names = client
            .list_role_policies(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list inline IAM role policies during heartbeat check"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?
            .list_role_policies_result
            .policy_names
            .map(|names| names.member)
            .unwrap_or_default();

        emit_aws_service_account_heartbeat(
            ctx,
            &config.id,
            &role.get_role_result.role,
            attached_policies,
            inline_policy_names,
            self.stack_permissions_applied,
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let previous = ctx.previous_resource_config::<ServiceAccount>()?;
        // An unchanged config means only the stack's ordering edges changed, such as
        // the edge to management added for the AWS permissions boundary. The executor
        // records the new edges when the update starts, and the role needs nothing.
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
            && previous == config
        {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        // Frozen compatibility has already verified the installed explicit grants.
        // Capturing their metadata changes no AWS role policy, and runtime does
        // not need IAM write access to record it.
        if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::ImportedHandoff
            && previous.resource_permission_sets.is_empty()
            && !config.resource_permission_sets.is_empty()
            && previous.id == config.id
            && previous.stack_permission_sets == config.stack_permission_sets
        {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
            return Err(AlienError::new(ErrorData::ImportedSetupStateInvalid {
                message: "service-account trust changes require privileged setup; regenerate and rerun setup".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
        self.generate_stack_policy_document(config, ctx)?;
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        let role_name = self.role_name.as_ref().unwrap();
        // State keeps only the ExternalId's hash, so a run without the ExternalId reads the
        // value the role's trust checks from the role itself. When state recorded one, the role
        // must still check it; when state recorded none, as for an imported role, the role's own
        // ExternalId is kept. Either way the update never writes a trust without the ExternalId
        // the role checks now.
        let live_trust = match (
            &config.extension,
            &ctx.deployment_config.management_external_id,
        ) {
            (Some(_), None) => {
                let recorded = self
                    .assume_role_policy
                    .as_deref()
                    .map(redacted_trust)
                    .transpose()?
                    .and_then(|trust| trusted_external_id(&trust));
                let kept = |message: String| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message,
                        operation: Some("update the extension identity's trust".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                };
                let live = client
                    .get_role(role_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to read the trust policy of IAM role '{role_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?
                    .get_role_result
                    .role
                    .assume_role_policy_document
                    .and_then(|encoded| urlencoding::decode(&encoded).ok().map(String::from))
                    .ok_or_else(|| {
                        kept(format!(
                            "IAM role '{role_name}' returned no trust policy to keep its ExternalId from; run setup with the deployment's ExternalId"
                        ))
                    })?;
                // Parsed on every path: a trust that doesn't parse names no ExternalId to keep.
                let live_external_id = trusted_external_id(&redacted_trust(&live)?);
                if let Some(recorded) = recorded {
                    if live_external_id.as_deref() != Some(recorded.as_str()) {
                        return Err(kept(format!(
                            "IAM role '{role_name}' no longer trusts the ExternalId this deployment set up, and this run has none to write; run setup with the deployment's ExternalId"
                        )));
                    }
                }
                Some(live)
            }
            _ => None,
        };
        let policy = Self::generate_assume_role_policy_for_service_account(
            config,
            ctx,
            live_trust.as_deref(),
        )?;
        IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .prepare_existing(role_name, ctx.resource_prefix, &config.id)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to prepare service account IAM role '{role_name}'"),
            resource_id: Some(config.id.clone()),
        })?;
        client
            .update_assume_role_policy(role_name, &policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update trust policy for IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        self.assume_role_policy = Some(redacted_trust(&policy)?);
        Ok(HandlerAction::Continue {
            state: UpdatingStackPermissions,
            suggested_delay: None,
        })
    }

    // Update-specific permission phases: a failure here is an UpdateFailed of a
    // live role, never a failed create that could be replaced.
    #[handler(
        state = UpdatingStackPermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_stack_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_stack_permissions(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdatingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Self::apply_resource_permissions(ctx).await?;
        self.remove_dropped_resource_grants(ctx).await?;
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
        let role_name = match &self.role_name {
            Some(name) => name,
            None => {
                // No role was created, skip to deleted
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        if !self.can_delete_role(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        match IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .prepare_for_removal(role_name, ctx.resource_prefix, &config.id)
        .await
        {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.role_name = None;
                self.role_arn = None;
                self.stack_permissions_applied = false;
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to prepare service account IAM role '{role_name}' for deletion"
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
        }

        info!(
            role_name = %role_name,
            "Starting comprehensive policy cleanup before role deletion"
        );

        let mut more_policies = false;
        // Step 1: List and detach all managed policies
        match client.list_attached_role_policies(role_name).await {
            Ok(response) => {
                more_policies |=
                    response.list_attached_role_policies_result.is_truncated == Some(true);
                if let Some(attached_policies) = response
                    .list_attached_role_policies_result
                    .attached_policies
                {
                    for policy in &attached_policies.member {
                        info!(
                            role_name = %role_name,
                            policy_arn = %policy.policy_arn,
                            policy_name = %policy.policy_name,
                            "Detaching managed policy"
                        );

                        match client
                            .detach_role_policy(role_name, &policy.policy_arn)
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_arn = %policy.policy_arn,
                                    "Managed policy detached successfully"
                                );
                            }
                            Err(e) => {
                                // Check if it's a resource not found error
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(
                                        role_name = %role_name,
                                        policy_arn = %policy.policy_arn,
                                        "Managed policy already detached or doesn't exist"
                                    );
                                } else {
                                    return Err(e
                                        .context(ErrorData::CloudPlatformError {
                                            message: format!(
                                                "Failed to detach managed policy '{}'",
                                                policy.policy_arn
                                            ),
                                            resource_id: Some(
                                                ctx.desired_resource_config::<ServiceAccount>()?
                                                    .id
                                                    .clone(),
                                            ),
                                        })
                                        .into());
                                }
                            }
                        }
                    }
                } else {
                    info!(role_name = %role_name, "No managed policies attached to role");
                }
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Role not found during managed policy cleanup");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list attached managed policies".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        // Step 2: List and delete all inline policies
        match client.list_role_policies(role_name).await {
            Ok(response) => {
                more_policies |= response.list_role_policies_result.is_truncated == Some(true);
                if let Some(policy_names) = response.list_role_policies_result.policy_names {
                    for policy_name in &policy_names.member {
                        info!(
                            role_name = %role_name,
                            policy_name = %policy_name,
                            "Deleting inline policy"
                        );

                        match client.delete_role_policy(role_name, policy_name).await {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_name = %policy_name,
                                    "Inline policy deleted successfully"
                                );
                            }
                            Err(e) => {
                                // Check if it's a resource not found error
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(
                                        role_name = %role_name,
                                        policy_name = %policy_name,
                                        "Inline policy already deleted or doesn't exist"
                                    );
                                } else {
                                    return Err(e
                                        .context(ErrorData::CloudPlatformError {
                                            message: format!(
                                                "Failed to delete inline policy '{}'",
                                                policy_name
                                            ),
                                            resource_id: Some(
                                                ctx.desired_resource_config::<ServiceAccount>()?
                                                    .id
                                                    .clone(),
                                            ),
                                        })
                                        .into());
                                }
                            }
                        }
                    }
                } else {
                    info!(role_name = %role_name, "No inline policies attached to role");
                }
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Role not found during inline policy cleanup");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list inline policies".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        if more_policies {
            // Deleting this page makes the remaining policies available on the next
            // first-page read. Keep the cleanup checkpoint until both lists finish.
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        info!(role_name = %role_name, "Policy cleanup completed, proceeding to role deletion");

        Ok(HandlerAction::Continue {
            state: DeletingRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if !self.can_delete_role(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }
        let role_name = self.role_name.as_ref().unwrap();
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        info!(
            role_name = %role_name,
            "Deleting IAM role"
        );

        match client.delete_role(role_name).await {
            Ok(_) => {
                info!(
                    role_name = %role_name,
                    "IAM role deleted successfully"
                );
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "IAM role already deleted");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to delete IAM role".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        self.role_name = None;
        self.role_arn = None;
        self.stack_permissions_applied = false;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
            return Ok(false);
        }
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        // Compared with the ExternalId as its hash on both sides: a run without the ExternalId
        // takes the stored hash in its place.
        let wanted = Self::generate_assume_role_policy_for_service_account(
            config,
            ctx,
            self.assume_role_policy.as_deref(),
        )?;
        Ok(self
            .assume_role_policy
            .as_deref()
            .map(redacted_trust)
            .transpose()?
            != Some(redacted_trust(&wanted)?))
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(role_arn), Some(role_name)) = (&self.role_arn, &self.role_name) {
            Some(ResourceOutputs::new(ServiceAccountOutputs {
                identity: role_arn.clone(),
                resource_id: role_name.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        if let (Some(role_arn), Some(role_name)) = (&self.role_arn, &self.role_name) {
            let binding = ServiceAccountBinding::aws_iam(
                BindingValue::Value(role_name.clone()),
                BindingValue::Value(role_arn.clone()),
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

// Separate impl block for helper methods
impl AwsServiceAccountController {
    async fn can_delete_role(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if ctx.deployment_config.stack_settings.setup_management
            != alien_core::SetupManagement::Automatic
            || ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            || self.runtime_created
        {
            return Ok(true);
        }

        let Some(role_name) = &self.role_name else {
            return Ok(false);
        };
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        let role = match client.get_role(role_name).await {
            Ok(response) => response.get_role_result.role,
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(false);
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to verify IAM role ownership before deletion".to_string(),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }))
            }
        };
        // Older checkpoints have no creation marker. Require all creation tags from
        // the live role; a matching name alone does not establish ownership.
        let expected = standard_resource_tags(ctx.resource_prefix, ctx.desired_config.id());
        let owned = role.tags.as_ref().is_some_and(|tags| {
            expected.iter().all(|(key, value)| {
                tags.member
                    .iter()
                    .any(|tag| &tag.key == key && &tag.value == value)
            })
        });
        if !owned {
            info!(
                role_name,
                "Leaving IAM role without runtime ownership proof for setup teardown"
            );
        }
        Ok(owned)
    }

    /// Write the stack-level inline policy, or remove it when an update leaves
    /// no stack permissions. Requires privileged setup.
    async fn apply_stack_permissions(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
            return Err(AlienError::new(ErrorData::ImportedSetupStateInvalid {
                message: "service-account policy changes require privileged setup; regenerate and rerun setup".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
        let policy_document = self.generate_stack_policy_document(config, ctx)?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            stack_permission_sets_count = config.stack_permission_sets.len(),
            "Applying stack-level permission sets to IAM role"
        );

        if !policy_document.is_empty()
            || ctx
                .state
                .resources
                .get(&config.id)
                .is_some_and(|state| state.previous_config.is_some())
        {
            IamRoleWriter::new(
                client.as_ref(),
                ctx.aws_permissions_boundary_arn()?.as_deref(),
            )
            .prepare_existing(role_name, ctx.resource_prefix, &config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to prepare service account IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        }

        if !policy_document.is_empty() {
            client
                .put_role_policy(role_name, MANAGED_POLICY_NAME, &policy_document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply stack permissions to IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                "Stack-level permissions applied successfully"
            );
        } else if ctx
            .state
            .resources
            .get(&config.id)
            .is_some_and(|state| state.previous_config.is_some())
        {
            match client
                .delete_role_policy(role_name, MANAGED_POLICY_NAME)
                .await
            {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to remove empty stack permissions from IAM role '{role_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        } else {
            info!(
                role_name = %role_name,
                "No stack-level permissions to apply"
            );
        }

        self.stack_permissions_applied = true;
        Ok(())
    }

    /// Delete the inline policies that grant this role access to a resource it no longer has
    /// grants on, typically because the resource or its profile grant was removed. Each resource
    /// writes `deployment-<resource>-<set>` onto the role when created and never removes it, so
    /// without this the role keeps access to a removed resource, and to anything later created
    /// under the same name. Requires privileged setup, like every role change.
    ///
    /// The role is the source of truth for what it holds: an account installed before grants
    /// were captured records none, so the previous config cannot say what to remove.
    async fn remove_dropped_resource_grants(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let previous = ctx.previous_resource_config::<ServiceAccount>()?;
        let desired_profile = config
            .id
            .strip_suffix("-sa")
            .and_then(|name| ctx.desired_stack.permissions.profiles.get(name));
        let desired = config
            .concrete_permission_sets(desired_profile, |name| {
                alien_permissions::get_stack_permission_set(ctx.desired_stack, name).cloned()
            })
            .context(ErrorData::ResourceConfigInvalid {
                message: "Cannot resolve the service account's resource grants".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        let with_legacy = |name: String| [legacy_policy_name(&name), Some(name)];
        let kept: HashSet<String> = desired
            .iter()
            .flat_map(|(resource_id, sets)| {
                sets.iter()
                    .map(move |set| aws_resource_policy_name(resource_id, &set.id))
            })
            .flat_map(with_legacy)
            .flatten()
            .collect();
        let set_ids: HashSet<&str> = alien_permissions::list_permission_set_ids()
            .into_iter()
            .chain(
                previous
                    .resource_permission_sets
                    .values()
                    .chain(desired.values())
                    .flatten()
                    .map(|set| set.id.as_str()),
            )
            .collect();

        let role_name = self.role_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "IAM role name not set in state".to_string(),
            })
        })?;
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        let held = client
            .list_role_policies(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to list inline policies of IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            })?
            .list_role_policies_result;
        if held.is_truncated == Some(true) {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM role '{role_name}' has more inline policies than one ListRolePolicies page; cannot tell which resource grants it still holds"
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
        let mut dropped = held
            .policy_names
            .map(|names| names.member)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| {
                !kept.contains(name) && names_aws_resource_grant(name, set_ids.iter().copied())
            })
            .peekable();
        if dropped.peek().is_none() {
            return Ok(());
        }
        IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .prepare_existing(role_name, ctx.resource_prefix, &config.id)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to prepare service account IAM role '{role_name}'"),
            resource_id: Some(config.id.clone()),
        })?;
        for policy_name in dropped {
            match client.delete_role_policy(role_name, &policy_name).await {
                Ok(()) => info!(
                    role_name = %role_name,
                    policy_name = %policy_name,
                    "Removed a resource grant the service account no longer has"
                ),
                // Removed concurrently, or by an earlier attempt.
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete inline policy '{policy_name}' from IAM role '{role_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        Ok(())
    }

    /// Attach the resource-scoped permissions this service account grants.
    async fn apply_resource_permissions(ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            service_account_id = %config.id,
            "Applying resource-scoped permissions for service account"
        );

        // Apply resource-scoped permissions using the centralized helper.
        // This attaches management SA permissions (e.g., service-account/heartbeat)
        // as inline policies on the management role.
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "service-account",
            )
            .await?;
        }

        info!(
            service_account_id = %config.id,
            "Successfully applied resource-scoped permissions for service account"
        );
        Ok(())
    }

    /// Generate assume role policy for the service account based on stack analysis.
    ///
    /// This function determines which AWS services and IAM roles should be allowed to assume
    /// this service account's IAM role by analyzing:
    /// 1. Workers/Builds that use a permission profile matching this ServiceAccount
    /// 2. Other ServiceAccounts that have impersonation permissions for this ServiceAccount
    ///
    /// **Naming Convention Assumption**: ServiceAccounts created from permission profiles follow
    /// the pattern `{profile-name}-sa`. For example, the "execution" profile creates
    /// an "execution-sa" ServiceAccount resource. This function attempts to find the
    /// corresponding profile by stripping the "-sa" suffix. If the ServiceAccount
    /// was created directly (not from a profile), or uses a non-standard name, it falls back to
    /// using the full ServiceAccount ID as the profile name.
    ///
    /// `applied` is the trust the role has now, if any. For the extension identity, a run without
    /// the ExternalId keeps the one `applied` checks; when `applied` is the stored trust, that is
    /// the value's hash, and the policy is fit only to compare with another redacted one.
    fn generate_assume_role_policy_for_service_account(
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
        applied: Option<&str>,
    ) -> Result<String> {
        if let Some(extension) = &service_account.extension {
            let management = ctx.get_aws_management_config()?.ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: format!(
                        "AWS management configuration is required for the identity of the extension '{extension}'"
                    ),
                    operation: Some("generate_assume_role_policy".to_string()),
                    resource_id: Some(service_account.id.clone()),
                })
            })?;
            // The setup run passes the ExternalId the control plane issued; a later run that has
            // none keeps the one the trust already checks, so an update never drops it.
            let external_id = ctx
                .deployment_config
                .management_external_id
                .as_ref()
                .map(|id| id.expose().to_string())
                .or_else(|| applied.and_then(trusted_external_id));
            return Ok(crate::remote_bindings::managing_role_trust_policy(
                &management.managing_role_arn,
                external_id.as_deref(),
            ));
        }
        let mut services = Vec::new();
        let mut role_arns = Vec::new();

        // Attempt to determine the corresponding permission profile name.
        // Convention: ServiceAccount "xyz-sa" corresponds to profile "xyz"
        // If no suffix is found, use the ServiceAccount ID as-is (handles direct ServiceAccount resources)
        let profile_name = service_account
            .id
            .strip_suffix("-sa")
            .unwrap_or(&service_account.id);

        // Analyze the stack in a single pass to determine:
        // 1. Which AWS services need to assume this role (Workers using this profile -> Lambda, Builds -> CodeBuild)
        // 2. Which other ServiceAccounts can impersonate this one
        for (_, resource_entry) in ctx.desired_stack.resources() {
            let resource = &resource_entry.config;

            // Check if there are Workers that use THIS service account's profile
            if let Some(function) = resource.downcast_ref::<Worker>() {
                if function.get_permissions() == profile_name {
                    if !services.contains(&"lambda.amazonaws.com".to_string()) {
                        services.push("lambda.amazonaws.com".to_string());
                    }
                }
            }

            // Check if there are Builds that use THIS service account's profile
            if let Some(build) = resource.downcast_ref::<Build>() {
                if build.get_permissions() == profile_name {
                    if !services.contains(&"codebuild.amazonaws.com".to_string()) {
                        services.push("codebuild.amazonaws.com".to_string());
                    }
                }
            }
        }

        // Check if any Container in the stack uses this profile — if so, the ComputeCluster VM
        // role needs to assume this SA role to vend per-container credentials via the IMDS proxy.
        // Keep both exact node identities trusted while existing nodes drain during
        // an isolation rollout. A node still needs its own sts:AssumeRole grant.
        let has_container_using_profile = ctx.desired_stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .map(|c| c.get_permissions() == profile_name)
                .unwrap_or(false)
        });

        if has_container_using_profile {
            let account_id = ctx
                .get_aws_config()
                .ok()
                .map(|c| c.account_id.clone())
                .unwrap_or_default();

            for (cluster_id, entry) in ctx.desired_stack.resources() {
                if entry.config.downcast_ref::<ComputeCluster>().is_some() {
                    for suffix in ["role", "isolation-v1-role"] {
                        let vm_role_arn = format!(
                            "arn:aws:iam::{}:role/{}-{}-{}",
                            account_id, ctx.resource_prefix, cluster_id, suffix,
                        );
                        if !role_arns.contains(&vm_role_arn) {
                            info!(
                                service_account = %service_account.id,
                                cluster_id = %cluster_id,
                                vm_role_arn = %vm_role_arn,
                                "Adding ComputeCluster VM role to SA trust policy for IMDS credential vending"
                            );
                            role_arns.push(vm_role_arn);
                        }
                    }
                }
            }
        }

        // Find ServiceAccounts that can impersonate this service account by checking permission profiles.
        //
        // Impersonation logic: A permission profile with resource-scoped "service-account/impersonate"
        // permission for this ServiceAccount allows the ServiceAccount created from that profile to
        // assume this ServiceAccount's role.
        //
        // Example: Profile "execution" with resource-scoped permission:
        //   "agent-management": ["service-account/impersonate"]
        // allows "execution-sa" to assume "agent-management-sa"'s role.
        //
        // Note: This only works for ServiceAccounts created from profiles (via the ServiceAccountMutation).
        // Manually-defined ServiceAccounts cannot be granted impersonation permissions this way.
        //
        // The permission can be scoped to either:
        // 1. The profile name (e.g., "agent-management") that creates the ServiceAccount
        // 2. The full ServiceAccount resource ID (e.g., "agent-management-sa")
        for (profile_name, permission_profile) in &ctx.desired_stack.permissions.profiles {
            // Determine the profile name for this service account (strip "-sa" suffix if present)
            let target_profile_name = service_account
                .id
                .strip_suffix("-sa")
                .unwrap_or(&service_account.id);

            // Check if this profile has impersonate permission scoped to either:
            // - The profile name (e.g., "agent-management")
            // - The full ServiceAccount ID (e.g., "agent-management-sa")
            let permission_set_refs = permission_profile
                .0
                .get(target_profile_name)
                .or_else(|| permission_profile.0.get(&service_account.id));

            if let Some(permission_set_refs) = permission_set_refs {
                let has_impersonate = permission_set_refs.iter().any(|perm_ref| match perm_ref {
                    alien_core::permissions::PermissionSetReference::Name(name) => {
                        name == "service-account/impersonate"
                    }
                    alien_core::permissions::PermissionSetReference::Inline(inline) => {
                        inline.id == "service-account/impersonate"
                    }
                });

                if has_impersonate {
                    // Find the ServiceAccount resource created from this profile
                    // Convention: profile "xyz" creates ServiceAccount "xyz-sa"
                    let impersonator_sa_id = format!("{}-sa", profile_name);

                    // Verify this ServiceAccount actually exists in the stack
                    let sa_exists = ctx.desired_stack.resources().any(|(resource_id, entry)| {
                        resource_id == &impersonator_sa_id
                            && entry.config.downcast_ref::<ServiceAccount>().is_some()
                    });

                    if sa_exists && impersonator_sa_id != service_account.id {
                        let impersonator_role_arn = format!(
                            "arn:aws:iam::{}:role/{}-{}",
                            ctx.get_aws_config()
                                .ok()
                                .map(|c| c.account_id.clone())
                                .unwrap_or_default(),
                            ctx.resource_prefix,
                            impersonator_sa_id
                        );

                        if !role_arns.contains(&impersonator_role_arn) {
                            info!(
                                service_account = %service_account.id,
                                impersonator_profile = %profile_name,
                                impersonator_sa = %impersonator_sa_id,
                                impersonator_role = %impersonator_role_arn,
                                "Adding impersonator service account to trust policy"
                            );
                            role_arns.push(impersonator_role_arn);
                        }
                    }
                }
            }
        }

        services.sort();
        role_arns.sort();

        // Build trust policy statements
        let mut statements = Vec::new();

        // Ensure we have at least one principal (service or role)
        if services.is_empty() && role_arns.is_empty() {
            warn!(
                service_account = %service_account.id,
                "No principals found for service account trust policy - defaulting to Lambda service"
            );
            services.push("lambda.amazonaws.com".to_string());
        }

        // Statement for AWS services (only if there are services that need to assume this role)
        if !services.is_empty() {
            let principal_value = if services.len() == 1 {
                TrustPolicyPrincipalValue::Single(services[0].clone())
            } else {
                TrustPolicyPrincipalValue::Multiple(services)
            };

            statements.push(TrustPolicyStatement {
                effect: "Allow".to_string(),
                principal: TrustPolicyPrincipal::Service {
                    service: principal_value,
                },
                action: "sts:AssumeRole".to_string(),
                condition: None,
            });
        }

        // IAM resolves role principals at policy creation time. These roles may still be
        // provisioning (or be recreated later), so constrain the account principal by
        // exact role ARNs instead. This also requires the caller's sts:AssumeRole grant;
        // it does not grant assumption to arbitrary identities in the account.
        if !role_arns.is_empty() {
            statements.push(role_trust_statement(
                &ctx.get_aws_config()?.account_id,
                role_arns,
            ));
        }

        // Create the complete trust policy document
        let trust_policy = TrustPolicyDocument {
            version: "2012-10-17".to_string(),
            statement: statements,
        };

        // Serialize to JSON
        serde_json::to_string(&trust_policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize trust policy document".to_string(),
                operation: Some("generate_assume_role_policy".to_string()),
                resource_id: Some(service_account.id.clone()),
            })
    }

    /// Generate IAM policy document for stack-level permission sets
    fn generate_stack_policy_document(
        &self,
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        if service_account.stack_permission_sets.is_empty() {
            return Ok(String::new());
        }

        let generator = AwsRuntimePermissionsGenerator::new();
        let mut all_statements = Vec::new();

        let aws_config = ctx.get_aws_config()?;

        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_aws_region(aws_config.region.clone())
            .with_aws_account_id(aws_config.account_id.clone());

        permission_context.aws_permissions_boundary_arn = ctx.aws_permissions_boundary_arn()?;

        // Add managing account ID from stack settings if available
        if let Some(aws_management) = ctx.get_aws_management_config()? {
            permission_context =
                permission_context.with_managing_role_arn(aws_management.managing_role_arn.clone());

            // Extract and set managing account ID from role ARN
            if let Some(managing_account_id) =
                alien_permissions::PermissionContext::extract_account_id_from_role_arn(
                    &aws_management.managing_role_arn,
                )
            {
                permission_context =
                    permission_context.with_managing_account_id(managing_account_id);
            }
        }

        for permission_set in &service_account.stack_permission_sets {
            // Generate stack-level policy for this permission set
            let policy = generator
                .generate_policy(permission_set, BindingTarget::Stack, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate IAM policy for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_stack_policy_document".to_string()),
                    resource_id: Some(service_account.id.clone()),
                })?;

            all_statements.extend(policy.statement);
        }

        all_statements.extend(aws_service_account_injected_statements(
            service_account,
            ctx.desired_stack,
            &permission_context,
        ));

        if all_statements.is_empty() {
            return Ok(String::new());
        }

        let final_policy = AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: all_statements,
        };

        let policy_document = serde_json::to_string(&final_policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize IAM policy document".to_string(),
                operation: Some("generate_stack_policy_document".to_string()),
                resource_id: Some(service_account.id.clone()),
            })?;

        // Compact JSON excludes formatting whitespace. IAM's limit is in
        // characters, not UTF-8 bytes. This document alone can prove overflow;
        // other inline policies on an existing role may consume more headroom.
        let size = policy_document
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .count();
        if size > 10_240 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "IAM role '{}' needs at least {size} inline policy characters; the aggregate per-role limit is 10240. Reduce grants or use managed policies. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html",
                    get_aws_role_name(ctx.resource_prefix, &service_account.id)
                ),
                resource_id: Some(service_account.id.clone()),
            }));
        }

        Ok(policy_document)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(role_name: &str) -> Self {
        Self {
            state: AwsServiceAccountState::Ready,
            role_arn: Some(format!("arn:aws:iam::123456789012:role/{}", role_name)),
            role_name: Some(role_name.to_string()),
            stack_permissions_applied: true,
            assume_role_policy: None,
            runtime_created: false,
            _internal_stay_count: None,
        }
    }
}

fn emit_aws_service_account_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    role: &alien_aws_clients::iam::Role,
    attached_policies: Vec<alien_aws_clients::iam::AttachedPolicy>,
    inline_policy_names: Vec<String>,
    stack_permissions_applied: bool,
) {
    let tag_count = role
        .tags
        .as_ref()
        .map(|tags| tags.member.len() as u32)
        .unwrap_or(0);
    let managed_tag_count = role
        .tags
        .as_ref()
        .map(|tags| {
            tags.member
                .iter()
                .filter(|tag| tag.key.starts_with("alien"))
                .count() as u32
        })
        .unwrap_or(0);
    let attached_policy_names = attached_policies
        .iter()
        .map(|policy| policy.policy_name.clone())
        .collect::<Vec<_>>();
    let attached_policy_count = attached_policy_names.len() as u32;
    let inline_policy_count = inline_policy_names.len() as u32;
    let message = format!("AWS IAM role '{}' is reachable", role.role_name);

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ServiceAccount::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ServiceAccount(ServiceAccountHeartbeatData::AwsIamRole(
            AwsIamRoleServiceAccountHeartbeatData {
                status: ServiceAccountHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(message),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                role_name: role.role_name.clone(),
                role_arn: role.arn.clone(),
                role_id: role.role_id.clone(),
                path: role.path.clone(),
                create_date: role.create_date.clone(),
                description: role.description.clone(),
                max_session_duration: role.max_session_duration,
                assume_role_policy_present: role.assume_role_policy_document.is_some(),
                permissions_boundary_type: role
                    .permissions_boundary
                    .as_ref()
                    .and_then(|boundary| boundary.permissions_boundary_type.clone()),
                permissions_boundary_arn: role
                    .permissions_boundary
                    .as_ref()
                    .and_then(|boundary| boundary.permissions_boundary_arn.clone()),
                tag_count,
                managed_tag_count,
                attached_policy_count,
                attached_policy_names,
                inline_policy_count,
                inline_policy_names,
                stack_permissions_applied,
                last_used_date: role
                    .role_last_used
                    .as_ref()
                    .and_then(|last_used| last_used.last_used_date.clone()),
                last_used_region: role
                    .role_last_used
                    .as_ref()
                    .and_then(|last_used| last_used.region.clone()),
            },
        )),
        raw: vec![],
    });
}

/// Trust only the named roles without requiring them to exist when setup creates the policy.
fn role_trust_statement(account_id: &str, role_arns: Vec<String>) -> TrustPolicyStatement {
    TrustPolicyStatement {
        effect: "Allow".to_string(),
        principal: TrustPolicyPrincipal::Aws {
            aws: TrustPolicyPrincipalValue::Single(format!("arn:aws:iam::{account_id}:root")),
        },
        action: "sts:AssumeRole".to_string(),
        condition: Some(serde_json::json!({
            "ArnEquals": { "aws:PrincipalArn": role_arns }
        })),
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;

    #[test]
    fn role_trust_requires_an_exact_allowed_role_in_the_same_account() {
        let roles = vec![
            "arn:aws:iam::123456789012:role/test-compute-role".to_string(),
            "arn:aws:iam::123456789012:role/test-execution-sa".to_string(),
        ];
        let policy = serde_json::to_value(role_trust_statement("123456789012", roles.clone()))
            .expect("trust statement should serialize");
        assert_eq!(
            policy,
            serde_json::json!({
                "Effect": "Allow",
                "Principal": {"AWS": "arn:aws:iam::123456789012:root"},
                "Action": "sts:AssumeRole",
                "Condition": {"ArnEquals": {"aws:PrincipalArn": roles}}
            })
        );
    }
}

/// `policy` with each `sts:ExternalId` it requires replaced by `sha256:<hex>` of the value. The
/// ExternalId is the one secret that guards the trust, so controller state, which reaches the
/// deployment's stored and synced state, keeps only its hash. A hash already in place stays.
/// A policy that isn't JSON is an error: kept as it is, it could hold the ExternalId in plain.
fn redacted_trust(policy: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut parsed = serde_json::from_str::<serde_json::Value>(policy)
        .into_alien_error()
        .context(ErrorData::InfrastructureError {
            message: "An IAM trust policy is not valid JSON".to_string(),
            operation: Some("redact the trust policy's ExternalId".to_string()),
            resource_id: None,
        })?;
    for statement in parsed["Statement"].as_array_mut().into_iter().flatten() {
        let Some(id) = statement
            .pointer_mut("/Condition/StringEquals/sts:ExternalId")
            .filter(|id| id.as_str().is_some_and(|id| !id.starts_with("sha256:")))
        else {
            continue;
        };
        let hash = format!(
            "sha256:{:x}",
            Sha256::digest(id.as_str().unwrap_or_default())
        );
        *id = serde_json::Value::String(hash);
    }
    Ok(parsed.to_string())
}

/// The `sts:ExternalId` a trust policy's statements require, if any.
fn trusted_external_id(policy: &str) -> Option<String> {
    let policy: serde_json::Value = serde_json::from_str(policy).ok()?;
    policy["Statement"]
        .as_array()?
        .iter()
        .find_map(|statement| statement["Condition"]["StringEquals"]["sts:ExternalId"].as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        controller_test::SingleControllerExecutor, MockPlatformServiceProvider, ResourceController,
        StackResourceStateExt,
    };
    use alien_aws_clients::iam::{
        ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, PolicyNames,
    };
    use alien_core::permissions::PermissionProfile;
    use std::sync::Arc;

    const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-deployment-management-boundary";

    fn role() -> serde_json::Value {
        serde_json::json!({
            "Path": "/", "RoleName": "test-execution-sa", "RoleId": "id",
            "Arn": "arn:aws:iam::123456789012:role/test-execution-sa", "CreateDate": "2026-01-01",
            "Tags": {"member": standard_resource_tags("test", "execution-sa").into_iter()
                .map(|(key, value)| serde_json::json!({"Key": key, "Value": value}))
                .collect::<Vec<_>>()}
        })
    }

    async fn executor(
        iam: MockIamApi,
        state: AwsServiceAccountState,
        bounded: bool,
    ) -> SingleControllerExecutor {
        executor_with_account(iam, state, bounded, policy_account(10)).await
    }

    async fn executor_with_account(
        iam: MockIamApi,
        state: AwsServiceAccountState,
        bounded: bool,
        account: ServiceAccount,
    ) -> SingleControllerExecutor {
        executor_with_controller(
            iam,
            AwsServiceAccountController {
                state,
                role_name: Some("test-execution-sa".to_string()),
                role_arn: Some("arn:aws:iam::123456789012:role/test-execution-sa".to_string()),
                ..Default::default()
            },
            bounded,
            account,
        )
        .await
    }

    async fn executor_with_controller(
        iam: MockIamApi,
        controller: AwsServiceAccountController,
        bounded: bool,
        account: ServiceAccount,
    ) -> SingleControllerExecutor {
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut management =
            crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                "test-management",
            );
        if bounded {
            management.permissions_boundary_arn = Some(BOUNDARY.to_string());
        }
        SingleControllerExecutor::builder()
            .resource(account)
            .previous_resource(policy_account(10))
            .controller(controller)
            .platform(Platform::Aws)
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
                management,
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap()
    }

    /// Removal adopts the role without auditing its attached policies.
    fn expect_removal_adoption(iam: &mut MockIamApi, sequence: &mut mockall::Sequence) {
        iam.expect_get_role()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-execution-sa");
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role()}}))
                        .unwrap(),
                )
            });
        iam.expect_put_role_permissions_boundary()
            .times(1)
            .in_sequence(sequence)
            .returning(|name, boundary| {
                assert_eq!(name, "test-execution-sa");
                assert_eq!(boundary, BOUNDARY);
                Ok(())
            });
    }

    fn expect_adoption(iam: &mut MockIamApi, sequence: &mut mockall::Sequence) {
        iam.expect_get_role()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-execution-sa");
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role()}}))
                        .unwrap(),
                )
            });
        iam.expect_list_attached_role_policies()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-execution-sa");
                Ok(serde_json::from_value(
                    serde_json::json!({"ListAttachedRolePoliciesResult": {"IsTruncated": false}}),
                )
                .unwrap())
            });
        iam.expect_put_role_permissions_boundary()
            .times(1)
            .in_sequence(sequence)
            .returning(|name, boundary| {
                assert_eq!(name, "test-execution-sa");
                assert_eq!(boundary, BOUNDARY);
                Ok(())
            });
    }

    #[tokio::test]
    async fn create_passes_the_recorded_boundary_and_ownership_tags() {
        for bounded in [false, true] {
            let mut iam = MockIamApi::new();
            iam.expect_create_role().times(1).returning(move |request| {
                assert_eq!(request.role_name, "test-execution-sa");
                assert_eq!(
                    request.permissions_boundary.as_deref(),
                    bounded.then_some(BOUNDARY)
                );
                let tags: std::collections::HashMap<_, _> = request
                    .tags
                    .unwrap()
                    .into_iter()
                    .map(|tag| (tag.key, tag.value))
                    .collect();
                assert_eq!(tags, standard_resource_tags("test", "execution-sa"));
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&request.assume_role_policy_document)
                        .unwrap()["Version"],
                    "2012-10-17"
                );
                Ok(serde_json::from_value(
                    serde_json::json!({"CreateRoleResult": {"Role": role()}}),
                )
                .unwrap())
            });
            let mut executor = executor(iam, AwsServiceAccountState::CreatingRole, bounded).await;
            executor.step().await.unwrap();
            let controller = executor
                .internal_state::<AwsServiceAccountController>()
                .unwrap();
            assert_eq!(
                controller.state,
                AwsServiceAccountState::ApplyingStackPermissions
            );
            assert_eq!(controller.role_name.as_deref(), Some("test-execution-sa"));
            assert_eq!(
                controller.role_arn.as_deref(),
                Some("arn:aws:iam::123456789012:role/test-execution-sa")
            );
        }
    }

    #[tokio::test]
    async fn update_adopts_before_writing_trust_and_preserves_legacy_behavior() {
        for bounded in [false, true] {
            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            if bounded {
                expect_adoption(&mut iam, &mut sequence);
            }
            iam.expect_update_assume_role_policy()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|name, policy| {
                    assert_eq!(name, "test-execution-sa");
                    assert_eq!(
                        serde_json::from_str::<serde_json::Value>(policy).unwrap()["Version"],
                        "2012-10-17"
                    );
                    Ok(())
                });
            let mut executor = executor(iam, AwsServiceAccountState::UpdateStart, bounded).await;
            executor.step().await.unwrap();
            assert_eq!(
                executor
                    .internal_state::<AwsServiceAccountController>()
                    .unwrap()
                    .state,
                AwsServiceAccountState::UpdatingStackPermissions
            );
        }
    }

    /// A successful attachment can outlive a failed policy write and process restart.
    #[tokio::test]
    async fn policy_write_failure_resumes_from_checkpoint_with_existing_boundary() {
        for state in [
            AwsServiceAccountState::ApplyingStackPermissions,
            AwsServiceAccountState::UpdatingStackPermissions,
        ] {
            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            expect_adoption(&mut iam, &mut sequence);
            iam.expect_put_role_policy()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|_, _, _| {
                    Err(AlienError::new(
                        alien_client_core::ErrorData::InvalidInput {
                            message: "injected policy write failure".into(),
                            field_name: None,
                        },
                    ))
                });
            let mut first = executor(iam, state.clone(), true).await;
            let error = first.step().await.unwrap_err();
            assert!(error.to_string().contains("injected policy write failure"));
            let checkpoint = first
                .internal_state::<AwsServiceAccountController>()
                .unwrap()
                .clone();
            assert_eq!(checkpoint.state, state);
            assert!(!checkpoint.stack_permissions_applied);
            assert_eq!(checkpoint.role_name.as_deref(), Some("test-execution-sa"));
            assert_eq!(
                checkpoint.role_arn.as_deref(),
                Some("arn:aws:iam::123456789012:role/test-execution-sa")
            );
            // Persist the retry checkpoint separately from the failed display state,
            // as the executor does for last_failed_state.
            let mut record = first.stack_state().resources["execution-sa"].clone();
            record
                .set_last_failed_controller(Some(Box::new(checkpoint.clone())))
                .unwrap();
            let mut failed = checkpoint;
            failed.transition_to_failure();
            assert_eq!(
                failed.get_status(),
                if state == AwsServiceAccountState::ApplyingStackPermissions {
                    ResourceStatus::ProvisionFailed
                } else {
                    ResourceStatus::UpdateFailed
                }
            );
            record.status = failed.get_status();
            record.retry_attempt = 3;
            record.error = Some(error.into_generic());
            record
                .set_internal_controller(Some(Box::new(failed)))
                .unwrap();
            let saved = serde_json::to_vec(&record).unwrap();
            drop(first);
            let mut record: alien_core::StackResourceState =
                serde_json::from_slice(&saved).unwrap();
            assert!(record.retry_failed().unwrap());
            assert!(!record.has_last_failed_state());
            assert_eq!(record.retry_attempt, 0);
            assert!(record.error.is_none());
            assert_eq!(
                record.status,
                if state == AwsServiceAccountState::ApplyingStackPermissions {
                    ResourceStatus::Provisioning
                } else {
                    ResourceStatus::Updating
                }
            );

            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            iam.expect_get_role()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|name| {
                    assert_eq!(name, "test-execution-sa");
                    let mut role = role();
                    role["PermissionsBoundary"] = serde_json::json!({
                        "PermissionsBoundaryType": "Policy", "PermissionsBoundaryArn": BOUNDARY
                    });
                    Ok(
                        serde_json::from_value(
                            serde_json::json!({"GetRoleResult": {"Role": role}}),
                        )
                        .unwrap(),
                    )
                });
            // No audit or attachment is expected after IAM confirms the boundary.
            iam.expect_put_role_policy()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|name, policy, document| {
                    assert_eq!((name, policy), ("test-execution-sa", MANAGED_POLICY_NAME));
                    let policy: serde_json::Value = serde_json::from_str(document).unwrap();
                    assert_eq!(
                        policy["Statement"][0]["Resource"].as_array().unwrap().len(),
                        10
                    );
                    Ok(())
                });
            let restored = record
                .get_internal_controller_typed::<AwsServiceAccountController>()
                .unwrap();
            let mut resumed =
                executor_with_controller(iam, restored, true, policy_account(10)).await;
            resumed.step().await.unwrap();
            let controller = resumed
                .internal_state::<AwsServiceAccountController>()
                .unwrap();
            assert!(controller.stack_permissions_applied);
            assert_eq!(
                controller.state,
                if state == AwsServiceAccountState::ApplyingStackPermissions {
                    AwsServiceAccountState::ApplyingResourcePermissions
                } else {
                    AwsServiceAccountState::UpdatingResourcePermissions
                }
            );
        }
    }

    #[tokio::test]
    async fn failed_boundary_attachment_preserves_checkpoint_and_retries_adoption() {
        let mut iam = MockIamApi::new();
        let mut sequence = mockall::Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role()}}))
                        .unwrap(),
                )
            });
        iam.expect_list_attached_role_policies()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(serde_json::from_value(
                    serde_json::json!({"ListAttachedRolePoliciesResult": {"IsTruncated": false}}),
                )
                .unwrap())
            });
        iam.expect_put_role_permissions_boundary()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name, boundary| {
                assert_eq!((name, boundary), ("test-execution-sa", BOUNDARY));
                Err(AlienError::new(
                    alien_client_core::ErrorData::InvalidInput {
                        message: "injected attachment failure".into(),
                        field_name: None,
                    },
                ))
            });
        // Any policy write before attachment succeeds is an unexpected mock call.
        let mut first = executor(iam, AwsServiceAccountState::UpdatingStackPermissions, true).await;
        let error = first.step().await.unwrap_err();
        assert!(error.to_string().contains("injected attachment failure"));
        let checkpoint = first
            .internal_state::<AwsServiceAccountController>()
            .unwrap()
            .clone();
        assert_eq!(
            checkpoint.state,
            AwsServiceAccountState::UpdatingStackPermissions
        );
        assert!(!checkpoint.stack_permissions_applied);
        let saved = serde_json::to_vec(&checkpoint).unwrap();
        drop(first);

        let mut iam = MockIamApi::new();
        let mut sequence = mockall::Sequence::new();
        expect_adoption(&mut iam, &mut sequence);
        iam.expect_put_role_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name, policy, _| {
                assert_eq!((name, policy), ("test-execution-sa", MANAGED_POLICY_NAME));
                Ok(())
            });
        let mut resumed = executor_with_controller(
            iam,
            serde_json::from_slice(&saved).unwrap(),
            true,
            policy_account(10),
        )
        .await;
        resumed.step().await.unwrap();
        let controller = resumed
            .internal_state::<AwsServiceAccountController>()
            .unwrap();
        assert_eq!(
            controller.state,
            AwsServiceAccountState::UpdatingResourcePermissions
        );
        assert!(controller.stack_permissions_applied);
        assert_eq!(controller.role_name, checkpoint.role_name);
        assert_eq!(controller.role_arn, checkpoint.role_arn);
    }

    #[tokio::test]
    async fn delete_adopts_before_removing_policies_and_role() {
        let mut iam = MockIamApi::new();
        let mut sequence = mockall::Sequence::new();
        expect_removal_adoption(&mut iam, &mut sequence);
        iam.expect_list_attached_role_policies()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Ok(serde_json::from_value(
                    serde_json::json!({"ListAttachedRolePoliciesResult": {"AttachedPolicies": {"member": [{"PolicyName": "AdministratorAccess", "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess"}]}, "IsTruncated": false}}),
                )
                .unwrap())
            });
        iam.expect_detach_role_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name, policy| {
                assert_eq!(
                    (name, policy),
                    (
                        "test-execution-sa",
                        "arn:aws:iam::aws:policy/AdministratorAccess"
                    )
                );
                Ok(())
            });
        iam.expect_list_role_policies()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name| {
                assert_eq!(name, "test-execution-sa");
                Ok(
                    serde_json::from_value(serde_json::json!({"ListRolePoliciesResult": {
                        "PolicyNames": {"member": ["grant"]}, "IsTruncated": false
                    }}))
                    .unwrap(),
                )
            });
        iam.expect_delete_role_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name, policy| {
                assert_eq!((name, policy), ("test-execution-sa", "grant"));
                Ok(())
            });
        iam.expect_delete_role()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name| {
                assert_eq!(name, "test-execution-sa");
                Ok(())
            });
        let mut executor = executor(iam, AwsServiceAccountState::DeleteStart, true).await;
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AwsServiceAccountController>()
                .unwrap()
                .state,
            AwsServiceAccountState::DeletingRole
        );
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<AwsServiceAccountController>()
            .unwrap();
        assert_eq!(controller.state, AwsServiceAccountState::Deleted);
        assert_eq!(controller.role_name, None);
        assert_eq!(controller.role_arn, None);
    }

    #[tokio::test]
    async fn missing_role_completes_bounded_delete_without_mutations() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role().times(1).returning(|name| {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Role".to_string(),
                    resource_name: name.to_string(),
                },
            ))
        });
        let mut executor = executor(iam, AwsServiceAccountState::DeleteStart, true).await;
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<AwsServiceAccountController>()
            .unwrap();
        assert_eq!(controller.state, AwsServiceAccountState::Deleted);
        assert_eq!(controller.role_name, None);
        assert_eq!(controller.role_arn, None);
    }

    #[tokio::test]
    async fn unsafe_roles_stop_update_and_delete_before_mutation() {
        for state in [
            AwsServiceAccountState::UpdateStart,
            AwsServiceAccountState::ApplyingStackPermissions,
            AwsServiceAccountState::UpdatingStackPermissions,
            AwsServiceAccountState::DeleteStart,
        ] {
            for unsafe_policy in [false, true] {
                // AWS managed policies keep a role from being adopted to keep running, not
                // from being deleted.
                if unsafe_policy && state == AwsServiceAccountState::DeleteStart {
                    continue;
                }
                let mut iam = MockIamApi::new();
                iam.expect_get_role().times(1).returning(move |_| {
                    let mut role = role();
                    if !unsafe_policy {
                        role["Tags"] = serde_json::Value::Null;
                    }
                    Ok(
                        serde_json::from_value(
                            serde_json::json!({"GetRoleResult": {"Role": role}}),
                        )
                        .unwrap(),
                    )
                });
                if unsafe_policy {
                    iam.expect_list_attached_role_policies().times(1).returning(|_| {
                        Ok(serde_json::from_value(serde_json::json!({"ListAttachedRolePoliciesResult": {
                            "AttachedPolicies": {"member": [{"PolicyName": "AdministratorAccess", "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess"}]}, "IsTruncated": false
                        }})).unwrap())
                    });
                }
                let mut executor = executor(iam, state.clone(), true).await;
                let error = executor
                    .step()
                    .await
                    .expect_err("unsafe role must not be modified");
                let expected_code = if unsafe_policy {
                    "INVALID_INPUT"
                } else {
                    "EXISTING_CLOUD_RESOURCE_NOT_OWNED"
                };
                assert!(
                    std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
                        .any(|cause| cause.code == expected_code),
                    "{error:?}"
                );
                let controller = executor
                    .internal_state::<AwsServiceAccountController>()
                    .unwrap();
                assert_eq!(controller.state, state);
                assert_eq!(controller.role_name.as_deref(), Some("test-execution-sa"));
                assert!(controller.role_arn.is_some());
            }
        }
    }

    #[tokio::test]
    async fn stack_policy_writes_adopt_first() {
        for state in [
            AwsServiceAccountState::ApplyingStackPermissions,
            AwsServiceAccountState::UpdatingStackPermissions,
        ] {
            for bounded in [false, true] {
                let mut iam = MockIamApi::new();
                let mut sequence = mockall::Sequence::new();
                if bounded {
                    expect_adoption(&mut iam, &mut sequence);
                }
                iam.expect_put_role_policy()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(|name, policy, document| {
                        assert_eq!((name, policy), ("test-execution-sa", MANAGED_POLICY_NAME));
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(document).unwrap()
                                ["Statement"][0]["Resource"]
                                .as_array()
                                .unwrap()
                                .len(),
                            10
                        );
                        Ok(())
                    });
                let next = if state == AwsServiceAccountState::ApplyingStackPermissions {
                    AwsServiceAccountState::ApplyingResourcePermissions
                } else {
                    AwsServiceAccountState::UpdatingResourcePermissions
                };
                let mut executor = executor(iam, state.clone(), bounded).await;
                executor.step().await.unwrap();
                assert_eq!(
                    executor
                        .internal_state::<AwsServiceAccountController>()
                        .unwrap()
                        .state,
                    next
                );
            }
        }
    }

    #[tokio::test]
    async fn empty_stack_policy_and_dropped_resource_grants_adopt_once_before_removal() {
        for (resource_grant, count) in [(false, 1), (true, 0), (true, 1), (true, 2)] {
            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            let policies: Vec<String> = if resource_grant {
                (0..count)
                    .map(|index| format!("deployment-old{index}-storage-data-read"))
                    .collect()
            } else {
                vec![MANAGED_POLICY_NAME.to_string()]
            };
            if resource_grant {
                let mut held = policies.clone();
                held.push("unrelated".to_string());
                iam.expect_list_role_policies()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(move |name| {
                        assert_eq!(name, "test-execution-sa");
                        Ok(
                            serde_json::from_value(serde_json::json!({"ListRolePoliciesResult": {
                                "PolicyNames": {"member": held}, "IsTruncated": false
                            }}))
                            .unwrap(),
                        )
                    });
            }
            if !policies.is_empty() {
                expect_adoption(&mut iam, &mut sequence);
            }
            for expected in policies {
                iam.expect_delete_role_policy()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(move |name, policy| {
                        assert_eq!(name, "test-execution-sa");
                        assert_eq!(policy, expected);
                        Ok(())
                    });
            }
            let state = if resource_grant {
                AwsServiceAccountState::UpdatingResourcePermissions
            } else {
                AwsServiceAccountState::UpdatingStackPermissions
            };
            let mut executor = executor_with_account(
                iam,
                state,
                true,
                ServiceAccount::new("execution-sa".into()).build(),
            )
            .await;
            executor.step().await.unwrap();
            assert_eq!(
                executor
                    .internal_state::<AwsServiceAccountController>()
                    .unwrap()
                    .state,
                if resource_grant {
                    AwsServiceAccountState::Ready
                } else {
                    AwsServiceAccountState::UpdatingResourcePermissions
                }
            );
        }
    }

    #[tokio::test]
    async fn records_runtime_ownership_only_after_automatic_role_creation() {
        for management in [
            alien_core::SetupManagement::Manual,
            alien_core::SetupManagement::Automatic,
        ] {
            let mut iam = alien_aws_clients::iam::MockIamApi::new();
            iam.expect_create_role().times(1).returning(|request| {
                Ok(serde_json::from_value(serde_json::json!({"CreateRoleResult": {"Role": {
                    "Path": "/", "RoleName": request.role_name, "RoleId": "created-role",
                    "Arn": "arn:aws:iam::123456789012:role/created-role", "CreateDate": "2026-10-30T00:00:00Z"
                }}})).unwrap())
            });
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource(ServiceAccount::new("execution-sa".to_string()).build())
                .controller(AwsServiceAccountController::default())
                .platform(Platform::Aws)
                .stack_settings(alien_core::StackSettings {
                    setup_management: management,
                    ..Default::default()
                })
                .service_provider(Arc::new(provider))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            assert!(
                !executor
                    .internal_state::<AwsServiceAccountController>()
                    .unwrap()
                    .runtime_created
            );
            executor.step().await.unwrap();
            let checkpoint = executor
                .internal_state::<AwsServiceAccountController>()
                .unwrap()
                .to_json_value()
                .unwrap();
            let restored: AwsServiceAccountController = serde_json::from_value(checkpoint).unwrap();
            assert_eq!(
                restored.runtime_created,
                management == alien_core::SetupManagement::Automatic
            );
            assert!(restored.role_arn.is_some());
        }
    }

    #[tokio::test]
    async fn resumed_legacy_role_deletion_checks_live_ownership() {
        for owner in ["runtime", "setup"] {
            let mut iam = alien_aws_clients::iam::MockIamApi::new();
            iam.expect_get_role().times(1).returning(move |_| {
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": {
                        "Path": "/", "RoleName": "test-execution-sa", "RoleId": "existing-role",
                        "Arn": "arn:aws:iam::123456789012:role/test-execution-sa",
                        "CreateDate": "2026-10-30T00:00:00Z", "Tags": {"member": [
                            {"Key": "managed-by", "Value": owner},
                            {"Key": "deployment", "Value": "test"},
                            {"Key": "resource", "Value": "execution-sa"}
                        ]}
                    }}}))
                    .unwrap(),
                )
            });
            iam.expect_delete_role()
                .withf(|role| role == "test-execution-sa")
                .times(usize::from(owner == "runtime"))
                .returning(|_| Ok(()));
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let controller: AwsServiceAccountController =
                serde_json::from_value(serde_json::json!({
                    "state": "deletingRole", "roleName": "test-execution-sa",
                    "roleArn": "arn:aws:iam::123456789012:role/test-execution-sa",
                    "stackPermissionsApplied": true
                }))
                .unwrap();
            let mut executor = SingleControllerExecutor::builder()
                .resource(ServiceAccount::new("execution-sa".to_string()).build())
                .controller(controller)
                .platform(Platform::Aws)
                .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
                .stack_settings(alien_core::StackSettings {
                    setup_management: alien_core::SetupManagement::Automatic,
                    ..Default::default()
                })
                .service_provider(Arc::new(provider))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            executor.step().await.unwrap();
            let restored = executor
                .internal_state::<AwsServiceAccountController>()
                .unwrap();
            assert_eq!(restored.get_status(), ResourceStatus::Deleted);
            assert_eq!(restored.role_name.is_some(), owner == "setup");
        }
    }

    fn policy_account(count: usize) -> ServiceAccount {
        let resources: Vec<_> = (0..count)
            .map(|index| format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}"))
            .collect();
        let set = serde_json::from_value(serde_json::json!({
            "id": "q5-policy-read", "description": "Read explicit test objects",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]}, "binding": {"stack": {"resources": resources}}}]}
        })).unwrap();
        ServiceAccount::new("execution-sa".into())
            .stack_permission_set(set)
            .build()
    }

    /// Direct setup creates an extension's identity as the setup template does: only the managing
    /// role may assume it, and it holds the extension's sets.
    #[tokio::test]
    async fn direct_setup_creates_the_extension_identity_trusted_by_the_managing_role() {
        let set = serde_json::from_value(serde_json::json!({
            "id": "acme/dns-zone/provision", "description": "Create the zone",
            "platforms": {"aws": [{"grant": {"actions": ["route53:CreateHostedZone"]},
                "binding": {"stack": {"resources": ["*"]}}}]}
        }))
        .unwrap();
        let account = ServiceAccount::new("acme-ext".into())
            .stack_permission_set(set)
            .extension("acme".into())
            .build();
        let mut iam = MockIamApi::new();
        let mut sequence = mockall::Sequence::new();
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|request| {
                assert_eq!(request.role_name, "test-acme-ext");
                let manager = "arn:aws:iam::111122223333:role/alien-test-manager";
                let trust: serde_json::Value =
                    serde_json::from_str(&request.assume_role_policy_document).unwrap();
                assert_eq!(
                    trust["Statement"],
                    serde_json::json!([{
                        "Sid": "AllowOnlySpecificSourceRole",
                        "Effect": "Allow",
                        "Principal": { "AWS": manager },
                        "Action": "sts:AssumeRole",
                        "Condition": { "StringEquals": { "aws:PrincipalArn": manager } },
                    }])
                );
                let mut role = role();
                role["RoleName"] = "test-acme-ext".into();
                role["Arn"] = "arn:aws:iam::123456789012:role/test-acme-ext".into();
                Ok(
                    serde_json::from_value(serde_json::json!({"CreateRoleResult": {"Role": role}}))
                        .unwrap(),
                )
            });
        iam.expect_put_role_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|role, name, document| {
                let policy: serde_json::Value = serde_json::from_str(document).unwrap();
                role == "test-acme-ext"
                    && name == MANAGED_POLICY_NAME
                    && policy["Statement"][0]["Action"]
                        == serde_json::json!(["route53:CreateHostedZone"])
            })
            .returning(|_, _, _| Ok(()));
        let mut executor = executor_with_controller(
            iam,
            AwsServiceAccountController {
                state: AwsServiceAccountState::CreatingRole,
                ..Default::default()
            },
            false,
            account,
        )
        .await;
        executor.step().await.unwrap();
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<AwsServiceAccountController>()
            .unwrap();
        assert_eq!(
            controller.state,
            AwsServiceAccountState::ApplyingResourcePermissions
        );
        let outputs = controller.build_outputs().unwrap();
        assert_eq!(
            outputs
                .downcast_ref::<alien_core::ServiceAccountOutputs>()
                .unwrap()
                .identity,
            "arn:aws:iam::123456789012:role/test-acme-ext"
        );
    }

    /// With the ExternalId the control plane issued, direct setup makes the managing role send it
    /// to assume the extension's identity, so the managing role can't be pointed at this role for
    /// another deployment; a later run without the value keeps the trust as it is.
    #[tokio::test]
    async fn the_extension_identity_trust_requires_the_deployments_external_id() {
        const ID: &str = "0123456789abcdef0123456789ABCDEF";
        let account = ServiceAccount::new("acme-ext".into())
            .extension("acme".into())
            .build();
        let mut iam = MockIamApi::new();
        iam.expect_create_role().times(1).returning(|request| {
            let manager = "arn:aws:iam::111122223333:role/alien-test-manager";
            let trust: serde_json::Value =
                serde_json::from_str(&request.assume_role_policy_document).unwrap();
            assert_eq!(
                trust["Statement"][0]["Condition"],
                serde_json::json!({ "StringEquals": {
                    "aws:PrincipalArn": manager, "sts:ExternalId": ID
                }})
            );
            let mut role = role();
            role["RoleName"] = "test-acme-ext".into();
            role["Arn"] = "arn:aws:iam::123456789012:role/test-acme-ext".into();
            Ok(
                serde_json::from_value(serde_json::json!({"CreateRoleResult": {"Role": role}}))
                    .unwrap(),
            )
        });
        iam.expect_put_role_policy().returning(|_, _, _| Ok(()));
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(account)
            .controller(AwsServiceAccountController {
                state: AwsServiceAccountState::CreatingRole,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
            .management_external_id(alien_core::ManagementExternalId::new(ID).unwrap())
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
                crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                    "test-management",
                ),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let stored = executor
            .internal_state::<AwsServiceAccountController>()
            .unwrap()
            .assume_role_policy
            .clone()
            .unwrap();
        assert!(!stored.contains(ID), "state keeps only the hash: {stored}");
        assert!(stored.contains("sha256:"), "{stored}");

        executor.set_management_external_id(None);
        assert!(
            !executor.needs_update().unwrap(),
            "the trust keeps its ExternalId"
        );
    }

    /// State keeps only the ExternalId's hash, so an update that runs without the ExternalId
    /// reads the value from the role's trust, and keeps it.
    #[tokio::test]
    async fn an_update_without_the_external_id_keeps_the_one_the_role_trusts() {
        const ID: &str = "0123456789abcdef0123456789ABCDEF";
        let manager = "arn:aws:iam::111122223333:role/alien-test-manager";
        let live = crate::remote_bindings::managing_role_trust_policy(manager, Some(ID));
        let account = ServiceAccount::new("acme-ext".into())
            .extension("acme".into())
            .build();
        let stored_trust = redacted_trust(&live).unwrap();
        let mut iam = MockIamApi::new();
        iam.expect_get_role().times(1).returning(move |_| {
            let mut role = role();
            role["RoleName"] = "test-acme-ext".into();
            role["AssumeRolePolicyDocument"] = urlencoding::encode(&live).into_owned().into();
            Ok(
                serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role}}))
                    .unwrap(),
            )
        });
        iam.expect_update_assume_role_policy()
            .times(1)
            .withf(|_, policy| trusted_external_id(policy).as_deref() == Some(ID))
            .returning(|_, _| Ok(()));
        iam.expect_list_role_policies().returning(|_| {
            Ok(serde_json::from_value(serde_json::json!({
                "ListRolePoliciesResult": { "PolicyNames": [], "IsTruncated": false }
            }))
            .unwrap())
        });
        iam.expect_list_attached_role_policies().returning(|_| {
            Ok(serde_json::from_value(serde_json::json!({
                "ListAttachedRolePoliciesResult": { "AttachedPolicies": [], "IsTruncated": false }
            }))
            .unwrap())
        });
        iam.expect_put_role_policy().returning(|_, _, _| Ok(()));
        iam.expect_delete_role_policy().returning(|_, _| Ok(()));
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut controller = AwsServiceAccountController::mock_ready("acme-ext");
        controller.role_name = Some("test-acme-ext".into());
        controller.assume_role_policy = Some(stored_trust);
        let mut executor = SingleControllerExecutor::builder()
            .resource(account.clone())
            .controller(controller)
            .platform(Platform::Aws)
            .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
                crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                    "test-management",
                ),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.update(account).unwrap();
        executor.step().await.unwrap();
        let stored = executor
            .internal_state::<AwsServiceAccountController>()
            .unwrap()
            .assume_role_policy
            .clone()
            .unwrap();
        assert!(!stored.contains(ID), "{stored}");
    }

    /// An update without the ExternalId never writes a trust without it: when the role's trust
    /// no longer checks the value state recorded, the update fails and leaves the trust alone.
    #[tokio::test]
    async fn an_update_without_the_external_id_refuses_a_trust_it_cannot_keep() {
        const ID: &str = "0123456789abcdef0123456789ABCDEF";
        let manager = "arn:aws:iam::111122223333:role/alien-test-manager";
        let recorded = redacted_trust(&crate::remote_bindings::managing_role_trust_policy(
            manager,
            Some(ID),
        ))
        .unwrap();
        let account = ServiceAccount::new("acme-ext".into())
            .extension("acme".into())
            .build();
        for live in [
            Some(crate::remote_bindings::managing_role_trust_policy(
                manager, None,
            )),
            Some(crate::remote_bindings::managing_role_trust_policy(
                manager,
                Some("ffffffffffffffffffffffffffffffff"),
            )),
            None,
        ] {
            let mut iam = MockIamApi::new();
            iam.expect_get_role().times(1).returning(move |_| {
                let mut role = role();
                role["RoleName"] = "test-acme-ext".into();
                role["AssumeRolePolicyDocument"] = match &live {
                    Some(live) => urlencoding::encode(live).into_owned().into(),
                    None => serde_json::Value::Null,
                };
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role}}))
                        .unwrap(),
                )
            });
            iam.expect_update_assume_role_policy().times(0);
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let mut controller = AwsServiceAccountController::mock_ready("acme-ext");
            controller.role_name = Some("test-acme-ext".into());
            controller.assume_role_policy = Some(recorded.clone());
            let mut executor = SingleControllerExecutor::builder()
                .resource(account.clone())
                .controller(controller)
                .platform(Platform::Aws)
                .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
                .with_dependency(
                    alien_core::RemoteStackManagement::new("management".to_string()).build(),
                    crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                        "test-management",
                    ),
                )
                .service_provider(Arc::new(provider))
                .build()
                .await
                .unwrap();
            executor.update(account.clone()).unwrap();
            let error = executor.step().await.unwrap_err();
            assert!(error.message.contains("ExternalId"), "{}", error.message);
        }
    }

    /// A role state holds no ExternalId for, such as one an import recorded without its trust,
    /// keeps the ExternalId its live trust checks when an update runs without one. A live trust
    /// that doesn't parse names none to keep, so the update fails without writing.
    #[tokio::test]
    async fn an_update_without_the_external_id_keeps_the_live_one_state_never_recorded() {
        const ID: &str = "0123456789abcdef0123456789ABCDEF";
        let manager = "arn:aws:iam::111122223333:role/alien-test-manager";
        let account = ServiceAccount::new("acme-ext".into())
            .extension("acme".into())
            .build();
        let live = crate::remote_bindings::managing_role_trust_policy(manager, Some(ID));
        for (recorded, live, writes) in [
            (None, live.clone(), true),
            (
                Some(crate::remote_bindings::managing_role_trust_policy(
                    manager, None,
                )),
                live,
                true,
            ),
            (None, format!("{{\"sts:ExternalId\": \"{ID}\""), false),
        ] {
            let mut iam = MockIamApi::new();
            iam.expect_get_role().times(1).returning(move |_| {
                let mut role = role();
                role["RoleName"] = "test-acme-ext".into();
                role["AssumeRolePolicyDocument"] = urlencoding::encode(&live).into_owned().into();
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role}}))
                        .unwrap(),
                )
            });
            iam.expect_update_assume_role_policy()
                .times(usize::from(writes))
                .withf(|_, policy| trusted_external_id(policy).as_deref() == Some(ID))
                .returning(|_, _| Ok(()));
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let mut controller = AwsServiceAccountController::mock_ready("acme-ext");
            controller.role_name = Some("test-acme-ext".into());
            controller.assume_role_policy = recorded;
            let mut executor = SingleControllerExecutor::builder()
                .resource(account.clone())
                .controller(controller)
                .platform(Platform::Aws)
                .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
                .with_dependency(
                    alien_core::RemoteStackManagement::new("management".to_string()).build(),
                    crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                        "test-management",
                    ),
                )
                .service_provider(Arc::new(provider))
                .build()
                .await
                .unwrap();
            executor.update(account.clone()).unwrap();
            assert_eq!(executor.step().await.is_ok(), writes);
        }
    }

    /// A trust that isn't JSON can't have its ExternalId replaced, so it never reaches state.
    #[test]
    fn a_trust_that_is_not_json_is_never_kept() {
        let trust = r#"{"Statement": [{"Condition": {"StringEquals": {"sts:ExternalId": "0123456789abcdef0123456789ABCDEF"}}}"#;
        assert!(redacted_trust(trust).is_err());
    }

    /// A long extension identity's role name is cut to IAM's limit as the template cuts it.
    #[tokio::test]
    async fn direct_setup_names_a_long_extension_identity_as_the_template_does() {
        let id = "contoso-backup-and-restore-for-postgres-mysql-and-sqlserver-ext";
        let expected = alien_core::extension_identity_aws_role_name("test", id);
        assert_eq!(expected.len(), 64);
        let mut iam = MockIamApi::new();
        let name = expected.clone();
        iam.expect_create_role().times(1).returning(move |request| {
            assert_eq!(request.role_name, name);
            assert!(
                request.description.as_deref().is_some_and(
                    |description| description.starts_with("Extension identity (contoso)")
                ),
                "{:?}",
                request.description
            );
            let mut role = role();
            role["RoleName"] = name.clone().into();
            role["Arn"] = format!("arn:aws:iam::123456789012:role/{name}").into();
            Ok(
                serde_json::from_value(serde_json::json!({"CreateRoleResult": {"Role": role}}))
                    .unwrap(),
            )
        });
        let mut executor = executor_with_controller(
            iam,
            AwsServiceAccountController {
                state: AwsServiceAccountState::CreatingRole,
                ..Default::default()
            },
            false,
            ServiceAccount::new(id.into())
                .extension("contoso".into())
                .build(),
        )
        .await;
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AwsServiceAccountController>()
                .unwrap()
                .role_name,
            Some(expected)
        );
    }

    #[tokio::test]
    async fn oversized_inline_policy_rejects_create_and_update_before_iam() {
        for state in [
            AwsServiceAccountState::CreatingRole,
            AwsServiceAccountState::UpdateStart,
            AwsServiceAccountState::ApplyingStackPermissions,
            AwsServiceAccountState::UpdatingStackPermissions,
        ] {
            let mut executor = SingleControllerExecutor::builder()
                .resource(policy_account(400))
                .previous_resource(policy_account(10))
                .controller(AwsServiceAccountController {
                    state,
                    role_name: Some("q5-policy-execution-sa".into()),
                    role_arn: Some("arn:aws:iam::123456789012:role/q5-policy-execution-sa".into()),
                    ..Default::default()
                })
                .platform(Platform::Aws)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            let error = executor.step().await.unwrap_err();
            assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
            assert!(error.to_string().contains("10240"), "{error}");
            assert!(error.to_string().contains("execution-sa"), "{error}");
        }
    }

    #[tokio::test]
    async fn small_inline_policy_reaches_iam_with_the_rendered_document() {
        let mut iam = alien_aws_clients::iam::MockIamApi::new();
        iam.expect_put_role_policy()
            .times(1)
            .withf(|role, name, document| {
                let policy: serde_json::Value = serde_json::from_str(document).unwrap();
                role == "q5-policy-execution-sa"
                    && name == MANAGED_POLICY_NAME
                    && policy["Statement"][0]["Resource"].as_array().unwrap().len() == 10
                    && document.chars().count() < 10240
            })
            .returning(|_, _, _| Ok(()));
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .times(1)
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(policy_account(10))
            .controller(AwsServiceAccountController {
                state: AwsServiceAccountState::ApplyingStackPermissions,
                role_name: Some("q5-policy-execution-sa".into()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
    }

    /// Creates the workload role through the real controller. The companion
    /// cloud runner verifies role chaining from legacy, isolated and unrelated
    /// task-owned node identities, and deletes all test resources.
    #[tokio::test]
    #[ignore = "requires isolated AWS node-role fixtures"]
    async fn live_create_service_account_for_compute_nodes() {
        let prefix = std::env::var("ALIEN_TEST_TRUST_PREFIX").unwrap();
        assert!(prefix.starts_with("e2e-"));
        let workload = Container::new("api".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "example.invalid/probe:1".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.25".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "256Mi".to_string(),
                desired: "256Mi".to_string(),
            })
            .port(8080)
            .permissions("execution".to_string())
            .build();
        let config = alien_core::AwsClientConfig {
            account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").unwrap(),
            region: std::env::var("AWS_TARGET_REGION").unwrap(),
            credentials: alien_core::AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID").unwrap(),
                secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY").unwrap(),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
            },
            service_overrides: None,
        };
        let existing = std::env::var("ALIEN_TEST_TRUST_EXISTING").as_deref() == Ok("1");
        let resource = ServiceAccount::new("execution-sa".to_string()).build();
        let controller = if existing {
            serde_json::from_value(serde_json::json!({
                "state": "ready",
                "roleName": format!("{prefix}-execution-sa"),
                "roleArn": format!("arn:aws:iam::{}:role/{prefix}-execution-sa", config.account_id),
                "stackPermissionsApplied": true
            }))
            .unwrap()
        } else {
            AwsServiceAccountController::default()
        };
        let mut executor = SingleControllerExecutor::builder()
            .resource(resource.clone())
            .controller(controller)
            .platform(Platform::Aws)
            .client_config(alien_core::ClientConfig::Aws(Box::new(config)))
            .resource_prefix(prefix)
            .with_stack_resource(workload, alien_core::ResourceLifecycle::Live)
            .with_stack_resource(
                ComputeCluster::new("compute".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .real_delays()
            .build()
            .await
            .unwrap();
        if existing {
            assert!(
                executor.needs_update().unwrap(),
                "legacy checkpoint must request setup reconciliation"
            );
            executor.update(resource).unwrap();
        }
        executor
            .run_until_status(ResourceStatus::Running)
            .await
            .unwrap();
        assert!(
            !executor.needs_update().unwrap(),
            "trust must converge after setup"
        );
    }

    /// A real config change, here a dropped stack grant, still needs setup.
    #[tokio::test]
    async fn resumed_trust_and_policy_updates_reject_runtime_authority_before_iam() {
        let granted = ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &PermissionProfile::new().global(["vault/data-read"]),
            |id| alien_permissions::get_permission_set(id).cloned(),
        )
        .unwrap();
        for state in [
            AwsServiceAccountState::UpdateStart,
            AwsServiceAccountState::ApplyingStackPermissions,
            AwsServiceAccountState::UpdatingStackPermissions,
        ] {
            let controller = AwsServiceAccountController {
                state,
                role_name: Some("test-reader-sa".to_string()),
                role_arn: Some("arn:aws:iam::123456789012:role/test-reader-sa".to_string()),
                ..Default::default()
            };
            let mut executor = SingleControllerExecutor::builder()
                .resource(ServiceAccount::new("reader-sa".to_string()).build())
                .previous_resource(granted.clone())
                .controller(controller)
                .platform(Platform::Aws)
                .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            assert!(!executor.needs_update().unwrap());
            let error = executor.step().await.unwrap_err();
            assert_eq!(error.code, "IMPORTED_SETUP_STATE_INVALID");
        }
    }

    /// An update with an unchanged config only records new stack edges, so a template-installed
    /// role is left alone. Direct setup still reconciles the role.
    #[tokio::test]
    async fn unchanged_config_updates_avoid_iam_only_for_imported_handoffs() {
        let account = ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &PermissionProfile::new()
                .global(["vault/data-read"])
                .resource("objects", ["storage/data-read"]),
            |id| alien_permissions::get_permission_set(id).cloned(),
        )
        .unwrap();
        for authority in [
            alien_core::InitialSetupAuthority::ImportedHandoff,
            alien_core::InitialSetupAuthority::DirectSetup,
        ] {
            let direct_setup = authority == alien_core::InitialSetupAuthority::DirectSetup;
            let mut iam = alien_aws_clients::iam::MockIamApi::new();
            iam.expect_update_assume_role_policy()
                .times(usize::from(direct_setup))
                .returning(|_, _| Ok(()));
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .times(usize::from(direct_setup))
                .returning(move |_| Ok(iam.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource(account.clone())
                .controller(AwsServiceAccountController::mock_ready("reader-sa"))
                .platform(Platform::Aws)
                .initial_setup_authority(authority)
                .service_provider(Arc::new(provider))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            executor.update(account.clone()).unwrap();
            executor.step().await.unwrap();
            assert_eq!(
                executor.status(),
                if direct_setup {
                    ResourceStatus::Updating
                } else {
                    ResourceStatus::Running
                },
                "{authority:?}"
            );
        }
    }

    #[tokio::test]
    async fn legacy_grant_capture_avoids_iam_only_for_imported_handoffs() {
        let profile = PermissionProfile::new().resource("objects", ["storage/data-read"]);
        let captured =
            ServiceAccount::from_permission_profile("reader-sa".to_string(), &profile, |id| {
                alien_permissions::get_permission_set(id).cloned()
            })
            .unwrap();
        for authority in [
            alien_core::InitialSetupAuthority::ImportedHandoff,
            alien_core::InitialSetupAuthority::DirectSetup,
        ] {
            let direct_setup = authority == alien_core::InitialSetupAuthority::DirectSetup;
            let mut legacy = captured.clone();
            legacy.resource_permission_sets.clear();
            let controller = AwsServiceAccountController {
                state: AwsServiceAccountState::Ready,
                role_arn: Some("arn:aws:iam::123456789012:role/reader-sa".to_string()),
                role_name: Some("reader-sa".to_string()),
                stack_permissions_applied: true,
                ..Default::default()
            };
            let mut iam = alien_aws_clients::iam::MockIamApi::new();
            iam.expect_update_assume_role_policy()
                .times(usize::from(direct_setup))
                .returning(|_, _| Ok(()));
            iam.expect_delete_role_policy()
                .times(usize::from(direct_setup))
                .returning(|_, _| Ok(()));
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .times(2 * usize::from(direct_setup))
                .returning(move |_| Ok(iam.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource(legacy)
                .controller(controller)
                .platform(Platform::Aws)
                .initial_setup_authority(authority)
                .service_provider(Arc::new(provider))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            executor.update(captured.clone()).unwrap();
            executor.step().await.unwrap();
            if direct_setup {
                executor.step().await.unwrap();
            }
            assert_eq!(
                executor.status(),
                if direct_setup {
                    ResourceStatus::Updating
                } else {
                    ResourceStatus::Running
                }
            );
        }
    }

    /// A grant on a resource the account no longer has, for example because the resource was
    /// removed from the stack, is deleted from the role by setup, whether or not the installed
    /// account recorded its grants (accounts installed before grants were captured record none).
    /// Grants it keeps, under either name, and the role's other policies stay.
    #[tokio::test]
    async fn setup_removes_the_grant_of_a_removed_resource() {
        let account = |profile: &PermissionProfile| {
            ServiceAccount::from_permission_profile("app-sa".to_string(), profile, |id| {
                alien_permissions::get_permission_set(id).cloned()
            })
            .unwrap()
        };
        let installed = account(
            &PermissionProfile::new()
                .resource("jobs", ["queue/data-write"])
                .resource("extra", ["queue/data-write"]),
        );
        let mut legacy = installed.clone();
        legacy.resource_permission_sets.clear();
        let desired_profile = PermissionProfile::new().resource("jobs", ["queue/data-write"]);
        let desired = account(&desired_profile);

        for (case, previous) in [("captured", installed), ("legacy", legacy)] {
            let deleted = Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut iam = alien_aws_clients::iam::MockIamApi::new();
            iam.expect_list_role_policies().returning(|_| {
                Ok(ListRolePoliciesResponse {
                    list_role_policies_result: ListRolePoliciesResult {
                        policy_names: Some(PolicyNames {
                            member: [
                                "deployment-permissions",
                                "alien-jobs-queue-data-write",
                                "deployment-extra-queue-data-write",
                                "sandbox-image-build",
                            ]
                            .map(String::from)
                            .to_vec(),
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });
            let recorded = deleted.clone();
            iam.expect_delete_role_policy()
                .returning(move |role, policy| {
                    recorded.lock().unwrap().push(format!("{role}/{policy}"));
                    Ok(())
                });
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let mut executor = SingleControllerExecutor::builder()
                .resource(desired.clone())
                .previous_resource(previous)
                .controller(AwsServiceAccountController {
                    state: AwsServiceAccountState::UpdatingResourcePermissions,
                    role_arn: Some("arn:aws:iam::123456789012:role/test-app-sa".to_string()),
                    role_name: Some("test-app-sa".to_string()),
                    stack_permissions_applied: true,
                    ..Default::default()
                })
                .platform(Platform::Aws)
                .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
                .service_provider(Arc::new(provider))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            executor.step().await.unwrap_or_else(|error| {
                panic!("{case}: the resource permission step fails: {error}")
            });

            assert_eq!(
                *deleted.lock().unwrap(),
                vec!["test-app-sa/deployment-extra-queue-data-write".to_string()],
                "{case}"
            );
        }
    }

    /// A stack-wide grant of an extension's permission set resolves from the stack: Alien's own
    /// registry has no set by that id.
    #[tokio::test]
    async fn a_stack_wide_extension_grant_resolves_from_the_stack() {
        let (extension, profile) = crate::service_account::extension_with_a_stack_wide_set();
        let account = ServiceAccount::new("app-sa".to_string()).build();
        let mut iam = alien_aws_clients::iam::MockIamApi::new();
        iam.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec!["deployment-permissions".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(account.clone())
            .previous_resource(account)
            .controller(AwsServiceAccountController {
                state: AwsServiceAccountState::UpdatingResourcePermissions,
                role_arn: Some("arn:aws:iam::123456789012:role/test-app-sa".to_string()),
                role_name: Some("test-app-sa".to_string()),
                stack_permissions_applied: true,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .permission_profile("app", profile)
            .extension(extension)
            .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("the extension's set did not resolve: {error}"));
    }

    /// The executor replaces a ProvisionFailed resource whose config changed.
    /// A failed update of a live role must stay UpdateFailed instead.
    #[test]
    fn update_permission_failures_never_look_like_failed_creates() {
        for state in [
            AwsServiceAccountState::UpdateStart,
            AwsServiceAccountState::UpdatingStackPermissions,
            AwsServiceAccountState::UpdatingResourcePermissions,
        ] {
            let mut controller = AwsServiceAccountController {
                state,
                ..AwsServiceAccountController::mock_ready("test-reader-sa")
            };
            assert_eq!(controller.get_status(), ResourceStatus::Updating);
            controller.transition_to_failure();
            assert_eq!(controller.get_status(), ResourceStatus::UpdateFailed);
        }
    }
}
