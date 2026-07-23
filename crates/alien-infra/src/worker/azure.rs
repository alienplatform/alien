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

#[path = "azure_cleanup.rs"]
mod cleanup;
use cleanup::{AzureCommandsQueueTarget, CommandsQueueTargetPreparation};
#[path = "azure_command_sender.rs"]
mod command_sender;
use command_sender::{AzureCommandsSenderRoleAssignmentIntent, CommandsSenderReconcileResult};
#[path = "azure_operations.rs"]
mod operations;
use operations::{
    poll_pending_operation, poll_reconciled_operation, AzureOperationPoll,
    AzureOperationPollRequest, AzureStrictOperationPoll,
};
#[path = "azure_role_assignments.rs"]
mod role_assignments;
#[path = "azure_trigger_targets.rs"]
mod trigger_targets;
use trigger_targets::{StorageDeliveryReconcileResult, StorageTargetPreparation};

/// Generates a deterministic Azure Container Apps name for a worker.
fn get_azure_container_app_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[cfg(not(test))]
const AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS: u64 = 60;
#[cfg(test)]
const AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
const AZURE_READY_RBAC_WAIT_SECS: u64 = 120;
#[cfg(test)]
const AZURE_READY_RBAC_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
const AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS: u64 = 900;
#[cfg(test)]
const AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS: u64 = 600;
#[cfg(test)]
const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS: u64 = 0;

const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;
const AZURE_RBAC_WAIT_MAX_ATTEMPTS: u32 = 1_000;
const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_POLL_SECS: u64 = 30;

fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn ensure_rbac_wait_deadline(wait_until_epoch_secs: &mut Option<u64>, wait_secs: u64) -> u64 {
    let now = current_unix_timestamp_secs();
    *wait_until_epoch_secs.get_or_insert_with(|| now.saturating_add(wait_secs))
}

fn rbac_wait_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_RBAC_WAIT_POLL_SECS),
        ))
    }
}

fn container_apps_environment_wake_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_POLL_SECS),
        ))
    }
}

fn retry_after_delay(retry_after_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = retry_after_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(remaining))
    }
}

fn dns_name_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('.')
        .to_string()
}

fn management_profile_dispatches_commands(
    ctx: &ResourceControllerContext<'_>,
    worker_id: &str,
) -> bool {
    ctx.desired_stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(worker_id))
        .is_some_and(|refs| refs.iter().any(|r| r.id() == "worker/dispatch-command"))
}

fn is_azure_container_apps_environment_waking_error(error: &AlienError<ErrorData>) -> bool {
    fn matches_layer(message: &str, context: Option<&serde_json::Value>) -> bool {
        let context_text = context.map(|value| value.to_string()).unwrap_or_default();
        message.contains("ContainerAppEnvironmentDisabled")
            || message.contains("environment is stopped due to a long period of inactivity")
            || context_text.contains("ContainerAppEnvironmentDisabled")
            || context_text.contains("environment is stopped due to a long period of inactivity")
    }

    if matches_layer(&error.message, error.context.as_ref()) {
        return true;
    }

    let mut source = error.source.as_deref();
    while let Some(layer) = source {
        if matches_layer(&layer.message, layer.context.as_ref()) {
            return true;
        }
        source = layer.source.as_deref();
    }

    false
}

fn get_container_apps_certificate_name(prefix: &str, worker_id: &str) -> String {
    format!("{}-{}", prefix, worker_id)
        .replace('_', "-")
        .to_lowercase()
}

/// Domain information for a worker.
struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    keyvault_cert_id: Option<String>,
    container_apps_certificate_id: Option<String>,
    uses_custom_domain: bool,
}

pub(super) enum DaprComponentOperation {
    Completed,
    Creating(Duration),
    Deleting(Duration),
    Pending(Duration),
}

enum CommandsSetupOperation {
    Completed,
    Creating(Duration),
    Deleting(Duration),
    Pending(Duration),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageTriggerInfrastructure {
    #[serde(default)]
    pub storage_id: Option<String>,
    pub source_resource_id: String,
    #[serde(default)]
    pub source_container_name: Option<String>,
    pub event_subscription_name: String,
    pub service_bus_resource_group: String,
    pub namespace_name: String,
    pub queue_name: String,
    #[serde(default)]
    pub queue_applied: bool,
    pub receiver_role_assignment_id: Option<String>,
    #[serde(default)]
    pub delivery_reconciled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AzureStorageTriggerTeardownProgress {
    #[default]
    EventSubscription,
    ReceiverRoleAssignment,
    Queue,
}

enum StorageTriggerTeardownResult {
    Complete,
    Mutated,
}

enum CommandsTeardownResult {
    Complete,
    Mutated,
    LongRunning(Duration),
}

fn emit_azure_container_apps_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    worker_config: &Worker,
    container_app_name: &str,
    container_app: &ContainerApp,
) {
    let properties = container_app.properties.as_ref();
    let template = properties.and_then(|properties| properties.template.as_ref());
    let container = template.and_then(|template| template.containers.first());
    let resources = container.and_then(|container| container.resources.as_ref());
    let scale = template.and_then(|template| template.scale.as_ref());
    let ingress = properties
        .and_then(|properties| properties.configuration.as_ref())
        .and_then(|configuration| configuration.ingress.as_ref());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: worker_config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::AzureContainerApps(
            AzureContainerAppsWorkerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "Azure Container App '{container_app_name}' is reachable"
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                app_name: container_app_name.to_string(),
                revision: properties.and_then(|properties| properties.latest_revision_name.clone()),
                environment_name: properties.and_then(|properties| {
                    properties
                        .managed_environment_id
                        .clone()
                        .or_else(|| properties.environment_id.clone())
                }),
                provisioning_state: properties
                    .and_then(|properties| properties.provisioning_state.as_ref())
                    .map(|state| format!("{state:?}")),
                running_status: properties
                    .and_then(|properties| properties.running_status.as_ref())
                    .map(|status| format!("{status:?}")),
                ingress_fqdn: ingress.and_then(|ingress| ingress.fqdn.clone()),
                min_replicas: scale.and_then(|scale| scale.min_replicas),
                max_replicas: scale.map(|scale| scale.max_replicas),
                cpu: resources.and_then(|resources| resources.cpu),
                memory: resources.and_then(|resources| resources.memory.clone()),
            },
        )),
        raw: vec![],
    });
}

/// Converts PEM-encoded private key and certificate chain to PKCS#12 format for Azure Key Vault.
/// Azure Key Vault requires certificates in PKCS#12 (PFX) format.
fn pem_to_pkcs12(private_key_pem: &str, certificate_chain_pem: &str) -> Result<Vec<u8>> {
    use alien_error::IntoAlienError;

    // Parse private key PEM
    let key_blocks = pem::parse_many(private_key_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse private key PEM".to_string(),
            resource_id: None,
        })?;

    let key_block = key_blocks
        .into_iter()
        .find(|p| p.tag().ends_with("PRIVATE KEY"))
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message:
                    "No PRIVATE KEY block found in private key PEM (expected BEGIN PRIVATE KEY)"
                        .to_string(),
                resource_id: None,
            })
        })?;

    // p12 expects PKCS#8 PrivateKeyInfo DER bytes (BEGIN PRIVATE KEY)
    if key_block.tag() != "PRIVATE KEY" {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Unsupported key type '{}'. Expected 'PRIVATE KEY' (PKCS#8). Convert to PKCS#8 first.",
                key_block.tag()
            ),
            resource_id: None,
        }));
    }
    let key_der = key_block.contents().to_vec();

    // Parse certificate chain PEM
    let cert_blocks = pem::parse_many(certificate_chain_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse certificate chain PEM".to_string(),
            resource_id: None,
        })?;

    let mut certs: Vec<pem::Pem> = cert_blocks
        .into_iter()
        .filter(|p| p.tag().contains("CERTIFICATE"))
        .collect();

    if certs.is_empty() {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "No CERTIFICATE blocks found in PEM".to_string(),
            resource_id: None,
        }));
    }

    // Leaf is first, rest are intermediates
    let leaf_pem = certs.remove(0);
    let leaf_der = leaf_pem.contents().to_vec();

    let intermediate_ders: Vec<Vec<u8>> =
        certs.into_iter().map(|p| p.contents().to_vec()).collect();
    let intermediate_refs: Vec<&[u8]> = intermediate_ders.iter().map(|v| v.as_slice()).collect();

    // Build PKCS#12 with empty password
    let pfx = p12::PFX::new_with_cas(
        &leaf_der,
        &key_der,
        &intermediate_refs,
        "",
        "Alien Worker Certificate",
    )
    .ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "Failed to build PKCS#12 (p12::PFX::new_with_cas returned None)".to_string(),
            resource_id: None,
        })
    })?;

    Ok(pfx.to_der())
}

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

impl AzureWorkerController {
    fn wait_for_container_apps_environment_wake_retry(
        &mut self,
        worker_id: &str,
        operation: &str,
    ) -> Option<Duration> {
        let retry_after = self.container_apps_environment_wake_retry_after_epoch_secs?;
        if let Some(delay) = retry_after_delay(retry_after) {
            debug!(
                worker=%worker_id,
                operation=%operation,
                remaining_secs=retry_after.saturating_sub(current_unix_timestamp_secs()),
                "Waiting before retrying Azure Container Apps Environment operation"
            );
            Some(delay)
        } else {
            self.container_apps_environment_wake_retry_after_epoch_secs = None;
            None
        }
    }

    fn record_container_apps_environment_wake_retry(
        &mut self,
        deadline_epoch_secs: u64,
    ) -> Option<Duration> {
        let delay = container_apps_environment_wake_delay(deadline_epoch_secs)?;
        self.container_apps_environment_wake_retry_after_epoch_secs =
            Some(current_unix_timestamp_secs().saturating_add(delay.as_secs()));
        Some(delay)
    }

