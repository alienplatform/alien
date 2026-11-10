//! Pulls the CloudWatch metrics of a deployment's AWS Lambda functions and maps them to OTLP.
//!
//! The pull is read-only and needs one provider action, `cloudwatch:GetMetricData`, carried by
//! the `observe/metrics-read` permission set. Function names come from the deployment's stack
//! state, so no account scan is needed.
//!
//! Mapping rules:
//! - `Invocations`, `Errors` and `Throttles` (60 s `Sum`) become delta sums. A point starts at
//!   the period start CloudWatch reports and ends 60 s later.
//! - `Duration` (`Average`, `Maximum`) and `ConcurrentExecutions` (`Maximum`) become gauges at the
//!   period end, with the CloudWatch statistic in the `statistic` attribute. No percentile is
//!   ever computed here.
//! - A period CloudWatch reports no point for stays absent. Nothing is filled with zero, and a
//!   denied read is an error, never an empty batch.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_aws_clients::cloudwatch::{
    Dimension, GetMetricDataRequest, Metric, MetricDataQuery, MetricDataResult, MetricStat,
};
use alien_aws_clients::CloudWatchApi;
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{Platform, StackState, WorkerOutputs};
use alien_error::{AlienError, ContextError};
use chrono::{DateTime, Duration, DurationRound, Utc};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::ExportMetricsServiceRequest,
    common::v1::{any_value::Value, AnyValue, InstrumentationScope, KeyValue},
    metrics::v1::{
        metric::Data, number_data_point, AggregationTemporality, Gauge, Metric as OtlpMetric,
        NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
    },
    resource::v1::Resource,
};

use crate::error::{MetricsErrorData, MetricsResult};

/// The CloudWatch namespace of Lambda's own metrics.
pub const LAMBDA_NAMESPACE: &str = "AWS/Lambda";
/// The permission set that grants the one provider action this module needs.
pub const METRICS_READ_PERMISSION_SET: &str = "observe/metrics-read";
/// The provider action behind [`METRICS_READ_PERMISSION_SET`].
pub const GET_METRIC_DATA_ACTION: &str = "cloudwatch:GetMetricData";
/// The seconds each point covers.
pub const PERIOD_SECONDS: i32 = 60;
/// The length of the window one pull reads, in seconds. It overlaps the previous pulls so that
/// points CloudWatch publishes late are still picked up.
pub const WINDOW_SECONDS: i64 = 15 * 60;
/// The most queries one `GetMetricData` call may carry.
pub const MAX_QUERIES_PER_CALL: usize = 500;
/// Pages followed for one call before the pull gives up instead of truncating silently.
const MAX_PAGES_PER_CALL: usize = 20;

const METRICS_SOURCE: &str = "CloudWatch";
const SCOPE_NAME: &str = "aws.cloudwatch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    DeltaSum,
    Gauge,
}

struct CatalogEntry {
    cloudwatch_metric: &'static str,
    statistic: &'static str,
    otlp_name: &'static str,
    unit: &'static str,
    description: &'static str,
    kind: Kind,
}

/// The fixed set of Lambda metrics that are pulled.
const CATALOG: [CatalogEntry; 6] = [
    CatalogEntry {
        cloudwatch_metric: "Invocations",
        statistic: "Sum",
        otlp_name: "aws.lambda.invocations",
        unit: "{invocation}",
        description: "Function invocations in the period, including failed ones",
        kind: Kind::DeltaSum,
    },
    CatalogEntry {
        cloudwatch_metric: "Errors",
        statistic: "Sum",
        otlp_name: "aws.lambda.errors",
        unit: "{error}",
        description: "Invocations that ended in a function error",
        kind: Kind::DeltaSum,
    },
    CatalogEntry {
        cloudwatch_metric: "Throttles",
        statistic: "Sum",
        otlp_name: "aws.lambda.throttles",
        unit: "{throttle}",
        description: "Invocation attempts rejected for lack of concurrency",
        kind: Kind::DeltaSum,
    },
    CatalogEntry {
        cloudwatch_metric: "Duration",
        statistic: "Average",
        otlp_name: "aws.lambda.duration",
        unit: "ms",
        description: "Time the function code ran, per statistic",
        kind: Kind::Gauge,
    },
    CatalogEntry {
        cloudwatch_metric: "Duration",
        statistic: "Maximum",
        otlp_name: "aws.lambda.duration",
        unit: "ms",
        description: "Time the function code ran, per statistic",
        kind: Kind::Gauge,
    },
    CatalogEntry {
        cloudwatch_metric: "ConcurrentExecutions",
        statistic: "Maximum",
        otlp_name: "aws.lambda.concurrent_executions",
        unit: "{execution}",
        description: "Highest number of function instances running at once in the period",
        kind: Kind::Gauge,
    },
];

