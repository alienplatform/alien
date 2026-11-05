//! Explicit, read-only metric collection. Collection does not discover or mutate resources.
//! The caller supplies approved sources and exports the resulting standard OTLP request.
#[cfg(feature = "aws")]
pub mod aws;
#[cfg(feature = "azure")]
pub mod azure;
#[cfg(feature = "gcp")]
pub mod gcp;

pub mod scrape;

use std::collections::BTreeMap;

use alien_client_core::ErrorData;
use alien_error::AlienError;
use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    common::v1::{any_value::Value, AnyValue, KeyValue},
    metrics::v1::{
        metric::Data, number_data_point, AggregationTemporality, Gauge, Metric, NumberDataPoint,
        ResourceMetrics, ScopeMetrics, Sum,
    },
    resource::v1::Resource,
};
use prost::Message;
use serde::{Deserialize, Serialize};

use crate::Result;

/// A bounded source interval; checkpoints advance only after the export is acknowledged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl CollectionWindow {
    pub fn validate(&self) -> Result<()> {
        if self.start >= self.end
            || self.start.timestamp() < 0
            || self.end - self.start > chrono::Duration::days(1)
        {
            return Err(invalid(
                "Collection window must be ordered, after epoch and at most one day",
            ));
        }
        Ok(())
    }
}

/// Source scalar semantics. Interval totals must never be treated as cumulative counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScalarKind {
    Gauge,
    DeltaSum,
    DeltaCounter,
    CumulativeSum,
}

/// A trusted resource identity, attached outside source-controlled metric labels.
pub use alien_core::MetricResource;

/// Complete collection result. No partial page is exported as a complete collection.
#[derive(Debug, Clone, Default)]
pub struct MetricBatch {
    pub metrics: Vec<Metric>,
}

impl MetricBatch {
    pub fn encode_otlp(self, resource: &MetricResource) -> Vec<u8> {
        ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(Resource {
                    attributes: attributes(&BTreeMap::from([
                        ("alien.deployment.id".into(), resource.deployment_id.clone()),
                        ("alien.resource.id".into(), resource.resource_id.clone()),
                        ("cloud.provider".into(), resource.provider.clone()),
                        ("cloud.region".into(), resource.region.clone()),
                    ])),
                    ..Default::default()
                }),
                scope_metrics: vec![ScopeMetrics {
                    metrics: self.metrics,
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
        .encode_to_vec()
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidClientConfig {
        message: message.into(),
        errors: None,
    })
}

pub(crate) fn timestamp(value: &str) -> Result<u64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|time| time.timestamp_nanos_opt())
        .and_then(|time| u64::try_from(time).ok())
        .ok_or_else(|| {
            invalid("Metric timestamp must be a valid non-negative nanosecond timestamp")
        })
}

pub(crate) fn attributes(values: &BTreeMap<String, String>) -> Vec<KeyValue> {
    values
        .iter()
        .map(|(key, value)| KeyValue {
            key: key.clone(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(value.clone())),
            }),
        })
        .collect()
}

pub(crate) fn scalar(
    name: String,
    unit: String,
    kind: ScalarKind,
    points: Vec<NumberDataPoint>,
) -> Metric {
    let data = match kind {
        ScalarKind::Gauge => Data::Gauge(Gauge {
            data_points: points,
        }),
        ScalarKind::DeltaSum | ScalarKind::DeltaCounter | ScalarKind::CumulativeSum => {
            Data::Sum(Sum {
                data_points: points,
                aggregation_temporality: if matches!(
                    kind,
                    ScalarKind::DeltaSum | ScalarKind::DeltaCounter
                ) {
                    AggregationTemporality::Delta as i32
                } else {
                    AggregationTemporality::Cumulative as i32
                },
                // Cloud interval totals may include corrections and are not assumed monotonic.
                is_monotonic: matches!(kind, ScalarKind::CumulativeSum | ScalarKind::DeltaCounter),
            })
        }
    };
    Metric {
        name,
        unit,
        data: Some(data),
        ..Default::default()
    }
}

pub(crate) fn point(
    start: u64,
    end: u64,
    value: f64,
    labels: &BTreeMap<String, String>,
) -> Result<NumberDataPoint> {
    if !value.is_finite() || start > end {
        return Err(invalid(
            "Metric point has a non-finite value or reversed interval",
        ));
    }
    Ok(NumberDataPoint {
        attributes: attributes(labels),
        start_time_unix_nano: start,
        time_unix_nano: end,
        value: Some(number_data_point::Value::AsDouble(value)),
        ..Default::default()
    })
}
