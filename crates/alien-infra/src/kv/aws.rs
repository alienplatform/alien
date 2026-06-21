use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::aws_sdk::{DynamoDbTableDescription, DynamoDbTtlDescription};
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    standard_resource_tags, AwsDynamoDbKeySchemaElement, AwsDynamoDbKvHeartbeatData,
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, Kv, KvHeartbeatData, KvHeartbeatStatus, KvOutputs, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Generates the full, prefixed AWS DynamoDB table name.
fn get_aws_table_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct AwsKvController {
    /// The actual DynamoDB table name (includes stack name prefix).
    /// This is None until the table is created.
    pub(crate) table_name: Option<String>,
    /// The table ARN for outputs
    pub(crate) table_arn: Option<String>,
}

#[controller]
impl AwsKvController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let aws_config = ctx.get_aws_config()?;
        let table_name = get_aws_table_name(&ctx.resource_prefix, &config.id);

        info!(id=%config.id, table_name=%table_name, "Creating DynamoDB table for KV store");

        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        client
            .create_kv_table(
                &table_name,
                standard_resource_tags(ctx.resource_prefix, &config.id),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create DynamoDB table '{}'", table_name),
                resource_id: Some(config.id.clone()),
            })?;

        self.table_name = Some(table_name.clone());
        info!(table_name=%table_name, "DynamoDB table creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForTableCreation,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForTableCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_table_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        debug!(table_name=%table_name, "Checking DynamoDB table status");

        match client.describe_table(table_name).await {
            Ok(Some(table)) => {
                match table.table_status().map(|status| status.as_str()) {
                    Some("ACTIVE") => {
                        info!(table_name=%table_name, "DynamoDB table is now active");
                        self.table_arn = table.table_arn().map(ToString::to_string);

                        // Enable TTL on the table
                        Ok(HandlerAction::Continue {
                            state: EnablingTtl,
                            suggested_delay: Some(Duration::from_secs(5)),
                        })
                    }
                    Some("CREATING") => {
                        debug!(table_name=%table_name, "DynamoDB table still creating");
                        Ok(HandlerAction::Continue {
                            state: WaitingForTableCreation,
                            suggested_delay: Some(Duration::from_secs(15)),
                        })
                    }
                    Some(status) => {
                        warn!(table_name=%table_name, status=?status, "Unexpected table status");
                        Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "DynamoDB table '{}' has unexpected status: {:?}",
                                table_name, status
                            ),
                            resource_id: Some(config.id.clone()),
                        }))
                    }
                    None => {
                        debug!(table_name=%table_name, "Table status not available yet");
                        Ok(HandlerAction::Continue {
                            state: WaitingForTableCreation,
                            suggested_delay: Some(Duration::from_secs(10)),
                        })
                    }
                }
            }
            Ok(None) => {
                debug!(table_name=%table_name, "Table not found yet, continuing to wait");
                Ok(HandlerAction::Continue {
                    state: WaitingForTableCreation,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to describe DynamoDB table '{}'", table_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = EnablingTtl,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn enable_ttl(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        info!(table_name=%table_name, "Enabling TTL on DynamoDB table");

        client
            .enable_ttl(table_name, "ttl")
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to enable TTL on DynamoDB table '{}'", table_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(table_name=%table_name, "TTL enabled successfully on DynamoDB table");

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
        let config = ctx.desired_resource_config::<Kv>()?;

        info!(kv=%config.id, "Applying resource-scoped permissions for DynamoDB table");

        if let Some(table_name) = &self.table_name {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx, &config.id, table_name, "kv",
            )
            .await?;
        }

        info!(kv=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        // Heartbeat check: verify table still exists and is active
        match client.describe_table(table_name).await {
            Ok(Some(table)) => {
                if table.table_status().map(|status| status.as_str()) != Some("ACTIVE") {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Table status changed from Active to {:?}",
                            table.table_status()
                        ),
                    }));
                }

                let (ttl_description, ttl_issue) = match client.describe_ttl(table_name).await {
                    Ok(ttl_description) => (ttl_description, None),
                    Err(e) => (
                        None,
                        Some(HeartbeatCollectionIssue {
                            source: "ttl".to_string(),
                            reason: HeartbeatCollectionIssueReason::CollectionFailed,
                            severity: HeartbeatIssueSeverity::Warning,
                            message: format!(
                                "Failed to describe DynamoDB TTL metadata for table '{}': {}",
                                table_name, e
                            ),
                        }),
                    ),
                };

                emit_aws_dynamodb_kv_heartbeat(ctx, &config.id, table, ttl_description, ttl_issue);
            }
            Ok(None) => {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: "Table no longer exists".to_string(),
                }));
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to describe DynamoDB table '{}' during heartbeat",
                        table_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // KV has no mutable fields — update is a no-op that also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        info!(id=%config.id, "AWS KV update (no-op — no mutable fields)");
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
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = match self.table_name.as_ref() {
            Some(name) => name,
            None => {
                // No table was ever created (e.g. deleted from Provisioning state before
                // CreateStart completed). Nothing to clean up.
                info!(id=%config.id, "DynamoDB table name not set -- nothing to delete");
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(table_name=%table_name, "Deleting DynamoDB table");

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        match client.delete_table(table_name).await {
            Ok(true) => {
                info!(table_name=%table_name, "DynamoDB table deletion initiated");
                Ok(HandlerAction::Continue {
                    state: WaitingForTableDeletion,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Ok(false) => {
                info!(table_name=%table_name, "DynamoDB table already deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete DynamoDB table '{}'", table_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForTableDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_table_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        debug!(table_name=%table_name, "Checking DynamoDB table deletion status");

        match client.describe_table(table_name).await {
            Ok(Some(table)) => {
                if table.table_status().map(|status| status.as_str()) == Some("DELETING") {
                    debug!(table_name=%table_name, "DynamoDB table still deleting");
                    Ok(HandlerAction::Continue {
                        state: WaitingForTableDeletion,
                        suggested_delay: Some(Duration::from_secs(15)),
                    })
                } else {
                    warn!(table_name=%table_name, status=?table.table_status(), "Unexpected table status during deletion");
                    Ok(HandlerAction::Continue {
                        state: WaitingForTableDeletion,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Ok(None) => {
                info!(table_name=%table_name, "DynamoDB table successfully deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check deletion status for DynamoDB table '{}'",
                    table_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────

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
        if let (Some(table_name), Some(table_arn)) = (&self.table_name, &self.table_arn) {
            Some(ResourceOutputs::new(KvOutputs {
                store_name: table_name.clone(),
                identifier: Some(table_arn.clone()),
                endpoint: None, // DynamoDB uses regional endpoints
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, KvBinding};

        if let (Some(table_name), Some(table_arn)) = (&self.table_name, &self.table_arn) {
            // Extract region from ARN (format: arn:aws:dynamodb:REGION:ACCOUNT:table/TABLE_NAME)
            let region = table_arn
                .split(':')
                .nth(3)
                .unwrap_or("us-east-1") // Fallback to default region
                .to_string();

            let binding = KvBinding::dynamodb(
                BindingValue::value(table_name.clone()),
                BindingValue::value(region),
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
impl AwsKvController {
    fn clear_state(&mut self) {
        self.table_name = None;
        self.table_arn = None;
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(table_name: &str) -> Self {
        Self {
            state: AwsKvState::Ready,
            table_name: Some(table_name.to_string()),
            table_arn: Some(format!(
                "arn:aws:dynamodb:us-east-1:123456789012:table/{}",
                table_name
            )),
            _internal_stay_count: None,
        }
    }
}

fn emit_aws_dynamodb_kv_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    table: DynamoDbTableDescription,
    ttl_description: Option<DynamoDbTtlDescription>,
    ttl_issue: Option<HeartbeatCollectionIssue>,
) {
    let table_name = table
        .table_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| resource_id.to_string());
    let table_arn = table.table_arn().map(ToString::to_string);
    let table_status = table
        .table_status()
        .map(|status| status.as_str().to_string());
    let billing_mode = table
        .billing_mode_summary()
        .and_then(|summary| summary.billing_mode())
        .map(|mode| mode.as_str().to_string());
    let item_count = nonnegative_i64_to_u64(table.item_count());
    let table_size_bytes = nonnegative_i64_to_u64(table.table_size_bytes());
    let stream_enabled = table
        .stream_specification()
        .map(|stream| stream.stream_enabled());
    let stream_view_type = table
        .stream_specification()
        .and_then(|stream| stream.stream_view_type())
        .map(|stream_type| stream_type.as_str().to_string());
    let sse_status = table
        .sse_description()
        .and_then(|sse| sse.status())
        .map(|status| status.as_str().to_string());
    let sse_type = table
        .sse_description()
        .and_then(|sse| sse.sse_type())
        .map(|sse_type| sse_type.as_str().to_string());
    let table_class = table
        .table_class_summary()
        .and_then(|summary| summary.table_class())
        .map(|table_class| table_class.as_str().to_string());
    let ttl_status = ttl_description
        .as_ref()
        .and_then(|ttl| ttl.time_to_live_status())
        .map(|status| status.as_str().to_string());
    let ttl_attribute_name = ttl_description
        .as_ref()
        .and_then(|ttl| ttl.attribute_name())
        .map(ToString::to_string);
    let collection_issues = ttl_issue.into_iter().collect::<Vec<_>>();
    let partial = !collection_issues.is_empty();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Kv::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Kv(KvHeartbeatData::AwsDynamoDb(AwsDynamoDbKvHeartbeatData {
            status: KvHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: table_status
                    .as_ref()
                    .map(|status| format!("DynamoDB table status is {}", status)),
                stale: false,
                partial,
                collection_issues,
            },
            name: table_name,
            region: region_from_table_arn(table_arn.as_deref()),
            table_arn,
            table_status,
            billing_mode,
            key_schema: table
                .key_schema()
                .iter()
                .map(|key| AwsDynamoDbKeySchemaElement {
                    attribute_name: key.attribute_name().to_string(),
                    key_type: key.key_type().as_str().to_string(),
                })
                .collect(),
            global_secondary_index_count: usize_to_u32(table.global_secondary_indexes().len()),
            local_secondary_index_count: usize_to_u32(table.local_secondary_indexes().len()),
            item_count,
            table_size_bytes,
            stream_enabled,
            stream_view_type,
            ttl_status,
            ttl_attribute_name,
            deletion_protection_enabled: table.deletion_protection_enabled(),
            sse_status,
            sse_type,
            table_class,
            replica_count: usize_to_u32(table.replicas().len()),
            restore_in_progress: table
                .restore_summary()
                .map(|summary| summary.restore_in_progress()),
        })),
        raw: vec![],
    });
}

fn nonnegative_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn usize_to_u32(value: usize) -> Option<u32> {
    u32::try_from(value).ok()
}

fn region_from_table_arn(table_arn: Option<&str>) -> Option<String> {
    table_arn
        .and_then(|arn| arn.split(':').nth(3))
        .filter(|region| !region.is_empty())
        .map(ToString::to_string)
}
