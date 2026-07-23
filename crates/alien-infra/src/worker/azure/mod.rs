use alien_azure_clients::container_apps::{
    ManagedEnvironmentCertificate, ManagedEnvironmentCertificateKeyVaultProperties,
    ManagedEnvironmentCertificateProperties,
};
use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::container_apps::{
    Configuration, ConfigurationActiveRevisionsMode, Container, ContainerApp,
    ContainerAppProperties, ContainerAppPropertiesProvisioningState, ContainerResources,
    CustomDomain, CustomDomainBindingType, EnvironmentVar, IdentitySettings,
    IdentitySettingsLifecycle, IngressTransport, RegistryCredentials, Scale, Secret, Template,
    TrafficWeight,
};
use alien_azure_clients::AzureClientConfig;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureContainerAppsWorkerHeartbeatData, CertificateStatus, DnsRecordStatus, HeartbeatBackend,
    ObservedHealth, Platform, ProviderLifecycleState, RemoteStackManagement,
    RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceRef, ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs,
    WorkloadHeartbeatStatus, ENV_AZURE_CLIENT_ID,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use base64::Engine;
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::core::EnvironmentVariableBuilder;
use crate::core::{ResourceController, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_name, get_container_apps_environment_outputs,
    get_resource_group_name, is_azure_authorization_propagation_error,
};
use crate::worker::azure_dapr_components::{
    delete_owned_legacy_dapr_components, ensure_dapr_component, service_bus_dapr_component,
    DaprComponentEnsureOperation, LegacyDaprComponentCleanupStep, TrackedDaprComponentDeleteStep,
};
use crate::worker::azure_dapr_names_migration::{
    DaprComponentMigrationStep, CURRENT_DAPR_COMPONENT_NAMING_VERSION,
};
use crate::worker::azure_names::{
    commands_queue_name, get_azure_blob_trigger_dapr_component_name, get_azure_dapr_component_name,
    get_azure_internal_commands_dapr_component_name, get_azure_queue_trigger_dapr_component_name,
    get_azure_storage_event_subscription_name, get_legacy_azure_blob_trigger_dapr_component_names,
    get_legacy_azure_internal_commands_dapr_component_names,
    get_legacy_azure_queue_trigger_dapr_component_names,
};
use crate::worker::readiness_probe::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_macros::controller;

#[path = "../azure_cleanup.rs"]
mod cleanup;
use cleanup::{AzureCommandsQueueTarget, CommandsQueueTargetPreparation};
#[path = "../azure_command_sender.rs"]
mod command_sender;
use command_sender::{AzureCommandsSenderRoleAssignmentIntent, CommandsSenderReconcileResult};
#[path = "../azure_operations.rs"]
mod operations;
use operations::{
    poll_pending_operation, poll_reconciled_operation, AzureOperationPoll,
    AzureOperationPollRequest, AzureStrictOperationPoll,
};
#[path = "../azure_role_assignments.rs"]
mod role_assignments;
#[path = "../azure_trigger_targets.rs"]
mod trigger_targets;
use trigger_targets::{StorageDeliveryReconcileResult, StorageTargetPreparation};

mod helpers;
mod support;

use support::*;

pub use support::AzureStorageTriggerInfrastructure;

// ≡ Controller definition =======================================================
#[controller]
pub struct AzureWorkerController {
    // ─────────── Persisted fields ───────────
    /// Azure Container App name. Filled on *create* and reused for update/delete.
    pub(crate) container_app_name: Option<String>,

    /// Resource ID of the Container App (ARM ID).
    pub(crate) resource_id: Option<String>,

    /// Public URL (if `Ingress::Public`).
    pub(crate) url: Option<String>,

    /// The Container App's own ingress host (`*.azurecontainerapps.io`). `url` may be overridden to
    /// the public display FQDN (from `public_urls`), but DNS records must target THIS host:
    /// targeting the public FQDN makes the CNAME self-referential (target == record name) and the
    /// DNS provider rejects it as a loop. See `build_outputs`.
    pub(crate) container_app_url: Option<String>,

    /// URL returned by Azure ARM for *current* long‑running operation.
    pub(crate) pending_operation_url: Option<String>,
    /// Retry‑after seconds for the current LRO (populated when Azure returns it).
    pub(crate) pending_operation_retry_after: Option<u64>,
    /// Dapr component names for all worker triggers.
    pub(crate) dapr_components: Vec<String>,
    /// Event Grid and Service Bus resources created for storage triggers.
    #[serde(default)]
    pub(crate) storage_trigger_infrastructure: Vec<AzureStorageTriggerInfrastructure>,
    /// Next durable resource deletion within the first tracked storage trigger.
    #[serde(default)]
    pub(crate) storage_trigger_teardown_progress: AzureStorageTriggerTeardownProgress,

    // Domain & Certificate
    /// The fully qualified domain name for the worker
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The Azure Key Vault certificate ID
    pub(crate) keyvault_cert_id: Option<String>,
    /// The Azure Container Apps managed environment certificate resource ID.
    pub(crate) container_apps_certificate_id: Option<String>,
    /// Whether this worker uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // Commands infrastructure
    /// Service Bus resource group used for commands delivery.
    #[serde(default)]
    pub(crate) commands_resource_group_name: Option<String>,
    /// Service Bus namespace name for commands delivery
    pub(crate) commands_namespace_name: Option<String>,
    /// Service Bus queue name for commands delivery
    pub(crate) commands_queue_name: Option<String>,
    /// Whether the tracked commands queue has been applied in the current setup cycle.
    #[serde(default)]
    pub(crate) commands_queue_applied: bool,
    /// Dapr component name for commands queue
    pub(crate) commands_dapr_component: Option<String>,
    /// Current and historical command Dapr names still requiring ownership-aware teardown.
    #[serde(default)]
    pub(crate) commands_dapr_component_deletion_candidates: Vec<String>,
    /// Role assignment ID for Service Bus Data Sender on the deploying identity (for cleanup)
    pub(crate) commands_sender_role_assignment_id: Option<String>,
    /// Durable direct-manager sender grant planned before its idempotent Azure PUT.
    #[serde(default)]
    pub(crate) commands_sender_role_assignment_intent:
        Option<AzureCommandsSenderRoleAssignmentIntent>,
    /// Whether the exact commands queue has been inspected for controller-owned sender grants.
    #[serde(default)]
    pub(crate) commands_sender_role_assignment_discovery_complete: bool,
    /// Legacy setup-owned receiver cursor. It is ignored and never remotely deleted.
    pub(crate) commands_receiver_role_assignment_id: Option<String>,