/// One Lambda function whose metrics are pulled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaFunctionTarget {
    /// The Lambda function name, which is CloudWatch's `FunctionName` dimension.
    pub function_name: String,
    /// The id of the resource in the stack that owns the function.
    pub resource_id: String,
}

/// The deployment the pulled metrics belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaMetricsScope {
    pub deployment_id: String,
    pub account_id: String,
    pub region: String,
}

/// The time range of one pull: whole minutes, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl MetricsWindow {
    /// The last 15 whole minutes that ended at or before `now`.
    pub fn last_completed(now: DateTime<Utc>) -> Self {
        let end = now
            .duration_trunc(Duration::seconds(i64::from(PERIOD_SECONDS)))
            .expect("truncating to a whole minute cannot overflow");
        Self {
            start: end - Duration::seconds(WINDOW_SECONDS),
            end,
        }
    }
}

/// What one pull produced. Hand `request` to whatever sends OTLP metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaMetricsBatch {
    pub window: MetricsWindow,
    /// One resource per function that had at least one point.
    pub request: ExportMetricsServiceRequest,
    /// The functions that were queried, including ones without any point in the window.
    pub functions_queried: usize,
    pub data_points: usize,
}

/// The Lambda functions of an AWS deployment, from its stack state.
///
/// A worker counts once it has published its ARN (that is, once its function exists). The name
/// is read from that ARN, so a worker whose state has no function ARN is skipped rather than
/// queried under a guessed name.
pub fn lambda_targets_from_state(state: &StackState) -> Vec<LambdaFunctionTarget> {
    if state.platform != Platform::Aws {
        return Vec::new();
    }
    let mut targets: Vec<LambdaFunctionTarget> = state
        .resources
        .iter()
        .filter_map(|(resource_id, resource)| {
            let outputs = resource.outputs.as_ref()?.downcast_ref::<WorkerOutputs>()?;
            let function_name = function_name_from_arn(outputs.identifier.as_deref()?)?;
            Some(LambdaFunctionTarget {
                function_name,
                resource_id: resource_id.clone(),
            })
        })
        .collect();
    targets.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    targets
}

