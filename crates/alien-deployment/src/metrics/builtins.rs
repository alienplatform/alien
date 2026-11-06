//! Built-in source derivation from trusted resource outputs. No provider discovery.
use alien_core::{
    metric_catalog, AzureStorageAccountOutputs, QueueOutputs, StorageOutputs, WorkerOutputs,
};
use alien_core::{ClientConfig, DeploymentState, MetricsSource};

/// Built-ins use only persisted outputs plus the deployment's trusted target context.
/// Controllers can attach richer exact-identity descriptors without provider discovery.
pub(super) fn collection_jobs(
    state: &DeploymentState,
    target: &ClientConfig,
    config: &alien_core::MetricsCollectionConfig,
) -> (
    Vec<alien_core::MetricsCollectionJob>,
    std::collections::BTreeMap<String, String>,
) {
    let mut jobs = config.jobs.clone();
    let mut errors = std::collections::BTreeMap::new();
    if !config.built_in_resources {
        return (jobs, errors);
    }
    let Some(stack) = &state.stack_state else {
        errors.insert(
            "builtin:status".into(),
            "Needs setup: deployed resource outputs unavailable".into(),
        );
        return (jobs, errors);
    };
    let mut resources: Vec<_> = stack.resources.iter().collect();
    resources.sort_by_key(|(id, _)| *id);
    for (id, resource) in resources {
        if !matches!(
            resource.resource_type.as_str(),
            "worker"
                | "container"
                | "daemon"
                | "storage"
                | "queue"
                | "postgres"
                | "network"
                | "compute-cluster"
                | "azure_storage_account"
        ) {
            continue;
        }
        if matches!(
            resource.status,
            alien_core::ResourceStatus::Deleted | alien_core::ResourceStatus::Deleting
        ) {
            continue;
        }
        let status_id = format!("builtin:{id}:status");
        let Some(outputs) = &resource.outputs else {
            errors.insert(
                status_id,
                "Needs setup: physical resource outputs unavailable".into(),
            );
            continue;
        };
        let mut sources = outputs.builtin_metric_sources().to_vec();
        if sources.is_empty() {
            if let Some(worker) = outputs.downcast_ref::<WorkerOutputs>() {
                if target.aws_config().is_some()
                    && resource.controller_platform.unwrap_or(state.platform)
                        == alien_core::Platform::Aws
                {
                    sources.push(MetricsSource::Aws(metric_catalog::aws::lambda(
                        &worker.worker_name,
                    )));
                } else if let Some(gcp) = target.gcp_config().filter(|_| {
                    resource.controller_platform.unwrap_or(state.platform)
                        == alien_core::Platform::Gcp
                }) {
                    sources.extend(
                        metric_catalog::gcp::cloud_run(
                            &gcp.project_id,
                            &worker.worker_name,
                            &gcp.region,
                        )
                        .into_iter()
                        .map(MetricsSource::Gcp),
                    );
                } else if target.azure_config().is_some()
                    && resource.controller_platform.unwrap_or(state.platform)
                        == alien_core::Platform::Azure
                {
                    if let Some(uri) = worker
                        .identifier
                        .as_ref()
                        .filter(|uri| uri.contains("/providers/Microsoft.App/containerApps/"))
                    {
                        sources.extend(
                            metric_catalog::azure::container_app(uri)
                                .into_iter()
                                .map(MetricsSource::Azure),
                        );
                    }
                }
            } else if let Some(storage) = outputs.downcast_ref::<StorageOutputs>() {
                if target.aws_config().is_some() {
                    sources.push(MetricsSource::Aws(metric_catalog::aws::s3_capacity(
                        &storage.bucket_name,
                        &["StandardStorage".into()],
                    )));
                } else if let Some(gcp) = target.gcp_config() {
                    sources.extend(
                        metric_catalog::gcp::storage(&gcp.project_id, &storage.bucket_name)
                            .into_iter()
                            .map(MetricsSource::Gcp),
                    );
                }
                // Azure container metrics are not account totals. The account has its own job.
            } else if let Some(queue) = outputs.downcast_ref::<QueueOutputs>() {
                if target.aws_config().is_some() {
                    sources.push(MetricsSource::Aws(metric_catalog::aws::sqs(
                        &queue.queue_name,
                    )));
                } else if let (Some(gcp), Some(subscription)) =
                    (target.gcp_config(), queue.identifier.as_ref())
                {
                    sources.extend(
                        metric_catalog::gcp::pubsub_subscription(&gcp.project_id, subscription)
                            .into_iter()
                            .map(MetricsSource::Gcp),
                    );
                } else if target.azure_config().is_some() {
                    if let Some((namespace, entity)) = queue
                        .identifier
                        .as_ref()
                        .and_then(|value| value.split_once('/'))
                    {
                        let physical = stack
                            .resources
                            .values()
                            .filter_map(|state| state.outputs.as_ref())
                            .filter_map(|outputs| {
                                outputs
                                    .downcast_ref::<alien_core::AzureServiceBusNamespaceOutputs>()
                            })
                            .find(|outputs| outputs.namespace_name == namespace);
                        if let Some(physical) = physical {
                            sources.extend(
                                metric_catalog::azure::service_bus(&physical.resource_id, entity)
                                    .into_iter()
                                    .map(MetricsSource::Azure),
                            );
                        }
                    }
                }
            } else if let Some(account) = outputs.downcast_ref::<AzureStorageAccountOutputs>() {
                sources.extend(
                    metric_catalog::azure::blob_service(&format!(
                        "{}/blobServices/default",
                        account.resource_id
                    ))
                    .into_iter()
                    .map(MetricsSource::Azure),
                );
            }
        }
        if sources.is_empty() {
            errors.insert(status_id, "Unsupported built-in source: owning controller has no exact physical metric identity; explicit scraping may be configured separately".into());
        }
        for (index, source) in sources.into_iter().enumerate() {
            jobs.push(alien_core::MetricsCollectionJob {
                id: format!("builtin:{id}:{index}"),
                resource_id: id.clone(),
                source,
            });
        }
    }
    (jobs, errors)
}
