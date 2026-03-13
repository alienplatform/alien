//! Mock data generators for TUI storybook demos

use alien_core::{
    ContainerOutputs, DeploymentModel, EnvironmentInfo, FunctionOutputs, HeartbeatsMode, KvOutputs,
    LocalEnvironmentInfo, QueueOutputs, ResourceLifecycle, ResourceOutputs, ResourceStatus,
    ResourceType, StackSettings, StorageOutputs, TelemetryMode, UpdatesMode,
};
use alien_error::{AlienError, GenericError};
use chrono::{Duration, Utc};

use alien_cli::tui::state::{
    CommandItem, CommandState, DeploymentGroupItem, DeploymentItem, DeploymentMetadata,
    DeploymentPlatform, DeploymentStatus, LogLine, PackageItem, PackageStatus, ReleaseItem,
    ResourceInfo,
};

// ============ Deployments ============

pub fn mock_deployments(count: usize) -> Vec<DeploymentItem> {
    (0..count)
        .map(|i| DeploymentItem {
            id: format!("dpl_{:024}", i),
            name: format!("deployment-{}", i + 1),
            deployment_group_id: format!("dg_{:024}", i % 3),
            deployment_group_name: Some(format!("group-{}", i % 3)),
            status: DeploymentStatus::Running,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        })
        .collect()
}

pub fn mock_deployments_various_statuses() -> Vec<DeploymentItem> {
    vec![
        DeploymentItem {
            id: "dpl_000000000000000000000001".to_string(),
            name: "production-api".to_string(),
            deployment_group_id: "dg_production".to_string(),
            deployment_group_name: Some("production".to_string()),
            status: DeploymentStatus::Running,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000002".to_string(),
            name: "staging-api".to_string(),
            deployment_group_id: "dg_staging".to_string(),
            deployment_group_name: Some("staging".to_string()),
            status: DeploymentStatus::Updating,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000003".to_string(),
            name: "dev-worker".to_string(),
            deployment_group_id: "dg_dev".to_string(),
            deployment_group_name: Some("dev".to_string()),
            status: DeploymentStatus::Provisioning,
            platform: DeploymentPlatform::Gcp,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000004".to_string(),
            name: "test-runner".to_string(),
            deployment_group_id: "dg_test".to_string(),
            deployment_group_name: Some("test".to_string()),
            status: DeploymentStatus::InitialSetupFailed,
            platform: DeploymentPlatform::Azure,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000005".to_string(),
            name: "local-dev".to_string(),
            deployment_group_id: "dg_local".to_string(),
            deployment_group_name: Some("local".to_string()),
            status: DeploymentStatus::Running,
            platform: DeploymentPlatform::Local,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000006".to_string(),
            name: "batch-processor".to_string(),
            deployment_group_id: "dg_production".to_string(),
            deployment_group_name: Some("production".to_string()),
            status: DeploymentStatus::UpdateFailed,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000007".to_string(),
            name: "data-pipeline".to_string(),
            deployment_group_id: "dg_production".to_string(),
            deployment_group_name: Some("production".to_string()),
            status: DeploymentStatus::Pending,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        },
    ]
}

pub fn mock_deployments_long_names() -> Vec<DeploymentItem> {
    vec![
        DeploymentItem {
            id: "dpl_000000000000000000000001".to_string(),
            name: "very-long-deployment-name-that-might-overflow-the-column-width".to_string(),
            deployment_group_id: "dg_with_extremely_long_deployment_group_name_here".to_string(),
            deployment_group_name: Some("very-long-group-name".to_string()),
            status: DeploymentStatus::Running,
            platform: DeploymentPlatform::Aws,
            release_info: None,
        },
        DeploymentItem {
            id: "dpl_000000000000000000000002".to_string(),
            name: "another-extremely-long-name-for-testing-purposes".to_string(),
            deployment_group_id: "dg_short".to_string(),
            deployment_group_name: Some("short".to_string()),
            status: DeploymentStatus::Updating,
            platform: DeploymentPlatform::Gcp,
            release_info: None,
        },
    ]
}

// ============ Deployment Detail (Logs & Resources) ============

