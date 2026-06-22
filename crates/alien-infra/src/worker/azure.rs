use alien_core::{
    AzureClientConfig, AzureContainerAppsWorkerHeartbeatData, CertificateStatus, DnsRecordStatus,
    HeartbeatBackend, Ingress, ObservedHealth, Platform, ProviderLifecycleState,
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceRef, ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs,
    WorkloadHeartbeatStatus, ENV_AZURE_CLIENT_ID,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use base64::Engine;
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::core::EnvironmentVariableBuilder;
use crate::core::OperationResult;
use crate::core::{
    map_azure_core_021_delete_lro_response, map_azure_core_021_lro_response,
    map_azure_core_021_sdk_error, AzurePermissionsHelper, ResourceController,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_name, get_container_apps_environment_outputs,
    get_resource_group_name, is_azure_authorization_propagation_error,
};
use crate::worker::readiness_probe::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_macros::controller;
use azure_mgmt_app::package_preview_2024_08::models::{
    certificate, configuration, container_app, custom_domain, dapr, dapr_component,
    identity_settings, ingress, BaseContainer, Certificate, CertificateKeyVaultProperties,
    Configuration, Container, ContainerApp, ContainerResources, CustomDomain, Dapr, DaprComponent,
    DaprMetadata, EnvironmentVar, IdentitySettings, Ingress as AzureContainerAppsIngress,
    RegistryCredentials, Scale, Secret, Template, TrackedResource, TrafficWeight,
};
use azure_mgmt_servicebus::package_2024_01::models::{SbQueue, SbQueueProperties};

/// Generates a deterministic Azure Container Apps name for a worker.
fn get_azure_container_app_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

fn service_bus_queue_request(queue_name: &str) -> SbQueue {
    SbQueue {
        proxy_resource: azure_mgmt_servicebus::package_2024_01::models::ProxyResource {
            id: None,
            name: Some(queue_name.to_string()),
            type_: None,
            location: None,
        },
        properties: Some(SbQueueProperties::default()),
        system_data: None,
    }
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

async fn parse_azure_core_021_response_body_or_default_certificate(
    response: azure_core_021::Response,
    resource_type: &str,
    resource_name: &str,
) -> Result<Certificate> {
    let body = response
        .into_body()
        .collect()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read {resource_type} '{resource_name}' response"),
            resource_id: None,
        })?;

    if body.is_empty() {
        return Ok(Certificate::new(TrackedResource::new(String::new())));
    }

    serde_json::from_slice(&body)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to parse {resource_type} '{resource_name}' response"),
            resource_id: None,
        })
}

/// Domain information for a worker.
struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    keyvault_cert_id: Option<String>,
    container_apps_certificate_id: Option<String>,
    uses_custom_domain: bool,
}

enum DaprComponentOperation {
    Completed,
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
    let resources = container.and_then(|container| container.base_container.resources.as_ref());
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
                running_status: None,
                ingress_fqdn: ingress.and_then(|ingress| ingress.fqdn.clone()),
                min_replicas: scale.and_then(|scale| scale.min_replicas),
                max_replicas: scale.and_then(|scale| scale.max_replicas),
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

