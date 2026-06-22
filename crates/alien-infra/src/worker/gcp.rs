use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{
    Binding as GcpBinding, EnvironmentVariableBuilder, Expr as GcpExpr, HttpTarget, OidcToken,
    Policy, PushConfig, ResourcePermissionsHelper, SchedulerHttpMethod, SchedulerJob,
    SchedulerOidcToken, Subscription, Topic,
};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::gcp_cloudrun::{
    ConditionState, Container, ContainerPort, EnvVar,
    ExecutionEnvironment as CloudRunExecutionEnvironment, Ingress as CloudRunIngress,
    NetworkInterface, OperationResult, ResourceRequirements, RevisionScaling, RevisionTemplate,
    Service, TrafficTarget, TrafficTargetAllocationType, VpcAccess, VpcEgress,
};
use crate::gcp_compute::{
    Address, AddressType, Backend, BackendService, BackendServiceLoadBalancingScheme,
    BackendServiceProtocol, BalancingMode, ForwardingRule, ForwardingRuleLoadBalancingScheme,
    ForwardingRuleProtocol, NetworkEndpointGroup, NetworkEndpointGroupCloudRun,
    NetworkEndpointType, Operation as ComputeOperation, SslCertificate, SslCertificateSelfManaged,
    SslCertificateType, TargetHttpsProxy, UrlMap,
};
use crate::worker::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_client_core::ErrorData as CloudClientErrorData;
// Note: Role controller removed - workers now use ServiceAccount and permission profiles
use alien_core::{
    CertificateStatus, DnsRecordStatus, GcpClientConfig, GcpCloudRunWorkerHeartbeatData,
    HeartbeatBackend, Ingress, Network, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceDefinition, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs, WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, AlienErrorData, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use sha2::{Digest, Sha256};

const CLOUD_RUN_SERVICE_NAME_MAX_LEN: usize = 49;
const GCP_RESOURCE_NAME_MAX_LEN: usize = 63;
const GCP_RESOURCE_NAME_HASH_LEN: usize = 8;

fn is_remote_resource_conflict<T>(error: &AlienError<T>) -> bool
where
    T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_CONFLICT" | "CLOUD_RESOURCE_CONFLICT"
    )
}

fn is_remote_resource_not_found<T>(error: &AlienError<T>) -> bool
where
    T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_NOT_FOUND" | "CLOUD_RESOURCE_NOT_FOUND"
    )
}

fn same_unordered_strings(left: &[String], right: &[String]) -> bool {
    left.iter().collect::<HashSet<_>>() == right.iter().collect::<HashSet<_>>()
}

fn gcs_notification_matches_existing(
    existing: &serde_json::Value,
    desired: &serde_json::Value,
) -> bool {
    json_string(existing, "topic") == json_string(desired, "topic")
        && same_unordered_strings(
            &json_string_array(existing, "eventTypes"),
            &json_string_array(desired, "eventTypes"),
        )
        && json_string(existing, "payloadFormat") == json_string(desired, "payloadFormat")
        && json_string(existing, "objectNamePrefix") == json_string(desired, "objectNamePrefix")
        && json_string_map(existing, "customAttributes")
            == json_string_map(desired, "customAttributes")
}

fn json_string<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(|value| value.as_str())
}

fn json_string_array(value: &serde_json::Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(ToString::to_string)
        .collect()
}

fn json_string_map(value: &serde_json::Value, field: &str) -> HashMap<String, String> {
    value
        .get(field)
        .and_then(|value| value.as_object())
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

/// Generates the Cloud Run service name from stack prefix and worker ID
fn get_cloudrun_service_name(prefix: &str, name: &str) -> String {
    let raw = format!("{}-{}", prefix, name);
    let sanitized = sanitize_gcp_resource_name(&raw);

    if sanitized == raw && sanitized.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN {
        return sanitized;
    }

    stable_hashed_gcp_resource_name(&raw, &sanitized, CLOUD_RUN_SERVICE_NAME_MAX_LEN)
}

fn get_gcp_worker_resource_name(prefix: &str, worker_id: &str, suffix: &str) -> String {
    let raw = format!("{prefix}-{worker_id}-{suffix}");
    let sanitized = sanitize_gcp_resource_name(&raw);

    if sanitized == raw && sanitized.len() <= GCP_RESOURCE_NAME_MAX_LEN {
        return sanitized;
    }

    stable_hashed_gcp_resource_name(&raw, &sanitized, GCP_RESOURCE_NAME_MAX_LEN)
}

fn sanitize_gcp_resource_name(raw: &str) -> String {
    let mut name = String::with_capacity(raw.len());
    let mut last_was_dash = false;

    for ch in raw.chars() {
        let normalized = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            '-' => Some('-'),
            _ => Some('-'),
        };

        if let Some(ch) = normalized {
            if ch == '-' {
                if !last_was_dash && !name.is_empty() {
                    name.push(ch);
                }
                last_was_dash = true;
            } else {
                name.push(ch);
                last_was_dash = false;
            }
        }
    }

    while name.ends_with('-') {
        name.pop();
    }

    if !name
        .chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false)
    {
        name.insert_str(0, "a-");
    }

    name
}

fn stable_hashed_gcp_resource_name(raw: &str, sanitized: &str, max_len: usize) -> String {
    let hash = stable_name_hash(raw);
    let max_stem_len = max_len - GCP_RESOURCE_NAME_HASH_LEN - "-".len();
    let mut stem = sanitized
        .chars()
        .take(max_stem_len)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string();

    if stem.is_empty() {
        stem = "a".to_string();
    }

    format!("{stem}-{hash}")
}

fn stable_name_hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest
        .iter()
        .take(GCP_RESOURCE_NAME_HASH_LEN / 2)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Domain information for a worker.
struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    ssl_certificate_name: Option<String>,
    uses_custom_domain: bool,
}

fn emit_gcp_cloud_run_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    worker_config: &Worker,
    service_name: &str,
    service: &Service,
) {
    let container = service
        .template
        .as_ref()
        .and_then(|template| template.containers.first());
    let limits = container.and_then(|container| {
        container
            .resources
            .as_ref()
            .map(|resources| &resources.limits)
    });
    let scaling = service.scaling.as_ref();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: worker_config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::GcpCloudRun(
            GcpCloudRunWorkerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!("Cloud Run service '{service_name}' is ready")),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                service: service_name.to_string(),
                region: Some(
                    ctx.get_gcp_config()
                        .map(|config| config.region.clone())
                        .unwrap_or_default(),
                ),
                uri: if service.uri.is_empty() {
                    None
                } else {
                    Some(service.uri.clone())
                },
                urls: service.urls.clone(),
                latest_created_revision: if service.latest_created_revision.is_empty() {
                    None
                } else {
                    Some(service.latest_created_revision.clone())
                },
                latest_ready_revision: if service.latest_ready_revision.is_empty() {
                    None
                } else {
                    Some(service.latest_ready_revision.clone())
                },
                generation: (service.generation != 0).then_some(service.generation),
                observed_generation: (service.observed_generation != 0)
                    .then_some(service.observed_generation),
                traffic_count: service.traffic.len() as u32,
                min_instance_count: scaling
                    .map(|scaling| scaling.min_instance_count)
                    .filter(|count| *count != 0),
                max_instance_count: scaling
                    .map(|scaling| scaling.max_instance_count)
                    .filter(|count| *count != 0),
                container_image: container.map(|container| container.image.clone()),
                cpu_limit: limits.and_then(|limits| limits.get("cpu").cloned()),
                memory_limit: limits.and_then(|limits| limits.get("memory").cloned()),
            },
        )),
        raw: vec![],
    });
}

/// Tracks a GCS notification configuration for cleanup during deletion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcsNotificationTracker {
    /// The bucket the notification is attached to
    pub bucket_name: String,
    /// The server-assigned notification ID
    pub notification_id: String,
}

#[controller]
pub struct GcpWorkerController {
    /// The Cloud Run service name
    pub(crate) service_name: Option<String>,
    /// The invocation URL of the worker, available after creation.
    pub(crate) url: Option<String>,
    /// The operation name for long-running operations (for create, update, delete)
    pub(crate) operation_name: Option<String>,
    /// The Compute Engine operation name for load-balancer infrastructure.
    pub(crate) compute_operation_name: Option<String>,
    /// Region for regional Compute Engine operations. `None` means global.
    pub(crate) compute_operation_region: Option<String>,
    /// Push subscription names for queue triggers (one per queue trigger)
    pub(crate) push_subscriptions: Vec<String>,
    /// Pub/Sub topic names created for storage trigger notifications
    pub(crate) storage_notification_topics: Vec<String>,
    /// GCS notification IDs for storage triggers (for cleanup)
    pub(crate) gcs_notification_ids: Vec<GcsNotificationTracker>,
    /// Cloud Scheduler job names for schedule triggers
    pub(crate) scheduler_job_names: Vec<String>,

    // Domain & Certificate
    /// The fully qualified domain name for the worker
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The GCP SSL certificate name
    pub(crate) ssl_certificate_name: Option<String>,
    /// Whether this worker uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // HTTPS Load Balancer components
    /// The serverless NEG name pointing to Cloud Run
    pub(crate) serverless_neg_name: Option<String>,
    /// The backend service name
    pub(crate) backend_service_name: Option<String>,
    /// The URL map name
    pub(crate) url_map_name: Option<String>,
    /// The target HTTPS proxy name
    pub(crate) target_https_proxy_name: Option<String>,
    /// The global static IP address name
    pub(crate) global_address_name: Option<String>,
    /// The global static IP address value
    pub(crate) global_address_ip: Option<String>,
    /// The forwarding rule name
    pub(crate) forwarding_rule_name: Option<String>,

    // GCP project/region (stored for binding output)
    /// The GCP project ID
    pub(crate) project_id: Option<String>,
    /// The GCP region
    pub(crate) region: Option<String>,

    // Commands infrastructure
    /// Pub/Sub topic short name for commands delivery (without project prefix)
    pub(crate) commands_topic_name: Option<String>,
    /// Pub/Sub subscription name for commands delivery
    pub(crate) commands_subscription_name: Option<String>,
}

impl GcpWorkerController {
    fn record_compute_operation(
        &mut self,
        operation: ComputeOperation,
        region: Option<String>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<()> {
        if crate::gcp_compute::operation_has_error(&operation) {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        if crate::gcp_compute::operation_is_done(&operation) {
            self.compute_operation_name = None;
            self.compute_operation_region = None;
            return Ok(());
        }

        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} returned without operation name"),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        self.compute_operation_name = Some(operation_name);
        self.compute_operation_region = region;
        Ok(())
    }

    async fn compute_operation_done(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<bool> {
        let Some(operation_name) = self.compute_operation_name.as_ref() else {
            return Ok(true);
        };

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = if let Some(region) = &self.compute_operation_region {
            compute_client
                .get_region_operation(region.clone(), operation_name.clone())
                .await
        } else {
            compute_client
                .get_global_operation(operation_name.clone())
                .await
        }
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to check {operation_label} status"),
            resource_id: Some(resource_id.to_string()),
        })?;

        if !crate::gcp_compute::operation_is_done(&operation) {
            debug!(
                operation_name=%operation_name,
                operation=%operation_label,
                "Compute operation still in progress"
            );
            return Ok(false);
        }

        if crate::gcp_compute::operation_has_error(&operation) {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        self.compute_operation_name = None;
        self.compute_operation_region = None;
        Ok(true)
    }
}

#[controller]
impl GcpWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        info!(name=%cfg.id, "Initiating Cloud Run service creation");