/// Generate mock logs for a specific deployment
pub fn mock_logs(count: usize) -> Vec<LogLine> {
    mock_logs_for_deployment(count, "dpl_demo")
}

/// Generate mock logs for a specific deployment ID
pub fn mock_logs_for_deployment(count: usize, deployment_id: &str) -> Vec<LogLine> {
    let messages = vec![
        ("system", "Starting deployment..."),
        ("api-handler", "Server listening on port 3000"),
        ("api-handler", "[INFO] Request received: GET /api/users"),
        ("api-handler", "[DEBUG] Fetching users from database"),
        ("data-processor", "[INFO] Processing batch of 100 items"),
        ("data-processor", "[WARN] Slow query detected: 2.3s"),
        ("api-handler", "[ERROR] Database connection timeout"),
        ("scheduler", "[INFO] Job scheduled: cleanup-old-data"),
        ("api-handler", "[INFO] Request completed: 200 OK"),
        ("system", "Health check passed"),
    ];

    (0..count)
        .map(|i| {
            let (resource_id, content) = &messages[i % messages.len()];
            LogLine::new(deployment_id, *resource_id, *content)
        })
        .collect()
}

pub fn mock_resources() -> Vec<ResourceInfo> {
    use alien_core::ContainerStatus;

    vec![
        ResourceInfo {
            id: "api-handler".to_string(),
            resource_type: ResourceType::from("function"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(FunctionOutputs {
                function_name: "demo-api-handler".to_string(),
                url: Some("http://localhost:3000".to_string()),
                identifier: None,
                load_balancer_endpoint: None,
            })),
        },
        ResourceInfo {
            id: "web-container".to_string(),
            resource_type: ResourceType::from("container"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(ContainerOutputs {
                name: "demo-web".to_string(),
                status: ContainerStatus::Running,
                current_replicas: 3,
                desired_replicas: 3,
                internal_dns: "web.svc".to_string(),
                url: Some("http://localhost:8080".to_string()),
                replicas: vec![],
                load_balancer_endpoint: None,
            })),
        },
        ResourceInfo {
            id: "user-storage".to_string(),
            resource_type: ResourceType::from("storage"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(StorageOutputs {
                bucket_name: "demo-user-storage".to_string(),
            })),
        },
        ResourceInfo {
            id: "session-kv".to_string(),
            resource_type: ResourceType::from("kv"),
            lifecycle: ResourceLifecycle::Frozen,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(KvOutputs {
                store_name: "demo-sessions".to_string(),
                identifier: None,
                endpoint: None,
            })),
        },
        ResourceInfo {
            id: "job-queue".to_string(),
            resource_type: ResourceType::from("queue"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Provisioning,
            outputs: None,
        },
    ]
}

