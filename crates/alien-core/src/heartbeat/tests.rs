use super::*;
use chrono::TimeZone as _;
use serde_json::json;

fn observed_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 28, 10, 30, 0).unwrap()
}

fn workload_status() -> WorkloadHeartbeatStatus {
    WorkloadHeartbeatStatus {
        health: ObservedHealth::Healthy,
        lifecycle: ProviderLifecycleState::Running,
        message: None,
        stale: false,
        partial: false,
        collection_issues: vec![],
    }
}

fn workload_replicas() -> WorkloadReplicaStatus {
    WorkloadReplicaStatus {
        desired: Some(2),
        current: Some(2),
        ready: Some(1),
        available: Some(1),
        updated: Some(2),
        misscheduled: None,
    }
}

fn heartbeat(data: ResourceHeartbeatData, resource_type: &str) -> ResourceHeartbeat {
    ResourceHeartbeat {
        deployment_id: Some("dep_123".to_string()),
        resource_id: "api".to_string(),
        resource_type: ResourceType::from(resource_type),
        controller_platform: Platform::Kubernetes,
        backend: HeartbeatBackend::Kubernetes,
        observed_at: observed_at(),
        data,
        raw: vec![RawHeartbeatSnippet {
            source: "kubernetes/apps/v1/deployments/api".to_string(),
            format: RawHeartbeatSnippetFormat::Json,
            collected_at: observed_at(),
            body: r#"{"readyReplicas":1}"#.to_string(),
            truncated: false,
        }],
    }
}

#[test]
fn resource_heartbeat_roundtrips_managed_resource_data() {
    let heartbeat_json = serde_json::to_value(heartbeat(
        ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
            KubernetesContainerHeartbeatData {
                status: workload_status(),
                namespace: "default".to_string(),
                name: "api".to_string(),
                workload_kind: KubernetesWorkloadKind::Deployment,
                replicas: workload_replicas(),
                restarts: None,
                cpu: None,
                memory: None,
                workload: None,
                pods: vec![],
                events: vec![],
            },
        )),
        "container",
    ))
    .unwrap();

    let parsed: ResourceHeartbeat = serde_json::from_value(heartbeat_json).unwrap();
    assert_eq!(parsed.resource_id, "api");
    assert!(matches!(
        parsed.data,
        ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(_))
    ));
}

#[test]
fn local_daemon_heartbeat_defaults_missing_daemon_name() {
    let mut value = serde_json::to_value(DaemonHeartbeatData::Local(LocalDaemonHeartbeatData {
        status: workload_status(),
        daemon_name: "agent".to_string(),
        runtime_id: "local-agent".to_string(),
        pid: None,
        command_supported: false,
        image_path_present: true,
        restart_count: None,
        exit_reason: None,
        daemon_instance: None,
        events: vec![],
    }))
    .unwrap();

    value.as_object_mut().unwrap().remove("daemonName");

    let parsed: DaemonHeartbeatData = serde_json::from_value(value).unwrap();
    match parsed {
        DaemonHeartbeatData::Local(data) => assert_eq!(data.daemon_name, ""),
        other => panic!("expected local daemon heartbeat, got {other:?}"),
    }
}

#[test]
fn container_heartbeat_serializes_resource_first_data() {
    let heartbeat = heartbeat(
        ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
            KubernetesContainerHeartbeatData {
                status: workload_status(),
                namespace: "default".to_string(),
                name: "api".to_string(),
                workload_kind: KubernetesWorkloadKind::Deployment,
                replicas: workload_replicas(),
                restarts: Some(1),
                cpu: Some(MetricSample {
                    value: 0.5,
                    unit: MetricUnit::Cores,
                }),
                memory: None,
                workload: None,
                pods: vec![],
                events: vec![],
            },
        )),
        "container",
    );

    let value = serde_json::to_value(&heartbeat).unwrap();

    assert_eq!(value["resourceType"], "container");
    assert_eq!(value["data"]["resourceType"], "container");
    assert_eq!(value["data"]["data"]["backend"], "kubernetes");
    assert_eq!(value["raw"][0]["body"], r#"{"readyReplicas":1}"#);
    assert!(value.get("collection").is_none());
    assert!(value["data"]["data"].get("summary").is_none());
    assert!(value["data"]["data"].get("detail").is_none());
}

