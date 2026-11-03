use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::dynamodb::{
    attribute_types, billing_modes, key_types, table_status, AttributeDefinition,
    CreateTableRequest, DeleteTableRequest, DescribeContinuousBackupsRequest, DescribeTableRequest,
    DescribeTimeToLiveRequest, DynamoDbApi, KeySchemaElement, PointInTimeRecoverySpecification,
    TableDescription, Tag, TimeToLiveDescription, TimeToLiveSpecification,
    UpdateContinuousBackupsRequest, UpdateTimeToLiveRequest,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsDynamoDbKeySchemaElement, AwsDynamoDbKvHeartbeatData,
    HeartbeatBackend, HeartbeatCollectionIssue, HeartbeatCollectionIssueReason,
    HeartbeatIssueSeverity, InitialSetupAuthority, Kv, KvHeartbeatData, KvHeartbeatStatus,
    KvOutputs, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceLifecycle, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AwsKvController {
    /// The actual DynamoDB table name (includes stack name prefix).
    /// This is None until the table is created.
    pub(crate) table_name: Option<String>,
    /// The table ARN for outputs
    pub(crate) table_arn: Option<String>,
    /// The next update must try to turn on point-in-time recovery. Defaults to true so tables
    /// created before this field existed are checked on their first update.
    #[serde(default = "point_in_time_recovery_pending_default")]
    pub(crate) point_in_time_recovery_pending: bool,
    /// When IAM last denied turning on point-in-time recovery. Retries wait
    /// `POINT_IN_TIME_RECOVERY_DENIED_RETRY` after it.
    #[serde(default)]
    pub(crate) point_in_time_recovery_denied_at: Option<chrono::DateTime<Utc>>,
}

/// How long a denied attempt waits before the next one. The grant arrives with a setup update,
/// so retrying on every heartbeat would only repeat the denial.
const POINT_IN_TIME_RECOVERY_DENIED_RETRY: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// UpdateContinuousBackups calls a new table gets before its create finishes without it.
const POINT_IN_TIME_RECOVERY_ATTEMPTS: u32 = 12;

fn point_in_time_recovery_pending_default() -> bool {
    true
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
        let table_name = aws_prefixed_resource_name(&ctx.resource_prefix, &config.id);

        info!(id=%config.id, table_name=%table_name, "Creating DynamoDB table for KV store");

        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        // Create table with a simple key schema for KV store
        // pk (partition key) = hash bucket for load distribution
        // sk (sort key) = actual key for the KV operation
        let create_table_request = CreateTableRequest::builder()
            .table_name(table_name.clone())
            .key_schema(vec![
                KeySchemaElement::builder()
                    .attribute_name("pk".to_string())
                    .key_type(key_types::HASH.to_string())
                    .build(),
                KeySchemaElement::builder()
                    .attribute_name("sk".to_string())
                    .key_type(key_types::RANGE.to_string())
                    .build(),
            ])
            .attribute_definitions(vec![
                AttributeDefinition::builder()
                    .attribute_name("pk".to_string())
                    .attribute_type(attribute_types::STRING.to_string())
                    .build(),
                AttributeDefinition::builder()
                    .attribute_name("sk".to_string())
                    .attribute_type(attribute_types::STRING.to_string())
                    .build(),
            ])
            .billing_mode(billing_modes::PAY_PER_REQUEST.to_string())
            .tags(
                standard_resource_tags(ctx.resource_prefix, &config.id)
                    .into_iter()
                    .map(|(key, value)| Tag::builder().key(key).value(value).build())
                    .collect(),
            )
            .build();

        client
            .create_table(create_table_request)
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

        let describe_table_request = DescribeTableRequest::builder()
            .table_name(table_name.clone())
            .build();

        match client.describe_table(describe_table_request).await {
            Ok(output) => {
                let table = output.table;
                match table.table_status.as_deref() {
                    Some(table_status::ACTIVE) => {
                        info!(table_name=%table_name, "DynamoDB table is now active");
                        self.table_arn = table.table_arn;

                        // Enable TTL on the table
                        Ok(HandlerAction::Continue {
                            state: EnablingTtl,
                            suggested_delay: Some(Duration::from_secs(5)),
                        })
                    }
                    Some(table_status::CREATING) => {
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
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
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

        let ttl_spec = TimeToLiveSpecification::builder()
            .attribute_name("ttl".to_string())
            .enabled(true)
            .build();

        let update_ttl_request = UpdateTimeToLiveRequest::builder()
            .table_name(table_name.clone())
            .time_to_live_specification(ttl_spec)
            .build();

        client
            .update_time_to_live(update_ttl_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to enable TTL on DynamoDB table '{}'", table_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(table_name=%table_name, "TTL enabled successfully on DynamoDB table");

        Ok(HandlerAction::Continue {
            state: EnablingPointInTimeRecovery,
            suggested_delay: None,
        })
    }

    #[handler(
        state = EnablingPointInTimeRecovery,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn enable_point_in_time_recovery(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.clone().ok_or_else(|| {
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

        // A new table starts with point-in-time recovery off, so there is nothing to read first.
        if !self
            .turn_on_point_in_time_recovery(client.as_ref(), &table_name, &config.id)
            .await?
        {
            // DynamoDB can briefly answer TableNotFoundException for a table that just became
            // ACTIVE. Wait a little, then finish the create anyway: the table works without
            // point-in-time recovery, the heartbeat reports it off and the next update retries.
            if self._internal_stay_count.unwrap_or_default() + 1 < POINT_IN_TIME_RECOVERY_ATTEMPTS {
                debug!(table_name=%table_name, "Table not visible to UpdateContinuousBackups yet");
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(Duration::from_secs(5)),
                });
            }
            warn!(
                table_name=%table_name,
                "DynamoDB still does not find the table for UpdateContinuousBackups; \
                 the next update turns on point-in-time recovery"
            );
            self.point_in_time_recovery_pending = true;
        }

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
        let table_name = &self.table_name.clone().ok_or_else(|| {
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
        let describe_table_request = DescribeTableRequest::builder()
            .table_name(table_name.clone())
            .build();

        match client.describe_table(describe_table_request).await {
            Ok(output) => {
                let table = output.table;
                if table.table_status.as_deref() != Some(table_status::ACTIVE) {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Table status changed from Active to {:?}",
                            table.table_status
                        ),
                    }));
                }

                let (ttl_description, ttl_issue) = match client
                    .describe_time_to_live(
                        DescribeTimeToLiveRequest::builder()
                            .table_name(table_name.clone())
                            .build(),
                    )
                    .await
                {
                    Ok(output) => (output.time_to_live_description, None),
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

                let point_in_time_recovery_issue = self
                    .check_point_in_time_recovery(ctx, client.as_ref(), table_name, &config.id)
                    .await;

                emit_aws_dynamodb_kv_heartbeat(
                    ctx,
                    &config.id,
                    table,
                    ttl_description,
                    ttl_issue,
                    point_in_time_recovery_issue,
                );
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
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
    // KV has no mutable config fields. Update turns on point-in-time recovery when it is off,
    // and also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Table name not set in state".to_string(),
            })
        })?;

        if !may_manage_table(ctx, &config.id) {
            info!(id=%config.id, "AWS KV update: setup owns this table, nothing to change");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let aws_config = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(aws_config)
            .await?;

        let already_enabled = match client
            .describe_continuous_backups(
                DescribeContinuousBackupsRequest::builder()
                    .table_name(table_name.clone())
                    .build(),
            )
            .await
        {
            Ok(response) => response.point_in_time_recovery_enabled(),
            // Installs from before this check may lack the read grant. The update call below
            // is safe to repeat, so make it and let it report its own outcome.
            Err(e) if is_access_denied(&e) => false,
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to read point-in-time recovery status of DynamoDB table '{}'",
                        table_name
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };

        if already_enabled {
            self.point_in_time_recovery_pending = false;
            self.point_in_time_recovery_denied_at = None;
        } else if !self
            .turn_on_point_in_time_recovery(client.as_ref(), &table_name, &config.id)
            .await?
        {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Table no longer exists".to_string(),
            }));
        }

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

        let delete_table_request = DeleteTableRequest::builder()
            .table_name(table_name.clone())
            .build();

        match client.delete_table(delete_table_request).await {
            Ok(_) => {
                info!(table_name=%table_name, "DynamoDB table deletion initiated");
                Ok(HandlerAction::Continue {
                    state: WaitingForTableDeletion,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
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

        let describe_table_request = DescribeTableRequest::builder()
            .table_name(table_name.clone())
            .build();

        match client.describe_table(describe_table_request).await {
            Ok(output) => {
                let table = output.table;
                if table.table_status.as_deref() == Some(table_status::DELETING) {
                    debug!(table_name=%table_name, "DynamoDB table still deleting");
                    Ok(HandlerAction::Continue {
                        state: WaitingForTableDeletion,
                        suggested_delay: Some(Duration::from_secs(15)),
                    })
                } else {
                    warn!(table_name=%table_name, status=?table.table_status, "Unexpected table status during deletion");
                    Ok(HandlerAction::Continue {
                        state: WaitingForTableDeletion,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
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

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let config = ctx.desired_resource_config::<Kv>()?;
        Ok(self.table_name.is_some()
            && self.point_in_time_recovery_pending
            && may_manage_table(ctx, &config.id))
    }

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

    /// Turns on point-in-time recovery. A denied call is logged and recorded, and the table
    /// stays usable: installs whose management role predates this grant receive it with their
    /// next setup update, and the following deployment update tries again.
    ///
    /// Returns false when DynamoDB does not find the table, without recording an attempt.
    async fn turn_on_point_in_time_recovery(
        &mut self,
        client: &dyn DynamoDbApi,
        table_name: &str,
        resource_id: &str,
    ) -> Result<bool> {
        info!(table_name=%table_name, "Turning on point-in-time recovery for DynamoDB table");

        let result = client
            .update_continuous_backups(
                UpdateContinuousBackupsRequest::builder()
                    .table_name(table_name.to_string())
                    .point_in_time_recovery_specification(
                        PointInTimeRecoverySpecification::builder()
                            .point_in_time_recovery_enabled(true)
                            .build(),
                    )
                    .build(),
            )
            .await;

        if matches!(&result, Err(e) if matches!(e.error, Some(CloudClientErrorData::RemoteResourceNotFound { .. })))
        {
            return Ok(false);
        }

        // Attempted either way; Ready marks it pending again while the table reports it off.
        self.point_in_time_recovery_pending = false;
        match result {
            Ok(_) => {
                self.point_in_time_recovery_denied_at = None;
                info!(table_name=%table_name, "Point-in-time recovery is on for DynamoDB table");
                Ok(true)
            }
            Err(e) if is_access_denied(&e) => {
                self.point_in_time_recovery_denied_at = Some(Utc::now());
                warn!(
                    table_name=%table_name,
                    "Point-in-time recovery stays off for DynamoDB table: the management role \
                     is missing dynamodb:UpdateContinuousBackups. A setup update grants it."
                );
                Ok(true)
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to turn on point-in-time recovery for DynamoDB table '{}'",
                    table_name
                ),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }

    /// Reads point-in-time recovery for the heartbeat. Returns a warning when it is off or
    /// cannot be read. When it is off, marks the next update to turn it on, at most once per
    /// `POINT_IN_TIME_RECOVERY_DENIED_RETRY` after a denial.
    async fn check_point_in_time_recovery(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        client: &dyn DynamoDbApi,
        table_name: &str,
        resource_id: &str,
    ) -> Option<HeartbeatCollectionIssue> {
        let result = client
            .describe_continuous_backups(
                DescribeContinuousBackupsRequest::builder()
                    .table_name(table_name.to_string())
                    .build(),
            )
            .await;

        let enabled = matches!(&result, Ok(response) if response.point_in_time_recovery_enabled());
        if enabled {
            self.point_in_time_recovery_pending = false;
            self.point_in_time_recovery_denied_at = None;
            return None;
        }
        if result.is_ok() {
            self.point_in_time_recovery_pending =
                self.point_in_time_recovery_denied_at
                    .is_none_or(|denied_at| {
                        Utc::now() - denied_at >= POINT_IN_TIME_RECOVERY_DENIED_RETRY
                    });
        }

        let (reason, message) = match result {
            Ok(_) if !may_manage_table(ctx, resource_id) => (
                HeartbeatCollectionIssueReason::NotInstalled,
                format!(
                    "Point-in-time recovery is off for DynamoDB table '{}'. Setup owns this \
                     table; run a setup update to turn it on.",
                    table_name
                ),
            ),
            Ok(_) if self.point_in_time_recovery_denied_at.is_some() => (
                HeartbeatCollectionIssueReason::Forbidden,
                format!(
                    "Point-in-time recovery is off for DynamoDB table '{}': the management role \
                     is missing dynamodb:UpdateContinuousBackups. Run a setup update to grant it; \
                     the next deployment update then turns it on.",
                    table_name
                ),
            ),
            Ok(_) => (
                HeartbeatCollectionIssueReason::NotInstalled,
                format!(
                    "Point-in-time recovery is off for DynamoDB table '{}'. The next deployment \
                     update turns it on.",
                    table_name
                ),
            ),
            Err(e) if is_access_denied(&e) => (
                HeartbeatCollectionIssueReason::Forbidden,
                format!(
                    "Cannot read point-in-time recovery for DynamoDB table '{}': the management \
                     role is missing dynamodb:DescribeContinuousBackups. Run a setup update to \
                     grant it.",
                    table_name
                ),
            ),
            Err(e) => (
                HeartbeatCollectionIssueReason::CollectionFailed,
                format!(
                    "Failed to read point-in-time recovery for DynamoDB table '{}': {}",
                    table_name, e
                ),
            ),
        };
        debug!(table_name=%table_name, message=%message, "Point-in-time recovery is not on");

        Some(HeartbeatCollectionIssue {
            source: "point-in-time-recovery".to_string(),
            reason,
            severity: HeartbeatIssueSeverity::Warning,
            message,
        })
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
            point_in_time_recovery_pending: false,
            point_in_time_recovery_denied_at: None,
            _internal_stay_count: None,
        }
    }
}

/// Runtime may change the table unless setup created it. A direct setup creates even
/// setup-owned tables through this controller.
fn may_manage_table(ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
    ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup
        || !ctx
            .desired_stack
            .resources
            .get(resource_id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen)
}

fn is_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

fn emit_aws_dynamodb_kv_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    table: TableDescription,
    ttl_description: Option<TimeToLiveDescription>,
    ttl_issue: Option<HeartbeatCollectionIssue>,
    point_in_time_recovery_issue: Option<HeartbeatCollectionIssue>,
) {
    let table_name = table
        .table_name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());
    let table_status = table.table_status.clone();
    let item_count = nonnegative_i64_to_u64(table.item_count);
    let table_size_bytes = nonnegative_i64_to_u64(table.table_size_bytes);
    // A TTL read failure leaves the snapshot incomplete; a point-in-time recovery warning does not.
    let partial = ttl_issue.is_some();
    let collection_issues = ttl_issue
        .into_iter()
        .chain(point_in_time_recovery_issue)
        .collect::<Vec<_>>();

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
            region: region_from_table_arn(table.table_arn.as_deref()),
            table_arn: table.table_arn,
            table_status,
            billing_mode: table
                .billing_mode_summary
                .and_then(|summary| summary.billing_mode),
            key_schema: table
                .key_schema
                .unwrap_or_default()
                .into_iter()
                .map(|key| AwsDynamoDbKeySchemaElement {
                    attribute_name: key.attribute_name,
                    key_type: key.key_type,
                })
                .collect(),
            global_secondary_index_count: len_to_u32(&table.global_secondary_indexes),
            local_secondary_index_count: len_to_u32(&table.local_secondary_indexes),
            item_count,
            table_size_bytes,
            stream_enabled: table
                .stream_specification
                .as_ref()
                .and_then(|stream| stream.stream_enabled),
            stream_view_type: table
                .stream_specification
                .and_then(|stream| stream.stream_view_type),
            ttl_status: ttl_description
                .as_ref()
                .and_then(|ttl| ttl.time_to_live_status.clone()),
            ttl_attribute_name: ttl_description.and_then(|ttl| ttl.attribute_name),
            deletion_protection_enabled: table.deletion_protection_enabled,
            sse_status: table
                .sse_description
                .as_ref()
                .and_then(|sse| sse.status.clone()),
            sse_type: table.sse_description.and_then(|sse| sse.sse_type),
            table_class: table
                .table_class_summary
                .and_then(|summary| summary.table_class),
            replica_count: len_to_u32(&table.replicas),
            restore_in_progress: table
                .restore_summary
                .and_then(|summary| summary.restore_in_progress),
        })),
        raw: vec![],
    });
}

fn nonnegative_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn len_to_u32<T>(items: &Option<Vec<T>>) -> Option<u32> {
    items
        .as_ref()
        .and_then(|items| u32::try_from(items.len()).ok())
}

fn region_from_table_arn(table_arn: Option<&str>) -> Option<String> {
    table_arn
        .and_then(|arn| arn.split(':').nth(3))
        .filter(|region| !region.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use alien_aws_clients::dynamodb::{ContinuousBackupsResponse, MockDynamoDbApi};
    use alien_core::InitialSetupAuthority;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    const TABLE: &str = "test-cache";

    /// A DynamoDB table whose point-in-time recovery the management role may or may not change.
    #[derive(Default)]
    struct Table {
        point_in_time_recovery: bool,
        update_denied: bool,
        describe_denied: bool,
        /// UpdateContinuousBackups calls answered with TableNotFoundException before it works.
        not_found_answers: usize,
        updates: usize,
    }

    fn denied() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "Table".to_string(),
            resource_name: TABLE.to_string(),
        })
    }

    fn continuous_backups(enabled: bool) -> ContinuousBackupsResponse {
        serde_json::from_value(json!({"ContinuousBackupsDescription": {
            "ContinuousBackupsStatus": "ENABLED",
            "PointInTimeRecoveryDescription": {
                "PointInTimeRecoveryStatus": if enabled { "ENABLED" } else { "DISABLED" }
            }
        }}))
        .unwrap()
    }

    fn provider(table: Arc<Mutex<Table>>) -> Arc<MockPlatformServiceProvider> {
        let mut dynamodb = MockDynamoDbApi::new();
        dynamodb.expect_create_table().returning(|request| {
            assert_eq!(request.table_name, TABLE);
            Ok(serde_json::from_value(json!({"TableDescription": {}})).unwrap())
        });
        dynamodb.expect_describe_table().returning(|_| {
            Ok(serde_json::from_value(json!({"Table": {
                "TableName": TABLE,
                "TableStatus": "ACTIVE",
                "TableArn": format!("arn:aws:dynamodb:us-east-1:123456789012:table/{TABLE}"),
            }}))
            .unwrap())
        });
        dynamodb
            .expect_update_time_to_live()
            .returning(|_| Ok(serde_json::from_value(json!({})).unwrap()));
        dynamodb
            .expect_describe_time_to_live()
            .returning(|_| Ok(serde_json::from_value(json!({})).unwrap()));
        let reads = table.clone();
        dynamodb
            .expect_describe_continuous_backups()
            .returning(move |request| {
                assert_eq!(request.table_name, TABLE);
                let table = reads.lock().unwrap();
                if table.describe_denied {
                    return Err(denied());
                }
                Ok(continuous_backups(table.point_in_time_recovery))
            });
        dynamodb
            .expect_update_continuous_backups()
            .returning(move |request| {
                assert_eq!(request.table_name, TABLE);
                assert!(
                    request
                        .point_in_time_recovery_specification
                        .point_in_time_recovery_enabled
                );
                let mut table = table.lock().unwrap();
                if table.not_found_answers > 0 {
                    table.not_found_answers -= 1;
                    return Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Table".to_string(),
                            resource_name: TABLE.to_string(),
                        },
                    ));
                }
                table.updates += 1;
                if table.update_denied {
                    return Err(denied());
                }
                table.point_in_time_recovery = true;
                Ok(continuous_backups(true))
            });
        let dynamodb = Arc::new(dynamodb);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_dynamodb_client()
            .returning(move |_| Ok(dynamodb.clone()));
        Arc::new(provider)
    }

    async fn executor(
        controller: AwsKvController,
        table: Arc<Mutex<Table>>,
        lifecycle: ResourceLifecycle,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(Kv::new("cache".to_string()).build())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(lifecycle)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .service_provider(provider(table))
            .build()
            .await
            .unwrap()
    }

    /// Steps Ready once and returns the point-in-time recovery warning it reported, if any.
    async fn heartbeat_warning(executor: &mut SingleControllerExecutor) -> Option<String> {
        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let [heartbeat] = executor.last_heartbeats() else {
            panic!("Ready should emit one heartbeat");
        };
        let ResourceHeartbeatData::Kv(KvHeartbeatData::AwsDynamoDb(data)) = &heartbeat.data else {
            panic!("unexpected heartbeat data");
        };
        assert!(!data.status.partial);
        match data.status.collection_issues.as_slice() {
            [] => None,
            [issue] => {
                assert_eq!(issue.source, "point-in-time-recovery");
                assert_eq!(issue.severity, HeartbeatIssueSeverity::Warning);
                Some(format!("{:?}: {}", issue.reason, issue.message))
            }
            issues => panic!("unexpected issues: {issues:?}"),
        }
    }

    /// A Ready checkpoint saved before the point-in-time recovery fields existed.
    fn previous_ready_checkpoint() -> AwsKvController {
        let mut saved = serde_json::to_value(AwsKvController::mock_ready(TABLE)).unwrap();
        let fields = saved.as_object_mut().unwrap();
        assert!(fields.remove("pointInTimeRecoveryPending").is_some());
        assert!(fields.remove("pointInTimeRecoveryDeniedAt").is_some());
        serde_json::from_value(saved).unwrap()
    }

    #[tokio::test]
    async fn create_turns_on_point_in_time_recovery() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert!(!executor.needs_update().unwrap());
        assert_eq!(heartbeat_warning(&mut executor).await, None);
    }

    #[tokio::test]
    async fn create_waits_while_a_new_table_is_not_found() {
        let table = Arc::new(Mutex::new(Table {
            not_found_answers: 2,
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().not_found_answers, 0);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
    }

    #[tokio::test]
    async fn create_finishes_when_a_new_table_stays_not_found() {
        let table = Arc::new(Mutex::new(Table {
            not_found_answers: usize::MAX,
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 0);
        // The next update tries again.
        assert!(executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn denied_update_keeps_the_table_ready_and_reports_the_missing_action() {
        let table = Arc::new(Mutex::new(Table {
            update_denied: true,
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        // The attempt just ran, so this update round does not try again.
        assert!(!executor.needs_update().unwrap());

        assert_eq!(
            heartbeat_warning(&mut executor).await.as_deref(),
            Some(
                "Forbidden: Point-in-time recovery is off for DynamoDB table 'test-cache': the \
                 management role is missing dynamodb:UpdateContinuousBackups. Run a setup update \
                 to grant it; the next deployment update then turns it on."
            )
        );
        assert!(executor.outputs().is_some());
        // The heartbeat sees it off, but the retry waits out the backoff.
        assert!(!executor.needs_update().unwrap());
        assert_eq!(table.lock().unwrap().updates, 1);
    }

    /// A Ready checkpoint whose last attempt was denied `minutes_ago`.
    fn denied_checkpoint(minutes_ago: i64) -> AwsKvController {
        AwsKvController {
            point_in_time_recovery_denied_at: Some(
                Utc::now() - chrono::TimeDelta::minutes(minutes_ago),
            ),
            ..AwsKvController::mock_ready(TABLE)
        }
    }

    #[tokio::test]
    async fn denied_attempt_is_retried_after_the_backoff_once_the_grant_arrives() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            denied_checkpoint(61),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        // Still off: the warning names the missing action until the retry turns it on.
        assert!(heartbeat_warning(&mut executor)
            .await
            .unwrap()
            .starts_with("Forbidden: "));
        assert!(executor.needs_update().unwrap());
        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert_eq!(heartbeat_warning(&mut executor).await, None);
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn denied_update_flow_keeps_the_table_running_and_waits_before_retrying() {
        let table = Arc::new(Mutex::new(Table {
            update_denied: true,
            ..Default::default()
        }));
        let mut executor = executor(
            previous_ready_checkpoint(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(heartbeat_warning(&mut executor)
            .await
            .unwrap()
            .contains("missing dynamodb:UpdateContinuousBackups"));
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn denied_read_keeps_the_table_running_and_reports_the_missing_action() {
        let table = Arc::new(Mutex::new(Table {
            describe_denied: true,
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::mock_ready(TABLE),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert_eq!(
            heartbeat_warning(&mut executor).await.as_deref(),
            Some(
                "Forbidden: Cannot read point-in-time recovery for DynamoDB table 'test-cache': \
                 the management role is missing dynamodb:DescribeContinuousBackups. Run a setup \
                 update to grant it."
            )
        );
        // An unread status schedules nothing.
        assert!(!executor.needs_update().unwrap());

        // An update that cannot read the status still turns it on.
        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
    }

    #[tokio::test]
    async fn table_from_previous_version_turns_on_point_in_time_recovery_on_update() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            previous_ready_checkpoint(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(executor.needs_update().unwrap());
        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn update_leaves_point_in_time_recovery_alone_when_already_on() {
        let table = Arc::new(Mutex::new(Table {
            point_in_time_recovery: true,
            ..Default::default()
        }));
        let mut executor = executor(
            previous_ready_checkpoint(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 0);
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn setup_owned_table_is_reported_but_never_changed() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            previous_ready_checkpoint(),
            table.clone(),
            ResourceLifecycle::Frozen,
        )
        .await;

        assert!(!executor.needs_update().unwrap());
        assert_eq!(
            heartbeat_warning(&mut executor).await.as_deref(),
            Some(
                "NotInstalled: Point-in-time recovery is off for DynamoDB table 'test-cache'. \
                 Setup owns this table; run a setup update to turn it on."
            )
        );
        assert!(!executor.needs_update().unwrap());

        executor
            .update(Kv::new("cache".to_string()).build())
            .unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 0);
    }
}
