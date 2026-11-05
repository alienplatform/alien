use crate::GcpMetricSource;

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

/// Cloud SQL native instance metrics; advanced query insights remain separate.
pub fn cloud_sql(project: &str, instance: &str) -> Vec<GcpMetricSource> {
    let identity = format!("{project}:{instance}");
    [
        "database/cpu/utilization",
        "database/memory/utilization",
        "database/disk/utilization",
        "database/postgresql/num_backends",
        "database/network/received_bytes_count",
        "database/network/sent_bytes_count",
    ]
    .into_iter()
    .map(|metric| {
        resource_source(
            project,
            &format!("cloudsql.googleapis.com/{metric}"),
            "cloudsql_database",
            &[("database_id", &identity)],
        )
    })
    .collect()
}

/// Native pool size, without enabling predictive autoscaling or guest agents.
pub fn instance_group(project: &str, group: &str, location: &str) -> Vec<GcpMetricSource> {
    vec![resource_source(
        project,
        "compute.googleapis.com/instance_group/size",
        "instance_group",
        &[("instance_group_name", group), ("location", location)],
    )]
}

pub fn nat_gateway(
    project: &str,
    region: &str,
    router_id: &str,
    gateway: &str,
) -> Vec<GcpMetricSource> {
    [
        "nat/sent_bytes_count",
        "nat/received_bytes_count",
        "nat/dropped_sent_packets_count",
        "nat/allocated_ports",
    ]
    .into_iter()
    .map(|metric| {
        resource_source(
            project,
            &format!("router.googleapis.com/{metric}"),
            "nat_gateway",
            &[
                ("region", region),
                ("router_id", router_id),
                ("gateway_name", gateway),
            ],
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
        monotonic: metric.ends_with("_count"),
        // Capacity is measured daily, then repeated every five minutes with up to
        // 690 seconds of reporting delay. Other selected native families fit 300s.
        minimum_lag_seconds: if metric.starts_with("storage.googleapis.com/storage/") {
            900
        } else {
            300
        },
    }
}
