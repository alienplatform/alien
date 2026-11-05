use std::collections::{BTreeMap, BTreeSet};

use alien_gcp_clients::{
    monitoring::{ListTimeSeriesRequest, TimeInterval, TimeSeriesView},
    MonitoringApi,
};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, AggregationTemporality, Histogram, HistogramDataPoint, Metric,
};

use super::{
    attributes, invalid, point, scalar, timestamp, CollectionWindow, MetricBatch, ScalarKind,
};
use crate::Result;

/// A project and exact provider filter approved by setup; no cross-project discovery.
pub use alien_core::GcpMetricSource;

pub async fn collect(
    client: &dyn MonitoringApi,
    source: &GcpMetricSource,
    window: &CollectionWindow,
) -> Result<MetricBatch> {
    window.validate()?;
    if source.project_id.is_empty() || source.filter.is_empty() {
        return Err(invalid(
            "GCP collection requires a project and explicit filter",
        ));
    }
    let mut token = None;
    let mut seen = BTreeSet::new();
    let mut batch = MetricBatch::default();
    for _ in 0..100 {
        let response = client
            .list_time_series(
                ListTimeSeriesRequest::builder()
                    .name(format!("projects/{}", source.project_id))
                    .filter(source.filter.clone())
                    .interval(TimeInterval {
                        start_time: window.start.to_rfc3339(),
                        end_time: window.end.to_rfc3339(),
                    })
                    .view(TimeSeriesView::Full)
                    .page_size(1000)
                    .maybe_page_token(token)
                    .build(),
            )
            .await?;
        if response
            .execution_errors
            .as_ref()
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(invalid("GCP returned time-series execution errors"));
        }
        for series in response.time_series {
            let metric = series
                .metric
                .ok_or_else(|| invalid("GCP series missing metric identity"))?;
            let name = metric
                .type_
                .ok_or_else(|| invalid("GCP series missing metric type"))?;
            let mut labels: BTreeMap<_, _> = metric
                .labels
                .into_iter()
                .map(|(key, value)| (format!("metric.{key}"), value))
                .collect();
            if let Some(resource) = series.resource {
                labels.extend(
                    resource
                        .labels
                        .into_iter()
                        .map(|(key, value)| (format!("resource.{key}"), value)),
                );
                if let Some(kind) = resource.type_ {
                    labels.insert("resource.type".into(), kind);
                }
            }
            let kind = match series.metric_kind.as_deref() {
                Some("GAUGE") => ScalarKind::Gauge,
                Some("DELTA") => ScalarKind::DeltaSum,
                Some("CUMULATIVE") => ScalarKind::CumulativeSum,
                _ => return Err(invalid("GCP series missing or unsupported metricKind")),
            };
            let mut numbers = Vec::new();
            let mut histograms = Vec::new();
            for sample in series.points {
                let interval = sample
                    .interval
                    .ok_or_else(|| invalid("GCP point missing interval"))?;
                let end = timestamp(&interval.end_time)?;
                let start = if interval.start_time.is_empty() && kind == ScalarKind::Gauge {
                    0
                } else {
                    timestamp(&interval.start_time)?
                };
                let value = sample
                    .value
                    .ok_or_else(|| invalid("GCP point missing value"))?;
                if let Some(distribution) = value.distribution_value {
                    histograms.push(distribution_point(&distribution, start, end, &labels)?);
                } else {
                    let sample = match (value.int64_value, value.double_value) {
                        (Some(integer), None) => {
                            let integer = integer
                                .parse::<i64>()
                                .map_err(|_| invalid("GCP int64Value is invalid"))?;
                            let mut sample = point(start, end, 0.0, &labels)?;
                            sample.value = Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(integer));
                            sample
                        }
                        (None, Some(value)) => point(start, end, value, &labels)?,
                        _ => {
                            return Err(invalid("GCP metric value is not a supported numeric type"))
                        }
                    };
                    numbers.push(sample);
                }
            }
            if !numbers.is_empty() && !histograms.is_empty() {
                return Err(invalid("GCP series mixes numeric and distribution values"));
            }
            if !histograms.is_empty() {
                batch.metrics.push(Metric {
                    name,
                    unit: series.unit.unwrap_or_default(),
                    data: Some(Data::Histogram(Histogram {
                        data_points: histograms,
                        aggregation_temporality: match kind {
                            ScalarKind::DeltaSum => AggregationTemporality::Delta as i32,
                            ScalarKind::CumulativeSum => AggregationTemporality::Cumulative as i32,
                            ScalarKind::Gauge => {
                                return Err(invalid(
                                    "Gauge distributions require a separate representation",
                                ))
                            }
                        },
                    })),
                    ..Default::default()
                });
            } else {
                batch
                    .metrics
                    .push(scalar(name, series.unit.unwrap_or_default(), kind, numbers));
            }
        }
        token = response.next_page_token.filter(|token| !token.is_empty());
        match token.as_ref() {
            None => return Ok(batch),
            Some(token) if !seen.insert(token.clone()) => {
                return Err(invalid("GCP pagination repeated a token"))
            }
            _ => {}
        }
    }
    Err(invalid("GCP collection exceeded page limit"))
}

