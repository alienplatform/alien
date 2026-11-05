use std::collections::BTreeMap;

use alien_aws_clients::{
    cloudwatch::{Dimension, GetMetricDataRequest, Metric, MetricDataQuery, MetricStat},
    CloudWatchApi,
};

use super::{invalid, point, scalar, CollectionWindow, MetricBatch, ScalarKind};
use crate::Result;

/// One approved CloudWatch series, including all identity dimensions.
pub use alien_core::AwsMetricSource;

/// Collects all pages. Any partial/error result rejects the batch so its checkpoint stays put.
pub async fn collect(
    client: &dyn CloudWatchApi,
    sources: &[AwsMetricSource],
    window: &CollectionWindow,
) -> Result<MetricBatch> {
    window.validate()?;
    if sources.is_empty() || sources.len() > 500 {
        return Err(invalid("Expected 1–500 CloudWatch series"));
    }
    for source in sources {
        if source.period_seconds < 60
            || source.period_seconds > 86400
            || source.period_seconds % 60 != 0
            || !matches!(
                source.statistic.as_str(),
                "Sum" | "Average" | "Minimum" | "Maximum" | "SampleCount"
            )
        {
            return Err(invalid(
                "Invalid CloudWatch source period or unsupported statistic",
            ));
        }
    }
    let queries: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            MetricDataQuery::builder()
                .id(format!("m{index}"))
                .metric_stat(
                    MetricStat::builder()
                        .metric(
                            Metric::builder()
                                .namespace(source.namespace.clone())
                                .metric_name(source.metric_name.clone())
                                .dimensions(
                                    source
                                        .dimensions
                                        .iter()
                                        .map(|(name, value)| Dimension {
                                            name: name.clone(),
                                            value: value.clone(),
                                        })
                                        .collect(),
                                )
                                .build(),
                        )
                        .period(source.period_seconds)
                        .stat(source.statistic.clone())
                        .build(),
                )
                .return_data(true)
                .build()
        })
        .collect();
    let mut batch = MetricBatch::default();
    let mut token = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let response = client
            .get_metric_data(
                GetMetricDataRequest::builder()
                    .start_time(window.start.timestamp())
                    .end_time(window.end.timestamp())
                    .metric_data_queries(queries.clone())
                    .max_datapoints(10000)
                    .scan_by("TimestampAscending".to_string())
                    .maybe_next_token(token)
                    .build(),
            )
            .await?;
        if !response.messages.is_empty() {
            return Err(invalid(
                "CloudWatch returned collection messages; batch is incomplete",
            ));
        }
        let has_more = response
            .next_token
            .as_ref()
            .is_some_and(|token| !token.is_empty());
        for result in response.metric_data_results {
            let index = result
                .id
                .strip_prefix('m')
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| *index < sources.len())
                .ok_or_else(|| invalid("CloudWatch returned unknown query ID"))?;
            if !(matches!(result.status_code.as_deref(), Some("Complete"))
                || (has_more && matches!(result.status_code.as_deref(), Some("PartialData"))))
                || !result.messages.is_empty()
                || result.timestamps.len() != result.values.len()
            {
                return Err(invalid(
                    "CloudWatch returned incomplete or malformed series",
                ));
            }
            let source = &sources[index];
            let mut labels = source.dimensions.clone();
            labels.insert("source.statistic".into(), source.statistic.clone());
            labels.insert(
                "source.period_seconds".into(),
                source.period_seconds.to_string(),
            );
            let mut points = Vec::new();
            for (time, value) in result.timestamps.into_iter().zip(result.values) {
                // CloudWatch timestamps denote the beginning of the aggregation period.
                let start = u64::try_from(time)
                    .ok()
                    .and_then(|time| time.checked_mul(1_000_000_000))
                    .ok_or_else(|| invalid("Invalid CloudWatch timestamp"))?;
                let end = start
                    .checked_add(source.period_seconds as u64 * 1_000_000_000)
                    .ok_or_else(|| invalid("CloudWatch timestamp overflow"))?;
                points.push(point(start, end, value, &labels)?);
            }
            batch.metrics.push(scalar(
                format!("{}/{}", source.namespace, source.metric_name),
                source.unit.clone(),
                if source.statistic == "Sum" || source.statistic == "SampleCount" {
                    ScalarKind::DeltaSum
                } else {
                    ScalarKind::Gauge
                },
                points,
            ));
        }
        token = response.next_token.filter(|token| !token.is_empty());
        match token.as_ref() {
            None => return Ok(batch),
            Some(token) if !seen.insert(token.clone()) => {
                return Err(invalid("CloudWatch pagination repeated a token"))
            }
            _ => {}
        }
    }
    Err(invalid("CloudWatch collection exceeded page limit"))
}

