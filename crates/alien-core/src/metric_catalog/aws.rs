use crate::AwsMetricSource;
use std::collections::BTreeMap;

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
        monotonic: stat == "Sum" || stat == "SampleCount",
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
        monotonic: statistic == "Sum" || statistic == "SampleCount",
    }
}
