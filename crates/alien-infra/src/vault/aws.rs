use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::core::ResourcePermissionsHelper;
use crate::error::{ErrorData, Result};
use alien_aws_clients::ssm::{
    DescribeParametersRequest, DescribeParametersResponse, ParameterMetadata, ParameterStringFilter,
};
use alien_core::{
    AwsParameterStoreVaultHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Vault, VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use chrono::{DateTime, Utc};

/// AWS Vault controller.
///
/// AWS SSM Parameter Store implicitly exists in every AWS account and region.
/// This controller simply sets up the vault reference without creating any infrastructure.
/// The vault represents a namespace prefix for SecureString parameters in SSM.
#[controller]
pub struct AwsVaultController {
    /// AWS account ID for generating the Secrets Manager reference
    pub(crate) account_id: Option<String>,
    /// The AWS region for this vault
    pub(crate) region: Option<String>,
    /// The vault prefix (resource id)
    pub(crate) vault_prefix: Option<String>,
}

#[controller]
impl AwsVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            region = %aws_cfg.region,
            "Setting up AWS SSM Parameter Store vault reference"
        );

        let account_id = aws_cfg.account_id.to_string();

        let vault_prefix = format!("{}-{}", ctx.resource_prefix, config.id);

        ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
            ctx,
            &config.id,
            &vault_prefix,
            "vault",
        )
        .await?;

        // Store the vault prefix using resource_prefix-config.id pattern
        self.vault_prefix = Some(vault_prefix);

        info!(
            vault_id = %config.id,
            account_id = %account_id,
            region = %aws_cfg.region,
            vault_prefix = %self.vault_prefix.as_deref().unwrap_or("unknown"),
            "AWS SSM Parameter Store vault is ready (implicitly exists)"
        );

        self.account_id = Some(account_id);
        self.region = Some(aws_cfg.region.clone());

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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "AWS SSM Parameter Store vault update complete (no infrastructure to update)"
        );

        // No infrastructure to update - Secrets Manager exists implicitly
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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "Deleting AWS SSM Parameter Store vault reference (no infrastructure to delete)"
        );

        // Clear stored values
        self.account_id = None;
        self.region = None;
        self.vault_prefix = None;

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
        let config = ctx.desired_resource_config::<Vault>()?;

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

            debug!(account_id=%stored_account_id, region=%stored_region, "AWS SSM Parameter Store vault heartbeat check passed");
        }

        if let Some(vault_prefix) = &self.vault_prefix {
            let client = ctx.service_provider.get_aws_ssm_client(aws_cfg).await?;
            let response = client
                .describe_parameters(DescribeParametersRequest {
                    parameter_filters: Some(vec![ParameterStringFilter {
                        key: "Name".to_string(),
                        option: Some("BeginsWith".to_string()),
                        values: Some(vec![vault_prefix.clone()]),
                    }]),
                    max_results: Some(50),
                    next_token: None,
                })
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to describe SSM Parameter Store metadata for prefix '{}'",
                        vault_prefix
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_aws_parameter_store_vault_heartbeat(
                ctx,
                &config.id,
                &aws_cfg.account_id.to_string(),
                &aws_cfg.region,
                vault_prefix,
                response,
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
            let vault_id = format!("{}:{}", account_id, region);
            Some(ResourceOutputs::new(VaultOutputs { vault_id }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_prefix) = &self.vault_prefix {
            let binding = VaultBinding::parameter_store(vault_prefix.clone());

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

fn emit_aws_parameter_store_vault_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    account_id: &str,
    region: &str,
    prefix: &str,
    response: DescribeParametersResponse,
) {
    let parameters = response.parameters.unwrap_or_default();
    let has_more_parameters = response.next_token.is_some();
    let sampled_parameter_count = parameters.len() as u32;
    let latest_modified_at = latest_modified_at(&parameters);

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Vault::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Vault(VaultHeartbeatData::AwsParameterStore(
            AwsParameterStoreVaultHeartbeatData {
                status: VaultHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "SSM Parameter Store metadata sample for prefix '{}' is reachable",
                        prefix
                    )),
                    stale: false,
                    partial: has_more_parameters,
                    collection_issues: vec![],
                },
                account_id: account_id.to_string(),
                region: region.to_string(),
                prefix: prefix.to_string(),
                parameter_metadata_sampled: true,
                sampled_parameter_count: Some(sampled_parameter_count),
                sampled_secure_string_count: Some(count_parameter_type(
                    &parameters,
                    "SecureString",
                )),
                sampled_string_count: Some(count_parameter_type(&parameters, "String")),
                sampled_string_list_count: Some(count_parameter_type(&parameters, "StringList")),
                sampled_advanced_tier_count: Some(count_parameter_tier(&parameters, "Advanced")),
                sampled_kms_key_metadata_present_count: Some(
                    parameters
                        .iter()
                        .filter(|parameter| parameter.key_id.is_some())
                        .count() as u32,
                ),
                latest_modified_at,
                has_more_parameters: Some(has_more_parameters),
                events: vec![],
            },
        )),
        raw: vec![],
    });
}

fn count_parameter_type(parameters: &[ParameterMetadata], parameter_type: &str) -> u32 {
    parameters
        .iter()
        .filter(|parameter| parameter.parameter_type.as_deref() == Some(parameter_type))
        .count() as u32
}

fn count_parameter_tier(parameters: &[ParameterMetadata], tier: &str) -> u32 {
    parameters
        .iter()
        .filter(|parameter| parameter.tier.as_deref() == Some(tier))
        .count() as u32
}

fn latest_modified_at(parameters: &[ParameterMetadata]) -> Option<DateTime<Utc>> {
    parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .last_modified_date
                .and_then(aws_epoch_seconds_to_utc)
        })
        .max()
}

fn aws_epoch_seconds_to_utc(seconds: f64) -> Option<DateTime<Utc>> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    let secs = seconds.trunc() as i64;
    let nanos = (seconds.fract() * 1_000_000_000.0).round() as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos.min(999_999_999))
}