#[test]
fn representative_workload_data_has_stable_tags() {
    let daemon = serde_json::to_value(ResourceHeartbeatData::Daemon(
        DaemonHeartbeatData::Kubernetes(KubernetesDaemonHeartbeatData {
            status: workload_status(),
            namespace: "default".to_string(),
            name: "agent".to_string(),
            replicas: workload_replicas(),
            restarts: Some(0),
            command_supported: true,
            cpu: None,
            memory: None,
            workload: None,
            pods: vec![],
            events: vec![],
        }),
    ))
    .unwrap();
    let worker = serde_json::to_value(ResourceHeartbeatData::Worker(
        WorkerHeartbeatData::AwsLambda(AwsLambdaWorkerHeartbeatData {
            status: workload_status(),
            function_name: "handler".to_string(),
            runtime: Some("nodejs22.x".to_string()),
            package_type: None,
            memory_size_mb: None,
            timeout_seconds: None,
            version: None,
            revision_id: None,
            last_modified: None,
            state: None,
            state_reason: None,
            state_reason_code: None,
            last_update_status: None,
            last_update_status_reason: None,
            last_update_status_reason_code: None,
            code_sha256: None,
            layer_count: 0,
            function_url_auth_type: None,
            function_url_cors_present: false,
            trigger_count: 0,
        }),
    ))
    .unwrap();

    assert_eq!(daemon["resourceType"], "daemon");
    assert_eq!(daemon["data"]["backend"], "kubernetes");
    assert_eq!(worker["resourceType"], "worker");
    assert_eq!(worker["data"]["backend"], "awsLambda");
}