    /// Deadline for retrying commands infrastructure creation while Azure IAM grants propagate.
    #[serde(default)]
    pub(crate) commands_infrastructure_auth_wait_until_epoch_secs: Option<u64>,
    /// Deadline for retrying Container Apps Environment operations while Azure wakes an idle environment.
    #[serde(default)]
    pub(crate) container_apps_environment_wake_wait_until_epoch_secs: Option<u64>,
    /// Next time the controller should retry a Container Apps Environment operation after an idle wake response.
    #[serde(default)]
    pub(crate) container_apps_environment_wake_retry_after_epoch_secs: Option<u64>,
    /// Deadline before creating the Container App after pre-created RBAC assignments.
    #[serde(default)]
    pub(crate) pre_container_app_rbac_wait_until_epoch_secs: Option<u64>,
    /// Deadline before reporting Ready after all consumer-visible permissions were applied.
    #[serde(default)]
    pub(crate) ready_rbac_wait_until_epoch_secs: Option<u64>,
    /// Whether the current update flow changed the workload and should wait for RBAC propagation.
    #[serde(default)]
    pub(crate) update_rbac_wait_required: bool,
    /// Whether the current update flow has already deleted old Dapr trigger components.
    #[serde(default)]
    pub(crate) update_dapr_components_deleted: bool,
    /// Version of the deterministic Dapr component naming scheme applied to this worker.
    #[serde(default)]
    pub(crate) dapr_component_naming_version: u8,
    /// Trigger component whose asynchronous deletion is currently being polled.
    #[serde(default)]
    pub(crate) pending_dapr_component_deletion_name: Option<String>,
    /// Whether delete has persisted the complete current and historical Dapr cleanup plan.
    #[serde(default)]
    pub(crate) dapr_component_deletion_candidates_initialized: bool,
    /// Whether imported auxiliary command/storage cleanup candidates have been reconstructed.
    #[serde(default)]
    pub(crate) auxiliary_teardown_candidates_initialized: bool,
    /// Whether a commands-only update teardown has reconstructed imported cleanup cursors.
    #[serde(default)]
    pub(crate) commands_update_teardown_candidates_initialized: bool,
    /// Whether trigger update teardown has reconstructed candidates from the previous config.
    #[serde(default)]
    pub(crate) trigger_update_teardown_candidates_initialized: bool,
    /// Whether this update durably invalidated storage delivery verification latches.
    #[serde(default)]
    pub(crate) storage_delivery_update_reconciliation_initialized: bool,
}

