use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::remote_stack_management::management_permissions_revision;
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
use serde::{Deserialize, Serialize};

#[controller]
pub struct AwsKvController {
    /// The actual DynamoDB table name (includes stack name prefix).
    /// This is None until the table is created.
    pub(crate) table_name: Option<String>,
    /// The table ARN for outputs
    pub(crate) table_arn: Option<String>,
    /// Whether point-in-time recovery was on when last read. Tables created by earlier code
    /// have no value, and their next update turns it on.
    #[serde(default)]
    pub(crate) point_in_time_recovery_enabled: bool,
    /// The management role was denied turning on point-in-time recovery. Reported in the
    /// heartbeat, and keeps the table from asking for an update until setup may have granted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) point_in_time_recovery_denial: Option<PointInTimeRecoveryDenial>,
}

/// A denied UpdateContinuousBackups call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointInTimeRecoveryDenial {
    /// What AWS answered.
    pub message: String,
    /// Revision of the management permissions the stack asked for when it was denied. A
    /// release that changes them retries.
    pub management_permissions_revision: Option<String>,
    /// When it was denied. A CloudFormation or Terraform setup update that installs the grant
    /// does not reach the controller, so the table asks again on the next update at least a day
    /// later.
    pub denied_at: chrono::DateTime<Utc>,
    /// Denied during a setup run. A setup run asks again only for denials from outside one:
    /// within a setup run the grants stay the same, and a later setup run that changes them
    /// changes the management permissions revision.
    #[serde(default)]
    pub in_setup_run: bool,
}

/// How long after a denial the next update asks again.
const POINT_IN_TIME_RECOVERY_DENIAL_RECHECK: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// Times a table is polled, 10 s apart, while DynamoDB prepares point-in-time recovery.
const POINT_IN_TIME_RECOVERY_POLLS: u32 = 30;