#[test]
fn representative_cluster_and_data_variants_have_optional_counts() {
    let cluster = serde_json::to_value(ResourceHeartbeatData::KubernetesCluster(
        KubernetesClusterHeartbeatData {
            status: WorkloadHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            node_counts: ObservedCounts {
                desired: Some(3),
                current: Some(3),
                ready: None,
            },
            pod_counts: ObservedCounts {
                desired: None,
                current: Some(12),
                ready: Some(11),
            },
            cpu: None,
            memory: None,
            name: "prod".to_string(),
            region: Some("us-east-1".to_string()),
            namespace: None,
            version: Some("1.33".to_string()),
            node_statuses: vec![],
            events: vec![],
        },
    ))
    .unwrap();
    let queue = serde_json::to_value(ResourceHeartbeatData::Queue(QueueHeartbeatData::AwsSqs(
        AwsSqsQueueHeartbeatData {
            status: QueueHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: "jobs".to_string(),
            region: Some("us-east-1".to_string()),
            queue_url: Some("https://sqs.us-east-1.amazonaws.com/123/jobs".to_string()),
            queue_arn: Some("arn:aws:sqs:us-east-1:123:jobs".to_string()),
            visibility_timeout_seconds: Some(30),
            message_retention_period_seconds: Some(345600),
            delay_seconds: Some(0),
            receive_message_wait_time_seconds: Some(0),
            maximum_message_size: Some(262144),
            redrive_policy: None,
            redrive_allow_policy: None,
            fifo_queue: Some(false),
            content_based_deduplication: None,
            deduplication_scope: None,
            fifo_throughput_limit: None,
            sse_enabled: Some(false),
            kms_master_key_id: None,
            kms_data_key_reuse_period_seconds: None,
            sqs_managed_sse_enabled: Some(false),
            approximate_visible_messages: Some(42),
            approximate_in_flight_messages: Some(1),
            approximate_delayed_messages: Some(0),
            approximate_counts: true,
        },
    )))
    .unwrap();
    let storage = serde_json::to_value(ResourceHeartbeatData::Storage(
        StorageHeartbeatData::AwsS3(AwsS3StorageHeartbeatData {
            status: StorageHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: "assets".to_string(),
            region: Some("us-east-1".to_string()),
            bucket_location: Some("us-east-1".to_string()),
            versioning_status: Some("Enabled".to_string()),
            versioning_enabled: Some(true),
            lifecycle_present: false,
            lifecycle_rule_count: Some(0),
            encryption_config_present: true,
            encryption_enabled: Some(true),
            public_access_block_present: true,
            block_public_acls: Some(true),
            ignore_public_acls: Some(true),
            block_public_policy: Some(true),
            restrict_public_buckets: Some(true),
            bucket_policy_present: Some(false),
            bucket_acl_present: Some(true),
        }),
    ))
    .unwrap();
    let gcp_storage = serde_json::to_value(ResourceHeartbeatData::Storage(
        StorageHeartbeatData::GcpCloudStorage(GcpCloudStorageHeartbeatData {
            status: StorageHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: "assets".to_string(),
            bucket_id: Some("project/assets".to_string()),
            location: Some("US".to_string()),
            location_type: Some("multi-region".to_string()),
            storage_class: Some("STANDARD".to_string()),
            versioning_enabled: Some(true),
            lifecycle_present: false,
            lifecycle_rule_count: Some(0),
            retention_policy_effective_time: None,
            retention_policy_is_locked: None,
            retention_period: None,
            soft_delete_retention_duration_seconds: None,
            soft_delete_effective_time: None,
            uniform_bucket_level_access_enabled: Some(true),
            uniform_bucket_level_access_locked_time: None,
            public_access_prevention: Some("enforced".to_string()),
            encryption_config_present: true,
            default_kms_key_name: Some(
                "projects/p/locations/l/keyRings/r/cryptoKeys/k".to_string(),
            ),
        }),
    ))
    .unwrap();
    let kv = serde_json::to_value(ResourceHeartbeatData::Kv(KvHeartbeatData::AwsDynamoDb(
        AwsDynamoDbKvHeartbeatData {
            status: KvHeartbeatStatus {
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name: "state".to_string(),
            region: Some("us-east-1".to_string()),
            table_arn: None,
            table_status: Some("ACTIVE".to_string()),
            billing_mode: Some("PAY_PER_REQUEST".to_string()),
            key_schema: vec![AwsDynamoDbKeySchemaElement {
                attribute_name: "pk".to_string(),
                key_type: "HASH".to_string(),
            }],
            global_secondary_index_count: Some(0),
            local_secondary_index_count: Some(0),
            item_count: None,
            table_size_bytes: None,
            stream_enabled: Some(false),
            stream_view_type: None,
            ttl_status: Some("ENABLED".to_string()),
            ttl_attribute_name: Some("ttl".to_string()),
            deletion_protection_enabled: Some(false),
            sse_status: Some("ENABLED".to_string()),
            sse_type: Some("KMS".to_string()),
            table_class: None,
            replica_count: Some(0),
            restore_in_progress: None,
        },
    )))
    .unwrap();

    assert_eq!(cluster["resourceType"], "kubernetes-cluster");
    assert!(cluster["data"].get("summary").is_none());
    assert!(cluster["data"].get("detail").is_none());
    assert_eq!(cluster["data"]["name"], "prod");
    assert_eq!(queue["data"]["backend"], "awsSqs");
    assert_eq!(queue["data"]["approximateVisibleMessages"], 42);
    assert!(queue["data"].get("summary").is_none());
    assert_eq!(storage["data"]["backend"], "awsS3");
    assert!(storage["data"].get("summary").is_none());
    assert_eq!(gcp_storage["data"]["backend"], "gcpCloudStorage");
    assert_eq!(gcp_storage["data"]["publicAccessPrevention"], "enforced");
    assert_eq!(kv["data"]["backend"], "awsDynamoDb");
    assert!(kv["data"].get("summary").is_none());
    assert_eq!(kv["data"]["itemCount"], json!(null));
}