#[controller]
impl AzureWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if self.container_app_name.is_none() {
            self.pre_container_app_rbac_wait_until_epoch_secs = None;
            self.ready_rbac_wait_until_epoch_secs = None;
            self.commands_infrastructure_auth_wait_until_epoch_secs = None;
            self.container_apps_environment_wake_wait_until_epoch_secs = None;
            self.container_apps_environment_wake_retry_after_epoch_secs = None;
            self.update_rbac_wait_required = false;
        }

        // Product limitation: Only allow at most one queue trigger per worker
        let queue_trigger_count = func_cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    func_cfg.id, queue_trigger_count
                ),
                resource_id: Some(func_cfg.id.clone()),
            }));
        }

        // Derive deterministic resource names.
        let container_app_name = get_azure_container_app_name(ctx.resource_prefix, &func_cfg.id);

        // Pre-create commands infrastructure BEFORE the Container App so the Dapr
        // sidecar starts with the component already defined AND the RBAC roles already
        // assigned (giving them time to propagate during Container App creation ~30s).
        // This eliminates the race condition where the sidecar starts before permissions exist.
        self.container_app_name = Some(container_app_name.clone());
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "commands infrastructure")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        info!(name=%func_cfg.id, "Initiating Azure Container App worker creation");
        if func_cfg.commands_enabled {
            match self
                .setup_commands_infrastructure(ctx, azure_cfg, func_cfg, &container_app_name)
                .await
            {
                Ok(CommandsSetupOperation::Completed) => {
                    self.commands_infrastructure_auth_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_retry_after_epoch_secs = None;
                }
                Ok(CommandsSetupOperation::Creating(delay)) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateCommandsDaprComponentOperation,
                        suggested_delay: Some(delay),
                    });
                }
                Ok(CommandsSetupOperation::Deleting(delay)) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateDaprComponentDeletion,
                        suggested_delay: Some(delay),
                    });
                }
                Ok(CommandsSetupOperation::Pending(delay)) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(e) if is_azure_authorization_propagation_error(&e) => {
                    let deadline = ensure_rbac_wait_deadline(
                        &mut self.commands_infrastructure_auth_wait_until_epoch_secs,
                        AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS,
                    );

                    if let Some(delay) = rbac_wait_delay(deadline) {
                        warn!(
                            name=%func_cfg.id,
                            remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                            error=%e,
                            "Azure authorization is not ready for commands infrastructure; retrying"
                        );
                        return Ok(HandlerAction::Stay {
                            max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                            suggested_delay: Some(delay),
                        });
                    }

                    return Err(e);
                }
                Err(e) if is_azure_container_apps_environment_waking_error(&e) => {
                    let deadline = ensure_rbac_wait_deadline(
                        &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                        AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                    );

                    if let Some(delay) = self.record_container_apps_environment_wake_retry(deadline)
                    {
                        warn!(
                            name=%func_cfg.id,
                            remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                            error=%e,
                            "Azure Container Apps Environment is waking; retrying commands infrastructure"
                        );
                        return Ok(HandlerAction::Stay {
                            max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                            suggested_delay: Some(delay),
                        });
                    }

                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }

        // Pre-create every trigger input component before the Container App. The
        // Dapr sidecar loads scoped components when the app revision starts; adding
        // queue or cron components only after that revision is running leaves the
        // input binding inactive. Storage triggers additionally create Event Grid ->
        // a dedicated Service Bus queue, and their receiver role can propagate while
        // the app starts.
        let mut cron_index = 0usize;
        for trigger in &func_cfg.triggers {
            let operation = match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    self.create_dapr_service_bus_component(
                        ctx,
                        &container_app_name,
                        func_cfg,
                        queue,
                    )
                    .await?
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    self.create_azure_storage_trigger(
                        ctx,
                        &container_app_name,
                        func_cfg,
                        storage,
                        events,
                    )
                    .await?
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    let operation = self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            func_cfg,
                            cron,
                            cron_index,
                        )
                        .await?;
                    cron_index += 1;
                    operation
                }
            };

            match operation {
                DaprComponentOperation::Completed => {}
                DaprComponentOperation::Creating(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateCommandsDaprComponentOperation,
                        suggested_delay: Some(delay),
                    });
                }
                DaprComponentOperation::Deleting(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateDaprComponentDeletion,
                        suggested_delay: Some(delay),
                    });
                }
                DaprComponentOperation::Pending(delay) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
            }
        }

        // Wait in a real controller state for Azure RBAC propagation. A
        // suggested delay alone is only a scheduling hint and can be shortened
        // by other resources in the executor.
        info!(name=%func_cfg.id, "Trigger infrastructure ready, waiting for RBAC propagation before creating Container App");
        Ok(HandlerAction::Continue {
            state: WaitingBeforeContainerAppCreation,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForPreCreateCommandsDaprComponentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_pre_create_commands_dapr_component_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for pre-created Dapr component"
                    .to_string(),
                operation: Some(
                    "waiting_for_pre_create_commands_dapr_component_operation".to_string(),
                ),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let azure_cfg = ctx.get_azure_config()?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        let status = operation_client
            .check_status(&lro, "CreateOrUpdateDaprComponent", &func_cfg.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for pre-created Dapr component".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = WaitingForPreCreateDaprComponentDeletion,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_pre_create_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_pre_create_dapr_component_deletion",
                "Azure ARM operation failed for pre-create Dapr deletion",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = WaitingBeforeContainerAppCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_before_container_app_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let deadline = ensure_rbac_wait_deadline(
            &mut self.pre_container_app_rbac_wait_until_epoch_secs,
            AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS,
        );

        if let Some(delay) = rbac_wait_delay(deadline) {
            info!(
                name=%func_cfg.id,
                remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure RBAC propagation before creating Container App"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.pre_container_app_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: CreatingContainerAppResource,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingContainerAppResource,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_container_app_resource(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self
            .container_app_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set".to_string(),
                })
            })?
            .clone();
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;

        // Build ARM request body.
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let container_app = self
            .build_container_app(
                func_cfg,
                &environment_name,
                &container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;

        // Fire the CREATE/UPDATE call.
        let op_result = client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        match op_result {
            OperationResult::Completed(immediate_app) => {
                info!(name=%container_app_name, "Container app creation completed immediately");
                self.handle_creation_completed(ctx, &immediate_app);

                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForCreateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
    }

    #[handler(
        state = WaitingForCreateOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let operation_url = match &self.pending_operation_url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: "No pending operation URL recorded in WaitingForCreateOperation"
                        .to_string(),
                    operation: Some("waiting_for_create_operation".to_string()),
                    resource_id: Some(ctx.desired_resource_config::<Worker>()?.id.clone()),
                }));
            }
        };

        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();

        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let lro = LongRunningOperation {
            url: operation_url.clone(),
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        // Poll ARM operation.
        let op_status = operation_client
            .check_status(&lro, "CreateContainerApp", container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if op_status.is_some() {
            info!(name=%container_app_name, "LRO completed – checking resource status");
            Ok(HandlerAction::Continue {
                state: CreatingContainerApp,
                suggested_delay: None,
            })
        } else {
            // Still running – schedule another poll.
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = CreatingContainerApp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let resource_group_name = get_resource_group_name(ctx.state)?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, container_app_name)
            .await
        {
            Ok(app) => {
                if let Some(props) = &app.properties {
                    match props.provisioning_state.as_ref() {
                        Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                            info!(name=%container_app_name, "Provisioning succeeded – configuring Dapr components");
                            self.handle_creation_completed(ctx, &app);

                            // Branch based on ingress type
                            // If public, resolve domain and proceed to certificate flow
                            // If private, skip directly to Dapr component configuration
                            if !func_cfg.public_endpoints.is_empty() {
                                match Self::resolve_domain_info(ctx, &func_cfg.id) {
                                    Ok(domain_info) => {
                                        info!(fqdn=%domain_info.fqdn, "Resolved domain for public worker");
                                        self.fqdn = Some(domain_info.fqdn);
                                        self.certificate_id = domain_info.certificate_id;
                                        self.keyvault_cert_id = domain_info.keyvault_cert_id;
                                        self.container_apps_certificate_id =
                                            domain_info.container_apps_certificate_id;
                                        self.uses_custom_domain = domain_info.uses_custom_domain;

                                        // Proceed to certificate flow
                                        return Ok(HandlerAction::Continue {
                                            state: WaitingForCertificate,
                                            suggested_delay: None,
                                        });
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to resolve domain info, skipping custom domain setup: {}",
                                            e
                                        );
                                        // Continue without custom domain
                                    }
                                }
                            }

                            Ok(HandlerAction::Continue {
                                state: ConfiguringDaprComponents,
                                suggested_delay: None,
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                            debug!(name=%container_app_name, "Provisioning still in progress");
                            Ok(HandlerAction::Stay {
                                max_times: Some(60),
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::Failed) => {
                            error!(name=%container_app_name, "Container app provisioning failed");
                            Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Container app provisioning failed".to_string(),
                                resource_id: Some(func_cfg.id.clone()),
                            }))
                        }
                        _ => Ok(HandlerAction::Stay {
                            max_times: Some(60),
                            suggested_delay: Some(Duration::from_secs(10)),
                        }),
                    }
                } else {
                    debug!(name=%container_app_name, "Properties missing – retry");
                    Ok(HandlerAction::Stay {
                        max_times: Some(60),
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
                debug!(name=%container_app_name, "Resource not yet visible – retry");
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.certificate_status);
        if !self.ensure_domain_info(ctx, &worker_config.id)? {
            return Ok(HandlerAction::Continue {
                state: ConfiguringDaprComponents,
                suggested_delay: None,
            });
        }
        if self.uses_custom_domain && self.keyvault_cert_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: ConfiguringCustomDomain,
                suggested_delay: None,
            });
        }

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = ImportingCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        self.ensure_domain_info(ctx, &worker_config.id)?;
        let azure_cfg = ctx.get_azure_config()?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        // Convert PEM to PKCS#12 for Azure Key Vault
        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);

        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let location = azure_cfg.region.as_deref().unwrap_or("East US").to_string();
        let certificate = ManagedEnvironmentCertificate {
            location,
            properties: Some(ManagedEnvironmentCertificateProperties {
                value: Some(pkcs12_base64),
                password: Some(String::new()),
                certificate_key_vault_properties: None,
            }),
            tags: HashMap::new(),
        };

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let response = container_apps_client
            .create_or_update_managed_environment_certificate(
                &resource_group_name,
                &environment_name,
                &certificate_name,
                &certificate,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to Azure Container Apps Environment"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.container_apps_certificate_id = Some(response.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Azure Container Apps Environment certificate response missing ID"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?);

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            worker=%worker_config.id,
            cert_id=?self.container_apps_certificate_id,
            "Certificate imported to Azure Container Apps Environment"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringCustomDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_custom_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set".to_string(),
            })
        })?;

        let fqdn = self.fqdn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "FQDN not set".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        let environment_name = get_container_apps_environment_name(ctx.state)?;
        if self.container_apps_certificate_id.is_none() {
            let keyvault_cert_id = self.keyvault_cert_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker_config.id.clone(),
                    message: "Container Apps certificate ID not set".to_string(),
                })
            })?;
            let rsm_ref = ResourceRef::new(
                RemoteStackManagement::RESOURCE_TYPE,
                "remote-stack-management",
            );
            let rsm_outputs = ctx
                .state
                .get_resource_outputs::<RemoteStackManagementOutputs>(rsm_ref.id())
                .context(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: rsm_ref.id().to_string(),
                })?;
            let certificate_name =
                get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
            let certificate = ManagedEnvironmentCertificate {
                location: azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
                properties: Some(ManagedEnvironmentCertificateProperties {
                    value: None,
                    password: None,
                    certificate_key_vault_properties: Some(
                        ManagedEnvironmentCertificateKeyVaultProperties {
                            identity: rsm_outputs.management_resource_id.clone(),
                            key_vault_url: keyvault_cert_id.clone(),
                        },
                    ),
                }),
                tags: HashMap::new(),
            };
            let response = client
                .create_or_update_managed_environment_certificate(
                    &resource_group_name,
                    &environment_name,
                    &certificate_name,
                    &certificate,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message:
                        "Failed to import Key Vault certificate to Azure Container Apps Environment"
                            .to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?;
            self.container_apps_certificate_id = Some(response.id.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Azure Container Apps Environment certificate response missing ID"
                        .to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?);
        }
        let container_apps_certificate_id =
            self.container_apps_certificate_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker_config.id.clone(),
                    message: "Container Apps certificate ID not set".to_string(),
                })
            })?;

        let mut app = self
            .build_container_app(
                worker_config,
                &environment_name,
                container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;
        Self::set_custom_domain(
            &mut app,
            fqdn.clone(),
            container_apps_certificate_id.clone(),
        );

        // Update the container app with custom domain
        let _operation = client
            .create_or_update_container_app(&resource_group_name, container_app_name, &app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to configure custom domain for container app".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        info!(
            worker=%worker_config.id,
            fqdn=%fqdn,
            "Custom domain configured for container app"
        );

        Ok(HandlerAction::Continue {
            state: ConfiguringDaprComponents,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(
                    worker=%worker_config.id,
                    fqdn=%self.fqdn.as_ref().unwrap_or(&"unknown".to_string()),
                    "DNS record created successfully"
                );
                if self.container_apps_certificate_id.is_some() {
                    return Ok(HandlerAction::Continue {
                        state: ConfiguringCustomDomain,
                        suggested_delay: None,
                    });
                }
                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
                    suggested_delay: None,
                })
            }
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = ConfiguringDaprComponents,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "Dapr component creation")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        info!(name=%container_app_name, "Configuring Dapr components for triggers");

        // Create Dapr components for all trigger types
        let mut created_any = false;
        let mut cron_index = 0usize;
        for trigger in &func_cfg.triggers {
            match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    info!(worker=%func_cfg.id, queue=%queue.id, "Creating Dapr Service Bus component");
                    let operation = match self
                        .create_dapr_service_bus_component(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            queue,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    created_any = true;
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    info!(worker=%func_cfg.id, storage=%storage.id, "Creating Azure storage trigger delivery");
                    let operation = match self
                        .create_azure_storage_trigger(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            storage,
                            events,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    created_any = true;
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    info!(worker=%func_cfg.id, cron=%cron, "Creating Dapr cron component");
                    let operation = match self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            cron,
                            cron_index,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    cron_index += 1;
                    created_any = true;
                }
            }
        }

        if !created_any {
            info!(worker=%func_cfg.id, "No triggers found, skipping Dapr component creation");
        }
        self.container_apps_environment_wake_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_retry_after_epoch_secs = None;

        // Go to commands infrastructure next
        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForDaprComponentCreateOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dapr_component_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for Dapr component".to_string(),
                operation: Some("waiting_for_dapr_component_create_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let azure_cfg = ctx.get_azure_config()?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        let status = operation_client
            .check_status(&lro, "CreateOrUpdateDaprComponent", &func_cfg.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for Dapr component creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: ConfiguringDaprComponents,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = WaitingForLegacyDaprComponentDeletionDuringCreate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_legacy_dapr_component_deletion_during_create(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_legacy_dapr_component_deletion_during_create",
                "Azure ARM operation failed for legacy Dapr component deletion during create",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: ConfiguringDaprComponents,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = CreatingCommandsInfrastructure,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "commands Dapr component")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        if !func_cfg.commands_enabled {
            debug!(worker=%func_cfg.id, "Commands not enabled, skipping commands infrastructure");
            return Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            });
        }

        let azure_config = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;
        match self
            .setup_commands_infrastructure(ctx, azure_config, func_cfg, &container_app_name)
            .await
        {
            Ok(CommandsSetupOperation::Completed) => Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            }),
            Ok(CommandsSetupOperation::Creating(delay)) => Ok(HandlerAction::Continue {
                state: WaitingForCommandsDaprComponentOperation,
                suggested_delay: Some(delay),
            }),
            Ok(CommandsSetupOperation::Deleting(delay)) => Ok(HandlerAction::Continue {
                state: WaitingForLegacyCommandsDaprComponentDeletionDuringCreate,
                suggested_delay: Some(delay),
            }),
            Ok(CommandsSetupOperation::Pending(delay)) => Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            }),
            Err(error) if is_azure_authorization_propagation_error(&error) => {
                let deadline = ensure_rbac_wait_deadline(
                    &mut self.commands_infrastructure_auth_wait_until_epoch_secs,
                    AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS,
                );
                if let Some(delay) = rbac_wait_delay(deadline) {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(error)
            }
            Err(error) if is_azure_container_apps_environment_waking_error(&error) => {
                let deadline = ensure_rbac_wait_deadline(
                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                );
                if let Some(delay) = self.record_container_apps_environment_wake_retry(deadline) {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    #[handler(
        state = WaitingForCommandsDaprComponentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_commands_dapr_component_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for commands Dapr component"
                    .to_string(),
                operation: Some("waiting_for_commands_dapr_component_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let azure_cfg = ctx.get_azure_config()?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        let status = operation_client
            .check_status(&lro, "CreateOrUpdateDaprComponent", &func_cfg.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for commands Dapr component".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    #[handler(
        state = WaitingForLegacyCommandsDaprComponentDeletionDuringCreate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_legacy_commands_dapr_component_deletion_during_create(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_legacy_commands_dapr_component_deletion_during_create",
                "Azure ARM operation failed for legacy commands Dapr deletion during create",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        // If no readiness probe is configured, the worker is ready.
        if func_cfg.readiness_probe.is_none() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // Only run probe for public ingress where we have a URL.
        if func_cfg.public_endpoints.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let url = match &self.url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL is missing".to_string(),
                }));
            }
        };

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => {
                info!(name=%func_cfg.id, "Readiness probe succeeded");

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: Some(READINESS_PROBE_MAX_ATTEMPTS as u32),
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    #[handler(
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        info!(name=%func_cfg.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_app_name) = &self.container_app_name {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            let config = ctx.desired_resource_config::<Worker>()?;
            let deployment_rg = azure_utils::get_resource_group_name(ctx.state)?;

            // Build Azure resource scope for the container app
            let resource_scope = Scope::Resource {
                resource_group_name: deployment_rg.clone(),
                resource_provider: "Microsoft.App".to_string(),
                parent_resource_path: None,
                resource_type: "containerApps".to_string(),
                resource_name: container_app_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                container_app_name,
                resource_scope,
                "Worker",
                "worker",
            )
            .await?;
        }

        info!(name=%func_cfg.id, "Successfully applied resource-scoped permissions");

        // The worker is the consumer of every upstream Azure role assignment
        // applied during this application install (storage, KV, queue, vault,
        // plus the worker/execute role just applied above). Azure RBAC propagation can
        // take 2-5 minutes for resource-scope assignments — caller code that
        // invokes the worker within seconds otherwise hits 403
        // `AuthorizationPermissionMismatch`. Wait here, in the consumer, before
        // running readiness probes or signalling Ready.
        self.ready_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: WaitingForRbacPropagation,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForRbacPropagation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let deadline = ensure_rbac_wait_deadline(
            &mut self.ready_rbac_wait_until_epoch_secs,
            AZURE_READY_RBAC_WAIT_SECS,
        );

        if let Some(delay) = rbac_wait_delay(deadline) {
            info!(
                name=%func_cfg.id,
                remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure RBAC propagation before marking worker Ready"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.ready_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: RunningReadinessProbe,
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
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let storage_trigger_count = func_cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Storage { .. }))
            .count();
        let needs_auxiliary_checkpoint = !self.auxiliary_teardown_candidates_initialized
            && (storage_trigger_count > 0
                || self.dapr_component_naming_version < CURRENT_DAPR_COMPONENT_NAMING_VERSION);
        let needs_storage_delivery_reconciliation = storage_trigger_count
            != self.storage_trigger_infrastructure.len()
            || self
                .storage_trigger_infrastructure
                .iter()
                .any(|target| !target.delivery_reconciled || target.storage_id.is_none());
        if needs_auxiliary_checkpoint || needs_storage_delivery_reconciliation {
            let container_app_name = self.container_app_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            if needs_auxiliary_checkpoint {
                self.initialize_auxiliary_teardown_candidates(ctx, func_cfg, &container_app_name)
                    .await?;
                return Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                });
            }
            for trigger in &func_cfg.triggers {
                let alien_core::WorkerTrigger::Storage { storage, events } = trigger else {
                    continue;
                };
                let desired = self
                    .desired_storage_trigger_target(ctx, func_cfg, &container_app_name, storage)
                    .await?;
                if matches!(
                    self.prepare_storage_trigger_target(ctx, &desired.infrastructure)
                        .await?,
                    StorageTargetPreparation::Pending
                ) {
                    return Ok(HandlerAction::Continue {
                        state: Ready,
                        suggested_delay: None,
                    });
                }
                match self
                    .ensure_storage_delivery_infrastructure(
                        ctx, func_cfg, storage, events, &desired,
                    )
                    .await?
                {
                    StorageDeliveryReconcileResult::Complete => {}
                    StorageDeliveryReconcileResult::Pending(delay) => {
                        return Ok(HandlerAction::Continue {
                            state: Ready,
                            suggested_delay: Some(delay),
                        });
                    }
                }
            }
        }
        if self.dapr_component_naming_version < CURRENT_DAPR_COMPONENT_NAMING_VERSION {
            info!(
                worker=%func_cfg.id,
                from_version=self.dapr_component_naming_version,
                to_version=CURRENT_DAPR_COMPONENT_NAMING_VERSION,
                "Migrating Dapr component names"
            );
            return Ok(HandlerAction::Continue {
                state: MigratingDaprComponentNames,
                suggested_delay: None,
            });
        }
        if func_cfg.commands_enabled
            && (self.commands_resource_group_name.is_none()
                || self.commands_namespace_name.is_none()
                || self.commands_queue_name.is_none())
        {
            let container_app_name = self.container_app_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            self.initialize_commands_teardown_candidates(ctx, func_cfg, &container_app_name)
                .await?;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        if !matches!(
            self.reconcile_commands_sender_role_assignment(ctx, func_cfg)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        // Heartbeat check: verify Container App still exists and is in correct state
        let container_app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Container App during heartbeat check".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        // Verify Container App is in Succeeded state - drift is non-retryable
        if let Some(properties) = &container_app.properties {
            use alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState;
            if properties.provisioning_state
                == Some(ContainerAppPropertiesProvisioningState::Failed)
            {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: func_cfg.id.clone(),
                    message: "Container App is in Failed state".to_string(),
                }));
            }
        }

        // Imported workers skip the create flow, so the heartbeat is where they pick up the ingress
        // host (the DNS CNAME target — see `container_app_url`).
        self.container_app_url = self.extract_url_from_container_app(&container_app);

        // Check for certificate renewal on auto-managed public domains.
        if !func_cfg.public_endpoints.is_empty() && !self.uses_custom_domain {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&func_cfg.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            worker=%func_cfg.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateImportingCertificate,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        emit_azure_container_apps_worker_heartbeat(
            ctx,
            &func_cfg,
            container_app_name,
            &container_app,
        );

        debug!(name = %func_cfg.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = MigratingDaprComponentNames,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn migrating_dapr_component_names(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.migrate_dapr_component_names(ctx).await? {
            DaprComponentMigrationStep::Complete => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            DaprComponentMigrationStep::Mutated => Ok(HandlerAction::Continue {
                state: MigratingDaprComponentNames,
                suggested_delay: None,
            }),
            DaprComponentMigrationStep::LongRunning {
                operation,
                deleted_component,
            } => {
                let delay = operation.retry_after.unwrap_or(Duration::from_secs(15));
                self.pending_operation_url = Some(operation.url);
                self.pending_operation_retry_after = operation
                    .retry_after
                    .map(|retry_after| retry_after.as_secs());
                self.pending_dapr_component_deletion_name = deleted_component;
                Ok(HandlerAction::Continue {
                    state: WaitingForDaprComponentNameMigrationOperation,
                    suggested_delay: Some(delay),
                })
            }
        }
    }

    #[handler(
        state = WaitingForDaprComponentNameMigrationOperation,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_name_migration_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "MigrateDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_dapr_component_name_migration_operation",
                failure_message: "Azure ARM operation failed during Dapr name migration",
                // A missing operation URL is not proof that either a create or
                // delete finished. Re-enter migration and let ownership GETs
                // plus idempotent ensure/delete determine the remote state.
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: MigratingDaprComponentNames,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: MigratingDaprComponentNames,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if func_cfg.public_endpoints.is_empty() || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&func_cfg.id))
        else {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        };

        if resource.issued_at == self.certificate_issued_at {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &func_cfg.id);
        let certificate = ManagedEnvironmentCertificate {
            location: azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
            properties: Some(ManagedEnvironmentCertificateProperties {
                value: Some(pkcs12_base64),
                password: Some(String::new()),
                certificate_key_vault_properties: None,
            }),
            tags: HashMap::new(),
        };

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let response = container_apps_client
            .create_or_update_managed_environment_certificate(
                &resource_group_name,
                &environment_name,
                &certificate_name,
                &certificate,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to re-import certificate to Azure Container Apps Environment"
                    .to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        let container_apps_certificate_id = response.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Azure Container Apps Environment certificate response missing ID"
                    .to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        if self.fqdn.is_some() {
            let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            let fqdn = self.fqdn.clone().unwrap();
            let environment_name = get_container_apps_environment_name(ctx.state)?;
            let mut app = self
                .build_container_app(
                    func_cfg,
                    &environment_name,
                    container_app_name,
                    azure_cfg,
                    ctx,
                )
                .await?;
            Self::set_custom_domain(&mut app, fqdn, container_apps_certificate_id.clone());

            container_apps_client
                .create_or_update_container_app(&resource_group_name, container_app_name, &app)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to bind renewed certificate to custom domain".to_string(),
                    resource_id: Some(func_cfg.id.clone()),
                })?;
        }

        self.container_apps_certificate_id = Some(container_apps_certificate_id);
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: UpdateStart,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_dapr_components_deleted = false;
        self.commands_update_teardown_candidates_initialized = false;
        self.trigger_update_teardown_candidates_initialized = false;
        self.commands_sender_role_assignment_discovery_complete = false;
        self.commands_queue_applied = false;

        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "container_app_name missing prior to update_start".to_string(),
                operation: Some("update_start".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        if !self.storage_delivery_update_reconciliation_initialized {
            let mut desired_storage_targets = Vec::new();
            for trigger in &func_cfg.triggers {
                let alien_core::WorkerTrigger::Storage { storage, .. } = trigger else {
                    continue;
                };
                desired_storage_targets.push(
                    self.desired_storage_trigger_target(
                        ctx,
                        func_cfg,
                        &container_app_name,
                        storage,
                    )
                    .await?
                    .infrastructure,
                );
            }
            for desired in desired_storage_targets {
                if let Some(tracked) = self
                    .storage_trigger_infrastructure
                    .iter_mut()
                    .find(|tracked| tracked.matches_target(&desired))
                {
                    tracked.queue_applied = false;
                    tracked.delivery_reconciled = false;
                }
            }
            self.storage_delivery_update_reconciliation_initialized = true;
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        self.update_rbac_wait_required = true;

        // Build desired spec
        let desired_app = self
            .build_container_app(
                func_cfg,
                &environment_name,
                &container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;
        let mut desired_app = desired_app;
        if let (Some(fqdn), Some(keyvault_cert_id)) = (&self.fqdn, &self.keyvault_cert_id) {
            Self::set_custom_domain(&mut desired_app, fqdn.clone(), keyvault_cert_id.clone());
        }

        // Issue UPDATE
        let op_result = client
            .update_container_app(&resource_group_name, &container_app_name, &desired_app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app update".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        match op_result {
            OperationResult::Completed(_) => {
                info!(name=%container_app_name, "Update completed immediately – polling app status");
                Ok(HandlerAction::Continue {
                    state: UpdatingContainerApp,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
            OperationResult::LongRunning(lro) => {
                debug!(name=%container_app_name, "Update is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForUpdateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
    }

    #[handler(
        state = WaitingForUpdateOperation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_update_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Container app name not set while polling update".to_string(),
            })
        })?;
        match poll_pending_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "UpdateContainerApp",
                operation_target: container_app_name,
                resource_id: &worker.id,
                handler_name: "waiting_for_update_operation",
                failure_message: "Azure ARM operation failed for container app update",
            },
        )
        .await?
        {
            AzureStrictOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdatingContainerApp,
                    suggested_delay: None,
                })
            }
            AzureStrictOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdatingContainerApp,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        let app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Error checking container app update status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if let Some(props) = &app.properties {
            match props.provisioning_state.as_ref() {
                Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                    info!(name=%container_app_name, "Update provisioning succeeded – updating Dapr components");

                    let container_app_url = self.extract_url_from_container_app(&app);
                    // Capture the ingress host (DNS CNAME target) before `url` is overridden below.
                    self.container_app_url = container_app_url.clone();

                    // Check for URL override in deployment config, otherwise use Container App URL
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(&func_cfg.id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or(container_app_url);

                    Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                    Ok(HandlerAction::Stay {
                        max_times: Some(60),
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::Failed) => {
                    Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: "Container app update failed".to_string(),
                        resource_id: Some(func_cfg.id.clone()),
                    }))
                }
                _ => Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(10)),
                }),
            }
        } else {
            Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }

    #[handler(
        state = UpdateDaprComponents,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self.wait_for_container_apps_environment_wake_retry(
            &current_config.id,
            "Dapr component update",
        ) {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let permissions_changed =
            current_config.get_permissions() != previous_config.get_permissions();
        let commands_changed = current_config.commands_enabled != previous_config.commands_enabled;
        if current_config.commands_enabled {
            let azure_config = ctx.get_azure_config()?;
            match self
                .setup_commands_infrastructure(
                    ctx,
                    azure_config,
                    current_config,
                    &container_app_name,
                )
                .await?
            {
                CommandsSetupOperation::Completed => {
                    self.commands_update_teardown_candidates_initialized = false;
                }
                CommandsSetupOperation::Creating(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForDaprComponentUpdateOperation,
                        suggested_delay: Some(delay),
                    });
                }
                CommandsSetupOperation::Deleting(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: UpdateWaitingForCommandsDaprComponentDeletionForSetup,
                        suggested_delay: Some(delay),
                    });
                }
                CommandsSetupOperation::Pending(delay) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
            }
        } else if !self.commands_update_teardown_candidates_initialized
            && (commands_changed
                || self.commands_dapr_component.is_some()
                || self.commands_sender_role_assignment_id.is_some()
                || self.commands_sender_role_assignment_intent.is_some()
                || self.commands_resource_group_name.is_some()
                || self.commands_namespace_name.is_some()
                || self.commands_queue_name.is_some())
        {
            self.initialize_commands_teardown_candidates(ctx, previous_config, &container_app_name)
                .await?;
            self.commands_update_teardown_candidates_initialized = true;
            return Ok(HandlerAction::Continue {
                state: UpdateDeletingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        let storage_targets_changed = self
            .storage_trigger_targets_changed(ctx, current_config, &container_app_name)
            .await?;
        let triggers_changed = current_config.triggers != previous_config.triggers
            || permissions_changed
            || storage_targets_changed;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating Dapr components");

            if !self.update_dapr_components_deleted {
                if !self.trigger_update_teardown_candidates_initialized {
                    self.initialize_storage_trigger_teardown_candidates(
                        ctx,
                        previous_config,
                        &container_app_name,
                    )
                    .await?;
                    self.initialize_trigger_update_teardown_candidates(
                        previous_config,
                        &container_app_name,
                    );
                    self.trigger_update_teardown_candidates_initialized = true;
                    return Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    });
                }

                // Trigger components are keyed by trigger shape. Delete the previous
                // set once, then recreate desired components across possible ARM LROs.
                if matches!(
                    self.delete_storage_trigger_infrastructure(ctx).await?,
                    StorageTriggerTeardownResult::Mutated
                ) {
                    return Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    });
                }
                match self.delete_all_dapr_components(ctx).await? {
                    TrackedDaprComponentDeleteStep::Complete => {
                        self.update_dapr_components_deleted = true;
                    }
                    TrackedDaprComponentDeleteStep::Mutated => {
                        return Ok(HandlerAction::Continue {
                            state: UpdateDaprComponents,
                            suggested_delay: None,
                        });
                    }
                    TrackedDaprComponentDeleteStep::LongRunning {
                        operation,
                        component_name,
                    } => {
                        let delay = operation.retry_after.unwrap_or(Duration::from_secs(15));
                        self.pending_operation_url = Some(operation.url);
                        self.pending_operation_retry_after = operation
                            .retry_after
                            .map(|retry_after| retry_after.as_secs());
                        self.pending_dapr_component_deletion_name = Some(component_name);
                        return Ok(HandlerAction::Continue {
                            state: WaitingForDaprComponentDeletionForUpdate,
                            suggested_delay: Some(delay),
                        });
                    }
                }
            }
        }

        // Reconcile components for all triggers. This is intentionally
        // idempotent so dependency-only changes update Dapr metadata even
        // when the Worker config itself is unchanged.
        let mut cron_index = 0usize;
        for trigger in &current_config.triggers {
            match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    let operation = match self
                        .create_dapr_service_bus_component(
                            ctx,
                            &container_app_name,
                            &current_config,
                            queue,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    let operation = match self
                        .create_azure_storage_trigger(
                            ctx,
                            &container_app_name,
                            &current_config,
                            storage,
                            events,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    let operation = match self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            &current_config,
                            cron,
                            cron_index,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    cron_index += 1;
                }
            }
        }
        self.container_apps_environment_wake_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_retry_after_epoch_secs = None;

        if !matches!(
            self.reconcile_commands_sender_role_assignment(ctx, current_config)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        if self.update_rbac_wait_required {
            Ok(HandlerAction::Continue {
                state: UpdateWaitingForRbacPropagation,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = WaitingForDaprComponentDeletionForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_deletion_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_dapr_component_deletion_for_update",
                failure_message: "Azure ARM operation failed for Dapr component deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForLegacyDaprComponentDeletion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_legacy_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "update_waiting_for_legacy_dapr_component_deletion",
                "Azure ARM operation failed for legacy Dapr component deletion during update",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = WaitingForDaprComponentUpdateOperation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_update_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        match poll_pending_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "CreateOrUpdateDaprComponent",
                operation_target: &func_cfg.id,
                resource_id: &func_cfg.id,
                handler_name: "waiting_for_dapr_component_update_operation",
                failure_message: "Azure ARM operation failed for Dapr component update",
            },
        )
        .await?
        {
            AzureStrictOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureStrictOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Re‑use the same readiness‑probe helper.
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if func_cfg.readiness_probe.is_none() || func_cfg.public_endpoints.is_empty() {
            self.storage_delivery_update_reconciliation_initialized = false;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let url = self
            .url
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL missing after update".to_string(),
                })
            })?
            .clone();

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => {
                self.storage_delivery_update_reconciliation_initialized = false;
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: Some(READINESS_PROBE_MAX_ATTEMPTS as u32),
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    #[handler(
        state = UpdateWaitingForRbacPropagation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let deadline = ensure_rbac_wait_deadline(
            &mut self.ready_rbac_wait_until_epoch_secs,
            AZURE_READY_RBAC_WAIT_SECS,
        );

        if let Some(delay) = rbac_wait_delay(deadline) {
            info!(
                name=%func_cfg.id,
                remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure RBAC propagation before completing worker update"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_rbac_wait_required = false;
        Ok(HandlerAction::Continue {
            state: UpdateRunningReadinessProbe,
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
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if self.pending_operation_url.is_some() {
            return Ok(HandlerAction::Continue {
                state: WaitingForPendingOperationBeforeDelete,
                suggested_delay: self.pending_operation_retry_after.map(Duration::from_secs),
            });
        }

        // Handle case where container_app_name is not set (e.g., creation failed early)
        let _container_app_name = match self.container_app_name.as_ref() {
            Some(name) => name.clone(),
            None => {
                // No container app was created, nothing to delete
                info!(resource_id=%func_cfg.id, "No Container App to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.clear_all();

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        // Always go to deleting Dapr components first (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingDaprComponents,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForPendingOperationBeforeDelete,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_pending_operation_before_delete(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "CompleteOperationBeforeWorkerDelete",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_pending_operation_before_delete",
                failure_message: "Azure ARM operation failed before worker deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = DeletingDaprComponents,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%worker_config.id, components=?self.dapr_components, "Deleting Dapr components");

        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;
        if self.initialize_dapr_component_deletion_candidates(worker_config, &container_app_name) {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }
        if self
            .initialize_auxiliary_teardown_candidates(ctx, worker_config, &container_app_name)
            .await?
        {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }

        if matches!(
            self.delete_storage_trigger_infrastructure(ctx).await?,
            StorageTriggerTeardownResult::Mutated
        ) {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }

        match self.delete_all_dapr_components(ctx).await? {
            TrackedDaprComponentDeleteStep::Complete => {}
            TrackedDaprComponentDeleteStep::Mutated => {
                return Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                });
            }
            TrackedDaprComponentDeleteStep::LongRunning {
                operation,
                component_name,
            } => {
                let delay = operation.retry_after.unwrap_or(Duration::from_secs(15));
                self.pending_operation_url = Some(operation.url);
                self.pending_operation_retry_after = operation
                    .retry_after
                    .map(|retry_after| retry_after.as_secs());
                self.pending_dapr_component_deletion_name = Some(component_name);
                return Ok(HandlerAction::Continue {
                    state: WaitingForDaprComponentDeletion,
                    suggested_delay: Some(delay),
                });
            }
        }

        // Continue to commands infrastructure cleanup
        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForDaprComponentDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for Dapr component deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = DeletingCommandsInfrastructure,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.delete_commands_infrastructure_step(ctx).await? {
            CommandsTeardownResult::Complete => Ok(HandlerAction::Continue {
                state: DeletingApp,
                suggested_delay: None,
            }),
            CommandsTeardownResult::Mutated => Ok(HandlerAction::Continue {
                state: DeletingCommandsInfrastructure,
                suggested_delay: None,
            }),
            CommandsTeardownResult::LongRunning(delay) => Ok(HandlerAction::Continue {
                state: WaitingForCommandsDaprComponentDeletion,
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = WaitingForCommandsDaprComponentDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_commands_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_commands_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for commands Dapr deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_commands_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdateDeletingCommandsInfrastructure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_deleting_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.delete_commands_infrastructure_step(ctx).await? {
            // Keep the checkpoint latched until the next UpdateStart. `commands_changed` remains
            // true for this whole update, so clearing it here would restart teardown forever.
            CommandsTeardownResult::Complete => Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: None,
            }),
            CommandsTeardownResult::Mutated => Ok(HandlerAction::Continue {
                state: UpdateDeletingCommandsInfrastructure,
                suggested_delay: None,
            }),
            CommandsTeardownResult::LongRunning(delay) => Ok(HandlerAction::Continue {
                state: UpdateWaitingForCommandsDaprComponentDeletion,
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForCommandsDaprComponentDeletion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_commands_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "update_waiting_for_commands_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for commands Dapr update deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_commands_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: UpdateDeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForCommandsDaprComponentDeletionForSetup,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_commands_dapr_component_deletion_for_setup(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "update_waiting_for_commands_dapr_component_deletion_for_setup",
                "Azure ARM operation failed for commands setup Dapr deletion",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = DeletingApp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_app(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .delete_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Ok(OperationResult::Completed(_)) => {
                info!(name=%container_app_name, "Container app deleted immediately");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(OperationResult::LongRunning(lro)) => {
                debug!(name=%container_app_name, "Deletion is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                Ok(HandlerAction::Continue {
                    state: WaitingForDeleteOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app already deleted");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                let worker_config = ctx.desired_resource_config::<Worker>()?;
                Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete container app".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = WaitingForDeleteOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Container app name not set while polling deletion".to_string(),
            })
        })?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteContainerApp",
                operation_target: container_app_name,
                resource_id: &worker.id,
                handler_name: "waiting_for_delete_operation",
                failure_message: "Azure ARM operation failed for container app deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingContainerApp,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingApp,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(delay),
            }),
        }
    }

    #[handler(
        state = DeletingContainerApp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = match &self.container_app_name {
            Some(n) => n.clone(),
            None => {
                return Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                });
            }
        };
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app confirmed deleted");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                debug!(name=%container_app_name, "Container app still exists – retry");
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app deletion status".to_string(),
                resource_id: Some(ctx.desired_resource_config::<Worker>()?.id.clone()),
            })),
        }
    }

    #[handler(
        state = DeletingCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let has_resolvable_domain = !worker_config.public_endpoints.is_empty()
            && Self::resolve_domain_info(ctx, &worker_config.id).is_ok();
        if self.container_apps_certificate_id.is_none()
            && !self.uses_custom_domain
            && !has_resolvable_domain
        {
            self.clear_all();
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .delete_managed_environment_certificate(
                &resource_group_name,
                &environment_name,
                &certificate_name,
            )
            .await
        {
            Ok(OperationResult::Completed(())) => {
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(OperationResult::LongRunning(lro)) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                Ok(HandlerAction::Continue {
                    state: WaitingForCertificateDeleteOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to delete Container Apps managed environment certificate"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForCertificateDeleteOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_certificate_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteManagedEnvironmentCertificate",
                operation_target: &certificate_name,
                resource_id: &worker_config.id,
                handler_name: "waiting_for_certificate_delete_operation",
                failure_message:
                    "Azure ARM operation failed for managed environment certificate deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(delay),
            }),
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

    // Implementation of get_outputs trait method
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.resource_id.as_ref().map(|id| {
            // CNAME target = the ingress host; fall back to `url` when `container_app_url` is unset.
            let load_balancer_endpoint =
                self.container_app_url
                    .as_ref()
                    .or(self.url.as_ref())
                    .map(|host| alien_core::LoadBalancerEndpoint {
                        dns_name: dns_name_from_url(host),
                        hosted_zone_id: None,
                    });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .container_app_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                identifier: Some(id.clone()),
                public_endpoints: self
                    .url
                    .as_ref()
                    .map(|url| {
                        std::collections::HashMap::from([(
                            "default".to_string(),
                            alien_core::PublicEndpointOutput {
                                host: alien_core::public_url_host(url).unwrap_or_default(),
                                protocol: alien_core::ExposeProtocol::Http,
                                port: alien_core::public_url_port(url).unwrap_or(443),
                                url: url.clone(),
                                wildcard_host: None,
                                load_balancer_endpoint,
                            },
                        )])
                    })
                    .unwrap_or_default(),
                commands_push_target: match (
                    &self.commands_namespace_name,
                    &self.commands_queue_name,
                ) {
                    (Some(ns), Some(q)) => Some(format!("{}/{}", ns, q)),
                    _ => None,
                },
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ContainerAppWorkerBinding, WorkerBinding};

        if let (Some(container_app_name), Some(resource_id)) =
            (&self.container_app_name, &self.resource_id)
        {
            // Extract resource group name from ARM resource ID
            // Format: /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.App/containerApps/{name}
            let resource_group_name = resource_id
                .split('/')
                .nth(4)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Malformed ARM resource ID (missing resource group): {}",
                            resource_id
                        ),
                        operation: Some("parse_arm_resource_id".to_string()),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?
                .to_string();

            // Extract subscription ID from ARM resource ID
            // Format: /subscriptions/{sub}/resourceGroups/{rg}/providers/...
            let subscription_id = resource_id
                .split('/')
                .nth(2)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Malformed ARM resource ID (missing subscription): {}",
                            resource_id
                        ),
                        operation: Some("parse_arm_resource_id".to_string()),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?
                .to_string();

            // Private URL is the internal FQDN (same as public URL for Container Apps
            // with external ingress; for internal ingress it would differ).
            let private_url = self
                .url
                .clone()
                .unwrap_or_else(|| format!("https://{}", container_app_name));

            let binding = WorkerBinding::ContainerApp(ContainerAppWorkerBinding {
                subscription_id: BindingValue::Value(subscription_id),
                resource_group_name: BindingValue::Value(resource_group_name),
                container_app_name: BindingValue::Value(container_app_name.clone()),
                private_url: BindingValue::Value(private_url),
                public_url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
            });
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

#[cfg(test)]
#[path = "../azure_tests.rs"]
mod tests;
