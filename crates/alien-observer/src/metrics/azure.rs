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
        || source.interval_seconds > 86400
        || window.start.timestamp() % i64::from(source.interval_seconds.max(1)) != 0
        || window.end.timestamp() % i64::from(source.interval_seconds.max(1)) != 0
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
                    if source.monotonic && value < 0.0 {
                        return Err(invalid("Azure counter increment is negative"));
                    }
                    if start
                        < u64::try_from(window.start.timestamp())
                            .map_err(|_| invalid("Invalid source window"))?
                            * 1_000_000_000
                        || end
                            > u64::try_from(window.end.timestamp())
                                .map_err(|_| invalid("Invalid source window"))?
                                * 1_000_000_000
                    {
                        return Err(invalid(
                            "Azure returned an unclosed or out-of-window aggregation period",
                        ));
                    }
                    points.push(point(start, end, value, &labels)?);
                }
            }
            batch.metrics.push(scalar(
                format!("{}/{}", source.metric_namespace, name),
                metric.unit.clone().unwrap_or_default(),
                if matches!(source.aggregation.as_str(), "Total" | "Count") {
                    if source.monotonic {
                        ScalarKind::DeltaCounter
                    } else {
                        ScalarKind::DeltaSum
                    }
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

pub use alien_core::metric_catalog::azure::*;
