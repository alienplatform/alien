use std::collections::BTreeMap;

use alien_azure_clients::{monitor::ListMetricsRequest, MonitorApi};

use super::{invalid, point, scalar, timestamp, CollectionWindow, MetricBatch, ScalarKind};
use crate::Result;

/// Uses per-resource ARM reads; never upgrades to subscription-wide batch authorization.
pub use alien_core::AzureMetricSource;

pub async fn collect(
    client: &dyn MonitorApi,
    source: &AzureMetricSource,
    window: &CollectionWindow,
) -> Result<MetricBatch> {
    window.validate()?;
    if source.metric_names.is_empty()
        || source.metric_names.len() > 20
        || source.interval_seconds == 0
        || !matches!(
            source.aggregation.as_str(),
            "Total" | "Average" | "Minimum" | "Maximum" | "Count"
        )
    {
        return Err(invalid(
            "Invalid Azure metric names, interval or aggregation",
        ));
    }
    let mut request = ListMetricsRequest::new(&source.resource_uri);
    request.timespan = Some(format!(
        "{}/{}",
        window.start.to_rfc3339(),
        window.end.to_rfc3339()
    ));
    request.interval = Some(format!("PT{}S", source.interval_seconds));
    request.metric_names = source.metric_names.clone();
    request.metric_namespace = Some(source.metric_namespace.clone());
    request.aggregation = Some(source.aggregation.clone());
    request.filter = source.filter.clone();
    request.result_type = Some("Data".into());
    request.auto_adjust_timegrain = Some(false);
    request.validate_dimensions = Some(true);
    let response = client.list_metrics(request).await?;
    let interval = response
        .interval
        .as_deref()
        .ok_or_else(|| invalid("Azure response missing timegrain"))?;
    let seconds = duration_seconds(interval)?;
    if seconds != u64::from(source.interval_seconds) {
        return Err(invalid("Azure returned a different timegrain"));
    }
    let mut batch = MetricBatch::default();
    for metric in response.value {
        if metric
            .error_code
            .as_deref()
            .is_some_and(|code| code != "Success")
        {
            return Err(invalid("Azure returned a metric-level error"));
        }
        let name = metric
            .name
            .and_then(|name| name.value)
            .ok_or_else(|| invalid("Azure metric missing identity"))?;
        if !source.metric_names.contains(&name) {
            return Err(invalid("Azure returned an unrequested metric"));
        }
        for series in metric.timeseries {
            let mut labels = BTreeMap::from([
                ("source.statistic".into(), source.aggregation.clone()),
                ("source.period_seconds".into(), seconds.to_string()),
            ]);
            for dimension in series.metadata_values {
                let key = dimension
                    .name
                    .and_then(|name| name.value)
                    .ok_or_else(|| invalid("Azure dimension missing name"))?;
                let value = dimension
                    .value
                    .ok_or_else(|| invalid("Azure dimension missing value"))?;
                if labels.insert(format!("dimension.{key}"), value).is_some() {
                    return Err(invalid("Azure returned duplicate dimension"));
                }
            }
            let mut points = Vec::new();
            for sample in series.data {
                // Azure timestamps denote the start of the returned aggregation interval.
                let start = timestamp(
                    sample
                        .time_stamp
                        .as_deref()
                        .ok_or_else(|| invalid("Azure sample missing timestamp"))?,
                )?;
                let end = start
                    .checked_add(seconds * 1_000_000_000)
                    .ok_or_else(|| invalid("Azure timestamp overflow"))?;
                let value = match source.aggregation.as_str() {
                    "Total" => sample.total,
                    "Average" => sample.average,
                    "Minimum" => sample.minimum,
                    "Maximum" => sample.maximum,
                    "Count" => sample.count,
                    _ => unreachable!(),
                };
                // Null means unavailable, never zero.
                if let Some(value) = value {
                    points.push(point(start, end, value, &labels)?);
                }
            }
            batch.metrics.push(scalar(
                format!("{}/{}", source.metric_namespace, name),
                metric.unit.clone().unwrap_or_default(),
                if matches!(source.aggregation.as_str(), "Total" | "Count") {
                    ScalarKind::DeltaSum
                } else {
                    ScalarKind::Gauge
                },
                points,
            ));
        }
    }
    Ok(batch)
}

fn duration_seconds(value: &str) -> Result<u64> {
    let time = value
        .strip_prefix("PT")
        .ok_or_else(|| invalid("Unsupported Azure interval"))?;
    let (number, multiplier) = if let Some(value) = time.strip_suffix('S') {
        (value, 1)
    } else if let Some(value) = time.strip_suffix('M') {
        (value, 60)
    } else if let Some(value) = time.strip_suffix('H') {
        (value, 3600)
    } else {
        return Err(invalid("Unsupported Azure interval"));
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier))
        .filter(|n| *n > 0 && *n <= 86400)
        .ok_or_else(|| invalid("Invalid Azure interval"))
}

pub fn container_app(resource_uri: &str) -> Vec<AzureMetricSource> {
    vec![
        native(
            resource_uri,
            "Microsoft.App/containerApps",
            &["Requests"],
            "Total",
        ),
        native(
            resource_uri,
            "Microsoft.App/containerApps",
            &[
                "UsageNanoCores",
                "WorkingSetBytes",
                "Replicas",
                "RestartCount",
            ],
            "Average",
        ),
    ]
}

/// Blob-service platform metrics apply to the service/account, not an individual container.
pub fn blob_service(blob_service_uri: &str) -> Vec<AzureMetricSource> {
    vec![
        native(
            blob_service_uri,
            "Microsoft.Storage/storageAccounts/blobServices",
            &["Transactions", "Ingress", "Egress"],
            "Total",
        ),
        native(
            blob_service_uri,
            "Microsoft.Storage/storageAccounts/blobServices",
            &["SuccessE2ELatency", "SuccessServerLatency", "Availability"],
            "Average",
        ),
    ]
}

pub fn service_bus(namespace_uri: &str, entity_name: &str) -> Vec<AzureMetricSource> {
    let mut sources = vec![
        native(
            namespace_uri,
            "Microsoft.ServiceBus/namespaces",
            &["ActiveMessages", "DeadletteredMessages"],
            "Average",
        ),
        native(
            namespace_uri,
            "Microsoft.ServiceBus/namespaces",
            &[
                "IncomingMessages",
                "OutgoingMessages",
                "ServerErrors",
                "UserErrors",
                "ThrottledRequests",
            ],
            "Total",
        ),
    ];
    for source in &mut sources {
        source.filter = Some(format!(
            "EntityName eq '{}'",
            entity_name.replace('\'', "''")
        ));
    }
    sources
}

fn native(
    resource_uri: &str,
    namespace: &str,
    metrics: &[&str],
    aggregation: &str,
) -> AzureMetricSource {
    AzureMetricSource {
        resource_uri: resource_uri.into(),
        metric_namespace: namespace.into(),
        metric_names: metrics.iter().map(|name| (*name).into()).collect(),
        aggregation: aggregation.into(),
        interval_seconds: 60,
        filter: None,
    }
}
