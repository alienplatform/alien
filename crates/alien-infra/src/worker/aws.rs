mod linked_containers;

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::linked_bindings;
use crate::core::EnvironmentVariableBuilder;

use crate::core::aws_tag_scoped::{
    certificates_imported_with_token, delete_imported_certificate, delete_tag_scoped,
    is_remote_access_denied, is_remote_not_found, with_import_token, TagScopedDelete,
    CREATE_ATTEMPT_TAG,
};
use crate::core::split_certificate_chain;
use crate::core::ResourceController;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::certificate_wait::{CERTIFICATE_WAIT_MAX_POLLS, CERTIFICATE_WAIT_POLL_SECS};
use crate::worker::readiness_probe::{
    run_readiness_probe_with_dns_override, ReadinessProbeDnsOverride, READINESS_PROBE_MAX_ATTEMPTS,
};
use alien_aws_clients::apigateway::{
    CreateBasePathMappingRequest, CreateDeploymentRequest,
    CreateDomainNameRequest as CreateRestDomainNameRequest, CreateResourceRequest,
    CreateRestApiRequest, EndpointConfiguration as RestEndpointConfiguration,
    PutIntegrationRequest, PutMethodRequest,
};
use alien_aws_clients::apigatewayv2::{
    CreateApiMappingRequest, CreateApiRequest, CreateDomainNameRequest, CreateIntegrationRequest,
    CreateRouteRequest, CreateStageRequest, DomainNameConfiguration,
};
use alien_aws_clients::cloudwatch_logs::{CreateLogGroupRequest, PutRetentionPolicyRequest};
use alien_aws_clients::ec2::{DescribeNetworkInterfacesRequest, Filter};
use alien_aws_clients::eventbridge::{
    EventBridgeTag, EventBridgeTarget, PutRuleRequest, PutTargetsRequest,
};
use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, Environment, FunctionCode, FunctionConfiguration,
    ListEventSourceMappingsRequest, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
    VpcConfig,
};
use alien_aws_clients::s3::{LambdaFunctionConfiguration, NotificationConfiguration};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsLambdaWorkerHeartbeatData, CertificateStatus, DnsRecordStatus,
    HeartbeatBackend, Network, NetworkSettings, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceDefinition, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs, WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use chrono::Utc;
use uuid::Uuid;

/// A reservation race has the same quota identity as the early account check.
fn lambda_concurrency_error(error: AlienError<CloudClientErrorData>, region: &str) -> AlienError {
    if let Some(CloudClientErrorData::InvalidInput { message, .. }) = &error.error {
        if message.contains("UnreservedConcurrentExecution") {
            let quota = alien_preflights::runtime::aws_lambda::quota_identity(region);
            let message = format!(
                "{} ({}, {}): {}. Request an increase: {}",
                quota.name,
                quota.provider_id,
                quota.scope,
                message.trim_end_matches('.'),
                quota.increase_url
            );
            return error
                .context(ErrorData::CloudQuotaExceeded { quota, message })
                .into_generic();
        }
    }
    error.into_generic()
}

const AWS_LAMBDA_ACTIVE_MAX_POLLS: u32 = 60;

// Lambda can take 20 minutes to release Hyperplane ENIs after VPC detachment.
// Shared ENIs in customer networks may remain in use by other functions indefinitely.
const EXTERNAL_VPC_ENI_WAIT_SECONDS: i64 = 20 * 60;

/// Days a worker's CloudWatch log group keeps events. Matches the log group
/// the CloudFormation template declares for the same worker.
const LAMBDA_LOG_RETENTION_DAYS: i32 = 30;

/// Generates the full, prefixed AWS resource name.
fn get_aws_worker_name(prefix: &str, name: &str) -> String {
    aws_prefixed_resource_name(prefix, name)
}

// AWS emits both the exact function name and a name followed by a UUID.
// A loose prefix also matches sibling functions such as "worker-v2".
fn lambda_eni_description_matches(description: &str, prefix: &str) -> bool {
    let Some(suffix) = description.strip_prefix(prefix) else {
        return false;
    };
    suffix.is_empty()
        || suffix.strip_prefix('-').is_some_and(|id| {
            id.len() == 36
                && Uuid::parse_str(id)
                    .is_ok_and(|uuid| uuid.hyphenated().to_string().eq_ignore_ascii_case(id))
        })
}

fn readiness_probe_dns_override(
    url: &str,
    fqdn: Option<&str>,
    load_balancer: Option<&LoadBalancerState>,
) -> Option<ReadinessProbeDnsOverride> {
    let fqdn = fqdn?;
    let endpoint = load_balancer?.endpoint.as_ref()?;
    let parsed = reqwest::Url::parse(url).ok()?;
    let url_host = parsed.host_str()?;

    if url_host != fqdn {
        return None;
    }

    Some(ReadinessProbeDnsOverride {
        host: fqdn.to_string(),
        target_dns_name: endpoint.dns_name.clone(),
        port: parsed.port_or_known_default().unwrap_or(443),
    })
}

fn eventbridge_tags(prefix: &str, resource_id: &str) -> Vec<EventBridgeTag> {
    standard_resource_tags(prefix, resource_id)
        .into_iter()
        .map(|(key, value)| EventBridgeTag { key, value })
        .collect()
}

fn is_remote_resource_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

/// Whether `tags` hold every one of `expected`.
fn carries_tags(
    tags: Option<&HashMap<String, String>>,
    expected: &HashMap<String, String>,
) -> bool {
    tags.is_some_and(|tags| {
        expected
            .iter()
            .all(|(key, value)| tags.get(key) == Some(value))
    })
}

fn replace_lambda_notification_config(
    notification_config: &mut NotificationConfiguration,
    replacement: LambdaFunctionConfiguration,
) {
    if let Some(replacement_id) = replacement.id.as_ref() {
        notification_config
            .lambda_function_configurations
            .retain(|config| config.id.as_ref() != Some(replacement_id));
    }
    notification_config
        .lambda_function_configurations
        .push(replacement);
}

impl AwsWorkerController {
    async fn renew_certificate(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if worker_config.public_endpoints.is_empty() || self.uses_custom_domain {
            return Ok(());
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
        else {
            return Ok(());
        };

        if resource.issued_at.is_none() || resource.issued_at == self.certificate_issued_at {
            return Ok(());
        }

        let Some(certificate_arn) = self.certificate_arn.clone() else {
            return Ok(());
        };
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

        let (leaf, chain) = split_certificate_chain(certificate_chain);
        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;

        // ACM rejects tags on a reimport; the certificate keeps the ones it was imported with.
        acm_client
            .reimport_certificate(
                alien_aws_clients::acm::ReimportCertificateRequest::builder()
                    .certificate_arn(certificate_arn)
                    .certificate(leaf)
                    .private_key(private_key.clone())
                    .maybe_certificate_chain(chain)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to re-import renewed certificate to ACM".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.certificate_issued_at = resource.issued_at.clone();

        Ok(())
    }

    fn owns_network(ctx: &ResourceControllerContext<'_>) -> bool {
        matches!(
            ctx.deployment_config.stack_settings.network,
            Some(NetworkSettings::Create { .. })
        )
    }

    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<Option<DomainInfo>> {
        let stack_settings = &ctx.deployment_config.stack_settings;
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let cert_arn = custom
                .certificate
                .aws
                .as_ref()
                .map(|cert| cert.certificate_arn.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an AWS certificate ARN".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(Some(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                certificate_arn: Some(cert_arn),
                uses_custom_domain: true,
            }));
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|metadata| metadata.resources.get(resource_id))
        else {
            return Ok(None);
        };

        Ok(Some(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            certificate_arn: None,
            uses_custom_domain: false,
        }))
    }

    fn ensure_domain_info(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        config: &Worker,
    ) -> Result<bool> {
        let resource_id = config.id.as_str();
        if self.fqdn.is_some()
            && self.domain_name.is_some()
            && (self.certificate_id.is_some()
                || self.certificate_arn.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id)? {
            Some(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.domain_name = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.certificate_arn = domain_info.certificate_arn;
                self.uses_custom_domain = domain_info.uses_custom_domain;
                if self.url.is_none() {
                    self.url = config
                        .configured_primary_public_url(
                            ctx.deployment_config.public_endpoints.as_ref(),
                        )
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Streaming forks to REST V1 — the only API Gateway flavor that both
    /// streams and carries a custom domain — while buffered workers stay on
    /// the V2 HTTP API.
    fn gateway_entry_state(worker: &Worker) -> AwsWorkerState {
        if worker_wants_streaming(worker) {
            AwsWorkerState::CreatingRestApi
        } else {
            AwsWorkerState::CreatingApiGateway
        }
    }

    /// Settles a domain name recorded before its create was confirmed. The name stays, now
    /// confirmed, when the domain exists with this worker's tags. It is forgotten when the
    /// domain does not exist or is not this worker's: a domain the role cannot read is one of
    /// those, because the read is granted only for domains carrying the stack's tags. Any other
    /// error keeps the name, so the delete retries.
    async fn confirm_recorded_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker_config: &Worker,
    ) -> Result<()> {
        let Some(domain_name) = self.domain_name.clone() else {
            return Ok(());
        };
        let aws_cfg = ctx.get_aws_config()?;
        // The delete runs against the config the worker was created with, which picked the
        // API Gateway flavor its domain was made in.
        let lookup = if worker_wants_streaming(worker_config) {
            let client = ctx
                .service_provider
                .get_aws_apigateway_client(aws_cfg)
                .await?;
            client
                .get_domain_name(&domain_name)
                .await
                .map(|domain| domain.tags)
        } else {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            client
                .get_domain_name(&domain_name)
                .await
                .map(|domain| domain.tags)
        };

        let ours = standard_resource_tags(ctx.resource_prefix, &worker_config.id);
        match lookup {
            Ok(tags) if carries_tags(tags.as_ref(), &ours) => {
                info!(worker=%worker_config.id, domain=%domain_name, "Recorded custom domain exists and is this worker's");
                self.domain_confirmed = true;
            }
            Ok(_) => {
                warn!(worker=%worker_config.id, domain=%domain_name, "Custom domain exists but is not this worker's; leaving it");
                self.forget_domain();
            }
            Err(error) if is_remote_not_found(&error) || is_remote_access_denied(&error) => {
                info!(worker=%worker_config.id, domain=%domain_name, "Recorded custom domain was never created");
                self.forget_domain();
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to look up custom domain '{domain_name}'"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        }
        Ok(())
    }

    /// Drops a domain this worker must not delete, with the mappings recorded under it.
    fn forget_domain(&mut self) {
        self.domain_name = None;
        self.domain_confirmed = false;
        self.api_mapping_id = None;
        self.rest_base_path = None;
    }

    /// The tags a domain create carries: this worker's ownership tags plus the create token.
    ///
    /// `None` means no token was saved yet: the caller records the new one and returns, and the
    /// create runs in the next step. The executor saves state only between steps, so a token
    /// generated in the step that calls create would be lost if the process stopped after AWS
    /// accepted the call, and the retry would reject the domain as not this worker's.
    fn domain_create_tags(
        &mut self,
        prefix: &str,
        resource_id: &str,
    ) -> Option<HashMap<String, String>> {
        let Some(token) = self.domain_create_token.clone() else {
            self.domain_create_token = Some(Uuid::new_v4().to_string());
            return None;
        };
        let mut tags = standard_resource_tags(prefix, resource_id);
        tags.insert(CREATE_ATTEMPT_TAG.to_string(), token);
        Some(tags)
    }

    fn foreign_function_error(
        function_name: &str,
        resource_id: &str,
        conflict: AlienError<CloudClientErrorData>,
    ) -> AlienError<ErrorData> {
        conflict.context(ErrorData::CloudPlatformError {
            message: format!(
                "Lambda function '{function_name}' already exists and was not created by this worker"
            ),
            resource_id: Some(resource_id.to_string()),
        })
    }

    fn foreign_domain_error(
        fqdn: &str,
        resource_id: &str,
        conflict: AlienError<CloudClientErrorData>,
    ) -> AlienError<ErrorData> {
        conflict.context(ErrorData::CloudPlatformError {
            message: format!(
                "API Gateway domain '{fqdn}' already exists and was not created by this worker"
            ),
            resource_id: Some(resource_id.to_string()),
        })
    }

    fn unexpected_update_wrapper_state(
        resource_id: &str,
        handler: &str,
        state: AwsWorkerState,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("{handler} returned unexpected state during update: {state:?}"),
        })
    }
}

#[controller]
pub struct AwsWorkerController {
    pub(crate) arn: Option<String>,
    pub(crate) url: Option<String>,
    /// The logical AWS Lambda worker name (stack prefix + id). Stored to expose in outputs.
    pub(crate) worker_name: Option<String>,
    /// Event source mapping UUIDs for queue triggers
    pub(crate) event_source_mappings: Vec<String>,
    /// Fully qualified domain name for public ingress
    pub(crate) fqdn: Option<String>,
    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,
    /// ACM certificate ARN (auto-imported or custom)
    pub(crate) certificate_arn: Option<String>,
    /// API Gateway HTTP API ID
    pub(crate) api_id: Option<String>,
    /// API Gateway integration ID
    pub(crate) integration_id: Option<String>,
    /// API Gateway route ID
    pub(crate) route_id: Option<String>,
    /// API Gateway stage name
    pub(crate) stage_name: Option<String>,
    /// API Gateway API mapping ID
    pub(crate) api_mapping_id: Option<String>,
    /// API Gateway REST (V1) API ID, set for streaming workers
    #[serde(default)]
    pub(crate) rest_api_id: Option<String>,
    /// REST API root resource ID
    #[serde(default)]
    pub(crate) rest_root_resource_id: Option<String>,
    /// REST API `{proxy+}` resource ID
    #[serde(default)]
    pub(crate) rest_proxy_resource_id: Option<String>,
    /// REST API deployment ID
    #[serde(default)]
    pub(crate) rest_deployment_id: Option<String>,
    /// REST API base path mapping key, stored verbatim from the API (the empty
    /// base path is the literal `(none)`, which deletion must address by key)
    #[serde(default)]
    pub(crate) rest_base_path: Option<String>,
    /// API Gateway custom domain name. Until `domain_confirmed` is set, this is only the name a
    /// create will use: the domain may not exist, or may not be this worker's.
    pub(crate) domain_name: Option<String>,
    /// Whether `domain_name` is known to be this worker's domain: CreateDomainName returned it,
    /// or a read found it carrying this worker's tags. Older versions recorded the name before
    /// creating the domain and saved no such flag, so their state reads as unconfirmed.
    #[serde(default)]
    pub(crate) domain_confirmed: bool,
    /// Token tagged on the domain by its create call, recorded before the call, so a retry
    /// after a lost response adopts that domain and no other.
    #[serde(default)]
    pub(crate) domain_create_token: Option<String>,
    /// Token tagged on the Lambda function by its create call, recorded before the call, so a
    /// retry after a lost response adopts that function and no other.
    #[serde(default)]
    pub(crate) function_create_token: Option<String>,
    /// Token tagged on the certificate this worker imports into ACM, recorded before the import
    /// so a retry after a lost response, and the delete, find that certificate and no other.
    #[serde(default)]
    pub(crate) certificate_import_token: Option<String>,
    /// Endpoint metadata for DNS controller
    pub(crate) load_balancer: Option<LoadBalancerState>,
    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,
    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,
    /// Statement IDs for Lambda permissions granted to S3 for storage triggers
    pub(crate) s3_permission_statement_ids: Vec<String>,
    /// EventBridge rule names for schedule triggers
    pub(crate) eventbridge_rule_names: Vec<String>,
    /// Statement IDs for Lambda permissions granted to EventBridge for schedule triggers
    pub(crate) eventbridge_permission_statement_ids: Vec<String>,
    /// Set by the delete flow once it removed (or found gone) any recorded object, so a delete
    /// denied after that does not put the failed create back.
    #[serde(default)]
    pub(crate) delete_removed_something: bool,
    /// Wall-clock start of the external-network ENI wait, preserved across checkpoints.
    /// The execution role must retain its VPC permissions until this wait finishes.
    #[serde(default)]
    pub(crate) vpc_eni_wait_started_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerEndpoint {
    pub dns_name: String,
    pub hosted_zone_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerState {
    pub endpoint: Option<LoadBalancerEndpoint>,
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    certificate_arn: Option<String>,
    uses_custom_domain: bool,
}

fn emit_aws_lambda_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    worker_config: &Worker,
    aws_worker_name: &str,
    function_info: &FunctionConfiguration,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: worker_config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::AwsLambda(
            AwsLambdaWorkerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!("AWS Lambda function '{aws_worker_name}' is active")),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                function_name: function_info
                    .function_name
                    .clone()
                    .unwrap_or_else(|| aws_worker_name.to_string()),
                runtime: None,
                package_type: None,
                memory_size_mb: None,
                timeout_seconds: None,
                version: None,
                revision_id: None,
                last_modified: None,
                state: function_info.state.clone(),
                state_reason: None,
                state_reason_code: None,
                last_update_status: function_info.last_update_status.clone(),
                last_update_status_reason: None,
                last_update_status_reason_code: None,
                code_sha256: None,
                layer_count: 0,
                function_url_auth_type: None,
                function_url_cors_present: false,
                trigger_count: worker_config.triggers.len() as u32,
            },
        )),
        raw: vec![],
    });
}