/// `arn:aws:lambda:<region>:<account>:function:<name>[:<qualifier>]` to `<name>`.
fn function_name_from_arn(arn: &str) -> Option<String> {
    let mut parts = arn.splitn(8, ':');
    let (prefix, _partition, service) = (parts.next()?, parts.next()?, parts.next()?);
    let (_region, _account, kind, name) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if prefix != "arn" || service != "lambda" || kind != "function" || name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

fn query_id(target_index: usize, catalog_index: usize) -> String {
    format!("f{target_index}c{catalog_index}")
}

fn parse_query_id(id: &str) -> Option<(usize, usize)> {
    let (target, catalog) = id.strip_prefix('f')?.split_once('c')?;
    let (target, catalog) = (target.parse().ok()?, catalog.parse().ok()?);
    (catalog < CATALOG.len()).then_some((target, catalog))
}

/// Every query for every target, in a fixed order. Result ids tell which query they answer.
fn build_queries(targets: &[LambdaFunctionTarget]) -> Vec<MetricDataQuery> {
    targets
        .iter()
        .enumerate()
        .flat_map(|(target_index, target)| {
            CATALOG
                .iter()
                .enumerate()
                .map(move |(catalog_index, entry)| MetricDataQuery {
                    id: query_id(target_index, catalog_index),
                    expression: None,
                    label: None,
                    metric_stat: Some(MetricStat {
                        metric: Metric {
                            namespace: Some(LAMBDA_NAMESPACE.to_string()),
                            metric_name: Some(entry.cloudwatch_metric.to_string()),
                            dimensions: vec![Dimension {
                                name: "FunctionName".to_string(),
                                value: target.function_name.clone(),
                            }],
                        },
                        period: PERIOD_SECONDS,
                        stat: entry.statistic.to_string(),
                        unit: None,
                    }),
                    period: None,
                    return_data: Some(true),
                })
        })
        .collect()
}

/// Pulls Lambda metrics from CloudWatch.
pub struct LambdaMetricsPuller {
    cloudwatch: Arc<dyn CloudWatchApi>,
    scope: LambdaMetricsScope,
}

impl LambdaMetricsPuller {
    pub fn new(cloudwatch: Arc<dyn CloudWatchApi>, scope: LambdaMetricsScope) -> Self {
        Self { cloudwatch, scope }
    }

    /// Reads `window` for `targets`: up to 500 queries per call, every `NextToken` followed.
    ///
    /// Fails with [`MetricsErrorData::SetupUpdateRequired`] when the identity may not call
    /// `GetMetricData`. Any other failure aborts the whole pull, so a caller never sends a
    /// batch with a function silently missing. Retrying is the caller's decision.
    pub async fn pull(
        &self,
        targets: &[LambdaFunctionTarget],
        window: MetricsWindow,
    ) -> MetricsResult<LambdaMetricsBatch> {
        let queries = build_queries(targets);
        let mut series: BTreeMap<String, SeriesPoints> = BTreeMap::new();
        for chunk in queries.chunks(MAX_QUERIES_PER_CALL) {
            self.read_chunk(chunk, window, &mut series).await?;
        }
        let request = lambda_metrics_to_otlp(&self.scope, targets, &series)?;
        let data_points = request
            .resource_metrics
            .iter()
            .flat_map(|resource| &resource.scope_metrics)
            .flat_map(|scope| &scope.metrics)
            .map(metric_point_count)
            .sum();
        Ok(LambdaMetricsBatch {
            window,
            request,
            functions_queried: targets.len(),
            data_points,
        })
    }

    async fn read_chunk(
        &self,
        chunk: &[MetricDataQuery],
        window: MetricsWindow,
        series: &mut BTreeMap<String, SeriesPoints>,
    ) -> MetricsResult<()> {
        let mut next_token: Option<String> = None;
        for _ in 0..MAX_PAGES_PER_CALL {
            let response = self
                .cloudwatch
                .get_metric_data(GetMetricDataRequest {
                    start_time: window.start.timestamp(),
                    end_time: window.end.timestamp(),
                    metric_data_queries: chunk.to_vec(),
                    max_datapoints: None,
                    next_token: next_token.take(),
                    scan_by: Some("TimestampAscending".to_string()),
                })
                .await
                .map_err(map_client_error)?;
            for result in response.metric_data_results {
                absorb_result(series, result)?;
            }
            match response.next_token.filter(|token| !token.is_empty()) {
                Some(token) => next_token = Some(token),
                None => return Ok(()),
            }
        }
        Err(AlienError::new(MetricsErrorData::ResponseInvalid {
            metrics_source: METRICS_SOURCE.to_string(),
            message: format!("still more pages after {MAX_PAGES_PER_CALL} for one call"),
        }))
    }
}

fn map_client_error(error: AlienError<ClientErrorData>) -> AlienError<MetricsErrorData> {
    if matches!(
        error.error,
        Some(ClientErrorData::RemoteAccessDenied { .. })
    ) {
        return error.context(MetricsErrorData::SetupUpdateRequired {
            metrics_source: METRICS_SOURCE.to_string(),
            missing_action: GET_METRIC_DATA_ACTION.to_string(),
            permission_set: METRICS_READ_PERMISSION_SET.to_string(),
        });
    }
    error.context(MetricsErrorData::PullFailed {
        metrics_source: METRICS_SOURCE.to_string(),
        message: "GetMetricData failed".to_string(),
    })
}

/// The points of one query, merged across pages, by epoch second.
type SeriesPoints = BTreeMap<i64, f64>;

/// Adds one page's answer for a query to the merged series.
fn absorb_result(
    series: &mut BTreeMap<String, SeriesPoints>,
    result: MetricDataResult,
) -> MetricsResult<()> {
    let invalid = |message: String| {
        AlienError::new(MetricsErrorData::ResponseInvalid {
            metrics_source: METRICS_SOURCE.to_string(),
            message,
        })
    };
    let notes = || {
        result
            .messages
            .iter()
            .filter_map(|message| message.value.as_deref())
            .collect::<Vec<_>>()
            .join("; ")
    };
    match result.status_code.as_deref() {
        // The last page of a query is Complete. Earlier pages of a long one say PartialData.
        Some("Complete") | Some("PartialData") => {}
        Some("Forbidden") => {
            return Err(AlienError::new(MetricsErrorData::SetupUpdateRequired {
                metrics_source: METRICS_SOURCE.to_string(),
                missing_action: GET_METRIC_DATA_ACTION.to_string(),
                permission_set: METRICS_READ_PERMISSION_SET.to_string(),
            }));
        }
        other => {
            return Err(invalid(format!(
                "query '{}' has status {}: {}",
                result.id,
                other.unwrap_or("missing"),
                notes()
            )));
        }
    }
    if result.timestamps.len() != result.values.len() {
        return Err(invalid(format!(
            "query '{}' has {} timestamps but {} values",
            result.id,
            result.timestamps.len(),
            result.values.len()
        )));
    }
    let points = series.entry(result.id.clone()).or_default();
    for (timestamp, value) in result.timestamps.into_iter().zip(result.values) {
        if !value.is_finite() {
            return Err(invalid(format!(
                "query '{}' has a non-finite value at {timestamp}",
                result.id
            )));
        }
        points.insert(timestamp, value);
    }
    Ok(())
}

fn string_attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_string())),
        }),
    }
}