fn distribution_point(
    value: &serde_json::Value,
    start: u64,
    end: u64,
    labels: &BTreeMap<String, String>,
) -> Result<HistogramDataPoint> {
    let integer = |value: &serde_json::Value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    };
    let count = value
        .get("count")
        .and_then(integer)
        .ok_or_else(|| invalid("GCP distribution missing count"))?;
    let buckets: Vec<u64> = value
        .get("bucketCounts")
        .and_then(|value| value.as_array())
        .ok_or_else(|| invalid("GCP distribution missing buckets"))?
        .iter()
        .map(|value| integer(value).ok_or_else(|| invalid("Invalid GCP bucket count")))
        .collect::<Result<_>>()?;
    let options = value
        .get("bucketOptions")
        .ok_or_else(|| invalid("GCP distribution missing bucket options"))?;
    let bounds: Vec<f64> = if let Some(explicit) = options.get("explicitBuckets") {
        explicit
            .get("bounds")
            .and_then(|value| value.as_array())
            .ok_or_else(|| invalid("Missing explicit bounds"))?
            .iter()
            .map(|bound| {
                bound
                    .as_f64()
                    .ok_or_else(|| invalid("Invalid explicit bound"))
            })
            .collect::<Result<_>>()?
    } else if let Some(linear) = options.get("linearBuckets") {
        let n = linear
            .get("numFiniteBuckets")
            .and_then(integer)
            .filter(|n| *n <= 10000)
            .ok_or_else(|| invalid("Invalid linear bucket count"))?;
        let offset = linear.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let width = linear
            .get("width")
            .and_then(|v| v.as_f64())
            .filter(|v| *v > 0.0)
            .ok_or_else(|| invalid("Invalid linear bucket width"))?;
        (0..=n).map(|i| offset + width * i as f64).collect()
    } else if let Some(exponential) = options.get("exponentialBuckets") {
        let n = exponential
            .get("numFiniteBuckets")
            .and_then(integer)
            .filter(|n| *n <= 10000)
            .ok_or_else(|| invalid("Invalid exponential bucket count"))?;
        let growth = exponential
            .get("growthFactor")
            .and_then(|v| v.as_f64())
            .filter(|v| *v > 1.0)
            .ok_or_else(|| invalid("Invalid bucket growth"))?;
        let scale = exponential
            .get("scale")
            .and_then(|v| v.as_f64())
            .filter(|v| *v > 0.0)
            .ok_or_else(|| invalid("Invalid bucket scale"))?;
        (0..=n).map(|i| scale * growth.powf(i as f64)).collect()
    } else {
        return Err(invalid("Unsupported GCP distribution bucket options"));
    };
    if start > end
        || buckets.len() != bounds.len() + 1
        || buckets
            .iter()
            .try_fold(0u64, |sum, count| sum.checked_add(*count))
            != Some(count)
        || bounds.iter().any(|v| !v.is_finite())
        || bounds.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(invalid(
            "GCP distribution bucket boundaries/counts are inconsistent",
        ));
    }
    let mean = value.get("mean").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let sum = mean * count as f64;
    if !sum.is_finite() {
        return Err(invalid("Invalid distribution sum"));
    }
    Ok(HistogramDataPoint {
        attributes: attributes(labels),
        start_time_unix_nano: start,
        time_unix_nano: end,
        count,
        sum: Some(sum),
        bucket_counts: buckets,
        explicit_bounds: bounds,
        ..Default::default()
    })
}

/// Exact-resource Cloud Run sources; latency remains a provider distribution.
pub fn cloud_run(project: &str, service: &str, location: &str) -> Vec<GcpMetricSource> {
    [
        "run.googleapis.com/request_count",
        "run.googleapis.com/request_latencies",
        "run.googleapis.com/container/instance_count",
        "run.googleapis.com/container/cpu/utilizations",
        "run.googleapis.com/container/memory/utilizations",
    ]
    .into_iter()
    .map(|metric| {
        resource_source(
            project,
            metric,
            "cloud_run_revision",
            &[("service_name", service), ("location", location)],
        )
    })
    .collect()
}

pub fn storage(project: &str, bucket: &str) -> Vec<GcpMetricSource> {
    [
        "storage.googleapis.com/api/request_count",
        "storage.googleapis.com/network/sent_bytes_count",
        "storage.googleapis.com/network/received_bytes_count",
        "storage.googleapis.com/storage/total_bytes",
        "storage.googleapis.com/storage/object_count",
    ]
    .into_iter()
    .map(|metric| resource_source(project, metric, "gcs_bucket", &[("bucket_name", bucket)]))
    .collect()
}

pub fn pubsub_subscription(project: &str, subscription: &str) -> Vec<GcpMetricSource> {
    [
        "pubsub.googleapis.com/subscription/num_undelivered_messages",
        "pubsub.googleapis.com/subscription/oldest_unacked_message_age",
        "pubsub.googleapis.com/subscription/sent_message_count",
        "pubsub.googleapis.com/subscription/ack_message_count",
    ]
    .into_iter()
    .map(|metric| {
        resource_source(
            project,
            metric,
            "pubsub_subscription",
            &[("subscription_id", subscription)],
        )
    })
    .collect()
}

fn resource_source(
    project: &str,
    metric: &str,
    resource_type: &str,
    labels: &[(&str, &str)],
) -> GcpMetricSource {
    let quote =
        |value: &str| serde_json::to_string(value).expect("string JSON encoding cannot fail");
    let mut filter = format!(
        "metric.type={} AND resource.type={}",
        quote(metric),
        quote(resource_type)
    );
    for (key, value) in labels {
        filter.push_str(&format!(" AND resource.labels.{key}={}", quote(value)));
    }
    GcpMetricSource {
        project_id: project.into(),
        filter,
    }
}