pub fn mock_resources_various_statuses() -> Vec<ResourceInfo> {
    vec![
        ResourceInfo {
            id: "api-handler".to_string(),
            resource_type: ResourceType::from("function"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Running,
            outputs: Some(ResourceOutputs::new(FunctionOutputs {
                function_name: "demo-api-handler".to_string(),
                url: Some("http://localhost:3000".to_string()),
                identifier: None,
                load_balancer_endpoint: None,
            })),
        },
        ResourceInfo {
            id: "worker".to_string(),
            resource_type: ResourceType::from("function"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Provisioning,
            outputs: None,
        },
        ResourceInfo {
            id: "failed-service".to_string(),
            resource_type: ResourceType::from("function"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::ProvisionFailed,
            outputs: None,
        },
        ResourceInfo {
            id: "user-storage".to_string(),
            resource_type: ResourceType::from("storage"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Updating,
            outputs: Some(ResourceOutputs::new(StorageOutputs {
                bucket_name: "demo-user-storage".to_string(),
            })),
        },
        ResourceInfo {
            id: "old-service".to_string(),
            resource_type: ResourceType::from("function"),
            lifecycle: ResourceLifecycle::Live,
            status: ResourceStatus::Deleting,
            outputs: None,
        },
    ]
}

/// Generate many resources for scrolling demo
pub fn mock_many_resources(count: usize) -> Vec<ResourceInfo> {
    (0..count)
        .map(|i| {
            let resource_type = match i % 5 {
                0 => "function",
                1 => "container",
                2 => "storage",
                3 => "kv",
                _ => "queue",
            };

            let outputs = match resource_type {
                "function" => Some(ResourceOutputs::new(FunctionOutputs {
                    function_name: format!("demo-func-{}", i),
                    url: Some(format!("http://localhost:{}", 3000 + i)),
                    identifier: None,
                    load_balancer_endpoint: None,
                })),
                "storage" => Some(ResourceOutputs::new(StorageOutputs {
                    bucket_name: format!("demo-bucket-{}", i),
                })),
                "kv" => Some(ResourceOutputs::new(KvOutputs {
                    store_name: format!("demo-kv-{}", i),
                    identifier: None,
                    endpoint: None,
                })),
                "queue" => Some(ResourceOutputs::new(QueueOutputs {
                    queue_name: format!("demo-queue-{}", i),
                    identifier: None,
                })),
                _ => None,
            };

            ResourceInfo {
                id: format!("resource-{}", i),
                resource_type: ResourceType::from(resource_type),
                lifecycle: if i % 7 == 0 {
                    ResourceLifecycle::Frozen
                } else {
                    ResourceLifecycle::Live
                },
                status: ResourceStatus::Running,
                outputs,
            }
        })
        .collect()
}

/// Generate mock deployment metadata
pub fn mock_deployment_metadata() -> DeploymentMetadata {
    DeploymentMetadata {
        created_at: Utc::now().to_rfc3339(),
        platform: DeploymentPlatform::Local,
        stack_settings: StackSettings {
            network: None,
            domains: None,
            deployment_model: DeploymentModel::Pull,
            updates: UpdatesMode::Auto,
            telemetry: TelemetryMode::Auto,
            heartbeats: HeartbeatsMode::On,
        },
        environment_info: Some(EnvironmentInfo::Local(LocalEnvironmentInfo {
            hostname: "macbook-pro.local".to_string(),
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
        })),
        current_release_id: Some("rel_000000000000000000000001".to_string()),
        error: None,
    }
}

/// Generate mock deployment metadata with error
pub fn mock_deployment_metadata_with_error() -> DeploymentMetadata {
    // Create a realistic error with context and source chain
    let source_error = AlienError::new(GenericError {
        message: "Connection timeout after 30s".to_string(),
    });

    let mut context = serde_json::Map::new();
    context.insert(
        "resource_id".to_string(),
        serde_json::Value::String("api-handler".to_string()),
    );
    context.insert(
        "operation".to_string(),
        serde_json::Value::String("provision".to_string()),
    );
    context.insert(
        "timeout_seconds".to_string(),
        serde_json::Value::Number(30.into()),
    );

    let mut main_error = AlienError::new(GenericError {
        message: "Failed to provision function".to_string(),
    });
    main_error.code = "PROVISION_FAILED".to_string();
    main_error.context = Some(serde_json::Value::Object(context));
    main_error.retryable = true;
    main_error.source = Some(Box::new(source_error));

    DeploymentMetadata {
        created_at: Utc::now().to_rfc3339(),
        platform: DeploymentPlatform::Local,
        stack_settings: StackSettings {
            network: None,
            domains: None,
            deployment_model: DeploymentModel::Pull,
            updates: UpdatesMode::Auto,
            telemetry: TelemetryMode::Auto,
            heartbeats: HeartbeatsMode::On,
        },
        environment_info: Some(EnvironmentInfo::Local(LocalEnvironmentInfo {
            hostname: "macbook-pro.local".to_string(),
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
        })),
        current_release_id: Some("rel_000000000000000000000001".to_string()),
        error: Some(main_error),
    }
}

// ============ Deployment Groups ============

pub fn mock_deployment_groups(count: usize) -> Vec<DeploymentGroupItem> {
    (0..count)
        .map(|i| DeploymentGroupItem {
            id: format!("dg_{:024}", i),
            name: format!("deployment-group-{}", i + 1),
            max_deployments: ((i % 5) + 1) as u64 * 10,
            created_at: Utc::now() - Duration::days(i as i64),
        })
        .collect()
}

// ============ Commands ============

pub fn mock_commands(count: usize) -> Vec<CommandItem> {
    let states = [
        CommandState::Pending,
        CommandState::Dispatched,
        CommandState::Succeeded,
        CommandState::Failed,
        CommandState::Expired,
    ];

    (0..count)
        .map(|i| CommandItem {
            id: format!("cmd_{:024}", i),
            name: format!("run-task-{}", i + 1),
            state: states[i % states.len()].clone(),
            deployment_id: format!("dpl_{:024}", i % 5),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now() - Duration::hours(i as i64),
        })
        .collect()
}

pub fn mock_commands_all_states() -> Vec<CommandItem> {
    vec![
        CommandItem {
            id: "cmd_000000000000000000000001".to_string(),
            name: "deploy-production".to_string(),
            state: CommandState::Succeeded,
            deployment_id: "dpl_production".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now() - Duration::hours(1),
        },
        CommandItem {
            id: "cmd_000000000000000000000002".to_string(),
            name: "run-migrations".to_string(),
            state: CommandState::Dispatched,
            deployment_id: "dpl_staging".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now() - Duration::minutes(5),
        },
        CommandItem {
            id: "cmd_000000000000000000000003".to_string(),
            name: "cleanup-cache".to_string(),
            state: CommandState::Pending,
            deployment_id: "dpl_dev".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now(),
        },
        CommandItem {
            id: "cmd_000000000000000000000004".to_string(),
            name: "backup-database".to_string(),
            state: CommandState::Failed,
            deployment_id: "dpl_production".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now() - Duration::days(1),
        },
        CommandItem {
            id: "cmd_000000000000000000000005".to_string(),
            name: "sync-data".to_string(),
            state: CommandState::Expired,
            deployment_id: "dpl_batch".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            created_at: Utc::now() - Duration::days(7),
        },
    ]
}

// ============ Releases ============

pub fn mock_releases(count: usize) -> Vec<ReleaseItem> {
    (0..count)
        .map(|i| ReleaseItem {
            id: format!("rel_{:024}", i),
            project_id: format!("prj_{:024}", i % 3),
            created_at: Utc::now() - Duration::hours(i as i64 * 2),
        })
        .collect()
}

// ============ Packages ============

pub fn mock_packages(count: usize) -> Vec<PackageItem> {
    let statuses = [
        PackageStatus::Ready,
        PackageStatus::Building,
        PackageStatus::Pending,
        PackageStatus::Failed,
    ];

    let types = ["Cli", "Cloudformation", "Helm", "Terraform"];

    (0..count)
        .map(|i| PackageItem {
            id: format!("pkg_{:024}", i),
            type_display: types[i % types.len()].to_string(),
            version: format!("1.{}.0", i),
            status: statuses[i % statuses.len()].clone(),
            created_at: Utc::now() - Duration::hours(i as i64),
        })
        .collect()
}

pub fn mock_packages_all_statuses() -> Vec<PackageItem> {
    vec![
        PackageItem {
            id: "pkg_000000000000000000000001".to_string(),
            type_display: "Cli".to_string(),
            version: "2.1.0".to_string(),
            status: PackageStatus::Ready,
            created_at: Utc::now() - Duration::hours(1),
        },
        PackageItem {
            id: "pkg_000000000000000000000002".to_string(),
            type_display: "Cloudformation".to_string(),
            version: "2.0.5".to_string(),
            status: PackageStatus::Building,
            created_at: Utc::now() - Duration::minutes(10),
        },
        PackageItem {
            id: "pkg_000000000000000000000003".to_string(),
            type_display: "Helm".to_string(),
            version: "1.9.0".to_string(),
            status: PackageStatus::Pending,
            created_at: Utc::now(),
        },
        PackageItem {
            id: "pkg_000000000000000000000004".to_string(),
            type_display: "Terraform".to_string(),
            version: "1.8.3".to_string(),
            status: PackageStatus::Failed,
            created_at: Utc::now() - Duration::days(1),
        },
        PackageItem {
            id: "pkg_000000000000000000000005".to_string(),
            type_display: "Cli".to_string(),
            version: "1.7.0".to_string(),
            status: PackageStatus::Canceled,
            created_at: Utc::now() - Duration::days(2),
        },
    ]
}