/// Outcome of asking DynamoDB to turn on point-in-time recovery.
enum PointInTimeRecoveryRequest {
    Accepted,
    /// DynamoDB cannot turn it on yet. For a short time after a table becomes ACTIVE it
    /// answers `TableNotFoundException` or `ContinuousBackupsUnavailableException`.
    NotReady,
    /// IAM denied the call. Management roles installed before this grant lack it until their
    /// next setup update.
    Denied,
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
            state: EnablingBackups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = EnablingBackups,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn enabling_backups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // A new table starts with point-in-time recovery off, so there is nothing to read first.
        match self.request_point_in_time_recovery(ctx).await? {
            PointInTimeRecoveryRequest::Accepted => Ok(HandlerAction::Continue {
                state: WaitingForBackups,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
            PointInTimeRecoveryRequest::NotReady => Ok(HandlerAction::Stay {
                max_times: Some(POINT_IN_TIME_RECOVERY_POLLS),
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            // The table works without it, and the heartbeat reports the denial.
            PointInTimeRecoveryRequest::Denied => Ok(HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay: None,
            }),
        }
    }

    #[handler(
        state = WaitingForBackups,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_backups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.read_point_in_time_recovery(ctx).await? {
            Ok(HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Stay {
                max_times: Some(POINT_IN_TIME_RECOVERY_POLLS),
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
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
        let table_name = &self.table_name(&config.id)?;

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
                    .point_in_time_recovery_issue(ctx, client.as_ref(), table_name, &config.id)
                    .await?;

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
    // KV has no mutable fields. An update recovers RefreshFailed and turns on point-in-time
    // recovery when it is off, such as on tables created by earlier code.
    //
    // It does not read first: management roles from earlier releases lack the read grant when
    // heartbeats are off, and turning it on is idempotent.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if !self.should_request_point_in_time_recovery(ctx)? {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        Ok(HandlerAction::Continue {
            state: UpdatingBackups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingBackups,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_backups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.request_point_in_time_recovery(ctx).await? {
            PointInTimeRecoveryRequest::Accepted => Ok(HandlerAction::Continue {
                state: WaitingForUpdatedBackups,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
            PointInTimeRecoveryRequest::NotReady => Ok(HandlerAction::Stay {
                max_times: Some(POINT_IN_TIME_RECOVERY_POLLS),
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            PointInTimeRecoveryRequest::Denied => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
        }
    }

    #[handler(
        state = WaitingForUpdatedBackups,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_updated_backups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.read_point_in_time_recovery(ctx).await? {
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Stay {
                max_times: Some(POINT_IN_TIME_RECOVERY_POLLS),
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    // With point-in-time recovery on, DynamoDB keeps a system backup of a deleted table for 35
    // days at no extra cost, and refuses to delete it earlier:
    // https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/PointInTimeRecovery_Howitworks.html#howitworks-pitr-deleting-table

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
        self.should_request_point_in_time_recovery(ctx)
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
            point_in_time_recovery_enabled: true,
            point_in_time_recovery_denial: None,
            _internal_stay_count: None,
        }
    }

    /// Tables runtime manages ask for point-in-time recovery while it is off. After a denial
    /// they wait for a setup run that did not record it, a release that changes the management
    /// permissions, or an update at least a day later, so a role without the grant is not asked
    /// again on every deployment step.
    fn should_request_point_in_time_recovery(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        if self.point_in_time_recovery_enabled || !may_manage_table(ctx, ctx.desired_config.id()) {
            return Ok(false);
        }
        match &self.point_in_time_recovery_denial {
            None => Ok(true),
            Some(denial) => Ok((ctx.deployment_config.setup_run && !denial.in_setup_run)
                || Utc::now() - denial.denied_at >= POINT_IN_TIME_RECOVERY_DENIAL_RECHECK
                || management_permissions_revision(ctx)? != denial.management_permissions_revision),
        }
    }

    fn table_name(&self, resource_id: &str) -> Result<String> {
        self.table_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(resource_id.to_string()),
                message: "Table name not set in state".to_string(),
            })
        })
    }

    /// Asks DynamoDB to turn on point-in-time recovery. A denial is recorded and logged with
    /// AWS's answer; any other error fails the step.
    async fn request_point_in_time_recovery(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<PointInTimeRecoveryRequest> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name(&config.id)?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(ctx.get_aws_config()?)
            .await?;

        info!(table_name=%table_name, "Turning on point-in-time recovery for DynamoDB table");
        let result = client
            .update_continuous_backups(
                UpdateContinuousBackupsRequest::builder()
                    .table_name(table_name.clone())
                    .point_in_time_recovery_specification(
                        PointInTimeRecoverySpecification::builder()
                            .point_in_time_recovery_enabled(true)
                            .build(),
                    )
                    .build(),
            )
            .await;

        match result {
            Ok(_) => Ok(PointInTimeRecoveryRequest::Accepted),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) || dynamodb_error_code(&e).as_deref()
                    == Some("ContinuousBackupsUnavailableException") =>
            {
                debug!(table_name=%table_name, "DynamoDB is not ready to turn on point-in-time recovery yet");
                Ok(PointInTimeRecoveryRequest::NotReady)
            }
            Err(e) => match access_denied_message(&e) {
                Some(message) => {
                    warn!(
                        table_name=%table_name,
                        aws_message=%message,
                        "Point-in-time recovery stays off for DynamoDB table: the management role \
                         is missing dynamodb:UpdateContinuousBackups. A setup update grants it."
                    );
                    self.point_in_time_recovery_denial = Some(PointInTimeRecoveryDenial {
                        message,
                        management_permissions_revision: management_permissions_revision(ctx)?,
                        denied_at: Utc::now(),
                        in_setup_run: ctx.deployment_config.setup_run,
                    });
                    Ok(PointInTimeRecoveryRequest::Denied)
                }
                None => Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to turn on point-in-time recovery for DynamoDB table '{}'",
                        table_name
                    ),
                    resource_id: Some(config.id.clone()),
                })),
            },
        }
    }

    /// Reads whether point-in-time recovery is on and records it.
    async fn read_point_in_time_recovery(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let config = ctx.desired_resource_config::<Kv>()?;
        let table_name = self.table_name(&config.id)?;
        let client = ctx
            .service_provider
            .get_aws_dynamodb_client(ctx.get_aws_config()?)
            .await?;
        let status = client
            .describe_continuous_backups(
                DescribeContinuousBackupsRequest::builder()
                    .table_name(table_name.clone())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read point-in-time recovery of DynamoDB table '{}'",
                    table_name
                ),
                resource_id: Some(config.id.clone()),
            })?;
        self.record_point_in_time_recovery(status.point_in_time_recovery_enabled());
        Ok(self.point_in_time_recovery_enabled)
    }

    fn record_point_in_time_recovery(&mut self, enabled: bool) {
        self.point_in_time_recovery_enabled = enabled;
        if enabled {
            self.point_in_time_recovery_denial = None;
        }
    }

    /// Reads point-in-time recovery for the heartbeat and returns a warning while it is off.
    /// The heartbeat never changes it; the next update turns it on.
    ///
    /// A denied read is an error: the read grant has been in the heartbeat permissions from the
    /// start, so every management role has it. Other read errors only warn, as for TTL.
    async fn point_in_time_recovery_issue(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        client: &dyn DynamoDbApi,
        table_name: &str,
        resource_id: &str,
    ) -> Result<Option<HeartbeatCollectionIssue>> {
        let status = client
            .describe_continuous_backups(
                DescribeContinuousBackupsRequest::builder()
                    .table_name(table_name.to_string())
                    .build(),
            )
            .await;

        let (reason, message) = match status {
            Err(e) if access_denied_message(&e).is_some() => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to read point-in-time recovery of DynamoDB table '{}'",
                        table_name
                    ),
                    resource_id: Some(resource_id.to_string()),
                }))
            }
            Err(e) => (
                HeartbeatCollectionIssueReason::CollectionFailed,
                format!(
                    "Failed to read point-in-time recovery for DynamoDB table '{}': {}",
                    table_name, e
                ),
            ),
            Ok(status) => {
                self.record_point_in_time_recovery(status.point_in_time_recovery_enabled());
                if self.point_in_time_recovery_enabled {
                    return Ok(None);
                }
                match &self.point_in_time_recovery_denial {
                    _ if !may_manage_table(ctx, resource_id) => (
                        HeartbeatCollectionIssueReason::NotInstalled,
                        format!(
                            "Point-in-time recovery is off for DynamoDB table '{}'. Setup owns \
                             this table; run a setup update to turn it on.",
                            table_name
                        ),
                    ),
                    Some(denial) => (
                        HeartbeatCollectionIssueReason::Forbidden,
                        format!(
                            "Point-in-time recovery is off for DynamoDB table '{}': the \
                             management role is missing dynamodb:UpdateContinuousBackups. Run a \
                             setup update to grant it. AWS said: {}",
                            table_name, denial.message
                        ),
                    ),
                    None => (
                        HeartbeatCollectionIssueReason::NotInstalled,
                        format!(
                            "Point-in-time recovery is off for DynamoDB table '{}'; the next \
                             update turns it on.",
                            table_name
                        ),
                    ),
                }
            }
        };

        Ok(Some(HeartbeatCollectionIssue {
            source: "point-in-time-recovery".to_string(),
            reason,
            severity: HeartbeatIssueSeverity::Warning,
            message,
        }))
    }
}