/// Streaming REST (V1) exposure is opt-in per worker through
/// `WORKER_RESPONSE_STREAMING=true` in its environment.
fn worker_wants_streaming(worker: &Worker) -> bool {
    worker
        .environment
        .get("WORKER_RESPONSE_STREAMING")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[controller]
impl AwsWorkerController {
    // DeleteStart only routes. The API gateway step deletes the mapping, domain and API one
    // by one and records each removal, so a denial part-way through it is not mistaken for
    // nothing deleted.
    fn nothing_deleted_yet(&self) -> bool {
        matches!(
            self.state,
            AwsWorkerState::DeleteStart | AwsWorkerState::DeletingApiGateway
        ) && !self.delete_removed_something
    }

    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let cfg = ctx.desired_resource_config::<Worker>()?;
        info!(name=%cfg.id, code=?cfg.code, "Initiating creation");

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

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's role ARN
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;

        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let image_uri = match &cfg.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Resolve proxy URIs to native ECR URIs. Lambda can only pull from ECR.
        // The release stores proxy URIs; native_image_host carries the ECR prefix.
        let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
            alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                .unwrap_or(image_uri)
        } else {
            image_uri
        };

        // Lambda requires container images in the same region as the worker.
        // If the image URI points to ECR in a different region (e.g., the management
        // region), rewrite it to reference the local region where the replicated copy
        // lives. ECR private image replication must be configured separately.
        let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

        let code = FunctionCode::builder().image_uri(image_uri).build();
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &cfg.id);
        let mut function_tags = standard_resource_tags(ctx.resource_prefix, &cfg.id);
        function_tags.insert("Name".to_string(), aws_worker_name.clone());

        if !cfg.public_endpoints.is_empty() {
            match Self::resolve_domain_info(ctx, &cfg.id)? {
                Some(domain_info) => {
                    self.fqdn = Some(domain_info.fqdn.clone());
                    self.certificate_id = domain_info.certificate_id;
                    self.certificate_arn = domain_info.certificate_arn;
                    self.uses_custom_domain = domain_info.uses_custom_domain;
                    self.domain_name = Some(domain_info.fqdn.clone());

                    // Check for URL override in deployment config, otherwise use domain FQDN
                    self.url = cfg
                        .configured_primary_public_url(
                            ctx.deployment_config.public_endpoints.as_ref(),
                        )
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                None => {
                    // Standalone mode: no domain metadata available.
                    // Use API Gateway with its default endpoint URL (no custom domain).
                    // The URL will be set after API Gateway creation.
                    info!(
                        worker=%cfg.id,
                        "No domain metadata — will use API Gateway default endpoint (standalone mode)"
                    );
                }
            }
        }

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, &aws_worker_name)
            .await?;

        let environment = if !env_vars.is_empty() {
            Some(Environment::builder().variables(env_vars).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = linked_containers::vpc_config(ctx, self.get_vpc_config(ctx)?).await?;
        if vpc_config.is_some() {
            info!(name=%aws_worker_name, "Configuring Lambda worker to run inside VPC");
        }

        // The create token is saved by a step of its own, after the checks above and before the
        // step's first call that changes anything. The executor saves state only between steps,
        // so a token made in the creating step would be lost if the process stopped after Lambda
        // accepted the call.
        let Some(create_token) = self.function_create_token.clone() else {
            self.function_create_token = Some(Uuid::new_v4().to_string());
            return Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: None,
            });
        };
        function_tags.insert(CREATE_ATTEMPT_TAG.to_string(), create_token);

        Self::ensure_log_group(ctx, aws_cfg, &aws_worker_name, &cfg.id).await?;

        let request = CreateFunctionRequest::builder()
            .function_name(aws_worker_name.clone())
            .role(role_arn)
            .code(code)
            .package_type("Image".to_string())
            .description(format!("Runtime worker: {}", cfg.id))
            .timeout(cfg.timeout_seconds as i32)
            .memory_size(cfg.memory_mb as i32)
            .publish(false)
            .tags(function_tags.clone())
            .maybe_environment(environment)
            .architectures(vec!["arm64".to_string()])
            .maybe_vpc_config(vpc_config)
            .build();

        let function_arn = match client.create_function(request).await {
            Ok(response) => response.function_arn,
            Err(error) if is_remote_resource_conflict(&error) => {
                // Function names are unique per account and region, and CreateFunction takes no
                // client token. Our own earlier call may have created the function before its
                // response was lost: that function carries this create's token, any other does
                // not. The read is granted only on functions tagged for this stack, so a denied
                // read means the function is not ours.
                match client.get_function(&aws_worker_name).await {
                    Ok(existing) if carries_tags(existing.tags.as_ref(), &function_tags) => {
                        info!(name=%aws_worker_name, "Function already exists and carries this create's token; adopting it");
                        existing.configuration.function_arn
                    }
                    Ok(_) => {
                        return Err(Self::foreign_function_error(
                            &aws_worker_name,
                            &cfg.id,
                            error,
                        ))
                    }
                    Err(lookup)
                        if is_remote_not_found(&lookup) || is_remote_access_denied(&lookup) =>
                    {
                        return Err(Self::foreign_function_error(
                            &aws_worker_name,
                            &cfg.id,
                            error,
                        ))
                    }
                    Err(lookup) => {
                        return Err(lookup.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to look up Lambda function '{aws_worker_name}' after a create conflict"
                            ),
                            resource_id: Some(cfg.id.clone()),
                        }))
                    }
                }
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create Lambda worker".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }))
            }
        };

        self.arn = function_arn;
        self.worker_name = Some(aws_worker_name.clone());
        info!(name=%aws_worker_name, arn=%self.arn.as_deref().unwrap_or("unknown"), "Worker created, waiting for active state");

        Ok(HandlerAction::Continue {
            state: CreateWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = CreateWaitForActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        debug!(name=%aws_worker_name, "Checking worker state");

        let response = client
            .get_function_configuration(&aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Lambda worker configuration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Check if worker is active based on state and last_update_status
        let is_active = response.state.as_deref() == Some("Active")
            && response.last_update_status.as_deref() == Some("Successful");

        if is_active {
            if !worker_config.public_endpoints.is_empty() {
                let has_domain_info = self.ensure_domain_info(ctx, &worker_config)?;
                let next_state = if has_domain_info {
                    // Platform mode: wait for certificate then create API Gateway + custom domain
                    WaitingForCertificate
                } else {
                    // Standalone mode: skip certificate/custom domain, use API Gateway default endpoint
                    Self::gateway_entry_state(&worker_config)
                };
                Ok(HandlerAction::Continue {
                    state: next_state,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: ApplyingResourcePermissions,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            }
        } else {
            debug!(
                name = %aws_worker_name,
                state = %response.state.as_deref().unwrap_or("unknown"),
                last_update_status = %response.last_update_status.as_deref().unwrap_or("unknown"),
                "Worker not yet active, retrying"
            );
            Ok(HandlerAction::Stay {
                max_times: Some(AWS_LAMBDA_ACTIVE_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(3)),
            })
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
        if !self.ensure_domain_info(ctx, &worker_config)? {
            return Ok(HandlerAction::Continue {
                state: Self::gateway_entry_state(&worker_config),
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }
        if self.uses_custom_domain && self.certificate_arn.is_some() {
            return Ok(HandlerAction::Continue {
                state: Self::gateway_entry_state(&worker_config),
                suggested_delay: Some(Duration::from_secs(1)),
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
                max_times: Some(CERTIFICATE_WAIT_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(CERTIFICATE_WAIT_POLL_SECS)),
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
        self.ensure_domain_info(ctx, &worker_config)?;
        if self.certificate_arn.is_some() {
            // A recorded certificate is not imported again.
            return Ok(HandlerAction::Continue {
                state: Self::gateway_entry_state(&worker_config),
                suggested_delay: None,
            });
        }
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

        let (leaf, chain) = split_certificate_chain(certificate_chain);

        // The token is saved by a step of its own, before the step that imports. The executor
        // saves state only between steps, so a token made in the importing step would be lost
        // if the process stopped after ACM accepted the import, and the retry would import a
        // second certificate.
        let Some(token) = self.certificate_import_token.clone() else {
            self.certificate_import_token = Some(Uuid::new_v4().to_string());
            return Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            });
        };

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        // Every ImportCertificate without an ARN makes a new certificate, so an earlier import
        // under this token whose response was lost is looked up first.
        let earlier_import = certificates_imported_with_token(acm_client.as_ref(), &token)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up certificates imported by an earlier attempt"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?
            .and_then(|found| found.into_iter().next());
        let certificate_arn = match earlier_import {
            Some(certificate_arn) => {
                info!(worker=%worker_config.id, certificate_arn=%certificate_arn, "Adopting the certificate an earlier import made");
                certificate_arn
            }
            None => {
                let tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id)
                    .into_iter()
                    .map(|(key, value)| alien_aws_clients::acm::Tag { key, value })
                    .collect();
                acm_client
                    .import_certificate(
                        alien_aws_clients::acm::ImportCertificateRequest::builder()
                            .certificate(leaf)
                            .private_key(private_key.clone())
                            .maybe_certificate_chain(chain)
                            .tags(with_import_token(tags, &token))
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to import certificate to ACM".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    })?
                    .certificate_arn
            }
        };

        self.certificate_arn = Some(certificate_arn);

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: Self::gateway_entry_state(&worker_config),
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingApiGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Streaming workers reroute to REST V1 (see gateway_entry_state).
        if worker_wants_streaming(&worker_config) {
            return Ok(HandlerAction::Continue {
                state: CreatingRestApi,
                suggested_delay: None,
            });
        }

        if self.api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let api_tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id);

        let api = client
            .create_api(
                CreateApiRequest::builder()
                    .name(format!("{}-{}-api", ctx.resource_prefix, worker_config.id))
                    .protocol_type("HTTP".to_string())
                    .tags(api_tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API Gateway HTTP API".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let api_id = api.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "API Gateway ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.api_id = Some(api_id);

        Ok(HandlerAction::Continue {
            state: CreatingApiIntegration,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiIntegration,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.integration_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let function_arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration = client
            .create_integration(
                &api_id,
                CreateIntegrationRequest::builder()
                    .integration_type("AWS_PROXY".to_string())
                    .integration_uri(function_arn)
                    .payload_format_version("2.0".to_string())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let integration_id = integration.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Integration ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.integration_id = Some(integration_id.clone());

        Ok(HandlerAction::Continue {
            state: CreatingApiRoute,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiRoute,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.route_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration_id = self.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Integration ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let route = client
            .create_route(
                &api_id,
                CreateRouteRequest::builder()
                    .route_key("$default".to_string())
                    .target(format!("integrations/{}", integration_id))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.route_id = route.route_id.clone();

        Ok(HandlerAction::Continue {
            state: CreatingApiStage,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiStage,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.stage_name.is_some() {
            if self.fqdn.is_some() {
                return Ok(HandlerAction::Continue {
                    state: CreatingApiDomain,
                    suggested_delay: Some(Duration::from_secs(1)),
                });
            }

            let aws_cfg = ctx.get_aws_config()?;
            let api_id = self.api_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "API ID missing for default endpoint".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;
            self.url = Some(format!(
                "https://{}.execute-api.{}.amazonaws.com",
                api_id, aws_cfg.region
            ));
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = client
            .create_stage(
                &api_id,
                CreateStageRequest::builder()
                    .stage_name("$default".to_string())
                    .auto_deploy(true)
                    .tags(standard_resource_tags(
                        ctx.resource_prefix,
                        &worker_config.id,
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.stage_name = stage.stage_name.clone().or(Some("$default".to_string()));

        if self.fqdn.is_some() {
            // Platform mode: proceed to custom domain setup
            Ok(HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        } else {
            // Standalone mode: use the default API Gateway endpoint URL
            let aws_cfg = ctx.get_aws_config()?;
            let region = &aws_cfg.region;
            let default_url = format!("https://{}.execute-api.{}.amazonaws.com", api_id, region);
            info!(
                worker=%worker_config.id,
                url=%default_url,
                "Using API Gateway default endpoint (no custom domain)"
            );
            self.url = Some(default_url);
            Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        }
    }

    #[handler(
        state = CreatingApiDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.load_balancer.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let fqdn = self.fqdn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "FQDN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let Some(tags) = self.domain_create_tags(ctx.resource_prefix, &worker_config.id) else {
            return Ok(HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay: None,
            });
        };
        let domain = match client
            .create_domain_name(
                CreateDomainNameRequest::builder()
                    .domain_name(fqdn.clone())
                    .domain_name_configurations(vec![DomainNameConfiguration::builder()
                        .certificate_arn(cert_arn)
                        .endpoint_type("REGIONAL".to_string())
                        .security_policy("TLS_1_2".to_string())
                        .build()])
                    .tags(tags.clone())
                    .build(),
            )
            .await
        {
            Ok(domain) => domain,
            Err(error) if is_remote_resource_conflict(&error) => {
                // Our own earlier attempt may have created it before its response was lost.
                match client.get_domain_name(&fqdn).await {
                    Ok(existing) if carries_tags(existing.tags.as_ref(), &tags) => existing,
                    Ok(_) => {
                        return Err(Self::foreign_domain_error(&fqdn, &worker_config.id, error))
                    }
                    Err(lookup)
                        if is_remote_not_found(&lookup) || is_remote_access_denied(&lookup) =>
                    {
                        return Err(Self::foreign_domain_error(&fqdn, &worker_config.id, error))
                    }
                    Err(lookup) => {
                        return Err(lookup.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to look up API domain '{fqdn}' after a create conflict"
                            ),
                            resource_id: Some(worker_config.id.clone()),
                        }))
                    }
                }
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create API domain name".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        };
        self.domain_name = Some(fqdn);
        self.domain_confirmed = true;

        let endpoint = domain
            .domain_name_configurations
            .as_ref()
            .and_then(|configs| configs.first())
            .and_then(|config| {
                let dns_name = config.api_gateway_domain_name.clone()?;
                let hosted_zone_id = config.hosted_zone_id.clone()?;
                Some(LoadBalancerEndpoint {
                    dns_name,
                    hosted_zone_id,
                })
            });

        self.load_balancer = Some(LoadBalancerState { endpoint });

        Ok(HandlerAction::Continue {
            state: CreatingApiMapping,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingApiMapping,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.api_mapping_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain_name = self.domain_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain name missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = self
            .stage_name
            .clone()
            .unwrap_or_else(|| "$default".to_string());

        let mapping = client
            .create_api_mapping(
                &domain_name,
                CreateApiMappingRequest::builder()
                    .api_id(api_id.clone())
                    .stage(stage)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.api_mapping_id = mapping.api_mapping_id.clone();

        Ok(HandlerAction::Continue {
            state: AddingApiGatewayPermission,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingRestApi,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_api(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.rest_api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingRestResource,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let api_tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id);

        let api = client
            .create_rest_api(
                CreateRestApiRequest::builder()
                    .name(format!("{}-{}-api", ctx.resource_prefix, worker_config.id))
                    .endpoint_configuration(RestEndpointConfiguration {
                        types: vec!["REGIONAL".to_string()],
                    })
                    .tags(api_tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API Gateway REST API".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let rest_api_id = api.id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "REST API ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let root_resource_id = api.root_resource_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "REST API root resource ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.rest_api_id = Some(rest_api_id);
        self.rest_root_resource_id = Some(root_resource_id);

        Ok(HandlerAction::Continue {
            state: CreatingRestResource,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingRestResource,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_resource(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.rest_proxy_resource_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingRestMethods,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let rest_api_id = self.rest_api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API ID missing for proxy resource".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let root_resource_id = self.rest_root_resource_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API root resource ID missing for proxy resource".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let resource = client
            .create_resource(
                &rest_api_id,
                &root_resource_id,
                CreateResourceRequest {
                    path_part: "{proxy+}".to_string(),
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create REST API proxy resource".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let proxy_resource_id = resource.id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "REST API proxy resource ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.rest_proxy_resource_id = Some(proxy_resource_id);

        Ok(HandlerAction::Continue {
            state: CreatingRestMethods,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingRestMethods,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_methods(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let rest_api_id = self.rest_api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API ID missing for methods".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let root_resource_id = self.rest_root_resource_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API root resource ID missing for methods".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let proxy_resource_id = self.rest_proxy_resource_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API proxy resource ID missing for methods".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let function_arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN missing for REST integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration_uri = format!(
            "arn:aws:apigateway:{}:lambda:path/2021-11-15/functions/{}/response-streaming-invocations",
            aws_cfg.region, function_arn
        );

        // Both the root resource and `{proxy+}` need the ANY method so the REST
        // API catches every path, matching the V2 `$default` catch-all route.
        // put_integration overwrites idempotently on retry, but put_method
        // returns ConflictException once the method exists (which the client
        // maps to a *retryable* error) — so a retry after a mid-loop throttle
        // would loop to CreateFailed. Tolerate that conflict as success.
        for resource_id in [&root_resource_id, &proxy_resource_id] {
            if let Err(e) = client
                .put_method(
                    &rest_api_id,
                    resource_id,
                    "ANY",
                    PutMethodRequest {
                        authorization_type: "NONE".to_string(),
                    },
                )
                .await
            {
                if !is_remote_resource_conflict(&e) {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to put REST API method".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }

            client
                .put_integration(
                    &rest_api_id,
                    resource_id,
                    "ANY",
                    PutIntegrationRequest::builder()
                        .integration_type("AWS_PROXY".to_string())
                        .integration_http_method("POST".to_string())
                        .uri(integration_uri.clone())
                        .response_transfer_mode("STREAM".to_string())
                        .timeout_in_millis(900_000)
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to put REST API streaming integration".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingRestDeployment,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingRestDeployment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_deployment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        let rest_api_id = self.rest_api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API ID missing for deployment".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;

        if self.rest_deployment_id.is_none() {
            let deployment = client
                .create_deployment(
                    &rest_api_id,
                    CreateDeploymentRequest {
                        stage_name: "prod".to_string(),
                    },
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create REST API deployment".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?;

            let deployment_id = deployment.id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "REST API deployment ID not returned".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

            self.rest_deployment_id = Some(deployment_id);
            self.stage_name = Some("prod".to_string());
        }

        // CreateDeployment takes no tags, so the stage it creates would sit outside
        // the deployment's tag boundary that every management grant is conditioned
        // on. Tagging runs outside the creation guard because it is idempotent and
        // a retry must reach it even once the deployment id is recorded.
        let region = &aws_cfg.region;
        client
            .tag_resource(
                &format!("arn:aws:apigateway:{region}::/restapis/{rest_api_id}/stages/prod"),
                standard_resource_tags(ctx.resource_prefix, &worker_config.id),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to tag REST API stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if self.fqdn.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingRestDomain,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        } else {
            // Standalone mode: the REST default endpoint serves under /{stage},
            // unlike the V2 `$default` stage which serves at the domain root.
            self.url = Some(format!(
                "https://{}.execute-api.{}.amazonaws.com/prod",
                rest_api_id, aws_cfg.region
            ));
            Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        }
    }

    #[handler(
        state = CreatingRestDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.load_balancer.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingRestBasePathMapping,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let fqdn = self.fqdn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "FQDN missing for REST API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for REST API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let Some(tags) = self.domain_create_tags(ctx.resource_prefix, &worker_config.id) else {
            return Ok(HandlerAction::Continue {
                state: CreatingRestDomain,
                suggested_delay: None,
            });
        };
        let domain = match client
            .create_domain_name(
                CreateRestDomainNameRequest::builder()
                    .domain_name(fqdn.clone())
                    .regional_certificate_arn(cert_arn)
                    .endpoint_configuration(RestEndpointConfiguration {
                        types: vec!["REGIONAL".to_string()],
                    })
                    .security_policy("TLS_1_2".to_string())
                    .tags(tags.clone())
                    .build(),
            )
            .await
        {
            Ok(domain) => domain,
            Err(error) if is_remote_resource_conflict(&error) => {
                // Our own earlier attempt may have created it before its response was lost.
                match client.get_domain_name(&fqdn).await {
                    Ok(existing) if carries_tags(existing.tags.as_ref(), &tags) => existing,
                    Ok(_) => {
                        return Err(Self::foreign_domain_error(&fqdn, &worker_config.id, error))
                    }
                    Err(lookup)
                        if is_remote_not_found(&lookup) || is_remote_access_denied(&lookup) =>
                    {
                        return Err(Self::foreign_domain_error(&fqdn, &worker_config.id, error))
                    }
                    Err(lookup) => {
                        return Err(lookup.context(ErrorData::CloudPlatformError {
                            message: format!(
                            "Failed to look up REST API domain '{fqdn}' after a create conflict"
                        ),
                            resource_id: Some(worker_config.id.clone()),
                        }))
                    }
                }
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create REST API domain name".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        };

        let endpoint = domain.regional_domain_name.clone().and_then(|dns_name| {
            let hosted_zone_id = domain.regional_hosted_zone_id.clone()?;
            Some(LoadBalancerEndpoint {
                dns_name,
                hosted_zone_id,
            })
        });

        self.domain_name = Some(fqdn);
        self.domain_confirmed = true;
        self.load_balancer = Some(LoadBalancerState { endpoint });

        Ok(HandlerAction::Continue {
            state: CreatingRestBasePathMapping,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = CreatingRestBasePathMapping,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_rest_base_path_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.rest_base_path.is_some() {
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigateway_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let rest_api_id = self.rest_api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "REST API ID missing for base path mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain_name = self.domain_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain name missing for base path mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = self
            .stage_name
            .clone()
            .unwrap_or_else(|| "prod".to_string());

        let mapping = client
            .create_base_path_mapping(
                &domain_name,
                CreateBasePathMappingRequest {
                    rest_api_id,
                    stage,
                    base_path: None,
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create REST API base path mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // The API reports the empty base path as the literal `(none)`, which is
        // also the key deletion must use.
        self.rest_base_path = Some(
            mapping
                .base_path
                .clone()
                .unwrap_or_else(|| "(none)".to_string()),
        );

        Ok(HandlerAction::Continue {
            state: AddingApiGatewayPermission,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    #[handler(
        state = AddingApiGatewayPermission,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        // Without a source ARN the grant lets any API Gateway in any account
        // invoke this function, so scope it to whichever API fronts the worker.
        let api_id = self
            .rest_api_id
            .as_ref()
            .or(self.api_id.as_ref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "API Gateway permission needs the API it fronts".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;
        let request = AddPermissionRequest::builder()
            .statement_id("ApiGatewayInvoke".to_string())
            .action("lambda:InvokeFunction".to_string())
            .principal("apigateway.amazonaws.com".to_string())
            .source_arn(format!(
                "arn:aws:execute-api:{}:{}:{api_id}/*",
                aws_cfg.region, aws_cfg.account_id
            ))
            .build();

        client
            .add_permission(&aws_worker_name, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add API Gateway permission".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if self.fqdn.is_some() {
            if self.uses_custom_domain {
                // Custom domain: readiness probe then done
                Ok(HandlerAction::Continue {
                    state: RunningReadinessProbe,
                    suggested_delay: None,
                })
            } else {
                // Platform-managed domain: wait for DNS propagation
                Ok(HandlerAction::Continue {
                    state: WaitingForDns,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        } else {
            // Standalone mode: no custom domain, skip DNS and readiness probe
            Ok(HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay: None,
            })
        }
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
            Some(DnsRecordStatus::Active) => Ok(HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay: None,
            }),
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
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && !worker_config.public_endpoints.is_empty() {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: Some(READINESS_PROBE_MAX_ATTEMPTS),
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded - proceed to ApplyingResourcePermissions
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
        let config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%config.id, "Applying resource-scoped permissions for Lambda worker");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(worker_name) = &self
            .arn
            .as_ref()
            .and_then(|arn| arn.split(':').last().map(|s| s.to_string()))
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &worker_name,
                "worker",
            )
            .await?;
        }

        info!(worker=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: UpdatingEnvVarsWithSelfBinding,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingEnvVarsWithSelfBinding,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn updating_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        self.ensure_batch_queue_visibility(ctx, &config, false)
            .await?;

        // Skip this step if the worker doesn't have public ingress
        // For private workers, the initial env vars already have complete self-binding
        // (no URL to add later)
        if config.public_endpoints.is_empty() {
            info!(worker=%config.id, "Skipping env var update - no public URL to add");
            return Ok(HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        // Now that we have the URL, update the environment variables
        // with the complete self-binding information including the URL
        let final_env_vars = self
            .prepare_environment_variables(
                &config.environment,
                &config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(Environment::builder().variables(final_env_vars).build())
        } else {
            None
        };

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;
        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Get VPC configuration if a Network resource exists
        let vpc_config = linked_containers::vpc_config(ctx, self.get_vpc_config(ctx)?).await?;

        let request = UpdateFunctionConfigurationRequest::builder()
            .role(role_arn)
            .timeout(config.timeout_seconds as i32)
            .memory_size(config.memory_mb as i32)
            .maybe_environment(lambda_environment)
            .maybe_vpc_config(vpc_config)
            .build();

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for env var update".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda worker configuration with self-binding"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(worker=%config.id, "Successfully updated environment variables with complete self-binding");

        Ok(HandlerAction::Continue {
            state: CreatingEventSourceMappings,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingEventSourceMappings,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    config.id, queue_trigger_count
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Linear flow principle: Always perform this state. Create mappings for ALL queue triggers
        let mut created_any = false;
        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Queue { queue, .. } = trigger {
                info!(worker=%config.id, queue=%queue.id, "Creating SQS event source mapping");
                self.create_queue_event_source_mapping(ctx, aws_cfg, &config, queue)
                    .await?;
                created_any = true;
            }
        }
        if !created_any {
            info!(worker=%config.id, "No queue triggers found, skipping event source mapping creation");
        }

        // Handle storage triggers: configure S3 bucket notifications to invoke this Lambda
        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().unwrap_or("unknown");

        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Storage {
                storage: storage_ref,
                events,
            } = trigger
            {
                info!(worker=%config.id, storage=%storage_ref.id, "Configuring S3 storage trigger");

                // Get storage controller to access bucket name
                let storage_controller =
                    ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                let bucket_name = storage_controller.bucket_name.as_deref().ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: config.id.clone(),
                        dependency_id: storage_ref.id.clone(),
                    })
                })?;

                // Add Lambda permission for S3 to invoke this worker
                let statement_id = format!("{}-s3-{}", worker_name, storage_ref.id);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionRequest::builder()
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("s3.amazonaws.com".to_string())
                    .source_arn(format!("arn:aws:s3:::{}", bucket_name))
                    .build();

                match lambda_client
                    .add_permission(worker_name, permission_request)
                    .await
                {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "S3 invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add S3 invoke permission for storage '{}'",
                                storage_ref.id
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Get current notification config and merge in new Lambda config
                let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                let mut notification_config = s3_client
                    .get_bucket_notification_configuration(bucket_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get notification configuration for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                // Map alien event types to S3 event types
                let s3_events: Vec<String> = events
                    .iter()
                    .map(|e| match e.as_str() {
                        "created" => "s3:ObjectCreated:*".to_string(),
                        "deleted" => "s3:ObjectRemoved:*".to_string(),
                        other => format!("s3:{}", other),
                    })
                    .collect();

                replace_lambda_notification_config(
                    &mut notification_config,
                    LambdaFunctionConfiguration {
                        id: Some(statement_id.clone()),
                        lambda_function_arn: function_arn.to_string(),
                        events: s3_events,
                        filter: None,
                    },
                );

                s3_client
                    .put_bucket_notification_configuration(bucket_name, &notification_config)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to put notification configuration for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                if !self.s3_permission_statement_ids.contains(&statement_id) {
                    self.s3_permission_statement_ids.push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    storage=%storage_ref.id,
                    bucket=%bucket_name,
                    statement_id=%statement_id,
                    "S3 storage trigger configured"
                );
            }
        }

        // Continue to schedule trigger creation (linear flow)
        Ok(HandlerAction::Continue {
            state: CreatingScheduleTriggers,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingScheduleTriggers,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for schedule trigger".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        for (index, trigger) in config.triggers.iter().enumerate() {
            if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                info!(worker=%config.id, cron=%cron, index=%index, "Creating EventBridge schedule trigger");

                let rule_name = format!("{}-cron-{}", worker_name, index);

                // Convert standard 5-field cron to EventBridge format
                let schedule_expression =
                    crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron).map_err(
                        |e| {
                            AlienError::new(ErrorData::ResourceConfigInvalid {
                                message: format!("Invalid cron expression '{}': {}", cron, e),
                                resource_id: Some(config.id.clone()),
                            })
                        },
                    )?;

                // Create EventBridge rule
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                let rule_response = eventbridge_client
                    .put_rule(PutRuleRequest {
                        name: rule_name.clone(),
                        schedule_expression,
                        state: Some("ENABLED".to_string()),
                        description: Some(format!("Schedule trigger for worker '{}'", config.id)),
                        tags: Some(eventbridge_tags(ctx.resource_prefix, &config.id)),
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create EventBridge rule '{}'", rule_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                let rule_arn = rule_response.rule_arn.unwrap_or_default();

                // Add Lambda permission for EventBridge
                let statement_id = format!("{}-eb-{}", worker_name, index);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionRequest::builder()
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("events.amazonaws.com".to_string())
                    .source_arn(rule_arn)
                    .build();

                match lambda_client
                    .add_permission(worker_name, permission_request)
                    .await
                {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "EventBridge invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add EventBridge invoke permission for rule '{}'",
                                rule_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Add Lambda as the target of the rule
                eventbridge_client
                    .put_targets(PutTargetsRequest {
                        rule: rule_name.clone(),
                        targets: vec![EventBridgeTarget {
                            id: "1".to_string(),
                            arn: function_arn.to_string(),
                        }],
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to add target to EventBridge rule '{}'",
                            rule_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                if !self.eventbridge_rule_names.contains(&rule_name) {
                    self.eventbridge_rule_names.push(rule_name.clone());
                }
                if !self
                    .eventbridge_permission_statement_ids
                    .contains(&statement_id)
                {
                    self.eventbridge_permission_statement_ids
                        .push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    rule_name=%rule_name,
                    statement_id=%statement_id,
                    "EventBridge schedule trigger created"
                );
            }
        }

        // Continue to concurrency configuration (linear flow)
        Ok(HandlerAction::Continue {
            state: SettingConcurrency,
            suggested_delay: None,
        })
    }

    #[handler(
        state = SettingConcurrency,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if let Some(limit) = config.concurrency_limit {
            info!(worker=%config.id, limit=%limit, "Setting reserved concurrency on worker");
            client
                .put_function_concurrency(&aws_worker_name, limit)
                .await
                .map_err(|error| lambda_concurrency_error(error, &aws_cfg.region))
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to set worker reserved concurrency".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        } else {
            debug!(worker=%config.id, "No concurrency limit configured, skipping");
        }

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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        // Heartbeat check: verify worker still exists and is in correct state
        let function_info = client
            .get_function_configuration(&aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration during heartbeat check".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Verify worker is in active state - drift is non-retryable
        if function_info.state.as_deref() != Some("Active") {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker_config.id.clone(),
                message: format!(
                    "Worker state is '{}', expected 'Active'",
                    function_info.state.as_deref().unwrap_or("unknown")
                ),
            }));
        }

        emit_aws_lambda_worker_heartbeat(ctx, &worker_config, &aws_worker_name, &function_info);

        self.renew_certificate(ctx).await?;

        // A linked resource can change what its binding publishes without this worker being
        // updated: a sandbox promoting a rebuilt image version after this worker rendered, or a
        // re-registration. Compare the bindings rendered now with the ones Lambda runs, and
        // refresh only when they differ.
        if binding_check_due(&aws_worker_name, Utc::now()) {
            if let Some(expected) = self.linked_binding_environment(ctx, &worker_config).await? {
                // The comparison is optional to the heartbeat: a throttled or failed read skips
                // it until the next window instead of failing a healthy worker.
                match client.get_function_environment(&aws_worker_name).await {
                    Ok(live) => {
                        let drifted = linked_bindings::drifted_bindings(&expected, &live);
                        if !drifted.is_empty() {
                            // Names only: binding values can carry secrets.
                            info!(
                                worker = %worker_config.id,
                                variables = ?drifted,
                                reason = "linked binding drift",
                                "Refreshing the worker's linked bindings"
                            );
                            return Ok(HandlerAction::Continue {
                                state: RefreshingLinkedBindings,
                                suggested_delay: None,
                            });
                        }
                    }
                    Err(error) => {
                        warn!(
                            worker = %worker_config.id,
                            code = %error.code,
                            "Skipped the linked binding comparison; it runs again in the next window"
                        );
                    }
                }
            }
        }

        debug!(name = %worker_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    /// Re-renders the environment with the linked bindings as they are now and applies it. Only
    /// the environment changes; the worker's configuration is unchanged. The worker reports
    /// Updating meanwhile, so the planner waits for the refresh before starting an update (which
    /// only begins from Ready) and dependents wait for the binding to settle.
    #[handler(
        state = RefreshingLinkedBindings,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn refreshing_linked_bindings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let environment = self
            .prepare_environment_variables(
                &worker_config.environment,
                &worker_config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;
        let arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for a binding refresh".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        client
            .update_function_configuration(
                &arn,
                UpdateFunctionConfigurationRequest::builder()
                    .environment(Environment::builder().variables(environment).build())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to refresh the worker's linked bindings".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: RefreshingLinkedBindingsWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = RefreshingLinkedBindingsWaitForActive,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn refreshing_linked_bindings_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let result = client
            .get_function_configuration(&aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read the worker after a binding refresh".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
        match (result.state.as_deref(), result.last_update_status.as_deref()) {
            (Some("Active"), Some("Successful")) => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            (Some("Pending"), _) | (_, Some("InProgress")) => Ok(HandlerAction::Stay {
                max_times: Some(AWS_LAMBDA_ACTIVE_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
            (state, status) => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "The worker's binding refresh failed. State: {state:?}, Last Update: {status:?}"
                ),
                resource_id: Some(worker_config.id.clone()),
            })),
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
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // A worker created while the management role could not manage its
        // log group gets the retention period once setup grants it.
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        Self::ensure_log_group(
            ctx,
            ctx.get_aws_config()?,
            &aws_worker_name,
            &worker_config.id,
        )
        .await?;

        self.renew_certificate(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdateCodeStart,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateCodeStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Older heartbeats entered this update state after installing a certificate,
        // even though the executor had never planned an update for this worker.
        if self.certificate_arn.is_some()
            && self.certificate_issued_at.is_some()
            && ctx
                .state
                .resources
                .get(ctx.desired_config.id())
                .is_some_and(|resource| resource.previous_config.is_none())
        {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }

        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        self.ensure_batch_queue_visibility(ctx, &current_config, false)
            .await?;
        let code_changed = current_config.code != previous_config.code;

        // UpdateCodeStart only handles code updates if needed
        if code_changed {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            let image_uri = match &current_config.code {
                alien_core::WorkerCode::Image { image } => image.clone(),
                alien_core::WorkerCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Worker is configured with source code for update, but only pre-built images are supported".to_string(),
                        resource_id: Some(current_config.id.clone()),
                    }));
                }
            };

            // Resolve proxy URIs to native ECR URIs.
            let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
                alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                    .unwrap_or(image_uri)
            } else {
                image_uri
            };

            let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

            let request = UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri)
                .publish(true)
                .build();

            let arn = self.arn.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker ARN not available for code update".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })
            })?;

            client.update_function_code(arn, request).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to update Lambda worker code".to_string(),
                    resource_id: Some(current_config.id.clone()),
                },
            )?;
        }

        // Always transition to wait for code update (even if no code change) - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateCodeWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateCodeWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for code status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for code update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            },
        )?;

        let is_active = result.state.as_deref() == Some("Active");
        let is_successful = result.last_update_status.as_deref() == Some("Successful");

        if is_active && is_successful {
            // Always proceed to config update phase - linear flow
            Ok(HandlerAction::Continue {
                state: UpdateConfigStart,
                suggested_delay: None,
            })
        } else if result.state.as_deref() == Some("Pending")
            || result.last_update_status.as_deref() == Some("InProgress")
        {
            Ok(HandlerAction::Stay {
                max_times: Some(AWS_LAMBDA_ACTIVE_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Code update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status,
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
    }

    #[handler(
        state = UpdateConfigStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        self.ensure_batch_queue_visibility(ctx, &current_config, false)
            .await?;
        let config_changed = current_config.permissions != previous_config.permissions
            || current_config.memory_mb != previous_config.memory_mb
            || current_config.timeout_seconds != previous_config.timeout_seconds
            || current_config.environment != previous_config.environment
            || current_config.links != previous_config.links
            || (current_config.triggers != previous_config.triggers
                && (current_config.has_queue_batches() || previous_config.has_queue_batches()));

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);

        // An update whose own configuration is unchanged (new code, or a dependency's update)
        // still re-renders the linked bindings: they may have moved since this worker rendered.
        if !config_changed {
            let bindings_drifted = match self
                .linked_binding_environment(ctx, &current_config)
                .await?
            {
                Some(expected) => {
                    let live = client
                        .get_function_environment(&aws_worker_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to read the worker environment for an update"
                                .to_string(),
                            resource_id: Some(current_config.id.clone()),
                        })?;
                    !linked_bindings::drifted_bindings(&expected, &live).is_empty()
                }
                None if current_config.runtime_binding_links().is_empty() => false,
                // A linked resource is still in its own flow; wait for the value it settles on.
                None => {
                    return Err(AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: current_config.id.clone(),
                        dependency_id: linked_bindings::unsettled_link(
                            ctx,
                            &current_config.runtime_binding_links(),
                        )
                        .map(|link| link.id().to_string())
                        .unwrap_or_default(),
                    }))
                }
            };
            if !bindings_drifted {
                return Ok(HandlerAction::Continue {
                    state: UpdateConfigWaitForActive,
                    suggested_delay: None,
                });
            }
            info!(
                worker = %current_config.id,
                reason = "linked binding drift",
                "Re-rendering the worker's linked bindings during its update"
            );
        }

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", current_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's role ARN
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;

        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: current_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let final_env_vars = self
            .prepare_environment_variables(
                &current_config.environment,
                &current_config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(Environment::builder().variables(final_env_vars).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = linked_containers::vpc_config(ctx, self.get_vpc_config(ctx)?).await?;

        let request = UpdateFunctionConfigurationRequest::builder()
            .role(role_arn)
            .timeout(current_config.timeout_seconds as i32)
            .memory_size(current_config.memory_mb as i32)
            .maybe_environment(lambda_environment)
            .maybe_vpc_config(vpc_config)
            .build();

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config update".to_string(),
                resource_id: Some(current_config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda worker configuration".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

        // Always transition to wait state - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateConfigWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateConfigWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for config update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            },
        )?;

        let is_active = result.state.as_deref() == Some("Active");
        let is_successful = result.last_update_status.as_deref() == Some("Successful");

        if is_active && is_successful {
            Ok(HandlerAction::Continue {
                state: UpdateEnsuringPublicExposure,
                suggested_delay: None,
            })
        } else if result.state.as_deref() == Some("Pending")
            || result.last_update_status.as_deref() == Some("InProgress")
        {
            Ok(HandlerAction::Stay {
                max_times: Some(AWS_LAMBDA_ACTIVE_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Config update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
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
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        if current_config.public_endpoints.is_empty() {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        if previous_config.public_endpoints.is_empty()
            && self.api_id.is_none()
            && self.rest_api_id.is_none()
        {
            self.url = None;
        }

        let has_domain_info = self.ensure_domain_info(ctx, &current_config)?;
        if self.api_id.is_some() || self.rest_api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        let next_state = if has_domain_info {
            UpdateWaitingForCertificate
        } else if worker_wants_streaming(&current_config) {
            UpdateCreatingRestApi
        } else {
            UpdateCreatingApiGateway
        };

        Ok(HandlerAction::Continue {
            state: next_state,
            suggested_delay: Some(Duration::from_secs(2)),
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
                state: ImportingCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingRestApi,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestApi,
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
        state = UpdateImportingInitialCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_certificate(ctx).await? {
            HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingRestApi,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestApi,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_certificate",
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
        state = UpdateCreatingApiGateway,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_gateway(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiIntegration,
                suggested_delay,
            }),
            // Reached when a worker with no endpoint yet reruns this state and
            // now wants streaming: creating_api_gateway reroutes to REST V1.
            HandlerAction::Continue {
                state: CreatingRestApi,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestApi,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_gateway",
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
        state = UpdateCreatingApiIntegration,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_integration(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiRoute,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_integration",
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
        state = UpdateCreatingApiRoute,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_route(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiStage,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_route",
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
        state = UpdateCreatingApiStage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_stage(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiDomain,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_stage",
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
        state = UpdateCreatingApiDomain,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_domain(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiMapping,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiDomain,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_domain",
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
        state = UpdateCreatingApiMapping,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_mapping(ctx).await? {
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_mapping",
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
        state = UpdateCreatingRestApi,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_api(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_api(ctx).await? {
            HandlerAction::Continue {
                state: CreatingRestResource,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestResource,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_api",
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
        state = UpdateCreatingRestResource,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_resource(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_resource(ctx).await? {
            HandlerAction::Continue {
                state: CreatingRestMethods,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestMethods,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_resource",
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
        state = UpdateCreatingRestMethods,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_methods(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_methods(ctx).await? {
            HandlerAction::Continue {
                state: CreatingRestDeployment,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestDeployment,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_methods",
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
        state = UpdateCreatingRestDeployment,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_deployment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_deployment(ctx).await? {
            HandlerAction::Continue {
                state: CreatingRestDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestDomain,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_deployment",
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
        state = UpdateCreatingRestDomain,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_domain(ctx).await? {
            HandlerAction::Continue {
                state: CreatingRestBasePathMapping,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestBasePathMapping,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingRestDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingRestDomain,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_domain",
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
        state = UpdateCreatingRestBasePathMapping,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_rest_base_path_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_rest_base_path_mapping(ctx).await? {
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_rest_base_path_mapping",
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
        state = UpdateAddingApiGatewayPermission,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.adding_api_gateway_permission(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateApplyingResourcePermissions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "adding_api_gateway_permission",
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
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
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
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && !worker_config.public_endpoints.is_empty() {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: Some(READINESS_PROBE_MAX_ATTEMPTS),
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded.
        Ok(HandlerAction::Continue {
            state: UpdateApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateApplyingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.applying_resource_permissions(ctx).await? {
            HandlerAction::Continue {
                state: UpdatingEnvVarsWithSelfBinding,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEnvVarsWithSelfBinding,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "applying_resource_permissions",
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
        state = UpdateEnvVarsWithSelfBinding,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.updating_env_vars_with_self_binding(ctx).await? {
            HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEventSourceMappings,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "updating_env_vars_with_self_binding",
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
        state = UpdateEventSourceMappings,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = current_config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    current_config.id, queue_trigger_count
                ),
                resource_id: Some(current_config.id.clone()),
            }));
        }

        let aws_cfg = ctx.get_aws_config()?;
        if current_config.triggers != previous_config.triggers
            || current_config.has_queue_batches()
            || previous_config.has_queue_batches()
        {
            // Reconcile retained queues in place, including capacity-only updates.
            // Keep all owned IDs until each deletion succeeds so retries retain cleanup ownership.
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
            let mut desired_mappings = std::collections::HashSet::new();
            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Queue { queue, .. } = trigger {
                    desired_mappings.insert(
                        self.create_queue_event_source_mapping(
                            ctx,
                            aws_cfg,
                            &current_config,
                            queue,
                        )
                        .await?,
                    );
                }
            }
            for uuid in self.event_source_mappings.clone() {
                if desired_mappings.contains(&uuid) {
                    continue;
                }
                match client.delete_event_source_mapping(&uuid).await {
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to remove queue event source mapping '{uuid}'"
                            ),
                            resource_id: Some(current_config.id.clone()),
                        }))
                    }
                }
                self.event_source_mappings.retain(|owned| owned != &uuid);
            }
        }

        let triggers_changed = current_config.triggers != previous_config.triggers;
        if triggers_changed {
            // Clean up old S3 storage trigger notifications
            for trigger in &previous_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    ..
                } = trigger
                {
                    if let Ok(storage_controller) =
                        ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)
                    {
                        if let Some(bucket_name) = storage_controller.bucket_name.as_deref() {
                            let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                            let empty_config = NotificationConfiguration::default();
                            if let Err(e) = s3_client
                                .put_bucket_notification_configuration(bucket_name, &empty_config)
                                .await
                            {
                                warn!(
                                    worker=%current_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear old S3 notification configuration (best-effort)"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();

            // Clean up old EventBridge schedule triggers
            if !self.eventbridge_rule_names.is_empty() {
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                for rule_name in &self.eventbridge_rule_names.clone() {
                    if let Err(e) = eventbridge_client
                        .remove_targets(rule_name, vec!["1".to_string()])
                        .await
                    {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to remove targets from old EventBridge rule (best-effort)"
                        );
                    }
                    if let Err(e) = eventbridge_client.delete_rule(rule_name).await {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to delete old EventBridge rule (best-effort)"
                        );
                    }
                }
                self.eventbridge_rule_names.clear();
                self.eventbridge_permission_statement_ids.clear();
            }

            // Recreate storage triggers
            let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
            let function_arn = self.arn.as_deref().unwrap_or("unknown");

            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    events,
                } = trigger
                {
                    let storage_controller = ctx
                        .require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                    let bucket_name =
                        storage_controller.bucket_name.as_deref().ok_or_else(|| {
                            AlienError::new(ErrorData::DependencyNotReady {
                                resource_id: current_config.id.clone(),
                                dependency_id: storage_ref.id.clone(),
                            })
                        })?;

                    let statement_id = format!("{}-s3-{}", worker_name, storage_ref.id);
                    let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                    let permission_request = AddPermissionRequest::builder()
                        .statement_id(statement_id.clone())
                        .action("lambda:InvokeFunction".to_string())
                        .principal("s3.amazonaws.com".to_string())
                        .source_arn(format!("arn:aws:s3:::{}", bucket_name))
                        .build();

                    match lambda_client
                        .add_permission(worker_name, permission_request)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) if is_remote_resource_conflict(&e) => {
                            info!(
                                worker=%current_config.id,
                                statement_id=%statement_id,
                                "S3 invoke permission already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to add S3 invoke permission for storage '{}'",
                                    storage_ref.id
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                    let mut notification_config = s3_client
                        .get_bucket_notification_configuration(bucket_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to get notification configuration for bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    let s3_events: Vec<String> = events
                        .iter()
                        .map(|e| match e.as_str() {
                            "created" => "s3:ObjectCreated:*".to_string(),
                            "deleted" => "s3:ObjectRemoved:*".to_string(),
                            other => format!("s3:{}", other),
                        })
                        .collect();

                    replace_lambda_notification_config(
                        &mut notification_config,
                        LambdaFunctionConfiguration {
                            id: Some(statement_id.clone()),
                            lambda_function_arn: function_arn.to_string(),
                            events: s3_events,
                            filter: None,
                        },
                    );

                    s3_client
                        .put_bucket_notification_configuration(bucket_name, &notification_config)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to put notification configuration for bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    if !self.s3_permission_statement_ids.contains(&statement_id) {
                        self.s3_permission_statement_ids.push(statement_id);
                    }
                }
            }

            // Recreate schedule triggers
            for (index, trigger) in current_config.triggers.iter().enumerate() {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    let rule_name = format!("{}-cron-{}", worker_name, index);
                    let schedule_expression =
                        crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron)
                            .map_err(|e| {
                                AlienError::new(ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid cron expression '{}': {}", cron, e),
                                    resource_id: Some(current_config.id.clone()),
                                })
                            })?;

                    let eventbridge_client = ctx
                        .service_provider
                        .get_aws_eventbridge_client(aws_cfg)
                        .await?;

                    let rule_response = eventbridge_client
                        .put_rule(PutRuleRequest {
                            name: rule_name.clone(),
                            schedule_expression,
                            state: Some("ENABLED".to_string()),
                            description: Some(format!(
                                "Schedule trigger for worker '{}'",
                                current_config.id
                            )),
                            tags: Some(eventbridge_tags(ctx.resource_prefix, &current_config.id)),
                        })
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to create EventBridge rule '{}'", rule_name),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    let rule_arn = rule_response.rule_arn.unwrap_or_default();
                    let statement_id = format!("{}-eb-{}", worker_name, index);
                    let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                    let permission_request = AddPermissionRequest::builder()
                        .statement_id(statement_id.clone())
                        .action("lambda:InvokeFunction".to_string())
                        .principal("events.amazonaws.com".to_string())
                        .source_arn(rule_arn)
                        .build();

                    match lambda_client
                        .add_permission(worker_name, permission_request)
                        .await
                    {
                        Ok(_) => {}
                        Err(e) if is_remote_resource_conflict(&e) => {
                            info!(
                                worker=%current_config.id,
                                statement_id=%statement_id,
                                "EventBridge invoke permission already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to add EventBridge invoke permission for rule '{}'",
                                    rule_name
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    eventbridge_client
                        .put_targets(PutTargetsRequest {
                            rule: rule_name.clone(),
                            targets: vec![EventBridgeTarget {
                                id: "1".to_string(),
                                arn: function_arn.to_string(),
                            }],
                        })
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add target to EventBridge rule '{}'",
                                rule_name
                            ),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    if !self.eventbridge_rule_names.contains(&rule_name) {
                        self.eventbridge_rule_names.push(rule_name);
                    }
                    if !self
                        .eventbridge_permission_statement_ids
                        .contains(&statement_id)
                    {
                        self.eventbridge_permission_statement_ids.push(statement_id);
                    }
                }
            }
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
        }

        Ok(HandlerAction::Continue {
            state: UpdatingConcurrency,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingConcurrency,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let prev_config = ctx.previous_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if config.concurrency_limit != prev_config.concurrency_limit {
            match config.concurrency_limit {
                Some(limit) => {
                    info!(worker=%config.id, limit=%limit, "Updating reserved concurrency on worker");
                    client
                        .put_function_concurrency(&aws_worker_name, limit)
                        .await
                        .map_err(|error| lambda_concurrency_error(error, &aws_cfg.region))
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to update worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
                None => {
                    info!(worker=%config.id, "Removing reserved concurrency from worker");
                    client
                        .delete_function_concurrency(&aws_worker_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to remove worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            }
        } else {
            debug!(worker=%config.id, "Concurrency limit unchanged, skipping");
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
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.url = None;
        Ok(HandlerAction::Continue {
            state: DeletingApiGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingApiGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // A domain name recorded but never confirmed may not exist, or may not be ours. This role
        // may delete only domains that carry our tags, so AWS would answer a delete of a missing
        // one with AccessDenied: check it with a read before deleting anything under its name.
        if self.domain_name.is_some() && !self.domain_confirmed {
            self.confirm_recorded_domain(ctx, &worker_config).await?;
        }

        // Ordering matters: delete API mapping before domain name, domain name before API.
        // A worker is V1 xor V2, so at most one branch of each pair runs. The REST V1
        // order is the same — base path mapping, then domain, then the REST API, whose
        // deletion cascades to resources, methods, deployments, and stages.
        if let (Some(domain_name), true) = (self.domain_name.clone(), self.rest_api_id.is_some()) {
            // An imported worker never learns its mapping key, so fall back to the
            // literal the API reports for an empty base path. Deleting the domain
            // while a mapping still points at it fails.
            let base_path = self
                .rest_base_path
                .clone()
                .unwrap_or_else(|| "(none)".to_string());
            let client = ctx
                .service_provider
                .get_aws_apigateway_client(aws_cfg)
                .await?;
            // The mapping is authorized by its domain's tags, so the domain is the read.
            let outcome = delete_tag_scoped(
                client
                    .delete_base_path_mapping(&domain_name, &base_path)
                    .await,
                || async { client.get_domain_name(&domain_name).await.map(|_| ()) },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete REST base path mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.delete_removed_something = true;
            info!(worker=%worker_config.id, outcome=?outcome, "REST base path mapping removed");
        }
        self.rest_base_path = None;

        if let (Some(domain_name), Some(api_mapping_id)) =
            (self.domain_name.clone(), self.api_mapping_id.clone())
        {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            let outcome = delete_tag_scoped(
                client
                    .delete_api_mapping(&domain_name, &api_mapping_id)
                    .await,
                || async { client.get_domain_name(&domain_name).await.map(|_| ()) },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete API mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.delete_removed_something = true;
            info!(worker=%worker_config.id, outcome=?outcome, "API mapping removed");
        }
        self.api_mapping_id = None;

        if let Some(domain_name) = self.domain_name.clone() {
            let outcome = if self.rest_api_id.is_some() {
                let client = ctx
                    .service_provider
                    .get_aws_apigateway_client(aws_cfg)
                    .await?;
                delete_tag_scoped(client.delete_domain_name(&domain_name).await, || async {
                    client.get_domain_name(&domain_name).await.map(|_| ())
                })
                .await
            } else {
                let client = ctx
                    .service_provider
                    .get_aws_apigatewayv2_client(aws_cfg)
                    .await?;
                delete_tag_scoped(client.delete_domain_name(&domain_name).await, || async {
                    client.get_domain_name(&domain_name).await.map(|_| ())
                })
                .await
            }
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete custom domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.delete_removed_something = true;
            info!(worker=%worker_config.id, domain=%domain_name, outcome=?outcome, "Custom domain removed");
        }
        self.domain_name = None;
        self.domain_confirmed = false;

        // Deleting the API cascades to routes, integrations, and stages.
        if let Some(api_id) = self.api_id.clone() {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            let outcome = delete_tag_scoped(client.delete_api(&api_id).await, || async {
                client.get_api(&api_id).await.map(|_| ())
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete API Gateway".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.delete_removed_something = true;
            info!(worker=%worker_config.id, api_id=%api_id, outcome=?outcome, "API Gateway removed");
        }
        self.api_id = None;
        self.integration_id = None;
        self.route_id = None;

        if let Some(rest_api_id) = self.rest_api_id.clone() {
            let client = ctx
                .service_provider
                .get_aws_apigateway_client(aws_cfg)
                .await?;
            let outcome = delete_tag_scoped(client.delete_rest_api(&rest_api_id).await, || async {
                client.get_rest_api(&rest_api_id).await.map(|_| ())
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete REST API".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.delete_removed_something = true;
            info!(worker=%worker_config.id, rest_api_id=%rest_api_id, outcome=?outcome, "REST API removed");
        }
        self.rest_api_id = None;
        self.rest_root_resource_id = None;
        self.rest_proxy_resource_id = None;
        self.rest_deployment_id = None;
        self.stage_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingEventSourceMappings,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingEventSourceMappings,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Linear flow principle: Always perform this state, even if no event source mappings
        if !self.event_source_mappings.is_empty() {
            info!(worker=%worker_config.id, mappings=?self.event_source_mappings, "Deleting event source mappings");

            // Each mapping leaves the list once it is gone, so a retry resumes after it.
            while let Some(uuid) = self.event_source_mappings.first().cloned() {
                let outcome =
                    delete_tag_scoped(client.delete_event_source_mapping(&uuid).await, || async {
                        client.get_event_source_mapping(&uuid).await.map(|_| ())
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete event source mapping '{}'", uuid),
                        resource_id: Some(worker_config.id.clone()),
                    })?;
                info!(worker=%worker_config.id, uuid=%uuid, outcome=?outcome, "Event source mapping removed");
                self.event_source_mappings.remove(0);
            }
        } else {
            info!(worker=%worker_config.id, "No event source mappings to delete");
        }

        // Clean up S3 storage trigger notifications (best-effort)
        if !self.s3_permission_statement_ids.is_empty() {
            info!(worker=%worker_config.id, "Cleaning up S3 storage trigger notifications");

            // Best-effort: put empty notification configuration on any referenced buckets
            // We don't track which bucket each statement_id maps to, so we attempt to
            // clean up by iterating over storage triggers from the config
            for trigger in &worker_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    ..
                } = trigger
                {
                    if let Ok(storage_controller) =
                        ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)
                    {
                        if let Some(bucket_name) = storage_controller.bucket_name.as_deref() {
                            let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                            let empty_config = NotificationConfiguration::default();
                            if let Err(e) = s3_client
                                .put_bucket_notification_configuration(bucket_name, &empty_config)
                                .await
                            {
                                warn!(
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear S3 notification configuration (best-effort)"
                                );
                            } else {
                                info!(
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    "S3 notification configuration cleared"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();
        }

        // Always continue to DeletingScheduleTriggers state (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingScheduleTriggers,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingScheduleTriggers,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Delete EventBridge rules and their targets (best-effort)
        if !self.eventbridge_rule_names.is_empty() {
            info!(
                worker=%worker_config.id,
                rules=?self.eventbridge_rule_names,
                "Deleting EventBridge schedule triggers"
            );

            let eventbridge_client = ctx
                .service_provider
                .get_aws_eventbridge_client(aws_cfg)
                .await?;

            for rule_name in &self.eventbridge_rule_names.clone() {
                // Remove targets first (required before deleting rule)
                if let Err(e) = eventbridge_client
                    .remove_targets(rule_name, vec!["1".to_string()])
                    .await
                {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to remove targets from EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule targets removed");
                }

                // Delete the rule
                if let Err(e) = eventbridge_client.delete_rule(rule_name).await {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to delete EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule deleted");
                }
            }
            self.eventbridge_rule_names.clear();
        }

        // Clear EventBridge permission statement IDs
        // (Lambda permissions are removed when the worker is deleted)
        self.eventbridge_permission_statement_ids.clear();

        // Detach the Lambda from the VPC before deleting it. AWS otherwise
        // keeps Lambda-managed ENIs around after function deletion, which can
        // block Terraform/CloudFormation from deleting customer-owned subnets.
        Ok(HandlerAction::Continue {
            state: DetachingVpcConfig,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DetachingVpcConfig,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detaching_vpc_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        if self.get_vpc_config(ctx)?.is_none() {
            return Ok(HandlerAction::Continue {
                state: DeletingWorker,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let request = UpdateFunctionConfigurationRequest::builder()
            .vpc_config(
                VpcConfig::builder()
                    .subnet_ids(Vec::new())
                    .security_group_ids(Vec::new())
                    .build(),
            )
            .build();

        // The update is granted, like the delete, only on functions carrying the stack's tags.
        let outcome = delete_tag_scoped(
            client
                .update_function_configuration(function_identifier, request)
                .await,
            || async {
                client
                    .get_function_configuration(function_identifier, None)
                    .await
                    .map(|_| ())
            },
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to detach Lambda worker from VPC".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;
        match outcome {
            TagScopedDelete::Deleted => {
                info!(worker=%worker_config.id, "Lambda VPC config detach requested");
                Ok(HandlerAction::Continue {
                    state: DetachVpcWaitForActive,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            TagScopedDelete::Gone => {
                info!(worker=%worker_config.id, "Lambda already gone while detaching VPC config");
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
        }
    }

    #[handler(
        state = DetachVpcWaitForActive,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detach_vpc_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        match client
            .get_function_configuration(function_identifier, None)
            .await
        {
            Ok(result)
                if result.state.as_deref() == Some("Active")
                    && result.last_update_status.as_deref() == Some("Successful") =>
            {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Ok(result)
                if result.state.as_deref() == Some("Pending")
                    || result.last_update_status.as_deref() == Some("InProgress") =>
            {
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            Ok(result) => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Lambda VPC detach failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(worker_config.id.clone()),
            })),
            // A function that is gone has no tags, so the read is denied rather than not found.
            Err(e) if is_remote_not_found(&e) || is_remote_access_denied(&e) => {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Lambda VPC detach status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = DeletingWorker,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_function(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        info!(name=%aws_worker_name, "Deleting worker itself: {}", aws_worker_name);

        let outcome = delete_tag_scoped(
            client.delete_function(&aws_worker_name, None).await,
            || async {
                client
                    .get_function_configuration(&aws_worker_name, None)
                    .await
                    .map(|_| ())
            },
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to delete Lambda worker".to_string(),
            resource_id: Some(worker_config.id.clone()),
        })?;
        info!(name=%aws_worker_name, outcome=?outcome, "Lambda function removed, waiting until it is gone");

        Ok(HandlerAction::Continue {
            state: DeleteWaitForNotFound,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteWaitForNotFound,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_wait_for_not_found(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let arn = self.arn.as_ref();
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let lookup_identifier = arn.map(|a| a.as_str()).unwrap_or(&aws_worker_name);

        match client
            .get_function_configuration(lookup_identifier, None)
            .await
        {
            // The delete above succeeded or confirmed the function gone, so this read only
            // waits for it to disappear. A deleted function has no tags: a role set up before
            // the read was granted on untagged functions gets AccessDenied, not NotFound, and
            // both mean it is gone.
            Err(e)
                if matches!(
                    e.error,
                    Some(
                        CloudClientErrorData::RemoteResourceNotFound { .. }
                            | CloudClientErrorData::RemoteAccessDenied { .. }
                    )
                ) =>
            {
                self.arn = None;
                self.url = None;
                self.worker_name = None;
                self.event_source_mappings.clear();
                if self.get_vpc_config(ctx)?.is_some() {
                    Ok(HandlerAction::Continue {
                        state: WaitingForVpcEnisReleased,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                } else {
                    Ok(HandlerAction::Continue {
                        state: DeletingCertificate,
                        suggested_delay: None,
                    })
                }
            }
            Ok(_) => Ok(HandlerAction::Stay {
                max_times: Some(10),
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check worker deletion status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForVpcEnisReleased,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_vpc_enis_released(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if self.get_vpc_config(ctx)?.is_none() {
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }
        let owns_network = Self::owns_network(ctx);
        let wait_started_at = *self.vpc_eni_wait_started_at.get_or_insert_with(Utc::now);

        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let network = ctx.require_dependency::<crate::network::AwsNetworkController>(
            &ResourceRef::new(Network::RESOURCE_TYPE, "default-network"),
        )?;
        let vpc_id = network.vpc_id.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: "default-network".to_string(),
            })
        })?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let result = client
            .describe_network_interfaces(
                DescribeNetworkInterfacesRequest::builder()
                    .filters(vec![
                        Filter {
                            name: "description".to_string(),
                            values: vec![format!("AWS Lambda VPC ENI-{}*", aws_worker_name)],
                        },
                        Filter {
                            name: "interface-type".to_string(),
                            // Lambda changes detached ENIs to the ordinary interface type
                            // before its execution role can finish deleting them.
                            values: vec!["lambda".to_string(), "interface".to_string()],
                        },
                        Filter {
                            name: "vpc-id".to_string(),
                            values: vec![vpc_id],
                        },
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to check Lambda VPC network interface cleanup".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Lambda can share an ENI named for another function; those interfaces are
        // inherently invisible to this function-name lookup. Do not filter by status:
        // detached/available interfaces still need the role for their final deletion.
        // Old leaked interfaces with the same name can therefore consume the full wait.
        let description_prefix = format!("AWS Lambda VPC ENI-{aws_worker_name}");
        let network_interfaces = result
            .network_interface_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|eni| {
                eni.description.as_deref().is_some_and(|description| {
                    lambda_eni_description_matches(description, &description_prefix)
                })
            })
            .collect::<Vec<_>>();

        if network_interfaces.is_empty() {
            self.vpc_eni_wait_started_at = None;
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        let network_interface_ids = network_interfaces
            .iter()
            .filter_map(|eni| eni.network_interface_id.as_deref())
            .collect::<Vec<_>>();

        if !owns_network
            && Utc::now()
                .signed_duration_since(wait_started_at)
                .num_seconds()
                >= EXTERNAL_VPC_ENI_WAIT_SECONDS
        {
            warn!(
                worker=%worker_config.id,
                network_interfaces=?network_interface_ids,
                wait_seconds=EXTERNAL_VPC_ENI_WAIT_SECONDS,
                "Lambda VPC interfaces remain after the cleanup wait; they may be shared with other functions. Continuing deletion in the externally managed network; unshared interfaces may require manual cleanup after the execution role is removed"
            );
            self.vpc_eni_wait_started_at = None;
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        info!(
            worker=%worker_config.id,
            network_interfaces=?network_interface_ids,
            "Waiting for Lambda VPC network interfaces to be released"
        );

        Ok(HandlerAction::Stay {
            // Keep the owned-network failure behavior. External networks use the persisted
            // deadline: reconcile frequency must not turn a 20-minute wait into seconds.
            max_times: if owns_network { Some(90) } else { None },
            suggested_delay: Some(Duration::from_secs(10)),
        })
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

        // Custom-domain certificates belong to the customer and may be shared.
        let mut certificate_arns: Vec<String> = self
            .certificate_arn
            .iter()
            .filter(|_| !self.uses_custom_domain)
            .cloned()
            .collect();
        if certificate_arns.is_empty() && self.certificate_import_token.is_none() {
            self.certificate_arn = None;
            return Ok(HandlerAction::Continue {
                state: DeletingLogGroup,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        // An import whose response was lost left a certificate that only its token finds.
        if let Some(token) = self.certificate_import_token.as_deref() {
            let found = certificates_imported_with_token(acm_client.as_ref(), token)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to look up the certificates this worker imported".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?;
            for certificate_arn in found.into_iter().flatten() {
                if !certificate_arns.contains(&certificate_arn) {
                    certificate_arns.push(certificate_arn);
                }
            }
        }
        for certificate_arn in &certificate_arns {
            let outcome = delete_imported_certificate(acm_client.as_ref(), certificate_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete ACM certificate '{certificate_arn}'"),
                    resource_id: Some(worker_config.id.clone()),
                })?;
            info!(worker=%worker_config.id, certificate_arn=%certificate_arn, outcome=?outcome, "ACM certificate removed");
            // A retry after a later certificate fails must not delete this one again. Token
            // lookups list only certificates that still exist.
            if self.certificate_arn.as_ref() == Some(certificate_arn) {
                self.certificate_arn = None;
            }
        }
        self.certificate_arn = None;
        self.certificate_import_token = None;

        Ok(HandlerAction::Continue {
            state: DeletingLogGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingLogGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_log_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        // The group `ensure_log_group` creates, or Lambda creates on first invoke, under the
        // function's own name.
        let log_group_name = format!("/aws/lambda/{aws_worker_name}");
        let aws_cfg = ctx.get_aws_config()?;
        let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
        match logs.delete_log_group(&log_group_name).await {
            Ok(()) => info!(log_group = %log_group_name, "Deleted worker log group"),
            Err(error) if is_remote_not_found(&error) => {
                info!(log_group = %log_group_name, "Worker log group already deleted")
            }
            // A role installed before logs:DeleteLogGroup was granted. The function and
            // everything else are gone; returning the denial would only end the delete the
            // same way, so it is reported and the delete finishes.
            Err(error) if is_remote_access_denied(&error) => warn!(
                log_group = %log_group_name,
                "Not allowed to delete the worker log group; it is left behind. Re-run setup to grant logs:DeleteLogGroup."
            ),
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete log group '{log_group_name}'"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        }

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    #[handler(
        state = RefreshFailed,
        on_failure = RefreshFailed,
        status = ResourceStatus::RefreshFailed,
    )]
    async fn refresh_failed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.ready(ctx).await
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.arn.as_ref().map(|arn| {
            // Map the load balancer endpoint for DNS management
            let load_balancer_endpoint = self
                .load_balancer
                .as_ref()
                .and_then(|lb| lb.endpoint.as_ref())
                .map(|endpoint| alien_core::LoadBalancerEndpoint {
                    dns_name: endpoint.dns_name.clone(),
                    hosted_zone_id: Some(endpoint.hosted_zone_id.clone()),
                });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .worker_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                identifier: Some(arn.clone()),
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
                commands_push_target: self.worker_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, LambdaWorkerBinding, WorkerBinding};

        if let (Some(worker_name), Some(arn)) = (&self.worker_name, &self.arn) {
            // Extract region from ARN: arn:aws:lambda:us-east-1:123456789:function:name
            let region = arn.split(':').nth(3).unwrap_or("us-east-1").to_string();

            let binding = WorkerBinding::Lambda(LambdaWorkerBinding {
                worker_name: BindingValue::Value(worker_name.clone()),
                region: BindingValue::Value(region),
                url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
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
impl AwsWorkerController {
    /// Creates the function's log group with a retention period before the
    /// function exists. Otherwise Lambda creates `/aws/lambda/<name>` on the
    /// first invocation and keeps its logs forever. A group that already
    /// exists (an earlier attempt, or a worker recreated under the same name)
    /// gets the retention period too.
    ///
    /// Deleting the worker keeps the group, as the CloudFormation template's
    /// `Retain` policy does: its logs outlive the function for debugging and
    /// expire on their own.
    ///
    /// The management role's permissions are fixed at setup, and a setup that
    /// predates these log permissions lacks them. Access denied therefore only
    /// warns: the worker is still created, and Lambda creates the group on
    /// first invoke without a retention period. Re-running setup grants them,
    /// and the worker's next update sets the retention period.
    ///
    /// To verify against AWS: deploy an AWS worker with the push model, run
    /// `aws logs describe-log-groups --log-group-name-prefix /aws/lambda/<prefix>-<worker>`
    /// and check `retentionInDays` is 30 before the first invocation.
    async fn ensure_log_group(
        ctx: &ResourceControllerContext<'_>,
        aws_cfg: &alien_aws_clients::AwsClientConfig,
        function_name: &str,
        resource_id: &str,
    ) -> Result<()> {
        let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
        let log_group_name = format!("/aws/lambda/{function_name}");

        match logs
            .create_log_group(
                CreateLogGroupRequest::builder()
                    .log_group_name(log_group_name.clone())
                    .build(),
            )
            .await
        {
            Ok(()) => info!(log_group = %log_group_name, "Created worker log group"),
            Err(error) if is_remote_resource_conflict(&error) => {
                info!(log_group = %log_group_name, "Worker log group already exists")
            }
            Err(error) if is_remote_access_denied(&error) => {
                warn!(
                    log_group = %log_group_name,
                    "Not allowed to create the worker log group; Lambda creates it on first invoke without a retention period. Re-run setup to grant logs:CreateLogGroup."
                );
                return Ok(());
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create log group '{log_group_name}'"),
                    resource_id: Some(resource_id.to_string()),
                }))
            }
        }

        match logs
            .put_retention_policy(
                PutRetentionPolicyRequest::builder()
                    .log_group_name(log_group_name.clone())
                    .retention_in_days(LAMBDA_LOG_RETENTION_DAYS)
                    .build(),
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if is_remote_access_denied(&error) => {
                warn!(
                    log_group = %log_group_name,
                    "Not allowed to set the worker log group's retention; its logs do not expire. Re-run setup to grant logs:PutRetentionPolicy."
                );
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to set the retention of log group '{log_group_name}'"),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }

    /// Rewrite an ECR image URI to use the given region if it points to a different one.
    ///
    /// Lambda requires container images in the same region as the worker.
    /// When the management account's ECR is in a different region and private
    /// image replication copies images to the target region, the image URI must
    /// reference the replicated copy.
    ///
    /// Only rewrites URIs matching the ECR format: `{account}.dkr.ecr.{region}.amazonaws.com/...`
    fn rewrite_ecr_region_if_needed(image_uri: &str, target_region: &str) -> String {
        // ECR URI format: {account_id}.dkr.ecr.{region}.amazonaws.com/{repo}:{tag}
        let Some(host_end) = image_uri.find('/') else {
            return image_uri.to_string();
        };
        let host = &image_uri[..host_end];
        let parts: Vec<&str> = host.split('.').collect();
        // parts: [account_id, "dkr", "ecr", region, "amazonaws", "com"]
        if parts.len() >= 6
            && parts[1] == "dkr"
            && parts[2] == "ecr"
            && parts[4] == "amazonaws"
            && parts[3] != target_region
        {
            let new_host = format!("{}.dkr.ecr.{}.amazonaws.com", parts[0], target_region);
            format!("{}{}", new_host, &image_uri[host_end..])
        } else {
            image_uri.to_string()
        }
    }

    /// Worker-only batch changes do not schedule a queue update. Check setup-owned
    /// queues before changing the worker, and increase Live visibility before enabling
    /// its mapping. Both writes are idempotent if a response or checkpoint is lost.
    async fn ensure_batch_queue_visibility(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        apply: bool,
    ) -> Result<()> {
        for trigger in &worker.triggers {
            let alien_core::WorkerTrigger::Queue {
                queue,
                batch: Some(batch),
            } = trigger
            else {
                continue;
            };
            let required = worker
                .timeout_seconds
                .saturating_mul(6)
                .saturating_add(batch.max_window_seconds)
                .clamp(30, 43_200);
            let controller =
                ctx.require_dependency::<crate::queue::aws::AwsQueueController>(queue)?;
            let url = controller.queue_url.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: queue.id.clone(),
                })
            })?;
            let client = ctx
                .service_provider
                .get_aws_sqs_client(ctx.get_aws_config()?)
                .await?;
            let attributes = client
                .get_queue_attributes(
                    url,
                    alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
                        .attribute_names(vec!["VisibilityTimeout".into()])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Cannot verify visibility for queue '{}' before batching",
                        queue.id
                    ),
                    resource_id: Some(worker.id.clone()),
                })?;
            let current = attributes
                .get_queue_attributes_result
                .attributes
                .iter()
                .find(|attribute| attribute.name == "VisibilityTimeout")
                .and_then(|attribute| attribute.value.parse::<u32>().ok())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Queue '{}' returned no valid visibility timeout",
                            queue.id
                        ),
                        resource_id: Some(worker.id.clone()),
                    })
                })?;
            if current >= required {
                continue;
            }
            let resource = ctx.desired_stack.resources.get(&queue.id).ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: queue.id.clone(),
                })
            })?;
            if resource.lifecycle != alien_core::ResourceLifecycle::Live {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Queue '{}' is setup-owned: rerun setup with VisibilityTimeout at least {required} seconds before enabling batching (current {current})", queue.id),
                    resource_id: Some(worker.id.clone()),
                }));
            }
            if apply {
                client
                    .set_queue_attributes(
                        url,
                        alien_aws_clients::sqs::SetQueueAttributesRequest::builder()
                            .attributes(std::collections::HashMap::from([(
                                "VisibilityTimeout".into(),
                                required.to_string(),
                            )]))
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Cannot increase queue '{}' visibility before enabling batching",
                            queue.id
                        ),
                        resource_id: Some(worker.id.clone()),
                    })?;
            }
        }
        Ok(())
    }

    /// Creates an SQS event source mapping for a queue trigger
    async fn create_queue_event_source_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        aws_cfg: &alien_aws_clients::AwsClientConfig,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<String> {
        self.ensure_batch_queue_visibility(ctx, worker_config, true)
            .await?;
        let batch = worker_config.queue_batch_config(&queue_ref.id);
        if let Some(batch) = batch {
            batch.validate().context(ErrorData::ResourceConfigInvalid {
                message: "Invalid queue batch settings".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;
        }
        let batch_size = batch.map_or(1, |batch| batch.max_messages as i32);
        let window = batch.map_or(0, |batch| batch.max_window_seconds as i32);
        let response_types = if batch.is_some() {
            vec!["ReportBatchItemFailures".to_string()]
        } else {
            Vec::new()
        };
        let scaling = alien_aws_clients::lambda::ScalingConfig {
            maximum_concurrency: worker_config
                .queue_source_maximum_concurrency()
                .map(|capacity| capacity as i32),
        };
        let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        // Get queue controller to access outputs
        let queue_controller =
            ctx.require_dependency::<crate::queue::aws::AwsQueueController>(queue_ref)?;
        let queue_outputs_wrapper = queue_controller.get_outputs().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let queue_outputs = queue_outputs_wrapper
            .downcast_ref::<alien_core::QueueOutputs>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Invalid queue outputs type".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Extract queue name from the queue URL
        let queue_name = if let Some(url) = &queue_outputs.identifier {
            // SQS URL format: https://sqs.region.amazonaws.com/account-id/queue-name
            url.split('/')
                .last()
                .unwrap_or(&queue_outputs.queue_name)
                .to_string()
        } else {
            queue_outputs.queue_name.clone()
        };

        // Construct SQS queue ARN: arn:aws:sqs:region:account-id:queue-name
        let queue_arn = format!(
            "arn:aws:sqs:{}:{}:{}",
            aws_cfg.region, aws_cfg.account_id, queue_name
        );

        info!(
            worker=%worker_config.id,
            queue_arn=%queue_arn,
            "Creating SQS event source mapping"
        );

        let worker_name = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for event source mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let existing_mappings = lambda_client
            .list_event_source_mappings(ListEventSourceMappingsRequest {
                event_source_arn: Some(queue_arn.clone()),
                function_name: Some(worker_name.clone()),
                marker: None,
                max_items: None,
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list event source mappings for queue '{}'",
                    queue_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if let Some(existing_mapping) = existing_mappings
            .event_source_mappings
            .unwrap_or_default()
            .into_iter()
            .find(|mapping| {
                mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                    && mapping.function_arn.as_deref() == Some(worker_name.as_str())
            })
        {
            if let Some(uuid) = existing_mapping.uuid {
                if !self.event_source_mappings.contains(&uuid) {
                    self.event_source_mappings.push(uuid.clone());
                }
                let was_batched = ctx
                    .state
                    .resources
                    .get(&worker_config.id)
                    .and_then(|state| state.previous_config.as_ref())
                    .and_then(|config| config.downcast_ref::<Worker>())
                    .is_some_and(|previous| previous.queue_batch_config(&queue_ref.id).is_some());
                if batch.is_none() && !was_batched {
                    return Ok(uuid);
                }
                lambda_client
                    .update_event_source_mapping(
                        &uuid,
                        alien_aws_clients::lambda::UpdateEventSourceMappingRequest::builder()
                            .batch_size(batch_size)
                            .maximum_batching_window_in_seconds(window)
                            .function_response_types(response_types)
                            .scaling_config(scaling)
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to reconcile queue event source mapping".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    })?;
                info!(
                    worker=%worker_config.id,
                    queue_arn=%queue_arn,
                    uuid=%uuid,
                    "SQS event source mapping already exists; treating as created"
                );
                return Ok(uuid);
            }
        }

        let request = alien_aws_clients::lambda::CreateEventSourceMappingRequest::builder()
            .event_source_arn(queue_arn.clone())
            .function_name(worker_name.clone())
            .batch_size(batch_size)
            .maybe_maximum_batching_window_in_seconds(batch.map(|_| window))
            .maybe_function_response_types(batch.map(|_| response_types))
            .maybe_scaling_config(batch.map(|_| scaling))
            .enabled(true)
            .build();

        let response = match lambda_client.create_event_source_mapping(request).await {
            Ok(response) => response,
            Err(e) if is_remote_resource_conflict(&e) => {
                let existing_mappings = lambda_client
                    .list_event_source_mappings(ListEventSourceMappingsRequest {
                        event_source_arn: Some(queue_arn.clone()),
                        function_name: Some(worker_name.clone()),
                        marker: None,
                        max_items: None,
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to list event source mappings for queue '{}' after conflict",
                            queue_name
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    })?;

                existing_mappings
                    .event_source_mappings
                    .unwrap_or_default()
                    .into_iter()
                    .find(|mapping| {
                        mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                            && mapping.function_arn.as_deref() == Some(worker_name.as_str())
                    })
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Event source mapping for queue '{}' already exists but could not be found",
                                queue_name
                            ),
                            resource_id: Some(worker_config.id.clone()),
                        })
                    })?
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create event source mapping for queue '{}'",
                        queue_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        };

        if let Some(uuid) = response.uuid {
            if !self.event_source_mappings.contains(&uuid) {
                self.event_source_mappings.push(uuid.clone());
            }
            info!(
                worker=%worker_config.id,
                queue_arn=%queue_arn,
                uuid=%uuid,
                "Successfully created SQS event source mapping"
            );
            return Ok(uuid);
        }

        Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "Queue event source mapping response has no UUID".to_string(),
            resource_id: Some(worker_config.id.clone()),
        }))
    }

    // ─────────────── HELPER METHODS ────────────────────────────

    /// Gets VPC configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the VPC ID, subnet IDs, and security group ID from the Network controller
    /// to configure the Lambda worker to run inside the VPC.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_config(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcConfig>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        // Deletion uses an empty desired stack; the recorded dependency still owns
        // the VPC configuration until this worker has finished deleting.
        if !ctx.state.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::AwsNetworkController>(&network_ref)?;

        // Only configure VPC if we have subnet IDs and a security group
        // For Lambda, we use private subnets (no public IP assignment)
        if network_state.private_subnet_ids.is_empty() {
            return Ok(None);
        }

        let security_group_ids = match &network_state.security_group_id {
            Some(sg) => vec![sg.clone()],
            None => vec![],
        };

        if security_group_ids.is_empty() {
            return Ok(None);
        }

        Ok(Some(
            VpcConfig::builder()
                .subnet_ids(network_state.private_subnet_ids.clone())
                .security_group_ids(security_group_ids)
                .build(),
        ))
    }

    /// The linked resources' binding variables as they would be rendered now, or `None` while
    /// any linked resource is not Running: a dependency in the middle of its own flow has not
    /// reached the value its dependents should see.
    async fn linked_binding_environment(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker_config: &Worker,
    ) -> Result<Option<HashMap<String, String>>> {
        Ok(linked_bindings::render_linked_bindings(
            ctx,
            &worker_config.runtime_binding_links(),
            &worker_config.id,
        )
        .await?
        .map(|rendered| rendered.variables))
    }

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        worker_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let mut runtime_links = links.to_vec();
        for link in ctx
            .desired_resource_config::<Worker>()?
            .runtime_binding_links()
        {
            if !runtime_links.contains(&link) {
                runtime_links.push(link);
            }
        }
        let env_vars = EnvironmentVariableBuilder::try_new(initial_env)?
            .add_worker_runtime_env_vars(ctx, &worker_config.id, worker_config.timeout_seconds)?
            .add_linked_resources(&runtime_links, ctx, worker_name_for_error_logging)
            .await?
            .add_self_worker_binding(&worker_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(worker_name: &str) -> Self {
        Self {
            state: AwsWorkerState::Ready,
            arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            )),
            url: Some(format!("https://abcd1234.lambda-url.us-east-1.on.aws/")),
            worker_name: Some(worker_name.to_string()),
            event_source_mappings: Vec::new(),
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id: None,
            integration_id: None,
            route_id: None,
            stage_name: None,
            api_mapping_id: None,
            rest_api_id: None,
            rest_root_resource_id: None,
            rest_proxy_resource_id: None,
            rest_deployment_id: None,
            rest_base_path: None,
            domain_name: None,
            domain_confirmed: false,
            domain_create_token: None,
            function_create_token: None,
            certificate_import_token: None,
            load_balancer: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            s3_permission_statement_ids: Vec::new(),
            eventbridge_rule_names: Vec::new(),
            eventbridge_permission_statement_ids: Vec::new(),
            delete_removed_something: false,
            vpc_eni_wait_started_at: None,
            _internal_stay_count: None,
        }
    }
}

/// Time is cut into slots of this many seconds, and each worker compares its linked bindings
/// with its live environment in one slot out of every [`LINKED_BINDING_CHECK_SLOTS`]. The
/// comparison costs a GetFunction call, against Lambda's 100 per second per Region, so it runs on
/// a fraction of heartbeats rather than on each one.
const LINKED_BINDING_CHECK_SLOT_SECONDS: i64 = 30;
/// Prime, so a worker reaches its slot within this many heartbeats whatever the heartbeat
/// interval (30 s, the manager's 60 s, 90 s, ...), unless that interval is a multiple of
/// `LINKED_BINDING_CHECK_SLOTS * LINKED_BINDING_CHECK_SLOT_SECONDS` (330 s).
const LINKED_BINDING_CHECK_SLOTS: i64 = 11;

/// Whether `now` falls in this worker's comparison slot. The slot is derived from the worker's
/// name and the clock, so the cadence stores nothing and writes no state per heartbeat, and
/// workers in one account spread their reads.
fn binding_check_due(worker_name: &str, now: chrono::DateTime<Utc>) -> bool {
    // Tests open the slot explicitly, so heartbeat tests about something else never depend on
    // the clock.
    #[cfg(test)]
    {
        let _ = (worker_name, now);
        tests::FORCED_BINDING_CHECK.with(|forced| forced.get())
    }
    #[cfg(not(test))]
    binding_window_open(worker_name, now)
}

fn binding_window_open(worker_name: &str, now: chrono::DateTime<Utc>) -> bool {
    let offset = worker_name.bytes().fold(0i64, |hash, byte| {
        (hash * 31 + i64::from(byte)) % LINKED_BINDING_CHECK_SLOTS
    });
    (now.timestamp() / LINKED_BINDING_CHECK_SLOT_SECONDS).rem_euclid(LINKED_BINDING_CHECK_SLOTS)
        == offset
}

#[cfg(test)]
mod tests {
    //! # AWS Worker Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.
    thread_local! {
        /// Forces the linked-binding comparison window open or closed in tests.
        pub(super) static FORCED_BINDING_CHECK: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use alien_aws_clients::acm::{
        CertificateSummary, ImportCertificateResponse, ListCertificatesResponse, MockAcmApi, Tag,
    };
    use alien_aws_clients::apigateway::MockApiGatewayApi;
    use alien_aws_clients::apigatewayv2::{
        Api, ApiMapping, DomainName, DomainNameConfiguration, Integration, MockApiGatewayV2Api,
        Route, Stage,
    };
    use alien_aws_clients::cloudwatch_logs::MockCloudWatchLogsApi;
    use alien_aws_clients::ec2::{DescribeNetworkInterfacesResponse, Ec2Client, MockEc2Api};
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::lambda::{
        AddPermissionResponse, EventSourceMapping, FunctionConfiguration, GetFunctionResponse,
        ListEventSourceMappingsResponse, MockLambdaApi,
    };
    use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _, AwsCredentialProvider};
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{
        AwsServiceOverrides, CertificateStatus, ClientConfig, DeploymentConfig, DnsRecordStatus,
        DomainMetadata, EnvironmentVariablesSnapshot, ExternalBindings, Network, NetworkSettings,
        Platform, PublicEndpointUrls, Resource, ResourceDefinition, ResourceDomainInfo,
        ResourceLifecycle, ResourceStatus, ServiceAccount, Stack, StackResourceState,
        StackSettings, StackState, Worker, WorkerCode, WorkerOutputs, WorkerPublicEndpoint,
    };
    use alien_error::AlienError;
    use httpmock::prelude::*;
    use rstest::rstest;

    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{
        serialize_controller, MockPlatformServiceProvider, ResourceController, StackExecutor,
    };
    use crate::network::AwsNetworkController;
    use crate::service_account::AwsServiceAccountController;
    use crate::worker::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock, AwsWorkerController,
        AwsWorkerState,
    };
    use alien_core::standard_resource_tags;

    #[tokio::test]
    async fn queue_rollout_updates_private_runtime_environment_on_batch_only_changes() {
        for enabled in [true, false] {
            let mut worker = function_with_queue_trigger();
            let mut previous = worker.clone();
            let batched = alien_core::WorkerTrigger::queue_batch(
                &test_queue(),
                alien_core::QueueBatchConfig {
                    max_messages: 8,
                    max_window_seconds: 5,
                },
            )
            .unwrap();
            if enabled {
                worker.triggers = vec![batched];
            } else {
                previous.triggers = vec![batched];
            }
            let mut lambda = MockLambdaApi::new();
            lambda
                .expect_update_function_configuration()
                .times(1)
                .returning(move |_, request| {
                    let variables = request
                        .environment
                        .as_ref()
                        .unwrap()
                        .variables
                        .as_ref()
                        .unwrap();
                    assert_eq!(variables.contains_key("ALIEN_QUEUE_BATCH_CONFIG"), enabled);
                    assert_eq!(variables.contains_key("ALIEN_TEST_QUEUE_BINDING"), enabled);
                    Ok(create_successful_function_response("test-queue-func"))
                });
            let mut sqs = alien_aws_clients::sqs::MockSqsApi::new();
            sqs.expect_get_queue_attributes()
                .times(if enabled { 1 } else { 0 })
                .returning(|_, _| {
                    Ok(alien_aws_clients::sqs::GetQueueAttributesResponse {
                        get_queue_attributes_result:
                            alien_aws_clients::sqs::GetQueueAttributesResult {
                                attributes: vec![alien_aws_clients::sqs::Attribute {
                                    name: "VisibilityTimeout".into(),
                                    value: "43200".into(),
                                }],
                            },
                    })
                });
            let sqs = Arc::new(sqs);
            let mut provider = setup_mock_service_provider(Arc::new(lambda), None, None);
            Arc::get_mut(&mut provider)
                .unwrap()
                .expect_get_aws_sqs_client()
                .returning(move |_| Ok(sqs.clone()));
            let controller = AwsWorkerController {
                state: AwsWorkerState::UpdateConfigStart,
                ..AwsWorkerController::mock_ready("test-queue-func")
            };
            let restored = serde_json::from_value::<AwsWorkerController>(
                serde_json::to_value(controller).unwrap(),
            )
            .unwrap();
            let mut executor = SingleControllerExecutor::builder()
                .resource(worker)
                .previous_resource(previous)
                .controller(restored)
                .platform(Platform::Aws)
                .service_provider(provider)
                .with_test_dependencies()
                .with_dependency(
                    test_queue(),
                    crate::queue::aws::AwsQueueController {
                        state: crate::queue::aws::AwsQueueState::Ready,
                        queue_name: Some("physical-jobs".into()),
                        queue_url: Some(
                            "https://sqs.us-east-1.amazonaws.com/123456789012/physical-jobs".into(),
                        ),
                        ..Default::default()
                    },
                )
                .build()
                .await
                .unwrap();
            executor
                .step()
                .await
                .expect("batch-only change must update private worker environment");
            assert_eq!(
                executor
                    .internal_state::<AwsWorkerController>()
                    .unwrap()
                    .state,
                AwsWorkerState::UpdateConfigWaitForActive
            );
        }
    }

    #[tokio::test]
    async fn batch_enable_checks_setup_owned_queue_before_worker_mutation() {
        for setup_applied in [false, true] {
            let mut worker = function_with_queue_trigger();
            let previous = worker.clone();
            worker.triggers = vec![alien_core::WorkerTrigger::queue_batch(
                &test_queue(),
                alien_core::QueueBatchConfig {
                    max_messages: 8,
                    max_window_seconds: 5,
                },
            )
            .unwrap()];
            worker.code = alien_core::WorkerCode::Image {
                image: "registry.example.com/worker:new".into(),
            };
            let required = worker.timeout_seconds * 6 + 5;
            let mut sqs = alien_aws_clients::sqs::MockSqsApi::new();
            sqs.expect_get_queue_attributes()
                .times(1)
                .returning(move |_, _| {
                    Ok(alien_aws_clients::sqs::GetQueueAttributesResponse {
                        get_queue_attributes_result:
                            alien_aws_clients::sqs::GetQueueAttributesResult {
                                attributes: vec![alien_aws_clients::sqs::Attribute {
                                    name: "VisibilityTimeout".into(),
                                    value: (if setup_applied {
                                        required + 30
                                    } else {
                                        required - 5
                                    })
                                    .to_string(),
                                }],
                            },
                    })
                });
            // Frozen queues are never written; sufficient installed visibility permits only the Worker update.
            let mut lambda = MockLambdaApi::new();
            lambda
                .expect_update_function_code()
                .times(usize::from(setup_applied))
                .returning(|_, request| {
                    assert_eq!(request.image_uri, "registry.example.com/worker:new");
                    Ok(create_successful_function_response("test-queue-func"))
                });
            let sqs = Arc::new(sqs);
            let mut provider = setup_mock_service_provider(Arc::new(lambda), None, None);
            Arc::get_mut(&mut provider)
                .unwrap()
                .expect_get_aws_sqs_client()
                .returning(move |_| Ok(sqs.clone()));
            let controller = AwsWorkerController {
                state: AwsWorkerState::UpdateCodeStart,
                ..AwsWorkerController::mock_ready("test-queue-func")
            };
            let restored = serde_json::from_value::<AwsWorkerController>(
                serde_json::to_value(controller).unwrap(),
            )
            .unwrap();
            let mut executor = SingleControllerExecutor::builder()
                .resource(worker)
                .previous_resource(previous)
                .controller(restored)
                .platform(Platform::Aws)
                .service_provider(provider)
                .with_test_dependencies()
                .with_dependency(
                    test_queue(),
                    crate::queue::aws::AwsQueueController {
                        state: crate::queue::aws::AwsQueueState::Ready,
                        queue_name: Some("physical-jobs".into()),
                        queue_url: Some(
                            "https://sqs.us-east-1.amazonaws.com/123456789012/physical-jobs".into(),
                        ),
                        ..Default::default()
                    },
                )
                .dependency_lifecycle("test-queue", alien_core::ResourceLifecycle::Frozen)
                .build()
                .await
                .unwrap();
            if setup_applied {
                executor
                    .step()
                    .await
                    .expect("setup-updated Frozen queue permits code update");
                assert_eq!(
                    executor
                        .internal_state::<AwsWorkerController>()
                        .unwrap()
                        .state,
                    AwsWorkerState::UpdateCodeWaitForActive
                );
            } else {
                let error = executor
                    .step()
                    .await
                    .expect_err("setup must increase visibility first");
                assert!(error.to_string().contains("setup-owned"), "{error}");
                assert!(error.to_string().contains(&required.to_string()), "{error}");
            }
        }
    }

    #[tokio::test]
    async fn queue_mapping_updates_preserve_identity_and_reset_removed_settings() {
        for mode in 0..4 {
            let mut worker = function_with_queue_trigger();
            worker.concurrency_limit = Some(if mode == 1 { 1 } else { 8 });
            let mut previous = worker.clone();
            let batch = alien_core::QueueBatchConfig {
                max_messages: 10,
                max_window_seconds: 5,
            };
            if let alien_core::WorkerTrigger::Queue { batch: target, .. } = &mut worker.triggers[0]
            {
                *target = (mode != 1 && mode != 3).then_some(batch.clone());
            }
            if let alien_core::WorkerTrigger::Queue { batch: target, .. } =
                &mut previous.triggers[0]
            {
                *target = (mode != 0 && mode != 3).then_some(batch);
            }
            if mode == 2 {
                previous.concurrency_limit = Some(4);
            }
            let mapping = || {
                serde_json::from_value::<EventSourceMapping>(serde_json::json!({
                    "UUID": "retained-id",
                    "EventSourceArn": "arn:aws:sqs:us-east-1:123456789012:imported-a7d9-jobs",
                    "FunctionArn": "arn:aws:lambda:us-east-1:123456789012:function:test-queue-func",
                    "State": "Enabled",
                }))
                .expect("mapping")
            };
            let mut lambda = MockLambdaApi::new();
            lambda
                .expect_list_event_source_mappings()
                .times(if mode == 3 { 0 } else { 1 })
                .withf(|request| {
                    request.event_source_arn.as_deref()
                        == Some("arn:aws:sqs:us-east-1:123456789012:imported-a7d9-jobs")
                })
                .returning(move |_| {
                    Ok(ListEventSourceMappingsResponse {
                        event_source_mappings: Some(vec![mapping()]),
                        next_marker: None,
                    })
                });
            lambda
                .expect_update_event_source_mapping()
                .times(if mode == 3 { 0 } else { 1 })
                .withf(move |id, request| {
                    id == "retained-id"
                        && request.batch_size == Some(if mode == 1 { 1 } else { 10 })
                        && request.maximum_batching_window_in_seconds
                            == Some(if mode == 1 { 0 } else { 5 })
                        && request
                            .function_response_types
                            .as_ref()
                            .is_some_and(|types| {
                                if mode == 1 {
                                    types.is_empty()
                                } else {
                                    types == &["ReportBatchItemFailures"]
                                }
                            })
                        && request.scaling_config.as_ref().is_some_and(|scaling| {
                            scaling.maximum_concurrency == if mode == 1 { None } else { Some(8) }
                        })
                })
                .returning(move |_, _| Ok(mapping()));
            lambda
                .expect_delete_event_source_mapping()
                .times(if mode == 3 { 0 } else { 1 })
                .withf(|id| id == "removed-id")
                .returning(move |_| Ok(mapping()));
            // No create expectation: retained subscriptions must never be replaced.
            let required = worker.timeout_seconds * 6 + 5;
            let visibility = Arc::new(std::sync::atomic::AtomicU32::new(required - 5));
            let observed_visibility = visibility.clone();
            let mut sqs = alien_aws_clients::sqs::MockSqsApi::new();
            sqs.expect_get_queue_attributes()
                .times(if mode == 0 {
                    2
                } else if mode == 2 {
                    1
                } else {
                    0
                })
                .returning(move |_, _| {
                    Ok(alien_aws_clients::sqs::GetQueueAttributesResponse {
                        get_queue_attributes_result:
                            alien_aws_clients::sqs::GetQueueAttributesResult {
                                attributes: vec![alien_aws_clients::sqs::Attribute {
                                    name: "VisibilityTimeout".into(),
                                    value: observed_visibility
                                        .load(std::sync::atomic::Ordering::SeqCst)
                                        .to_string(),
                                }],
                            },
                    })
                });
            sqs.expect_set_queue_attributes()
                .times(if mode == 0 || mode == 2 { 1 } else { 0 })
                .withf(move |_, request| {
                    request.attributes.get("VisibilityTimeout") == Some(&required.to_string())
                })
                .returning(move |_, _| {
                    visibility.store(required, std::sync::atomic::Ordering::SeqCst);
                    if mode == 0 {
                        Err(AlienError::new(CloudClientErrorData::InvalidInput {
                            message: "response lost after visibility was increased".into(),
                            field_name: None,
                        }))
                    } else {
                        Ok(())
                    }
                });
            let sqs = Arc::new(sqs);
            let mut provider = setup_mock_service_provider(Arc::new(lambda), None, None);
            Arc::get_mut(&mut provider)
                .unwrap()
                .expect_get_aws_sqs_client()
                .returning(move |_| Ok(sqs.clone()));
            let mut controller = AwsWorkerController::mock_ready("test-queue-func");
            controller.state = AwsWorkerState::UpdateEventSourceMappings;
            controller.event_source_mappings = vec!["retained-id".into(), "removed-id".into()];
            let restored = crate::core::deserialize_controller(
                serialize_controller(&controller).expect("checkpoint"),
            )
            .expect("restore");
            let queue = crate::queue::aws::AwsQueueController {
                state: crate::queue::aws::AwsQueueState::Ready,
                queue_name: Some("imported-a7d9-jobs".into()),
                queue_url: Some(
                    "https://sqs.us-east-1.amazonaws.com/123456789012/imported-a7d9-jobs".into(),
                ),
                ..Default::default()
            };
            let build = |controller: AwsWorkerController| {
                SingleControllerExecutor::builder()
                    .resource(worker.clone())
                    .previous_resource(previous.clone())
                    .controller(controller)
                    .platform(Platform::Aws)
                    .service_provider(provider.clone())
                    .with_test_dependencies()
                    .with_dependency(test_queue(), queue.clone())
                    .build()
            };
            let mut executor = build(
                restored
                    .as_any()
                    .downcast_ref::<AwsWorkerController>()
                    .unwrap()
                    .clone(),
            )
            .await
            .expect("executor");
            if mode == 0 {
                executor
                    .step()
                    .await
                    .expect_err("visibility response was lost before mapping mutation");
                let saved = executor.internal_state::<AwsWorkerController>().unwrap();
                assert_eq!(
                    saved.event_source_mappings,
                    vec!["retained-id", "removed-id"]
                );
                let mut restored: AwsWorkerController =
                    serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
                restored.state = AwsWorkerState::UpdateEventSourceMappings;
                executor = build(restored).await.expect("restore retry checkpoint");
            }
            executor
                .step()
                .await
                .expect("reconcile retained and removed sources");
            assert_eq!(
                executor
                    .internal_state::<AwsWorkerController>()
                    .expect("controller")
                    .event_source_mappings,
                if mode == 3 {
                    vec!["retained-id", "removed-id"]
                } else {
                    vec!["retained-id"]
                }
            );
        }
    }

    fn create_successful_function_response(worker_name: &str) -> FunctionConfiguration {
        FunctionConfiguration {
            function_name: Some(worker_name.to_string()),
            function_arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            )),
            state: Some("Active".to_string()),
            last_update_status: Some("Successful".to_string()),
            kms_key_arn: None,
        }
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
                endpoints: HashMap::new(),
                aliases: Vec::new(),
                issued_at: Some("2024-01-01T00:00:00Z".to_string()),
            },
        );
        DomainMetadata {
            base_domain: "test.example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "Z1234567890ABC".to_string(),
            resources,
        }
    }

    fn create_acm_mock_for_creation() -> Arc<MockAcmApi> {
        let mut mock_acm = MockAcmApi::new();
        mock_acm
            .expect_list_certificates()
            .returning(|_| Ok(ListCertificatesResponse::default()));
        mock_acm.expect_import_certificate().returning(|_| {
            Ok(ImportCertificateResponse {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
            })
        });
        Arc::new(mock_acm)
    }

    fn create_acm_mock_for_creation_and_deletion() -> Arc<MockAcmApi> {
        let mut mock_acm = MockAcmApi::new();
        mock_acm
            .expect_list_certificates()
            .returning(|_| Ok(ListCertificatesResponse::default()));
        mock_acm.expect_import_certificate().returning(|_| {
            Ok(ImportCertificateResponse {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
            })
        });
        mock_acm.expect_delete_certificate().returning(|_| Ok(()));
        Arc::new(mock_acm)
    }

    fn create_apigatewayv2_mock_for_creation() -> Arc<MockApiGatewayV2Api> {
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw.expect_create_api().returning(|_| {
            Ok(Api {
                api_id: Some("test-api-id".to_string()),
                api_endpoint: Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                name: None,
                protocol_type: None,
            })
        });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
                tags: None,
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        Arc::new(mock_apigw)
    }

    fn create_apigatewayv2_mock_for_creation_and_deletion() -> Arc<MockApiGatewayV2Api> {
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw.expect_create_api().returning(|_| {
            Ok(Api {
                api_id: Some("test-api-id".to_string()),
                api_endpoint: Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                name: None,
                protocol_type: None,
            })
        });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
                tags: None,
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        mock_apigw
            .expect_delete_api_mapping()
            .returning(|_, _| Ok(()));
        mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
        mock_apigw.expect_delete_api().returning(|_| Ok(()));
        Arc::new(mock_apigw)
    }

    fn setup_mock_client_for_creation_and_update(
        worker_name: &str,
        has_url: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock successful worker creation
        let worker_name = worker_name.to_string();
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        // Mock worker status checks - first pending, then active
        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)));

        // Mock API Gateway permission and self-binding env var update if public ingress
        if has_url {
            mock_lambda
                .expect_add_permission()
                .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

            let worker_name_for_self_binding = worker_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &worker_name_for_self_binding,
                    ))
                });
        }

        // Mock concurrency operations (may or may not be called depending on worker config)
        mock_lambda
            .expect_put_function_concurrency()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_delete_function_concurrency()
            .returning(|_| Ok(()));

        // Mock successful updates
        let worker_name_for_code_update = worker_name.clone();
        mock_lambda
            .expect_update_function_code()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &worker_name_for_code_update,
                ))
            });

        if !has_url {
            let worker_name_for_config_update = worker_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &worker_name_for_config_update,
                    ))
                });
        }

        Arc::new(mock_lambda)
    }

    fn setup_mock_client_for_creation_and_deletion(
        worker_name: &str,
        has_url: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock successful worker creation
        let worker_name = worker_name.to_string();
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        // Mock worker status checks
        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
            .times(1); // Only for creation flow

        // Mock API Gateway permission and self-binding env var update if public ingress
        if has_url {
            mock_lambda
                .expect_add_permission()
                .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

            // Mock update_function_configuration for self-binding env var update
            let worker_name_for_config_update = worker_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &worker_name_for_config_update,
                    ))
                });
        }

        // Mock concurrency operations (may or may not be called depending on worker config)
        mock_lambda
            .expect_put_function_concurrency()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_delete_function_concurrency()
            .returning(|_| Ok(()));

        // Mock successful worker deletion
        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));

        // Mock worker not found during deletion check
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        Arc::new(mock_lambda)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _worker_name: &str,
        function_missing: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock worker deletion (might fail if worker missing)
        if function_missing {
            mock_lambda.expect_delete_function().returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });
        } else {
            mock_lambda
                .expect_delete_function()
                .returning(|_, _| Ok(()));
        }

        // Always return not found for final status check
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        Arc::new(mock_lambda)
    }

    fn create_aws_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));
        Arc::new(mock_iam)
    }

    /// CloudWatch Logs mock that accepts the log group a create makes.
    fn create_logs_mock_accepting_log_groups() -> Arc<MockCloudWatchLogsApi> {
        let mut mock_logs = MockCloudWatchLogsApi::new();
        mock_logs.expect_create_log_group().returning(|_| Ok(()));
        mock_logs
            .expect_put_retention_policy()
            .returning(|_| Ok(()));
        mock_logs.expect_delete_log_group().returning(|_| Ok(()));
        Arc::new(mock_logs)
    }

    fn setup_mock_service_provider(
        mock_lambda: Arc<MockLambdaApi>,
        mock_acm: Option<Arc<MockAcmApi>>,
        mock_apigw: Option<Arc<MockApiGatewayV2Api>>,
    ) -> Arc<MockPlatformServiceProvider> {
        setup_mock_service_provider_with_logs(
            mock_lambda,
            mock_acm,
            mock_apigw,
            create_logs_mock_accepting_log_groups(),
        )
    }

    fn setup_mock_service_provider_with_logs(
        mock_lambda: Arc<MockLambdaApi>,
        mock_acm: Option<Arc<MockAcmApi>>,
        mock_apigw: Option<Arc<MockApiGatewayV2Api>>,
        mock_logs: Arc<MockCloudWatchLogsApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(mock_lambda.clone()));
        mock_provider
            .expect_get_aws_logs_client()
            .returning(move |_| Ok(mock_logs.clone()));

        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_network_interfaces().returning(|_| {
            Ok(DescribeNetworkInterfacesResponse {
                network_interface_set: None,
                next_token: None,
            })
        });
        let ec2 = Arc::new(ec2);
        mock_provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));

        // Mock IAM client for resource-scoped permissions (ApplyingResourcePermissions state)
        let mock_iam = create_aws_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        if let Some(acm) = mock_acm {
            mock_provider
                .expect_get_aws_acm_client()
                .returning(move |_| Ok(acm.clone()));
        }

        if let Some(apigw) = mock_apigw {
            mock_provider
                .expect_get_aws_apigatewayv2_client()
                .returning(move |_| Ok(apigw.clone()));
        }

        Arc::new(mock_provider)
    }

    /// Sets up all mocks for a worker test, including Lambda, ACM, and API Gateway.
    ///
    /// Returns `(mock_provider, optional_mock_server, optional_domain_metadata, optional_public_urls)`.
    /// For public workers, `domain_metadata` and `public_urls` must be set on the executor builder.
    /// When a readiness probe is configured, `public_urls` overrides the FQDN URL so the probe
    /// hits the local mock HTTP server instead.
    fn setup_mocks_for_function(
        worker: &Worker,
        worker_name: &str,
        for_deletion: bool,
    ) -> (
        Arc<MockPlatformServiceProvider>,
        Option<MockServer>,
        Option<DomainMetadata>,
        Option<PublicEndpointUrls>,
    ) {
        let has_url = !worker.public_endpoints.is_empty();
        let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(worker))
        } else {
            None
        };

        // Set up Lambda client mock (same for both flows; URL config calls are removed)
        let lambda_mock = if for_deletion {
            setup_mock_client_for_creation_and_deletion(worker_name, has_url)
        } else {
            setup_mock_client_for_creation_and_update(worker_name, has_url)
        };

        // Set up ACM and API Gateway mocks for public workers
        let (acm_mock, apigw_mock, domain_metadata, public_endpoints) = if has_url {
            let dm = create_test_domain_metadata(&worker.id);
            let acm = if for_deletion {
                create_acm_mock_for_creation_and_deletion()
            } else {
                create_acm_mock_for_creation()
            };
            let apigw = if for_deletion {
                create_apigatewayv2_mock_for_creation_and_deletion()
            } else {
                create_apigatewayv2_mock_for_creation()
            };
            // For readiness probe tests, override the FQDN URL with the mock server URL
            let pub_endpoints = mock_server.as_ref().map(|server| {
                HashMap::from([(
                    worker.id.clone(),
                    HashMap::from([("api".to_string(), server.base_url())]),
                )])
            });
            (Some(acm), Some(apigw), Some(dm), pub_endpoints)
        } else {
            (None, None, None, None)
        };

        let mock_provider = setup_mock_service_provider(lambda_mock, acm_mock, apigw_mock);

        (
            mock_provider,
            mock_server,
            domain_metadata,
            public_endpoints,
        )
    }

    /// A worker with two public endpoints reports its first declared endpoint's URL in
    /// every run, whatever order the deployment config's endpoint URL map iterates in.
    #[tokio::test]
    async fn worker_reports_first_declared_endpoint_url_in_every_run() {
        for _ in 0..16 {
            let mut worker = function_public_ingress();
            worker.public_endpoints.push(WorkerPublicEndpoint {
                name: "admin".to_string(),
                host_label: Some("admin".to_string()),
                wildcard_subdomains: false,
            });
            let worker_name = format!("test-{}", worker.id);
            let (mock_provider, _mock_server, domain_metadata, _) =
                setup_mocks_for_function(&worker, &worker_name, false);
            // A fresh map per run gets its own hash seed, as a newly loaded deployment
            // config does, so its iteration order varies between runs.
            let urls = HashMap::from([(
                worker.id.clone(),
                HashMap::from([
                    ("api".to_string(), "https://api.example.test".to_string()),
                    (
                        "admin".to_string(),
                        "https://admin.example.test".to_string(),
                    ),
                ]),
            )]);
            let mut executor = SingleControllerExecutor::builder()
                .resource(worker)
                .controller(AwsWorkerController::default())
                .platform(Platform::Aws)
                .service_provider(mock_provider)
                .domain_metadata(domain_metadata.expect("public worker has domain metadata"))
                .public_endpoints(urls)
                .with_test_dependencies()
                .build()
                .await
                .expect("executor builds");

            executor
                .run_until_terminal()
                .await
                .expect("create flow completes");
            assert_eq!(executor.status(), ResourceStatus::Running);
            let outputs = executor.outputs().expect("worker has outputs");
            let outputs = outputs
                .downcast_ref::<WorkerOutputs>()
                .expect("worker outputs");
            assert_eq!(
                outputs.public_endpoints["default"].url,
                "https://api.example.test"
            );
        }
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::env_vars(function_with_env_vars(), false)]
    #[case::storage_link(function_with_storage_link(), false)]
    #[case::env_and_storage(function_with_env_and_storage(), false)]
    #[case::multiple_storages(function_with_multiple_storages(), false)]
    #[case::public_ingress(function_public_ingress(), true)]
    #[case::private_ingress(function_private_ingress(), false)]
    #[case::concurrency(function_with_concurrency(), false)]
    #[case::custom_config(function_custom_config(), false)]
    #[case::readiness_probe(function_with_readiness_probe(), true)]
    #[case::complete_test(function_complete_test(), true)]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker, #[case] _has_url: bool) {
        let worker_name = format!("test-{}", worker.id);
        let (mock_provider, _mock_server, domain_metadata, public_endpoints) =
            setup_mocks_for_function(&worker, &worker_name, true);

        let mut builder = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_endpoints {
            builder = builder.public_endpoints(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let delays = executor.take_suggested_delays();
        // Lambda creation is asynchronous, so create polls until the function is active.
        assert!(
            !delays.is_empty(),
            "AWS worker create should poll at least once"
        );
        crate::core::controller_test::assert_polling_delays(
            &delays,
            std::time::Duration::from_secs(1),
            "AWS worker create",
        );

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
        // Lambda deletion completes in the call, so delete may not poll at all.
        crate::core::controller_test::assert_polling_delays(
            &executor.take_suggested_delays(),
            std::time::Duration::from_secs(1),
            "AWS worker delete",
        );

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    /// A worker created while the management role could not set its log
    /// group's retention gets it on the next update.
    #[tokio::test]
    async fn test_update_sets_log_retention_on_the_existing_group() {
        let worker_id = "test-update-worker".to_string();
        let mut from_function = basic_function();
        from_function.id = worker_id.clone();
        let mut to_function = function_with_env_vars();
        to_function.id = worker_id.clone();
        let worker_name = format!("test-{worker_id}");
        let calls = Arc::new(Mutex::new(Vec::new()));

        let mut mock_logs = MockCloudWatchLogsApi::new();
        let create_calls = calls.clone();
        mock_logs
            .expect_create_log_group()
            .times(1)
            .returning(move |request| {
                create_calls
                    .lock()
                    .unwrap()
                    .push(format!("CreateLogGroup {}", request.log_group_name));
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "CloudWatch Logs log group".to_string(),
                        resource_name: request.log_group_name,
                        message: "The specified log group already exists".to_string(),
                    },
                ))
            });
        let retention_calls = calls.clone();
        mock_logs
            .expect_put_retention_policy()
            .times(1)
            .returning(move |request| {
                retention_calls.lock().unwrap().push(format!(
                    "PutRetentionPolicy {} {}",
                    request.log_group_name, request.retention_in_days
                ));
                Ok(())
            });
        let mock_provider = setup_mock_service_provider_with_logs(
            setup_mock_client_for_creation_and_update(&worker_name, false),
            None,
            None,
            Arc::new(mock_logs),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(AwsWorkerController::mock_ready(&worker_name))
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.update(to_function).unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                format!("CreateLogGroup /aws/lambda/{worker_name}"),
                format!("PutRetentionPolicy /aws/lambda/{worker_name} 30"),
            ]
        );
    }

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

        let worker_name = format!("test-{}", worker_id);
        let (mock_provider, mock_server, domain_metadata, public_endpoints) =
            setup_mocks_for_function(&to_function, &worker_name, false);

        // Start with the "from" worker in Ready state
        let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);

        // If the target worker has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        }

        let mut builder = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_endpoints {
            builder = builder.public_endpoints(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new worker
        let target_is_public = !to_function.public_endpoints.is_empty();
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        if target_is_public {
            let url = function_outputs
                .public_endpoints
                .get("default")
                .expect("default public endpoint")
                .url
                .as_str();
            assert!(url.starts_with("http://") || url.starts_with("https://"));
            assert!(!url.contains("lambda-url"));
        }
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_function(function_public_ingress(), true)]
    #[case::public(function_public_ingress(), false)]
    #[case::private_with_missing_function(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] worker: Worker,
        #[case] function_missing: bool,
    ) {
        let worker_name = format!("test-{}", worker.id);
        let has_url = !worker.public_endpoints.is_empty();
        let mock_lambda =
            setup_mock_client_for_best_effort_deletion(&worker_name, function_missing);
        let mock_provider = setup_mock_service_provider(mock_lambda, None, None);

        // Start with a ready controller
        let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);
        if has_url {
            ready_controller.url =
                Some("https://example.execute-api.us-east-1.amazonaws.com".to_string());
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(ready_controller)
            .platform(Platform::Aws)
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

    fn add_recorded_test_network(state: &mut StackState) {
        let mut network = AwsNetworkController::mock_ready("vpc-test", 2);
        network.transition_to_delete_start().unwrap();
        state.resources.insert(
            "default-network".into(),
            StackResourceState::builder()
                .resource_type(Network::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Deleting)
                .config(Resource::new(
                    Network::new("default-network".into())
                        .settings(NetworkSettings::UseDefault)
                        .build(),
                ))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .internal_state(serialize_controller(&network).unwrap())
                .build(),
        );
    }

    fn existing_vpc_settings() -> StackSettings {
        StackSettings {
            network: Some(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-test".into(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec!["subnet-test".into()],
                security_group_ids: vec!["sg-test".into()],
            }),
            ..Default::default()
        }
    }

    #[rstest]
    #[case::bare("", true)]
    #[case::uuid("-12345678-1234-1234-1234-123456789abc", true)]
    #[case::sibling("-v2", false)]
    #[case::sibling_uuid("-v2-12345678-1234-1234-1234-123456789abc", false)]
    #[case::invalid_uuid("-12345678-1234-1234-1234-123456789abz", false)]
    #[case::trailing_text("-12345678-1234-1234-1234-123456789abc-extra", false)]
    #[tokio::test]
    async fn eni_wait_matches_only_the_exact_function(
        #[case] suffix: &'static str,
        #[case] should_wait: bool,
    ) {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_network_interfaces()
            .times(1)
            .withf(|request| {
                let filters = request.filters.as_ref().unwrap();
                filters.len() == 3
                    && filters
                        .iter()
                        .any(|f| f.name == "interface-type" && f.values == ["lambda", "interface"])
                    && [
                        ("description", "AWS Lambda VPC ENI-test-basic-func*"),
                        ("vpc-id", "vpc-test"),
                    ]
                    .iter()
                    .all(|(name, value)| {
                        filters
                            .iter()
                            .any(|f| f.name == *name && f.values == [*value])
                    })
            })
            .returning(move |_| {
                Ok(serde_json::from_value(serde_json::json!({
                    "networkInterfaceSet": { "item": [{
                        "networkInterfaceId": "eni-test",
                        "description": format!("AWS Lambda VPC ENI-test-basic-func{suffix}"),
                        "status": "available"
                    }] }
                }))
                .unwrap())
            });
        let ec2 = Arc::new(ec2);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(AwsWorkerController {
                state: AwsWorkerState::WaitingForVpcEnisReleased,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .stack_settings(existing_vpc_settings())
            .with_dependency(
                Network::new("default-network".into())
                    .settings(NetworkSettings::UseDefault)
                    .build(),
                AwsNetworkController::mock_ready("vpc-test", 2),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            if should_wait {
                AwsWorkerState::WaitingForVpcEnisReleased
            } else {
                AwsWorkerState::DeletingCertificate
            }
        );
    }

    #[rstest]
    #[case::no_network(false)]
    #[case::default_network_without_vpc_attachment(true)]
    #[tokio::test]
    async fn eni_wait_skips_workers_without_vpc_configuration(#[case] default_network: bool) {
        // No provider expectations: neither case should call EC2.
        let mut builder = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(AwsWorkerController {
                state: AwsWorkerState::WaitingForVpcEnisReleased,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()));
        if default_network {
            builder = builder
                .stack_settings(StackSettings {
                    network: Some(NetworkSettings::UseDefault),
                    ..Default::default()
                })
                .with_dependency(
                    Network::new("default-network".into())
                        .settings(NetworkSettings::UseDefault)
                        .build(),
                    AwsNetworkController::mock_byo_vpc_ready("vpc-test", vec![], vec![], None),
                );
        }
        let mut executor = builder.build().await.unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            AwsWorkerState::DeletingCertificate
        );
    }

    #[rstest]
    #[case::existing_wait(false, 1190, true)]
    #[case::existing_timeout(false, 1200, true)]
    #[case::existing_released(false, 0, false)]
    #[case::created_wait(true, 1200, true)]
    #[case::created_released(true, 0, false)]
    #[tokio::test]
    async fn eni_wait_respects_network_ownership_and_wall_clock(
        #[case] owns_network: bool,
        #[case] elapsed_seconds: i64,
        #[case] interfaces_remain: bool,
    ) {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_network_interfaces().times(1)
            .withf(|request| request.filters.as_ref().unwrap()[0].values
                == ["AWS Lambda VPC ENI-test-basic-func*"])
            .returning(move |_| Ok(serde_json::from_value(serde_json::json!({
                "networkInterfaceSet": { "item": if interfaces_remain {
                    vec![serde_json::json!({"networkInterfaceId": "eni-shared", "description": "AWS Lambda VPC ENI-test-basic-func", "status": "available"})]
                } else { vec![] } }
            })).unwrap()));
        let ec2 = Arc::new(ec2);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        let started = chrono::Utc::now() - chrono::Duration::seconds(elapsed_seconds);
        let controller = AwsWorkerController {
            state: AwsWorkerState::WaitingForVpcEnisReleased,
            vpc_eni_wait_started_at: Some(started),
            ..Default::default()
        };
        // Persist and reload the deadline just as a later reconcile does.
        let controller: AwsWorkerController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        let settings = if owns_network {
            StackSettings {
                network: Some(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                }),
                ..Default::default()
            }
        } else {
            existing_vpc_settings()
        };
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Aws)
            .with_dependency(
                Network::new("default-network".into())
                    .settings(settings.network.clone().unwrap())
                    .build(),
                AwsNetworkController::mock_ready("vpc-test", 2),
            )
            .stack_settings(settings)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let controller = executor.internal_state::<AwsWorkerController>().unwrap();
        let waiting = interfaces_remain && (owns_network || elapsed_seconds < 1200);
        assert_eq!(
            controller.state,
            if waiting {
                AwsWorkerState::WaitingForVpcEnisReleased
            } else {
                AwsWorkerState::DeletingCertificate
            }
        );
        assert_eq!(
            controller.vpc_eni_wait_started_at,
            if waiting { Some(started) } else { None }
        );
    }

    #[rstest]
    #[case::interfaces_released(false)]
    #[case::shared_interfaces_outlive_deadline(true)]
    #[tokio::test]
    async fn existing_vpc_wait_keeps_execution_role_permissions_until_wait_finishes(
        #[case] reaches_deadline: bool,
    ) {
        let server = MockServer::start_async().await;
        let eni_response = |interface_type: &str, status: &str| {
            format!(
                "<DescribeNetworkInterfacesResponse><networkInterfaceSet><item>\
             <networkInterfaceId>eni-test</networkInterfaceId>\
             <description>AWS Lambda VPC ENI-test-basic-func</description>\
             <interfaceType>{interface_type}</interfaceType><status>{status}</status>\
             <vpcId>vpc-test</vpcId></item></networkInterfaceSet>\
             </DescribeNetworkInterfacesResponse>"
            )
        };
        let attached = server
            .mock_async(|when, then| {
                when.method(POST)
                    .body_contains("Action=DescribeNetworkInterfaces");
                then.status(200).body(eni_response("lambda", "in-use"));
            })
            .await;
        let mut aws_config = AwsClientConfig::mock();
        aws_config.service_overrides = Some(AwsServiceOverrides {
            endpoints: HashMap::from([("ec2".to_string(), server.base_url())]),
        });
        let ec2 = Ec2Client::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );
        let iam_calls = Arc::new(AtomicUsize::new(0));
        let mut iam = MockIamApi::new();
        let calls = iam_calls.clone();
        iam.expect_list_attached_role_policies()
            .times(1)
            .returning(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::from_value(
                    serde_json::json!({"ListAttachedRolePoliciesResult": {}}),
                )
                .unwrap())
            });
        iam.expect_list_role_policies().times(1).returning(|_| {
            Ok(
                serde_json::from_value(serde_json::json!({"ListRolePoliciesResult": {
                    "PolicyNames": {"member": ["vpc-access"]}
                }}))
                .unwrap(),
            )
        });
        iam.expect_delete_role_policy()
            .times(1)
            .withf(|_, policy| policy == "vpc-access")
            .returning(|_, _| Ok(()));
        iam.expect_delete_role().times(1).returning(|_| Ok(()));
        let mut logs = MockCloudWatchLogsApi::new();
        logs.expect_delete_log_group().returning(|_| Ok(()));
        let ec2 = Arc::new(ec2);
        let iam = Arc::new(iam);
        let logs = Arc::new(logs);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
            .expect_get_aws_logs_client()
            .returning(move |_| Ok(logs.clone()));
        let worker = basic_function();
        let mut state = StackState::new(Platform::Aws);
        state.resource_prefix = "test".into();
        add_recorded_test_network(&mut state);
        state.resources.insert(
            worker.id.clone(),
            StackResourceState::builder()
                .resource_type(Worker::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Deleting)
                .config(Resource::new(worker))
                .lifecycle(ResourceLifecycle::Live)
                .dependencies(vec![
                    alien_core::ResourceRef::new(
                        ServiceAccount::RESOURCE_TYPE,
                        "default-profile-sa",
                    ),
                    alien_core::ResourceRef::new(Network::RESOURCE_TYPE, "default-network"),
                ])
                .internal_state(
                    serialize_controller(&AwsWorkerController {
                        state: AwsWorkerState::WaitingForVpcEnisReleased,
                        ..Default::default()
                    })
                    .unwrap(),
                )
                .build(),
        );
        state
            .resources
            .get_mut("basic-func")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("vpcEniWaitStartedAt");
        let mut role_controller =
            AwsServiceAccountController::mock_ready("test-default-profile-sa");
        role_controller.transition_to_delete_start().unwrap();
        state.resources.insert(
            "default-profile-sa".into(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Deleting)
                .config(Resource::new(
                    ServiceAccount::new("default-profile-sa".into()).build(),
                ))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![alien_core::ResourceRef::new(
                    Network::RESOURCE_TYPE,
                    "default-network",
                )])
                .internal_state(serialize_controller(&role_controller).unwrap())
                .build(),
        );
        let config = DeploymentConfig::builder()
            .stack_settings(existing_vpc_settings())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let executor = StackExecutor::for_deletion_with_service_provider(
            ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &config,
            Arc::new(provider),
            None,
        )
        .unwrap();
        state = executor.step(state).await.unwrap().next_state;
        attached.assert_hits_async(1).await;
        attached.delete_async().await;
        assert_eq!(iam_calls.load(Ordering::SeqCst), 0);

        // AWS changes a detached Lambda ENI to interface/available before deleting it.
        // Model EC2's type filtering: a lambda-only query incorrectly sees no interfaces.
        let detached = server
            .mock_async(|when, then| {
                when.method(POST)
                    .body_contains("Action=DescribeNetworkInterfaces")
                    .body_contains("Filter.2.Name=interface-type")
                    .body_contains("Filter.2.Value.2=interface");
                then.status(200)
                    .body(eni_response("interface", "available"));
            })
            .await;
        let absent = server
            .mock_async(|when, then| {
                when.method(POST)
                    .body_contains("Action=DescribeNetworkInterfaces");
                then.status(200).body(
                    "<DescribeNetworkInterfacesResponse><networkInterfaceSet/>\
                 </DescribeNetworkInterfacesResponse>",
                );
            })
            .await;
        // More than the old 90-poll limit, without advancing wall time. A scheduler
        // that reconciles quickly must neither fail deletion nor remove permissions.
        for _ in 0..125 {
            state = executor.step(state).await.unwrap().next_state;
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            assert_eq!(
                state.resources["basic-func"].status,
                ResourceStatus::Deleting
            );
            assert_eq!(iam_calls.load(Ordering::SeqCst), 0);
        }
        detached.assert_hits_async(125).await;
        absent.assert_hits_async(0).await;
        if reaches_deadline {
            let resource = state.resources.get_mut("basic-func").unwrap();
            let mut controller: AwsWorkerController =
                serde_json::from_value(resource.internal_state.clone().unwrap()).unwrap();
            controller.vpc_eni_wait_started_at =
                Some(chrono::Utc::now() - chrono::Duration::seconds(20 * 60));
            resource.internal_state = Some(serialize_controller(&controller).unwrap());
        } else {
            detached.delete_async().await;
        }
        for _ in 0..20 {
            state = executor.step(state).await.unwrap().next_state;
            if state.resources["default-profile-sa"].status == ResourceStatus::Deleted {
                break;
            }
        }
        assert_eq!(
            state.resources["basic-func"].status,
            ResourceStatus::Deleted
        );
        assert_eq!(
            state.resources["default-profile-sa"].status,
            ResourceStatus::Deleted
        );
        assert_eq!(iam_calls.load(Ordering::SeqCst), 1);
    }

    // ─────────────── DELETE WITH TAG-CONDITIONED READS ────────────────

    const TEST_CERTIFICATE_ARN: &str =
        "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id";

    fn access_denied() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "Worker".to_string(),
            resource_name: "test-public-func".to_string(),
        })
    }

    /// Lambda's answer to a read of a function that is gone, once the read is granted on
    /// functions without the stack's tag.
    fn function_gone() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Worker".to_string(),
            resource_name: "test-public-func".to_string(),
        })
    }

    /// Deletes a running public worker with the stack executor that production uses, in a
    /// stack that owns its VPC. The VPC detach is always denied, as it is for a function
    /// without the stack's tags. Returns the worker's final state.
    ///
    /// The stack executor matters here: it decides what a denied call does to the rest of the
    /// delete flow.
    #[bon::builder]
    async fn delete_public_worker_in_created_vpc(
        mut mock_lambda: MockLambdaApi,
        mock_ec2: MockEc2Api,
        mock_acm: MockAcmApi,
        #[builder(default = MockCloudWatchLogsApi::new())] mock_logs: MockCloudWatchLogsApi,
        #[builder(default)] uses_custom_domain: bool,
    ) -> StackResourceState {
        let worker = function_public_ingress();
        let worker_id = worker.id.clone();
        let mut controller = AwsWorkerController::mock_ready(&format!("test-{worker_id}"));
        controller.certificate_arn = Some(TEST_CERTIFICATE_ARN.to_string());
        controller.uses_custom_domain = uses_custom_domain;

        let mut state = StackState::new(Platform::Aws);
        state.resource_prefix = "test".to_string();
        add_recorded_test_network(&mut state);
        state.resources.insert(
            worker_id.clone(),
            StackResourceState::builder()
                .resource_type(Worker::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(worker))
                .internal_state(serialize_controller(&controller).unwrap())
                .lifecycle(ResourceLifecycle::Live)
                .dependencies(vec![alien_core::ResourceRef::new(
                    Network::RESOURCE_TYPE,
                    "default-network",
                )])
                .build(),
        );

        mock_lambda
            .expect_update_function_configuration()
            .returning(|_, _| Err(access_denied()));
        let mock_lambda = Arc::new(mock_lambda);
        let mock_ec2 = Arc::new(mock_ec2);
        let mock_acm = Arc::new(mock_acm);
        let mock_logs = Arc::new(mock_logs);
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(mock_lambda.clone()));
        mock_provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(mock_ec2.clone()));
        mock_provider
            .expect_get_aws_acm_client()
            .returning(move |_| Ok(mock_acm.clone()));
        mock_provider
            .expect_get_aws_logs_client()
            .returning(move |_| Ok(mock_logs.clone()));

        let deployment_config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                network: Some(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                }),
                ..Default::default()
            })
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let executor = StackExecutor::for_deletion_with_service_provider(
            ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &deployment_config,
            Arc::new(mock_provider),
            None,
        )
        .unwrap();

        // Step without sleeping, as an external orchestrator would; the cap only stops a
        // regression from looping forever.
        for _ in 0..30 {
            state = executor.step(state).await.unwrap().next_state;
            let worker = &state.resources[&worker_id];
            if worker.status != ResourceStatus::Running && worker.status != ResourceStatus::Deleting
            {
                return worker.clone();
            }
        }
        panic!("worker delete did not finish within 30 steps");
    }

    /// A function deleted out of band has no tags, so the tag-scoped detach and delete are
    /// denied. The read granted on functions without the stack's tag answers NotFound, so the
    /// function is gone, and the delete still waits for the Lambda network interfaces and
    /// deletes only imported certificates.
    #[rstest]
    #[case::imported_certificate(false)]
    #[case::customer_certificate(true)]
    #[tokio::test]
    async fn test_delete_finishes_cleanup_when_the_function_was_deleted_out_of_band(
        #[case] uses_custom_domain: bool,
    ) {
        let calls = Arc::new(Mutex::new(Vec::new()));

        let mut mock_lambda = MockLambdaApi::new();
        let recorded = calls.clone();
        mock_lambda.expect_delete_function().returning(move |_, _| {
            recorded.lock().unwrap().push("DeleteFunction");
            Err(access_denied())
        });
        let recorded = calls.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| {
                recorded.lock().unwrap().push("GetFunctionConfiguration");
                Err(function_gone())
            });

        let mut mock_ec2 = MockEc2Api::new();
        let recorded = calls.clone();
        mock_ec2
            .expect_describe_network_interfaces()
            .withf(|request| {
                request.filters.as_ref().is_some_and(|filters| {
                    filters.iter().any(|filter| {
                        filter.name == "description"
                            && filter.values == ["AWS Lambda VPC ENI-test-public-func*"]
                    })
                })
            })
            .returning(move |_| {
                recorded.lock().unwrap().push("DescribeNetworkInterfaces");
                Ok(DescribeNetworkInterfacesResponse {
                    network_interface_set: None,
                    next_token: None,
                })
            });

        let mut mock_acm = MockAcmApi::new();
        let recorded = calls.clone();
        mock_acm
            .expect_delete_certificate()
            .times(if uses_custom_domain { 0 } else { 1 })
            .withf(|certificate_arn| certificate_arn == TEST_CERTIFICATE_ARN)
            .returning(move |_| {
                recorded.lock().unwrap().push("DeleteCertificate");
                Ok(())
            });

        let mut mock_logs = MockCloudWatchLogsApi::new();
        let recorded = calls.clone();
        mock_logs
            .expect_delete_log_group()
            .withf(|name| name == "/aws/lambda/test-public-func")
            .times(1)
            .returning(move |_| {
                recorded.lock().unwrap().push("DeleteLogGroup");
                Ok(())
            });

        let worker = delete_public_worker_in_created_vpc()
            .mock_lambda(mock_lambda)
            .mock_ec2(mock_ec2)
            .mock_acm(mock_acm)
            .mock_logs(mock_logs)
            .uses_custom_domain(uses_custom_domain)
            .call()
            .await;

        // Detach probe, delete and its probe, then the wait for the function to be gone.
        let mut expected_calls = vec![
            "GetFunctionConfiguration",
            "DeleteFunction",
            "GetFunctionConfiguration",
            "GetFunctionConfiguration",
            "DescribeNetworkInterfaces",
        ];
        if !uses_custom_domain {
            expected_calls.push("DeleteCertificate");
        }
        expected_calls.push("DeleteLogGroup");
        assert_eq!(*calls.lock().unwrap(), expected_calls);
        assert_eq!(worker.status, ResourceStatus::Deleted);
    }

    /// A role that has lost its permissions cannot confirm the Lambda network interfaces are
    /// released. The delete stops there as `DeleteFailed` instead of reporting `Deleted`.
    #[tokio::test]
    async fn test_delete_fails_when_calls_after_the_function_delete_are_denied() {
        let calls = Arc::new(Mutex::new(Vec::new()));

        let mut mock_lambda = MockLambdaApi::new();
        let recorded = calls.clone();
        mock_lambda.expect_delete_function().returning(move |_, _| {
            recorded.lock().unwrap().push("DeleteFunction");
            Err(access_denied())
        });
        let recorded = calls.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| {
                recorded.lock().unwrap().push("GetFunctionConfiguration");
                Err(function_gone())
            });

        let mut mock_ec2 = MockEc2Api::new();
        let recorded = calls.clone();
        mock_ec2
            .expect_describe_network_interfaces()
            .returning(move |_| {
                recorded.lock().unwrap().push("DescribeNetworkInterfaces");
                Err(access_denied())
            });

        let worker = delete_public_worker_in_created_vpc()
            .mock_lambda(mock_lambda)
            .mock_ec2(mock_ec2)
            .mock_acm(MockAcmApi::new())
            .call()
            .await;

        // The denial may be transient, so the describe is retried before the delete fails.
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[..4],
            [
                "GetFunctionConfiguration",
                "DeleteFunction",
                "GetFunctionConfiguration",
                "GetFunctionConfiguration"
            ]
        );
        assert!(calls.len() > 5, "{calls:?}");
        assert!(
            calls[4..]
                .iter()
                .all(|call| *call == "DescribeNetworkInterfaces"),
            "{calls:?}"
        );
        assert_eq!(worker.status, ResourceStatus::DeleteFailed);
    }

    /// Whether the codes of `error` and its sources include `code`.
    fn error_chain_has_code(error: &AlienError, code: &str) -> bool {
        let mut current = Some(error);
        while let Some(error) = current {
            if error.code == code {
                return true;
            }
            current = error.source.as_deref();
        }
        false
    }

    /// The function still exists: the tag-scoped detach is denied but the read succeeds, so
    /// the denial is real. The delete ends `DeleteFailed` with the denial, without deleting
    /// anything, and the worker keeps its ARN.
    #[tokio::test]
    async fn test_delete_of_a_function_that_still_exists_behind_a_denial_fails() {
        let mut mock_lambda = MockLambdaApi::new();
        mock_lambda.expect_delete_function().times(0);
        mock_lambda
            .expect_get_function_configuration()
            .times(1)
            .returning(|_, _| Ok(create_successful_function_response("test-public-func")));

        let worker = delete_public_worker_in_created_vpc()
            .mock_lambda(mock_lambda)
            .mock_ec2(MockEc2Api::new())
            .mock_acm(MockAcmApi::new())
            .call()
            .await;

        assert_eq!(worker.status, ResourceStatus::DeleteFailed);
        let error = worker.error.as_ref().expect("the denial is recorded");
        assert!(
            error_chain_has_code(error, "REMOTE_ACCESS_DENIED"),
            "{error:?}"
        );
        let kept = worker
            .internal_state
            .as_ref()
            .expect("state is kept")
            .to_string();
        assert!(
            kept.contains("arn:aws:lambda:us-east-1:123456789012:function:test-public-func"),
            "the worker keeps its ARN: {kept}"
        );
    }

    /// A role set up before the read was granted on functions without the stack's tag: the
    /// read of a gone function is denied like the detach. Nothing confirms the function is
    /// gone, so the delete ends `DeleteFailed` with the denial instead of `Deleted`.
    #[tokio::test]
    async fn test_delete_without_the_tag_absent_read_reports_the_denial() {
        let mut mock_lambda = MockLambdaApi::new();
        mock_lambda.expect_delete_function().times(0);
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| Err(access_denied()));

        let worker = delete_public_worker_in_created_vpc()
            .mock_lambda(mock_lambda)
            .mock_ec2(MockEc2Api::new())
            .mock_acm(MockAcmApi::new())
            .call()
            .await;

        assert_eq!(worker.status, ResourceStatus::DeleteFailed);
        let error = worker.error.as_ref().expect("the denial is recorded");
        assert!(
            error_chain_has_code(error, "REMOTE_ACCESS_DENIED"),
            "{error:?}"
        );
        assert!(
            worker.internal_state.is_some(),
            "the controller state is kept"
        );
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public workers go through ACM certificate import and API Gateway setup.
    #[tokio::test]
    async fn test_public_function_creates_api_gateway_and_certificate() {
        let worker = function_public_ingress();
        let worker_name = format!("test-{}", worker.id);
        let domain_metadata = create_test_domain_metadata(&worker.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Mock worker creation
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
            .times(1);

        // Validate API Gateway permission is added with the correct apigateway principal
        mock_lambda
            .expect_add_permission()
            .withf(|_, request| {
                request.statement_id == "ApiGatewayInvoke"
                    && request.action == "lambda:InvokeFunction"
                    && request.principal == "apigateway.amazonaws.com"
            })
            .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

        // Mock self-binding env var update
        let worker_name_for_config_update = worker_name.clone();
        mock_lambda
            .expect_update_function_configuration()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &worker_name_for_config_update,
                ))
            });

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        // Validate ACM certificate import
        let mut mock_acm = MockAcmApi::new();
        mock_acm
            .expect_list_certificates()
            .returning(|_| Ok(ListCertificatesResponse::default()));
        mock_acm
            .expect_import_certificate()
            .times(1)
            .returning(|_| {
                Ok(ImportCertificateResponse {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                })
            });
        mock_acm.expect_delete_certificate().returning(|_| Ok(()));

        // Validate API Gateway is created with the worker's name in the API name
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw
            .expect_create_api()
            .withf(|request| request.name.contains("public-func"))
            .returning(|_| {
                Ok(Api {
                    api_id: Some("test-api-id".to_string()),
                    api_endpoint: None,
                    name: None,
                    protocol_type: None,
                })
            });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("public-func.test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
                tags: None,
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        mock_apigw
            .expect_delete_api_mapping()
            .returning(|_, _| Ok(()));
        mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
        mock_apigw.expect_delete_api().returning(|_| Ok(()));

        let mock_provider = setup_mock_service_provider(
            Arc::new(mock_lambda),
            Some(Arc::new(mock_acm)),
            Some(Arc::new(mock_apigw)),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .domain_metadata(domain_metadata)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs (derived from domain_metadata FQDN)
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
        assert!(function_outputs.public_endpoints.contains_key("default"));
    }

    /// Test that verifies private workers don't get URL creation
    #[tokio::test]
    async fn test_private_function_skips_url_creation() {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Mock worker creation
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
            .times(1);

        // API Gateway and permission should NOT be called for private workers
        mock_lambda.expect_add_permission().times(0);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
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

    /// Lambda mock for a private worker's create flow that records when the
    /// function is created.
    fn lambda_mock_recording_create(
        worker_name: &str,
        calls: Arc<Mutex<Vec<String>>>,
    ) -> MockLambdaApi {
        let mut mock_lambda = MockLambdaApi::new();
        let worker_name_for_create = worker_name.to_string();
        mock_lambda.expect_create_function().returning(move |_| {
            calls.lock().unwrap().push("CreateFunction".to_string());
            Ok(create_successful_function_response(&worker_name_for_create))
        });
        let worker_name_for_get = worker_name.to_string();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)));
        mock_lambda
    }

    #[tokio::test]
    async fn test_create_makes_the_log_group_with_retention_before_the_function() {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);
        let calls = Arc::new(Mutex::new(Vec::new()));

        let mut mock_logs = MockCloudWatchLogsApi::new();
        let create_calls = calls.clone();
        mock_logs
            .expect_create_log_group()
            .times(1)
            .returning(move |request| {
                assert!(request.tags.is_none());
                create_calls
                    .lock()
                    .unwrap()
                    .push(format!("CreateLogGroup {}", request.log_group_name));
                Ok(())
            });
        let retention_calls = calls.clone();
        mock_logs
            .expect_put_retention_policy()
            .times(1)
            .returning(move |request| {
                retention_calls.lock().unwrap().push(format!(
                    "PutRetentionPolicy {} {}",
                    request.log_group_name, request.retention_in_days
                ));
                Ok(())
            });
        let mock_provider = setup_mock_service_provider_with_logs(
            Arc::new(lambda_mock_recording_create(&worker_name, calls.clone())),
            None,
            None,
            Arc::new(mock_logs),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                format!("CreateLogGroup /aws/lambda/{worker_name}"),
                format!("PutRetentionPolicy /aws/lambda/{worker_name} 30"),
                "CreateFunction".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn test_create_sets_retention_on_an_existing_log_group() {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);
        let calls = Arc::new(Mutex::new(Vec::new()));

        let mut mock_logs = MockCloudWatchLogsApi::new();
        mock_logs
            .expect_create_log_group()
            .times(1)
            .returning(|request| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "CloudWatch Logs log group".to_string(),
                        resource_name: request.log_group_name,
                        message: "The specified log group already exists".to_string(),
                    },
                ))
            });
        let retention_calls = calls.clone();
        mock_logs
            .expect_put_retention_policy()
            .times(1)
            .returning(move |request| {
                retention_calls
                    .lock()
                    .unwrap()
                    .push(format!("PutRetentionPolicy {}", request.retention_in_days));
                Ok(())
            });
        let mock_provider = setup_mock_service_provider_with_logs(
            Arc::new(lambda_mock_recording_create(&worker_name, calls.clone())),
            None,
            None,
            Arc::new(mock_logs),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "PutRetentionPolicy 30".to_string(),
                "CreateFunction".to_string()
            ]
        );
    }

    /// Mock CloudWatch Logs whose create and retention calls answer with the
    /// given results, recording each call.
    fn logs_mock_answering(
        calls: Arc<Mutex<Vec<String>>>,
        create: fn(String) -> alien_client_core::Result<()>,
        retention: fn(String) -> alien_client_core::Result<()>,
    ) -> MockCloudWatchLogsApi {
        let mut mock_logs = MockCloudWatchLogsApi::new();
        let create_calls = calls.clone();
        mock_logs
            .expect_create_log_group()
            .returning(move |request| {
                create_calls
                    .lock()
                    .unwrap()
                    .push("CreateLogGroup".to_string());
                create(request.log_group_name)
            });
        mock_logs
            .expect_put_retention_policy()
            .returning(move |request| {
                calls.lock().unwrap().push("PutRetentionPolicy".to_string());
                retention(request.log_group_name)
            });
        mock_logs
    }

    fn log_group_access_denied(log_group_name: String) -> alien_client_core::Result<()> {
        Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "CloudWatch Logs log group".to_string(),
            resource_name: log_group_name,
        }))
    }

    fn throttled(_: String) -> alien_client_core::Result<()> {
        Err(AlienError::new(CloudClientErrorData::RateLimitExceeded {
            message: "Rate exceeded".to_string(),
        }))
    }

    async fn run_private_worker_create(
        logs: MockCloudWatchLogsApi,
        calls: Arc<Mutex<Vec<String>>>,
    ) -> (ResourceStatus, crate::error::Result<()>) {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);
        let mock_provider = setup_mock_service_provider_with_logs(
            Arc::new(lambda_mock_recording_create(&worker_name, calls)),
            None,
            None,
            Arc::new(logs),
        );
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        let result = executor.run_until_terminal().await;
        (executor.status(), result)
    }

    /// A setup from before the log permissions existed leaves the management
    /// role without them; the worker must still be created.
    #[rstest]
    #[case::create_denied(log_group_access_denied as fn(String) -> alien_client_core::Result<()>, &["CreateLogGroup", "CreateFunction"][..])]
    #[case::retention_denied(|_: String| Ok(()), &["CreateLogGroup", "PutRetentionPolicy", "CreateFunction"][..])]
    #[tokio::test]
    async fn test_create_continues_when_log_group_access_is_denied(
        #[case] create: fn(String) -> alien_client_core::Result<()>,
        #[case] expected_calls: &[&str],
    ) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let logs = logs_mock_answering(calls.clone(), create, log_group_access_denied);

        let (status, result) = run_private_worker_create(logs, calls.clone()).await;

        result.expect("access denied on the log group does not fail the worker");
        assert_eq!(status, ResourceStatus::Running);
        assert_eq!(*calls.lock().unwrap(), expected_calls);
    }

    #[tokio::test]
    async fn test_create_fails_on_other_log_group_errors() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let logs = logs_mock_answering(calls.clone(), throttled, |_| Ok(()));

        let (_, result) = run_private_worker_create(logs, calls.clone()).await;

        let error = result.expect_err("a throttled log group create fails the worker");
        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert_eq!(*calls.lock().unwrap(), vec!["CreateLogGroup".to_string()]);
    }

    #[tokio::test]
    async fn test_function_creation_allows_slow_aws_activation() {
        let worker = function_private_ingress();
        let worker_name = format!("test-{}", worker.id);
        let status_checks = Arc::new(AtomicUsize::new(0));

        let mut mock_lambda = MockLambdaApi::new();
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        let worker_name_for_get = worker_name.clone();
        let status_checks_for_mock = Arc::clone(&status_checks);
        mock_lambda
            .expect_get_function_configuration()
            .times(22)
            .returning(move |_, _| {
                if status_checks_for_mock.fetch_add(1, Ordering::SeqCst) < 21 {
                    return Ok(FunctionConfiguration {
                        function_name: Some(worker_name_for_get.clone()),
                        function_arn: Some(format!(
                            "arn:aws:lambda:us-east-1:123456789012:function:{}",
                            worker_name_for_get
                        )),
                        state: Some("Pending".to_string()),
                        last_update_status: Some("InProgress".to_string()),
                        kms_key_arn: None,
                    });
                }

                Ok(create_successful_function_response(&worker_name_for_get))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build");

        // Step directly so the test exercises the retry budget without sleeping for every
        // production polling delay.
        for _ in 0..30 {
            executor
                .step()
                .await
                .expect("slow AWS activation should remain within the polling window");
            if executor.status() == ResourceStatus::Running {
                break;
            }
        }

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(status_checks.load(Ordering::SeqCst), 22);
    }

    /// Test that verifies correct worker configuration parameters
    #[tokio::test]
    async fn test_worker_configuration_validation() {
        let worker = function_custom_config();
        let worker_name = format!("test-{}", worker.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Validate worker creation request has correct parameters
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .withf(|request| {
                request.memory_size == Some(512)
                    && request.timeout == Some(120)
                    && request.package_type == "Image"
                    && request
                        .architectures
                        .as_ref()
                        .map(|a| a.contains(&"arm64".to_string()))
                        .unwrap_or(false)
            })
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
            .times(1);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
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
        let worker_name = format!("test-{}", worker.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Validate worker creation request has environment variables
        let worker_name_for_create = worker_name.clone();
        mock_lambda
            .expect_create_function()
            .withf(|request| {
                if let Some(env) = &request.environment {
                    if let Some(vars) = &env.variables {
                        vars.get("APP_ENV") == Some(&"production".to_string())
                            && vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                            && vars.get("DB_NAME") == Some(&"myapp".to_string())
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

        let worker_name_for_get = worker_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
            .times(1);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Worker".to_string(),
                        resource_name: "test-worker".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// The API gateway step deletes the mapping before the domain. A denial on the domain after
    /// the mapping is gone must not count as "nothing deleted": restoring the failed create then
    /// would resume a worker without its mapping. The removed mapping leaves state at once.
    #[tokio::test]
    async fn api_gateway_delete_denied_after_the_mapping_is_gone_is_not_nothing_deleted() {
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw
            .expect_delete_api_mapping()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_apigw
            .expect_delete_domain_name()
            .times(1)
            .returning(|domain| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "DomainName".to_string(),
                    resource_name: domain.to_string(),
                }))
            });
        // The domain is still readable, so the denied delete is a real denial.
        mock_apigw
            .expect_get_domain_name()
            .times(1)
            .returning(|domain| Ok(domain_with_tags(domain, HashMap::new())));
        mock_apigw.expect_delete_api().times(0);
        let provider = setup_mock_service_provider(
            Arc::new(MockLambdaApi::new()),
            None,
            Some(Arc::new(mock_apigw)),
        );

        let controller = AwsWorkerController {
            state: AwsWorkerState::DeletingApiGateway,
            arn: Some("arn:aws:lambda:us-east-1:123456789012:function:test".to_string()),
            api_id: Some("api-1".to_string()),
            api_mapping_id: Some("mapping-1".to_string()),
            domain_name: Some("api.example.com".to_string()),
            domain_confirmed: true,
            ..Default::default()
        };
        assert!(controller.nothing_deleted_yet());

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .expect_err("the domain delete is denied");

        let state = executor
            .internal_state::<AwsWorkerController>()
            .expect("worker controller");
        assert_eq!(
            state.api_mapping_id, None,
            "the deleted mapping leaves state"
        );
        assert_eq!(state.domain_name.as_deref(), Some("api.example.com"));
        assert_eq!(state.api_id.as_deref(), Some("api-1"));
        assert!(!state.nothing_deleted_yet());
    }
    // ─────────────── CUSTOM DOMAIN RECORDED BEFORE IT EXISTS ────────────────

    const DOMAIN: &str = "public-func.test.example.com";

    fn domain_with_tags(domain: &str, tags: HashMap<String, String>) -> DomainName {
        DomainName {
            domain_name: Some(domain.to_string()),
            domain_name_configurations: Some(vec![DomainNameConfiguration {
                certificate_arn: TEST_CERTIFICATE_ARN.to_string(),
                endpoint_type: "REGIONAL".to_string(),
                security_policy: "TLS_1_2".to_string(),
                api_gateway_domain_name: Some(
                    "d-1.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
            }]),
            tags: Some(tags),
        }
    }

    fn not_found(resource_type: &str) -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: "missing".to_string(),
        })
    }

    type Calls = Arc<Mutex<Vec<String>>>;

    fn record(calls: &Calls, call: impl Into<String>) {
        calls.lock().unwrap().push(call.into());
    }

    /// The worker as a create that failed in `CreatingApiDomain` left it: the function and
    /// HTTP API exist; the domain name was recorded by `CreateStart` but never created.
    fn failed_at_creating_api_domain() -> AwsWorkerController {
        AwsWorkerController {
            state: AwsWorkerState::CreateFailed,
            arn: Some(
                "arn:aws:lambda:us-east-1:123456789012:function:test-public-func".to_string(),
            ),
            url: Some(format!("https://{DOMAIN}")),
            worker_name: Some("test-public-func".to_string()),
            fqdn: Some(DOMAIN.to_string()),
            certificate_id: Some("test-cert-id".to_string()),
            certificate_arn: Some(TEST_CERTIFICATE_ARN.to_string()),
            api_id: Some("api-1".to_string()),
            integration_id: Some("integration-1".to_string()),
            route_id: Some("route-1".to_string()),
            stage_name: Some("$default".to_string()),
            domain_name: Some(DOMAIN.to_string()),
            ..Default::default()
        }
    }

    /// Serialized as the previous version wrote it: it recorded the domain name before the
    /// create and had neither `domainConfirmed` nor `domainCreateToken`.
    fn saved_by_previous_version(controller: &AwsWorkerController) -> serde_json::Value {
        fn strip(value: &mut serde_json::Value) {
            if let Some(object) = value.as_object_mut() {
                object.remove("domainConfirmed");
                object.remove("domainCreateToken");
                object.values_mut().for_each(strip);
            }
        }
        let mut value = serialize_controller(controller).unwrap();
        strip(&mut value);
        let text = value.to_string();
        assert!(
            text.contains(DOMAIN) && !text.contains("domainConfirmed"),
            "{text}"
        );
        value
    }

    /// Lambda and ACM mocks for the delete of `test-public-func` and its imported certificate.
    fn delete_mocks(calls: &Calls) -> (MockLambdaApi, MockAcmApi) {
        let mut lambda = MockLambdaApi::new();
        let recorded = calls.clone();
        lambda
            .expect_delete_function()
            .withf(|name, _| name == "test-public-func")
            .times(1)
            .returning(move |_, _| {
                record(&recorded, "DeleteFunction");
                Ok(())
            });
        lambda
            .expect_get_function_configuration()
            .returning(|_, _| Err(not_found("Function")));
        let mut acm = MockAcmApi::new();
        let recorded = calls.clone();
        acm.expect_delete_certificate()
            .withf(|arn| arn == TEST_CERTIFICATE_ARN)
            .times(1)
            .returning(move |_| {
                record(&recorded, "DeleteCertificate");
                Ok(())
            });
        (lambda, acm)
    }

    /// API Gateway mock for a domain that does not exist: AWS answers the tag-conditioned read
    /// with `lookup_error`, and any domain delete is a test failure.
    fn apigw_without_domain(
        calls: &Calls,
        lookup_error: fn() -> AlienError<CloudClientErrorData>,
    ) -> MockApiGatewayV2Api {
        let mut apigw = MockApiGatewayV2Api::new();
        let recorded = calls.clone();
        apigw
            .expect_get_domain_name()
            .withf(|domain| domain == DOMAIN)
            .times(1)
            .returning(move |_| {
                record(&recorded, "GetDomainName");
                Err(lookup_error())
            });
        apigw.expect_delete_domain_name().times(0);
        apigw.expect_delete_api_mapping().times(0);
        let recorded = calls.clone();
        apigw
            .expect_delete_api()
            .withf(|api_id| api_id == "api-1")
            .times(1)
            .returning(move |_| {
                record(&recorded, "DeleteApi");
                Ok(())
            });
        apigw
    }

    fn provider(
        lambda: MockLambdaApi,
        acm: MockAcmApi,
        apigw: MockApiGatewayV2Api,
    ) -> Arc<MockPlatformServiceProvider> {
        let lambda = Arc::new(lambda);
        let acm = Arc::new(acm);
        let apigw = Arc::new(apigw);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(lambda.clone()));
        provider
            .expect_get_aws_acm_client()
            .returning(move |_| Ok(acm.clone()));
        provider
            .expect_get_aws_apigatewayv2_client()
            .returning(move |_| Ok(apigw.clone()));
        let logs = create_logs_mock_accepting_log_groups();
        provider
            .expect_get_aws_logs_client()
            .returning(move |_| Ok(logs.clone()));
        Arc::new(provider)
    }

    /// Stack state with the public worker failed in `CreatingApiDomain`, saved by the previous
    /// version, as on the deployment where this was found.
    fn state_after_failed_domain_create() -> StackState {
        let failed = failed_at_creating_api_domain();
        let mut checkpoint = failed.clone();
        checkpoint.state = AwsWorkerState::CreatingApiDomain;

        let mut state = StackState::new(Platform::Aws);
        state.resource_prefix = "test".to_string();
        let mut resource = StackResourceState::builder()
            .resource_type(Worker::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::ProvisionFailed)
            .config(Resource::new(function_public_ingress()))
            .internal_state(saved_by_previous_version(&failed))
            .lifecycle(ResourceLifecycle::Live)
            .dependencies(vec![])
            .build();
        resource.last_failed_state = Some(saved_by_previous_version(&checkpoint));
        state.resources.insert("public-func".to_string(), resource);
        state
    }

    fn no_network_deployment_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    /// The release that follows drops the public endpoint, so the failed create is replaced.
    /// Its delete must not send a domain delete (AWS answers 403 for a domain that does not
    /// exist), and must get through the API and the function so the new config is created.
    #[tokio::test]
    async fn replace_after_a_failed_domain_create_deletes_the_rest_and_proceeds() {
        let calls = Calls::default();
        let (lambda, acm) = delete_mocks(&calls);
        let apigw = apigw_without_domain(&calls, access_denied);

        let v2 = Worker::new("public-func".to_string())
            .code(WorkerCode::Image {
                image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/public:latest".to_string(),
            })
            .permissions("default-profile".to_string())
            .build();
        let stack = Stack::new("worker-replace".to_string())
            .add(v2, ResourceLifecycle::Live)
            .build();
        let executor =
            StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
                .deployment_config(&no_network_deployment_config())
                .service_provider(provider(lambda, acm, apigw))
                .build()
                .unwrap();

        let mut state = state_after_failed_domain_create();
        assert_eq!(executor.plan(&state).unwrap().replaces, ["public-func"]);

        for _ in 0..30 {
            state = executor.step(state).await.unwrap().next_state;
            let resource = &state.resources["public-func"];
            assert!(resource.error.is_none(), "{:?}", resource.error);
            if resource.status == ResourceStatus::Deleted {
                break;
            }
            assert_eq!(resource.status, ResourceStatus::Deleting);
        }
        assert_eq!(
            state.resources["public-func"].status,
            ResourceStatus::Deleted
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "GetDomainName",
                "DeleteApi",
                "DeleteFunction",
                "DeleteCertificate"
            ]
        );

        // The planner may list the same create twice (deleted, and deleted with a changed
        // config); the step creates it once.
        let plan = executor.plan(&state).unwrap();
        assert!(
            !plan.creates.is_empty() && plan.creates.iter().all(|id| id == "public-func"),
            "{plan:?}"
        );
        assert!(plan.replaces.is_empty(), "{plan:?}");

        // The next step starts a fresh create with the new config. This stack leaves out the
        // worker's service account, so the create stops at its first step, before any AWS
        // call; what matters is that it starts from nothing the failed create recorded.
        state = executor.step(state).await.unwrap().next_state;
        let recreated = &state.resources["public-func"];
        assert!(recreated
            .config
            .downcast_ref::<Worker>()
            .unwrap()
            .public_endpoints
            .is_empty());
        let started = recreated
            .last_failed_state
            .as_ref()
            .expect("the new create's checkpoint");
        assert_eq!(started["state"], "createStart", "{started}");
        assert!(
            started["apiId"].is_null() && started["domainName"].is_null(),
            "{started}"
        );
        assert_eq!(
            recreated.error.as_ref().map(|error| error.code.as_str()),
            Some("DEPENDENCY_NOT_FOUND")
        );
        assert_eq!(calls.lock().unwrap().len(), 4, "no further deletes");
    }

    /// Tearing down the same worker deletes its HTTP API and function. The read of the missing
    /// domain is denied under the tag-conditioned grant, or not found with broader credentials.
    #[rstest]
    #[case::read_denied(access_denied)]
    #[case::read_not_found(|| not_found("DomainName"))]
    #[tokio::test]
    async fn teardown_after_a_failed_domain_create_deletes_the_api_and_function(
        #[case] lookup_error: fn() -> AlienError<CloudClientErrorData>,
    ) {
        let calls = Calls::default();
        let (lambda, acm) = delete_mocks(&calls);
        let apigw = apigw_without_domain(&calls, lookup_error);
        let executor = StackExecutor::for_deletion_with_service_provider(
            ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &no_network_deployment_config(),
            provider(lambda, acm, apigw),
            None,
        )
        .unwrap();

        let mut state = state_after_failed_domain_create();
        for _ in 0..30 {
            state = executor.step(state).await.unwrap().next_state;
            if state.resources["public-func"].status == ResourceStatus::Deleted {
                break;
            }
        }
        assert_eq!(
            state.resources["public-func"].status,
            ResourceStatus::Deleted
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "GetDomainName",
                "DeleteApi",
                "DeleteFunction",
                "DeleteCertificate"
            ]
        );
    }

    /// The current version: a failed domain create leaves its token and an unconfirmed name,
    /// and the delete checks the name instead of deleting it.
    #[tokio::test]
    async fn failed_domain_create_is_recorded_unconfirmed_and_delete_checks_it_first() {
        let calls = Calls::default();
        let (lambda, acm) = delete_mocks(&calls);
        let mut apigw = apigw_without_domain(&calls, access_denied);
        let recorded = calls.clone();
        apigw
            .expect_create_domain_name()
            .times(1)
            .returning(move |request| {
                record(&recorded, "CreateDomainName");
                assert!(request
                    .tags
                    .as_ref()
                    .is_some_and(|tags| tags.contains_key("CreateAttempt")));
                Err(AlienError::new(CloudClientErrorData::InvalidInput {
                    message: "Certificate is not issued".to_string(),
                    field_name: None,
                }))
            });

        let mut controller = failed_at_creating_api_domain();
        controller.state = AwsWorkerState::CreatingApiDomain;
        let mut executor = SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(lambda, acm, apigw))
            .build()
            .await
            .unwrap();

        executor.step().await.expect("records the create token");
        executor.step().await.expect_err("the domain create fails");
        let failed = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(failed.domain_name.as_deref(), Some(DOMAIN));
        assert!(!failed.domain_confirmed);
        assert!(failed.domain_create_token.is_some());

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "CreateDomainName",
                "GetDomainName",
                "DeleteApi",
                "DeleteFunction",
                "DeleteCertificate"
            ]
        );
    }

    /// Runs the API Gateway delete step for a basic worker whose domain name was recorded but
    /// never confirmed, with `get_domain_name` answering `domain`.
    async fn delete_step_with_unconfirmed_domain(
        domain: DomainName,
        expected_domain_deletes: usize,
    ) -> AwsWorkerController {
        let mut apigw = MockApiGatewayV2Api::new();
        apigw
            .expect_get_domain_name()
            .withf(|name| name == "api.example.com")
            .times(1)
            .returning(move |_| Ok(domain.clone()));
        apigw
            .expect_delete_domain_name()
            .withf(|name| name == "api.example.com")
            .times(expected_domain_deletes)
            .returning(|_| Ok(()));
        let provider = setup_mock_service_provider(
            Arc::new(MockLambdaApi::new()),
            None,
            Some(Arc::new(apigw)),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(AwsWorkerController {
                state: AwsWorkerState::DeletingApiGateway,
                domain_name: Some("api.example.com".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(provider)
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .expect("the API Gateway delete step succeeds");
        let state = executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .clone();
        assert_eq!(state.state, AwsWorkerState::DeletingEventSourceMappings);
        assert_eq!(state.domain_name, None);
        state
    }

    /// A domain a lost create response left behind carries our tags: it is ours and deleted.
    #[tokio::test]
    async fn unconfirmed_domain_carrying_our_tags_is_deleted() {
        let mut tags = standard_resource_tags("test", &basic_function().id);
        tags.insert("CreateAttempt".to_string(), "any".to_string());
        let state =
            delete_step_with_unconfirmed_domain(domain_with_tags("api.example.com", tags), 1).await;
        assert!(state.delete_removed_something);
    }

    /// A domain with the same name that is not this worker's is never deleted.
    #[rstest]
    #[case::untagged(HashMap::new())]
    #[case::other_resource(standard_resource_tags("test", "other-worker"))]
    #[case::other_stack(standard_resource_tags("other", &basic_function().id))]
    #[tokio::test]
    async fn unconfirmed_domain_without_our_tags_is_left_alone(
        #[case] tags: HashMap<String, String>,
    ) {
        let state =
            delete_step_with_unconfirmed_domain(domain_with_tags("api.example.com", tags), 0).await;
        assert!(!state.delete_removed_something);
    }

    /// A streaming (REST) worker that failed in `CreatingRestDomain`: the domain is checked
    /// through the REST API, no base path mapping or domain delete is sent, and the REST API
    /// is deleted.
    #[tokio::test]
    async fn rest_worker_with_a_never_created_domain_deletes_its_rest_api() {
        let mut rest = MockApiGatewayApi::new();
        rest.expect_get_domain_name()
            .withf(|name| name == "stream.example.com")
            .times(1)
            .returning(|_| Err(access_denied()));
        rest.expect_delete_base_path_mapping().times(0);
        rest.expect_delete_domain_name().times(0);
        rest.expect_delete_rest_api()
            .withf(|id| id == "rest-1")
            .times(1)
            .returning(|_| Ok(()));
        let rest = Arc::new(rest);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_apigateway_client()
            .returning(move |_| Ok(rest.clone()));

        let worker = Worker::new("stream-func".to_string())
            .code(WorkerCode::Image {
                image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/stream:latest".to_string(),
            })
            .permissions("default-profile".to_string())
            .environment(HashMap::from([(
                "WORKER_RESPONSE_STREAMING".to_string(),
                "true".to_string(),
            )]))
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker)
            .controller(AwsWorkerController {
                state: AwsWorkerState::DeletingApiGateway,
                rest_api_id: Some("rest-1".to_string()),
                stage_name: Some("prod".to_string()),
                domain_name: Some("stream.example.com".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .expect("the API Gateway delete step succeeds");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::DeletingEventSourceMappings);
        assert_eq!(state.domain_name, None);
        assert_eq!(state.rest_api_id, None);
    }

    /// A confirmed object can be gone too (deleted out of band, or by an earlier attempt whose
    /// response was lost). Its tag-scoped delete is denied, the read granted on untagged APIs
    /// answers NotFound, and the delete goes on to the next object instead of ending there.
    #[tokio::test]
    async fn denied_delete_of_an_api_that_is_gone_continues_to_the_function() {
        let calls = Calls::default();
        let (lambda, acm) = delete_mocks(&calls);
        let mut apigw = MockApiGatewayV2Api::new();
        let recorded = calls.clone();
        apigw.expect_delete_api().times(1).returning(move |_| {
            record(&recorded, "DeleteApi");
            Err(access_denied())
        });
        let recorded = calls.clone();
        apigw.expect_get_api().times(1).returning(move |_| {
            record(&recorded, "GetApi");
            Err(not_found("API"))
        });

        let mut controller = failed_at_creating_api_domain();
        controller.domain_name = None;
        let mut executor = SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(lambda, acm, apigw))
            .build()
            .await
            .unwrap();
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            ["DeleteApi", "GetApi", "DeleteFunction", "DeleteCertificate"]
        );
    }

    /// On a role set up before the read was granted on untagged APIs, the read of a gone API is
    /// denied like its delete. Nothing confirms it is gone, so the denial is returned and the API
    /// id kept, and the function is not deleted.
    #[tokio::test]
    async fn denied_delete_of_an_api_whose_read_is_denied_too_is_an_error() {
        let calls = Calls::default();
        let mut lambda = MockLambdaApi::new();
        lambda.expect_delete_function().times(0);
        let mut acm = MockAcmApi::new();
        acm.expect_delete_certificate().times(0);
        let mut apigw = MockApiGatewayV2Api::new();
        let recorded = calls.clone();
        apigw.expect_delete_api().returning(move |_| {
            record(&recorded, "DeleteApi");
            Err(access_denied())
        });
        let recorded = calls.clone();
        apigw.expect_get_api().returning(move |_| {
            record(&recorded, "GetApi");
            Err(access_denied())
        });

        let mut controller = failed_at_creating_api_domain();
        controller.domain_name = None;
        let mut executor = SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider(lambda, acm, apigw))
            .build()
            .await
            .unwrap();
        executor.delete().unwrap();
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the denial stands");

        assert!(
            error_chain_codes(&error).contains(&"REMOTE_ACCESS_DENIED".to_string()),
            "{error:?}"
        );
        assert!(error.retryable, "the denial may still be transient");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(
            state.api_id.as_deref(),
            Some("api-1"),
            "the API id is kept for a retry"
        );
        assert_eq!(*calls.lock().unwrap(), ["DeleteApi", "GetApi"]);
    }

    /// When the API is still readable, its denied delete is real and is returned.
    #[tokio::test]
    async fn denied_delete_of_an_api_that_still_exists_is_an_error() {
        let mut apigw = MockApiGatewayV2Api::new();
        apigw
            .expect_delete_api()
            .times(1)
            .returning(|_| Err(access_denied()));
        apigw.expect_get_api().times(1).returning(|_| {
            Ok(Api {
                api_id: Some("api-1".to_string()),
                api_endpoint: None,
                name: None,
                protocol_type: None,
            })
        });
        let provider = setup_mock_service_provider(
            Arc::new(MockLambdaApi::new()),
            None,
            Some(Arc::new(apigw)),
        );
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_function())
            .controller(AwsWorkerController {
                state: AwsWorkerState::DeletingApiGateway,
                api_id: Some("api-1".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(provider)
            .build()
            .await
            .unwrap();
        let error = executor.step().await.expect_err("the denial is real");
        assert!(
            error_chain_codes(&error).contains(&"REMOTE_ACCESS_DENIED".to_string()),
            "{error:?}"
        );
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.api_id.as_deref(), Some("api-1"));
    }

    fn error_chain_codes(error: &AlienError<crate::error::ErrorData>) -> Vec<String> {
        let mut codes = vec![error.code.clone()];
        let mut source = error.source.as_deref();
        while let Some(inner) = source {
            codes.push(inner.code.clone());
            source = inner.source.as_deref();
        }
        codes
    }

    /// API Gateway as the tests below see it: at most one domain named `DOMAIN`, with its tags.
    type DomainWorld = Arc<Mutex<Option<HashMap<String, String>>>>;

    /// A V2 client over `world`: CreateDomainName stores the domain (or answers
    /// ConflictException when one exists) and GetDomainName reads it. While `lose_responses`
    /// is above zero, a create that AWS accepted answers 503 instead, as a lost response does.
    fn domain_world_provider(
        world: DomainWorld,
        creates: Arc<Mutex<Vec<HashMap<String, String>>>>,
        lose_responses: usize,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut apigw = MockApiGatewayV2Api::new();
        let created = world.clone();
        let mut lose_responses = lose_responses;
        apigw.expect_create_domain_name().returning(move |request| {
            let tags = request.tags.clone().unwrap();
            creates.lock().unwrap().push(tags.clone());
            let mut domain = created.lock().unwrap();
            if domain.is_some() {
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "ApiGateway".to_string(),
                        resource_name: DOMAIN.to_string(),
                        message: "already exists".to_string(),
                    },
                ));
            }
            *domain = Some(tags.clone());
            if lose_responses > 0 {
                lose_responses -= 1;
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteServiceUnavailable {
                        message: "connection reset".to_string(),
                    },
                ));
            }
            Ok(domain_with_tags(DOMAIN, tags))
        });
        apigw
            .expect_get_domain_name()
            .returning(move |name| match world.lock().unwrap().clone() {
                Some(tags) => Ok(domain_with_tags(name, tags)),
                None => Err(not_found("DomainName")),
            });
        setup_mock_service_provider(Arc::new(MockLambdaApi::new()), None, Some(Arc::new(apigw)))
    }

    async fn domain_executor(
        controller: AwsWorkerController,
        provider: Arc<MockPlatformServiceProvider>,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(provider)
            .build()
            .await
            .unwrap()
    }

    fn at_creating_api_domain() -> AwsWorkerController {
        let mut controller = failed_at_creating_api_domain();
        controller.state = AwsWorkerState::CreatingApiDomain;
        controller
    }

    /// The token is saved by a step of its own, before the step that calls create.
    #[tokio::test]
    async fn domain_create_token_is_saved_before_the_create_step() {
        let world = DomainWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = domain_executor(
            at_creating_api_domain(),
            domain_world_provider(world.clone(), creates.clone(), 0),
        )
        .await;

        executor.step().await.expect("records the token");
        let saved = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(saved.state, AwsWorkerState::CreatingApiDomain);
        let token = saved.domain_create_token.clone().expect("token saved");
        assert!(
            creates.lock().unwrap().is_empty(),
            "no create in the token step"
        );

        executor.step().await.expect("creates the domain");
        assert_eq!(
            creates.lock().unwrap()[0].get("CreateAttempt"),
            Some(&token)
        );
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiMapping);
        assert!(state.domain_confirmed);
    }

    /// AWS creates the domain but the process stops before the create step is saved. The
    /// controller restarts from the checkpoint saved before that step, which already holds the
    /// token, so the retry recognizes the domain as its own.
    #[tokio::test]
    async fn domain_created_before_a_crash_is_adopted_from_the_previous_checkpoint() {
        let world = DomainWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let provider = domain_world_provider(world.clone(), creates.clone(), 0);
        let mut executor = domain_executor(at_creating_api_domain(), provider.clone()).await;

        executor.step().await.expect("records the token");
        let checkpoint = executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .clone();
        executor.step().await.expect("AWS accepts the create");
        assert!(world.lock().unwrap().is_some());
        drop(executor);

        let mut restarted = domain_executor(checkpoint, provider).await;
        restarted
            .step()
            .await
            .expect("the retry adopts its own domain");
        let state = restarted.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiMapping);
        assert_eq!(state.domain_name.as_deref(), Some(DOMAIN));
        assert!(state.domain_confirmed);
        assert!(state.load_balancer.is_some());
        let creates = creates.lock().unwrap();
        assert_eq!(
            creates.len(),
            2,
            "the retry's create is answered ConflictException"
        );
        assert_eq!(creates[0], creates[1], "both carry the saved token");
    }

    /// The first create reaches AWS but its response is lost; the retry adopts the domain.
    #[tokio::test]
    async fn lost_domain_create_response_is_adopted_by_its_create_token() {
        let world = DomainWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = domain_executor(
            at_creating_api_domain(),
            domain_world_provider(world.clone(), creates.clone(), 1),
        )
        .await;

        executor.step().await.expect("records the token");
        executor.step().await.expect_err("the response is lost");
        executor
            .step()
            .await
            .expect("the retry adopts its own domain");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiMapping);
        assert!(state.domain_confirmed);
        let creates = creates.lock().unwrap();
        assert_eq!(creates.len(), 2);
        assert_eq!(creates[0], creates[1]);
    }

    /// Same name and ownership tags, but another create's token: a leftover from an earlier
    /// instance is not adopted.
    #[tokio::test]
    async fn domain_from_another_create_attempt_is_not_adopted() {
        let mut leftover = standard_resource_tags("test", "public-func");
        leftover.insert("CreateAttempt".to_string(), "another-attempt".to_string());
        let world: DomainWorld = Arc::new(Mutex::new(Some(leftover)));
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = domain_executor(
            at_creating_api_domain(),
            domain_world_provider(world, creates, 0),
        )
        .await;

        executor.step().await.expect("records the token");
        executor
            .step()
            .await
            .expect_err("the domain is not this create's");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert!(!state.domain_confirmed);
        assert!(state.load_balancer.is_none());
    }

    // ─────────────── FUNCTION CREATE TOKEN ────────────────

    /// Lambda as the tests below see it: at most one function, the private worker's, with the
    /// tags it was created with.
    type FunctionWorld = Arc<Mutex<Option<HashMap<String, String>>>>;

    /// How GetFunction answers in the tests below.
    #[derive(Clone, Copy)]
    enum FunctionRead {
        Allowed,
        Denied,
    }

    /// A Lambda client over `world`. CreateFunction stores the function, or answers
    /// ResourceConflictException when one exists. While `lose_responses` is above zero, a create
    /// that Lambda accepted fails with a dropped connection instead, as a lost response does.
    /// Every CreateFunction's tags are recorded in `creates`.
    fn function_world_lambda(
        world: FunctionWorld,
        creates: Arc<Mutex<Vec<HashMap<String, String>>>>,
        lose_responses: usize,
        read: FunctionRead,
    ) -> MockLambdaApi {
        let name = format!("test-{}", function_private_ingress().id);
        let mut lambda = MockLambdaApi::new();
        let created = world.clone();
        let create_name = name.clone();
        let mut lose_responses = lose_responses;
        lambda.expect_create_function().returning(move |request| {
            let tags = request.tags.clone().unwrap();
            creates.lock().unwrap().push(tags.clone());
            let mut function = created.lock().unwrap();
            if function.is_some() {
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "Function".to_string(),
                        resource_name: create_name.clone(),
                        message: format!("Function already exist: {create_name}"),
                    },
                ));
            }
            *function = Some(tags);
            if lose_responses > 0 {
                lose_responses -= 1;
                return Err(AlienError::new(CloudClientErrorData::HttpRequestFailed {
                    message: "connection closed before message completed".to_string(),
                }));
            }
            Ok(create_successful_function_response(&create_name))
        });
        let read_world = world.clone();
        let get_name = name.clone();
        lambda.expect_get_function().returning(move |_| match read {
            FunctionRead::Denied => {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "Function".to_string(),
                    resource_name: get_name.clone(),
                }))
            }
            FunctionRead::Allowed => match read_world.lock().unwrap().clone() {
                Some(tags) => Ok(GetFunctionResponse {
                    configuration: create_successful_function_response(&get_name),
                    tags: Some(tags),
                }),
                None => Err(not_found("Function")),
            },
        });
        lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&name)));
        lambda
    }

    async fn function_executor(
        controller: AwsWorkerController,
        lambda: MockLambdaApi,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(function_private_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(lambda), None, None))
            .with_test_dependencies()
            .build()
            .await
            .unwrap()
    }

    fn function_arn() -> String {
        format!(
            "arn:aws:lambda:us-east-1:123456789012:function:test-{}",
            function_private_ingress().id
        )
    }

    /// The token is saved by a step of its own, before the step that calls CreateFunction, and
    /// the create carries it.
    #[tokio::test]
    async fn function_create_token_is_saved_before_the_create_step() {
        let world = FunctionWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = function_executor(
            AwsWorkerController::default(),
            function_world_lambda(world, creates.clone(), 0, FunctionRead::Allowed),
        )
        .await;

        executor.step().await.expect("records the token");
        let saved = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(saved.state, AwsWorkerState::CreateStart);
        let token = saved.function_create_token.clone().expect("token saved");
        assert!(
            creates.lock().unwrap().is_empty(),
            "no create in the token step"
        );

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let creates = creates.lock().unwrap();
        assert_eq!(creates.len(), 1);
        assert_eq!(creates[0].get("CreateAttempt"), Some(&token));
    }

    /// Lambda creates the function but the response is lost. The retry's CreateFunction is
    /// answered ResourceConflictException, and the worker adopts the function its own create
    /// made instead of failing on every retry.
    #[tokio::test]
    async fn lost_function_create_response_is_adopted_by_its_create_token() {
        let world = FunctionWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = function_executor(
            AwsWorkerController::default(),
            function_world_lambda(world.clone(), creates.clone(), 1, FunctionRead::Allowed),
        )
        .await;

        executor.step().await.expect("records the token");
        executor.step().await.expect_err("the response is lost");
        assert!(world.lock().unwrap().is_some(), "Lambda made the function");
        executor
            .step()
            .await
            .expect("the retry adopts its own function");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreateWaitForActive);
        assert_eq!(state.arn.as_deref(), Some(function_arn().as_str()));

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let creates = creates.lock().unwrap();
        assert_eq!(creates.len(), 2);
        assert_eq!(creates[0], creates[1], "both creates carry the saved token");
    }

    /// Lambda creates the function but the process stops before the create step is saved. The
    /// controller restarts from the checkpoint saved before that step, which already holds the
    /// token, so the retry recognizes the function as its own.
    #[tokio::test]
    async fn function_created_before_a_crash_is_adopted_from_the_previous_checkpoint() {
        let world = FunctionWorld::default();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = function_executor(
            AwsWorkerController::default(),
            function_world_lambda(world.clone(), creates.clone(), 0, FunctionRead::Allowed),
        )
        .await;

        executor.step().await.expect("records the token");
        let checkpoint = executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .clone();
        executor.step().await.expect("Lambda accepts the create");
        assert!(world.lock().unwrap().is_some());
        drop(executor);

        let mut restarted = function_executor(
            checkpoint,
            function_world_lambda(world, creates.clone(), 0, FunctionRead::Allowed),
        )
        .await;
        restarted.run_until_terminal().await.unwrap();
        assert_eq!(restarted.status(), ResourceStatus::Running);
        let state = restarted.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.arn.as_deref(), Some(function_arn().as_str()));
        assert_eq!(creates.lock().unwrap().len(), 2);
    }

    /// A function with this worker's name that this create did not make stays an error: one
    /// left by another create attempt (same ownership tags, another token), one without our
    /// tags, and one the role is not allowed to read (reads are granted only on functions
    /// tagged for this stack).
    #[rstest]
    #[case::another_attempt(Some("another-attempt"), true, FunctionRead::Allowed)]
    #[case::untagged(None, false, FunctionRead::Allowed)]
    #[case::read_denied(None, false, FunctionRead::Denied)]
    #[tokio::test]
    async fn function_from_another_create_is_not_adopted(
        #[case] token: Option<&str>,
        #[case] ownership_tags: bool,
        #[case] read: FunctionRead,
    ) {
        let worker_id = function_private_ingress().id;
        let mut tags = if ownership_tags {
            standard_resource_tags("test", &worker_id)
        } else {
            HashMap::new()
        };
        if let Some(token) = token {
            tags.insert("CreateAttempt".to_string(), token.to_string());
        }
        let world: FunctionWorld = Arc::new(Mutex::new(Some(tags)));
        let creates = Arc::new(Mutex::new(Vec::new()));
        let mut executor = function_executor(
            AwsWorkerController::default(),
            function_world_lambda(world, creates, 0, read),
        )
        .await;

        executor.step().await.expect("records the token");
        let error = executor
            .step()
            .await
            .expect_err("the function is not this create's");
        assert!(
            error.message.contains("was not created by this worker"),
            "{error:?}"
        );
        assert!(
            error_chain_codes(&error).contains(&"REMOTE_RESOURCE_CONFLICT".to_string()),
            "{error:?}"
        );
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreateStart);
        assert!(state.arn.is_none());
    }

    // ─────────────── CERTIFICATE IMPORT TOKEN ────────────────

    /// ACM as the tests below see it: the imported certificates, by ARN, with their tags.
    type CertificateWorld = Arc<Mutex<Vec<(String, Vec<Tag>)>>>;

    fn imported_certificate_arn(n: usize) -> String {
        format!("arn:aws:acm:us-east-1:123456789012:certificate/imported-{n}")
    }

    /// The ACM calls a test made, in order.
    #[derive(Clone, Default)]
    struct AcmCalls {
        imports: Arc<Mutex<Vec<Vec<Tag>>>>,
        deletes: Calls,
    }

    /// An ACM client over `world`. ImportCertificate stores a new certificate; while
    /// `lose_responses` is above zero, an import ACM accepted answers 503 instead, as a lost
    /// response does. With `list_denied`, ListCertificates is denied, as for a role installed
    /// before it was granted.
    fn certificate_world_acm(
        world: CertificateWorld,
        calls: AcmCalls,
        lose_responses: usize,
        list_denied: bool,
    ) -> MockAcmApi {
        let mut acm = MockAcmApi::new();
        let stored = world.clone();
        let imports = calls.imports.clone();
        let mut lose_responses = lose_responses;
        acm.expect_import_certificate().returning(move |request| {
            let tags = request.tags.clone().unwrap_or_default();
            imports.lock().unwrap().push(tags.clone());
            let mut certificates = stored.lock().unwrap();
            let arn = imported_certificate_arn(certificates.len() + 1);
            certificates.push((arn.clone(), tags));
            if lose_responses > 0 {
                lose_responses -= 1;
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteServiceUnavailable {
                        message: "connection reset".to_string(),
                    },
                ));
            }
            Ok(ImportCertificateResponse {
                certificate_arn: arn,
            })
        });
        let listed = world.clone();
        acm.expect_list_certificates().returning(move |request| {
            if list_denied {
                return Err(access_denied());
            }
            assert_eq!(
                request
                    .includes
                    .and_then(|includes| includes.key_types)
                    .map(|types| types.len()),
                Some(7),
                "every key type is listed"
            );
            Ok(ListCertificatesResponse {
                certificate_summary_list: listed
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(arn, _)| CertificateSummary {
                        certificate_arn: Some(arn.clone()),
                        domain_name: Some(DOMAIN.to_string()),
                        status: Some("ISSUED".to_string()),
                        certificate_type: Some("IMPORTED".to_string()),
                        key_algorithm: Some("EC-prime256v1".to_string()),
                        in_use: Some(false),
                        imported_at: None,
                    })
                    .collect(),
                next_token: None,
            })
        });
        let tagged = world.clone();
        acm.expect_list_tags_for_certificate()
            .returning(move |arn| {
                tagged
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(stored, _)| stored == arn)
                    .map(|(_, tags)| tags.clone())
                    .ok_or_else(|| not_found("Certificate"))
            });
        let removed = world.clone();
        let deletes = calls.deletes.clone();
        acm.expect_delete_certificate().returning(move |arn| {
            record(&deletes, arn);
            let mut certificates = removed.lock().unwrap();
            let before = certificates.len();
            certificates.retain(|(stored, _)| stored != arn);
            if certificates.len() == before {
                return Err(not_found("Certificate"));
            }
            Ok(())
        });
        acm.expect_describe_certificate()
            .returning(|_| Err(not_found("Certificate")));
        acm
    }

    /// The worker at `ImportingCertificate`: the function exists and the domain metadata says
    /// the platform-issued certificate is ready.
    fn at_importing_certificate() -> AwsWorkerController {
        AwsWorkerController {
            state: AwsWorkerState::ImportingCertificate,
            arn: Some(
                "arn:aws:lambda:us-east-1:123456789012:function:test-public-func".to_string(),
            ),
            url: Some(format!("https://{DOMAIN}")),
            worker_name: Some("test-public-func".to_string()),
            fqdn: Some(DOMAIN.to_string()),
            certificate_id: Some("test-cert-id".to_string()),
            domain_name: Some(DOMAIN.to_string()),
            ..Default::default()
        }
    }

    async fn certificate_executor(
        controller: AwsWorkerController,
        acm: MockAcmApi,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(
                Arc::new(MockLambdaApi::new()),
                Some(Arc::new(acm)),
                None,
            ))
            .domain_metadata(create_test_domain_metadata("public-func"))
            .build()
            .await
            .unwrap()
    }

    fn import_token(tags: &[Tag]) -> Option<&str> {
        tags.iter()
            .find(|tag| tag.key == "CreateAttempt")
            .map(|tag| tag.value.as_str())
    }

    /// The token is saved by a step of its own, before the step that imports, and the import
    /// carries it next to the worker's ownership tags.
    #[tokio::test]
    async fn certificate_import_token_is_saved_before_the_import_step() {
        let world = CertificateWorld::default();
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            at_importing_certificate(),
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;

        executor.step().await.expect("records the token");
        let saved = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(saved.state, AwsWorkerState::ImportingCertificate);
        let token = saved.certificate_import_token.clone().expect("token saved");
        assert!(
            calls.imports.lock().unwrap().is_empty(),
            "no import in the token step"
        );

        executor.step().await.expect("imports the certificate");
        let imports = calls.imports.lock().unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(import_token(&imports[0]), Some(token.as_str()));
        for (key, value) in standard_resource_tags("test", "public-func") {
            assert!(
                imports[0].contains(&Tag {
                    key: key.clone(),
                    value
                }),
                "ownership tag {key} is on the import"
            );
        }
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiGateway);
        assert_eq!(
            state.certificate_arn.as_deref(),
            Some(imported_certificate_arn(1).as_str())
        );
        assert_eq!(
            state.certificate_issued_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
    }

    /// ACM imports the certificate but the process stops before the import step is saved. The
    /// controller restarts from the checkpoint saved before that step, which holds the token,
    /// and adopts the certificate instead of importing a second one.
    #[tokio::test]
    async fn certificate_imported_before_a_crash_is_adopted_from_the_previous_checkpoint() {
        let world = CertificateWorld::default();
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            at_importing_certificate(),
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;
        executor.step().await.expect("records the token");
        let checkpoint = executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .clone();
        executor.step().await.expect("ACM accepts the import");
        drop(executor);

        let mut restarted = certificate_executor(
            checkpoint,
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;
        restarted
            .step()
            .await
            .expect("the retry adopts its own certificate");
        let state = restarted.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiGateway);
        assert_eq!(
            state.certificate_arn.as_deref(),
            Some(imported_certificate_arn(1).as_str())
        );
        assert_eq!(calls.imports.lock().unwrap().len(), 1, "no second import");
        assert_eq!(world.lock().unwrap().len(), 1);
    }

    /// The import reaches ACM but its response is lost; the retry adopts the certificate.
    #[tokio::test]
    async fn lost_certificate_import_response_is_adopted_by_its_token() {
        let world = CertificateWorld::default();
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            at_importing_certificate(),
            certificate_world_acm(world.clone(), calls.clone(), 1, false),
        )
        .await;

        executor.step().await.expect("records the token");
        executor.step().await.expect_err("the response is lost");
        let failed = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(failed.certificate_arn, None);
        assert!(failed.certificate_import_token.is_some());
        executor
            .step()
            .await
            .expect("the retry adopts its own certificate");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::CreatingApiGateway);
        assert_eq!(
            state.certificate_arn.as_deref(),
            Some(imported_certificate_arn(1).as_str())
        );
        assert_eq!(calls.imports.lock().unwrap().len(), 1);
    }

    /// A leftover certificate with this worker's ownership tags but another import's token is
    /// not adopted.
    #[tokio::test]
    async fn certificate_from_another_import_is_not_adopted() {
        let mut leftover: Vec<Tag> = standard_resource_tags("test", "public-func")
            .into_iter()
            .map(|(key, value)| Tag { key, value })
            .collect();
        leftover.push(Tag {
            key: "CreateAttempt".to_string(),
            value: "another-import".to_string(),
        });
        let world: CertificateWorld =
            Arc::new(Mutex::new(vec![(imported_certificate_arn(1), leftover)]));
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            at_importing_certificate(),
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;

        executor.step().await.expect("records the token");
        executor.step().await.expect("imports its own certificate");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(
            state.certificate_arn.as_deref(),
            Some(imported_certificate_arn(2).as_str())
        );
        assert_eq!(calls.imports.lock().unwrap().len(), 1);
    }

    /// The import response is lost, then the worker is torn down: the delete finds the
    /// certificate by its token and deletes it, after the function.
    #[tokio::test]
    async fn teardown_after_a_lost_certificate_import_deletes_the_certificate() {
        let world = CertificateWorld::default();
        let acm_calls = AcmCalls::default();
        let calls = Calls::default();
        let mut lambda = MockLambdaApi::new();
        let recorded = calls.clone();
        lambda
            .expect_delete_function()
            .withf(|name, _| name == "test-public-func")
            .times(1)
            .returning(move |_, _| {
                record(&recorded, "DeleteFunction");
                Ok(())
            });
        lambda
            .expect_get_function_configuration()
            .returning(|_, _| Err(not_found("Function")));
        let mut apigw = MockApiGatewayV2Api::new();
        apigw
            .expect_get_domain_name()
            .returning(|_| Err(access_denied()));
        apigw.expect_delete_domain_name().times(0);
        let mut executor = SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(at_importing_certificate())
            .platform(Platform::Aws)
            .service_provider(provider(
                lambda,
                certificate_world_acm(world.clone(), acm_calls.clone(), 1, false),
                apigw,
            ))
            .domain_metadata(create_test_domain_metadata("public-func"))
            .build()
            .await
            .unwrap();

        executor.step().await.expect("records the token");
        executor.step().await.expect_err("the response is lost");
        assert_eq!(world.lock().unwrap().len(), 1, "ACM holds the certificate");

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*calls.lock().unwrap(), ["DeleteFunction"]);
        assert_eq!(
            *acm_calls.deletes.lock().unwrap(),
            [imported_certificate_arn(1)]
        );
        assert!(world.lock().unwrap().is_empty(), "no certificate is left");
    }

    /// The delete removes the recorded certificate and any other import under the same token
    /// (a second import after a lookup that missed the first), once each.
    #[tokio::test]
    async fn delete_removes_every_certificate_imported_under_the_token() {
        let tags = |token: &str| {
            vec![Tag {
                key: "CreateAttempt".to_string(),
                value: token.to_string(),
            }]
        };
        let world: CertificateWorld = Arc::new(Mutex::new(vec![
            (imported_certificate_arn(1), tags("token-1")),
            (imported_certificate_arn(2), tags("token-1")),
            (imported_certificate_arn(3), tags("another-worker")),
        ]));
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            AwsWorkerController {
                state: AwsWorkerState::DeletingCertificate,
                certificate_arn: Some(imported_certificate_arn(2)),
                certificate_import_token: Some("token-1".to_string()),
                ..Default::default()
            },
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;

        executor.step().await.expect("deletes the certificates");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::DeletingLogGroup);
        assert_eq!(state.certificate_arn, None);
        assert_eq!(state.certificate_import_token, None);
        assert_eq!(
            *calls.deletes.lock().unwrap(),
            [imported_certificate_arn(2), imported_certificate_arn(1)]
        );
        assert_eq!(
            world
                .lock()
                .unwrap()
                .iter()
                .map(|(arn, _)| arn.clone())
                .collect::<Vec<_>>(),
            [imported_certificate_arn(3)]
        );
    }

    /// A role installed before ListCertificates was granted: the import goes ahead (tagged with
    /// the token, so a later delete can find it once setup grants the list), and the delete
    /// still removes the recorded certificate.
    #[tokio::test]
    async fn denied_certificate_list_still_imports_and_deletes_the_recorded_certificate() {
        let world = CertificateWorld::default();
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            at_importing_certificate(),
            certificate_world_acm(world.clone(), calls.clone(), 0, true),
        )
        .await;
        executor.step().await.expect("records the token");
        executor
            .step()
            .await
            .expect("imports although the lookup is denied");
        let mut controller = executor
            .internal_state::<AwsWorkerController>()
            .unwrap()
            .clone();
        assert_eq!(
            controller.certificate_arn.as_deref(),
            Some(imported_certificate_arn(1).as_str())
        );

        controller.state = AwsWorkerState::DeletingCertificate;
        let mut deleting = certificate_executor(
            controller,
            certificate_world_acm(world.clone(), calls.clone(), 0, true),
        )
        .await;
        deleting
            .step()
            .await
            .expect("deletes the recorded certificate");
        assert_eq!(
            deleting
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            AwsWorkerState::DeletingLogGroup
        );
        assert!(world.lock().unwrap().is_empty());
    }

    /// A checkpoint saved by the previous version at `ImportingCertificate` has no token: the
    /// controller records one before importing.
    #[tokio::test]
    async fn import_checkpoint_from_the_previous_version_records_a_token_first() {
        let mut value = serialize_controller(&at_importing_certificate()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("certificateImportToken")
            .expect("the field is serialized");
        let restored = crate::core::deserialize_controller(value).unwrap();
        let restored = restored
            .as_any()
            .downcast_ref::<AwsWorkerController>()
            .unwrap()
            .clone();
        assert_eq!(restored.certificate_import_token, None);

        let world = CertificateWorld::default();
        let calls = AcmCalls::default();
        let mut executor = certificate_executor(
            restored,
            certificate_world_acm(world.clone(), calls.clone(), 0, false),
        )
        .await;
        executor.step().await.expect("records the token");
        assert!(calls.imports.lock().unwrap().is_empty());
        executor.step().await.expect("imports the certificate");
        assert_eq!(calls.imports.lock().unwrap().len(), 1);
        assert_eq!(
            executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            AwsWorkerState::CreatingApiGateway
        );
    }

    /// A renewed certificate is reimported into the same ARN without tags: ACM rejects tags on
    /// a reimport.
    #[tokio::test]
    async fn renewed_certificate_is_reimported_without_tags() {
        let mut acm = MockAcmApi::new();
        acm.expect_reimport_certificate()
            .withf(|request| {
                request.certificate_arn == imported_certificate_arn(1) && request.tags.is_none()
            })
            .times(1)
            .returning(|request| {
                Ok(ImportCertificateResponse {
                    certificate_arn: request.certificate_arn,
                })
            });
        let mut executor = certificate_executor(
            AwsWorkerController {
                state: AwsWorkerState::UpdateImportingCertificate,
                certificate_arn: Some(imported_certificate_arn(1)),
                certificate_issued_at: Some("2023-01-01T00:00:00Z".to_string()),
                ..Default::default()
            },
            acm,
        )
        .await;
        executor
            .step()
            .await
            .expect("reimports the renewed certificate");
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        assert_eq!(state.state, AwsWorkerState::UpdateCodeStart);
        assert_eq!(
            state.certificate_issued_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            AwsWorkerState::Ready
        );
    }

    /// Heartbeats must not replay the last deployment's code/config/trigger diff.
    #[rstest]
    #[case::never_updated(false, false, false)]
    #[case::previously_updated(true, false, false)]
    #[case::old_update_checkpoint(false, true, false)]
    #[case::pending_metadata(false, false, true)]
    #[tokio::test]
    async fn certificate_renewal_returns_to_ready_without_updating_worker(
        #[case] previously_updated: bool,
        #[case] old_checkpoint: bool,
        #[case] pending_metadata: bool,
    ) {
        let mut lambda = MockLambdaApi::new();
        lambda
            .expect_get_function_configuration()
            .returning(|name, _| Ok(create_successful_function_response(name)));
        // Every mutation is deliberately left without an expectation.
        let mut acm = MockAcmApi::new();
        acm.expect_reimport_certificate()
            .withf(|request| {
                request.certificate_arn == imported_certificate_arn(1)
                    && request.tags.is_none()
                    && request.certificate.contains("MIIBtest")
            })
            .times(if pending_metadata { 0 } else { 1 })
            .returning(|request| {
                Ok(ImportCertificateResponse {
                    certificate_arn: request.certificate_arn,
                })
            });
        let provider = setup_mock_service_provider(Arc::new(lambda), Some(Arc::new(acm)), None);
        let mut controller = AwsWorkerController {
            state: AwsWorkerState::Ready,
            certificate_arn: Some(imported_certificate_arn(1)),
            event_source_mappings: vec!["live-mapping-uuid".to_string()],
            certificate_issued_at: Some("2023-01-01T00:00:00Z".to_string()),
            ..at_importing_certificate()
        };
        if old_checkpoint {
            controller.state = AwsWorkerState::UpdateImportingCertificate;
        }
        let mut metadata = create_test_domain_metadata("public-func");
        if pending_metadata {
            let info = metadata.resources.get_mut("public-func").unwrap();
            info.issued_at = None;
            info.certificate_chain = None;
            info.private_key = None;
        }
        let mut completed = false;
        for _ in 0..10 {
            // Each step starts from the serialized checkpoint, as a fresh process would.
            let restored =
                crate::core::deserialize_controller(serialize_controller(&controller).unwrap())
                    .unwrap();
            let mut builder = SingleControllerExecutor::builder()
                .resource(function_public_ingress())
                .controller(
                    restored
                        .as_any()
                        .downcast_ref::<AwsWorkerController>()
                        .unwrap()
                        .clone(),
                )
                .platform(Platform::Aws)
                .service_provider(provider.clone())
                .domain_metadata(metadata.clone());
            if previously_updated {
                let mut previous = function_public_ingress();
                previous.code = alien_core::WorkerCode::Image {
                    image: "old-image:previous".to_string(),
                };
                previous.triggers = vec![alien_core::WorkerTrigger::Schedule {
                    cron: "0 * * * *".to_string(),
                }];
                builder = builder.previous_resource(previous);
            }
            let mut executor = builder.build().await.unwrap();
            let result = executor
                .step()
                .await
                .expect("certificate renewal must succeed");
            controller = executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .clone();
            assert!(matches!(
                executor.status(),
                ResourceStatus::Running | ResourceStatus::Updating
            ));
            if controller.state == AwsWorkerState::Ready {
                assert_eq!(controller.event_source_mappings, vec!["live-mapping-uuid"]);
                assert_eq!(
                    controller.certificate_issued_at.as_deref(),
                    Some(if pending_metadata {
                        "2023-01-01T00:00:00Z"
                    } else {
                        "2024-01-01T00:00:00Z"
                    })
                );
                assert!(result.suggested_delay.is_some_and(|delay| !delay.is_zero()));
                completed = true;
                break;
            }
        }
        assert!(completed, "renewal must finish at Ready");
    }

    #[rstest]
    #[case::retry(false, 1, false)]
    #[case::delete(true, 1, false)]
    #[case::exhausted_retry_budget(false, 12, true)]
    #[tokio::test]
    async fn certificate_renewal_lost_response_recovers_or_deletes(
        #[case] delete: bool,
        #[case] failures: usize,
        #[case] retryable: bool,
    ) {
        let installed = Arc::new(Mutex::new(Vec::new()));
        let writes = installed.clone();
        let mut acm = MockAcmApi::new();
        acm.expect_reimport_certificate()
            .times(if delete { failures } else { failures + 1 })
            .returning(move |request| {
                assert_eq!(request.certificate_arn, imported_certificate_arn(1));
                let mut writes = writes.lock().unwrap();
                writes.push(request.certificate.clone());
                if writes.len() <= failures {
                    // ACM committed the write, but the caller received an error. Use a
                    // non-retryable error to exercise the terminal refresh recovery path.
                    let mut error = access_denied();
                    error.retryable = retryable;
                    return Err(error);
                }
                Ok(ImportCertificateResponse {
                    certificate_arn: request.certificate_arn,
                })
            });
        let deleted_certificate = Arc::new(Mutex::new(false));
        if delete {
            let deleted = deleted_certificate.clone();
            acm.expect_delete_certificate()
                .times(1)
                .returning(move |arn| {
                    assert_eq!(arn, imported_certificate_arn(1));
                    *deleted.lock().unwrap() = true;
                    Ok(())
                });
        }
        let deleted_function = Arc::new(Mutex::new(false));
        let read_deleted = deleted_function.clone();
        let mut lambda = MockLambdaApi::new();
        lambda
            .expect_get_function_configuration()
            .returning(move |name, _| {
                if *read_deleted.lock().unwrap() {
                    Err(not_found("Function"))
                } else {
                    Ok(create_successful_function_response(name))
                }
            });
        if delete {
            lambda
                .expect_delete_function()
                .times(1)
                .returning(move |name, _| {
                    assert_eq!(name, "test-public-func");
                    *deleted_function.lock().unwrap() = true;
                    Ok(())
                });
        }
        let mut logs = MockCloudWatchLogsApi::new();
        if delete {
            logs.expect_delete_log_group()
                .times(1)
                .returning(|_| Ok(()));
        }
        let provider = setup_mock_service_provider_with_logs(
            Arc::new(lambda),
            Some(Arc::new(acm)),
            None,
            Arc::new(logs),
        );
        let single = SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(AwsWorkerController {
                state: AwsWorkerState::Ready,
                arn: Some(
                    "arn:aws:lambda:us-east-1:123456789012:function:test-public-func".to_string(),
                ),
                certificate_arn: Some(imported_certificate_arn(1)),
                certificate_issued_at: Some("2023-01-01T00:00:00Z".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(provider.clone())
            .domain_metadata(create_test_domain_metadata("public-func"))
            .build()
            .await
            .unwrap();
        let (mut stack, client, config) = single.with_context(|ctx| {
            (
                ctx.desired_stack.clone(),
                ctx.client_config.clone(),
                ctx.deployment_config.clone(),
            )
        });
        let mut state = single.stack_state().clone();
        for (id, entry) in &mut stack.resources {
            if id != "public-func" {
                entry.lifecycle = ResourceLifecycle::Frozen;
            }
        }
        for (id, entry) in &mut state.resources {
            if id != "public-func" {
                entry.lifecycle = Some(ResourceLifecycle::Frozen);
            }
        }
        let executor = StackExecutor::builder(&stack, client)
            .deployment_config(&config)
            .service_provider(provider.clone())
            .lifecycle_filter(vec![ResourceLifecycle::Live])
            .step_out_of_scope_resources(false)
            .build()
            .unwrap();
        for attempt in 1..=failures {
            let result = executor.refresh(state).await.unwrap();
            assert_eq!(
                result.heartbeats.len(),
                1,
                "failed renewal must preserve health telemetry"
            );
            assert!(result.suggested_delay_ms.is_some_and(|delay| delay > 0));
            state =
                serde_json::from_slice(&serde_json::to_vec(&result.next_state).unwrap()).unwrap();
            let resource = &state.resources["public-func"];
            assert!(resource
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("certificate"));
            if !retryable || attempt >= 10 {
                assert_eq!(resource.status, ResourceStatus::RefreshFailed);
                assert!(result.suggested_delay_ms.unwrap() >= 30_000);
            }
            let restored =
                crate::core::deserialize_controller(resource.internal_state.clone().unwrap())
                    .unwrap();
            assert_eq!(
                restored
                    .as_any()
                    .downcast_ref::<AwsWorkerController>()
                    .unwrap()
                    .certificate_issued_at
                    .as_deref(),
                Some("2023-01-01T00:00:00Z")
            );
        }
        let restored = crate::core::deserialize_controller(
            state.resources["public-func"]
                .internal_state
                .clone()
                .unwrap(),
        )
        .unwrap();
        if delete {
            let mut deleting = SingleControllerExecutor::builder()
                .resource(function_public_ingress())
                .controller(
                    restored
                        .as_any()
                        .downcast_ref::<AwsWorkerController>()
                        .unwrap()
                        .clone(),
                )
                .platform(Platform::Aws)
                .service_provider(provider)
                .build()
                .await
                .unwrap();
            deleting.delete().unwrap();
            deleting.run_until_terminal().await.unwrap();
            assert_eq!(deleting.status(), ResourceStatus::Deleted);
            assert!(*deleted_certificate.lock().unwrap());
        } else {
            let recovered = executor.refresh(state).await.unwrap();
            let resource = &recovered.next_state.resources["public-func"];
            assert_eq!(resource.status, ResourceStatus::Running);
            assert!(resource.error.is_none());
            assert!(recovered.suggested_delay_ms.is_some_and(|delay| delay > 0));
            let restored =
                crate::core::deserialize_controller(resource.internal_state.clone().unwrap())
                    .unwrap();
            assert_eq!(
                restored
                    .as_any()
                    .downcast_ref::<AwsWorkerController>()
                    .unwrap()
                    .certificate_issued_at
                    .as_deref(),
                Some("2024-01-01T00:00:00Z")
            );
            let writes = installed.lock().unwrap();
            assert_eq!(writes.len(), failures + 1);
            assert_eq!(writes[0], writes[1]);
        }
    }

    // ─────────────── LOG GROUP ────────────────

    /// The delete step for the function's log group, against `logs`.
    async fn log_group_executor(logs: MockCloudWatchLogsApi) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(function_public_ingress())
            .controller(AwsWorkerController {
                state: AwsWorkerState::DeletingCertificate,
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider_with_logs(
                Arc::new(MockLambdaApi::new()),
                None,
                None,
                Arc::new(logs),
            ))
            .build()
            .await
            .unwrap()
    }

    /// Teardown ends by deleting the log group `ensure_log_group` created, by its exact name.
    #[tokio::test]
    async fn teardown_deletes_the_function_log_group() {
        let mut logs = MockCloudWatchLogsApi::new();
        logs.expect_delete_log_group()
            .withf(|name| name == "/aws/lambda/test-public-func")
            .times(1)
            .returning(|_| Ok(()));
        let mut executor = log_group_executor(logs).await;

        executor
            .step()
            .await
            .expect("the certificate step continues");
        assert_eq!(
            executor
                .internal_state::<AwsWorkerController>()
                .unwrap()
                .state,
            AwsWorkerState::DeletingLogGroup
        );
        executor.run_until_terminal().await.expect("teardown runs");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// A log group that is already gone, or that the role may not delete, does not stop the
    /// delete; any other error is returned and the step is retried.
    #[rstest]
    #[case::gone(not_found("LogGroup"), true)]
    #[case::denied(access_denied(), true)]
    #[case::unavailable(
        AlienError::new(CloudClientErrorData::RemoteServiceUnavailable {
            message: "unavailable".to_string(),
        }),
        false
    )]
    #[tokio::test]
    async fn log_group_delete_outcomes(
        #[case] error: AlienError<CloudClientErrorData>,
        #[case] finishes: bool,
    ) {
        let mut logs = MockCloudWatchLogsApi::new();
        logs.expect_delete_log_group()
            .times(1)
            .returning(move |_| Err(error.clone()));
        let mut executor = log_group_executor(logs).await;
        executor
            .step()
            .await
            .expect("the certificate step continues");

        let result = executor.step().await;
        let state = executor.internal_state::<AwsWorkerController>().unwrap();
        if finishes {
            result.expect("the delete finishes");
            assert_eq!(state.state, AwsWorkerState::Deleted);
        } else {
            result.expect_err("the error is returned");
            assert_eq!(state.state, AwsWorkerState::DeletingLogGroup);
        }
    }

    /// A linked sandbox can publish a new image version after this worker rendered its binding
    /// (a rebuild that becomes active later, a re-registration). The worker must pick it up
    /// without its own configuration changing, and must not touch Lambda when nothing moved.
    mod linked_binding_refresh {
        use super::*;
        use crate::sandbox::{AwsSandboxController, AwsSandboxState};
        use alien_core::{ResourceRef, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

        const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-sb";

        fn sandbox() -> Sandbox {
            Sandbox::new("sb".to_string())
                .code(SandboxCode::Image {
                    image: "registry.example.com/sb:1".to_string(),
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(1800),
                    idle_pause_seconds: None,
                })
                .build()
        }

        fn sandbox_on(version: &str, state: AwsSandboxState) -> AwsSandboxController {
            AwsSandboxController {
                state,
                image_identifier: Some(IMAGE_ARN.to_string()),
                image_arn: Some(IMAGE_ARN.to_string()),
                active_version: Some(version.to_string()),
                region: Some("us-east-1".to_string()),
                ..Default::default()
            }
        }

        fn linked_worker() -> Worker {
            let mut worker = basic_function();
            worker.links = vec![ResourceRef::new(Sandbox::RESOURCE_TYPE, "sb")];
            worker
        }

        /// The sandbox binding variable as a worker renders it for `version`.
        fn binding_variable(version: &str) -> (String, String) {
            let params = sandbox_on(version, AwsSandboxState::Ready)
                .get_binding_params()
                .unwrap()
                .expect("a ready sandbox publishes a binding");
            alien_core::bindings::serialize_binding_as_env_var("sb", &params)
                .unwrap()
                .into_iter()
                .next()
                .unwrap()
        }

        async fn executor(
            lambda: MockLambdaApi,
            controller: AwsWorkerController,
            sandbox_controller: AwsSandboxController,
            previous: Option<Worker>,
        ) -> SingleControllerExecutor {
            super::FORCED_BINDING_CHECK.with(|forced| forced.set(true));
            let provider = setup_mock_service_provider(Arc::new(lambda), None, None);
            let mut builder = SingleControllerExecutor::builder()
                .resource(linked_worker())
                .controller(controller)
                .platform(Platform::Aws)
                .service_provider(provider)
                .with_test_dependencies()
                .with_dependency(
                    Network::new("default-network".into())
                        .settings(NetworkSettings::UseDefault)
                        .build(),
                    AwsNetworkController::mock_ready("vpc-test", 0),
                )
                .with_dependency(sandbox(), sandbox_controller);
            if let Some(previous) = previous {
                builder = builder.previous_resource(previous);
            }
            builder.build().await.unwrap()
        }

        fn active(lambda: &mut MockLambdaApi) {
            lambda
                .expect_get_function_configuration()
                .returning(|name, _| Ok(create_successful_function_response(name)));
        }

        /// The full environment the worker applies when the sandbox serves `version`, as Lambda
        /// would then hold it.
        async fn rendered_environment(version: &str) -> HashMap<String, String> {
            let captured = Arc::new(std::sync::Mutex::new(None));
            let sink = captured.clone();
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            lambda
                .expect_get_function_environment()
                .returning(|_| Ok(HashMap::new()));
            lambda
                .expect_update_function_configuration()
                .returning(move |name, request| {
                    *sink.lock().unwrap() =
                        request.environment.and_then(|environment| environment.variables);
                    Ok(create_successful_function_response(name))
                });
            let mut executor = executor(
                lambda,
                AwsWorkerController::mock_ready("basic-func"),
                sandbox_on(version, AwsSandboxState::Ready),
                None,
            )
            .await;
            executor.step().await.unwrap();
            executor.step().await.unwrap();
            let rendered = captured.lock().unwrap().clone();
            rendered.expect("the worker applies its environment")
        }

        /// The worker as the stack mutations prepare it, with its previous release's code, so an
        /// update sees only a code change.
        async fn previous_release() -> Worker {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            let mut executor = executor(
                lambda,
                AwsWorkerController::mock_ready("basic-func"),
                sandbox_on("1.0", AwsSandboxState::Ready),
                None,
            )
            .await;
            let mut previous = executor
                .desired_resource_config_mut("basic-func")
                .and_then(|config| config.downcast_ref::<Worker>().cloned())
                .expect("the prepared worker");
            previous.code = alien_core::WorkerCode::Image {
                image: "registry.example.com/worker:old".into(),
            };
            previous
        }

        fn carries(variables: Option<&HashMap<String, String>>, version: &str) -> bool {
            let (name, value) = binding_variable(version);
            variables
                .and_then(|variables| variables.get(&name))
                .is_some_and(|live| {
                    serde_json::from_str::<serde_json::Value>(live).unwrap()
                        == serde_json::from_str::<serde_json::Value>(&value).unwrap()
                })
        }

        /// The staging case: the sandbox moved from 1.0 to 2.0 and the worker was never updated.
        /// The heartbeat sees the difference and applies the current binding, once.
        #[tokio::test]
        async fn a_heartbeat_applies_a_linked_binding_that_moved() {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            let live = rendered_environment("1.0").await;
            assert!(carries(Some(&live), "1.0"));
            lambda
                .expect_get_function_environment()
                .times(1)
                .returning(move |_| Ok(live.clone()));
            lambda
                .expect_update_function_configuration()
                .times(1)
                .withf(|_, request| {
                    carries(
                        request
                            .environment
                            .as_ref()
                            .and_then(|environment| environment.variables.as_ref()),
                        "2.0",
                    ) && request.memory_size.is_none()
                        && request.role.is_none()
                })
                .returning(|name, _| Ok(create_successful_function_response(name)));
            let worker = AwsWorkerController::mock_ready("basic-func");
            let own_binding = worker.get_binding_params().unwrap();
            let mut executor = executor(
                lambda,
                worker,
                sandbox_on("2.0", AwsSandboxState::Ready),
                None,
            )
            .await;

            executor.step().await.expect("the heartbeat runs");
            assert_eq!(
                executor.internal_state::<AwsWorkerController>().unwrap().state,
                AwsWorkerState::RefreshingLinkedBindings
            );
            assert_eq!(
                executor.status(),
                ResourceStatus::Updating,
                "the planner must not start an update mid-refresh: updates begin only from Ready"
            );
            executor.step().await.expect("the refresh applies the binding");
            executor.step().await.expect("the refresh settles");
            let controller = executor.internal_state::<AwsWorkerController>().unwrap();
            assert_eq!(controller.state, AwsWorkerState::Ready);
            assert_eq!(
                controller.get_binding_params().unwrap(),
                own_binding,
                "a refresh must not change the worker's own binding, or its dependents would follow"
            );
        }

        /// No fleet churn: a live environment that already matches, even with the JSON keys in
        /// another order, makes no write.
        #[tokio::test]
        async fn a_heartbeat_leaves_matching_bindings_alone() {
            let current = rendered_environment("2.0").await;
            let (name, _) = binding_variable("2.0");
            let mut reformatted = current.clone();
            let pretty = serde_json::to_string_pretty(
                &serde_json::from_str::<serde_json::Value>(&current[&name]).unwrap(),
            )
            .unwrap();
            reformatted.insert(name.clone(), pretty);
            for live in [current, reformatted] {
                let mut lambda = MockLambdaApi::new();
                active(&mut lambda);
                lambda
                    .expect_get_function_environment()
                    .times(1)
                    .returning(move |_| Ok(live.clone()));
                lambda.expect_update_function_configuration().times(0);
                let mut executor = executor(
                    lambda,
                    AwsWorkerController::mock_ready("basic-func"),
                    sandbox_on("2.0", AwsSandboxState::Ready),
                    None,
                )
                .await;
                executor.step().await.expect("the heartbeat runs");
                assert_eq!(
                    executor.internal_state::<AwsWorkerController>().unwrap().state,
                    AwsWorkerState::Ready
                );
            }
        }

        /// Every worker compares once in any eleven consecutive heartbeats, at the 30-second
        /// heartbeat, the manager's 60-second tick and a 90-second one alike, so a heartbeat that
        /// keeps landing in one half-minute still reaches its slot.
        #[test]
        fn each_worker_compares_once_in_eleven_heartbeats_at_any_interval() {
            let start = chrono::DateTime::from_timestamp(1_800_000_007, 0).unwrap();
            for interval in [30, 60, 90] {
                for name in ["test-a", "test-b", "stack-worker", "w"] {
                    for shift in [0, 11, 29] {
                        let due = (0..super::super::LINKED_BINDING_CHECK_SLOTS)
                            .filter(|beat| {
                                super::super::binding_window_open(
                                    name,
                                    start + chrono::Duration::seconds(shift + beat * interval),
                                )
                            })
                            .count();
                        assert_eq!(due, 1, "{name} every {interval}s, shifted {shift}s");
                    }
                }
            }
        }

        /// A throttled read skips the comparison: the heartbeat still succeeds, the worker stays
        /// Ready and healthy, and nothing is written.
        #[tokio::test]
        async fn a_throttled_comparison_is_skipped_without_failing_the_heartbeat() {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            lambda.expect_get_function_environment().times(1).returning(|_| {
                Err(AlienError::new(CloudClientErrorData::RateLimitExceeded {
                    message: "Rate exceeded".to_string(),
                }))
            });
            lambda.expect_update_function_configuration().times(0);
            let mut executor = executor(
                lambda,
                AwsWorkerController::mock_ready("basic-func"),
                sandbox_on("2.0", AwsSandboxState::Ready),
                None,
            )
            .await;
            executor.step().await.expect("a throttled comparison does not fail the heartbeat");
            assert_eq!(
                executor.internal_state::<AwsWorkerController>().unwrap().state,
                AwsWorkerState::Ready
            );
            assert_eq!(executor.status(), ResourceStatus::Running);
        }

        /// No new write per heartbeat: a heartbeat that finds no drift leaves the controller's
        /// state exactly as it was, so the deployment loop has nothing new to persist.
        #[tokio::test]
        async fn a_heartbeat_without_drift_changes_no_state() {
            let current = rendered_environment("2.0").await;
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            lambda
                .expect_get_function_environment()
                .times(1)
                .returning(move |_| Ok(current.clone()));
            lambda.expect_update_function_configuration().times(0);
            let before = AwsWorkerController::mock_ready("basic-func");
            let mut executor = executor(
                lambda,
                before.clone(),
                sandbox_on("2.0", AwsSandboxState::Ready),
                None,
            )
            .await;
            executor.step().await.expect("the heartbeat runs");
            assert_eq!(
                serialize_controller(executor.internal_state::<AwsWorkerController>().unwrap())
                    .unwrap(),
                serialize_controller(&before).unwrap()
            );
        }

        /// Ordering: while the sandbox is in its own update it has not reached the value its
        /// dependents should see, so the worker neither compares nor writes.
        #[tokio::test]
        async fn a_heartbeat_waits_for_a_linked_resource_in_its_own_flow() {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            lambda.expect_get_function_environment().times(0);
            lambda.expect_update_function_configuration().times(0);
            let mut executor = executor(
                lambda,
                AwsWorkerController::mock_ready("basic-func"),
                sandbox_on("1.0", AwsSandboxState::WaitingForRolledImageActive),
                None,
            )
            .await;
            executor.step().await.expect("the heartbeat runs");
            assert_eq!(
                executor.internal_state::<AwsWorkerController>().unwrap().state,
                AwsWorkerState::Ready
            );
        }

        /// The other staging case: the worker's code changed in the same release, so it was
        /// updated, but its own configuration did not change and the update skipped the
        /// environment. It must re-render the binding the sandbox publishes now.
        #[tokio::test]
        async fn a_code_only_update_applies_a_linked_binding_that_moved() {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            let live = rendered_environment("1.0").await;
            lambda
                .expect_get_function_environment()
                .times(1)
                .returning(move |_| Ok(live.clone()));
            lambda
                .expect_update_function_configuration()
                .times(1)
                .withf(|_, request| {
                    carries(
                        request
                            .environment
                            .as_ref()
                            .and_then(|environment| environment.variables.as_ref()),
                        "2.0",
                    )
                })
                .returning(|name, _| Ok(create_successful_function_response(name)));
            let previous = previous_release().await;
            let controller = AwsWorkerController {
                state: AwsWorkerState::UpdateConfigStart,
                ..AwsWorkerController::mock_ready("basic-func")
            };
            let mut executor = executor(
                lambda,
                controller,
                sandbox_on("2.0", AwsSandboxState::Ready),
                Some(previous),
            )
            .await;
            executor.step().await.expect("the update re-renders the binding");
            assert_eq!(
                executor.internal_state::<AwsWorkerController>().unwrap().state,
                AwsWorkerState::UpdateConfigWaitForActive
            );
        }

        /// A code-only update whose bindings did not move still makes no configuration write.
        #[tokio::test]
        async fn a_code_only_update_with_current_bindings_writes_nothing() {
            let mut lambda = MockLambdaApi::new();
            active(&mut lambda);
            let live = rendered_environment("2.0").await;
            lambda
                .expect_get_function_environment()
                .times(1)
                .returning(move |_| Ok(live.clone()));
            lambda.expect_update_function_configuration().times(0);
            let previous = previous_release().await;
            let mut executor = executor(
                lambda,
                AwsWorkerController {
                    state: AwsWorkerState::UpdateConfigStart,
                    ..AwsWorkerController::mock_ready("basic-func")
                },
                sandbox_on("2.0", AwsSandboxState::Ready),
                Some(previous),
            )
            .await;
            executor.step().await.expect("the update runs");
            assert_eq!(
                executor.internal_state::<AwsWorkerController>().unwrap().state,
                AwsWorkerState::UpdateConfigWaitForActive
            );
        }
    }

}