        // Product limitation: Only allow at most one queue trigger per worker
        let queue_trigger_count = cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    cfg.id, queue_trigger_count
                ),
                resource_id: Some(cfg.id.clone()),
            }));
        }

        let gcp_config = ctx.get_gcp_config()?;
        self.project_id = Some(gcp_config.project_id.clone());
        self.region = Some(gcp_config.region.clone());
        let service_name = get_cloudrun_service_name(ctx.resource_prefix, &cfg.id);

        // Build the Cloud Run service
        let service = self
            .build_cloud_run_service(&service_name, cfg, ctx)
            .await?;

        // Create the service
        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .create_service(
                gcp_config.region.clone(),
                service_name.to_string(),
                service,
                None,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Cloud Run service".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        if operation.name.is_empty() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "Cloud Run create operation returned without name".to_string(),
                resource_id: Some(cfg.id.clone()),
            }));
        }
        let operation_name = operation.name;

        info!(name=%service_name, operation=%operation_name, "Cloud Run service creation initiated");

        self.service_name = Some(service_name);
        self.operation_name = Some(operation_name);

        Ok(HandlerAction::Continue {
            state: CreatingService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let operation_name = self.operation_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Operation name not set in state".to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;

        // Extract operation ID from the full operation name
        let operation_id = operation_name.split('/').last().unwrap_or(operation_name);

        debug!(operation=%operation_name, "Checking operation status");

        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_operation(gcp_config.region.clone(), operation_id.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run operation status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        if operation.done {
            // Check if there was an error
            if let Some(OperationResult::Error(error)) = &operation.result {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Operation failed: {} (code: {})", error.message, error.code),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, transition to next state
            info!(operation=%operation_name, "Operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceCreation,
                suggested_delay: None,
            })
        } else {
            // Operation still in progress.
            // Cloud Run service creation can take 2-5 minutes, especially for
            // first-time deployments that need to pull and start a container image.
            debug!(operation=%operation_name, "Operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_service_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Get the created service to extract the URL and verify readiness
        let service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service after creation".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Wait for the service to be Ready before proceeding. The create operation
        // may complete before the first revision is fully serving traffic, so the
        // Ready condition can still be false at this point.
        //
        // Cloud Run v2 API may not return a top-level "Ready" condition. When both
        // "RoutesReady" and "ConfigurationsReady" are Succeeded, the service is
        // effectively ready for traffic.
        let has_condition_succeeded = |name: &str| -> bool {
            service
                .conditions
                .iter()
                .any(|c| c.r#type == name && c.state == ConditionState::ConditionSucceeded)
        };

        let is_ready = has_condition_succeeded("Ready")
            || (has_condition_succeeded("RoutesReady")
                && has_condition_succeeded("ConfigurationsReady"));

        if !is_ready {
            // Log condition details at info level to aid debugging slow deployments
            let condition_summary: Vec<String> = service
                .conditions
                .iter()
                .map(|c| {
                    format!(
                        "{}={:?} (reason={:?}, message={})",
                        if c.r#type.is_empty() {
                            "?"
                        } else {
                            c.r#type.as_str()
                        },
                        c.state,
                        c.reasons,
                        c.message
                    )
                })
                .collect();
            info!(
                name=%service_name,
                conditions=?condition_summary,
                "Service not yet ready after creation, waiting"
            );
            // 240 attempts × ~9s (5s suggested + API latency) ≈ 36 minutes.
            // Cloud Run services that pull from cross-project Artifact Registry
            // may take 10-20 minutes while freshly-granted IAM bindings propagate.
            return Ok(HandlerAction::Stay {
                max_times: 240,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        let cloud_run_url = if service.uri.is_empty() {
            service.urls.first().cloned()
        } else {
            Some(service.uri.clone())
        };

        // Check for URL override in deployment config, otherwise use Cloud Run URL
        let config = ctx.desired_resource_config::<Worker>()?;
        self.url = ctx
            .deployment_config
            .public_urls
            .as_ref()
            .and_then(|urls| urls.get(&config.id).cloned())
            .or(cloud_run_url);

        info!(name=%service_name, url=?self.url, "Cloud Run service created successfully");

        // Branch based on ingress type
        // If public, resolve domain and proceed to certificate/load balancer flow
        // If private, skip directly to push subscriptions
        if config.ingress == Ingress::Public {
            match Self::resolve_domain_info(ctx, &config.id) {
                Ok(domain_info) => {
                    info!(fqdn=%domain_info.fqdn, "Resolved domain for public worker");
                    self.fqdn = Some(domain_info.fqdn);
                    self.certificate_id = domain_info.certificate_id;
                    self.ssl_certificate_name = domain_info.ssl_certificate_name;
                    self.uses_custom_domain = domain_info.uses_custom_domain;

                    // Proceed to certificate flow
                    return Ok(HandlerAction::Continue {
                        state: WaitingForCertificate,
                        suggested_delay: None,
                    });
                }
                Err(_) => {
                    // Standalone mode: no domain metadata available.
                    // The Cloud Run service URL is already set from the service
                    // creation response and is publicly accessible. Skip the
                    // custom domain / certificate / load balancer flow.
                    info!(
                        worker=%config.id,
                        url=?self.url,
                        "No domain metadata — skipping custom domain setup (standalone mode)"
                    );
                }
            }
        }

        // Always go to CreatingPushSubscriptions next (linear flow)
        Ok(HandlerAction::Continue {
            state: CreatingPushSubscriptions,
            suggested_delay: None,
        })
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
                state: CreatingPushSubscriptions,
                suggested_delay: None,
            });
        }
        if self.uses_custom_domain && self.ssl_certificate_name.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay: None,
            });
        }

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingSslCertificate,
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
        state = ImportingSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        self.ensure_domain_info(ctx, &worker_config.id)?;
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

        // For GCP, we use the full certificate chain
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let ssl_cert_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "cert");

        let ssl_certificate = SslCertificate::new()
            .set_name(ssl_cert_name.clone())
            .set_description(format!("SSL certificate for worker {}", worker_config.id))
            .set_type(SslCertificateType::SelfManaged)
            .set_self_managed(
                SslCertificateSelfManaged::new()
                    .set_certificate(certificate_chain.clone())
                    .set_private_key(private_key.clone()),
            );

        let operation = compute_client
            .insert_ssl_certificate(ssl_certificate)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import SSL certificate to GCP".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.ssl_certificate_name = Some(ssl_cert_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "SSL certificate import",
        )?;

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            worker=%worker_config.id,
            cert_name=%self.ssl_certificate_name.as_ref().unwrap(),
            "SSL certificate imported to GCP"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForSslCertificate,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "SSL certificate import")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingServerlessNeg,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.serverless_neg_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForServerlessNeg
            } else {
                CreatingBackendService
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service name not set".to_string(),
            })
        })?;

        let neg_name = get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "neg");

        // Create serverless NEG pointing to Cloud Run service
        // According to GCP API: https://docs.cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
        // For serverless NEGs, we must specify cloud_run, app_engine, or cloud_function
        let cloud_run_config =
            NetworkEndpointGroupCloudRun::new().set_service(service_name.clone());

        let neg = NetworkEndpointGroup::new()
            .set_name(neg_name.clone())
            .set_description(format!("Serverless NEG for worker {}", worker_config.id))
            .set_network_endpoint_type(NetworkEndpointType::Serverless)
            .set_cloud_run(cloud_run_config);

        let operation = compute_client
            .insert_region_network_endpoint_group(gcp_config.region.clone(), neg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create serverless NEG".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.serverless_neg_name = Some(neg_name);
        self.record_compute_operation(
            operation,
            Some(gcp_config.region.clone()),
            &worker_config.id,
            "serverless NEG creation",
        )?;

        info!(
            worker=%worker_config.id,
            neg_name=%self.serverless_neg_name.as_ref().unwrap(),
            "Serverless NEG created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForServerlessNeg,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "serverless NEG creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingBackendService,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.backend_service_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForBackendService
            } else {
                CreatingUrlMap
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let neg_name = self.serverless_neg_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Serverless NEG name not set".to_string(),
            })
        })?;

        let backend_service_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "backend");

        let neg_url = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            gcp_config.project_id, gcp_config.region, neg_name
        );

        // Create backend service with serverless NEG (no health check for serverless)
        let backend_service = BackendService::new()
            .set_name(backend_service_name.clone())
            .set_description(format!("Backend service for worker {}", worker_config.id))
            .set_protocol(BackendServiceProtocol::Https)
            .set_load_balancing_scheme(BackendServiceLoadBalancingScheme::External)
            .set_backends([Backend::new()
                .set_group(neg_url)
                .set_balancing_mode(BalancingMode::Utilization)]);

        let operation = compute_client
            .insert_backend_service(backend_service)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create backend service".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.backend_service_name = Some(backend_service_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "backend service creation",
        )?;

        info!(
            worker=%worker_config.id,
            backend_service_name=%self.backend_service_name.as_ref().unwrap(),
            "Backend service created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForBackendService,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "backend service creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingUrlMap,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.url_map_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForUrlMap
            } else {
                CreatingTargetHttpsProxy
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let backend_service_name = self.backend_service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Backend service name not set".to_string(),
            })
        })?;

        let url_map_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "urlmap");

        let backend_service_url = format!(
            "projects/{}/global/backendServices/{}",
            gcp_config.project_id, backend_service_name
        );

        // Create URL map routing to backend service
        let url_map = UrlMap::new()
            .set_name(url_map_name.clone())
            .set_description(format!("URL map for worker {}", worker_config.id))
            .set_default_service(backend_service_url);

        let operation = compute_client.insert_url_map(url_map).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to create URL map".to_string(),
                resource_id: Some(worker_config.id.clone()),
            },
        )?;

        self.url_map_name = Some(url_map_name);
        self.record_compute_operation(operation, None, &worker_config.id, "URL map creation")?;

        info!(
            worker=%worker_config.id,
            url_map_name=%self.url_map_name.as_ref().unwrap(),
            "URL map created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForUrlMap,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "URL map creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingTargetHttpsProxy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.target_https_proxy_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForTargetHttpsProxy
            } else {
                CreatingGlobalAddress
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let url_map_name = self.url_map_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "URL map name not set".to_string(),
            })
        })?;

        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "SSL certificate name not set".to_string(),
            })
        })?;

        let proxy_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https-proxy");

        let url_map_url = format!(
            "projects/{}/global/urlMaps/{}",
            gcp_config.project_id, url_map_name
        );

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_config.project_id, ssl_cert_name
        );

        // Create HTTPS proxy with SSL certificate
        let https_proxy = TargetHttpsProxy::new()
            .set_name(proxy_name.clone())
            .set_description(format!("HTTPS proxy for worker {}", worker_config.id))
            .set_url_map(url_map_url)
            .set_ssl_certificates([ssl_cert_url]);

        let operation = compute_client
            .insert_target_https_proxy(https_proxy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create target HTTPS proxy".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.target_https_proxy_name = Some(proxy_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "target HTTPS proxy creation",
        )?;

        info!(
            worker=%worker_config.id,
            proxy_name=%self.target_https_proxy_name.as_ref().unwrap(),
            "Target HTTPS proxy created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "target HTTPS proxy creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingGlobalAddress,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.global_address_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForGlobalAddress
            } else {
                CreatingForwardingRule
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let address_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "ip");

        // Create global static IP address
        let address = Address::new()
            .set_name(address_name.clone())
            .set_description(format!("Global IP for worker {}", worker_config.id))
            .set_address_type(AddressType::External);

        let operation = compute_client
            .insert_global_address(address)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global address".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.global_address_name = Some(address_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "global address creation",
        )?;

        info!(
            worker=%worker_config.id,
            address_name=%self.global_address_name.as_ref().unwrap(),
            "Global address created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForGlobalAddress,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "global address creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingForwardingRule,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.forwarding_rule_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForForwardingRule
            } else {
                WaitingForDns
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let proxy_name = self.target_https_proxy_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Target HTTPS proxy name not set".to_string(),
            })
        })?;

        let address_name = self.global_address_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Global address name not set".to_string(),
            })
        })?;

        let ip_address = self
            .ensure_global_address_ip(ctx, &worker_config.id, &address_name)
            .await?;

        let forwarding_rule_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https");

        let proxy_url = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            gcp_config.project_id, proxy_name
        );

        // Create forwarding rule exposing HTTPS endpoint
        let forwarding_rule = ForwardingRule::new()
            .set_name(forwarding_rule_name.clone())
            .set_description(format!("Forwarding rule for worker {}", worker_config.id))
            .set_ip_address(ip_address)
            .set_ip_protocol(ForwardingRuleProtocol::Tcp)
            .set_port_range("443-443")
            .set_target(proxy_url)
            .set_load_balancing_scheme(ForwardingRuleLoadBalancingScheme::External);

        let operation = compute_client
            .insert_global_forwarding_rule(forwarding_rule)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create forwarding rule".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.forwarding_rule_name = Some(forwarding_rule_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "forwarding rule creation",
        )?;

        info!(
            worker=%worker_config.id,
            forwarding_rule_name=%self.forwarding_rule_name.as_ref().unwrap(),
            "Forwarding rule created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForForwardingRule,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "forwarding rule creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
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
        if let Some(address_name) = self.global_address_name.clone() {
            self.ensure_global_address_ip(ctx, &worker_config.id, &address_name)
                .await?;
        }

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
                Ok(HandlerAction::Continue {
                    state: CreatingPushSubscriptions,
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
        state = CreatingPushSubscriptions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Creating Pub/Sub push subscriptions for queue triggers");

        // Create push subscriptions for queue triggers
        let mut created_any = false;
        for trigger in &cfg.triggers {
            if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                info!(worker=%cfg.id, queue=%queue.id, "Creating Pub/Sub push subscription");
                self.create_push_subscription(ctx, gcp_config, &service_name, &cfg, queue)
                    .await?;
                created_any = true;
            }
        }

        if !created_any {
            info!(worker=%cfg.id, "No queue triggers found, skipping push subscription creation");
        }

        // Create push subscriptions for storage triggers
        for trigger in &cfg.triggers {
            if let alien_core::WorkerTrigger::Storage { storage, events } = trigger {
                info!(worker=%cfg.id, storage=%storage.id, "Creating storage trigger infrastructure");
                self.create_storage_trigger(ctx, gcp_config, &service_name, &cfg, storage, events)
                    .await?;
            }
        }

        // Go to scheduler jobs next
        Ok(HandlerAction::Continue {
            state: CreatingSchedulerJobs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingSchedulerJobs,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;

        let schedule_triggers: Vec<(usize, &str)> = cfg
            .triggers
            .iter()
            .enumerate()
            .filter_map(|(i, trigger)| {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    Some((i, cron.as_str()))
                } else {
                    None
                }
            })
            .collect();

        if schedule_triggers.is_empty() {
            info!(worker=%cfg.id, "No schedule triggers found, skipping scheduler job creation");
            return Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;

        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service URL not available for scheduler job".to_string(),
            })
        })?;

        // Get service account email for OIDC authentication
        let service_account_email = self.get_service_account_email(ctx, &cfg)?;

        for (index, cron) in &schedule_triggers {
            let job_id = format!("{}-{}-cron-{}", ctx.resource_prefix, cfg.id, index);
            let job_full_name = format!(
                "projects/{}/locations/{}/jobs/{}",
                gcp_config.project_id, gcp_config.region, job_id
            );

            info!(
                worker=%cfg.id,
                job=%job_id,
                cron=%cron,
                "Creating Cloud Scheduler job"
            );

            let job = SchedulerJob::new()
                .set_description(format!(
                    "Schedule trigger for worker '{}' (index {})",
                    cfg.id, index
                ))
                .set_schedule(cron.to_string())
                .set_time_zone("UTC")
                .set_http_target(
                    HttpTarget::new()
                        .set_uri(service_url.clone())
                        .set_http_method(SchedulerHttpMethod::Post)
                        .set_oidc_token(
                            SchedulerOidcToken::new()
                                .set_service_account_email(service_account_email.clone())
                                .set_audience(service_url.clone()),
                        ),
                );

            match scheduler_client
                .create_job(gcp_config.region.clone(), job_id.clone(), job)
                .await
            {
                Ok(_) => {}
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        job=%job_id,
                        "Cloud Scheduler job already exists; treating as created"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create Cloud Scheduler job '{}' for worker '{}'",
                            job_id, cfg.id
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }

            if !self.scheduler_job_names.contains(&job_full_name) {
                self.scheduler_job_names.push(job_full_name);
            }

            info!(
                worker=%cfg.id,
                job=%job_id,
                "Successfully created Cloud Scheduler job"
            );
        }

        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
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
        let cfg = ctx.desired_resource_config::<Worker>()?;

        if !cfg.commands_enabled {
            debug!(worker=%cfg.id, "Commands not enabled, skipping commands infrastructure");
            return Ok(HandlerAction::Continue {
                state: SettingIamPolicy,
                suggested_delay: None,
            });
        }

        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Create commands Pub/Sub topic
        let topic_short_name = format!("{}-rq", service_name);
        let topic_full_name = format!(
            "projects/{}/topics/{}",
            gcp_config.project_id, topic_short_name
        );

        if self.commands_topic_name.is_none() {
            info!(
                worker=%cfg.id,
                topic=%topic_full_name,
                "Creating commands Pub/Sub topic"
            );

            match pubsub_client
                .create_topic(topic_short_name.clone(), Topic::default())
                .await
            {
                Ok(_) => {
                    self.commands_topic_name = Some(topic_short_name.clone());
                }
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        topic=%topic_short_name,
                        "Commands Pub/Sub topic already exists, adopting it"
                    );
                    self.commands_topic_name = Some(topic_short_name.clone());
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create commands Pub/Sub topic '{}'",
                            topic_short_name
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }
        }

        // Create push subscription that delivers to the Cloud Run service
        let subscription_name = format!("{}-rq-sub", service_name);
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service URL not available for commands push subscription".to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Only use OIDC authentication on the push subscription when the worker
        // is private. Public workers have invoker_iam_disabled=true on the Cloud
        // Run service, so PubSub can deliver without authentication. Using OIDC on
        // public workers would require the PubSub service agent to have
        // roles/iam.serviceAccountTokenCreator on the execution SA, which adds
        // unnecessary complexity.
        let oidc_token = if cfg.ingress != Ingress::Public {
            let service_account_id = format!("{}-sa", cfg.get_permissions());
            let service_account_ref = ResourceRef::new(
                alien_core::ServiceAccount::RESOURCE_TYPE,
                service_account_id.to_string(),
            );

            let service_account_state = ctx
                .require_dependency::<crate::service_account::GcpServiceAccountController>(
                    &service_account_ref,
                )?;
            let service_account_email = service_account_state
                .service_account_email
                .as_deref()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: cfg.id().to_string(),
                        dependency_id: service_account_id.to_string(),
                    })
                })?
                .to_string();

            Some(
                OidcToken::new()
                    .set_service_account_email(service_account_email)
                    .set_audience(push_endpoint.clone()),
            )
        } else {
            None
        };

        let mut push_config = PushConfig::new()
            .set_push_endpoint(push_endpoint.clone())
            .set_attributes(HashMap::<String, String>::new());
        if let Some(oidc_token) = oidc_token {
            push_config = push_config.set_oidc_token(oidc_token);
        }

        let subscription = Subscription::new()
            .set_name(subscription_name.clone())
            .set_topic(topic_full_name.clone())
            .set_push_config(push_config)
            .set_ack_deadline_seconds(cfg.timeout_seconds as i32)
            .set_retain_acked_messages(false)
            .set_labels([
                ("commands".to_string(), cfg.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
            ])
            .set_enable_message_ordering(false)
            .set_detached(false);

        if self.commands_subscription_name.is_none() {
            info!(
                worker=%cfg.id,
                topic=%topic_full_name,
                subscription=%subscription_name,
                endpoint=%push_endpoint,
                "Creating commands Pub/Sub push subscription"
            );

            match pubsub_client
                .create_subscription(subscription_name.clone(), subscription)
                .await
            {
                Ok(_) => {
                    self.commands_subscription_name = Some(subscription_name.clone());
                }
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        subscription=%subscription_name,
                        "Commands Pub/Sub push subscription already exists, adopting it"
                    );
                    self.commands_subscription_name = Some(subscription_name.clone());
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create commands push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }
        }

        self.apply_command_topic_management_permissions(ctx, &topic_short_name)
            .await?;

        info!(worker=%cfg.id, "Commands Pub/Sub infrastructure created");

        Ok(HandlerAction::Continue {
            state: SettingIamPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = SettingIamPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let _cfg = ctx.desired_resource_config::<Worker>()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Setting IAM policy for Cloud Run service");

        // Apply resource-scoped IAM bindings only. Public access is handled via
        // invoker_iam_disabled on the service (set during creation), not via allUsers
        // IAM binding. This avoids issues with domain-restricted sharing org policies.
        self.apply_consolidated_iam_policy(ctx, service_name, false)
            .await?;

        // Always go to readiness probe next (linear flow - may be no-op)
        Ok(HandlerAction::Continue {
            state: RunningReadinessProbe,
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
        let cfg = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL
        if cfg.readiness_probe.is_some() {
            if let Some(url) = self.url.as_ref() {
                match run_readiness_probe(ctx, url).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        } else {
            debug!(name=%ctx.desired_config.id(), "No readiness probe configured, proceeding to Ready");
        }

        // Either no readiness probe needed, or probe succeeded - proceed to Ready
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
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Heartbeat check: verify service still exists and is in correct state
        let service = client
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service during heartbeat check".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Verify service is ready. A service can be temporarily not-Ready due to
        // scaling events, GCP maintenance, or revision transitions. Use a retryable
        // error to allow recovery instead of immediately failing the deployment.
        // Cloud Run v2 may not return a "Ready" condition, so also accept both
        // sub-conditions as Succeeded.
        let has_condition_succeeded = |name: &str| -> bool {
            service
                .conditions
                .iter()
                .any(|c| c.r#type == name && c.state == ConditionState::ConditionSucceeded)
        };

        let is_ready = has_condition_succeeded("Ready")
            || (has_condition_succeeded("RoutesReady")
                && has_condition_succeeded("ConfigurationsReady"));

        if !is_ready {
            warn!(name=%worker_config.id, "Cloud Run service is not in Ready state during heartbeat");
            let mut err = AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker_config.id.clone(),
                message: "Cloud Run service is not in Ready state".to_string(),
            });
            err.retryable = true;
            return Err(err);
        }

        // Check for basic configuration drift - compare memory limits
        if let Some(template) = &service.template {
            if let Some(container) = template.containers.first() {
                if let Some(resources) = &container.resources {
                    if let Some(current_memory) = resources.limits.get("memory") {
                        let expected_memory = format!("{}Mi", worker_config.memory_mb);
                        if current_memory != &expected_memory {
                            return Err(AlienError::new(ErrorData::ResourceDrift {
                                resource_id: worker_config.id.clone(),
                                message: format!(
                                    "Service memory configuration has drifted. Expected: {}, but found: {}",
                                    expected_memory, current_memory
                                ),
                            }));
                        }
                    }
                }
            }
        }

        // Check for certificate renewal on auto-managed public domains.
        if worker_config.ingress == Ingress::Public && !self.uses_custom_domain {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&worker_config.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            worker=%worker_config.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateImportingSslCertificate,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        emit_gcp_cloud_run_worker_heartbeat(ctx, &worker_config, service_name, &service);

        debug!(name = %worker_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;

        if cfg.ingress != Ingress::Public || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&cfg.id))
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

        let Some(proxy_name) = self.target_https_proxy_name.clone() else {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        };

        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;

        let issued_suffix = resource
            .issued_at
            .as_deref()
            .unwrap_or("renewed")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect::<String>()
            .to_lowercase();
        let ssl_cert_name = get_gcp_worker_resource_name(
            ctx.resource_prefix,
            &cfg.id,
            &format!("cert-{issued_suffix}"),
        );
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let ssl_certificate = SslCertificate::new()
            .set_name(ssl_cert_name.clone())
            .set_description(format!("Renewed SSL certificate for worker {}", cfg.id))
            .set_type(SslCertificateType::SelfManaged)
            .set_self_managed(
                SslCertificateSelfManaged::new()
                    .set_certificate(certificate_chain.clone())
                    .set_private_key(private_key.clone()),
            );

        match compute_client.insert_ssl_certificate(ssl_certificate).await {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%cfg.id,
                    cert_name=%ssl_cert_name,
                    "Renewed SSL certificate already exists; treating as imported"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to import renewed SSL certificate to GCP".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        }

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_config.project_id, ssl_cert_name
        );
        compute_client
            .set_target_https_proxy_ssl_certificates(proxy_name, vec![ssl_cert_url])
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to bind renewed SSL certificate to target HTTPS proxy".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        let previous_ssl_certificate_name = self.ssl_certificate_name.clone();

        self.ssl_certificate_name = Some(ssl_cert_name);
        self.certificate_issued_at = resource.issued_at.clone();

        if let Some(previous_ssl_certificate_name) = previous_ssl_certificate_name {
            if self.ssl_certificate_name.as_deref() != Some(previous_ssl_certificate_name.as_str())
            {
                match compute_client
                    .delete_ssl_certificate(previous_ssl_certificate_name.clone())
                    .await
                {
                    Ok(_) => {
                        info!(
                            worker=%cfg.id,
                            cert_name=%previous_ssl_certificate_name,
                            "Deleted previous SSL certificate after renewal"
                        );
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(e) => {
                        warn!(
                            worker=%cfg.id,
                            cert_name=%previous_ssl_certificate_name,
                            error=%e,
                            "Failed to delete previous SSL certificate after renewal"
                        );
                    }
                }
            }
        }

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
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let previous_cfg = ctx.previous_resource_config::<Worker>()?;
        if cfg == previous_cfg {
            return Ok(HandlerAction::Continue {
                state: UpdateEnsuringPublicExposure,
                suggested_delay: None,
            });
        }

        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Starting Cloud Run service update");

        // Get current service to preserve etag
        let current_service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service for update".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Build updated service configuration
        let mut updated_service = self.build_cloud_run_service(service_name, cfg, ctx).await?;

        // Preserve important fields from current service
        updated_service.name = current_service.name;
        updated_service.etag = current_service.etag;

        // Patch the service
        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .patch_service(
                gcp_config.region.clone(),
                service_name.clone(),
                updated_service,
                None, // update_mask - let the API figure it out
                None, // validate_only
                None, // allow_missing
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to patch Cloud Run service".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        if operation.name.is_empty() {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Cloud Run update operation returned without name".to_string(),
            }));
        }
        let operation_name = operation.name;

        info!(name=%service_name, operation=%operation_name, "Cloud Run service update initiated");

        self.operation_name = Some(operation_name);

        Ok(HandlerAction::Continue {
            state: UpdatingService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = UpdatingService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let operation_name = self.operation_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Operation name not set in state".to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;

        // Extract operation ID from the full operation name
        let operation_id = operation_name.split('/').last().unwrap_or(operation_name);

        debug!(operation=%operation_name, "Checking update operation status");

        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_operation(gcp_config.region.clone(), operation_id.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run operation status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        if operation.done {
            // Check if there was an error
            if let Some(OperationResult::Error(error)) = &operation.result {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Update operation failed: {} (code: {})",
                        error.message, error.code
                    ),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, transition to next state
            info!(operation=%operation_name, "Update operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceUpdate,
                suggested_delay: None,
            })
        } else {
            // Operation still in progress
            debug!(operation=%operation_name, "Update operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_service_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Get the updated service
        let service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service after update".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Check if the service is ready. Cloud Run v2 may not return a "Ready"
        // condition, so also accept both sub-conditions as Succeeded.
        let has_condition_succeeded = |name: &str| -> bool {
            service
                .conditions
                .iter()
                .any(|c| c.r#type == name && c.state == ConditionState::ConditionSucceeded)
        };

        let is_ready = has_condition_succeeded("Ready")
            || (has_condition_succeeded("RoutesReady")
                && has_condition_succeeded("ConfigurationsReady"));

        if !is_ready {
            debug!(name=%service_name, "Service not yet ready after update");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        info!(name=%service_name, "Cloud Run service updated successfully");

        Ok(HandlerAction::Continue {
            state: UpdateEnsuringPublicExposure,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateEnsuringPublicExposure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_ensuring_public_exposure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;

        if current_config.ingress != Ingress::Public {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        let has_domain_info = self.ensure_domain_info(ctx, &current_config.id)?;
        if !has_domain_info {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        if self.forwarding_rule_name.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        Ok(HandlerAction::Continue {
            state: UpdateWaitingForCertificate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateWaitingForCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_certificate(ctx).await? {
            HandlerAction::Continue {
                state: ImportingSslCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialSslCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingPushSubscriptions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateImportingInitialSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_ssl_certificate(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForSslCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForSslCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_ssl_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_ssl_certificate(ctx).await? {
            HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_ssl_certificate",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_serverless_neg",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_serverless_neg",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_backend_service",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_backend_service",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_url_map(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_url_map",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_url_map(ctx).await? {
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_url_map",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_target_https_proxy",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_target_https_proxy",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_global_address(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_global_address",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_global_address(ctx).await? {
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_global_address",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateCreatingForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_forwarding_rule",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_forwarding_rule",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdateWaitingForDns,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_dns(ctx).await? {
            HandlerAction::Continue {
                state: CreatingPushSubscriptions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_dns",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    #[handler(
        state = UpdatePushSubscriptions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating push subscriptions");

            // Delete old subscriptions, storage notifications, topics, and scheduler jobs
            self.delete_all_push_subscriptions(ctx, gcp_config).await?;
            self.delete_all_storage_notifications(ctx, gcp_config)
                .await?;
            self.delete_all_storage_notification_topics(ctx, gcp_config)
                .await?;
            self.delete_all_scheduler_jobs(ctx, gcp_config).await?;

            // Recreate all trigger infrastructure
            for trigger in &current_config.triggers {
                match trigger {
                    alien_core::WorkerTrigger::Queue { queue } => {
                        self.create_push_subscription(
                            ctx,
                            gcp_config,
                            &service_name,
                            &current_config,
                            queue,
                        )
                        .await?;
                    }
                    alien_core::WorkerTrigger::Storage { storage, events } => {
                        self.create_storage_trigger(
                            ctx,
                            gcp_config,
                            &service_name,
                            &current_config,
                            storage,
                            events,
                        )
                        .await?;
                    }
                    alien_core::WorkerTrigger::Schedule { .. } => {
                        // Scheduler jobs are recreated below after all triggers
                    }
                }
            }

            // Recreate scheduler jobs
            let service_url = self.url.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: current_config.id.clone(),
                    message: "Service URL not available for scheduler job".to_string(),
                })
            })?;
            let service_account_email = self.get_service_account_email(ctx, &current_config)?;
            let scheduler_client = ctx
                .service_provider
                .get_gcp_cloud_scheduler_client(gcp_config)?;

            for (index, trigger) in current_config.triggers.iter().enumerate() {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    let job_id = format!(
                        "{}-{}-cron-{}",
                        ctx.resource_prefix, current_config.id, index
                    );
                    let job_full_name = format!(
                        "projects/{}/locations/{}/jobs/{}",
                        gcp_config.project_id, gcp_config.region, job_id
                    );

                    let job = SchedulerJob::new()
                        .set_description(format!(
                            "Schedule trigger for worker '{}' (index {})",
                            current_config.id, index
                        ))
                        .set_schedule(cron.to_string())
                        .set_time_zone("UTC")
                        .set_http_target(
                            HttpTarget::new()
                                .set_uri(service_url.clone())
                                .set_http_method(SchedulerHttpMethod::Post)
                                .set_oidc_token(
                                    SchedulerOidcToken::new()
                                        .set_service_account_email(service_account_email.clone())
                                        .set_audience(service_url.clone()),
                                ),
                        );

                    match scheduler_client
                        .create_job(gcp_config.region.clone(), job_id.clone(), job)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) if is_remote_resource_conflict(&e) => {
                            info!(
                                worker=%current_config.id,
                                job=%job_id,
                                "Cloud Scheduler job already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to create Cloud Scheduler job '{}' for worker '{}'",
                                    job_id, current_config.id
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    if !self.scheduler_job_names.contains(&job_full_name) {
                        self.scheduler_job_names.push(job_full_name);
                    }
                }
            }
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
        }

        Ok(HandlerAction::Continue {
            state: UpdateSettingIamPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateSettingIamPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_setting_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.setting_iam_policy(ctx).await? {
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "setting_iam_policy",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
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
        let cfg = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL
        if cfg.readiness_probe.is_some() {
            if let Some(url) = self.url.as_ref() {
                match run_readiness_probe(ctx, url).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        } else {
            debug!(name=%ctx.desired_config.id(), "No readiness probe configured, proceeding to Ready");
        }

        // Either no readiness probe needed, or probe succeeded - proceed to Ready
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
        let _gcp_config = ctx.get_gcp_config()?;

        // Handle case where service_name is not set (e.g., creation failed early)
        let service_name = match self.service_name.as_ref() {
            Some(name) => name,
            None => {
                // No service was created, nothing to delete
                info!(resource_id=%ctx.desired_config.id(), "No Cloud Run service to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();
                self.storage_notification_topics.clear();
                self.gcs_notification_ids.clear();
                self.scheduler_job_names.clear();

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(name=%service_name, "Initiating Cloud Run service deletion");

        // If we have load balancer resources, delete them first
        // Otherwise, skip directly to deleting push subscriptions
        if self.forwarding_rule_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingForwardingRule,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: DeletingPushSubscriptions,
                suggested_delay: None,
            })
        }
    }

    // ─────────────── LB DELETION STATES ───────────────────────────

    #[handler(
        state = DeletingForwardingRule,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(forwarding_rule_name) = &self.forwarding_rule_name {
            info!(name=%forwarding_rule_name, "Deleting forwarding rule");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_global_forwarding_rule(forwarding_rule_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%forwarding_rule_name, "Forwarding rule deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%forwarding_rule_name, "Forwarding rule was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete forwarding rule '{}'",
                            forwarding_rule_name
                        ),
                        resource_id: None,
                    }));
                }
            }

            self.forwarding_rule_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingTargetHttpsProxy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(proxy_name) = &self.target_https_proxy_name {
            info!(name=%proxy_name, "Deleting target HTTPS proxy");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_target_https_proxy(proxy_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%proxy_name, "Target HTTPS proxy deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%proxy_name, "Target HTTPS proxy was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete target HTTPS proxy '{}'", proxy_name),
                        resource_id: None,
                    }));
                }
            }

            self.target_https_proxy_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingUrlMap,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingUrlMap,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(url_map_name) = &self.url_map_name {
            info!(name=%url_map_name, "Deleting URL map");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_url_map(url_map_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%url_map_name, "URL map deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%url_map_name, "URL map was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete URL map '{}'", url_map_name),
                        resource_id: None,
                    }));
                }
            }

            self.url_map_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingBackendService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingBackendService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(backend_service_name) = &self.backend_service_name {
            info!(name=%backend_service_name, "Deleting backend service");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_backend_service(backend_service_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%backend_service_name, "Backend service deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%backend_service_name, "Backend service was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete backend service '{}'",
                            backend_service_name
                        ),
                        resource_id: None,
                    }));
                }
            }

            self.backend_service_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingServerlessNeg,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingServerlessNeg,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(neg_name) = &self.serverless_neg_name {
            info!(name=%neg_name, "Deleting serverless NEG");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_region_network_endpoint_group(gcp_config.region.clone(), neg_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%neg_name, "Serverless NEG deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%neg_name, "Serverless NEG was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete serverless NEG '{}'", neg_name),
                        resource_id: None,
                    }));
                }
            }

            self.serverless_neg_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingSslCertificate,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingSslCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(ssl_cert_name) = &self.ssl_certificate_name {
            info!(name=%ssl_cert_name, "Deleting SSL certificate");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_ssl_certificate(ssl_cert_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%ssl_cert_name, "SSL certificate deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%ssl_cert_name, "SSL certificate was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete SSL certificate '{}'", ssl_cert_name),
                        resource_id: None,
                    }));
                }
            }

            self.ssl_certificate_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingGlobalAddress,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingGlobalAddress,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(address_name) = &self.global_address_name {
            info!(name=%address_name, "Deleting global address");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_global_address(address_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%address_name, "Global address deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%address_name, "Global address was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete global address '{}'", address_name),
                        resource_id: None,
                    }));
                }
            }

            self.global_address_name = None;
            self.global_address_ip = None;
        }

        // Clear domain-related state
        self.fqdn = None;
        self.certificate_id = None;
        self.certificate_issued_at = None;
        self.uses_custom_domain = false;

        Ok(HandlerAction::Continue {
            state: DeletingPushSubscriptions,
            suggested_delay: None,
        })
    }

    // ─────────────── SERVICE DELETION STATES ──────────────────────

    #[handler(
        state = DeletingPushSubscriptions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%worker_config.id, subscriptions=?self.push_subscriptions, "Deleting push subscriptions");

        // Delete all push subscriptions using best-effort approach (ignore NotFound)
        self.delete_all_push_subscriptions(ctx, gcp_config).await?;

        // Delete GCS notifications (best-effort)
        self.delete_all_storage_notifications(ctx, gcp_config)
            .await?;

        // Delete storage notification topics (best-effort)
        self.delete_all_storage_notification_topics(ctx, gcp_config)
            .await?;

        // Continue to scheduler jobs cleanup
        Ok(HandlerAction::Continue {
            state: DeletingSchedulerJobs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSchedulerJobs,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.scheduler_job_names.is_empty() {
            return Ok(HandlerAction::Continue {
                state: DeletingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        info!(worker=%worker_config.id, jobs=?self.scheduler_job_names, "Deleting Cloud Scheduler jobs");

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
                    );
                }
                Err(e) if is_remote_resource_not_found(&e) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job was already deleted (not found)"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        job=%job_name,
                        error=%e,
                        "Failed to delete Cloud Scheduler job (best-effort, continuing)"
                    );
                }
            }
        }

        self.scheduler_job_names.clear();

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
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let derived_topic_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq"))
            })
            .flatten();
        let derived_subscription_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq-sub"))
            })
            .flatten();

        // Delete commands subscription (best-effort)
        if let Some(subscription_name) = self
            .commands_subscription_name
            .take()
            .or(derived_subscription_name)
        {
            info!(subscription=%subscription_name, "Deleting commands push subscription");
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(subscription=%subscription_name, "Commands push subscription deleted");
                }
                Err(e) => {
                    warn!(
                        subscription=%subscription_name,
                        error=%e,
                        "Failed to delete commands push subscription (may already be deleted)"
                    );
                }
            }
        }

        // Delete commands topic (best-effort)
        if let Some(topic_name) = self.commands_topic_name.take().or(derived_topic_name) {
            info!(topic=%topic_name, "Deleting commands Pub/Sub topic");
            match pubsub_client.delete_topic(topic_name.clone()).await {
                Ok(_) => {
                    info!(topic=%topic_name, "Commands Pub/Sub topic deleted");
                }
                Err(e) => {
                    warn!(
                        topic=%topic_name,
                        error=%e,
                        "Failed to delete commands Pub/Sub topic (may already be deleted)"
                    );
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingService,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Deleting Cloud Run service");

        // Try to delete the service, handling the case where it's already missing
        match ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .delete_service(gcp_config.region.clone(), service_name.clone(), None, None)
            .await
        {
            Ok(operation) => {
                // Service exists and deletion was initiated
                if operation.name.is_empty() {
                    return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: ctx.desired_config.id().to_string(),
                        message: "Cloud Run delete operation returned without name".to_string(),
                    }));
                }
                let operation_name = operation.name;

                info!(name=%service_name, operation=%operation_name, "Cloud Run service deletion initiated");

                self.operation_name = Some(operation_name);

                Ok(HandlerAction::Continue {
                    state: WaitingForDeleteOperation,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                // Service is already missing - deletion goal achieved
                info!(name=%service_name, "Cloud Run service was already deleted");

                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();

                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                // Other error - propagate it
                Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Cloud Run service".to_string(),
                    resource_id: Some(ctx.desired_config.id().to_string()),
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
        let operation_name = self.operation_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Operation name not set in state".to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;

        // Extract operation ID from the full operation name
        let operation_id = operation_name.split('/').last().unwrap_or(operation_name);

        debug!(operation=%operation_name, "Checking delete operation status");

        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_operation(gcp_config.region.clone(), operation_id.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run operation status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        if operation.done {
            // Check if there was an error
            if let Some(OperationResult::Error(error)) = &operation.result {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Delete operation failed: {} (code: {})",
                        error.message, error.code
                    ),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, now wait for the service to be gone
            info!(operation=%operation_name, "Delete operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceDeletion,
                suggested_delay: Some(Duration::from_secs(2)),
            })
        } else {
            // Operation still in progress
            debug!(operation=%operation_name, "Delete operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(3)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_service_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Try to get the service - if it's gone, we're done
        match ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%service_name, "Cloud Run service successfully deleted");

                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();

                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Cloud Run service deletion status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })),
            Ok(_) => {
                debug!(name=%service_name, "Service still exists, waiting for deletion");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.url.as_ref().map(|url| {
            let public_url = self
                .fqdn
                .as_ref()
                .map(|fqdn| format!("https://{fqdn}"))
                .unwrap_or_else(|| url.clone());

            let load_balancer_endpoint = self.global_address_ip.as_ref().map(|global_address_ip| {
                alien_core::LoadBalancerEndpoint {
                    dns_name: global_address_ip.clone(),
                    hosted_zone_id: None,
                }
            });

            ResourceOutputs::new(WorkerOutputs {
                // Use the service name if available, otherwise fall back to a placeholder
                worker_name: self
                    .service_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                url: Some(public_url),
                identifier: self.service_name.clone(),
                load_balancer_endpoint,
                commands_push_target: self.commands_topic_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, CloudRunWorkerBinding, WorkerBinding};

        if let (Some(service_name), Some(url)) = (&self.service_name, &self.url) {
            let project_id = self.project_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP project_id missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;
            let location = self.region.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP region missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;

            let binding = WorkerBinding::CloudRun(CloudRunWorkerBinding {
                project_id: BindingValue::Value(project_id),
                service_name: BindingValue::Value(service_name.clone()),
                location: BindingValue::Value(location),
                private_url: BindingValue::Value(url.clone()),
                public_url: Some(BindingValue::Value(url.clone())),
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

// Separate impl block for helper methods
impl GcpWorkerController {
    // ─────────────── HELPER METHODS ────────────────────────────

    async fn apply_command_topic_management_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        topic_name: &str,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let command_refs: Vec<_> = ctx
            .desired_stack
            .management()
            .profile()
            .and_then(|management_profile| management_profile.0.get(&config.id))
            .into_iter()
            .flat_map(|refs| refs.iter())
            .filter(|permission_set_ref| permission_set_ref.id() == "worker/dispatch-command")
            .cloned()
            .collect();

        let gcp_config = ctx.get_gcp_config()?;
        let mut permission_context = alien_permissions::PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(topic_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = alien_permissions::generators::GcpRuntimePermissionsGenerator::new();
        let mut all_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_management_bindings_for(
            ctx,
            &config.id,
            topic_name,
            &command_refs,
            &generator,
            &permission_context,
            alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
            &mut all_bindings,
        )
        .await?;

        let iam_policy = Policy::new().set_version(3).set_bindings(all_bindings);
        let bindings_count = iam_policy.bindings.len();

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        pubsub_client
            .set_topic_iam_policy(topic_name.to_string(), iam_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply management command permissions to Pub/Sub topic '{}'",
                    topic_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            worker = %config.id,
            topic = %topic_name,
            bindings_count,
            "Reconciled management command permissions on Pub/Sub topic"
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
            let ssl_cert_name = custom
                .certificate
                .gcp
                .as_ref()
                .map(|cert| cert.certificate_name.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires a GCP SSL certificate name".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                ssl_certificate_name: Some(ssl_cert_name),
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
            ssl_certificate_name: None,
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
                || self.ssl_certificate_name.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id) {
            Ok(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.ssl_certificate_name = domain_info.ssl_certificate_name;
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

    fn unexpected_update_wrapper_state(
        resource_id: &str,
        handler: &str,
        state: GcpWorkerState,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("{handler} returned unexpected state during update: {state:?}"),
        })
    }

    async fn ensure_global_address_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        address_name: &str,
    ) -> Result<String> {
        if let Some(ip_address) = &self.global_address_ip {
            return Ok(ip_address.clone());
        }

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;
        let address = compute_client
            .get_global_address(address_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get global address".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        let ip_address = address.address.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Global address has no IP".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        self.global_address_ip = Some(ip_address.clone());
        Ok(ip_address)
    }

    async fn build_cloud_run_service(
        &self,
        service_name: &str,
        cfg: &Worker,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Service> {
        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's email
        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        let service_account = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();
        let service_account = Some(service_account);

        // Extract container image
        let image = match &cfg.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Worker '{}' is configured with source code, but only pre-built images are supported in alien-infra.",
                        cfg.id
                    ),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Resolve proxy URIs to native GAR URIs. Cloud Run can only pull from GAR.
        let image = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
            alien_core::image_rewrite::resolve_native_image_uri(&image, native_host)
                .unwrap_or(image)
        } else {
            image
        };

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, service_name)
            .await?;

        let env: Vec<EnvVar> = env_vars
            .into_iter()
            .map(|(name, value)| EnvVar::new().set_name(name).set_value(value))
            .collect();

        // Build resource requirements
        let mut limits = HashMap::new();
        limits.insert("memory".to_string(), format!("{}Mi", cfg.memory_mb));
        // Cloud Run automatically allocates CPU based on memory

        let resources = ResourceRequirements::new()
            .set_limits(limits)
            .set_cpu_idle(true) // Allow CPU throttling when idle
            .set_startup_cpu_boost(true); // Boost CPU during startup

        // Build container port
        // NOTE: This must match the alien-runtime port on alien-build/src/lib.rs
        let ports = vec![ContainerPort::new()
            .set_name("http1")
            .set_container_port(8080)];

        // Build container
        let container = Container::new()
            .set_name("worker")
            .set_image(image)
            .set_env(env)
            .set_resources(resources)
            .set_ports(ports);

        // Map ingress settings
        let ingress = match cfg.ingress {
            Ingress::Public => CloudRunIngress::All,
            Ingress::Private => CloudRunIngress::InternalOnly,
        };

        // Get VPC access configuration if a Network resource exists
        let vpc_access = self.get_vpc_access(ctx)?;
        if vpc_access.is_some() {
            info!(name=%service_name, "Configuring Cloud Run service with Direct VPC Egress");
        }

        // Build revision template
        let mut scaling = RevisionScaling::new().set_min_instance_count(0); // Scale to zero
        if let Some(concurrency_limit) = cfg.concurrency_limit {
            scaling = scaling.set_max_instance_count(concurrency_limit as i32);
        }

        let mut template = RevisionTemplate::new()
            .set_labels([("worker", cfg.id.as_str())])
            .set_scaling(scaling)
            .set_timeout(wkt::Duration::clamp(cfg.timeout_seconds as i64, 0))
            .set_containers([container])
            .set_execution_environment(CloudRunExecutionEnvironment::Gen2)
            .set_max_instance_request_concurrency(1000); // Cloud Run default
        if let Some(service_account) = service_account {
            template = template.set_service_account(service_account);
        }
        if let Some(vpc_access) = vpc_access {
            template = template.set_vpc_access(vpc_access);
        }

        // Build traffic target
        let traffic = vec![TrafficTarget::new()
            .set_type(TrafficTargetAllocationType::Latest)
            .set_percent(100)];

        // Build service
        // When ingress is public, disable the IAM invoker check instead of adding
        // allUsers to IAM policy. This works even when the GCP organization has
        // domain-restricted sharing enabled (which blocks allUsers in IAM).
        let is_public = cfg.ingress == Ingress::Public;
        let service = Service::new()
            .set_description(format!("Runtime worker: {}", cfg.id))
            .set_labels([
                ("resource-type", "worker".to_string()),
                ("resource", cfg.id.clone()),
                ("deployment", ctx.resource_prefix.to_string()),
            ])
            .set_ingress(ingress)
            .set_template(template)
            .set_traffic(traffic)
            .set_invoker_iam_disabled(is_public);

        Ok(service)
    }

    /// Gets VPC access configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the network name and subnetwork name from the Network controller to configure
    /// the Cloud Run service with Direct VPC Egress.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_access(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcAccess>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::GcpNetworkController>(&network_ref)?;

        // Only configure VPC access if we have network and subnetwork names
        let network_name = match &network_state.network_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        let subnetwork_name = match &network_state.subnetwork_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        // Build Direct VPC Egress configuration using network interfaces
        let network_interface = NetworkInterface::new()
            .set_network(network_name)
            .set_subnetwork(subnetwork_name);

        Ok(Some(
            VpcAccess::new()
                .set_egress(VpcEgress::AllTraffic)
                .set_network_interfaces([network_interface]),
        ))
    }

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        function_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        use crate::core::ResourceController;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let env_vars = EnvironmentVariableBuilder::try_new(initial_env)?
            .add_worker_runtime_env_vars(ctx, &worker_config.id)?
            .add_linked_resources(links, ctx, function_name_for_error_logging)
            .await?
            .add_self_worker_binding(&worker_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Applies consolidated IAM policy (resource-scoped permissions + public access) in a single operation
    async fn apply_consolidated_iam_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        enable_public_access: bool,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;

        // Get existing IAM policy to preserve any existing bindings
        let mut policy = client
            .get_service_iam_policy(gcp_config.region.clone(), service_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for Cloud Run service '{}' before applying bindings. Refusing to proceed to avoid overwriting existing bindings.", service_name),
                resource_id: Some(config.id.clone()),
            })?;

        // Step 1: Apply resource-scoped permissions from the stack
        let mut resource_bindings = Vec::new();
        self.collect_resource_scoped_bindings(ctx, service_name, &mut resource_bindings)
            .await?;

        // Step 2: Add public access binding if needed
        if enable_public_access {
            info!(service_name = %service_name, "Adding public access to IAM policy");
            let invoker_role = "roles/run.invoker".to_string();
            let all_users_member = "allUsers".to_string();

            // Check if binding already exists
            let binding_exists = policy
                .bindings
                .iter()
                .any(|b| b.role == invoker_role && b.members.contains(&all_users_member));

            if !binding_exists {
                // Find existing binding or create new one
                if let Some(binding) = policy.bindings.iter_mut().find(|b| b.role == invoker_role) {
                    if !binding.members.contains(&all_users_member) {
                        binding.members.push(all_users_member);
                    }
                } else {
                    policy.bindings.push(
                        GcpBinding::new()
                            .set_role(invoker_role)
                            .set_members([all_users_member]),
                    );
                }
            }
        }

        // Step 3: Add resource-scoped bindings
        if !resource_bindings.is_empty() {
            info!(
                service_name = %service_name,
                bindings_count = resource_bindings.len(),
                "Adding resource-scoped permissions to IAM policy"
            );
            policy.bindings.extend(resource_bindings);
        }

        // Step 4: Apply the consolidated policy in one operation
        client
            .set_service_iam_policy(gcp_config.region.clone(), service_name.to_string(), policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply consolidated IAM policy to Cloud Run service '{}'",
                    service_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(service_name = %service_name, "Consolidated IAM policy applied successfully");
        Ok(())
    }

    /// Collect resource-scoped bindings without applying them
    async fn collect_resource_scoped_bindings(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        all_bindings: &mut Vec<GcpBinding>,
    ) -> Result<()> {
        use alien_permissions::{generators::GcpRuntimePermissionsGenerator, PermissionContext};

        let config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;

        // Build permission context for this specific worker resource
        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(service_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = GcpRuntimePermissionsGenerator::new();
        let type_prefix = "worker/";

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions
            let mut combined_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = profile.0.get(&config.id) {
                combined_refs.extend(
                    permission_set_refs
                        .iter()
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                combined_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if !combined_refs.is_empty() {
                info!(
                    service_name = %service_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing resource-scoped permissions for worker"
                );

                self.process_profile_permissions(
                    ctx,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to process permissions for profile '{}' on worker '{}'",
                        profile_name, service_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        // Process management SA permissions matching the worker resource type
        if let Some(management_profile) = ctx.desired_stack.management().profile() {
            let mut management_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = management_profile.0.get(&config.id) {
                management_refs.extend(
                    permission_set_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if let Some(wildcard_refs) = management_profile.0.get("*") {
                management_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if !management_refs.is_empty() {
                use crate::core::ResourcePermissionsHelper;
                ResourcePermissionsHelper::collect_gcp_management_bindings_for(
                    ctx,
                    &config.id,
                    service_name,
                    &management_refs,
                    &generator,
                    &permission_context,
                    alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
                    all_bindings,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &alien_permissions::generators::GcpRuntimePermissionsGenerator,
        permission_context: &alien_permissions::PermissionContext,
        all_bindings: &mut Vec<GcpBinding>,
    ) -> Result<()> {
        use alien_permissions::BindingTarget;

        // Get the service account email for this profile
        let service_account_email =
            self.get_service_account_email_for_profile(ctx, profile_name)?;

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;
            let selected_bindings = grant_plan.bindings_for_target(
                alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
            );

            // Convert and add bindings
            let member = format!("serviceAccount:{}", service_account_email);
            for binding in selected_bindings {
                all_bindings.push(
                    GcpBinding::new()
                        .set_role(binding.role)
                        .set_members([member.clone()])
                        .set_or_clear_condition(binding.condition.map(|cond| {
                            GcpExpr::new()
                                .set_expression(cond.expression)
                                .set_title(cond.title)
                                .set_description(cond.description)
                        })),
                );
            }
        }

        Ok(())
    }

    /// Get the service account email for a permission profile
    fn get_service_account_email_for_profile(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "worker".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }

    /// Creates a Pub/Sub push subscription for a queue trigger
    async fn create_push_subscription(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        // Get queue controller to access the topic name
        let queue_controller =
            ctx.require_dependency::<crate::queue::gcp::GcpQueueController>(queue_ref)?;
        let topic_name = queue_controller.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        // Generate push subscription name: stack-prefix-worker-id-queue-id
        let subscription_name = format!(
            "{}-{}-{}",
            ctx.resource_prefix, worker_config.id, queue_ref.id
        );

        // Get the service URL for push endpoint
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for push subscription".to_string(),
            })
        })?;

        // Build push endpoint URL (Cloud Run service URL)
        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;
        let service_account_email = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Create push config with OIDC authentication
        let oidc_token = OidcToken::new()
            .set_service_account_email(service_account_email.clone())
            .set_audience(push_endpoint.clone());

        let push_config = PushConfig::new()
            .set_push_endpoint(push_endpoint.clone())
            .set_attributes(HashMap::<String, String>::new())
            .set_oidc_token(oidc_token);

        let subscription = Subscription::new()
            .set_name(subscription_name.clone())
            .set_topic(topic_name.clone())
            .set_push_config(push_config)
            .set_ack_deadline_seconds(worker_config.timeout_seconds as i32)
            .set_retain_acked_messages(false)
            .set_labels([
                ("worker".to_string(), worker_config.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
            ])
            .set_enable_message_ordering(false)
            .set_detached(false);

        info!(
            worker=%worker_config.id,
            topic=%topic_name,
            subscription=%subscription_name,
            endpoint=%push_endpoint,
            "Creating Pub/Sub push subscription"
        );

        match pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    subscription=%subscription_name,
                    "Pub/Sub push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for queue '{}'",
                        subscription_name, queue_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name.clone());
        }

        info!(
            worker=%worker_config.id,
            subscription=%subscription_name,
            "Successfully created Pub/Sub push subscription"
        );

        Ok(())
    }

    /// Deletes all push subscriptions using best-effort approach
    async fn delete_all_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
    ) -> Result<()> {
        if self.push_subscriptions.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for subscription_name in &self.push_subscriptions.clone() {
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        subscription=%subscription_name,
                        "Push subscription deleted successfully"
                    );
                }
                Err(e) if is_remote_resource_not_found(&e) => {
                    info!(
                        worker=%worker_config.id,
                        subscription=%subscription_name,
                        "Push subscription was already deleted (not found)"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }

        self.push_subscriptions.clear();
        Ok(())
    }

    /// Gets the service account email for the worker's permission profile.
    fn get_service_account_email(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker_config: &alien_core::Worker,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        service_account_state.service_account_email.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id().to_string(),
                dependency_id: service_account_id,
            })
        })
    }

    /// Creates storage trigger infrastructure: Pub/Sub topic, GCS notification, and push subscription.
    async fn create_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Get bucket name from the storage controller dependency
        let storage_controller =
            ctx.require_dependency::<crate::storage::GcpStorageController>(storage_ref)?;
        let bucket_name = storage_controller.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: storage_ref.id.clone(),
            })
        })?;

        // 1. Create a dedicated Pub/Sub topic for this storage notification
        let topic_short_name = format!(
            "{}-{}-{}-notif",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );
        let topic_full_name = format!(
            "projects/{}/topics/{}",
            gcp_config.project_id, topic_short_name
        );

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            topic=%topic_full_name,
            "Creating Pub/Sub topic for storage notifications"
        );

        match pubsub_client
            .create_topic(topic_short_name.clone(), Topic::default())
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    topic=%topic_short_name,
                    "Storage notification topic already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create storage notification topic '{}'",
                        topic_short_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.storage_notification_topics.contains(&topic_short_name) {
            self.storage_notification_topics
                .push(topic_short_name.clone());
        }

        // 2. Grant the GCS service agent publish permissions on the topic
        //    The GCS service agent email uses the project ID as a fallback when
        //    project_number is not available.
        let gcs_service_agent = if let Some(ref project_number) = gcp_config.project_number {
            format!(
                "serviceAccount:service-{}@gs-project-accounts.iam.gserviceaccount.com",
                project_number
            )
        } else {
            // Fall back to project_id-based format (works for numeric project IDs)
            format!(
                "serviceAccount:service-{}@gs-project-accounts.iam.gserviceaccount.com",
                gcp_config.project_id
            )
        };

        let iam_policy = Policy::new().set_version(1).set_bindings([GcpBinding::new()
            .set_role("roles/pubsub.publisher")
            .set_members([gcs_service_agent])]);

        pubsub_client
            .set_topic_iam_policy(topic_short_name.clone(), iam_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set IAM policy on storage notification topic '{}'",
                    topic_short_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // 3. Create GCS notification on the bucket pointing to the topic
        let gcs_event_type_names: Vec<String> = events
            .iter()
            .map(|event| {
                match event.as_str() {
                    "created" => "OBJECT_FINALIZE".to_string(),
                    "deleted" => "OBJECT_DELETE".to_string(),
                    "archived" => "OBJECT_ARCHIVE".to_string(),
                    "metadataUpdated" => "OBJECT_METADATA_UPDATE".to_string(),
                    other => other.to_string(), // Pass through unknown events as-is
                }
            })
            .collect();

        let notification = serde_json::json!({
            "topic": topic_full_name.clone(),
            "eventTypes": gcs_event_type_names,
            "payloadFormat": "JSON_API_V1",
        });

        let existing_notification = gcs_client
            .list_notifications(bucket_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list GCS notifications on bucket '{}' for worker '{}'",
                    bucket_name, worker_config.id
                ),
                resource_id: Some(worker_config.id.clone()),
            })?
            .into_iter()
            .find(|existing| gcs_notification_matches_existing(existing, &notification));

        let created_notification = if let Some(existing_notification) = existing_notification {
            info!(
                worker=%worker_config.id,
                storage=%storage_ref.id,
                bucket=%bucket_name,
                notification_id=?json_string(&existing_notification, "id"),
                "GCS notification already exists; treating as created"
            );
            existing_notification
        } else {
            gcs_client
                .insert_notification(bucket_name.clone(), notification)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create GCS notification on bucket '{}' for worker '{}'",
                        bucket_name, worker_config.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                })?
        };

        if let Some(notification_id) = json_string(&created_notification, "id") {
            if !self.gcs_notification_ids.iter().any(|tracker| {
                tracker.bucket_name == *bucket_name && tracker.notification_id == notification_id
            }) {
                self.gcs_notification_ids.push(GcsNotificationTracker {
                    bucket_name: bucket_name.clone(),
                    notification_id: notification_id.to_string(),
                });
            }
        }

        // 4. Create a push subscription to the Cloud Run URL
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for storage trigger push subscription"
                    .to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_email = self.get_service_account_email(ctx, worker_config)?;

        let oidc_token = OidcToken::new()
            .set_service_account_email(service_account_email)
            .set_audience(push_endpoint.clone());

        let subscription_name = format!(
            "{}-{}-{}-notif-sub",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );

        let push_config = PushConfig::new()
            .set_push_endpoint(push_endpoint)
            .set_attributes(HashMap::<String, String>::new())
            .set_oidc_token(oidc_token);

        let subscription = Subscription::new()
            .set_name(subscription_name.clone())
            .set_topic(topic_full_name.clone())
            .set_push_config(push_config)
            .set_ack_deadline_seconds(worker_config.timeout_seconds as i32)
            .set_retain_acked_messages(false)
            .set_labels([
                ("worker".to_string(), worker_config.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
                ("storage".to_string(), storage_ref.id.clone()),
            ])
            .set_enable_message_ordering(false)
            .set_detached(false);

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            subscription=%subscription_name,
            "Creating Pub/Sub push subscription for storage trigger"
        );

        match pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    subscription=%subscription_name,
                    "Storage trigger push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for storage trigger '{}'",
                        subscription_name, storage_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name);
        }

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            "Successfully created storage trigger infrastructure"
        );

        Ok(())
    }

    /// Deletes all GCS notifications (best-effort)
    async fn delete_all_storage_notifications(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
    ) -> Result<()> {
        if self.gcs_notification_ids.is_empty() {
            return Ok(());
        }

        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for tracker in &self.gcs_notification_ids.clone() {
            match gcs_client
                .delete_notification(tracker.bucket_name.clone(), tracker.notification_id.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        "GCS notification deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        error=%e,
                        "Failed to delete GCS notification (best-effort, continuing)"
                    );
                }
            }
        }

        self.gcs_notification_ids.clear();
        Ok(())
    }

    /// Deletes all storage notification Pub/Sub topics (best-effort)
    async fn delete_all_storage_notification_topics(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
    ) -> Result<()> {
        if self.storage_notification_topics.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for topic_name in &self.storage_notification_topics.clone() {
            match pubsub_client.delete_topic(topic_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        "Storage notification topic deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        error=%e,
                        "Failed to delete storage notification topic (best-effort, continuing)"
                    );
                }
            }
        }

        self.storage_notification_topics.clear();
        Ok(())
    }

    /// Deletes all Cloud Scheduler jobs (best-effort)
    async fn delete_all_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &GcpClientConfig,
    ) -> Result<()> {
        if self.scheduler_job_names.is_empty() {
            return Ok(());
        }

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        job=%job_name,
                        error=%e,
                        "Failed to delete Cloud Scheduler job (best-effort, continuing)"
                    );
                }
            }
        }

        self.scheduler_job_names.clear();
        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: GcpWorkerState::Ready,
            service_name: Some(function_name.to_string()),
            url: Some(format!("https://{}-abcd1234-uc.a.run.app", function_name)),
            operation_name: None,
            compute_operation_name: None,
            compute_operation_region: None,
            push_subscriptions: Vec::new(),
            storage_notification_topics: Vec::new(),
            gcs_notification_ids: Vec::new(),
            scheduler_job_names: Vec::new(),
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            global_address_ip: None,
            forwarding_rule_name: None,
            project_id: Some("test-project".to_string()),
            region: Some("us-central1".to_string()),
            commands_topic_name: None,
            commands_subscription_name: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # GCP Worker Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::core::{MockGcpIamApi, Policy};
    use crate::gcp_cloudrun::{
        Condition, ConditionState, MockCloudRunApi, Operation as LongRunningOperation, Service,
    };
    use crate::gcp_compute::{Address, MockGcpComputeApi, Operation, OperationStatus};
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{
        CertificateStatus, DnsRecordStatus, DomainMetadata, Ingress, Platform, ResourceDomainInfo,
        ResourceStatus, Worker, WorkerOutputs,
    };
    use alien_error::AlienError;
    use httpmock::prelude::*;
    use rstest::rstest;

    use super::{
        get_cloudrun_service_name, get_gcp_worker_resource_name, CLOUD_RUN_SERVICE_NAME_MAX_LEN,
        GCP_RESOURCE_NAME_MAX_LEN,
    };
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{MockPlatformServiceProvider, MockPubSubApi};
    use crate::worker::readiness_probe::test_utils::create_readiness_probe_mock;
    use crate::worker::{fixtures::*, GcpWorkerController};
    use crate::GcpWorkerState;

    #[test]
    fn cloudrun_service_name_preserves_valid_short_names() {
        assert_eq!(
            get_cloudrun_service_name("test-stack", "worker"),
            "test-stack-worker"
        );
    }

    #[test]
    fn cloudrun_service_name_caps_long_e2e_names_with_stable_hash() {
        let service_name = get_cloudrun_service_name(
            "e2e-gcp-terraform-worker-mpfa2f19-15fb",
            "test-alien-ts-function",
        );

        assert!(service_name.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN);
        assert!(service_name.starts_with("e"));
        assert!(!service_name.ends_with('-'));
        assert!(service_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
        assert_eq!(
            service_name,
            get_cloudrun_service_name(
                "e2e-gcp-terraform-worker-mpfa2f19-15fb",
                "test-alien-ts-function",
            )
        );
    }

    #[test]
    fn cloudrun_service_name_sanitizes_invalid_input() {
        let service_name = get_cloudrun_service_name("123_Test.Stack", "Worker_Name_");

        assert!(service_name.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN);
        assert!(service_name.starts_with('a'));
        assert!(!service_name.ends_with('-'));
        assert!(service_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
    }

    #[test]
    fn gcp_worker_resource_name_caps_long_certificate_names_with_stable_hash() {
        let cert_name = get_gcp_worker_resource_name(
            "e2e-gcp-terraform-worker-mpfgzubr-tux",
            "test-alien-ts-function",
            "cert",
        );

        assert!(cert_name.len() <= GCP_RESOURCE_NAME_MAX_LEN);
        assert!(cert_name.starts_with('e'));
        assert!(!cert_name.ends_with('-'));
        assert!(cert_name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
        assert_eq!(
            cert_name,
            get_gcp_worker_resource_name(
                "e2e-gcp-terraform-worker-mpfgzubr-tux",
                "test-alien-ts-function",
                "cert",
            )
        );
    }

    fn create_test_domain_metadata(resource_id: &str) -> DomainMetadata {
        let mut resources = HashMap::new();
        resources.insert(
            resource_id.to_string(),
            ResourceDomainInfo {
                fqdn: format!("{}.test.example.com", resource_id),
                certificate_id: "test-cert-id".to_string(),
                certificate_status: CertificateStatus::Issued,
                dns_status: DnsRecordStatus::Active,
                dns_error: None,
                certificate_chain: Some(
                    "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n"
                        .to_string(),
                ),
                private_key: Some(
                    "-----BEGIN RSA PRIVATE KEY-----\nMIIBtest\n-----END RSA PRIVATE KEY-----\n"
                        .to_string(),
                ),
                issued_at: Some("2024-01-01T00:00:00Z".to_string()),
                aliases: Vec::new(),
            },
        );
        DomainMetadata {
            base_domain: "test.example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "Z1234567890ABC".to_string(),
            resources,
        }
    }

    fn create_ssl_compute_mock_for_creation_and_deletion() -> Arc<MockGcpComputeApi> {
        fn completed_compute_operation() -> Operation {
            Operation::new()
                .set_name("test-compute-operation")
                .set_status(OperationStatus::Done)
        }

        let mut mock = MockGcpComputeApi::new();
        mock.expect_insert_ssl_certificate()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_insert_region_network_endpoint_group()
            .returning(|_, _| Ok(completed_compute_operation()));
        mock.expect_insert_backend_service()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_insert_url_map()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_insert_target_https_proxy()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_insert_global_address()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_get_global_address()
            .returning(|_| Ok(Address::new().set_address("203.0.113.1")));
        mock.expect_insert_global_forwarding_rule()
            .returning(|_| Ok(completed_compute_operation()));
        mock.expect_delete_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_target_https_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_region_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_delete_ssl_certificate()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_global_address()
            .returning(|_| Ok(Operation::default()));
        Arc::new(mock)
    }

    fn create_successful_service_response(service_name: &str) -> Service {
        Service::new()
            .set_name(format!(
                "projects/test-project/locations/us-central1/services/{}",
                service_name
            ))
            .set_uri(format!("https://{}-abcd1234-uc.a.run.app", service_name))
            .set_urls([format!("https://{}-abcd1234-uc.a.run.app", service_name)])
            .set_conditions([Condition::new()
                .set_type("Ready")
                .set_state(ConditionState::ConditionSucceeded)])
    }

    fn create_successful_operation_response(operation_name: &str) -> LongRunningOperation {
        LongRunningOperation::new()
            .set_name(format!(
                "projects/test-project/locations/us-central1/operations/{}",
                operation_name
            ))
            .set_done(false)
    }

    fn create_completed_operation_response(operation_name: &str) -> LongRunningOperation {
        LongRunningOperation::new()
            .set_name(format!(
                "projects/test-project/locations/us-central1/operations/{}",
                operation_name
            ))
            .set_done(true)
    }

    fn create_empty_iam_policy() -> Policy {
        Policy::new().set_version(1)
    }

    fn setup_mock_client_for_creation_and_update(
        function_name: &str,
        _has_public_access: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks - first pending, then completed
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(&operation_name_for_get))
        });

        // Mock service retrieval after creation
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)));

        // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful updates
        let function_name_for_update = function_name.to_string();
        let update_operation_name = format!("update-{}", function_name_for_update);
        mock_cloudrun
            .expect_patch_service()
            .returning(move |_, _, _, _, _, _| {
                Ok(create_successful_operation_response(&update_operation_name))
            });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_creation_and_deletion(
        function_name: &str,
        _has_public_access: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks for creation
        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1); // Only for creation flow

        // Mock service retrieval after creation
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1); // Only for creation flow

        // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful service deletion
        let function_name_for_delete = function_name.to_string();
        let delete_operation_name = format!("delete-{}", function_name_for_delete);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        // Mock operation status checks for deletion
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        // Mock service not found during deletion check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _function_name: &str,
        service_missing: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service deletion (might fail if service missing)
        if service_missing {
            mock_cloudrun
                .expect_delete_service()
                .returning(|_, _, _, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Service".to_string(),
                            resource_name: "test-service".to_string(),
                        },
                    ))
                });
        } else {
            let delete_operation_name = "delete-test".to_string();
            let delete_operation_name_for_get = delete_operation_name.clone();
            mock_cloudrun
                .expect_delete_service()
                .returning(move |_, _, _, _| {
                    Ok(create_successful_operation_response(&delete_operation_name))
                });

            mock_cloudrun.expect_get_operation().returning(move |_, _| {
                Ok(create_completed_operation_response(
                    &delete_operation_name_for_get,
                ))
            });
        }

        // Always return not found for final status check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockGcpIamApi> {
        Arc::new(MockGcpIamApi::new())
    }

    fn setup_mock_service_provider(
        mock_cloudrun: Arc<MockCloudRunApi>,
        mock_compute: Option<Arc<MockGcpComputeApi>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_cloudrun_client()
            .returning(move |_| Ok(mock_cloudrun.clone()));

        if let Some(compute) = mock_compute {
            mock_provider
                .expect_get_gcp_compute_client()
                .returning(move |_| Ok(compute.clone()));
        }

        // Mock IAM client for resource-scoped permissions.
        let mock_iam = create_gcp_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        // Mock PubSub client for commands infrastructure cleanup
        let mock_pubsub = Arc::new(MockPubSubApi::new());
        mock_provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(mock_pubsub.clone()));

        Arc::new(mock_provider)
    }

    /// Sets up mock CloudRun client and optional readiness probe mock server
    /// Returns (cloudrun_mock_provider, optional_mock_server, optional_domain_metadata)
    fn setup_mocks_for_function(
        worker: &Worker,
        function_name: &str,
        for_deletion: bool,
    ) -> (
        Arc<MockPlatformServiceProvider>,
        Option<MockServer>,
        Option<DomainMetadata>,
    ) {
        let has_public_access = worker.ingress == Ingress::Public;
        let needs_readiness_probe = has_public_access && worker.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(worker))
        } else {
            None
        };

        // Set up CloudRun client mock
        let cloudrun_mock = if for_deletion {
            if let Some(ref _server) = mock_server {
                setup_mock_client_for_creation_and_deletion_with_mock_url(
                    function_name,
                    has_public_access,
                    &_server.base_url(),
                )
            } else {
                setup_mock_client_for_creation_and_deletion(function_name, has_public_access)
            }
        } else {
            if let Some(ref _server) = mock_server {
                setup_mock_client_for_creation_and_update_with_mock_url(
                    function_name,
                    has_public_access,
                    &_server.base_url(),
                )
            } else {
                setup_mock_client_for_creation_and_update(function_name, has_public_access)
            }
        };

        // For public workers, also set up compute mock and domain metadata
        let (compute_mock, domain_metadata) = if has_public_access {
            let dm = create_test_domain_metadata(&worker.id);
            let compute = create_ssl_compute_mock_for_creation_and_deletion();
            (Some(compute), Some(dm))
        } else {
            (None, None)
        };

        let mock_provider = setup_mock_service_provider(cloudrun_mock, compute_mock);

        (mock_provider, mock_server, domain_metadata)
    }

    fn setup_mock_client_for_creation_and_update_with_mock_url(
        function_name: &str,
        _has_public_access: bool,
        mock_url: &str,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(&operation_name_for_get))
        });

        // Mock service retrieval after creation - use mock URL
        let mock_url = mock_url.to_string();
        let function_name_for_get = function_name.to_string();
        mock_cloudrun.expect_get_service().returning(move |_, _| {
            let mut service = create_successful_service_response(&function_name_for_get);
            service.uri = mock_url.clone();
            service.urls = vec![mock_url.clone()];
            Ok(service)
        });

        // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful updates
        let function_name_for_update = function_name.to_string();
        let update_operation_name = format!("update-{}", function_name_for_update);
        mock_cloudrun
            .expect_patch_service()
            .returning(move |_, _, _, _, _, _| {
                Ok(create_successful_operation_response(&update_operation_name))
            });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_creation_and_deletion_with_mock_url(
        function_name: &str,
        _has_public_access: bool,
        mock_url: &str,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks for creation
        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1); // Only for creation flow

        // Mock service retrieval after creation - use mock URL
        let mock_url = mock_url.to_string();
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| {
                let mut service = create_successful_service_response(&function_name_for_get);
                service.uri = mock_url.clone();
                service.urls = vec![mock_url.clone()];
                Ok(service)
            })
            .times(1); // Only for creation flow

        // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful service deletion
        let function_name_for_delete = function_name.to_string();
        let delete_operation_name = format!("delete-{}", function_name_for_delete);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        // Mock operation status checks for deletion
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        // Mock service not found during deletion check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
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
        let worker_id = worker.id.clone();
        let worker_ingress = worker.ingress.clone();
        let function_name = format!("test-{}", worker.id);
        let (mock_provider, _mock_server, domain_metadata) =
            setup_mocks_for_function(&worker, &function_name, true);

        let mut builder = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }

        let mut executor = builder.build().await.unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.identifier.is_some());
        assert!(function_outputs.worker_name.starts_with("test-"));
        if worker_ingress == Ingress::Public {
            let expected_url = format!("https://{}.test.example.com", worker_id);
            assert_eq!(function_outputs.url.as_deref(), Some(expected_url.as_str()));
            assert_eq!(
                function_outputs
                    .load_balancer_endpoint
                    .as_ref()
                    .map(|endpoint| endpoint.dns_name.as_str()),
                Some("203.0.113.1")
            );
        }

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

        let function_name = format!("test-{}", worker_id);
        let (mock_provider, mock_server, domain_metadata) =
            setup_mocks_for_function(&to_function, &function_name, false);

        // Start with the "from" worker in Ready state
        let mut ready_controller = GcpWorkerController::mock_ready(&function_name);

        // If the target worker has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        }

        let mut builder = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }

        let mut executor = builder.build().await.unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new worker
        let target_is_public = to_function.ingress == Ingress::Public;
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        if target_is_public {
            let expected_url = format!("https://{}.test.example.com", worker_id);
            assert_eq!(function_outputs.url.as_deref(), Some(expected_url.as_str()));
        }
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_service(function_public_ingress(), true)]
    #[case::private_with_missing_service(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] worker: Worker,
        #[case] service_missing: bool,
    ) {
        let function_name = format!("test-{}", worker.id);
        let has_public_access = worker.ingress == Ingress::Public;
        let mock_cloudrun =
            setup_mock_client_for_best_effort_deletion(&function_name, service_missing);
        let mock_provider = setup_mock_service_provider(mock_cloudrun, None);

        // Start with a ready controller
        let mut ready_controller = GcpWorkerController::mock_ready(&function_name);
        if has_public_access {
            ready_controller.url = Some("https://example-abcd1234-uc.a.run.app".to_string());
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(ready_controller)
            .platform(Platform::Gcp)
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

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public workers get IAM policy update
    #[tokio::test]
    async fn test_public_function_sets_iam_policy() {
        let worker = function_public_ingress();
        let function_name = format!("test-{}", worker.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Validate IAM policy operations are called for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .withf(|location, service_name| {
                location == "us-central1" && service_name.starts_with("test-")
            })
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .withf(|location, service_name, _policy| {
                location == "us-central1" && service_name.starts_with("test-")
            })
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let compute_mock = create_ssl_compute_mock_for_creation_and_deletion();
        let mock_provider =
            setup_mock_service_provider(Arc::new(mock_cloudrun), Some(compute_mock));
        let domain_metadata = create_test_domain_metadata(&worker.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .domain_metadata(domain_metadata)
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
    }

    /// Test that verifies private workers handle resource-scoped permissions correctly
    #[tokio::test]
    async fn test_private_function_skips_iam_policy() {
        let worker = function_private_ingress();
        let function_name = format!("test-{}", worker.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // IAM policy operations are now called for all workers (for resource-scoped permissions)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is still available for private workers (internal access)
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
    }

    /// Test that verifies correct service configuration parameters
    #[tokio::test]
    async fn test_service_configuration_validation() {
        let worker = function_custom_config();
        let function_name = format!("test-{}", worker.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Validate service creation request has correct parameters
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .withf(|_, _, service, _| {
                // Check if the service has the expected configuration
                if let Some(template) = &service.template {
                    let containers = &template.containers;
                    if let Some(container) = containers.first() {
                        // Check memory configuration
                        if let Some(resources) = &container.resources {
                            if let Some(memory) = resources.limits.get("memory") {
                                return memory == "512Mi";
                            }
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Mock IAM policy operations for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
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
        let function_name = format!("test-{}", worker.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Validate service creation request has environment variables
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .withf(|_, _, service, _| {
                if let Some(template) = &service.template {
                    let containers = &template.containers;
                    if let Some(container) = containers.first() {
                        // Check environment variables
                        let env_vars: HashMap<String, String> = container
                            .env
                            .iter()
                            .filter_map(|env| env.value().map(|v| (env.name.clone(), v.clone())))
                            .collect();

                        return env_vars.get("APP_ENV") == Some(&"production".to_string())
                            && env_vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                            && env_vars.get("DB_NAME") == Some(&"myapp".to_string());
                    }
                }
                false
            })
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Mock IAM policy operations for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(GcpWorkerController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies deletion works when service_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_service_name_succeeds() {
        let worker = basic_function();

        // Create a controller with no service name set (simulating early creation failure)
        let controller = GcpWorkerController {
            state: GcpWorkerState::CreateFailed,
            service_name: None, // This is the key - no service name set
            url: None,
            operation_name: None,
            compute_operation_name: None,
            compute_operation_region: None,
            push_subscriptions: Vec::new(),
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            global_address_ip: None,
            forwarding_rule_name: None,
            commands_topic_name: None,
            commands_subscription_name: None,
            storage_notification_topics: Vec::new(),
            gcs_notification_ids: Vec::new(),
            scheduler_job_names: Vec::new(),
            project_id: None,
            region: None,
            _internal_stay_count: None,
        };

        // Mock provider - no expectations since no API calls should be made
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(controller)
            .platform(Platform::Gcp)
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