    async fn wait_for_reconciled_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        handler_name: &'static str,
        failure_message: &'static str,
    ) -> Result<Option<Duration>> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name,
                failure_message,
            },
        )
        .await?
        {
            AzureOperationPoll::Complete | AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(None)
            }
            AzureOperationPoll::Pending(delay) => Ok(Some(delay)),
        }
    }
}

// ≡ Lifecycle implementation ===================================================
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
        } else if commands_changed
            || self.commands_dapr_component.is_some()
            || self.commands_sender_role_assignment_id.is_some()
            || self.commands_sender_role_assignment_intent.is_some()
            || self.commands_resource_group_name.is_some()
            || self.commands_namespace_name.is_some()
            || self.commands_queue_name.is_some()
        {
            if !self.commands_update_teardown_candidates_initialized {
                self.initialize_commands_teardown_candidates(
                    ctx,
                    previous_config,
                    &container_app_name,
                )
                .await?;
                self.commands_update_teardown_candidates_initialized = true;
                return Ok(HandlerAction::Continue {
                    state: UpdateDeletingCommandsInfrastructure,
                    suggested_delay: None,
                });
            }
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
            CommandsTeardownResult::Complete => {
                self.commands_update_teardown_candidates_initialized = false;
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
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

impl AzureWorkerController {
    // ─────────────── HELPER METHODS ────────────────────────────

    /// Pre-create commands infrastructure (queue, Dapr component, role assignments)
    /// before the Container App is created. This ensures the Dapr sidecar starts
    /// with the component already defined and RBAC roles already propagating.
    async fn setup_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &alien_azure_clients::AzureClientConfig,
        func_cfg: &alien_core::Worker,
        container_app_name: &str,
    ) -> Result<CommandsSetupOperation> {
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let env_resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get the Service Bus namespace from the dependent resource
        let namespace_ref = alien_core::ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: namespace_ref.id.clone(),
                })
            })?
            .clone();
        let service_bus_resource_group = namespace_controller.resource_group_name(ctx)?;

        // Create commands queue
        let queue_name = commands_queue_name(container_app_name);
        match self
            .prepare_commands_target_for_setup(
                ctx,
                func_cfg,
                &container_app_name,
                &AzureCommandsQueueTarget {
                    resource_group_name: service_bus_resource_group.clone(),
                    namespace_name: namespace_name.clone(),
                    queue_name: queue_name.clone(),
                },
            )
            .await?
        {
            CommandsQueueTargetPreparation::Ready => {}
            CommandsQueueTargetPreparation::Checkpoint => {
                return Ok(CommandsSetupOperation::Pending(Duration::from_secs(1)));
            }
            CommandsQueueTargetPreparation::LongRunning(delay) => {
                return Ok(CommandsSetupOperation::Deleting(delay));
            }
        }
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        info!(
            worker=%func_cfg.id,
            namespace=%namespace_name,
            queue=%queue_name,
            "Pre-creating commands Service Bus queue (before Container App)"
        );

        mgmt.create_or_update_queue(
            service_bus_resource_group.clone(),
            namespace_name.clone(),
            queue_name.clone(),
            alien_azure_clients::models::queue::SbQueueProperties::default(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        let component_name = get_azure_internal_commands_dapr_component_name(&container_app_name);
        let service_account_id = format!("{}-sa", func_cfg.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let client_id = service_account
            .identity_client_id
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: service_account_ref.id,
                })
            })?;
        let dapr_component = service_bus_dapr_component(
            component_name.clone(),
            &container_app_name,
            &namespace_name,
            queue_name.clone(),
            client_id,
        );

        info!(
            worker=%func_cfg.id,
            component=%component_name,
            "Pre-creating commands Dapr Service Bus component (before Container App)"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        match delete_owned_legacy_dapr_components(
            client.as_ref(),
            &env_resource_group_name,
            &environment_name,
            container_app_name,
            &component_name,
            &get_legacy_azure_internal_commands_dapr_component_names(container_app_name),
            &func_cfg.id,
        )
        .await?
        {
            LegacyDaprComponentCleanupStep::Complete => {}
            LegacyDaprComponentCleanupStep::Mutated => {
                return Ok(CommandsSetupOperation::Pending(Duration::from_secs(1)));
            }
            LegacyDaprComponentCleanupStep::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|delay| delay.as_secs());
                return Ok(CommandsSetupOperation::Deleting(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        match ensure_dapr_component(
            client.as_ref(),
            &env_resource_group_name,
            &environment_name,
            container_app_name,
            &dapr_component,
            &func_cfg.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(CommandsSetupOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        self.commands_resource_group_name = Some(service_bus_resource_group.clone());
        self.commands_namespace_name = Some(namespace_name.clone());
        self.commands_queue_name = Some(queue_name);
        self.commands_dapr_component = Some(component_name);

        if !matches!(
            self.reconcile_commands_sender_role_assignment(ctx, func_cfg)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(CommandsSetupOperation::Pending(Duration::from_secs(1)));
        }

        info!(worker=%func_cfg.id, "Commands infrastructure pre-created successfully");
        Ok(CommandsSetupOperation::Completed)
    }

    /// Resolve domain information for a public worker.
    /// Returns either custom domain config or auto-generated domain from metadata.
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;

        // Check for custom domain configuration
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let keyvault_cert_id = custom
                .certificate
                .azure
                .as_ref()
                .map(|cert| cert.key_vault_certificate_id.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an Azure Key Vault certificate ID"
                            .to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                keyvault_cert_id: Some(keyvault_cert_id),
                container_apps_certificate_id: None,
                uses_custom_domain: true,
            });
        }

        // Use auto-generated domain from domain metadata
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
        })
    }

    fn ensure_domain_info(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        if self.fqdn.is_some()
            && (self.certificate_id.is_some()
                || self.keyvault_cert_id.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id) {
            Ok(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.keyvault_cert_id = domain_info.keyvault_cert_id;
                self.container_apps_certificate_id = domain_info.container_apps_certificate_id;
                self.uses_custom_domain = domain_info.uses_custom_domain;
                if self.url.is_none() {
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(resource_id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn clear_all(&mut self) {
        self.container_app_name = None;
        self.resource_id = None;
        self.url = None;
        self.container_app_url = None;
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        self.pending_dapr_component_deletion_name = None;
        self.dapr_components.clear();
        self.fqdn = None;
        self.certificate_id = None;
        self.keyvault_cert_id = None;
        self.container_apps_certificate_id = None;
        self.uses_custom_domain = false;
        self.certificate_issued_at = None;
        self.commands_resource_group_name = None;
        self.commands_namespace_name = None;
        self.commands_queue_name = None;
        self.commands_dapr_component = None;
        self.commands_dapr_component_deletion_candidates.clear();
        self.commands_sender_role_assignment_id = None;
        self.commands_sender_role_assignment_intent = None;
        self.commands_sender_role_assignment_discovery_complete = false;
        self.commands_receiver_role_assignment_id = None;
        self.commands_infrastructure_auth_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_retry_after_epoch_secs = None;
        self.pre_container_app_rbac_wait_until_epoch_secs = None;
        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_rbac_wait_required = false;
        self.update_dapr_components_deleted = false;
        self.dapr_component_naming_version = 0;
        self.storage_trigger_infrastructure.clear();
        self.storage_trigger_teardown_progress = AzureStorageTriggerTeardownProgress::default();
        self.dapr_component_deletion_candidates_initialized = false;
        self.auxiliary_teardown_candidates_initialized = false;
        self.commands_update_teardown_candidates_initialized = false;
        self.trigger_update_teardown_candidates_initialized = false;
        self.storage_delivery_update_reconciliation_initialized = false;
        self._internal_stay_count = None;
    }

    /// Called whenever provisioning *succeeds* and we have the live resource.
    fn handle_creation_completed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        app: &ContainerApp,
    ) {
        self.resource_id = app.id.clone();

        let container_app_url = self.extract_url_from_container_app(app);

        // Capture the ingress host (DNS CNAME target) before `url` is overridden below.
        self.container_app_url = container_app_url.clone();

        // Check for URL override in deployment config, otherwise use Container App URL
        if let Ok(config) = ctx.desired_resource_config::<Worker>() {
            self.url = ctx
                .deployment_config
                .public_endpoints
                .as_ref()
                .and_then(|resources| resources.get(&config.id))
                .and_then(|endpoints| endpoints.values().next().cloned())
                .or(container_app_url);
        } else {
            self.url = container_app_url;
        }

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
    }

    fn set_custom_domain(app: &mut ContainerApp, fqdn: String, certificate_id: String) {
        if let Some(props) = &mut app.properties {
            if let Some(config) = &mut props.configuration {
                if let Some(ingress) = &mut config.ingress {
                    ingress.custom_domains = vec![CustomDomain {
                        name: fqdn,
                        binding_type: Some(CustomDomainBindingType::SniEnabled),
                        certificate_id: Some(certificate_id),
                    }];
                }
            }
        }
    }

    fn extract_url_from_container_app(&self, app: &ContainerApp) -> Option<String> {
        let fqdn = app
            .properties
            .as_ref()?
            .configuration
            .as_ref()?
            .ingress
            .as_ref()?
            .fqdn
            .clone()?;

        if fqdn.starts_with("http://") || fqdn.starts_with("https://") {
            Some(fqdn)
        } else {
            Some(format!("https://{}", fqdn))
        }
    }

    /// Prepare environment variables using the shared logic, then convert to Azure's EnvironmentVar format
    async fn prepare_environment_variables_azure(
        &self,
        func: &Worker,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<EnvironmentVar>> {
        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        // Build complete environment using shared logic
        // IMPORTANT: Start with func.environment which includes injected vars from DeploymentConfig
        let complete_env = EnvironmentVariableBuilder::try_new(&func.environment)?
            .add_worker_runtime_env_vars(ctx, &func.id, func.timeout_seconds)?
            .add_linked_resources(&func.links, ctx, &func.id)
            .await?
            .add_self_worker_binding(&func.id, self_binding_params.as_ref())?
            .build();

        // Add managed identity environment variable from ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let mut env_vars = Vec::new();

        // Convert all environment variables to Azure format
        for (name, value) in complete_env {
            env_vars.push(EnvironmentVar {
                name: Some(name),
                value: Some(value),
                secret_ref: None,
            });
        }

        // Add Azure-specific managed identity client ID. A missing identity must
        // stop reconciliation; silently omitting it would detach the workload
        // identity during an otherwise idempotent update.
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let client_id = service_account_state
            .identity_client_id
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func.id.clone(),
                    dependency_id: service_account_ref.id.clone(),
                })
            })?;
        env_vars.push(EnvironmentVar {
            name: Some(ENV_AZURE_CLIENT_ID.to_string()),
            value: Some(client_id.clone()),
            secret_ref: None,
        });

        Ok(env_vars)
    }

    /// Build the full ContainerApps ARM spec for *desired* state.
    async fn build_container_app(
        &self,
        func: &Worker,
        _environment_name: &str,
        container_app_name: &str,
        azure_cfg: &AzureClientConfig,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<ContainerApp> {
        let location = azure_cfg.region.as_deref().unwrap_or("East US");

        let image = match &func.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Worker '{}' uses source code, but only pre‑built images are supported on Azure",
                        func.id
                    ),
                    resource_id: Some(func.id.clone()),
                }));
            }
        };

        // Prepare environment variables using shared logic
        let env_vars = self.prepare_environment_variables_azure(func, ctx).await?;

        // Note: Dapr input bindings (bindings.azure.servicebusqueues) auto-deliver
        // messages without requiring GET /dapr/subscribe. No subscription env var needed.

        // Azure Container Apps requires specific CPU/memory combinations.
        // The ratio is 0.5 Gi per 0.25 CPU (2 Gi per 1 CPU).
        let memory_gi = func.memory_mb as f64 / 1024.0;
        // Azure Container Apps requires specific CPU/memory pairs where CPU = memory_gi / 2.
        // The WorkerMemoryCheck preflight validates that memory_mb is a valid Azure value
        // (512, 1024, 1536, 2048, 2560, 3072, 3584, 4096).
        let cpu = memory_gi / 2.0;

        let container = Container {
            name: Some("main".to_string()),
            image: Some(image.clone()),
            resources: Some(ContainerResources {
                cpu: Some(cpu),
                memory: Some(format!("{}Gi", memory_gi)),
                ephemeral_storage: None,
            }),
            env: env_vars,
            args: vec![],
            command: vec![],
            probes: vec![],
            volume_mounts: vec![],
        };

        // Tags for traceability
        let mut tags = HashMap::new();
        tags.insert("resource-type".to_string(), "worker".to_string());
        tags.insert("resource".to_string(), func.id.clone());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

        let _resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_id = azure_utils::get_container_apps_environment_resource_id(ctx.state)?;

        let ingress_cfg = if !func.public_endpoints.is_empty() {
            Some(alien_azure_clients::models::container_apps::Ingress {
                external: true,
                target_port: Some(8080),
                traffic: vec![TrafficWeight {
                    weight: Some(100),
                    latest_revision: true,
                    revision_name: None,
                    label: None,
                }],
                transport: IngressTransport::Auto,
                allow_insecure: false,
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
                fqdn: None,
            })
        } else {
            None
        };

        let mut registries = vec![];
        let mut secrets = vec![];

        // Managed identity support from ServiceAccounts
        // Collect all ServiceAccounts:
        // 1. Permission-based ServiceAccount (from permission profile)
        // 2. Linked ServiceAccounts (from worker.links)
        use alien_azure_clients::models::container_apps::{
            ManagedServiceIdentity, ManagedServiceIdentityType, UserAssignedIdentities,
            UserAssignedIdentity,
        };

        let mut identity_map = HashMap::new();

        // Add permission-based ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let identity_id = service_account_state
            .identity_resource_id
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func.id.clone(),
                    dependency_id: service_account_ref.id.clone(),
                })
            })?;
        identity_map.insert(
            identity_id.clone(),
            UserAssignedIdentity {
                client_id: None,
                principal_id: None,
            },
        );

        // Add linked ServiceAccounts
        for link in &func.links {
            if link.resource_type() == &alien_core::ServiceAccount::RESOURCE_TYPE {
                if let Ok(linked_sa_state) = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                    link,
                ) {
                    if let Some(identity_id) = &linked_sa_state.identity_resource_id {
                        identity_map.insert(
                            identity_id.clone(),
                            UserAssignedIdentity {
                                client_id: None,
                                principal_id: None,
                            },
                        );
                    }
                }
            }
        }

        // Configure registry credentials for image pull.
        // The image URI points at the manager's registry (proxy URI from release).
        // Add Basic auth with the deployment token so the Container App can pull.
        let registry_token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "deployment_token is required for Azure Container Apps to pull images from the registry proxy".to_string(),
                resource_id: Some(func.id.clone()),
            })
        })?;
        let registry_server = image.split('/').next().unwrap_or_default().to_string();
        let secret_name = "registry-proxy-password";
        secrets.push(Secret {
            name: Some(secret_name.to_string()),
            value: Some(registry_token.clone()),
            identity: None,
            key_vault_url: None,
        });
        registries.push(RegistryCredentials {
            identity: None,
            password_secret_ref: Some(secret_name.to_string()),
            server: Some(registry_server),
            username: Some("deployment".to_string()),
        });

        // Create managed identity spec if we have any identities
        let identity_resource_ids: Vec<String> = identity_map.keys().cloned().collect();

        let managed_identity = if !identity_map.is_empty() {
            Some(ManagedServiceIdentity {
                principal_id: None,
                tenant_id: None,
                type_: ManagedServiceIdentityType::UserAssigned,
                user_assigned_identities: Some(UserAssignedIdentities(identity_map)),
            })
        } else {
            None
        };

        // Configure Dapr if the worker uses any triggers or commands.
        // Dapr handles delivery for queue (Service Bus), storage (blob), and cron triggers.
        let needs_dapr = func.commands_enabled || !func.triggers.is_empty();
        let dapr_config = if needs_dapr {
            use alien_azure_clients::models::container_apps::{Dapr, DaprAppProtocol};

            Some(Dapr {
                app_id: Some(container_app_name.to_string()),
                app_port: Some(8080), // Port that alien-worker-runtime listens on
                app_protocol: DaprAppProtocol::Http,
                enable_api_logging: Some(false),
                enabled: true,
                http_max_request_size: None,
                http_read_buffer_size: None,
                log_level: None,
            })
        } else {
            None
        };

        let configuration = Configuration {
            active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
            dapr: dapr_config,
            identity_settings: identity_resource_ids
                .iter()
                .map(|identity_id| IdentitySettings {
                    identity: identity_id.clone(),
                    lifecycle: IdentitySettingsLifecycle::All,
                })
                .collect(),
            ingress: ingress_cfg,
            max_inactive_revisions: None,
            registries,
            runtime: None,
            secrets,
            service: None,
        };

        let template = Template {
            containers: vec![container],
            init_containers: vec![],
            revision_suffix: None,
            scale: Some(Scale {
                cooldown_period: None,
                max_replicas: func.concurrency_limit.map(|c| c as i32).unwrap_or(10),
                min_replicas: Some(if func.public_endpoints.is_empty() {
                    0
                } else {
                    1
                }),
                polling_interval: None,
                rules: vec![],
            }),
            service_binds: vec![],
            termination_grace_period_seconds: None,
            volumes: vec![],
        };

        Ok(ContainerApp {
            extended_location: None,
            id: None,
            identity: managed_identity,
            location: location.to_string(),
            managed_by: None,
            name: None,
            properties: Some(ContainerAppProperties {
                configuration: Some(configuration),
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: Some(environment_id),
                outbound_ip_addresses: vec![],
                provisioning_state: None,
                running_status: None,
                template: Some(template),
                workload_profile_name: None,
            }),
            system_data: None,
            tags,
            type_: None,
        })
    }

    /// Creates a Dapr Service Bus component for a queue trigger
    async fn create_dapr_service_bus_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<DaprComponentOperation> {
        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get queue controller to access Service Bus namespace
        let queue_controller =
            ctx.require_dependency::<crate::queue::azure::AzureQueueController>(queue_ref)?;
        let namespace = queue_controller.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let component_name =
            get_azure_queue_trigger_dapr_component_name(container_app_name, &queue_ref.id);

        // Use Dapr input binding — the manager/user code sends directly to Service Bus
        // via Azure SDK, not through Dapr pubsub. Input bindings auto-deliver from the
        // named queue without requiring GET /dapr/subscribe subscriptions.
        let queue_name = queue_controller.queue_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id,
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let client_id = service_account
            .identity_client_id
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: service_account_ref.id,
                })
            })?;
        let dapr_component = service_bus_dapr_component(
            component_name.clone(),
            container_app_name,
            namespace,
            queue_name.clone(),
            client_id,
        );

        info!(
            worker=%worker_config.id,
            queue=%queue_ref.id,
            component=%component_name,
            environment=%environment_name,
            "Creating Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match delete_owned_legacy_dapr_components(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &component_name,
            &get_legacy_azure_queue_trigger_dapr_component_names(container_app_name, &queue_ref.id),
            &worker_config.id,
        )
        .await?
        {
            LegacyDaprComponentCleanupStep::Complete => {}
            LegacyDaprComponentCleanupStep::Mutated => {
                return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
            }
            LegacyDaprComponentCleanupStep::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|delay| delay.as_secs());
                return Ok(DaprComponentOperation::Deleting(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        match ensure_dapr_component(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            "Successfully created Dapr Service Bus component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates supported Azure storage-trigger delivery:
    /// Event Grid -> dedicated Service Bus queue -> Dapr Service Bus input binding.
    async fn create_azure_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<DaprComponentOperation> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let environment_resource_group = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let desired_target = self
            .desired_storage_trigger_target(ctx, worker_config, container_app_name, storage_ref)
            .await?;
        let desired_infrastructure = &desired_target.infrastructure;
        let event_subscription_name = desired_infrastructure.event_subscription_name.clone();
        let namespace_name = desired_infrastructure.namespace_name.clone();
        let queue_name = desired_infrastructure.queue_name.clone();

        let component_name =
            get_azure_blob_trigger_dapr_component_name(container_app_name, &storage_ref.id);

        if matches!(
            self.prepare_storage_trigger_target(ctx, desired_infrastructure)
                .await?,
            StorageTargetPreparation::Pending
        ) {
            return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
        }
        match self
            .ensure_storage_delivery_infrastructure(
                ctx,
                worker_config,
                storage_ref,
                events,
                &desired_target,
            )
            .await?
        {
            StorageDeliveryReconcileResult::Complete => {}
            StorageDeliveryReconcileResult::Pending(delay) => {
                return Ok(DaprComponentOperation::Pending(delay));
            }
        }

        let dapr_component = service_bus_dapr_component(
            component_name.clone(),
            container_app_name,
            &namespace_name,
            queue_name.clone(),
            &desired_target.execution_client_id,
        );

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match delete_owned_legacy_dapr_components(
            container_apps_client.as_ref(),
            &environment_resource_group,
            &environment_name,
            container_app_name,
            &component_name,
            &get_legacy_azure_blob_trigger_dapr_component_names(
                container_app_name,
                &storage_ref.id,
            ),
            &worker_config.id,
        )
        .await?
        {
            LegacyDaprComponentCleanupStep::Complete => {}
            LegacyDaprComponentCleanupStep::Mutated => {
                return Ok(DaprComponentOperation::Pending(Duration::from_secs(1)));
            }
            LegacyDaprComponentCleanupStep::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|delay| delay.as_secs());
                return Ok(DaprComponentOperation::Deleting(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        match ensure_dapr_component(
            container_apps_client.as_ref(),
            &environment_resource_group,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            subscription=%event_subscription_name,
            "Azure storage trigger delivery is ready"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates a Dapr cron input binding for a schedule trigger
    async fn create_dapr_cron_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        cron: &str,
        index: usize,
    ) -> Result<DaprComponentOperation> {
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let component_name =
            get_azure_dapr_component_name(&format!("cron-{container_app_name}-{index}"));

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.cron".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: vec![
                    DaprMetadata {
                        name: Some("schedule".into()),
                        value: Some(cron.to_string()),
                        secret_ref: None,
                    },
                    DaprMetadata {
                        name: Some("direction".into()),
                        value: Some("input".into()),
                        secret_ref: None,
                    },
                ],
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match ensure_dapr_component(
            client.as_ref(),
            &resource_group_name,
            &environment_name,
            container_app_name,
            &dapr_component,
            &worker_config.id,
        )
        .await?
        {
            DaprComponentEnsureOperation::Unchanged | DaprComponentEnsureOperation::Completed => {}
            DaprComponentEnsureOperation::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::Creating(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            schedule=%cron,
            "Successfully created Dapr cron component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Deletes tracked trigger components without touching a foreign component
    /// that happens to share a historical name.
    async fn delete_all_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<TrackedDaprComponentDeleteStep> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let Some(component_name) = self.dapr_components.first().cloned() else {
            return Ok(TrackedDaprComponentDeleteStep::Complete);
        };
        let step = self
            .delete_tracked_dapr_component(
                ctx,
                &container_app_name,
                &worker_config.id,
                &component_name,
            )
            .await?;
        if matches!(step, TrackedDaprComponentDeleteStep::Mutated) {
            self.dapr_components.retain(|name| name != &component_name);
            if self.pending_dapr_component_deletion_name.as_deref() == Some(component_name.as_str())
            {
                self.pending_dapr_component_deletion_name = None;
            }
        }
        Ok(step)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: AzureWorkerState::Ready,
            container_app_name: Some(function_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                function_name
            )),
            url: Some(format!("https://{}.azurecontainerapps.io", function_name)),
            container_app_url: None,
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            storage_trigger_infrastructure: Vec::new(),
            storage_trigger_teardown_progress: AzureStorageTriggerTeardownProgress::default(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_resource_group_name: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_dapr_component_deletion_candidates: Vec::new(),
            commands_sender_role_assignment_id: None,
            commands_sender_role_assignment_intent: None,
            commands_sender_role_assignment_discovery_complete: false,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
            dapr_component_naming_version: CURRENT_DAPR_COMPONENT_NAMING_VERSION,
            pending_dapr_component_deletion_name: None,
            dapr_component_deletion_candidates_initialized: false,
            auxiliary_teardown_candidates_initialized: false,
            commands_update_teardown_candidates_initialized: false,
            trigger_update_teardown_candidates_initialized: false,
            storage_delivery_update_reconciliation_initialized: false,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Worker Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    mod lro_routing_tests {
        use super::*;
        include!("azure_lro_routing_tests.rs");
    }

    mod commands_target_tests {
        use super::*;
        include!("azure_commands_target_tests.rs");
    }

    mod storage_target_tests {
        use super::*;
        include!("azure_storage_target_tests.rs");
        include!("azure_storage_delivery_update_tests.rs");
    }

    mod state_persistence_tests {
        use super::*;
        include!("azure_state_persistence_tests.rs");
    }

    mod operation_recovery_tests {
        use super::*;
        include!("azure_operation_recovery_tests.rs");
    }

    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    use alien_azure_clients::models::container_apps::{
        Configuration, ConfigurationActiveRevisionsMode, ContainerApp, ContainerAppProperties,
        ContainerAppPropertiesProvisioningState, TrafficWeight,
    };
    use alien_azure_clients::{
        authorization::MockAuthorizationApi,
        container_apps::MockContainerAppsApi,
        event_grid::MockEventGridApi,
        long_running_operation::{
            LongRunningOperation, MockLongRunningOperationApi, OperationResult,
        },
        service_bus::MockServiceBusManagementApi,
    };
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{Platform, ResourceStatus, Worker, WorkerOutputs, WorkerTrigger};
    use alien_error::{AlienError, ContextError};
    use httpmock::MockServer;
    use rstest::rstest;

    use super::{
        commands_queue_name, current_unix_timestamp_secs, dns_name_from_url,
        get_azure_internal_commands_dapr_component_name, AzureCommandsSenderRoleAssignmentIntent,
        AzureStorageTriggerTeardownProgress, AZURE_RBAC_WAIT_POLL_SECS,
    };
    use crate::core::{
        controller_test::{test_storage_1, SingleControllerExecutor},
        MockPlatformServiceProvider,
    };
    use crate::error::ErrorData;
    use crate::infra_requirements::azure_utils::is_azure_authorization_propagation_error;
    use crate::worker::azure::trigger_targets::azure_storage_event_types;
    use crate::worker::azure_dapr_components::service_bus_dapr_component;
    use crate::worker::azure_dapr_names_migration::CURRENT_DAPR_COMPONENT_NAMING_VERSION;
    use crate::worker::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock,
        AzureWorkerController,
    };
    use crate::AzureWorkerState;

    #[test]
    fn azure_storage_trigger_maps_only_supported_event_types() {
        assert_eq!(
            azure_storage_event_types(
                &[
                    "created".to_string(),
                    "deleted".to_string(),
                    "tierChanged".to_string(),
                ],
                "worker",
            )
            .unwrap(),
            vec![
                "Microsoft.Storage.BlobCreated",
                "Microsoft.Storage.BlobDeleted",
                "Microsoft.Storage.BlobTierChanged",
            ]
        );
        assert!(azure_storage_event_types(&["metadataUpdated".to_string()], "worker").is_err());
    }

    #[test]
    fn legacy_controller_state_defaults_dapr_naming_version_to_zero() {
        let mut serialized = serde_json::to_value(AzureWorkerController::mock_ready("worker"))
            .expect("controller state should serialize");
        serialized
            .as_object_mut()
            .expect("controller state should be an object")
            .remove("daprComponentNamingVersion");

        let controller: AzureWorkerController =
            serde_json::from_value(serialized).expect("legacy controller state should deserialize");

        assert_eq!(controller.dapr_component_naming_version, 0);
    }

    #[tokio::test]
    async fn ready_legacy_controller_enters_dapr_name_migration() {
        let mut controller = AzureWorkerController::mock_ready("worker");
        controller.dapr_component_naming_version = 0;
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();

        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::MigratingDaprComponentNames
        );
    }

    #[tokio::test]
    async fn delete_polls_pending_migration_operation_before_component_cleanup() {
        let operation_polled = Arc::new(AtomicBool::new(false));
        let operation_polled_by_lro = operation_polled.clone();
        let mut lro = MockLongRunningOperationApi::new();
        lro.expect_check_status()
            .times(1)
            .returning(move |_, _, _| {
                operation_polled_by_lro.store(true, Ordering::SeqCst);
                Ok(Some("completed".to_string()))
            });

        let operation_polled_by_cleanup = operation_polled.clone();
        let mut container_apps = MockContainerAppsApi::new();
        container_apps
            .expect_get_dapr_component()
            .times(1)
            .returning(move |_, _, component_name| {
                assert!(
                    operation_polled_by_cleanup.load(Ordering::SeqCst),
                    "pending migration must finish before component cleanup starts"
                );
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Dapr component".to_string(),
                        resource_name: component_name.to_string(),
                    },
                ))
            });

        let provider = setup_mock_service_provider(Arc::new(container_apps), Some(Arc::new(lro)));
        let mut controller = AzureWorkerController::mock_ready("worker-app");
        controller.state = AzureWorkerState::WaitingForDaprComponentNameMigrationOperation;
        controller.pending_operation_url =
            Some("https://management.azure.com/operations/migrate-dapr".to_string());
        controller.dapr_component_naming_version = 0;
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::WaitingForPendingOperationBeforeDelete
        );

        executor.step().await.unwrap();
        assert!(operation_polled.load(Ordering::SeqCst));
        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::DeleteStart
        );

        executor.step().await.unwrap();
        executor.step().await.unwrap();
        executor.step().await.unwrap();
        executor.step().await.unwrap();
    }

    #[tokio::test]
    async fn delete_drains_pending_operation_before_no_app_fast_path() {
        let mut lro = MockLongRunningOperationApi::new();
        lro.expect_check_status()
            .times(1)
            .returning(|_, _, _| Ok(Some("completed".to_string())));
        let provider =
            setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
        let mut controller = AzureWorkerController::mock_ready("worker-app");
        controller.container_app_name = None;
        controller.pending_operation_url =
            Some("https://management.azure.com/operations/create-app".to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::WaitingForPendingOperationBeforeDelete
        );
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::DeleteStart
        );
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::Deleted
        );
    }

    #[tokio::test]
    async fn imported_storage_teardown_orders_event_role_queue_before_dapr() {
        let order = Arc::new(AtomicUsize::new(0));

        let order_for_dapr = order.clone();
        let mut container_apps = MockContainerAppsApi::new();
        container_apps
            .expect_get_dapr_component()
            .times(1..)
            .returning(move |_, _, component_name| {
                assert_eq!(
                    order_for_dapr.load(Ordering::SeqCst),
                    3,
                    "storage delivery infrastructure must be removed before Dapr"
                );
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Dapr component".to_string(),
                        resource_name: component_name.to_string(),
                    },
                ))
            });

        let order_for_event = order.clone();
        let mut event_grid = MockEventGridApi::new();
        event_grid
            .expect_delete_event_subscription()
            .times(1)
            .returning(move |_, _| {
                assert_eq!(order_for_event.fetch_add(1, Ordering::SeqCst), 0);
                Ok(())
            });

        let order_for_role = order.clone();
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_build_role_assignment_id()
            .times(1)
            .returning(|_, name| format!("/roleAssignments/{name}"));
        authorization
            .expect_delete_role_assignment_by_id()
            .times(1)
            .returning(move |_| {
                assert_eq!(order_for_role.fetch_add(1, Ordering::SeqCst), 1);
                Ok(None)
            });
        let authorization = Arc::new(authorization);

        let order_for_queue = order.clone();
        let mut service_bus = MockServiceBusManagementApi::new();
        service_bus
            .expect_delete_queue()
            .times(1)
            .returning(move |_, _, _| {
                assert_eq!(order_for_queue.fetch_add(1, Ordering::SeqCst), 2);
                Ok(())
            });

        let mut provider = MockPlatformServiceProvider::new();
        let container_apps = Arc::new(container_apps);
        provider
            .expect_get_azure_container_apps_client()
            .returning(move |_| Ok(container_apps.clone()));
        let event_grid = Arc::new(event_grid);
        provider
            .expect_get_azure_event_grid_client()
            .returning(move |_| Ok(event_grid.clone()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        let service_bus = Arc::new(service_bus);
        provider
            .expect_get_azure_service_bus_management_client()
            .returning(move |_| Ok(service_bus.clone()));

        let storage = test_storage_1();
        let mut worker = basic_function();
        worker.triggers.push(WorkerTrigger::storage(
            &storage,
            vec!["created".to_string()],
        ));
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::mock_ready("worker-app"))
            .platform(Platform::Azure)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        for _ in 0..7 {
            executor.step().await.unwrap();
        }

        assert_eq!(order.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn imported_custom_domain_deletes_deterministic_certificate_without_tracked_id() {
        let worker = function_public_ingress();
        let mut custom_domains = std::collections::HashMap::new();
        custom_domains.insert(
            worker.id.clone(),
            alien_core::CustomDomainConfig {
                domain: "worker.example.com".to_string(),
                certificate: alien_core::CustomCertificateConfig {
                    azure: Some(alien_core::AzureCustomCertificateConfig {
                        key_vault_certificate_id: "https://vault.example/certificates/worker"
                            .to_string(),
                        key_vault_resource_id: None,
                    }),
                    ..Default::default()
                },
            },
        );
        let stack_settings = alien_core::StackSettings {
            domains: Some(alien_core::DomainSettings {
                custom_domains: Some(custom_domains),
                public_endpoint_target: None,
            }),
            ..Default::default()
        };
        let mut container_apps = MockContainerAppsApi::new();
        container_apps
            .expect_delete_managed_environment_certificate()
            .times(1)
            .returning(|_, _, _| Ok(OperationResult::Completed(())));
        let provider = setup_mock_service_provider(Arc::new(container_apps), None);
        let mut controller = AzureWorkerController::mock_ready("worker-app");
        controller.state = AzureWorkerState::DeletingCertificate;
        controller.container_apps_certificate_id = None;
        controller.uses_custom_domain = false;
        controller.fqdn = Some("worker.example.com".to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(controller)
            .platform(Platform::Azure)
            .stack_settings(stack_settings)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();

        assert_eq!(
            executor
                .internal_state::<AzureWorkerController>()
                .unwrap()
                .state,
            AzureWorkerState::Deleted
        );
    }

    #[tokio::test]
    async fn completed_update_operation_clears_persisted_lro_cursor() {
        let mut lro = MockLongRunningOperationApi::new();
        lro.expect_check_status()
            .times(1)
            .returning(|_, _, _| Ok(Some("completed".to_string())));
        let provider =
            setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
        let mut controller = AzureWorkerController::mock_ready("worker-app");
        controller.state = AzureWorkerState::WaitingForUpdateOperation;
        controller.pending_operation_url =
            Some("https://management.azure.com/operations/update-app".to_string());
        controller.pending_operation_retry_after = Some(15);
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();

        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert_eq!(controller.state, AzureWorkerState::UpdatingContainerApp);
        assert!(controller.pending_operation_url.is_none());
        assert!(controller.pending_operation_retry_after.is_none());
    }

    #[test]
    fn strips_scheme_and_path_from_dns_endpoint_url() {
        assert_eq!(
            dns_name_from_url("https://app.example.azurecontainerapps.io/health"),
            "app.example.azurecontainerapps.io"
        );
        assert_eq!(
            dns_name_from_url("app.example.azurecontainerapps.io."),
            "app.example.azurecontainerapps.io"
        );
    }

    #[test]
    fn platform_domain_outputs_target_container_app_host_not_public_fqdn() {
        let mut controller = AzureWorkerController::mock_ready("test-worker");
        controller.fqdn = Some("test-worker.public.example.com".to_string());
        controller.certificate_id = Some("cert_123".to_string());
        controller.url = Some("https://test-worker.azurecontainerapps.io".to_string());

        let outputs = controller.build_outputs().unwrap();
        let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        let endpoint = worker_outputs
            .public_endpoints
            .get("default")
            .expect("default public endpoint");

        assert_eq!(
            endpoint.url.as_str(),
            "https://test-worker.azurecontainerapps.io"
        );
        assert_eq!(
            endpoint
                .load_balancer_endpoint
                .as_ref()
                .map(|endpoint| endpoint.dns_name.as_str()),
            Some("test-worker.azurecontainerapps.io")
        );
    }

    #[test]
    fn dns_target_is_ingress_host_when_url_is_overridden_to_public_fqdn() {
        // Regression: when `url` is overridden to the public display FQDN (from `public_urls`), the
        // CNAME target must still be the Container App ingress host. Otherwise the record name (the
        // public FQDN) and the target collide into a self-referential CNAME, which the DNS provider
        // rejects — the bug that deadlocked the Azure worker in `waitingForDns`.
        let mut controller = AzureWorkerController::mock_ready("test-worker");
        controller.url = Some("https://test-worker.abc123.dev.vpc.direct".to_string());
        controller.container_app_url =
            Some("https://test-worker.kindsky.eastus2.azurecontainerapps.io".to_string());

        let outputs = controller.build_outputs().unwrap();
        let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        let endpoint = worker_outputs
            .public_endpoints
            .get("default")
            .expect("default public endpoint");

        // Display URL stays the public FQDN.
        assert_eq!(
            endpoint.url.as_str(),
            "https://test-worker.abc123.dev.vpc.direct"
        );
        // The CNAME target is the ingress host — and crucially NOT the record's own public FQDN.
        let dns_name = endpoint
            .load_balancer_endpoint
            .as_ref()
            .map(|endpoint| endpoint.dns_name.as_str());
        assert_eq!(
            dns_name,
            Some("test-worker.kindsky.eastus2.azurecontainerapps.io")
        );
        assert_ne!(dns_name, Some("test-worker.abc123.dev.vpc.direct"));
    }

    #[tokio::test]
    async fn imported_worker_heartbeat_rebuilds_ingress_host_for_dns() {
        // Regression for the create-path-only gap: an imported worker starts Ready with
        // `container_app_url = None` and `url` = the public display FQDN (the importer skips the
        // create flow). The heartbeat must rebuild `container_app_url` from the live Container App,
        // so the DNS CNAME targets the ingress host rather than the self-referential public FQDN.
        let app_name = "test-imported-worker";
        let mut mock = MockContainerAppsApi::new();
        mock.expect_get_container_app()
            .returning(move |_, _| Ok(create_successful_container_app_response(app_name, true)))
            .times(0..);
        let mock_provider = setup_mock_service_provider(Arc::new(mock), None);

        // Imported shape: ingress host unset, url is the public display FQDN.
        let mut controller = AzureWorkerController::mock_ready(app_name);
        controller.container_app_url = None;
        controller.url = Some("https://test-imported-worker.abc123.dev.vpc.direct".to_string());

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        // The heartbeat rebuilt the ingress host…
        assert_eq!(
            controller.container_app_url.as_deref(),
            Some("https://test-imported-worker.azurecontainerapps.io")
        );
        // …so build_outputs targets it, NOT the public display FQDN.
        let outputs = controller.build_outputs().unwrap();
        let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        let endpoint = worker_outputs
            .public_endpoints
            .get("default")
            .expect("default public endpoint");
        let dns_name = endpoint
            .load_balancer_endpoint
            .as_ref()
            .map(|endpoint| endpoint.dns_name.as_str());
        assert_eq!(dns_name, Some("test-imported-worker.azurecontainerapps.io"));
        assert_ne!(dns_name, Some("test-imported-worker.abc123.dev.vpc.direct"));
    }

    #[test]
    fn detects_azure_authorization_propagation_error_from_http_context() {
        let http_error = AlienError::new(CloudClientErrorData::HttpResponseError {
            message: "Azure CreateOrUpdateDaprComponent failed: HTTP 403 Forbidden".to_string(),
            url: "https://management.azure.com/test".to_string(),
            http_status: 403,
            http_request_text: None,
            http_response_text: Some(
                "{\"error\":{\"code\":\"AuthorizationFailed\",\"message\":\"The client does not have authorization to perform action. If access was recently granted, please refresh your credentials.\"}}"
                    .to_string(),
            ),
        });

        let error = http_error.context(ErrorData::CloudPlatformError {
            message: "Failed to create commands Dapr component".to_string(),
            resource_id: Some("alien-rs-fn".to_string()),
        });

        assert!(is_azure_authorization_propagation_error(&error));
    }

    #[test]
    fn ignores_non_authorization_cloud_platform_errors() {
        let error = AlienError::new(ErrorData::CloudPlatformError {
            message: "Failed to create commands Dapr component".to_string(),
            resource_id: Some("alien-rs-fn".to_string()),
        });

        assert!(!is_azure_authorization_propagation_error(&error));
    }

    fn create_successful_container_app_response(app_name: &str, has_url: bool) -> ContainerApp {
        let fqdn = if has_url {
            Some(format!("{}.azurecontainerapps.io", app_name))
        } else {
            None
        };

        let ingress = if has_url {
            Some(alien_azure_clients::models::container_apps::Ingress {
                external: true,
                target_port: Some(8080),
                fqdn: fqdn.clone(),
                traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
                    latest_revision: true,
                    weight: Some(100),
                    revision_name: None,
                    label: None,
                }],
                transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
                allow_insecure: false,
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
            })
        } else {
            None
        };

        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: vec![],
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    fn create_in_progress_container_app_response(app_name: &str) -> ContainerApp {
        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::InProgress),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
                configuration: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    fn setup_mock_client_for_creation_and_update(
        app_name: &str,
        has_url: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock successful app creation - immediate completion
        let app_name = app_name.to_string();
        let app_name_for_create = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_create, has_url),
                ))
            });

        // Mock successful updates - immediate completion
        let app_name_for_update = app_name.clone();
        mock_container_apps
            .expect_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_update, has_url),
                ))
            });

        // Mock get operations - may be called multiple times during creation and update flows
        let app_name_for_get = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get,
                    has_url,
                ))
            })
            .times(0..); // Allow 0 or more calls

        Arc::new(mock_container_apps)
    }

    fn setup_mock_client_for_creation_and_deletion(
        app_name: &str,
        has_url: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock successful app creation - immediate completion
        let app_name = app_name.to_string();
        let app_name_for_create = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_create, has_url),
                ))
            });

        // Mock successful deletion - immediate completion
        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Mock get operations during creation (may be called multiple times)
        let app_name_for_get_creation = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get_creation,
                    has_url,
                ))
            })
            .times(0..); // Allow 0 or more calls during creation

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        expect_dapr_components_missing(&mut mock_container_apps);
        Arc::new(mock_container_apps)
    }

    fn setup_mock_client_for_long_running_creation(
        app_name: &str,
        has_url: bool,
    ) -> (Arc<MockContainerAppsApi>, Arc<MockLongRunningOperationApi>) {
        let mut mock_container_apps = MockContainerAppsApi::new();
        let mut mock_lro = MockLongRunningOperationApi::new();

        // Mock creation that starts as long-running
        // Use minimal retry_after for fast tests (actual Azure would use seconds)
        let app_name = app_name.to_string();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(|_, _, _| {
                Ok(OperationResult::LongRunning(LongRunningOperation {
                    url: "https://management.azure.com/subscriptions/.../operations/test-op"
                        .to_string(),
                    retry_after: Some(Duration::from_millis(10)),
                    location_url: None,
                }))
            });

        // Mock LRO polling - first incomplete, then complete
        mock_lro
            .expect_check_status()
            .returning(|_, _, _| Ok(None)) // Still running
            .times(1);

        mock_lro
            .expect_check_status()
            .returning(|_, _, _| Ok(Some("completed".to_string()))) // Completed
            .times(1);

        // Mock get operations showing progression
        let app_name_for_get1 = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_in_progress_container_app_response(
                    &app_name_for_get1,
                ))
            })
            .times(1);

        let app_name_for_get2 = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get2,
                    has_url,
                ))
            });

        (Arc::new(mock_container_apps), Arc::new(mock_lro))
    }

    fn setup_mock_client_for_best_effort_deletion(
        _app_name: &str,
        app_missing: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock deletion (might fail if app missing)
        if app_missing {
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "ContainerApp".to_string(),
                            resource_name: "test-app".to_string(),
                        },
                    ))
                });
        } else {
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| Ok(OperationResult::Completed(())));
        }

        // Always return not found for final status check
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        expect_dapr_components_missing(&mut mock_container_apps);
        Arc::new(mock_container_apps)
    }

    fn setup_mock_service_provider(
        mock_container_apps: Arc<MockContainerAppsApi>,
        mock_lro: Option<Arc<MockLongRunningOperationApi>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_container_apps_client()
            .returning(move |_| Ok(mock_container_apps.clone()));

        if let Some(lro_client) = mock_lro {
            mock_provider
                .expect_get_azure_long_running_operation_client()
                .returning(move |_| Ok(lro_client.clone()));
        }

        // Mock Azure authorization client for resource-scoped permissions
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(|_| {
                use alien_azure_clients::authorization::MockAuthorizationApi;
                let mut mock_auth = MockAuthorizationApi::new();
                mock_auth
                    .expect_create_or_update_role_definition()
                    .returning(|_, _, role_def| Ok(role_def.clone()));
                mock_auth
                    .expect_build_role_assignment_id()
                    .returning(|_, name| {
                        format!(
                            "/test/providers/Microsoft.Authorization/roleAssignments/{}",
                            name
                        )
                    });
                mock_auth
                    .expect_create_or_update_role_assignment_by_id()
                    .returning(|_, role_assignment| Ok(role_assignment.clone()));
                mock_auth
                    .expect_delete_role_assignment_by_id()
                    .returning(|_| Ok(None));
                Ok(Arc::new(mock_auth))
            });

        Arc::new(mock_provider)
    }

    fn setup_commands_toggle_provider(
        container_apps: Arc<MockContainerAppsApi>,
        service_bus: Arc<MockServiceBusManagementApi>,
        role_assignment_created: Option<Arc<AtomicBool>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_container_apps_client()
            .returning(move |_| Ok(container_apps.clone()));
        provider
            .expect_get_azure_service_bus_management_client()
            .returning(move |_| Ok(service_bus.clone()));
        provider
            .expect_get_azure_caller_principal_id()
            .returning(|_| Ok("test-manager-principal".to_string()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| {
                let mut authorization = MockAuthorizationApi::new();
                authorization
                    .expect_create_or_update_role_definition()
                    .returning(|_, _, role_definition| Ok(role_definition.clone()));
                authorization
                    .expect_build_role_assignment_id()
                    .returning(|_, name| {
                        format!("/test/providers/Microsoft.Authorization/roleAssignments/{name}")
                    });
                let role_assignment_created = role_assignment_created.clone();
                authorization
                    .expect_create_or_update_role_assignment_by_id()
                    .returning(move |_, role_assignment| {
                        if let Some(created) = &role_assignment_created {
                            created.store(true, Ordering::SeqCst);
                        }
                        Ok(role_assignment.clone())
                    });
                authorization
                    .expect_delete_role_assignment_by_id()
                    .returning(|_| Ok(None));
                Ok(Arc::new(authorization))
            });
        Arc::new(provider)
    }

    /// Sets up mock Container Apps client and optional readiness probe mock server
    /// Returns (container_apps_mock_provider, optional_mock_server)
    fn setup_mocks_for_function(
        worker: &Worker,
        app_name: &str,
        for_deletion: bool,
    ) -> (Arc<MockPlatformServiceProvider>, Option<MockServer>) {
        let has_url = !worker.public_endpoints.is_empty();
        let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(worker))
        } else {
            None
        };

        // Set up Container Apps client mock - create custom response if we need to override URL
        let container_apps_mock = if needs_readiness_probe && mock_server.is_some() {
            // Create custom mock that returns the mock server URL
            let mock_server_url = mock_server.as_ref().unwrap().base_url();
            setup_mock_client_with_custom_url(app_name, &mock_server_url, for_deletion)
        } else if for_deletion {
            setup_mock_client_for_creation_and_deletion(app_name, has_url)
        } else {
            setup_mock_client_for_creation_and_update(app_name, has_url)
        };

        let mock_provider = setup_mock_service_provider(container_apps_mock, None);

        (mock_provider, mock_server)
    }

    fn setup_mock_client_with_custom_url(
        app_name: &str,
        custom_url: &str,
        for_deletion: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Create a container app response with custom URL
        let custom_response = create_container_app_with_custom_url(app_name, custom_url);

        // Mock successful app creation
        let response_for_create = custom_response.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| Ok(OperationResult::Completed(response_for_create.clone())));

        if for_deletion {
            // Mock successful deletion
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| Ok(OperationResult::Completed(())));

            // Mock get operations during creation (may be called multiple times)
            let response_for_get_creation = custom_response.clone();
            mock_container_apps
                .expect_get_container_app()
                .returning(move |_, _| Ok(response_for_get_creation.clone()))
                .times(0..);

            // Mock get operation failure for deletion verification
            mock_container_apps
                .expect_get_container_app()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "ContainerApp".to_string(),
                            resource_name: "test-app".to_string(),
                        },
                    ))
                })
                .times(0..);
            expect_dapr_components_missing(&mut mock_container_apps);
        } else {
            // Mock successful updates
            let response_for_update = custom_response.clone();
            mock_container_apps
                .expect_update_container_app()
                .returning(move |_, _, _| {
                    Ok(OperationResult::Completed(response_for_update.clone()))
                });

            // Mock get operations (may be called multiple times)
            let response_for_get = custom_response.clone();
            mock_container_apps
                .expect_get_container_app()
                .returning(move |_, _| Ok(response_for_get.clone()))
                .times(0..);
        }

        Arc::new(mock_container_apps)
    }

    fn expect_dapr_components_missing(mock_container_apps: &mut MockContainerAppsApi) {
        mock_container_apps
            .expect_get_dapr_component()
            .returning(|_, _, component_name| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Dapr component".to_string(),
                        resource_name: component_name.to_string(),
                    },
                ))
            })
            .times(0..);
    }

    fn create_container_app_with_custom_url(app_name: &str, custom_url: &str) -> ContainerApp {
        // For tests, just extract the host and port from the URL string
        let url_without_protocol = custom_url.strip_prefix("http://").unwrap_or(custom_url);
        let (host, _port) = if let Some(colon_pos) = url_without_protocol.find(':') {
            let host = &url_without_protocol[..colon_pos];
            let port_str = &url_without_protocol[colon_pos + 1..];
            let port = port_str.parse::<u16>().unwrap_or(80);
            (host, Some(port))
        } else {
            (url_without_protocol, None)
        };

        // Create FQDN that matches the custom URL
        let _fqdn = if let Some(port) = _port {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        };

        let ingress = Some(alien_azure_clients::models::container_apps::Ingress {
            external: true,
            target_port: Some(8080),
            fqdn: Some(custom_url.to_string()), // Use the full URL as FQDN for the test
            traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
                latest_revision: true,
                weight: Some(100),
                revision_name: None,
                label: None,
            }],
            transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
            allow_insecure: false,
            additional_port_mappings: vec![],
            custom_domains: vec![],
            ip_security_restrictions: vec![],
            cors_policy: None,
            client_certificate_mode: None,
            exposed_port: None,
            sticky_sessions: None,
        });

        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: vec![],
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    async fn executor_for_wait_state(
        controller: AzureWorkerController,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_pre_container_app_rbac_wait_holds_state_when_woken_early() {
        let deadline = current_unix_timestamp_secs().saturating_add(60);
        let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
        controller.state = AzureWorkerState::WaitingBeforeContainerAppCreation;
        controller.pre_container_app_rbac_wait_until_epoch_secs = Some(deadline);

        let mut executor = executor_for_wait_state(controller).await;
        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Provisioning);
        assert_eq!(
            controller.state,
            AzureWorkerState::WaitingBeforeContainerAppCreation
        );
        assert_eq!(
            step_result.suggested_delay,
            Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
        );
        assert_eq!(
            controller.pre_container_app_rbac_wait_until_epoch_secs,
            Some(deadline)
        );
    }

    #[tokio::test]
    async fn test_ready_rbac_wait_holds_state_when_woken_early() {
        let deadline = current_unix_timestamp_secs().saturating_add(60);
        let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
        controller.state = AzureWorkerState::WaitingForRbacPropagation;
        controller.ready_rbac_wait_until_epoch_secs = Some(deadline);

        let mut executor = executor_for_wait_state(controller).await;
        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Provisioning);
        assert_eq!(
            controller.state,
            AzureWorkerState::WaitingForRbacPropagation
        );
        assert_eq!(
            step_result.suggested_delay,
            Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
        );
        assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
    }

    #[tokio::test]
    async fn test_ready_rbac_wait_advances_after_deadline() {
        let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
        controller.state = AzureWorkerState::WaitingForRbacPropagation;
        controller.ready_rbac_wait_until_epoch_secs =
            Some(current_unix_timestamp_secs().saturating_sub(1));

        let mut executor = executor_for_wait_state(controller).await;
        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Provisioning);
        assert_eq!(controller.state, AzureWorkerState::RunningReadinessProbe);
        assert_eq!(step_result.suggested_delay, None);
        assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);

        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(controller.state, AzureWorkerState::Ready);
        assert_eq!(step_result.suggested_delay, None);
    }

    #[tokio::test]
    async fn test_update_rbac_wait_holds_and_clears() {
        let deadline = current_unix_timestamp_secs().saturating_add(60);
        let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
        controller.state = AzureWorkerState::UpdateWaitingForRbacPropagation;
        controller.ready_rbac_wait_until_epoch_secs = Some(deadline);
        controller.update_rbac_wait_required = true;

        let mut executor = executor_for_wait_state(controller).await;
        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Updating);
        assert_eq!(
            controller.state,
            AzureWorkerState::UpdateWaitingForRbacPropagation
        );
        assert_eq!(
            step_result.suggested_delay,
            Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
        );
        assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
        assert!(controller.update_rbac_wait_required);

        let mut controller = controller.clone();
        controller.ready_rbac_wait_until_epoch_secs =
            Some(current_unix_timestamp_secs().saturating_sub(1));
        let mut executor = executor_for_wait_state(controller).await;
        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Updating);
        assert_eq!(
            controller.state,
            AzureWorkerState::UpdateRunningReadinessProbe
        );
        assert_eq!(step_result.suggested_delay, None);
        assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);
        assert!(!controller.update_rbac_wait_required);

        let step_result = executor.step().await.unwrap();
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(controller.state, AzureWorkerState::Ready);
        assert_eq!(step_result.suggested_delay, None);
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function())]
    #[case::env_vars(function_with_env_vars())]
    #[case::storage_link(function_with_storage_link())]
    #[case::env_and_storage(function_with_env_and_storage())]
    #[case::multiple_storages(function_with_multiple_storages())]
    #[case::public_ingress(function_public_ingress())]
    #[case::private_ingress(function_private_ingress())]
    #[case::concurrency(function_with_concurrency())]
    #[case::custom_config(function_custom_config())]
    #[case::readiness_probe(function_with_readiness_probe())]
    #[case::complete_test(function_complete_test())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker) {
        let app_name = format!("test-{}", worker.id);
        let (mock_provider, _mock_server) = setup_mocks_for_function(&worker, &app_name, true);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.identifier.is_some());
        assert!(function_outputs.worker_name.starts_with("test-"));

        // Delete the worker
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_env(basic_function(), function_with_env_vars())]
    #[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
    #[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
    #[case::custom_to_public(function_custom_config(), function_public_ingress())]
    #[case::public_to_complete(function_public_ingress(), function_complete_test())]
    #[case::complete_to_basic(function_complete_test(), basic_function())]
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_function: Worker, #[case] to_function: Worker) {
        // Ensure both workers have the same ID for valid updates
        let worker_id = "test-update-worker".to_string();
        let mut from_function = from_function;
        from_function.id = worker_id.clone();

        let mut to_function = to_function;
        to_function.id = worker_id.clone();

        let app_name = format!("test-{}", worker_id);
        let (mock_provider, mock_server) = setup_mocks_for_function(&to_function, &app_name, false);

        // Start with the "from" worker in Ready state
        let mut ready_controller = AzureWorkerController::mock_ready(&app_name);

        // If the target worker has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        } else if !to_function.public_endpoints.is_empty() {
            // Ensure the controller has a URL for public workers
            ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new worker
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    #[tokio::test]
    async fn update_enables_commands_and_reconciles_partial_tracking() {
        let mut from_worker = basic_function();
        from_worker.id = "commands-toggle-worker".to_string();
        let mut to_worker = from_worker.clone();
        to_worker.commands_enabled = true;
        let app_name = "test-commands-toggle-worker";
        let component_name = get_azure_internal_commands_dapr_component_name(app_name);
        let desired_component = service_bus_dapr_component(
            component_name.clone(),
            app_name,
            "default-service-bus-namespace",
            commands_queue_name(app_name),
            "12345678-1234-1234-1234-123456789012",
        );
        let component_created = Arc::new(AtomicBool::new(false));

        let mut container_apps = MockContainerAppsApi::new();
        let app_name_for_update = app_name.to_string();
        container_apps
            .expect_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_update, false),
                ))
            });
        let app_name_for_get = app_name.to_string();
        container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get,
                    false,
                ))
            })
            .times(0..);
        let component_name_for_get = component_name.clone();
        let desired_for_get = desired_component.clone();
        let created_for_get = component_created.clone();
        container_apps
            .expect_get_dapr_component()
            .returning(move |_, _, name| {
                if name == component_name_for_get && created_for_get.load(Ordering::SeqCst) {
                    Ok(desired_for_get.clone())
                } else {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Dapr component".to_string(),
                            resource_name: name.to_string(),
                        },
                    ))
                }
            })
            .times(1..);
        let created_by_put = component_created.clone();
        container_apps
            .expect_create_or_update_dapr_component()
            .times(1)
            .returning(move |_, _, _, component| {
                created_by_put.store(true, Ordering::SeqCst);
                Ok(OperationResult::Completed(component.clone()))
            });

        let mut service_bus = MockServiceBusManagementApi::new();
        service_bus
            .expect_create_or_update_queue()
            .times(1..)
            .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
        let provider =
            setup_commands_toggle_provider(Arc::new(container_apps), Arc::new(service_bus), None);
        let mut controller = AzureWorkerController::mock_ready(app_name);
        controller.commands_namespace_name = Some("default-service-bus-namespace".to_string());
        let mut executor = SingleControllerExecutor::builder()
            .resource(from_worker)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(to_worker).unwrap();
        for step in 0..30 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.unwrap_or_else(|error| {
                let state = executor
                    .internal_state::<AzureWorkerController>()
                    .map(|controller| format!("{:?}", controller.state))
                    .unwrap_or_else(|| "unavailable".to_string());
                panic!("commands-enable update failed at step {step}, state {state}: {error}");
            });
        }

        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            controller.commands_namespace_name.as_deref(),
            Some("default-service-bus-namespace")
        );
        assert_eq!(
            controller.commands_queue_name.as_deref(),
            Some("test-commands-toggle-worker-rq")
        );
        assert_eq!(
            controller.commands_dapr_component.as_deref(),
            Some(component_name.as_str())
        );
    }

    #[tokio::test]
    async fn update_disables_imported_commands_without_touching_storage() {
        let mut from_worker = basic_function();
        from_worker.id = "commands-toggle-worker".to_string();
        from_worker.commands_enabled = true;
        let enabled_worker = from_worker.clone();
        let mut to_worker = from_worker.clone();
        to_worker.commands_enabled = false;
        let app_name = "test-commands-toggle-worker";
        let component_name = get_azure_internal_commands_dapr_component_name(app_name);
        let existing_component = service_bus_dapr_component(
            component_name.clone(),
            app_name,
            "default-service-bus-namespace",
            commands_queue_name(app_name),
            "12345678-1234-1234-1234-123456789012",
        );

        let mut container_apps = MockContainerAppsApi::new();
        let app_name_for_update = app_name.to_string();
        container_apps
            .expect_update_container_app()
            .times(2)
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_update, false),
                ))
            });
        let app_name_for_get = app_name.to_string();
        container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get,
                    false,
                ))
            })
            .times(0..);
        let component_name_for_get = component_name.clone();
        let existing_component_for_get = existing_component.clone();
        container_apps
            .expect_get_dapr_component()
            .times(2..)
            .returning(move |_, _, name| {
                if name == component_name_for_get {
                    Ok(existing_component_for_get.clone())
                } else {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Dapr component".to_string(),
                            resource_name: name.to_string(),
                        },
                    ))
                }
            });
        container_apps
            .expect_delete_dapr_component()
            .times(1)
            .returning(|_, _, _| Ok(OperationResult::Completed(())));
        container_apps
            .expect_create_or_update_dapr_component()
            .times(0);

        let mut service_bus = MockServiceBusManagementApi::new();
        service_bus
            .expect_delete_queue()
            .times(1)
            .returning(|_, _, _| Ok(()));
        service_bus
            .expect_create_or_update_queue()
            .times(1..)
            .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
        let role_assignment_created = Arc::new(AtomicBool::new(false));
        let provider = setup_commands_toggle_provider(
            Arc::new(container_apps),
            Arc::new(service_bus),
            Some(role_assignment_created.clone()),
        );
        let controller = AzureWorkerController::mock_ready(app_name);
        let mut executor = SingleControllerExecutor::builder()
            .resource(from_worker)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(to_worker).unwrap();
        for step in 0..30 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.unwrap_or_else(|error| {
                let state = executor
                    .internal_state::<AzureWorkerController>()
                    .map(|controller| format!("{:?}", controller.state))
                    .unwrap_or_else(|| "unavailable".to_string());
                panic!("commands-disable update failed at step {step}, state {state}: {error}");
            });
        }

        assert_eq!(executor.status(), ResourceStatus::Running);
        {
            let controller = executor.internal_state::<AzureWorkerController>().unwrap();
            assert!(controller.commands_namespace_name.is_none());
            assert!(controller.commands_queue_name.is_none());
            assert!(controller.commands_dapr_component.is_none());
            assert!(controller.storage_trigger_infrastructure.is_empty());
        }

        role_assignment_created.store(false, Ordering::SeqCst);
        executor.update(enabled_worker).unwrap();
        for step in 0..30 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.unwrap_or_else(|error| {
                let state = executor
                    .internal_state::<AzureWorkerController>()
                    .map(|controller| format!("{:?}", controller.state))
                    .unwrap_or_else(|| "unavailable".to_string());
                panic!("commands-reenable update failed at step {step}, state {state}: {error}");
            });
        }

        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(role_assignment_created.load(Ordering::SeqCst));
        assert!(controller.commands_sender_role_assignment_id.is_some());
        assert_eq!(
            controller.commands_dapr_component.as_deref(),
            Some(component_name.as_str())
        );
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_app(function_public_ingress(), true)]
    #[case::private_with_missing_app(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] worker: Worker,
        #[case] app_missing: bool,
    ) {
        let app_name = format!("test-{}", worker.id);
        let mock_container_apps =
            setup_mock_client_for_best_effort_deletion(&app_name, app_missing);
        let mock_provider = setup_mock_service_provider(mock_container_apps, None);

        // Start with a ready controller
        let mut ready_controller = AzureWorkerController::mock_ready(&app_name);
        if !worker.public_endpoints.is_empty() {
            ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the worker
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even when resources are missing
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── LONG RUNNING OPERATION TESTS ──────────────────────

    #[tokio::test]
    async fn test_long_running_creation_operation() {
        let worker = basic_function();
        let app_name = format!("test-{}", worker.id);
        let (mock_container_apps, mock_lro) =
            setup_mock_client_for_long_running_creation(&app_name, false);
        let mock_provider = setup_mock_service_provider(mock_container_apps, Some(mock_lro));

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify the controller went through LRO states
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert!(controller.container_app_name.is_some());
        assert!(controller.resource_id.is_some());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public workers get URL in outputs
    #[tokio::test]
    async fn test_public_function_gets_url_in_outputs() {
        let worker = function_public_ingress();
        let app_name = format!("test-{}", worker.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock creation with URL
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name, true),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        let endpoint = function_outputs
            .public_endpoints
            .get("default")
            .expect("default public endpoint");
        assert!(endpoint.url.contains("azurecontainerapps.io"));
    }

    /// Test that verifies private workers don't get URL in outputs
    #[tokio::test]
    async fn test_private_function_has_no_url_in_outputs() {
        let worker = function_private_ingress();
        let app_name = format!("test-{}", worker.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock creation without URL
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify no URL in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.public_endpoints.is_empty());
    }

    /// Test that verifies correct container app configuration parameters
    #[tokio::test]
    async fn test_container_app_configuration_validation() {
        let worker = function_custom_config();
        let app_name = format!("test-{}", worker.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Validate container app creation request has correct parameters
        let app_name_for_response = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .withf(|_rg, _name, container_app| {
                // Check that the container has correct resource configuration
                if let Some(properties) = &container_app.properties {
                    if let Some(template) = &properties.template {
                        if let Some(container) = template.containers.first() {
                            if let Some(resources) = &container.resources {
                                // function_custom_config has 512MB memory
                                let expected_memory = format!("{}Gi", 512.0 / 1024.0);
                                return resources.memory.as_ref() == Some(&expected_memory)
                                    && resources.cpu == Some(0.25);
                            }
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_response, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Allow get_container_app calls during creation (may be called 0 or more times)
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
            .times(0..);

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies environment variables are correctly passed
    #[tokio::test]
    async fn test_environment_variable_handling() {
        let worker = function_with_env_vars();
        let app_name = format!("test-{}", worker.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Validate container app creation request has environment variables
        let app_name_for_response = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .withf(|_rg, _name, container_app| {
                if let Some(properties) = &container_app.properties {
                    if let Some(template) = &properties.template {
                        if let Some(container) = template.containers.first() {
                            // Check that environment variables are present
                            let has_app_env = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("APP_ENV")
                                    && env_var.value.as_deref() == Some("production")
                            });
                            let has_log_level = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("LOG_LEVEL")
                                    && env_var.value.as_deref() == Some("debug")
                            });
                            let has_db_name = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("DB_NAME")
                                    && env_var.value.as_deref() == Some("myapp")
                            });
                            return has_app_env && has_log_level && has_db_name;
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_response, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Allow get_container_app calls during creation (may be called 0 or more times)
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
            .times(0..);

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AzureWorkerController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies deletion works when container_app_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_container_app_name_succeeds() {
        let worker = basic_function();

        // Create a controller with no container app name set (simulating early creation failure)
        let controller = AzureWorkerController {
            state: AzureWorkerState::CreateFailed,
            container_app_name: None, // This is the key - no container app name set
            resource_id: None,
            url: None,
            container_app_url: None,
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            storage_trigger_infrastructure: Vec::new(),
            storage_trigger_teardown_progress: AzureStorageTriggerTeardownProgress::default(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_resource_group_name: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_dapr_component_deletion_candidates: Vec::new(),
            commands_sender_role_assignment_id: None,
            commands_sender_role_assignment_intent: None,
            commands_sender_role_assignment_discovery_complete: false,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
            dapr_component_naming_version: CURRENT_DAPR_COMPONENT_NAMING_VERSION,
            pending_dapr_component_deletion_name: None,
            dapr_component_deletion_candidates_initialized: false,
            auxiliary_teardown_candidates_initialized: false,
            commands_update_teardown_candidates_initialized: false,
            trigger_update_teardown_candidates_initialized: false,
            storage_delivery_update_reconciliation_initialized: false,
            _internal_stay_count: None,
        };

        // Mock provider - no expectations since no API calls should be made
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Start in CreateFailed state
        assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

        // Delete the worker
        executor.delete().unwrap();

        // Run the delete flow - should succeed without making any API calls
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }
}