/// Runtime may change the table unless setup owns it. Setup-owned tables change only during a
/// direct setup run, which creates and updates them through this controller.
fn may_manage_table(ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
    let setup_owned = ctx
        .desired_stack
        .resources
        .get(resource_id)
        .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen);
    !setup_owned
        || (ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup
            && ctx.deployment_config.setup_run)
}

/// DynamoDB's error code and message from the response body in the error chain.
fn dynamodb_error(error: &AlienError<CloudClientErrorData>) -> Option<(String, Option<String>)> {
    let contexts = std::iter::once(error.context.as_ref()).chain(
        std::iter::successors(error.source.as_deref(), |source| source.source.as_deref())
            .map(|source| source.context.as_ref()),
    );
    contexts.flatten().find_map(|context| {
        let body = context.get("http_response_text")?.as_str()?;
        let body: serde_json::Value = serde_json::from_str(body).ok()?;
        let code = body.get("__type")?.as_str()?;
        let message = body
            .get("Message")
            .or_else(|| body.get("message"))
            .and_then(serde_json::Value::as_str);
        Some((
            code.rsplit('#').next().unwrap_or(code).to_string(),
            message.map(str::to_string),
        ))
    })
}

fn dynamodb_error_code(error: &AlienError<CloudClientErrorData>) -> Option<String> {
    dynamodb_error(error).map(|(code, _)| code)
}

