//! Settings of the CloudWatch metric alarms on AWS resources, shared by the runtime controllers and the
//! CloudFormation and Terraform setup emitters, so every path puts the same alarms.
//!
//! The alarms have no actions, so nothing is notified, and missing data counts as healthy: a
//! paused Aurora writer or an idle load balancer reports no datapoints.

use serde::Serialize;

/// How every alarm treats periods without datapoints.
pub const TREAT_MISSING_DATA: &str = "notBreaching";

/// One plain metric alarm on one resource.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricAlarm {
    /// Ends the alarm name, after the name of the resource it watches.
    pub name_suffix: &'static str,
    pub description: &'static str,
    pub namespace: &'static str,
    pub metric_name: &'static str,
    /// Names of the dimensions; the caller supplies the values of the watched resource.
    pub dimensions: &'static [&'static str],
    pub statistic: &'static str,
    pub period_secs: i32,
    pub evaluation_periods: i32,
    pub threshold: f64,
    pub comparison_operator: &'static str,
}

/// Dimension naming the Aurora writer instance.
pub const DB_INSTANCE_DIMENSION: &str = "DBInstanceIdentifier";

/// Alarms on the writer instance of an Aurora Postgres cluster, named
/// `<prefix>-<resource id>-<suffix>`.
pub const AURORA_WRITER_ALARMS: &[MetricAlarm] = &[
    MetricAlarm {
        name_suffix: "writer-cpu",
        description: "The database writer uses more than 90% CPU for 15 minutes",
        namespace: "AWS/RDS",
        metric_name: "CPUUtilization",
        dimensions: &[DB_INSTANCE_DIMENSION],
        statistic: "Average",
        period_secs: 300,
        evaluation_periods: 3,
        threshold: 90.0,
        comparison_operator: "GreaterThanThreshold",
    },
    MetricAlarm {
        name_suffix: "writer-free-memory",
        description: "The database writer has less than 256 MiB of freeable memory for 15 minutes",
        namespace: "AWS/RDS",
        metric_name: "FreeableMemory",
        dimensions: &[DB_INSTANCE_DIMENSION],
        statistic: "Average",
        period_secs: 300,
        evaluation_periods: 3,
        threshold: 268_435_456.0,
        comparison_operator: "LessThanThreshold",
    },
    MetricAlarm {
        name_suffix: "writer-disk-queue",
        description:
            "More than 64 I/O requests wait for the database writer's storage for 15 minutes",
        namespace: "AWS/RDS",
        metric_name: "DiskQueueDepth",
        dimensions: &[DB_INSTANCE_DIMENSION],
        statistic: "Average",
        period_secs: 300,
        evaluation_periods: 3,
        threshold: 64.0,
        comparison_operator: "GreaterThanThreshold",
    },
];

pub const LOAD_BALANCER_DIMENSION: &str = "LoadBalancer";
pub const TARGET_GROUP_DIMENSION: &str = "TargetGroup";

/// Alarms on an application load balancer and its target group, named
/// `<prefix>-<resource id>-<suffix>`.
pub const APPLICATION_LOAD_BALANCER_ALARMS: &[MetricAlarm] = &[
    MetricAlarm {
        name_suffix: "alb-unhealthy-hosts",
        description: "Targets behind the load balancer fail health checks for 5 minutes",
        namespace: "AWS/ApplicationELB",
        metric_name: "UnHealthyHostCount",
        dimensions: &[TARGET_GROUP_DIMENSION, LOAD_BALANCER_DIMENSION],
        statistic: "Maximum",
        period_secs: 60,
        evaluation_periods: 5,
        threshold: 1.0,
        comparison_operator: "GreaterThanOrEqualToThreshold",
    },
    MetricAlarm {
        name_suffix: "alb-latency",
        description: "Targets take more than 5 seconds to respond on average for 15 minutes",
        namespace: "AWS/ApplicationELB",
        metric_name: "TargetResponseTime",
        dimensions: &[LOAD_BALANCER_DIMENSION],
        statistic: "Average",
        period_secs: 300,
        evaluation_periods: 3,
        threshold: 5.0,
        comparison_operator: "GreaterThanThreshold",
    },
    MetricAlarm {
        name_suffix: "alb-target-5xx",
        description: "Targets return more than 50 server errors in each of three 5-minute periods",
        namespace: "AWS/ApplicationELB",
        metric_name: "HTTPCode_Target_5XX_Count",
        dimensions: &[LOAD_BALANCER_DIMENSION],
        statistic: "Sum",
        period_secs: 300,
        evaluation_periods: 3,
        threshold: 50.0,
        comparison_operator: "GreaterThanThreshold",
    },
];

pub const AUTO_SCALING_GROUP_DIMENSION: &str = "AutoScalingGroupName";

/// Alarm on the machines of one Auto Scaling group, named `<asg name>-<suffix>`.
pub const AUTO_SCALING_GROUP_CPU_ALARM: MetricAlarm = MetricAlarm {
    name_suffix: "cpu",
    description: "The group's machines use more than 90% CPU on average for 15 minutes",
    namespace: "AWS/EC2",
    metric_name: "CPUUtilization",
    dimensions: &[AUTO_SCALING_GROUP_DIMENSION],
    statistic: "Average",
    period_secs: 300,
    evaluation_periods: 3,
    threshold: 90.0,
    comparison_operator: "GreaterThanThreshold",
};