fn nanos(epoch_seconds: i64) -> u64 {
    u64::try_from(epoch_seconds).unwrap_or(0) * 1_000_000_000
}

/// Maps the merged answers of a pull to OTLP. `series` is keyed by query id.
fn lambda_metrics_to_otlp(
    scope: &LambdaMetricsScope,
    targets: &[LambdaFunctionTarget],
    series: &BTreeMap<String, SeriesPoints>,
) -> MetricsResult<ExportMetricsServiceRequest> {
    let mut per_target: BTreeMap<usize, BTreeMap<usize, &SeriesPoints>> = BTreeMap::new();
    for (id, points) in series {
        let unknown = || {
            AlienError::new(MetricsErrorData::ResponseInvalid {
                metrics_source: METRICS_SOURCE.to_string(),
                message: format!("result '{id}' answers no query that was sent"),
            })
        };
        let (target_index, catalog_index) = parse_query_id(id).ok_or_else(unknown)?;
        if target_index >= targets.len() {
            return Err(unknown());
        }
        per_target
            .entry(target_index)
            .or_default()
            .insert(catalog_index, points);
    }

    let mut resource_metrics = Vec::new();
    for (target_index, answers) in per_target {
        let target = &targets[target_index];
        let metrics = metrics_for_function(&answers);
        if metrics.is_empty() {
            continue;
        }
        resource_metrics.push(ResourceMetrics {
            resource: Some(Resource {
                attributes: vec![
                    string_attribute("cloud.provider", "aws"),
                    string_attribute("cloud.account.id", &scope.account_id),
                    string_attribute("cloud.region", &scope.region),
                    string_attribute("faas.name", &target.function_name),
                    string_attribute("alien.deployment_id", &scope.deployment_id),
                    string_attribute("alien.resource_id", &target.resource_id),
                ],
                ..Default::default()
            }),
            scope_metrics: vec![ScopeMetrics {
                scope: Some(InstrumentationScope {
                    name: SCOPE_NAME.to_string(),
                    ..Default::default()
                }),
                metrics,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        });
    }
    Ok(ExportMetricsServiceRequest { resource_metrics })
}

/// One OTLP metric per name. The two `Duration` statistics share one gauge.
fn metrics_for_function(answers: &BTreeMap<usize, &SeriesPoints>) -> Vec<OtlpMetric> {
    let mut names_in_order = Vec::new();
    let mut sums: BTreeMap<&str, Vec<NumberDataPoint>> = BTreeMap::new();
    let mut gauges: BTreeMap<&str, Vec<NumberDataPoint>> = BTreeMap::new();
    for (catalog_index, points) in answers {
        let entry = &CATALOG[*catalog_index];
        if points.is_empty() {
            continue;
        }
        if !names_in_order.contains(&entry.otlp_name) {
            names_in_order.push(entry.otlp_name);
        }
        match entry.kind {
            Kind::DeltaSum => sums
                .entry(entry.otlp_name)
                .or_default()
                .extend(points.iter().map(|(&start, &value)| NumberDataPoint {
                    start_time_unix_nano: nanos(start),
                    time_unix_nano: nanos(start + i64::from(PERIOD_SECONDS)),
                    value: Some(number_data_point::Value::AsDouble(value)),
                    ..Default::default()
                })),
            Kind::Gauge => gauges
                .entry(entry.otlp_name)
                .or_default()
                .extend(points.iter().map(|(&start, &value)| NumberDataPoint {
                    attributes: vec![string_attribute("statistic", entry.statistic)],
                    time_unix_nano: nanos(start + i64::from(PERIOD_SECONDS)),
                    value: Some(number_data_point::Value::AsDouble(value)),
                    ..Default::default()
                })),
        }
    }
    names_in_order.sort_by_key(|name| CATALOG.iter().position(|entry| entry.otlp_name == *name));
    names_in_order
        .into_iter()
        .map(|name| {
            let entry = CATALOG
                .iter()
                .find(|entry| entry.otlp_name == name)
                .expect("names come from the catalog");
            let data = match entry.kind {
                Kind::DeltaSum => Data::Sum(Sum {
                    data_points: sums.remove(name).unwrap_or_default(),
                    aggregation_temporality: AggregationTemporality::Delta as i32,
                    is_monotonic: true,
                }),
                Kind::Gauge => Data::Gauge(Gauge {
                    data_points: gauges.remove(name).unwrap_or_default(),
                }),
            };
            OtlpMetric {
                name: name.to_string(),
                description: entry.description.to_string(),
                unit: entry.unit.to_string(),
                metadata: Vec::new(),
                data: Some(data),
            }
        })
        .collect()
}

fn metric_point_count(metric: &OtlpMetric) -> usize {
    match &metric.data {
        Some(Data::Sum(sum)) => sum.data_points.len(),
        Some(Data::Gauge(gauge)) => gauge.data_points.len(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_completed_window_is_whole_minutes_ending_at_or_before_now() {
        let now = DateTime::parse_from_rfc3339("2026-11-10T08:17:42Z")
            .unwrap()
            .with_timezone(&Utc);
        let window = MetricsWindow::last_completed(now);
        assert_eq!(window.end.to_rfc3339(), "2026-11-10T08:17:00+00:00");
        assert_eq!(window.start.to_rfc3339(), "2026-11-10T08:02:00+00:00");

        let on_the_minute = DateTime::parse_from_rfc3339("2026-11-10T08:17:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            MetricsWindow::last_completed(on_the_minute).end,
            on_the_minute
        );
    }

    #[test]
    fn query_ids_round_trip_and_reject_foreign_ids() {
        assert_eq!(parse_query_id(&query_id(12, 5)), Some((12, 5)));
        assert_eq!(parse_query_id("f1c6"), None, "catalog index out of range");
        assert_eq!(parse_query_id("m1"), None);
        assert_eq!(parse_query_id("f1"), None);
    }

    #[test]
    fn function_names_are_read_from_the_function_arn_only() {
        assert_eq!(
            function_name_from_arn("arn:aws:lambda:us-east-1:111111111111:function:demo-api")
                .as_deref(),
            Some("demo-api")
        );
        assert_eq!(
            function_name_from_arn("arn:aws:lambda:us-east-1:111111111111:function:demo-api:live")
                .as_deref(),
            Some("demo-api"),
            "an alias or version is not part of the FunctionName dimension"
        );
        assert_eq!(
            function_name_from_arn("arn:aws:sqs:us-east-1:111111111111:demo-queue"),
            None
        );
        assert_eq!(
            function_name_from_arn("arn:aws:lambda:us-east-1:111111111111:layer:demo"),
            None
        );
    }
}
