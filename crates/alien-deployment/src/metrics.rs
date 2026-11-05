//! Metric collection under the deployment lease, with export-before-checkpoint ordering.
use std::collections::BTreeSet;
use std::time::Duration;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, MetricResource, MetricsCollectionCheckpoint,
    MetricsSource, PermissionSetReference,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_observer::metrics::{self, CollectionWindow};
use chrono::{TimeZone, Utc};
use sha2::{Digest, Sha256};

use crate::{transport::DeploymentLoopTransport, ErrorData, Result};

pub async fn collect_and_checkpoint(
    state: &mut DeploymentState,
    config: &DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    transport: &dyn DeploymentLoopTransport,
) -> Result<()> {
    let Some(collection) = &config.stack_settings.metrics_collection else {
        return Ok(());
    };
    if collection.jobs.len() > 100
        || collection.interval_seconds < 60
        || collection.interval_seconds > 86400
        || collection.source_lag_seconds > 3600
    {
        return Err(AlienError::new(ErrorData::DeploymentError {
            message: "Invalid metrics collection bounds".into(),
        }));
    }
    let mut ids = BTreeSet::new();
    if collection
        .jobs
        .iter()
        .any(|job| job.id.is_empty() || job.id.len() > 128 || !ids.insert(&job.id))
    {
        return Err(AlienError::new(ErrorData::DeploymentError {
            message: "Metric jobs require distinct non-empty IDs".into(),
        }));
    }
    let mut candidate = state.clone();
    let metadata = candidate
        .runtime_metadata
        .get_or_insert_with(Default::default);
    metadata
        .metrics_checkpoints
        .retain(|id, _| collection.jobs.iter().any(|job| &job.id == id));
    metadata
        .metrics_collection_errors
        .retain(|id, _| collection.jobs.iter().any(|job| &job.id == id));
    let mut changed = metadata.metrics_checkpoints.len()
        != state
            .runtime_metadata
            .as_ref()
            .map_or(0, |m| m.metrics_checkpoints.len());
    let now = Utc::now();
    for job in &collection.jobs {
        if metadata
            .metrics_collection_attempts
            .get(&job.id)
            .is_some_and(|last| {
                now.signed_duration_since(*last).num_seconds()
                    < i64::from(collection.interval_seconds)
            })
        {
            continue;
        }

        let source_json = serde_json::to_vec(job)
            .into_alien_error()
            .context(failure("Could not encode metric source"))?;
        let source_hash = hex::encode(Sha256::digest(source_json));
        // Different jobs have different native resolution: a daily capacity
        // measurement must not become a sequence of partially queried minutes.
        let source_period = match &job.source {
            MetricsSource::Aws(sources) => sources
                .iter()
                .map(|source| i64::from(source.period_seconds))
                .max()
                .unwrap_or(0),
            MetricsSource::Azure(source) => i64::from(source.interval_seconds),
            MetricsSource::Gcp(_) | MetricsSource::Scrape(_) => 0,
        };
        let interval = i64::from(collection.interval_seconds).max(source_period);
        let end_seconds =
            (now.timestamp() - i64::from(collection.source_lag_seconds)) / interval * interval;
        let end = Utc
            .timestamp_opt(end_seconds, 0)
            .single()
            .ok_or_else(|| AlienError::new(failure("Invalid metric window")))?;
        let previous = metadata
            .metrics_checkpoints
            .get(&job.id)
            .filter(|checkpoint| checkpoint.source_hash == source_hash);
        let start = previous
            .map(|checkpoint| checkpoint.collected_through)
            .unwrap_or(end - chrono::Duration::seconds(interval));
        if start >= end {
            continue;
        }
        // Catch up in bounded chunks. Do not discard a backlog after a collector outage.
        let end = end.min(start + chrono::Duration::days(1));
        let window = CollectionWindow { start, end };
        let result = async {
            let cloud = !matches!(job.source, MetricsSource::Scrape(_));
            if cloud && !has_metrics_grant(state) {
                return Err(AlienError::new(failure(
                    "Setup update required: explicitly grant observe/metrics-read",
                )));
            }
            if cloud && collection.source_lag_seconds < 60 {
                return Err(AlienError::new(failure(
                    "Cloud collection requires at least 60 seconds source lag",
                )));
            }
            let (batch, provider, region) =
                collect_source(&job.source, &window, client_config).await?;
            let data = batch.encode_otlp(&MetricResource {
                deployment_id: deployment_id.into(),
                resource_id: job.resource_id.clone(),
                provider,
                region,
            });
            transport
                .export_metrics(deployment_id, config, data)
                .await
                .context(failure("OTLP metrics export was not acknowledged"))?;
            Ok::<_, AlienError<ErrorData>>(())
        }
        .await;
        changed = true;
        metadata
            .metrics_collection_attempts
            .insert(job.id.clone(), now);
        match result {
            Ok(()) => {
                metadata.metrics_checkpoints.insert(
                    job.id.clone(),
                    MetricsCollectionCheckpoint {
                        source_hash,
                        collected_through: end,
                    },
                );
                metadata.metrics_collection_errors.remove(&job.id);
            }
            Err(error) => {
                metadata
                    .metrics_collection_errors
                    .insert(job.id.clone(), format!("{}: {}", error.code, error.message));
            }
        }
    }
    if changed {
        let reconciled = transport
            .reconcile_step(
                deployment_id,
                &candidate,
                config,
                false,
                None,
                Vec::new(),
                Vec::new(),
            )
            .await
            .context(failure("Metric checkpoint persistence failed"))?;
        *state = reconciled.state.unwrap_or(candidate);
    }
    Ok(())
}