    /// URL returned by Azure ARM for *current* long‑running operation.
    pub(crate) pending_operation_url: Option<String>,
    /// Retry‑after seconds for the current LRO (populated when Azure returns it).
    pub(crate) pending_operation_retry_after: Option<u64>,
    /// Dapr component names for queue triggers (one per queue trigger)
    pub(crate) dapr_components: Vec<String>,

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
    /// Service Bus namespace name for commands delivery
    pub(crate) commands_namespace_name: Option<String>,
    /// Service Bus queue name for commands delivery
    pub(crate) commands_queue_name: Option<String>,
    /// Dapr component name for commands queue
    pub(crate) commands_dapr_component: Option<String>,
    /// Role assignment ID for Service Bus Data Sender on the deploying identity (for cleanup)
    pub(crate) commands_sender_role_assignment_id: Option<String>,
    /// Role assignment ID for Service Bus Data Receiver on the execution UAMI (for cleanup)
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                suggested_delay: Some(delay),
            });
        }
        info!(name=%func_cfg.id, "Initiating Azure Container App worker creation");
        if func_cfg.commands_enabled {
            match self
                .setup_commands_infrastructure(ctx, azure_cfg, func_cfg, &container_app_name)
                .await
            {
                Ok(DaprComponentOperation::Completed) => {
                    self.commands_infrastructure_auth_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_retry_after_epoch_secs = None;
                }
                Ok(DaprComponentOperation::LongRunning(delay)) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateCommandsDaprComponentOperation,
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
                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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
                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                            suggested_delay: Some(delay),
                        });
                    }

                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }

        // Wait in a real controller state for Azure RBAC propagation. A
        // suggested delay alone is only a scheduling hint and can be shortened
        // by other resources in the executor.
        info!(name=%func_cfg.id, "Commands infrastructure ready, waiting for RBAC propagation before creating Container App");
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
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for commands Dapr component"
                    .to_string(),
                operation: Some(
                    "waiting_for_pre_create_commands_dapr_component_operation".to_string(),
                ),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        Ok(HandlerAction::Continue {
            state: CreateStart,
            suggested_delay: None,
        })
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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
            .get_azure_container_apps_management_client(azure_cfg)?;
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
        let result = client
            .container_apps_client()
            .create_or_update(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                container_app_name.clone(),
                container_app,
            )
            .send()
            .await;
        let op_result = map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "container app create or update",
            "Azure Container App",
            &container_app_name,
            |response| response.into_body(),
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
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded in WaitingForCreateOperation"
                    .to_string(),
                operation: Some("waiting_for_create_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let container_app_name = self.container_app_name.as_ref().unwrap();
        let delay = self.pending_operation_retry_after.map(Duration::from_secs);
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        info!(name=%container_app_name, "Container App create accepted, checking resource status");
        Ok(HandlerAction::Continue {
            state: CreatingContainerApp,
            suggested_delay: delay,
        })
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
            .get_azure_container_apps_management_client(azure_cfg)?;

        let result = client
            .container_apps_client()
            .get(
                azure_cfg.subscription_id.clone(),
                resource_group_name,
                container_app_name.clone(),
            )
            .await;
        match map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "container app get",
            "Azure Container App",
            container_app_name,
        ) {
            Ok(app) => {
                if let Some(props) = &app.properties {
                    match props.provisioning_state.as_ref() {
                        Some(container_app::properties::ProvisioningState::Succeeded) => {
                            info!(name=%container_app_name, "Provisioning succeeded – configuring Dapr components");
                            self.handle_creation_completed(ctx, &app);

                            // Branch based on ingress type
                            // If public, resolve domain and proceed to certificate flow
                            // If private, skip directly to Dapr component configuration
                            if func_cfg.ingress == Ingress::Public {
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
                        Some(container_app::properties::ProvisioningState::InProgress) => {
                            debug!(name=%container_app_name, "Provisioning still in progress");
                            Ok(HandlerAction::Stay {
                                max_times: 60,
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                        Some(container_app::properties::ProvisioningState::Failed) => {
                            error!(name=%container_app_name, "Container app provisioning failed");
                            Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Container app provisioning failed".to_string(),
                                resource_id: Some(func_cfg.id.clone()),
                            }))
                        }
                        _ => Ok(HandlerAction::Stay {
                            max_times: 60,
                            suggested_delay: Some(Duration::from_secs(10)),
                        }),
                    }
                } else {
                    debug!(name=%container_app_name, "Properties missing – retry");
                    Ok(HandlerAction::Stay {
                        max_times: 60,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                debug!(name=%container_app_name, "Resource not yet visible – retry");
                Ok(HandlerAction::Stay {
                    max_times: 60,
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
                max_times: 60,
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
        let certificate = Certificate {
            tracked_resource: TrackedResource::new(location),
            properties: Some(certificate::Properties {
                value: Some(pkcs12_base64),
                password: Some(String::new()),
                certificate_key_vault_properties: None,
                ..Default::default()
            }),
        };

        let container_apps_management_client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_cfg)?;
        let result = container_apps_management_client
            .certificates_client()
            .create_or_update(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                certificate_name.clone(),
            )
            .certificate_envelope(certificate)
            .send()
            .await;
        let response = map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "managed environment certificate create or update",
            "Azure Container Apps Managed Environment Certificate",
            &certificate_name,
        )?;
        let response = parse_azure_core_021_response_body_or_default_certificate(
            response.into_raw_response(),
            "Azure Container Apps Managed Environment Certificate",
            &certificate_name,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to import certificate to Azure Container Apps Environment".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;

        self.container_apps_certificate_id =
            Some(response.tracked_resource.resource.id.ok_or_else(|| {
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
            .get_azure_container_apps_management_client(azure_cfg)?;

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
            let certificate = Certificate {
                tracked_resource: TrackedResource::new(
                    azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
                ),
                properties: Some(certificate::Properties {
                    value: None,
                    password: None,
                    certificate_key_vault_properties: Some(CertificateKeyVaultProperties {
                        identity: Some(rsm_outputs.management_resource_id.clone()),
                        key_vault_url: Some(keyvault_cert_id.clone()),
                    }),
                    ..Default::default()
                }),
            };
            let management_client = ctx
                .service_provider
                .get_azure_container_apps_management_client(azure_cfg)?;
            let result = management_client
                .certificates_client()
                .create_or_update(
                    azure_cfg.subscription_id.clone(),
                    resource_group_name.clone(),
                    environment_name.clone(),
                    certificate_name.clone(),
                )
                .certificate_envelope(certificate)
                .send()
                .await;
            let response = map_azure_core_021_sdk_error(
                "Azure Container Apps",
                result,
                "managed environment certificate create or update",
                "Azure Container Apps Managed Environment Certificate",
                &certificate_name,
            )?;
            let response = parse_azure_core_021_response_body_or_default_certificate(
                response.into_raw_response(),
                "Azure Container Apps Managed Environment Certificate",
                &certificate_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message:
                    "Failed to import Key Vault certificate to Azure Container Apps Environment"
                        .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.container_apps_certificate_id =
                Some(response.tracked_resource.resource.id.ok_or_else(|| {
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
        let result = client
            .container_apps_client()
            .create_or_update(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                container_app_name.clone(),
                app,
            )
            .send()
            .await;
        let _operation = map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "container app create or update",
            "Azure Container App",
            container_app_name,
            |response| response.into_body(),
        )
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
                max_times: 60,
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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
                                        max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    if let DaprComponentOperation::LongRunning(delay) = operation {
                        return Ok(HandlerAction::Continue {
                            state: WaitingForDaprComponentCreateOperation,
                            suggested_delay: Some(delay),
                        });
                    }
                    created_any = true;
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    info!(worker=%func_cfg.id, storage=%storage.id, "Creating Dapr blob storage component");
                    let operation = match self
                        .create_dapr_blob_storage_component(
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
                                        max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    if let DaprComponentOperation::LongRunning(delay) = operation {
                        return Ok(HandlerAction::Continue {
                            state: WaitingForDaprComponentCreateOperation,
                            suggested_delay: Some(delay),
                        });
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
                                        max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    if let DaprComponentOperation::LongRunning(delay) = operation {
                        return Ok(HandlerAction::Continue {
                            state: WaitingForDaprComponentCreateOperation,
                            suggested_delay: Some(delay),
                        });
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
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for Dapr component".to_string(),
                operation: Some("waiting_for_dapr_component_create_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        Ok(HandlerAction::Continue {
            state: ConfiguringDaprComponents,
            suggested_delay: None,
        })
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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

        // Commands infrastructure (queue, Dapr component, role assignments) is now
        // pre-created in CreateStart before the Container App, so the Dapr sidecar
        // starts with permissions already propagated. Skip if already done.
        if self.commands_namespace_name.is_some() {
            info!(worker=%func_cfg.id, "Commands infrastructure already created in CreateStart, skipping");
            return Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            });
        }

        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let env_resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get the Service Bus namespace from the dependent resource
        let namespace_ref = ResourceRef::new(
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

        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        // Create commands queue in the Service Bus namespace
        let queue_name = format!("{}-rq", container_app_name);
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        info!(
            worker=%func_cfg.id,
            namespace=%namespace_name,
            queue=%queue_name,
            "Creating commands Service Bus queue"
        );

        let result = mgmt
            .queues_client()
            .create_or_update(
                service_bus_resource_group.to_string(),
                namespace_name.to_string(),
                queue_name.to_string(),
                service_bus_queue_request(&queue_name),
                azure_config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "queue create or update",
            "Azure Service Bus queue",
            &queue_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        // Create Dapr component for commands queue
        let ns_fqdn = format!("{}.servicebus.windows.net", namespace_name);
        let component_name = format!("servicebus-{}-commands", container_app_name);

        // Use Dapr input binding (not pubsub) because the manager sends directly
        // to Service Bus via Azure SDK — this is external-system integration, not
        // Dapr-to-Dapr communication. Input bindings auto-deliver messages without
        // requiring GET /dapr/subscribe subscriptions.
        let mut metadata = vec![
            DaprMetadata {
                name: Some("namespaceName".into()),
                value: Some(ns_fqdn),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("queueName".into()),
                value: Some(queue_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("direction".into()),
                value: Some("input".into()),
                secret_ref: None,
            },
        ];

        // Add client ID for user-assigned managed identity
        let service_account_id = format!("{}-sa", func_cfg.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        if let Ok(sa_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &sa_state.identity_client_id {
                metadata.push(DaprMetadata {
                    name: Some("azureClientId".into()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        let dapr_component = DaprComponent {
            proxy_resource: Default::default(),
            properties: Some(dapr_component::Properties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: Some(false),
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![container_app_name.clone()],
                secret_store_component: None,
                secrets: Vec::new(),
                service_component_bind: Vec::new(),
            }),
        };

        info!(
            worker=%func_cfg.id,
            component=%component_name,
            "Creating commands Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        let result = client
            .dapr_components_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                env_resource_group_name.clone(),
                environment_name.clone(),
                component_name.clone(),
                dapr_component,
            )
            .send()
            .await;
        match map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "Dapr component create or update",
            "Azure Container Apps Dapr Component",
            &component_name,
            |response| response.into_body(),
        )
        .await
        {
            Ok(OperationResult::Completed(_)) => {
                self.container_apps_environment_wake_wait_until_epoch_secs = None;
                self.container_apps_environment_wake_retry_after_epoch_secs = None;
            }
            Ok(OperationResult::LongRunning(lro)) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(HandlerAction::Continue {
                    state: WaitingForCommandsDaprComponentOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                });
            }
            Err(e) => {
                let e = e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create commands Dapr component '{}'",
                        component_name
                    ),
                    resource_id: Some(func_cfg.id.clone()),
                });
                if is_azure_container_apps_environment_waking_error(&e) {
                    let deadline = ensure_rbac_wait_deadline(
                        &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                        AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                    );
                    if let Some(delay) = self.record_container_apps_environment_wake_retry(deadline)
                    {
                        warn!(
                            worker=%func_cfg.id,
                            error=%e,
                            remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                            "Azure Container Apps Environment is waking; retrying commands Dapr component"
                        );
                        return Ok(HandlerAction::Stay {
                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                            suggested_delay: Some(delay),
                        });
                    }
                }
                return Err(e);
            }
        }

        self.commands_namespace_name = Some(namespace_name.clone());
        self.commands_queue_name = Some(queue_name);
        self.commands_dapr_component = Some(component_name);

        // Verify that command transport permissions are part of the setup-applied
        // management profile. Live worker provisioning should not create RBAC grants.
        self.assign_commands_sender_role(
            ctx,
            azure_config,
            &service_bus_resource_group,
            &namespace_name,
            func_cfg,
        )
        .await?;

        info!(worker=%func_cfg.id, "Commands Service Bus infrastructure created");

        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
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
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for commands Dapr component"
                    .to_string(),
                operation: Some("waiting_for_commands_dapr_component_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
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
        if func_cfg.ingress != Ingress::Public {
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
                    max_times: READINESS_PROBE_MAX_ATTEMPTS as u32,
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
            use crate::core::Scope;

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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_cfg)?;

        // Heartbeat check: verify Container App still exists and is in correct state
        let result = client
            .container_apps_client()
            .get(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                container_app_name.clone(),
            )
            .await;
        let container_app = map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "container app get",
            "Azure Container App",
            container_app_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: "Failed to get Container App during heartbeat check".to_string(),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        // Verify Container App is in Succeeded state - drift is non-retryable
        if let Some(properties) = &container_app.properties {
            if properties.provisioning_state
                == Some(container_app::properties::ProvisioningState::Failed)
            {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: func_cfg.id.clone(),
                    message: "Container App is in Failed state".to_string(),
                }));
            }
        }

        // Check for certificate renewal on auto-managed public domains.
        if func_cfg.ingress == Ingress::Public && !self.uses_custom_domain {
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

        if func_cfg.ingress != Ingress::Public || self.uses_custom_domain {
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
        let certificate = Certificate {
            tracked_resource: TrackedResource::new(
                azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
            ),
            properties: Some(certificate::Properties {
                value: Some(pkcs12_base64),
                password: Some(String::new()),
                certificate_key_vault_properties: None,
                ..Default::default()
            }),
        };

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_cfg)?;
        let result = container_apps_client
            .certificates_client()
            .create_or_update(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                certificate_name.clone(),
            )
            .certificate_envelope(certificate)
            .send()
            .await;
        let response = map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "managed environment certificate create or update",
            "Azure Container Apps Managed Environment Certificate",
            &certificate_name,
        )?;
        let response = parse_azure_core_021_response_body_or_default_certificate(
            response.into_raw_response(),
            "Azure Container Apps Managed Environment Certificate",
            &certificate_name,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to re-import certificate to Azure Container Apps Environment"
                .to_string(),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        let container_apps_certificate_id =
            response.tracked_resource.resource.id.ok_or_else(|| {
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

            let container_apps_client = ctx
                .service_provider
                .get_azure_container_apps_management_client(azure_cfg)?;
            let result = container_apps_client
                .container_apps_client()
                .create_or_update(
                    azure_cfg.subscription_id.clone(),
                    resource_group_name.clone(),
                    container_app_name.clone(),
                    app,
                )
                .send()
                .await;
            map_azure_core_021_lro_response(
                "Azure Container Apps",
                result,
                "container app create or update",
                "Azure Container App",
                container_app_name,
                |response| response.into_body(),
            )
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
        let previous_cfg = ctx.previous_resource_config::<Worker>()?;
        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_dapr_components_deleted = false;
        if func_cfg == previous_cfg {
            self.update_rbac_wait_required = false;
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "container_app_name missing prior to update_start".to_string(),
                operation: Some("update_start".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_cfg)?;
        self.update_rbac_wait_required = true;

        // Build desired spec
        let mut desired_app = self
            .build_container_app(
                func_cfg,
                &environment_name,
                container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;
        if let (Some(fqdn), Some(keyvault_cert_id)) = (&self.fqdn, &self.keyvault_cert_id) {
            Self::set_custom_domain(&mut desired_app, fqdn.clone(), keyvault_cert_id.clone());
        }

        // Issue UPDATE
        let result = client
            .container_apps_client()
            .update(
                azure_cfg.subscription_id.clone(),
                resource_group_name.clone(),
                container_app_name.clone(),
                desired_app,
            )
            .send()
            .await;
        let op_result = map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "container app update",
            "Azure Container App",
            container_app_name,
            |response| response.into_body(),
        )
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
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded in WaitingForUpdateOperation"
                    .to_string(),
                operation: Some("waiting_for_update_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let container_app_name = self.container_app_name.as_ref().unwrap();
        let delay = self.pending_operation_retry_after.map(Duration::from_secs);
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        info!(name=%container_app_name, "Container App update accepted, checking resource status");
        Ok(HandlerAction::Continue {
            state: UpdatingContainerApp,
            suggested_delay: delay,
        })
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
            .get_azure_container_apps_management_client(azure_cfg)?;

        let result = client
            .container_apps_client()
            .get(
                azure_cfg.subscription_id.clone(),
                resource_group_name,
                container_app_name.clone(),
            )
            .await;
        let app = map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "container app get",
            "Azure Container App",
            container_app_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: "Error checking container app update status".to_string(),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        if let Some(props) = &app.properties {
            match props.provisioning_state.as_ref() {
                Some(container_app::properties::ProvisioningState::Succeeded) => {
                    info!(name=%container_app_name, "Update provisioning succeeded – updating Dapr components");

                    let container_app_url = self.extract_url_from_container_app(&app);

                    // Check for URL override in deployment config, otherwise use Container App URL
                    self.url = ctx
                        .deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(&func_cfg.id).cloned())
                        .or(container_app_url);

                    Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    })
                }
                Some(container_app::properties::ProvisioningState::InProgress) => {
                    Ok(HandlerAction::Stay {
                        max_times: 60,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
                Some(container_app::properties::ProvisioningState::Failed) => {
                    Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: "Container app update failed".to_string(),
                        resource_id: Some(func_cfg.id.clone()),
                    }))
                }
                _ => Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(10)),
                }),
            }
        } else {
            Ok(HandlerAction::Stay {
                max_times: 60,
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating Dapr components");

            if !self.update_dapr_components_deleted {
                // Trigger components are keyed by trigger shape. Delete the previous
                // set once, then recreate desired components across possible ARM LROs.
                self.delete_all_dapr_components(ctx).await?;
                self.update_dapr_components_deleted = true;
            }

            // Recreate components for ALL triggers
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
                                        &mut self
                                            .container_apps_environment_wake_wait_until_epoch_secs,
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
                                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                            suggested_delay: Some(delay),
                                        });
                                    }
                                }
                                return Err(e);
                            }
                        };
                        if let DaprComponentOperation::LongRunning(delay) = operation {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                    }
                    alien_core::WorkerTrigger::Storage { storage, events } => {
                        let operation = match self
                            .create_dapr_blob_storage_component(
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
                                        &mut self
                                            .container_apps_environment_wake_wait_until_epoch_secs,
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
                                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                            suggested_delay: Some(delay),
                                        });
                                    }
                                }
                                return Err(e);
                            }
                        };
                        if let DaprComponentOperation::LongRunning(delay) = operation {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
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
                                        &mut self
                                            .container_apps_environment_wake_wait_until_epoch_secs,
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
                                            max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
                                            suggested_delay: Some(delay),
                                        });
                                    }
                                }
                                return Err(e);
                            }
                        };
                        if let DaprComponentOperation::LongRunning(delay) = operation {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        cron_index += 1;
                    }
                }
            }
            self.container_apps_environment_wake_wait_until_epoch_secs = None;
            self.container_apps_environment_wake_retry_after_epoch_secs = None;
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
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
        state = WaitingForDaprComponentUpdateOperation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_update_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for Dapr component update".to_string(),
                operation: Some("waiting_for_dapr_component_update_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        Ok(HandlerAction::Continue {
            state: UpdateDaprComponents,
            suggested_delay: None,
        })
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
        if func_cfg.readiness_probe.is_none() || func_cfg.ingress != Ingress::Public {
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
            Ok(()) => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: READINESS_PROBE_MAX_ATTEMPTS as u32,
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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

        // Delete all Dapr components using best-effort approach (ignore NotFound)
        self.delete_all_dapr_components(ctx).await?;

        // Continue to commands infrastructure cleanup
        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
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
        let azure_config = ctx.get_azure_config()?;

        // Delete commands Dapr component (best-effort)
        if let Some(component_name) = self.commands_dapr_component.take() {
            let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_container_apps_management_client(azure_config)?;

            let result = client
                .dapr_components_client()
                .delete(
                    azure_config.subscription_id.clone(),
                    env_outputs.resource_group_name.clone(),
                    env_outputs.environment_name.clone(),
                    component_name.clone(),
                )
                .send()
                .await;
            match map_azure_core_021_delete_lro_response(
                "Azure Container Apps",
                result,
                "Dapr component delete",
                "Azure Container Apps Dapr Component",
                &component_name,
            )
            .await
            {
                Ok(_) => {
                    info!(component=%component_name, "Commands Dapr component delete requested");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(component=%component_name, "Commands Dapr component was already deleted");
                }
                Err(e) => {
                    warn!(
                        component=%component_name,
                        error=%e,
                        "Failed to delete commands Dapr component"
                    );
                }
            }
        }

        // Delete commands role assignments (best-effort)
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;

        if let Some(assignment_id) = self.commands_sender_role_assignment_id.take() {
            let result = authorization_client
                .role_assignments_client()
                .delete_by_id(assignment_id.clone())
                .send()
                .await;
            match map_azure_core_021_sdk_error(
                "Azure Authorization",
                result,
                "role assignment delete",
                "Azure role assignment",
                &assignment_id,
            )
            .map(|_| ())
            {
                Ok(_) => {
                    info!(assignment_id=%assignment_id, "Commands sender role assignment deleted");
                }
                Err(e) => {
                    warn!(
                        assignment_id=%assignment_id,
                        error=%e,
                        "Failed to delete commands sender role assignment (may already be deleted)"
                    );
                }
            }
        }

        // Delete commands receiver role assignment (best-effort)
        if let Some(assignment_id) = self.commands_receiver_role_assignment_id.take() {
            let result = authorization_client
                .role_assignments_client()
                .delete_by_id(assignment_id.clone())
                .send()
                .await;
            match map_azure_core_021_sdk_error(
                "Azure Authorization",
                result,
                "role assignment delete",
                "Azure role assignment",
                &assignment_id,
            )
            .map(|_| ())
            {
                Ok(_) => {
                    info!(assignment_id=%assignment_id, "Commands receiver role assignment deleted");
                }
                Err(e) => {
                    warn!(
                        assignment_id=%assignment_id,
                        error=%e,
                        "Failed to delete commands receiver role assignment (may already be deleted)"
                    );
                }
            }
        }

        // Delete commands Service Bus queue (best-effort)
        if let (Some(namespace_name), Some(queue_name)) = (
            self.commands_namespace_name.take(),
            self.commands_queue_name.take(),
        ) {
            let namespace_ref = ResourceRef::new(
                alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
                "default-service-bus-namespace",
            );
            let resource_group_name = match ctx
                .require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)
            {
                Ok(controller) => controller.resource_group_name(ctx)?,
                Err(_) => get_resource_group_name(ctx.state)?,
            };
            info!(namespace=%namespace_name, queue=%queue_name, "Deleting commands Service Bus queue");
            let mgmt = ctx
                .service_provider
                .get_azure_service_bus_management_client(azure_config)?;
            let result = mgmt
                .queues_client()
                .delete(
                    resource_group_name.to_string(),
                    namespace_name.to_string(),
                    queue_name.to_string(),
                    azure_config.subscription_id.clone(),
                )
                .send()
                .await
                .map(|_| ());
            match map_azure_core_021_sdk_error(
                "Azure Service Bus",
                result,
                "queue delete",
                "Azure Service Bus queue",
                &queue_name,
            ) {
                Ok(_) => {
                    info!(queue=%queue_name, "Commands Service Bus queue deleted");
                }
                Err(e) => {
                    warn!(
                        queue=%queue_name,
                        error=%e,
                        "Failed to delete commands Service Bus queue (may already be deleted)"
                    );
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingApp,
            suggested_delay: None,
        })
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
            .get_azure_container_apps_management_client(azure_cfg)?;

        let result = client
            .container_apps_client()
            .delete(
                azure_cfg.subscription_id.clone(),
                resource_group_name,
                container_app_name.clone(),
            )
            .send()
            .await;
        match map_azure_core_021_delete_lro_response(
            "Azure Container Apps",
            result,
            "container app delete",
            "Azure Container App",
            &container_app_name,
        )
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending_operation_url in WaitingForDeleteOperation".to_string(),
                operation: Some("waiting_for_delete_operation".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let container_app_name = self.container_app_name.as_ref().unwrap();
        let delay = self.pending_operation_retry_after.map(Duration::from_secs);
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        info!(name=%container_app_name, "Container App delete accepted, checking resource deletion status");
        Ok(HandlerAction::Continue {
            state: DeletingContainerApp,
            suggested_delay: delay,
        })
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
            .get_azure_container_apps_management_client(azure_cfg)?;

        let result = client
            .container_apps_client()
            .get(
                azure_cfg.subscription_id.clone(),
                resource_group_name,
                container_app_name.clone(),
            )
            .await;
        match map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "container app get",
            "Azure Container App",
            &container_app_name,
        ) {
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(name=%container_app_name, "Container app confirmed deleted");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                debug!(name=%container_app_name, "Container app still exists – retry");
                Ok(HandlerAction::Stay {
                    max_times: 60,
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
        if self.container_apps_certificate_id.is_none() {
            self.clear_all();
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_cfg)?;

        let result = client
            .certificates_client()
            .delete(
                azure_cfg.subscription_id.clone(),
                resource_group_name,
                environment_name,
                certificate_name.clone(),
            )
            .send()
            .await;
        match map_azure_core_021_delete_lro_response(
            "Azure Container Apps",
            result,
            "managed environment certificate delete",
            "Azure Container Apps Managed Environment Certificate",
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
        self.pending_operation_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending_operation_url in WaitingForCertificateDeleteOperation"
                    .to_string(),
                operation: Some("waiting_for_certificate_delete_operation".to_string()),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);

        let result = client
            .certificates_client()
            .get(
                azure_config.subscription_id.clone(),
                resource_group_name,
                environment_name,
                certificate_name.clone(),
            )
            .await;
        match map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "managed environment certificate get",
            "Azure Container Apps Managed Environment Certificate",
            &certificate_name,
        ) {
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                let delay = self
                    .pending_operation_retry_after
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(15));
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(delay),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Container Apps managed environment certificate deletion"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
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

    // Implementation of get_outputs trait method
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.resource_id.as_ref().map(|id| {
            let load_balancer_endpoint =
                self.url
                    .as_ref()
                    .map(|url| alien_core::LoadBalancerEndpoint {
                        dns_name: dns_name_from_url(url),
                        hosted_zone_id: None,
                    });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .container_app_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                url: self.url.clone(),
                identifier: Some(id.clone()),
                load_balancer_endpoint,
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
        azure_config: &AzureClientConfig,
        func_cfg: &alien_core::Worker,
        container_app_name: &str,
    ) -> Result<DaprComponentOperation> {
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
        let queue_name = format!("{}-rq", container_app_name);
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        info!(
            worker=%func_cfg.id,
            namespace=%namespace_name,
            queue=%queue_name,
            "Pre-creating commands Service Bus queue (before Container App)"
        );

        let result = mgmt
            .queues_client()
            .create_or_update(
                service_bus_resource_group.to_string(),
                namespace_name.to_string(),
                queue_name.to_string(),
                service_bus_queue_request(&queue_name),
                azure_config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "queue create or update",
            "Azure Service Bus queue",
            &queue_name,
        )
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        // Create Dapr component for commands queue
        let ns_fqdn = format!("{}.servicebus.windows.net", namespace_name);
        let component_name = format!("servicebus-{}-commands", container_app_name);

        let mut metadata = vec![
            DaprMetadata {
                name: Some("namespaceName".into()),
                value: Some(ns_fqdn),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("queueName".into()),
                value: Some(queue_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("direction".into()),
                value: Some("input".into()),
                secret_ref: None,
            },
        ];

        // Add client ID for user-assigned managed identity
        let service_account_id = format!("{}-sa", func_cfg.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        if let Ok(sa_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &sa_state.identity_client_id {
                metadata.push(DaprMetadata {
                    name: Some("azureClientId".into()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        let dapr_component = DaprComponent {
            proxy_resource: Default::default(),
            properties: Some(dapr_component::Properties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: Some(false),
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: Vec::new(),
                service_component_bind: Vec::new(),
            }),
        };

        info!(
            worker=%func_cfg.id,
            component=%component_name,
            "Pre-creating commands Dapr Service Bus component (before Container App)"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        let result = client
            .dapr_components_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                env_resource_group_name.clone(),
                environment_name.clone(),
                component_name.clone(),
                dapr_component,
            )
            .send()
            .await;
        match map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "Dapr component create or update",
            "Azure Container Apps Dapr Component",
            &component_name,
            |response| response.into_body(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Dapr component '{}'",
                component_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        self.commands_namespace_name = Some(namespace_name.clone());
        self.commands_queue_name = Some(queue_name);
        self.commands_dapr_component = Some(component_name);

        // Command transport RBAC is setup-applied before live worker provisioning.
        self.assign_commands_sender_role(
            ctx,
            azure_config,
            &service_bus_resource_group,
            &namespace_name,
            func_cfg,
        )
        .await?;

        info!(worker=%func_cfg.id, "Commands infrastructure pre-created successfully");
        Ok(DaprComponentOperation::Completed)
    }

    /// Ensure command transport permissions are represented in the management profile.
    ///
    /// Azure command transport uses Service Bus data-plane roles. Those grants are
    /// setup-owned because both Terraform setup and runtime setup know the stack
    /// management and execution identities before live workers are created.
    async fn assign_commands_sender_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &AzureClientConfig,
        resource_group_name: &str,
        namespace_name: &str,
        func_cfg: &alien_core::Worker,
    ) -> Result<()> {
        if !management_profile_dispatches_commands(ctx, &func_cfg.id) {
            info!(
                worker = %func_cfg.id,
                "Skipping management command sender role because worker/dispatch-command is not granted"
            );
            return Ok(());
        }

        if AzurePermissionsHelper::get_management_uami_principal_id(ctx)?.is_none() {
            if self.commands_sender_role_assignment_id.is_some() {
                return Ok(());
            }

            let queue_name = self.commands_queue_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: "commands-queue".to_string(),
                })
            })?;
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;
            let principal_id = ctx
                .service_provider
                .get_azure_caller_principal_id(azure_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to resolve Azure command sender principal".to_string(),
                    resource_id: Some(func_cfg.id.clone()),
                })?;
            let queue_scope = crate::core::Scope::Resource {
                resource_group_name: resource_group_name.to_string(),
                resource_provider: "Microsoft.ServiceBus".to_string(),
                parent_resource_path: Some(format!("namespaces/{namespace_name}")),
                resource_type: "queues".to_string(),
                resource_name: queue_name.clone(),
            };
            let role_assignment_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!(
                    "deployment:azure:commands-sender:{}:{}:{}:{}:{}",
                    ctx.resource_prefix, func_cfg.id, principal_id, namespace_name, queue_name
                )
                .as_bytes(),
            )
            .to_string();
            let full_assignment_id = format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{}",
                queue_scope.to_scope_string(azure_config),
                role_assignment_id
            );
            let role_definition_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/69a216fc-b8fb-44d8-bc22-1f3c2cd27a39",
                azure_config.subscription_id
            );

            AzurePermissionsHelper::create_role_assignment(
                &authorization_client,
                azure_config,
                &queue_scope,
                &role_assignment_id,
                &principal_id,
                &role_definition_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to grant Azure command sender role".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

            self.commands_sender_role_assignment_id = Some(full_assignment_id);
            info!(
                worker = %func_cfg.id,
                principal_id = %principal_id,
                namespace = %namespace_name,
                queue = %queue_name,
                "Granted direct-manager Azure Service Bus command sender role"
            );
            return Ok(());
        }

        info!(
            worker = %func_cfg.id,
            "Using setup-applied Azure Service Bus command permissions"
        );

        Ok(())
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
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(resource_id).cloned())
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
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        self.dapr_components.clear();
    }

    /// Called whenever provisioning *succeeds* and we have the live resource.
    fn handle_creation_completed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        app: &ContainerApp,
    ) {
        self.resource_id = app.tracked_resource.resource.id.clone();

        let container_app_url = self.extract_url_from_container_app(app);

        // Check for URL override in deployment config, otherwise use Container App URL
        if let Ok(config) = ctx.desired_resource_config::<Worker>() {
            self.url = ctx
                .deployment_config
                .public_urls
                .as_ref()
                .and_then(|urls| urls.get(&config.id).cloned())
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
                        binding_type: Some(custom_domain::BindingType::SniEnabled),
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
            .add_worker_runtime_env_vars(ctx, &func.id)?
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

        // Add Azure-specific managed identity client ID
        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &service_account_state.identity_client_id {
                env_vars.push(EnvironmentVar {
                    name: Some(ENV_AZURE_CLIENT_ID.to_string()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

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
            base_container: BaseContainer {
                name: Some("main".to_string()),
                image: Some(image.clone()),
                image_type: None,
                resources: Some(ContainerResources {
                    cpu: Some(cpu),
                    memory: Some(format!("{}Gi", memory_gi)),
                    ephemeral_storage: None,
                }),
                env: env_vars,
                args: vec![],
                command: vec![],
                volume_mounts: vec![],
            },
            probes: vec![],
        };

        // Tags for traceability
        let mut tags = HashMap::new();
        tags.insert("resource-type".to_string(), "worker".to_string());
        tags.insert("resource".to_string(), func.id.clone());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

        let _resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_id = azure_utils::get_container_apps_environment_resource_id(ctx.state)?;

        let ingress_cfg = if func.ingress == Ingress::Public {
            Some(AzureContainerAppsIngress {
                external: Some(true),
                target_port: Some(8080),
                traffic: vec![TrafficWeight {
                    weight: Some(100),
                    latest_revision: Some(true),
                    revision_name: None,
                    label: None,
                }],
                transport: Some(ingress::Transport::Auto),
                allow_insecure: Some(false),
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
                target_port_http_scheme: None,
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
        let mut identity_map = serde_json::Map::new();

        // Add permission-based ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(identity_id) = &service_account_state.identity_resource_id {
                identity_map.insert(identity_id.clone(), serde_json::json!({}));
            }
        }

        // Add linked ServiceAccounts
        for link in &func.links {
            if link.resource_type() == &alien_core::ServiceAccount::RESOURCE_TYPE {
                if let Ok(linked_sa_state) = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                    link,
                ) {
                    if let Some(identity_id) = &linked_sa_state.identity_resource_id {
                        identity_map.insert(identity_id.clone(), serde_json::json!({}));
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

        let identity_resource_ids: Vec<String> = identity_map.keys().cloned().collect();

        // Configure Dapr if the worker uses any triggers or commands.
        // Dapr handles delivery for queue (Service Bus), storage (blob), and cron triggers.
        let needs_dapr = func.commands_enabled || !func.triggers.is_empty();
        let dapr_config = if needs_dapr {
            Some(Dapr {
                app_id: Some(container_app_name.to_string()),
                app_port: Some(8080), // Port that alien-runtime listens on
                app_protocol: Some(dapr::AppProtocol::Http),
                enable_api_logging: Some(false),
                enabled: Some(true),
                http_max_request_size: None,
                http_read_buffer_size: None,
                log_level: None,
            })
        } else {
            None
        };

        let configuration = Configuration {
            active_revisions_mode: Some(configuration::ActiveRevisionsMode::Single),
            dapr: dapr_config,
            identity_settings: identity_resource_ids
                .iter()
                .map(|identity_id| IdentitySettings {
                    identity: identity_id.clone(),
                    lifecycle: Some(identity_settings::Lifecycle::All),
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
                max_replicas: Some(func.concurrency_limit.map(|c| c as i32).unwrap_or(10)),
                min_replicas: Some(if func.ingress == Ingress::Private {
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
            tracked_resource: {
                let mut tracked_resource = TrackedResource::new(location.to_string());
                tracked_resource.tags = Some(serde_json::json!(tags));
                tracked_resource
            },
            extended_location: None,
            identity: None,
            managed_by: None,
            kind: None,
            properties: Some(container_app::Properties {
                configuration: Some(configuration),
                managed_environment_id: Some(environment_id),
                template: Some(template),
                ..Default::default()
            }),
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
        let ns_fqdn = format!("{}.servicebus.windows.net", namespace);

        // Generate component name: servicebus-{containerAppName}-{queueId}
        let component_name = format!("servicebus-{}-{}", container_app_name, queue_ref.id);

        // Use Dapr input binding — the manager/user code sends directly to Service Bus
        // via Azure SDK, not through Dapr pubsub. Input bindings auto-deliver from the
        // named queue without requiring GET /dapr/subscribe subscriptions.
        let queue_name = queue_controller.queue_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        let dapr_component = DaprComponent {
            proxy_resource: Default::default(),
            properties: Some(dapr_component::Properties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: Some(false),
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: {
                    let mut metadata = vec![
                        DaprMetadata {
                            name: Some("namespaceName".into()),
                            value: Some(ns_fqdn),
                            secret_ref: None,
                        },
                        DaprMetadata {
                            name: Some("queueName".into()),
                            value: Some(queue_name.clone()),
                            secret_ref: None,
                        },
                        DaprMetadata {
                            name: Some("direction".into()),
                            value: Some("input".into()),
                            secret_ref: None,
                        },
                    ];

                    // Add client ID for user-assigned managed identity
                    let service_account_id = format!("{}-sa", worker_config.get_permissions());
                    let service_account_ref = alien_core::ResourceRef::new(
                        alien_core::ServiceAccount::RESOURCE_TYPE,
                        service_account_id.to_string(),
                    );

                    if let Ok(service_account_state) = ctx.require_dependency::<crate::service_account::AzureServiceAccountController>(&service_account_ref) {
                        if let Some(client_id) = &service_account_state.identity_client_id {
                            metadata.push(DaprMetadata {
                                name: Some("azureClientId".into()),
                                value: Some(client_id.clone()),
                                secret_ref: None
                            });
                        }
                    }

                    metadata
                },
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: Vec::new(),
                service_component_bind: Vec::new(),
            }),
        };

        info!(
            worker=%worker_config.id,
            queue=%queue_ref.id,
            component=%component_name,
            environment=%environment_name,
            "Creating Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(&azure_config)?;

        let result = client
            .dapr_components_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                component_name.clone(),
                dapr_component,
            )
            .send()
            .await;
        match map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "Dapr component create or update",
            "Azure Container Apps Dapr Component",
            &component_name,
            |response| response.into_body(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create Dapr component '{}' for queue '{}'",
                component_name, queue_ref.id
            ),
            resource_id: Some(worker_config.id.clone()),
        })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
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

    /// Creates a Dapr blob storage input binding for a storage trigger
    async fn create_dapr_blob_storage_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        _events: &[String],
    ) -> Result<DaprComponentOperation> {
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get storage controller to access storage account and container names
        let storage_controller =
            ctx.require_dependency::<crate::storage::azure::AzureStorageController>(storage_ref)?;
        let storage_account_name = storage_controller
            .storage_account_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: storage_ref.id.clone(),
                })
            })?;
        let container_name = storage_controller.container_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: storage_ref.id.clone(),
            })
        })?;

        let component_name = format!("blobstorage-{}-{}", container_app_name, storage_ref.id);

        let mut metadata = vec![
            DaprMetadata {
                name: Some("storageAccount".into()),
                value: Some(storage_account_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("container".into()),
                value: Some(container_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("direction".into()),
                value: Some("input".into()),
                secret_ref: None,
            },
        ];

        // Add client ID for user-assigned managed identity
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &service_account_state.identity_client_id {
                metadata.push(DaprMetadata {
                    name: Some("azureClientId".into()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        let dapr_component = DaprComponent {
            proxy_resource: Default::default(),
            properties: Some(dapr_component::Properties {
                component_type: Some("bindings.azure.blobstorage".to_string()),
                ignore_errors: Some(false),
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: Vec::new(),
                service_component_bind: Vec::new(),
            }),
        };

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(&azure_config)?;

        let result = client
            .dapr_components_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                component_name.clone(),
                dapr_component,
            )
            .send()
            .await;
        match map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "Dapr component create or update",
            "Azure Container Apps Dapr Component",
            &component_name,
            |response| response.into_body(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create Dapr blob storage component '{}' for storage '{}'",
                component_name, storage_ref.id
            ),
            resource_id: Some(worker_config.id.clone()),
        })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
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
            "Successfully created Dapr blob storage component"
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
        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let component_name = format!("cron-{}-{}", container_app_name, index);

        let dapr_component = DaprComponent {
            proxy_resource: Default::default(),
            properties: Some(dapr_component::Properties {
                component_type: Some("bindings.cron".to_string()),
                ignore_errors: Some(false),
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
                secrets: Vec::new(),
                service_component_bind: Vec::new(),
            }),
        };

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(&azure_config)?;

        let result = client
            .dapr_components_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                component_name.clone(),
                dapr_component,
            )
            .send()
            .await;
        match map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "Dapr component create or update",
            "Azure Container Apps Dapr Component",
            &component_name,
            |response| response.into_body(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create Dapr cron component '{}' with schedule '{}'",
                component_name, cron
            ),
            resource_id: Some(worker_config.id.clone()),
        })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
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

    /// Deletes all Dapr components using best-effort approach
    async fn delete_all_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if self.dapr_components.is_empty() {
            return Ok(());
        }

        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(&azure_config)?;

        for component_name in &self.dapr_components.clone() {
            let result = client
                .dapr_components_client()
                .delete(
                    azure_config.subscription_id.clone(),
                    resource_group_name.clone(),
                    environment_name.clone(),
                    component_name.clone(),
                )
                .send()
                .await;
            match map_azure_core_021_delete_lro_response(
                "Azure Container Apps",
                result,
                "Dapr component delete",
                "Azure Container Apps Dapr Component",
                component_name,
            )
            .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        component=%component_name,
                        "Dapr component delete requested"
                    );
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    info!(
                        worker=%worker_config.id,
                        component=%component_name,
                        "Dapr component was already deleted or doesn't exist"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        component=%component_name,
                        error=%e,
                        "Failed to delete Dapr component during deletion"
                    );
                }
            }
        }

        self.dapr_components.clear();
        Ok(())
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
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_sender_role_assignment_id: None,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Worker Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::core::{azure_credential_from_config, AzureCore021Credential};
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{
        AzureClientConfig, AzureCredentials, Ingress, Platform, ResourceStatus, Worker,
        WorkerOutputs,
    };
    use alien_error::{AlienError, ContextError};
    use azure_core_021::{
        headers::Headers, Body, BytesStream, ClientOptions, Context, Method, Policy, PolicyResult,
        Request, Response, StatusCode, TransportOptions,
    };
    use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
    use azure_mgmt_app::package_preview_2024_08::models::{
        configuration, container_app, ingress, Configuration, ContainerApp,
        Ingress as AzureContainerAppsIngress, TrackedResource, TrafficWeight,
    };
    use azure_mgmt_authorization::package_2022_04_01 as azure_authorization_2022_04;
    use httpmock::MockServer;
    use rstest::rstest;
    use serde_json::json;

    use super::{current_unix_timestamp_secs, dns_name_from_url, AZURE_RBAC_WAIT_POLL_SECS};
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use crate::error::ErrorData;
    use crate::infra_requirements::azure_utils::is_azure_authorization_propagation_error;
    use crate::worker::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock,
        AzureWorkerController,
    };
    use crate::AzureWorkerState;

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

        assert_eq!(
            worker_outputs.url.as_deref(),
            Some("https://test-worker.azurecontainerapps.io")
        );
        assert_eq!(
            worker_outputs
                .load_balancer_endpoint
                .as_ref()
                .map(|endpoint| endpoint.dns_name.as_str()),
            Some("test-worker.azurecontainerapps.io")
        );
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
            Some(AzureContainerAppsIngress {
                external: Some(true),
                target_port: Some(8080),
                fqdn: fqdn.clone(),
                traffic: vec![TrafficWeight {
                    latest_revision: Some(true),
                    weight: Some(100),
                    revision_name: None,
                    label: None,
                }],
                transport: Some(ingress::Transport::Auto),
                allow_insecure: Some(false),
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
                target_port_http_scheme: None,
            })
        } else {
            None
        };

        ContainerApp {
            tracked_resource: {
                let mut tracked_resource = TrackedResource::new("East US".to_string());
                tracked_resource.resource.id = Some(format!(
                    "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                    app_name
                ));
                tracked_resource.resource.name = Some(app_name.to_string());
                tracked_resource
            },
            properties: Some(container_app::Properties {
                provisioning_state: Some(container_app::properties::ProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: Some(configuration::ActiveRevisionsMode::Single),
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: Vec::new(),
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                ..Default::default()
            }),
            extended_location: None,
            identity: None,
            kind: None,
            managed_by: None,
        }
    }

    fn create_in_progress_container_app_response(app_name: &str) -> ContainerApp {
        ContainerApp {
            tracked_resource: {
                let mut tracked_resource = TrackedResource::new("East US".to_string());
                tracked_resource.resource.id = Some(format!(
                    "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                    app_name
                ));
                tracked_resource.resource.name = Some(app_name.to_string());
                tracked_resource
            },
            properties: Some(container_app::Properties {
                provisioning_state: Some(container_app::properties::ProvisioningState::InProgress),
                ..Default::default()
            }),
            extended_location: None,
            identity: None,
            kind: None,
            managed_by: None,
        }
    }

    #[derive(Debug)]
    struct AuthorizationTransport;

    #[async_trait::async_trait]
    impl Policy for AuthorizationTransport {
        async fn send(
            &self,
            _ctx: &Context,
            request: &mut Request,
            next: &[Arc<dyn Policy>],
        ) -> PolicyResult {
            assert!(next.is_empty());
            let path = request.url().path();

            if path.contains("/providers/Microsoft.Authorization/roleDefinitions/") {
                assert_eq!(request.method(), &Method::Put);
                assert_eq!(request.url().query(), Some("api-version=2022-04-01"));
                assert_json_body(request);
                return Ok(json_response(json!({
                    "id": path,
                    "name": "test-role-definition",
                    "type": "Microsoft.Authorization/roleDefinitions",
                    "properties": {
                        "roleName": "test role",
                        "type": "CustomRole",
                        "permissions": [],
                        "assignableScopes": []
                    }
                })));
            }

            if path.contains("/providers/Microsoft.Authorization/roleAssignments/") {
                match request.method() {
                    &Method::Put => {
                        assert_eq!(request.url().query(), Some("api-version=2022-04-01"));
                        let body = assert_json_body(request);
                        return Ok(json_response(json!({
                            "id": path,
                            "name": "test-role-assignment",
                            "type": "Microsoft.Authorization/roleAssignments",
                            "properties": body["properties"].clone()
                        })));
                    }
                    &Method::Delete => {
                        assert_eq!(request.url().query(), Some("api-version=2022-04-01"));
                        return Ok(Response::new(
                            StatusCode::NoContent,
                            Headers::new(),
                            Box::pin(BytesStream::new("{}")),
                        ));
                    }
                    method => {
                        panic!("unexpected Azure Authorization role assignment method: {method:?}")
                    }
                }
            }

            panic!("unexpected Azure Authorization path: {path}");
        }
    }

    #[derive(Debug)]
    struct AppManagementTransport {
        app_name: String,
        has_url: bool,
        custom_url: Option<String>,
        container_app_delete_missing: bool,
        create_long_running: bool,
        assertion: Option<ContainerAppRequestAssertion>,
        get_count: Mutex<u32>,
    }

    impl AppManagementTransport {
        fn new(app_name: &str, has_url: bool) -> Self {
            Self {
                app_name: app_name.to_string(),
                has_url,
                custom_url: None,
                container_app_delete_missing: false,
                create_long_running: false,
                assertion: None,
                get_count: Mutex::new(0),
            }
        }

        fn custom_url(mut self, custom_url: &str) -> Self {
            self.custom_url = Some(custom_url.to_string());
            self
        }

        fn delete_missing(mut self, delete_missing: bool) -> Self {
            self.container_app_delete_missing = delete_missing;
            self
        }

        fn long_running_create(mut self) -> Self {
            self.create_long_running = true;
            self
        }

        fn assert_request(mut self, assertion: ContainerAppRequestAssertion) -> Self {
            self.assertion = Some(assertion);
            self
        }

        fn response_app(&self, in_progress: bool) -> ContainerApp {
            if let Some(custom_url) = &self.custom_url {
                return create_container_app_with_custom_url(&self.app_name, custom_url);
            }

            if in_progress {
                create_in_progress_container_app_response(&self.app_name)
            } else {
                create_successful_container_app_response(&self.app_name, self.has_url)
            }
        }
    }

    #[async_trait::async_trait]
    impl Policy for AppManagementTransport {
        async fn send(
            &self,
            _ctx: &Context,
            request: &mut Request,
            next: &[Arc<dyn Policy>],
        ) -> PolicyResult {
            assert!(next.is_empty());
            let path = request.url().path();

            if path.contains("/providers/Microsoft.App/managedEnvironments/")
                && path.contains("/certificates/")
            {
                match request.method() {
                    &Method::Put => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        let body = assert_json_body(request);
                        return Ok(json_response(json!({
                            "id": path,
                            "name": path.rsplit('/').next().unwrap_or("test-certificate"),
                            "type": "Microsoft.App/managedEnvironments/certificates",
                            "location": "eastus",
                            "properties": body["properties"].clone()
                        })));
                    }
                    &Method::Delete => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        return Ok(Response::new(
                            StatusCode::NoContent,
                            Headers::new(),
                            Box::pin(BytesStream::new("{}")),
                        ));
                    }
                    method => {
                        panic!("unexpected Azure Container Apps certificate method: {method:?}")
                    }
                }
            }

            if path.contains("/providers/Microsoft.App/managedEnvironments/")
                && path.contains("/daprComponents/")
            {
                match request.method() {
                    &Method::Put => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        let body = assert_json_body(request);
                        return Ok(json_response(json!({
                            "id": path,
                            "name": path.rsplit('/').next().unwrap_or("test-component"),
                            "type": "Microsoft.App/managedEnvironments/daprComponents",
                            "properties": body["properties"].clone()
                        })));
                    }
                    &Method::Delete => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        return Ok(Response::new(
                            StatusCode::NoContent,
                            Headers::new(),
                            Box::pin(BytesStream::new("{}")),
                        ));
                    }
                    method => panic!("unexpected Azure Container Apps Dapr method: {method:?}"),
                }
            }

            if path.contains("/providers/Microsoft.App/containerApps/") {
                match request.method() {
                    &Method::Put => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        let body = assert_json_body(request);
                        if let Some(assertion) = self.assertion {
                            assert!(
                                assertion(&body),
                                "Container App PUT body did not match test assertion: {body}"
                            );
                        }
                        if self.create_long_running {
                            let mut headers = Headers::new();
                            headers.insert(
                                "azure-asyncoperation",
                                "https://management.azure.com/subscriptions/00000000-0000-0000-0000-000000000000/providers/Microsoft.App/operations/test-op",
                            );
                            headers.insert("retry-after", "1");
                            return Ok(Response::new(
                                StatusCode::Accepted,
                                headers,
                                Box::pin(BytesStream::new("{}")),
                            ));
                        }
                        return Ok(json_response(json!(self.response_app(false))));
                    }
                    &Method::Patch => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        let body = assert_json_body(request);
                        if let Some(assertion) = self.assertion {
                            assert!(
                                assertion(&body),
                                "Container App PATCH body did not match test assertion: {body}"
                            );
                        }
                        return Ok(json_response(json!(self.response_app(false))));
                    }
                    &Method::Get => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        if self.container_app_delete_missing {
                            return Ok(Response::new(
                                StatusCode::NotFound,
                                Headers::new(),
                                Box::pin(BytesStream::new("{}")),
                            ));
                        }
                        let mut get_count = self.get_count.lock().expect(
                            "Container Apps management get count mutex should not be poisoned",
                        );
                        *get_count += 1;
                        let in_progress = self.create_long_running && *get_count == 1;
                        return Ok(json_response(json!(self.response_app(in_progress))));
                    }
                    &Method::Delete => {
                        assert_eq!(
                            request.url().query(),
                            Some("api-version=2024-08-02-preview")
                        );
                        if self.container_app_delete_missing {
                            return Ok(Response::new(
                                StatusCode::NotFound,
                                Headers::new(),
                                Box::pin(BytesStream::new("{}")),
                            ));
                        }
                        return Ok(Response::new(
                            StatusCode::NoContent,
                            Headers::new(),
                            Box::pin(BytesStream::new("{}")),
                        ));
                    }
                    method => {
                        panic!("unexpected Azure Container Apps generated app method: {method:?}")
                    }
                }
            }

            panic!("unexpected Azure Container Apps management path: {path}");
        }
    }

    type ContainerAppRequestAssertion = fn(&serde_json::Value) -> bool;

    fn container_app_body_has_custom_resources(body: &serde_json::Value) -> bool {
        let resources = &body["properties"]["template"]["containers"][0]["resources"];
        let expected_memory = format!("{}Gi", 512.0 / 1024.0);
        resources["memory"].as_str() == Some(expected_memory.as_str())
            && resources["cpu"].as_f64() == Some(0.25)
    }

    fn container_app_body_has_expected_env_vars(body: &serde_json::Value) -> bool {
        let Some(env) = body["properties"]["template"]["containers"][0]["env"].as_array() else {
            return false;
        };
        let has_var = |name: &str, value: &str| {
            env.iter().any(|env_var| {
                env_var["name"].as_str() == Some(name) && env_var["value"].as_str() == Some(value)
            })
        };
        has_var("APP_ENV", "production")
            && has_var("LOG_LEVEL", "debug")
            && has_var("DB_NAME", "myapp")
    }

    fn assert_json_body(request: &Request) -> serde_json::Value {
        match request.body() {
            Body::Bytes(bytes) => {
                serde_json::from_slice(bytes).expect("authorization request body should be JSON")
            }
            #[cfg(not(target_arch = "wasm32"))]
            Body::SeekableStream(_) => panic!("authorization request should use JSON bytes"),
        }
    }

    fn json_response(value: serde_json::Value) -> Response {
        Response::new(
            StatusCode::Ok,
            Headers::new(),
            Box::pin(BytesStream::new(value.to_string())),
        )
    }

    fn authorization_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_authorization_2022_04::Client {
        let config = AzureClientConfig {
            subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
            tenant_id: "11111111-1111-1111-1111-111111111111".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        };
        let credential = Arc::new(AzureCore021Credential::new(
            azure_credential_from_config(&config).expect("test credential should build"),
        ));

        azure_authorization_2022_04::Client::builder(credential)
            .endpoint(azure_core_021::Url::parse("https://management.azure.com").unwrap())
            .transport(TransportOptions::new_custom_policy(Arc::new(transport)))
            .build()
            .expect("Azure Authorization client should build")
    }

    fn app_management_client_with_transport(
        transport: impl Policy + 'static,
    ) -> azure_app_2024_08::Client {
        let config = AzureClientConfig {
            subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
            tenant_id: "11111111-1111-1111-1111-111111111111".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        };
        let credential = Arc::new(AzureCore021Credential::new(
            azure_credential_from_config(&config).expect("test credential should build"),
        ));

        let endpoint = azure_core_021::Url::parse("https://management.azure.com").unwrap();
        let scopes = vec![endpoint
            .join(azure_core_021::auth::DEFAULT_SCOPE_SUFFIX)
            .expect("management scope should parse")
            .to_string()];
        let mut options =
            ClientOptions::new(TransportOptions::new_custom_policy(Arc::new(transport)));
        options.per_call_policies_mut().push(Arc::new(
            crate::azure_container_apps_identity_policy::ContainerAppUserAssignedIdentityPolicy,
        ));

        azure_app_2024_08::Client::new(endpoint, credential, scopes, options)
    }

    fn setup_mock_service_provider(
        app_name: &str,
        has_url: bool,
    ) -> Arc<MockPlatformServiceProvider> {
        setup_mock_service_provider_with_app_management(AppManagementTransport::new(
            app_name, has_url,
        ))
    }

    fn setup_mock_service_provider_with_delete_missing(
        app_name: &str,
        has_url: bool,
        container_app_delete_missing: bool,
    ) -> Arc<MockPlatformServiceProvider> {
        setup_mock_service_provider_with_app_management(
            AppManagementTransport::new(app_name, has_url)
                .delete_missing(container_app_delete_missing),
        )
    }

    fn setup_mock_service_provider_with_app_management(
        app_management_transport: AppManagementTransport,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        let app_management_client = app_management_client_with_transport(app_management_transport);
        mock_provider
            .expect_get_azure_container_apps_management_client()
            .returning(move |_| Ok(app_management_client.clone()));

        let authorization_client = authorization_client_with_transport(AuthorizationTransport);
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization_client.clone()));

        Arc::new(mock_provider)
    }

    /// Sets up mock Container Apps client and optional readiness probe mock server
    /// Returns (container_apps_mock_provider, optional_mock_server)
    fn setup_mocks_for_function(
        worker: &Worker,
        app_name: &str,
        for_deletion: bool,
    ) -> (Arc<MockPlatformServiceProvider>, Option<MockServer>) {
        let has_url = worker.ingress == Ingress::Public;
        let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(worker))
        } else {
            None
        };

        let app_management_transport = if needs_readiness_probe && mock_server.is_some() {
            // Create custom mock that returns the mock server URL
            let mock_server_url = mock_server.as_ref().unwrap().base_url();
            AppManagementTransport::new(app_name, true).custom_url(&mock_server_url)
        } else if for_deletion {
            AppManagementTransport::new(app_name, has_url)
        } else {
            AppManagementTransport::new(app_name, has_url)
        };

        let mock_provider =
            setup_mock_service_provider_with_app_management(app_management_transport);

        (mock_provider, mock_server)
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

        let ingress = Some(AzureContainerAppsIngress {
            external: Some(true),
            target_port: Some(8080),
            fqdn: Some(custom_url.to_string()), // Use the full URL as FQDN for the test
            traffic: vec![TrafficWeight {
                latest_revision: Some(true),
                weight: Some(100),
                revision_name: None,
                label: None,
            }],
            transport: Some(ingress::Transport::Auto),
            allow_insecure: Some(false),
            additional_port_mappings: vec![],
            custom_domains: vec![],
            ip_security_restrictions: vec![],
            cors_policy: None,
            client_certificate_mode: None,
            exposed_port: None,
            sticky_sessions: None,
            target_port_http_scheme: None,
        });

        ContainerApp {
            tracked_resource: {
                let mut tracked_resource = TrackedResource::new("East US".to_string());
                tracked_resource.resource.id = Some(format!(
                    "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                    app_name
                ));
                tracked_resource.resource.name = Some(app_name.to_string());
                tracked_resource
            },
            properties: Some(container_app::Properties {
                provisioning_state: Some(container_app::properties::ProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: Some(configuration::ActiveRevisionsMode::Single),
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: Vec::new(),
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                ..Default::default()
            }),
            extended_location: None,
            identity: None,
            kind: None,
            managed_by: None,
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
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        } else if to_function.ingress == Ingress::Public {
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
        let mock_provider = setup_mock_service_provider_with_delete_missing(
            &app_name,
            worker.ingress == Ingress::Public,
            app_missing,
        );

        // Start with a ready controller
        let mut ready_controller = AzureWorkerController::mock_ready(&app_name);
        if worker.ingress == Ingress::Public {
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
        let mock_provider = setup_mock_service_provider_with_app_management(
            AppManagementTransport::new(&app_name, false).long_running_create(),
        );

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

        let mock_provider = setup_mock_service_provider(&app_name, true);

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
        assert!(function_outputs.url.is_some());
        assert!(function_outputs
            .url
            .as_ref()
            .unwrap()
            .contains("azurecontainerapps.io"));
    }

    /// Test that verifies private workers don't get URL in outputs
    #[tokio::test]
    async fn test_private_function_has_no_url_in_outputs() {
        let worker = function_private_ingress();
        let app_name = format!("test-{}", worker.id);

        let mock_provider = setup_mock_service_provider(&app_name, false);

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
        assert!(function_outputs.url.is_none());
    }

    /// Test that verifies correct container app configuration parameters
    #[tokio::test]
    async fn test_container_app_configuration_validation() {
        let worker = function_custom_config();
        let app_name = format!("test-{}", worker.id);

        let mock_provider = setup_mock_service_provider_with_app_management(
            AppManagementTransport::new(&app_name, false)
                .assert_request(container_app_body_has_custom_resources),
        );

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

        let mock_provider = setup_mock_service_provider_with_app_management(
            AppManagementTransport::new(&app_name, false)
                .assert_request(container_app_body_has_expected_env_vars),
        );

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
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_sender_role_assignment_id: None,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
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
