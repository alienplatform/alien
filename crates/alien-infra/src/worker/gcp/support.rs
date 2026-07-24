use std::collections::HashSet;
use std::time::Duration;

use crate::core::ResourceControllerContext;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    GcpCloudRunWorkerHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, Worker, WorkerHeartbeatData,
    WorkloadHeartbeatStatus,
};
use alien_error::{AlienError, GenericError};
use alien_gcp_clients::cloudrun::Service;
use alien_gcp_clients::gcs::GcsNotification;
use chrono::Utc;
use sha2::{Digest, Sha256};

pub(super) const CLOUD_RUN_SERVICE_NAME_MAX_LEN: usize = 49;
pub(super) const GCP_RESOURCE_NAME_MAX_LEN: usize = 63;
pub(super) const GCP_RESOURCE_NAME_HASH_LEN: usize = 8;
pub(super) const MAX_IMAGE_PULL_PERMISSION_RETRIES: u8 = 4;

pub(super) fn is_cross_project_image_pull_permission_error(message: &str) -> bool {
    message.contains("serverless-robot-prod.iam.gserviceaccount.com")
        && message.contains("artifactregistry.repositories.downloadArtifacts")
}

pub(super) fn image_pull_permission_retry_delay(attempt: u8) -> Duration {
    Duration::from_secs(10 * (1_u64 << attempt.saturating_sub(1).min(3)))
}

pub(super) fn is_remote_resource_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

pub(super) fn error_chain_contains_resource_in_use(
    code: &str,
    message: &str,
    source: Option<&AlienError<GenericError>>,
) -> bool {
    (code == "INVALID_INPUT" || code == "HTTP_RESPONSE_ERROR")
        && (message.contains("being used by") || message.contains("resourceInUseByAnotherResource"))
        || source.is_some_and(|source| {
            error_chain_contains_resource_in_use(
                &source.code,
                &source.message,
                source.source.as_deref(),
            )
        })
}

pub(super) fn is_gcp_resource_in_use(error: &AlienError<CloudClientErrorData>) -> bool {
    error_chain_contains_resource_in_use(&error.code, &error.message, error.source.as_deref())
}

pub(super) fn same_unordered_strings(left: &[String], right: &[String]) -> bool {
    left.iter().collect::<HashSet<_>>() == right.iter().collect::<HashSet<_>>()
}

pub(super) fn gcs_notification_matches_existing(
    existing: &GcsNotification,
    desired: &GcsNotification,
) -> bool {
    existing.topic == desired.topic
        && same_unordered_strings(&existing.event_types, &desired.event_types)
        && existing.payload_format == desired.payload_format
        && existing.object_name_prefix == desired.object_name_prefix
        && existing.custom_attributes == desired.custom_attributes
}

/// Generates the Cloud Run service name from stack prefix and worker ID
pub(super) fn get_cloudrun_service_name(prefix: &str, name: &str) -> String {
    let raw = format!("{}-{}", prefix, name);
    let sanitized = sanitize_gcp_resource_name(&raw);

    if sanitized == raw && sanitized.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN {
        return sanitized;
    }

    stable_hashed_gcp_resource_name(&raw, &sanitized, CLOUD_RUN_SERVICE_NAME_MAX_LEN)
}

pub(super) fn get_gcp_worker_resource_name(prefix: &str, worker_id: &str, suffix: &str) -> String {
    let raw = format!("{prefix}-{worker_id}-{suffix}");
    let sanitized = sanitize_gcp_resource_name(&raw);

    if sanitized == raw && sanitized.len() <= GCP_RESOURCE_NAME_MAX_LEN {
        return sanitized;
    }

    stable_hashed_gcp_resource_name(&raw, &sanitized, GCP_RESOURCE_NAME_MAX_LEN)
}

pub(super) fn sanitize_gcp_resource_name(raw: &str) -> String {
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

pub(super) fn stable_hashed_gcp_resource_name(
    raw: &str,
    sanitized: &str,
    max_len: usize,
) -> String {
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

pub(super) fn stable_name_hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest
        .iter()
        .take(GCP_RESOURCE_NAME_HASH_LEN / 2)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Domain information for a worker.
pub(super) struct DomainInfo {
    pub(super) fqdn: String,
    pub(super) certificate_id: Option<String>,
    pub(super) ssl_certificate_name: Option<String>,
    pub(super) uses_custom_domain: bool,
}

pub(super) fn emit_gcp_cloud_run_worker_heartbeat(
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
            .and_then(|resources| resources.limits.as_ref())
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
                uri: service.uri.clone(),
                urls: service.urls.clone(),
                latest_created_revision: service.latest_created_revision.clone(),
                latest_ready_revision: service.latest_ready_revision.clone(),
                generation: service
                    .generation
                    .as_deref()
                    .and_then(|generation| generation.parse::<i64>().ok()),
                observed_generation: service
                    .observed_generation
                    .as_deref()
                    .and_then(|generation| generation.parse::<i64>().ok()),
                traffic_count: service.traffic.len() as u32,
                min_instance_count: scaling.and_then(|scaling| scaling.min_instance_count),
                max_instance_count: scaling.and_then(|scaling| scaling.max_instance_count),
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