/// Always-generated native Lambda metrics; no Insights or source mutation.
pub fn lambda(function_name: &str) -> Vec<AwsMetricSource> {
    [
        ("Invocations", "Sum", "{invocation}"),
        ("Errors", "Sum", "{error}"),
        ("Throttles", "Sum", "{request}"),
        ("Duration", "Average", "ms"),
        ("Duration", "Maximum", "ms"),
        ("ConcurrentExecutions", "Maximum", "{execution}"),
    ]
    .into_iter()
    .map(|(name, stat, unit)| AwsMetricSource {
        namespace: "AWS/Lambda".into(),
        metric_name: name.into(),
        dimensions: BTreeMap::from([("FunctionName".into(), function_name.into())]),
        statistic: stat.into(),
        period_seconds: 60,
        unit: unit.into(),
    })
    .collect()
}

/// Free daily capacity instrumentation. The caller must poll a daily source window,
/// rather than interpreting its daily observation as a fresh minute measurement.
pub fn s3_capacity(bucket: &str, storage_types: &[String]) -> Vec<AwsMetricSource> {
    let mut sources = vec![source(
        "AWS/S3",
        "NumberOfObjects",
        "Average",
        "{object}",
        86400,
        BTreeMap::from([
            ("BucketName".into(), bucket.into()),
            ("StorageType".into(), "AllStorageTypes".into()),
        ]),
    )];
    sources.extend(storage_types.iter().map(|storage_type| {
        source(
            "AWS/S3",
            "BucketSizeBytes",
            "Average",
            "By",
            86400,
            BTreeMap::from([
                ("BucketName".into(), bucket.into()),
                ("StorageType".into(), storage_type.clone()),
            ]),
        )
    }));
    sources
}

pub fn sqs(queue_name: &str) -> Vec<AwsMetricSource> {
    catalog(
        "AWS/SQS",
        "QueueName",
        queue_name,
        &[
            ("ApproximateNumberOfMessagesVisible", "Average", "{message}"),
            (
                "ApproximateNumberOfMessagesNotVisible",
                "Average",
                "{message}",
            ),
            ("ApproximateAgeOfOldestMessage", "Maximum", "s"),
            ("NumberOfMessagesSent", "Sum", "{message}"),
            ("NumberOfMessagesReceived", "Sum", "{delivery}"),
            ("NumberOfMessagesDeleted", "Sum", "{deletion}"),
        ],
    )
}

pub fn rds(instance_id: &str) -> Vec<AwsMetricSource> {
    catalog(
        "AWS/RDS",
        "DBInstanceIdentifier",
        instance_id,
        &[
            ("CPUUtilization", "Average", "%"),
            ("FreeableMemory", "Minimum", "By"),
            ("DatabaseConnections", "Maximum", "{connection}"),
            ("ReadIOPS", "Average", "{operation}/s"),
            ("WriteIOPS", "Average", "{operation}/s"),
            ("ReadLatency", "Average", "s"),
            ("WriteLatency", "Average", "s"),
        ],
    )
}

pub fn application_load_balancer(full_dimension: &str) -> Vec<AwsMetricSource> {
    catalog(
        "AWS/ApplicationELB",
        "LoadBalancer",
        full_dimension,
        &[
            ("RequestCount", "Sum", "{request}"),
            ("HTTPCode_ELB_5XX_Count", "Sum", "{response}"),
            ("HTTPCode_Target_5XX_Count", "Sum", "{response}"),
            ("TargetResponseTime", "Average", "s"),
            ("TargetConnectionErrorCount", "Sum", "{error}"),
            ("ProcessedBytes", "Sum", "By"),
        ],
    )
}

pub fn nat_gateway(id: &str) -> Vec<AwsMetricSource> {
    catalog(
        "AWS/NATGateway",
        "NatGatewayId",
        id,
        &[
            ("ErrorPortAllocation", "Sum", "{error}"),
            ("PacketsDropCount", "Sum", "{packet}"),
            ("ActiveConnectionCount", "Maximum", "{connection}"),
            ("BytesInFromSource", "Sum", "By"),
            ("BytesOutToDestination", "Sum", "By"),
        ],
    )
}

/// Basic EC2 uses five-minute resolution; enabling detailed monitoring is separate.
pub fn ec2(instance_id: &str) -> Vec<AwsMetricSource> {
    let mut sources = catalog(
        "AWS/EC2",
        "InstanceId",
        instance_id,
        &[
            ("CPUUtilization", "Average", "%"),
            ("NetworkIn", "Sum", "By"),
            ("NetworkOut", "Sum", "By"),
            ("StatusCheckFailed", "Maximum", "{failure}"),
        ],
    );
    for source in &mut sources {
        source.period_seconds = 300;
    }
    sources
}

fn catalog(
    namespace: &str,
    dimension: &str,
    identity: &str,
    metrics: &[(&str, &str, &str)],
) -> Vec<AwsMetricSource> {
    metrics
        .iter()
        .map(|(name, statistic, unit)| {
            source(
                namespace,
                name,
                statistic,
                unit,
                60,
                BTreeMap::from([(dimension.into(), identity.into())]),
            )
        })
        .collect()
}

fn source(
    namespace: &str,
    name: &str,
    statistic: &str,
    unit: &str,
    period_seconds: i32,
    dimensions: BTreeMap<String, String>,
) -> AwsMetricSource {
    AwsMetricSource {
        namespace: namespace.into(),
        metric_name: name.into(),
        statistic: statistic.into(),
        unit: unit.into(),
        period_seconds,
        dimensions,
    }
}
