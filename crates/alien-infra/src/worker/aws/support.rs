use serde::{Deserialize, Serialize};

use crate::core::ResourceControllerContext;
use crate::worker::readiness_probe::ReadinessProbeDnsOverride;
use alien_aws_clients::eventbridge::EventBridgeTag;
use alien_aws_clients::lambda::FunctionConfiguration;
use alien_aws_clients::s3::{LambdaFunctionConfiguration, NotificationConfiguration};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsLambdaWorkerHeartbeatData, HeartbeatBackend, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, Worker,
    WorkerHeartbeatData, WorkloadHeartbeatStatus,
};
use alien_error::AlienError;
use chrono::Utc;

/// Generates the full, prefixed AWS resource name.
pub(super) fn get_aws_worker_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

pub(super) fn readiness_probe_dns_override(
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

pub(super) fn eventbridge_tags(prefix: &str, resource_id: &str) -> Vec<EventBridgeTag> {
    standard_resource_tags(prefix, resource_id)
        .into_iter()
        .map(|(key, value)| EventBridgeTag { key, value })
        .collect()
}

pub(super) fn is_remote_resource_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

pub(super) fn replace_lambda_notification_config(
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

pub(super) struct DomainInfo {
    pub(super) fqdn: String,
    pub(super) certificate_id: Option<String>,
    pub(super) certificate_arn: Option<String>,
    pub(super) uses_custom_domain: bool,
}

pub(super) fn emit_aws_lambda_worker_heartbeat(
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