/// AWS's message when IAM denied the call. The client reports expired or unknown credentials
/// as access denied too; those are not a missing grant, so this looks for DynamoDB's
/// `AccessDeniedException` in the response body.
fn access_denied_message(error: &AlienError<CloudClientErrorData>) -> Option<String> {
    match dynamodb_error(error)? {
        (code, message) if code == "AccessDeniedException" => {
            Some(message.unwrap_or_else(|| "access denied".to_string()))
        }
        _ => None,
    }
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
    use alien_aws_clients::dynamodb::{ContinuousBackupsResponse, DynamoDbClient, MockDynamoDbApi};
    use alien_aws_clients::{
        AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
    };
    use alien_core::InitialSetupAuthority;
    use httpmock::prelude::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const TABLE: &str = "test-cache";

    /// Recorded from DynamoDB for a management role without the grant (account id replaced).
    const ACCESS_DENIED: &str = r#"{"__type":"com.amazon.coral.service#AccessDeniedException","Message":"User: arn:aws:sts::123456789012:assumed-role/test-management/session is not authorized to perform: dynamodb:UpdateContinuousBackups on resource: arn:aws:dynamodb:us-east-1:123456789012:table/test-cache because no identity-based policy allows the dynamodb:UpdateContinuousBackups action"}"#;
    const AWS_MESSAGE: &str = "User: arn:aws:sts::123456789012:assumed-role/test-management/session is not authorized to perform: dynamodb:UpdateContinuousBackups on resource: arn:aws:dynamodb:us-east-1:123456789012:table/test-cache because no identity-based policy allows the dynamodb:UpdateContinuousBackups action";
    const DESCRIBE_DENIED: &str = r#"{"__type":"com.amazon.coral.service#AccessDeniedException","Message":"User: arn:aws:sts::123456789012:assumed-role/test-management/session is not authorized to perform: dynamodb:DescribeContinuousBackups on resource: arn:aws:dynamodb:us-east-1:123456789012:table/test-cache because no identity-based policy allows the dynamodb:DescribeContinuousBackups action"}"#;
    const TABLE_NOT_FOUND: &str = r#"{"__type":"com.amazonaws.dynamodb.v20120810#TableNotFoundException","message":"Table not found: test-cache"}"#;
    /// Seen live right after a table became ACTIVE.
    const BACKUPS_BEING_ENABLED: &str = r#"{"__type":"com.amazonaws.dynamodb.v20120810#ContinuousBackupsUnavailableException","message":"Backups are being enabled for the table: test-cache. Please retry later"}"#;
    const VALIDATION_FAILED: &str = r#"{"__type":"com.amazon.coral.validate#ValidationException","message":"1 validation error detected"}"#;
    const EXPIRED_TOKEN: &str = r#"{"__type":"com.amazon.coral.service#ExpiredTokenException","message":"The security token included in the request is expired"}"#;

    /// The error the real DynamoDB client returns when DynamoDB answers `body` with HTTP 400.
    async fn client_error(target: &str, body: &str) -> AlienError<CloudClientErrorData> {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", format!("DynamoDB_20120810.{target}"));
                then.status(400)
                    .header("content-type", "application/x-amz-json-1.0")
                    .body(body);
            })
            .await;
        let client = DynamoDbClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(AwsClientConfig {
                account_id: "123456789012".into(),
                region: "us-east-1".into(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test-access-key".into(),
                    secret_access_key: "test-secret-key".into(),
                    session_token: None,
                },
                service_overrides: Some(ServiceOverrides {
                    endpoints: HashMap::from([("dynamodb".into(), server.base_url())]),
                }),
            }),
        );
        let table_name = TABLE.to_string();
        let result = match target {
            "UpdateContinuousBackups" => {
                client
                    .update_continuous_backups(
                        UpdateContinuousBackupsRequest::builder()
                            .table_name(table_name)
                            .point_in_time_recovery_specification(
                                PointInTimeRecoverySpecification::builder()
                                    .point_in_time_recovery_enabled(true)
                                    .build(),
                            )
                            .build(),
                    )
                    .await
            }
            "DescribeContinuousBackups" => {
                client
                    .describe_continuous_backups(
                        DescribeContinuousBackupsRequest::builder()
                            .table_name(table_name)
                            .build(),
                    )
                    .await
            }
            other => panic!("unexpected target {other}"),
        };
        result.unwrap_err()
    }

    /// A DynamoDB table whose point-in-time recovery the management role may or may not change.
    #[derive(Default)]
    struct Table {
        point_in_time_recovery: bool,
        /// Errors the real client returned, answered by UpdateContinuousBackups in order before
        /// it works.
        update_errors: Vec<AlienError<CloudClientErrorData>>,
        describe_error: Option<AlienError<CloudClientErrorData>>,
        updates: usize,
        deleted: bool,
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
        let tables = table.clone();
        dynamodb.expect_describe_table().returning(move |_| {
            if tables.lock().unwrap().deleted {
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Table".to_string(),
                        resource_name: TABLE.to_string(),
                    },
                ));
            }
            Ok(serde_json::from_value(json!({"Table": {
                "TableName": TABLE,
                "TableStatus": "ACTIVE",
                "TableArn": format!("arn:aws:dynamodb:us-east-1:123456789012:table/{TABLE}"),
            }}))
            .unwrap())
        });
        let deletes = table.clone();
        dynamodb.expect_delete_table().returning(move |_| {
            deletes.lock().unwrap().deleted = true;
            Ok(serde_json::from_value(json!({"TableDescription": {}})).unwrap())
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
                if let Some(error) = &table.describe_error {
                    return Err(error.clone());
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
                table.updates += 1;
                if !table.update_errors.is_empty() {
                    return Err(table.update_errors.remove(0));
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
            .setup_run(false)
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

    fn denied_warning() -> String {
        format!(
            "Forbidden: Point-in-time recovery is off for DynamoDB table 'test-cache': the \
             management role is missing dynamodb:UpdateContinuousBackups. Run a setup update to \
             grant it. AWS said: {AWS_MESSAGE}"
        )
    }

    /// A Ready controller as the previous version saved it, before it recorded point-in-time
    /// recovery.
    fn saved_by_previous_version() -> AwsKvController {
        let mut value = serde_json::to_value(AwsKvController::mock_ready(TABLE)).unwrap();
        let object = value.as_object_mut().unwrap();
        assert!(object.remove("pointInTimeRecoveryEnabled").is_some());
        assert!(!object.contains_key("pointInTimeRecoveryDenial"));
        serde_json::from_value(value).unwrap()
    }

    fn saved(executor: &SingleControllerExecutor) -> AwsKvController {
        executor
            .internal_state::<AwsKvController>()
            .unwrap()
            .clone()
    }

    /// Whether a deployment step updates the table with its config unchanged. Only the
    /// executor's plan asks `needs_update`; a Running deployment that only refreshes never does.
    fn plans_update(executor: &SingleControllerExecutor) -> bool {
        let (mut stack, client_config, deployment_config, authority) =
            executor.with_context(|ctx| {
                (
                    ctx.desired_stack.clone(),
                    ctx.client_config.clone(),
                    ctx.deployment_config.clone(),
                    ctx.initial_setup_authority,
                )
            });
        // The test executor's management resource is a stand-in that a setup run would always
        // update, and its update would carry the table along. Only the table's own answer counts.
        stack
            .resources
            .get_mut("cache")
            .unwrap()
            .dependencies
            .clear();
        let stack_executor = crate::core::StackExecutor::builder(&stack, client_config)
            .deployment_config(&deployment_config)
            .initial_setup_authority(authority)
            .build()
            .unwrap();
        let plan = stack_executor.plan(executor.stack_state()).unwrap();
        plan.updates.contains_key("cache")
    }

    /// What the deployment executor does when `needs_update` holds: an update to the same config.
    async fn update(executor: &mut SingleControllerExecutor) -> Result<()> {
        executor.update(Kv::new("cache".to_string()).build())?;
        executor.run_until_terminal().await
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
        assert!(saved(&executor).point_in_time_recovery_enabled);
        assert!(!plans_update(&executor));
        assert_eq!(heartbeat_warning(&mut executor).await, None);
        assert_eq!(table.lock().unwrap().updates, 1);
    }

    #[tokio::test]
    async fn create_waits_while_a_new_table_is_not_ready() {
        let not_found = client_error("UpdateContinuousBackups", TABLE_NOT_FOUND).await;
        let being_enabled = client_error("UpdateContinuousBackups", BACKUPS_BEING_ENABLED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![not_found, being_enabled],
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
        assert_eq!(table.lock().unwrap().updates, 3);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert!(saved(&executor).point_in_time_recovery_enabled);
    }

    #[tokio::test]
    async fn create_fails_when_a_new_table_never_becomes_ready() {
        let not_found = client_error("UpdateContinuousBackups", TABLE_NOT_FOUND).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![not_found; POINT_IN_TIME_RECOVERY_POLLS as usize],
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        let error = executor.run_until_terminal().await.unwrap_err();

        assert_eq!(error.code, "RESOURCE_POLLING_TIMEOUT");
        assert_eq!(
            table.lock().unwrap().updates,
            POINT_IN_TIME_RECOVERY_POLLS as usize
        );
        assert!(!table.lock().unwrap().point_in_time_recovery);
    }

    #[tokio::test]
    async fn denied_create_keeps_the_table_running_and_warns() {
        let denied = client_error("UpdateContinuousBackups", ACCESS_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![denied],
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
        assert!(executor.outputs().is_some());
        assert_eq!(table.lock().unwrap().updates, 1);
        let denial = saved(&executor).point_in_time_recovery_denial.unwrap();
        assert_eq!(denial.message, AWS_MESSAGE);
        assert_eq!(
            denial.management_permissions_revision,
            executor
                .with_context(management_permissions_revision)
                .unwrap()
        );
        assert!(Utc::now() - denial.denied_at < chrono::TimeDelta::minutes(1));

        // The heartbeat names the missing action and never calls it; nothing asks for an update.
        assert_eq!(
            heartbeat_warning(&mut executor).await,
            Some(denied_warning())
        );
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(!plans_update(&executor));
    }

    #[tokio::test]
    async fn table_from_previous_version_gets_point_in_time_recovery_on_its_next_update() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(plans_update(&executor));
        // The heartbeat only reads.
        assert_eq!(
            heartbeat_warning(&mut executor).await.as_deref(),
            Some(
                "NotInstalled: Point-in-time recovery is off for DynamoDB table 'test-cache'; \
                 the next update turns it on."
            )
        );
        assert_eq!(table.lock().unwrap().updates, 0);

        update(&mut executor).await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert!(!plans_update(&executor));
        assert_eq!(heartbeat_warning(&mut executor).await, None);
    }

    #[tokio::test]
    async fn update_of_a_table_already_on_only_confirms_it() {
        let table = Arc::new(Mutex::new(Table {
            point_in_time_recovery: true,
            ..Default::default()
        }));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert!(!plans_update(&executor));
    }

    /// Management roles from earlier releases may lack the read grant too when heartbeats are
    /// off. The update never reads before asking.
    #[tokio::test]
    async fn previous_role_without_the_read_grant_keeps_the_table_running() {
        let read_denied = client_error("DescribeContinuousBackups", DESCRIBE_DENIED).await;
        let denied = client_error("UpdateContinuousBackups", ACCESS_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![denied],
            describe_error: Some(read_denied),
            ..Default::default()
        }));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        let denial = saved(&executor).point_in_time_recovery_denial.unwrap();
        assert_eq!(denial.message, AWS_MESSAGE);
        assert!(!plans_update(&executor));
    }

    /// Every step of a setup run plans again. A denial from the same setup run is not asked
    /// again, since its grants do not change within it.
    #[tokio::test]
    async fn a_setup_run_does_not_ask_again_for_its_own_denial() {
        let denied = client_error("UpdateContinuousBackups", ACCESS_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![denied],
            ..Default::default()
        }));
        let mut executor = SingleControllerExecutor::builder()
            .resource(Kv::new("cache".to_string()).build())
            .controller(saved_by_previous_version())
            .platform(Platform::Aws)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .setup_run(true)
            .service_provider(provider(table.clone()))
            .build()
            .await
            .unwrap();

        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(
            saved(&executor)
                .point_in_time_recovery_denial
                .unwrap()
                .in_setup_run
        );
        assert!(!plans_update(&executor));
        assert_eq!(table.lock().unwrap().updates, 1);
    }

    #[tokio::test]
    async fn previous_role_keeps_the_table_running_until_a_setup_run_grants_it() {
        let denied = client_error("UpdateContinuousBackups", ACCESS_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![denied],
            ..Default::default()
        }));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 1);
        assert_eq!(
            heartbeat_warning(&mut executor).await,
            Some(denied_warning())
        );
        // After the denied update, the same config has nothing to do.
        assert!(!plans_update(&executor));

        // A setup run asks again, and the role now has the grant.
        let mut executor = SingleControllerExecutor::builder()
            .resource(Kv::new("cache".to_string()).build())
            .controller(saved(&executor))
            .platform(Platform::Aws)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .setup_run(true)
            .service_provider(provider(table.clone()))
            .build()
            .await
            .unwrap();
        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 2);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert_eq!(saved(&executor).point_in_time_recovery_denial, None);
        assert!(!plans_update(&executor));

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn a_release_with_other_management_permissions_asks_again_after_a_denial() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut controller = saved_by_previous_version();
        controller.point_in_time_recovery_denial = Some(PointInTimeRecoveryDenial {
            message: AWS_MESSAGE.to_string(),
            management_permissions_revision: Some("earlier-release".to_string()),
            denied_at: Utc::now(),
            in_setup_run: false,
        });
        let executor = executor(controller, table, ResourceLifecycle::Live).await;

        assert!(plans_update(&executor));
    }

    /// A CloudFormation or Terraform setup update installs the grant without reaching the
    /// controller, so the next update at least a day after a denial asks again, and then turns
    /// it on.
    #[tokio::test]
    async fn the_next_update_a_day_after_a_denial_asks_again() {
        let denied = client_error("UpdateContinuousBackups", ACCESS_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![denied],
            ..Default::default()
        }));
        let mut first = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;
        update(&mut first).await.unwrap();
        assert!(!plans_update(&first));

        let denied_before = |age: chrono::TimeDelta| {
            let mut controller = saved(&first);
            controller
                .point_in_time_recovery_denial
                .as_mut()
                .unwrap()
                .denied_at -= age;
            controller
        };
        let earlier = executor(
            denied_before(POINT_IN_TIME_RECOVERY_DENIAL_RECHECK - chrono::TimeDelta::minutes(5)),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;
        assert!(!plans_update(&earlier));

        let mut executor = executor(
            denied_before(POINT_IN_TIME_RECOVERY_DENIAL_RECHECK),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        assert!(plans_update(&executor));
        update(&mut executor).await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 2);
        assert!(table.lock().unwrap().point_in_time_recovery);
        assert_eq!(saved(&executor).point_in_time_recovery_denial, None);
        assert!(!plans_update(&executor));
    }

    #[tokio::test]
    async fn expired_credentials_fail_the_update_instead_of_reading_as_a_missing_grant() {
        let expired = client_error("UpdateContinuousBackups", EXPIRED_TOKEN).await;
        // The client maps expired credentials to the same access-denied error.
        assert!(matches!(
            expired.error,
            Some(CloudClientErrorData::RemoteAccessDenied { .. })
        ));
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![expired],
            ..Default::default()
        }));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        let error = update(&mut executor).await.unwrap_err();

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert_eq!(saved(&executor).point_in_time_recovery_denial, None);
    }

    #[tokio::test]
    async fn a_rejected_request_fails_the_create() {
        let rejected = client_error("UpdateContinuousBackups", VALIDATION_FAILED).await;
        let table = Arc::new(Mutex::new(Table {
            update_errors: vec![rejected],
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::default(),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        let error = executor.run_until_terminal().await.unwrap_err();

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(
            error.message.contains(
                "Failed to turn on point-in-time recovery for DynamoDB table 'test-cache'"
            ),
            "{}",
            error.message
        );
        assert_eq!(executor.status(), ResourceStatus::Provisioning);
        assert!(!table.lock().unwrap().point_in_time_recovery);
    }

    #[tokio::test]
    async fn denied_read_fails_the_heartbeat() {
        // Every management role has had this read since the start, so a denial is a real fault.
        let denied = client_error("DescribeContinuousBackups", DESCRIBE_DENIED).await;
        let table = Arc::new(Mutex::new(Table {
            describe_error: Some(denied),
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::mock_ready(TABLE),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        let error = executor.step().await.unwrap_err();

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(
            error
                .message
                .contains("Failed to read point-in-time recovery of DynamoDB table 'test-cache'"),
            "{}",
            error.message
        );
        assert_eq!(table.lock().unwrap().updates, 0);
    }

    fn service_unavailable() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteServiceUnavailable {
            message: "We encountered an internal error. Please try again.".to_string(),
        })
    }

    #[tokio::test]
    async fn failed_read_keeps_the_table_running_and_warns() {
        let table = Arc::new(Mutex::new(Table {
            describe_error: Some(service_unavailable()),
            ..Default::default()
        }));
        let mut executor = executor(
            AwsKvController::mock_ready(TABLE),
            table.clone(),
            ResourceLifecycle::Live,
        )
        .await;

        let warning = heartbeat_warning(&mut executor).await.unwrap();
        assert!(
            warning.starts_with(
                "CollectionFailed: Failed to read point-in-time recovery for DynamoDB table \
                 'test-cache': "
            ),
            "{warning}"
        );
        assert_eq!(table.lock().unwrap().updates, 0);
        assert!(!plans_update(&executor));
    }

    /// A direct setup owns a Frozen table too, but changes it only during a setup run.
    #[tokio::test]
    async fn only_a_setup_run_updates_a_table_a_direct_setup_owns() {
        for setup_run in [false, true] {
            let executor = SingleControllerExecutor::builder()
                .resource(Kv::new("cache".to_string()).build())
                .controller(saved_by_previous_version())
                .platform(Platform::Aws)
                .resource_lifecycle(ResourceLifecycle::Frozen)
                .initial_setup_authority(InitialSetupAuthority::DirectSetup)
                .setup_run(setup_run)
                .service_provider(provider(Arc::new(Mutex::new(Table::default()))))
                .build()
                .await
                .unwrap();

            assert_eq!(plans_update(&executor), setup_run);
        }
    }

    #[tokio::test]
    async fn setup_owned_table_is_reported_but_never_changed() {
        let table = Arc::new(Mutex::new(Table::default()));
        let mut executor = executor(
            saved_by_previous_version(),
            table.clone(),
            ResourceLifecycle::Frozen,
        )
        .await;

        assert!(!plans_update(&executor));
        assert_eq!(
            heartbeat_warning(&mut executor).await.as_deref(),
            Some(
                "NotInstalled: Point-in-time recovery is off for DynamoDB table 'test-cache'. \
                 Setup owns this table; run a setup update to turn it on."
            )
        );
        assert!(!plans_update(&executor));
        update(&mut executor).await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(table.lock().unwrap().updates, 0);
    }
}
