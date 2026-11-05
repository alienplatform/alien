use crate::AzureMetricSource;

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
    let mut capacity = native(
        blob_service_uri,
        "Microsoft.Storage/storageAccounts/blobServices",
        &["BlobCapacity", "BlobCount", "ContainerCount"],
        "Average",
    );
    capacity.interval_seconds = 3600;
    vec![
        capacity,
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

/// Basic platform metrics; does not enable advanced PostgreSQL diagnostics.
pub fn postgres(resource_uri: &str) -> Vec<AzureMetricSource> {
    vec![native(
        resource_uri,
        "Microsoft.DBforPostgreSQL/flexibleServers",
        &[
            "cpu_percent",
            "memory_percent",
            "storage_percent",
            "active_connections",
        ],
        "Average",
    )]
}

pub fn nat_gateway(resource_uri: &str) -> Vec<AzureMetricSource> {
    vec![
        native(
            resource_uri,
            "Microsoft.Network/natGateways",
            &["ByteCount", "PacketCount", "PacketDropCount"],
            "Total",
        ),
        native(
            resource_uri,
            "Microsoft.Network/natGateways",
            &["DatapathAvailability"],
            "Average",
        ),
    ]
}

pub fn scale_set(resource_uri: &str) -> Vec<AzureMetricSource> {
    vec![
        native(
            resource_uri,
            "Microsoft.Compute/virtualMachineScaleSets",
            &["Percentage CPU"],
            "Average",
        ),
        native(
            resource_uri,
            "Microsoft.Compute/virtualMachineScaleSets",
            &[
                "Network In Total",
                "Network Out Total",
                "Disk Read Bytes",
                "Disk Write Bytes",
            ],
            "Total",
        ),
    ]
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
        monotonic: matches!(aggregation, "Total" | "Count"),
        interval_seconds: 60,
        filter: None,
    }
}