fn has_metrics_grant(state: &DeploymentState) -> bool {
    let stack = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.prepared_stack.as_ref())
        .or_else(|| state.current_release.as_ref().map(|release| &release.stack));
    stack.and_then(|stack| stack.permissions.management.profile()).and_then(|profile| profile.0.get("*")).is_some_and(|sets| sets.iter().any(|set| matches!(set, PermissionSetReference::Name(name) if name == "observe/metrics-read")))
}

async fn collect_source(
    source: &MetricsSource,
    window: &CollectionWindow,
    config: &ClientConfig,
) -> Result<(metrics::MetricBatch, String, String)> {
    let http = reqwest::Client::new();
    match source {
        MetricsSource::Aws(sources) => {
            let config = config.aws_config().ok_or_else(|| {
                AlienError::new(failure("AWS metric source requires AWS target credentials"))
            })?;
            let credentials = alien_aws_clients::AwsCredentialProvider::from_config(config.clone())
                .await
                .context(failure("Could not initialize CloudWatch credentials"))?;
            let client = alien_aws_clients::CloudWatchClient::new(http, credentials);
            Ok((
                metrics::aws::collect(&client, sources, window)
                    .await
                    .context(failure("CloudWatch collection failed"))?,
                "aws".into(),
                config.region.clone(),
            ))
        }
        MetricsSource::Gcp(source) => {
            let config = config.gcp_config().ok_or_else(|| {
                AlienError::new(failure("GCP metric source requires GCP target credentials"))
            })?;
            if source.project_id != config.project_id {
                return Err(AlienError::new(failure(
                    "Metric source project differs from target project",
                )));
            }
            let client = alien_gcp_clients::MonitoringClient::new(http, config.clone());
            Ok((
                metrics::gcp::collect(&client, source, window)
                    .await
                    .context(failure("Cloud Monitoring collection failed"))?,
                "gcp".into(),
                config.region.clone(),
            ))
        }
        MetricsSource::Azure(source) => {
            let config = config.azure_config().ok_or_else(|| {
                AlienError::new(failure(
                    "Azure metric source requires Azure target credentials",
                ))
            })?;
            if !source.resource_uri.to_lowercase().starts_with(&format!(
                "/subscriptions/{}/",
                config.subscription_id.to_lowercase()
            )) {
                return Err(AlienError::new(failure(
                    "Azure metric source is outside target subscription",
                )));
            }
            let client = alien_azure_clients::AzureMonitorClient::new(
                http,
                alien_azure_clients::AzureTokenCache::new(config.clone()),
            );
            Ok((
                metrics::azure::collect(&client, source, window)
                    .await
                    .context(failure("Azure Monitor collection failed"))?,
                "azure".into(),
                String::new(),
            ))
        }
        MetricsSource::Scrape(source) => {
            let now = u64::try_from(
                Utc::now()
                    .timestamp_nanos_opt()
                    .ok_or_else(|| AlienError::new(failure("Metric timestamp overflow")))?,
            )
            .map_err(|_| AlienError::new(failure("Invalid scrape timestamp")))?;
            Ok((
                metrics::scrape::collect(source, now)
                    .await
                    .context(failure("Workload metrics scrape failed"))?,
                "workload".into(),
                String::new(),
            ))
        }
    }
}

pub async fn export_to_endpoint(config: &DeploymentConfig, data: Vec<u8>) -> Result<()> {
    let monitoring = config.monitoring.as_ref().ok_or_else(|| {
        AlienError::new(failure("Metric collection requires configured OTLP export"))
    })?;
    let endpoint = monitoring
        .metrics_endpoint
        .as_ref()
        .ok_or_else(|| AlienError::new(failure("Metric collection requires metrics endpoint")))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .into_alien_error()
        .context(failure("Could not initialize OTLP export"))?;
    let mut request = client
        .post(endpoint)
        .header("content-type", "application/x-protobuf");
    for header in monitoring
        .metrics_auth_header
        .as_ref()
        .unwrap_or(&monitoring.logs_auth_header)
        .split(',')
        .filter(|part| !part.trim().is_empty())
    {
        let (key, value) = header
            .split_once('=')
            .ok_or_else(|| AlienError::new(failure("Invalid OTLP metric auth header")))?;
        request = request.header(key.trim(), value.trim());
    }
    let response = request
        .body(data)
        .send()
        .await
        .into_alien_error()
        .context(failure("OTLP metrics export failed"))?;
    if !response.status().is_success() {
        return Err(AlienError::new(failure(
            "OTLP metric endpoint did not acknowledge batch",
        )));
    }
    // OTLP partial_success is not a full ACK. Decode and verify in transport before checkpoint.
    let bytes = response
        .bytes()
        .await
        .into_alien_error()
        .context(failure("Could not read OTLP acknowledgement"))?;
    let response = <opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceResponse as prost::Message>::decode(bytes).into_alien_error().context(failure("Invalid OTLP metrics acknowledgement"))?;
    if response.partial_success.is_some_and(|partial| {
        partial.rejected_data_points > 0 || !partial.error_message.is_empty()
    }) {
        return Err(AlienError::new(failure(
            "OTLP endpoint partially rejected metric batch",
        )));
    }
    Ok(())
}

fn failure(message: &str) -> ErrorData {
    ErrorData::DeploymentError {
        message: message.into(),
    }
}
