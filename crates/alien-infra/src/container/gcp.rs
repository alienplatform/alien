//! GCP Container Controller
//!
//! This module implements the GCP-specific controller for managing Container resources.
//! A Container represents a deployable workload that runs on a ContainerCluster.
//!
//! The controller:
//! - Creates Network Endpoint Groups (NEGs) and Backend Services for public containers
//! - Creates Persistent Disks for stateful containers
//! - Calls Horizon API to create/update/delete containers
//! - Monitors container status via Horizon
//!
//! Container scheduling and replica management is handled by Horizon, not this controller.

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, Container, ContainerCluster, ContainerCode, ContainerOutputs,
    ContainerStatus, DnsRecordStatus, ExposeProtocol, HorizonClusterConfig, ResourceOutputs,
    ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::gcp::compute::{
    Address, AddressType, Backend, BackendService, BackendServiceProtocol, BalancingMode,
    ComputeApi, Disk, ForwardingRule, ForwardingRuleProtocol, HealthCheck, HealthCheckType,
    LoadBalancingScheme, NetworkEndpointGroup, NetworkEndpointType, TargetHttpProxy, UrlMap,
};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::num::NonZeroU64;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::container_cluster::GcpContainerClusterController;
use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, horizon_container_status_to_alien};
use crate::network::GcpNetworkController;

/// Tracks a Persistent Disk created for a stateful container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentDiskState {
    /// The disk name
    pub disk_name: String,
    /// The disk self-link URL
    pub disk_self_link: String,
    /// The zone where the disk exists
    pub zone: String,
    /// The ordinal this disk is for (for stateful containers)
    pub ordinal: u32,
    /// Size in GB
    pub size_gb: u32,
}

/// GCP Container Controller state machine.
///
/// This controller manages the lifecycle of containers via Horizon:
/// - Creates Network Endpoint Groups and Backend Services for public containers
/// - Creates Persistent Disks for stateful containers
/// - Creates containers in Horizon when the ContainerCluster is ready
/// - Updates container configuration via Horizon API
/// - Deletes containers from Horizon during cleanup
#[controller]
pub struct GcpContainerController {
    /// Horizon container name (derived from resource ID)
    pub(crate) container_name: Option<String>,

    /// Current status from Horizon
    pub(crate) horizon_status: Option<ContainerStatus>,

    /// Number of running replicas
    pub(crate) current_replicas: u32,

    /// Public URL (Backend Service IP/DNS if exposed publicly)
    pub(crate) public_url: Option<String>,

    /// Fully qualified domain name (custom or generated)
    pub(crate) fqdn: Option<String>,

    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,

    /// GCP SSL Certificate name (auto-imported or custom)
    pub(crate) ssl_certificate_name: Option<String>,

    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,

    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    /// Health Check name (for Backend Service)
    pub(crate) health_check_name: Option<String>,

    /// Network Endpoint Group name (if exposed publicly)
    pub(crate) neg_name: Option<String>,

    /// Backend Service name (if exposed publicly)
    pub(crate) backend_service_name: Option<String>,

    /// URL map name (if exposed publicly)
    pub(crate) url_map_name: Option<String>,

    /// Target HTTP proxy name (if exposed publicly)
    pub(crate) target_http_proxy_name: Option<String>,

    /// Global forwarding rule name (if exposed publicly)
    pub(crate) forwarding_rule_name: Option<String>,

    /// Global address name (if exposed publicly)
    pub(crate) global_address_name: Option<String>,

    /// Persistent Disks created for persistent storage
    pub(crate) persistent_disks: Vec<PersistentDiskState>,

    /// Number of iterations spent waiting for replicas to become healthy
    #[serde(default)]
    pub(crate) wait_for_replicas_iterations: u32,
}

/// Context for interacting with Horizon API for a specific cluster.
struct HorizonContext<'a> {
    /// Cluster configuration (contains cluster_id)
    cluster: &'a HorizonClusterConfig,
    /// Pre-authenticated Horizon client
    client: horizon_client_sdk::Client,
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    ssl_certificate_name: Option<String>,
    uses_custom_domain: bool,
}

impl GcpContainerController {
    /// Resolve domain information for a public container.
    /// Returns either custom domain config or auto-generated domain from metadata.
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;

        // Check for custom domain configuration
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let ssl_cert_name = custom
                .certificate
                .gcp
                .as_ref()
                .map(|cert| cert.certificate_name.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires a GCP SSL certificate name".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                ssl_certificate_name: Some(ssl_cert_name),
                uses_custom_domain: true,
            });
        }

        // Use auto-generated domain from domain metadata
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            ssl_certificate_name: None,
            uses_custom_domain: false,
        })
    }

    /// Get Horizon context for the given cluster.
    /// Returns the cluster config and an authenticated client.
    fn horizon<'a>(
        ctx: &'a ResourceControllerContext<'_>,
        cluster_resource_id: &str,
    ) -> Result<HorizonContext<'a>> {
        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(alien_core::ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container resources require a Horizon compute backend".to_string(),
                    resource_id: Some(cluster_resource_id.to_string()),
                }))
            }
        };

        let cluster = horizon_config
            .clusters
            .get(cluster_resource_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("No Horizon cluster config for '{}'", cluster_resource_id),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        let client = create_horizon_client(&horizon_config.url, &cluster.management_token)
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Horizon client: {}", e),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        Ok(HorizonContext { cluster, client })
    }

    /// Parse storage size string (e.g., "100Gi", "500GB") to GB.
    fn parse_storage_size_gb(size: &str) -> Result<u32> {
        let size = size.trim();
        let (num_str, unit) = if size.ends_with("Gi") || size.ends_with("GiB") {
            (size.trim_end_matches("GiB").trim_end_matches("Gi"), "Gi")
        } else if size.ends_with("GB") {
            (size.trim_end_matches("GB"), "GB")
        } else if size.ends_with("Ti") || size.ends_with("TiB") {
            (size.trim_end_matches("TiB").trim_end_matches("Ti"), "Ti")
        } else if size.ends_with("TB") {
            (size.trim_end_matches("TB"), "TB")
        } else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Invalid storage size format: {}. Expected format like '100Gi' or '500GB'",
                    size
                ),
                resource_id: None,
            }));
        };

        let num: u32 = num_str.parse().map_err(|_| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Invalid storage size number: {}", num_str),
                resource_id: None,
            })
        })?;

        let gb = match unit {
            "Gi" | "GiB" => num, // GiB is close enough to GB
            "GB" => num,
            "Ti" | "TiB" => num * 1024,
            "TB" => num * 1000,
            _ => num,
        };

        Ok(gb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_core::NetworkSettings;
    use alien_core::{
        CapacityGroup, ComputeBackend, ContainerAutoscaling, EnvironmentVariable,
        EnvironmentVariableType, EnvironmentVariablesSnapshot, HealthCheck, HorizonClusterConfig,
        HorizonConfig, Network, ResourceSpec,
    };
    use alien_gcp_clients::gcp::compute::MockComputeApi;
    use alien_gcp_clients::gcp::compute::{
        Address, Disk, DiskStatus, InstanceGroupManagersListManagedInstancesResponse,
        ManagedInstance, ManagedInstanceStatus, Operation,
    };
    use httpmock::MockServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn setup_horizon_server(
        cluster_id: &str,
        container_name: &str,
        healthy_replicas: u32,
    ) -> MockServer {
        let server = MockServer::start();

        let replica_infos: Vec<serde_json::Value> = (0..healthy_replicas)
            .map(|idx| {
                json!({
                    "replicaId": format!("{}-{}", container_name, idx),
                    "machineId": format!("machine-{}", idx),
                    "ip": format!("10.0.0.{}", idx + 10),
                    "status": "running",
                    "healthy": true,
                    "consecutiveFailures": 0
                })
            })
            .collect();

        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": {
                "cpu": { "min": "1", "desired": "1" },
                "memory": { "min": "1Gi", "desired": "1Gi" }
            },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": replica_infos,
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path(format!("/clusters/{}/containers", cluster_id));
            then.status(200).json_body(create_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(create_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::DELETE).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(json!({ "success": true }));
        });

        server
    }

    fn setup_mock_provider(mock_compute: Arc<MockComputeApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_compute_client()
            .returning(move |_| Ok(mock_compute.clone()));
        Arc::new(provider)
    }

    fn mock_compute_for_create_delete(ip: &str) -> Arc<MockComputeApi> {
        let mut mock = MockComputeApi::new();

        mock.expect_insert_health_check()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_insert_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_target_http_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_global_address()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        let ip = ip.to_string();
        mock.expect_get_global_address().returning(move |_| {
            Ok(Address {
                address: Some(ip.clone()),
                ..Default::default()
            })
        });

        mock.expect_insert_disk()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_get_disk().returning(|_, _| {
            Ok(Disk {
                status: Some(DiskStatus::Ready),
                ..Default::default()
            })
        });

        mock.expect_patch_health_check()
            .returning(|_, _| Ok(Operation::default()));

        mock.expect_delete_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_target_http_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_health_check()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_global_address()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_delete_disk()
            .returning(|_, _| Ok(Operation::default()));

        Arc::new(mock)
    }

    fn mock_compute_for_best_effort_delete() -> Arc<MockComputeApi> {
        let mut mock = MockComputeApi::new();

        let not_found = || {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Compute".to_string(),
                    resource_name: "missing".to_string(),
                },
            ))
        };

        mock.expect_delete_global_forwarding_rule()
            .returning(move |_| not_found());
        mock.expect_delete_target_http_proxy()
            .returning(move |_| not_found());
        mock.expect_delete_url_map().returning(move |_| not_found());
        mock.expect_delete_backend_service()
            .returning(move |_| not_found());
        mock.expect_delete_health_check()
            .returning(move |_| not_found());
        mock.expect_delete_global_address()
            .returning(move |_| not_found());
        mock.expect_delete_network_endpoint_group()
            .returning(move |_, _| not_found());
        mock.expect_delete_disk().returning(move |_, _| not_found());

        Arc::new(mock)
    }

    fn test_horizon_config(server: &MockServer, cluster_id: &str) -> ComputeBackend {
        let mut clusters = HashMap::new();
        clusters.insert(
            "compute".to_string(),
            HorizonClusterConfig {
                cluster_id: cluster_id.to_string(),
                management_token: "hm_test".to_string(),
            },
        );

        ComputeBackend::Horizon(HorizonConfig {
            url: server.base_url(),
            horizond_download_base_url: "http://releases.test".to_string(),
            horizond_binary_hash: None,
            clusters,
        })
    }

    fn test_container(cluster_id: &str) -> Container {
        Container::new("api".to_string())
            .cluster(cluster_id.to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .stateful(true)
            .replicas(1)
            .persistent_storage(alien_core::PersistentStorage {
                size: "10Gi".to_string(),
                mount_path: "/data".to_string(),
                storage_type: None,
                iops: None,
                throughput: None,
            })
            .permissions("execution".to_string())
            .build()
    }

    fn test_network() -> Network {
        Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: Some("10.0.0.0/16".to_string()),
                availability_zones: 2,
            })
            .build()
    }

    fn test_cluster_resource() -> ContainerCluster {
        ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("e2-medium".to_string()),
                profile: None,
                min_size: 1,
                max_size: 1,
            })
            .build()
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let mock_compute = mock_compute_for_create_delete("203.0.113.1");
        let mock_provider = setup_mock_provider(mock_compute);

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(GcpContainerController::default())
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let mock_compute = mock_compute_for_create_delete("203.0.113.2");
        let mock_provider = setup_mock_provider(mock_compute);

        let mut container = test_container("compute");
        container.code = ContainerCode::Image {
            image: "nginx:1.25".to_string(),
        };

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 2,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = GcpContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.2".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(container)
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let mock_compute = mock_compute_for_best_effort_delete();
        let mock_provider = setup_mock_provider(mock_compute);

        let mut controller = GcpContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.3".to_string()),
        );
        controller.persistent_disks.push(PersistentDiskState {
            disk_name: "missing-disk".to_string(),
            disk_self_link: "projects/test/zones/us-central1-a/disks/missing".to_string(),
            zone: "us-central1-a".to_string(),
            ordinal: 0,
            size_gb: 10,
        });

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Verifies the PATCH request to Horizon includes cpu, memory, and command.
    #[tokio::test]
    async fn test_update_sends_resources_and_command() {
        let cluster_id = "test-cluster";
        let container_name = "api";

        let server = MockServer::start();
        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });
        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": { "cpu": { "min": "1", "desired": "1" }, "memory": { "min": "1Gi", "desired": "1Gi" } },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": [{ "replicaId": "api-0", "machineId": "m-0", "ip": "10.0.0.10", "status": "running", "healthy": true, "consecutiveFailures": 0 }],
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        // PATCH mock requires specific cpu desired and command in body.
        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!(
                    "/clusters/{}/containers/{}",
                    cluster_id, container_name
                ))
                .body_contains("\"desired\":\"4\"")
                .body_contains("\"command\"");
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        let mock_compute = mock_compute_for_create_delete("203.0.113.5");
        let mock_provider = setup_mock_provider(mock_compute);

        let mut updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "2".to_string(),
                desired: "4".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 2,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build();
        updated_container.command = Some(vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep infinity".to_string(),
        ]);

        let ready_controller = GcpContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.5".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Verifies that when health_check.path changes, patch_health_check is called with that path.
    #[tokio::test]
    async fn test_update_health_check_path() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);

        let mut mock = MockComputeApi::new();
        mock.expect_patch_health_check()
            .withf(|_name, hc| {
                hc.http_health_check
                    .as_ref()
                    .and_then(|h| h.request_path.as_deref())
                    == Some("/custom-health")
            })
            .returning(|_, _| Ok(Operation::default()));
        // standard delete expectations
        mock.expect_delete_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_target_http_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_health_check()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_global_address()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_delete_disk()
            .returning(|_, _| Ok(Operation::default()));
        let mock_provider = setup_mock_provider(Arc::new(mock));

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .health_check(HealthCheck {
                path: "/custom-health".to_string(),
                port: None,
                method: "GET".to_string(),
                timeout_seconds: 2,
                failure_threshold: 5,
            })
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 2,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = GcpContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.6".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Verifies that updates including autoscaling http-in-flight and p95 latency targets complete.
    #[tokio::test]
    async fn test_update_with_http_in_flight() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = MockServer::start();
        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });
        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": { "cpu": { "min": "1", "desired": "1" }, "memory": { "min": "1Gi", "desired": "1Gi" } },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": [{ "replicaId": "api-0", "machineId": "m-0", "ip": "10.0.0.10", "status": "running", "healthy": true, "consecutiveFailures": 0 }],
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!(
                    "/clusters/{}/containers/{}",
                    cluster_id, container_name
                ))
                .body_contains("targetHttpInFlightPerReplica")
                .body_contains("maxHttpP95LatencyMs");
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        let mock_compute = mock_compute_for_create_delete("203.0.113.7");
        let mock_provider = setup_mock_provider(mock_compute);

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 5,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: Some(10),
                max_http_p95_latency_ms: Some(200.0),
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = GcpContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.7".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}

#[controller]
impl GcpContainerController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            container_id = %config.id,
            cluster = %cluster,
            "Starting GCP Container provisioning"
        );

        self.container_name = Some(config.id.clone());

        // Determine next step based on what infrastructure we need to create
        let exposed_port = config.ports.iter().find(|p| p.expose.is_some());

        if let Some(port_config) = exposed_port {
            let is_http = matches!(port_config.expose.as_ref().unwrap(), ExposeProtocol::Http);
            // Resolve domain information
            let domain_info = Self::resolve_domain_info(ctx, &config.id)?;
            self.fqdn = Some(domain_info.fqdn.clone());
            self.certificate_id = domain_info.certificate_id;
            self.ssl_certificate_name = domain_info.ssl_certificate_name;
            self.uses_custom_domain = domain_info.uses_custom_domain;

            // Check for URL override in deployment config, otherwise use domain FQDN
            self.public_url = ctx
                .deployment_config
                .public_urls
                .as_ref()
                .and_then(|urls| urls.get(&config.id).cloned())
                .or_else(|| Some(format!("https://{}", domain_info.fqdn)));

            // If using auto-managed domain, wait for certificate first
            if !self.uses_custom_domain {
                Ok(HandlerAction::Continue {
                    state: WaitingForCertificate,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: CreatingHealthCheck,
                    suggested_delay: None,
                })
            }
        } else if config.persistent_storage.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingPersistentDisks,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.certificate_status);

        match status {
            Some(CertificateStatus::Issued) => {
                info!(container_id = %config.id, "Certificate issued, proceeding to import");
                Ok(HandlerAction::Continue {
                    state: ImportingSslCertificate,
                    suggested_delay: None,
                })
            }
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => {
                debug!(container_id = %config.id, "Certificate not yet issued, waiting");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    #[handler(
        state = ImportingSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Import to GCP as SSL Certificate
        let ssl_cert_name = format!("{}-{}-cert", ctx.resource_prefix, config.id);

        info!(
            container_id = %config.id,
            cert_name = %ssl_cert_name,
            "Importing certificate to GCP SSL Certificates"
        );

        use alien_gcp_clients::gcp::compute::{SslCertificate, SslCertificateSelfManaged};
        let ssl_cert = SslCertificate {
            name: Some(ssl_cert_name.clone()),
            description: Some(format!("SSL certificate for {}", config.id)),
            r#type: Some("SELF_MANAGED".to_string()),
            self_managed: Some(SslCertificateSelfManaged {
                certificate: Some(certificate_chain.clone()),
                private_key: Some(private_key.clone()),
            }),
            ..Default::default()
        };

        compute_client
            .insert_ssl_certificate(ssl_cert)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import SSL certificate to GCP".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.ssl_certificate_name = Some(ssl_cert_name.clone());

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            container_id = %config.id,
            cert_name = %ssl_cert_name,
            "SSL certificate imported to GCP"
        );

        Ok(HandlerAction::Continue {
            state: CreatingHealthCheck,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingHealthCheck,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_health_check(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let health_check_name = format!("{}-{}-hc", ctx.resource_prefix, config.id);

        info!(
            container_id = %config.id,
            health_check_name = %health_check_name,
            "Creating HTTP health check for public container"
        );

        let health_check_path = config
            .health_check
            .as_ref()
            .map(|h| h.path.clone())
            .unwrap_or_else(|| "/".to_string());

        // Use first port for health check (or exposed port if available)
        let health_check_port = config
            .ports
            .iter()
            .find(|p| p.expose.is_some())
            .or_else(|| config.ports.first())
            .map(|p| p.port)
            .unwrap_or(8080);

        let health_check = HealthCheck::builder()
            .name(health_check_name.clone())
            .description(format!("Health check for Alien Container {}", config.id))
            .r#type(HealthCheckType::Http)
            .http_health_check(
                alien_gcp_clients::gcp::compute::HttpHealthCheck::builder()
                    .port(health_check_port as i32)
                    .request_path(health_check_path)
                    .build(),
            )
            .check_interval_sec(30)
            .timeout_sec(5)
            .healthy_threshold(2)
            .unhealthy_threshold(3)
            .build();

        compute_client
            .insert_health_check(health_check)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create health check".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.health_check_name = Some(health_check_name);

        info!(
            container_id = %config.id,
            "Health check created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingNetworkEndpointGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNetworkEndpointGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_network_endpoint_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        // Get network from dependency
        let network_ref = ResourceRef::new(
            alien_core::Network::RESOURCE_TYPE,
            "default-network".to_string(),
        );
        let network = ctx.require_dependency::<GcpNetworkController>(&network_ref)?;
        let network_url = network.network_self_link.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network self link not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let subnet_url = network.subnetwork_self_link.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No subnetwork available from Network".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // TODO: Support multi-zone deployment
        let zone = format!("{}-a", gcp_cfg.region);

        let neg_name = format!("{}-{}-neg", ctx.resource_prefix, config.id);

        info!(
            container_id = %config.id,
            neg_name = %neg_name,
            zone = %zone,
            "Creating Network Endpoint Group"
        );

        // Use the exposed port for NEG default port (preflight ensures only one)
        let exposed_port = config
            .ports
            .iter()
            .find(|p| p.expose.is_some())
            .map(|p| p.port)
            .unwrap_or_else(|| config.ports.first().map(|p| p.port).unwrap_or(8080));

        let neg = NetworkEndpointGroup::builder()
            .name(neg_name.clone())
            .description(format!("NEG for Alien Container {}", config.id))
            .network_endpoint_type(NetworkEndpointType::GceVmIpPort)
            .network(network_url.clone())
            .subnetwork(subnet_url.clone())
            .default_port(exposed_port as i32)
            .build();

        compute_client
            .insert_network_endpoint_group(zone.clone(), neg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Network Endpoint Group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.neg_name = Some(neg_name);

        info!(
            container_id = %config.id,
            "Network Endpoint Group created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingBackendService,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let backend_service_name = format!("{}-{}-bs", ctx.resource_prefix, config.id);

        let neg_name = self.neg_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "NEG name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let health_check_name = self.health_check_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Health check name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Build NEG URL
        let zone = format!("{}-a", gcp_cfg.region);
        let neg_url = format!(
            "projects/{}/zones/{}/networkEndpointGroups/{}",
            gcp_cfg.project_id, zone, neg_name
        );

        let health_check_url = format!(
            "projects/{}/global/healthChecks/{}",
            gcp_cfg.project_id, health_check_name
        );

        info!(
            container_id = %config.id,
            backend_service_name = %backend_service_name,
            "Creating Backend Service"
        );

        let backend_service = BackendService::builder()
            .name(backend_service_name.clone())
            .description(format!("Backend Service for Alien Container {}", config.id))
            .protocol(BackendServiceProtocol::Http)
            .port_name("http".to_string())
            .load_balancing_scheme(LoadBalancingScheme::External)
            .backends(vec![Backend {
                group: Some(neg_url),
                balancing_mode: Some(BalancingMode::Rate),
                max_rate_per_endpoint: Some(100.0),
                capacity_scaler: Some(1.0),
                ..Default::default()
            }])
            .health_checks(vec![health_check_url])
            .build();

        compute_client
            .insert_backend_service(backend_service)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Backend Service".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.backend_service_name = Some(backend_service_name.clone());

        info!(
            container_id = %config.id,
            "Backend Service created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingUrlMap,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let backend_service_name = self.backend_service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Backend service name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let url_map_name = format!("{}-{}-um", ctx.resource_prefix, config.id);
        let backend_service_url = format!(
            "projects/{}/global/backendServices/{}",
            gcp_cfg.project_id, backend_service_name
        );

        info!(
            container_id = %config.id,
            url_map_name = %url_map_name,
            "Creating URL map"
        );

        let url_map = UrlMap::builder()
            .name(url_map_name.clone())
            .description(format!("URL map for Alien Container {}", config.id))
            .default_service(backend_service_url)
            .build();

        compute_client
            .insert_url_map(url_map)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create URL map".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.url_map_name = Some(url_map_name);

        Ok(HandlerAction::Continue {
            state: CreatingTargetHttpProxy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingTargetHttpProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_target_http_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let url_map_name = self.url_map_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "URL map name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let proxy_name = format!("{}-{}-proxy", ctx.resource_prefix, config.id);
        let url_map_url = format!(
            "projects/{}/global/urlMaps/{}",
            gcp_cfg.project_id, url_map_name
        );

        // Public containers with domains always use HTTPS
        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "SSL certificate name not set for public container".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            container_id = %config.id,
            proxy_name = %proxy_name,
            "Creating Target HTTPS proxy with SSL certificate"
        );

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_cfg.project_id, ssl_cert_name
        );

        use alien_gcp_clients::gcp::compute::TargetHttpsProxy;
        let proxy = TargetHttpsProxy::builder()
            .name(proxy_name.clone())
            .description(format!("HTTPS Proxy for Alien Container {}", config.id))
            .url_map(url_map_url)
            .ssl_certificates(vec![ssl_cert_url])
            .build();

        compute_client
            .insert_target_https_proxy(proxy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Target HTTPS proxy".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.target_http_proxy_name = Some(proxy_name);

        Ok(HandlerAction::Continue {
            state: CreatingGlobalAddress,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let address_name = format!("{}-{}-ip", ctx.resource_prefix, config.id);

        info!(
            container_id = %config.id,
            address_name = %address_name,
            "Creating global address"
        );

        let address = Address::builder()
            .name(address_name.clone())
            .description(format!("Public IP for Alien Container {}", config.id))
            .address_type(AddressType::External)
            .build();

        compute_client
            .insert_global_address(address)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global address".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.global_address_name = Some(address_name);

        Ok(HandlerAction::Continue {
            state: CreatingForwardingRule,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let proxy_name = self.target_http_proxy_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Target HTTP proxy name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let address_name = self.global_address_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Global address name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let forwarding_rule_name = format!("{}-{}-fwd", ctx.resource_prefix, config.id);

        // Public containers with domains always use HTTPS on port 443
        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "SSL certificate name not set for public container".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let proxy_url = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            gcp_cfg.project_id, proxy_name
        );

        let address_url = format!(
            "projects/{}/global/addresses/{}",
            gcp_cfg.project_id, address_name
        );

        info!(
            container_id = %config.id,
            forwarding_rule_name = %forwarding_rule_name,
            "Creating global forwarding rule for HTTPS"
        );

        let forwarding_rule = ForwardingRule::builder()
            .name(forwarding_rule_name.clone())
            .description(format!("Forwarding rule for Alien Container {}", config.id))
            .ip_address(address_url)
            .ip_protocol(ForwardingRuleProtocol::Tcp)
            .port_range("443".to_string())
            .load_balancing_scheme(LoadBalancingScheme::External)
            .target(proxy_url)
            .build();

        compute_client
            .insert_global_forwarding_rule(forwarding_rule)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global forwarding rule".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.forwarding_rule_name = Some(forwarding_rule_name);

        if let Ok(address) = compute_client
            .get_global_address(address_name.clone())
            .await
        {
            if let Some(ip) = address.address {
                self.public_url = Some(format!("https://{}", ip));
            }
        }

        // Check if we also need to create persistent disks
        if config.persistent_storage.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingPersistentDisks,
                suggested_delay: None,
            })
        } else if self.fqdn.is_some() && !self.uses_custom_domain {
            // For auto-managed domains, wait for DNS propagation
            Ok(HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(container_id = %config.id, "DNS record active, proceeding");
                Ok(HandlerAction::Continue {
                    state: CreatingHorizonContainer,
                    suggested_delay: None,
                })
            }
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => {
                debug!(container_id = %config.id, "DNS record not yet active, waiting");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    #[handler(
        state = CreatingPersistentDisks,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_persistent_disks(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let persistent_storage = match &config.persistent_storage {
            Some(ps) => ps,
            None => {
                return Ok(HandlerAction::Continue {
                    state: CreatingHorizonContainer,
                    suggested_delay: None,
                })
            }
        };

        // Parse storage size from string to GB
        let size_gb = Self::parse_storage_size_gb(&persistent_storage.size)?;

        // TODO: Support multi-zone deployment
        let zone = format!("{}-a", gcp_cfg.region);

        // For stateful containers, create one disk per potential replica
        let replica_count = config
            .replicas
            .or(config.autoscaling.as_ref().map(|a| a.desired))
            .unwrap_or(1);

        info!(
            container_id = %config.id,
            size_gb = size_gb,
            replica_count = replica_count,
            zone = %zone,
            "Creating Persistent Disks for stateful container"
        );

        // Map storage_type string to GCP disk type
        let disk_type = persistent_storage
            .storage_type
            .as_deref()
            .unwrap_or("pd-standard");

        for ordinal in 0..replica_count {
            let disk_name = format!("{}-{}-disk-{}", ctx.resource_prefix, config.id, ordinal);

            let disk = Disk::builder()
                .name(disk_name.clone())
                .description(format!(
                    "Persistent disk for Alien Container {} ordinal {}",
                    config.id, ordinal
                ))
                .r#type(format!(
                    "projects/{}/zones/{}/diskTypes/{}",
                    gcp_cfg.project_id, zone, disk_type
                ))
                .size_gb(size_gb.to_string())
                .build();

            let created_disk = compute_client
                .insert_disk(zone.clone(), disk)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create disk for ordinal {}", ordinal),
                    resource_id: Some(config.id.clone()),
                })?;

            let disk_self_link = created_disk.self_link.clone().unwrap_or(format!(
                "projects/{}/zones/{}/disks/{}",
                gcp_cfg.project_id, zone, disk_name
            ));

            self.persistent_disks.push(PersistentDiskState {
                disk_name: disk_name.clone(),
                disk_self_link,
                zone: zone.clone(),
                ordinal,
                size_gb,
            });

            info!(
                container_id = %config.id,
                disk_name = %disk_name,
                ordinal = ordinal,
                "Persistent Disk created"
            );
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDisks,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDisks,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_disks(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        if self.persistent_disks.is_empty() {
            return Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            });
        }

        let mut all_ready = true;

        for disk_state in &self.persistent_disks {
            let disk = compute_client
                .get_disk(disk_state.zone.clone(), disk_state.disk_name.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get disk {}", disk_state.disk_name),
                    resource_id: Some(config.id.clone()),
                })?;

            if disk.status != Some(alien_gcp_clients::gcp::compute::DiskStatus::Ready) {
                all_ready = false;
            }
        }

        if all_ready {
            info!(
                container_id = %config.id,
                disk_count = self.persistent_disks.len(),
                "All persistent disks are ready"
            );

            // After disks are ready, check if we need DNS waiting
            if self.fqdn.is_some() && !self.uses_custom_domain {
                Ok(HandlerAction::Continue {
                    state: WaitingForDns,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: CreatingHorizonContainer,
                    suggested_delay: None,
                })
            }
        } else {
            debug!(
                container_id = %config.id,
                "Waiting for persistent disks to become ready"
            );
            Ok(HandlerAction::Stay {
                max_times: 30,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = CreatingHorizonContainer,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get the ContainerCluster to verify it's ready
        let cluster_ref = ResourceRef::new(ContainerCluster::RESOURCE_TYPE, cluster_id.clone());
        let _cluster = ctx.require_dependency::<GcpContainerClusterController>(&cluster_ref)?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            neg = ?self.neg_name,
            disk_count = self.persistent_disks.len(),
            "Creating container in Horizon"
        );

        // Get image from container code
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // Build resource requirements using SDK types
        let cpu: horizon_client_sdk::types::ResourceRequirementsCpu =
            horizon_client_sdk::types::ResourceRequirementsCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::ResourceRequirementsMemory =
            horizon_client_sdk::types::ResourceRequirementsMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder = horizon_client_sdk::types::ResourceRequirements::builder()
            .cpu(cpu)
            .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::ResourceRequirementsEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid ephemeral storage config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::ResourceRequirements =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Build ports from config
        let ports: Vec<NonZeroU64> = config
            .ports
            .iter()
            .filter_map(|p| NonZeroU64::new(p.port as u64))
            .collect();

        // Build capacity group
        let capacity_group = config.pool.clone().unwrap_or_else(|| "general".to_string());

        // Build environment variables
        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        // Start building request
        let mut request_builder = horizon_client_sdk::types::CreateContainerRequest::builder()
            .name(&config.id)
            .capacity_group(&capacity_group)
            .image(&image)
            .resources(resources)
            .stateful(config.stateful)
            .ports(ports)
            .env(env_vars);

        // Add replicas or autoscaling
        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::CreateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }

            let autoscaling_config: horizon_client_sdk::types::CreateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        // Add command if present
        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        // Wire per-container cloud identity so horizond can vend credentials via the IMDS proxy.
        // The SA dep is declared by ServiceAccountDependenciesMutation, so require_dependency
        // will only succeed once the SA is Running (has internal_state). If it somehow
        // succeeds but the email is still None, that is a controller bug — fail loudly
        // rather than silently skipping and creating the container without credentials.
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::GcpServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                let email = sa_ctrl.service_account_email.as_ref().ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: config.id.clone(),
                        dependency_id: sa_resource_id.clone(),
                    })
                })?;
                let sa = horizon_client_sdk::types::ServiceAccountTarget::from(
                    horizon_client_sdk::types::GcpServiceAccountTarget {
                        email: email.clone(),
                        type_: horizon_client_sdk::types::GcpServiceAccountTargetType::Gcp,
                    },
                );
                request_builder = request_builder.service_account(sa);
                info!(
                    container_id = %config.id,
                    email = %email,
                    "Wired GCP service account to container for IMDS credential vending"
                );
            }
        }

        // Add load balancer target for public containers
        if let Some(neg_name) = &self.neg_name {
            let lb_target = horizon_client_sdk::types::LoadBalancerTarget::Gcp {
                neg_name: neg_name.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid NEG name '{}'", neg_name),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
            };
            request_builder = request_builder.load_balancer_target(lb_target);
        }

        // Add volumes for persistent storage
        if !self.persistent_disks.is_empty() {
            let volumes = self
                .persistent_disks
                .iter()
                .map(|disk| {
                    Ok::<_, AlienError<ErrorData>>(horizon_client_sdk::types::VolumeRegistration {
                        ordinal: disk.ordinal as u64,
                        volume: horizon_client_sdk::types::VolumeTarget::Gcp {
                            disk_name: disk.disk_name.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid disk name '{}'", disk.disk_name),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            zone: disk.zone.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid zone '{}'", disk.zone),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                        },
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            request_builder = request_builder.volumes(volumes);
        }

        // Build the final request
        let request: horizon_client_sdk::types::CreateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build container request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Create container via Horizon SDK
        let response = horizon
            .client
            .create_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(response.status));

        info!(
            container_id = %config.id,
            "Container created in Horizon, waiting for replicas"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForReplicas,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForReplicas,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_replicas(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        let container_response = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(container_response.status));

        let healthy_replicas = container_response
            .replicas_info
            .iter()
            .filter(|r| r.healthy)
            .count() as u32;

        self.current_replicas = healthy_replicas;

        let desired = config
            .replicas
            .or(config.autoscaling.as_ref().map(|a| a.desired))
            .unwrap_or(1);

        debug!(
            container_id = %config.id,
            healthy = healthy_replicas,
            desired = desired,
            "Container replica status"
        );

        if desired == 0 {
            // No replicas expected
            info!(
                container_id = %config.id,
                "No replicas required (desired=0)"
            );
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else if healthy_replicas >= desired.min(1) {
            info!(
                container_id = %config.id,
                healthy_replicas = healthy_replicas,
                "Container replicas are healthy"
            );
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else {
            self.wait_for_replicas_iterations += 1;
            if self.wait_for_replicas_iterations >= 30 {
                // If the parent cluster is mid-update/provision, replica disruption is expected:
                // the rolling update will bring fresh VMs with updated horizond. Reset and wait.
                let cluster_is_updating = ctx.state.resources.get(cluster_id).map_or(false, |s| {
                    matches!(
                        s.status,
                        ResourceStatus::Updating | ResourceStatus::Provisioning
                    )
                });
                if cluster_is_updating {
                    info!(
                        container_id = %config.id,
                        cluster_id = %cluster_id,
                        "Parent cluster is updating, resetting health check counter"
                    );
                    self.wait_for_replicas_iterations = 0;
                } else {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Container replicas did not become healthy after 30 iterations (~5 min). \
                             Last Horizon status: {:?}, healthy replicas: {}/{}",
                            self.horizon_status, self.current_replicas, desired
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            debug!(
                container_id = %config.id,
                iteration = self.wait_for_replicas_iterations,
                "Waiting for more replicas to become healthy"
            );
            Ok(HandlerAction::Stay {
                max_times: 35, // safety backstop; manual check above fires first
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        debug!(container_id = %config.id, "GCP Container ready, checking health");

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        // Periodic health check
        let container_response = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status during health check: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(container_response.status));

        let healthy_replicas = container_response
            .replicas_info
            .iter()
            .filter(|r| r.healthy)
            .count() as u32;

        self.current_replicas = healthy_replicas;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "GCP Container update requested");

        Ok(HandlerAction::Continue {
            state: UpdatingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingHorizonContainer,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            "Updating container in Horizon"
        );

        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let image_typed: horizon_client_sdk::types::UpdateContainerRequestImage =
            image.as_str().try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid image: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        let cpu: horizon_client_sdk::types::UpdateContainerRequestResourcesCpu =
            horizon_client_sdk::types::UpdateContainerRequestResourcesCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::UpdateContainerRequestResourcesMemory =
            horizon_client_sdk::types::UpdateContainerRequestResourcesMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder =
            horizon_client_sdk::types::UpdateContainerRequestResources::builder()
                .cpu(cpu)
                .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::UpdateContainerRequestResourcesEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid ephemeral storage config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::UpdateContainerRequestResources =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let mut request_builder = horizon_client_sdk::types::UpdateContainerRequest::builder()
            .image(image_typed)
            .env(env_vars)
            .resources(resources);

        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::UpdateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }
            if let Some(p95_latency) = autoscaling.max_http_p95_latency_ms {
                autoscaling_builder = autoscaling_builder.max_http_p95_latency_ms(p95_latency);
            }

            let autoscaling_config: horizon_client_sdk::types::UpdateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        // Wire SA identity on updates — same fail-loud semantics as create.
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::GcpServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                let email = sa_ctrl.service_account_email.as_ref().ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: config.id.clone(),
                        dependency_id: sa_resource_id.clone(),
                    })
                })?;
                let sa = horizon_client_sdk::types::NullableServiceAccountTarget::from(
                    horizon_client_sdk::types::GcpServiceAccountTarget {
                        email: email.clone(),
                        type_: horizon_client_sdk::types::GcpServiceAccountTargetType::Gcp,
                    },
                );
                request_builder = request_builder.service_account(sa);
            }
        }

        let request: horizon_client_sdk::types::UpdateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build update request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        horizon
            .client
            .update_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to update container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        Ok(HandlerAction::Continue {
            state: UpdatingHealthCheck,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingHealthCheck,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_health_check(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if let Some(health_check_name) = &self.health_check_name {
            let gcp_cfg = ctx.get_gcp_config()?;
            let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
            let config = ctx.desired_resource_config::<Container>()?;

            let health_check_path = config
                .health_check
                .as_ref()
                .map(|h| h.path.clone())
                .unwrap_or_else(|| "/".to_string());

            let health_check_port = config
                .ports
                .iter()
                .find(|p| p.expose.is_some())
                .or_else(|| config.ports.first())
                .map(|p| p.port)
                .unwrap_or(8080);

            let failure_threshold = config
                .health_check
                .as_ref()
                .map(|h| h.failure_threshold as i32)
                .unwrap_or(3);

            let timeout_sec = config
                .health_check
                .as_ref()
                .map(|h| h.timeout_seconds as i32)
                .unwrap_or(5);

            info!(
                container_id = %config.id,
                health_check_name = %health_check_name,
                health_check_path = %health_check_path,
                "Updating GCP health check"
            );

            let health_check = HealthCheck::builder()
                .name(health_check_name.clone())
                .description(format!("Health check for Alien Container {}", config.id))
                .r#type(HealthCheckType::Http)
                .http_health_check(
                    alien_gcp_clients::gcp::compute::HttpHealthCheck::builder()
                        .port(health_check_port as i32)
                        .request_path(health_check_path)
                        .build(),
                )
                .check_interval_sec(30)
                .timeout_sec(timeout_sec)
                .healthy_threshold(2)
                .unhealthy_threshold(failure_threshold)
                .build();

            compute_client
                .patch_health_check(health_check_name.clone(), health_check)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update GCP health check".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(container_id = %config.id, "GCP health check updated");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Starting GCP Container deletion");

        Ok(HandlerAction::Continue {
            state: DeletingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingHorizonContainer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            "Deleting container from Horizon"
        );

        match horizon
            .client
            .delete_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
        {
            Ok(_) => info!(container_id = %config.id, "Container deleted from Horizon"),
            Err(e) if e.to_string().contains("404") || e.to_string().contains("not found") => {
                info!(container_id = %config.id, "Container already deleted from Horizon")
            }
            Err(e) => {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete container from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                }))
            }
        }

        self.container_name = None;
        self.horizon_status = None;
        self.current_replicas = 0;

        Ok(HandlerAction::Continue {
            state: DeletingForwardingRule,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = DeletingForwardingRule,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(forwarding_rule_name) = &self.forwarding_rule_name {
            info!(
                forwarding_rule_name = %forwarding_rule_name,
                "Deleting global forwarding rule"
            );

            match compute_client
                .delete_global_forwarding_rule(forwarding_rule_name.clone())
                .await
            {
                Ok(_) => {
                    info!(forwarding_rule_name = %forwarding_rule_name, "Forwarding rule deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(forwarding_rule_name = %forwarding_rule_name, "Forwarding rule already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete forwarding rule {}",
                            forwarding_rule_name
                        ),
                        resource_id: None,
                    })
                }
            }
        }

        self.forwarding_rule_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingTargetHttpProxy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingTargetHttpProxy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_target_http_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(proxy_name) = &self.target_http_proxy_name {
            // Public containers always use HTTPS proxy
            info!(proxy_name = %proxy_name, "Deleting Target HTTPS proxy");

            match compute_client
                .delete_target_https_proxy(proxy_name.clone())
                .await
            {
                Ok(_) => info!(proxy_name = %proxy_name, "Target HTTPS proxy deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(proxy_name = %proxy_name, "Target HTTPS proxy already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete Target HTTPS proxy {}", proxy_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.target_http_proxy_name = None;

        // After deleting proxy, delete SSL certificate if not using custom domain
        if !self.uses_custom_domain && self.ssl_certificate_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingSslCertificate,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: DeletingUrlMap,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = DeletingSslCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(cert_name) = &self.ssl_certificate_name {
            info!(cert_name = %cert_name, "Deleting SSL certificate");

            match compute_client
                .delete_ssl_certificate(cert_name.clone())
                .await
            {
                Ok(_) => info!(cert_name = %cert_name, "SSL certificate deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(cert_name = %cert_name, "SSL certificate already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete SSL certificate {}", cert_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.ssl_certificate_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingUrlMap,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingUrlMap,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(url_map_name) = &self.url_map_name {
            info!(url_map_name = %url_map_name, "Deleting URL map");

            match compute_client.delete_url_map(url_map_name.clone()).await {
                Ok(_) => info!(url_map_name = %url_map_name, "URL map deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(url_map_name = %url_map_name, "URL map already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete URL map {}", url_map_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.url_map_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingBackendService,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingBackendService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(backend_service_name) = &self.backend_service_name {
            info!(backend_service_name = %backend_service_name, "Deleting Backend Service");

            match compute_client
                .delete_backend_service(backend_service_name.clone())
                .await
            {
                Ok(_) => {
                    info!(backend_service_name = %backend_service_name, "Backend Service deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(backend_service_name = %backend_service_name, "Backend Service already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete Backend Service {}",
                            backend_service_name
                        ),
                        resource_id: None,
                    })
                }
            }
        }

        self.backend_service_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingNetworkEndpointGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingNetworkEndpointGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_network_endpoint_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(neg_name) = &self.neg_name {
            let zone = format!("{}-a", gcp_cfg.region);

            info!(neg_name = %neg_name, "Deleting Network Endpoint Group");

            match compute_client
                .delete_network_endpoint_group(zone.clone(), neg_name.clone())
                .await
            {
                Ok(_) => info!(neg_name = %neg_name, "NEG deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(neg_name = %neg_name, "NEG already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete NEG {}", neg_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.neg_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingHealthCheck,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingHealthCheck,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_health_check(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(health_check_name) = &self.health_check_name {
            info!(health_check_name = %health_check_name, "Deleting health check");

            match compute_client
                .delete_health_check(health_check_name.clone())
                .await
            {
                Ok(_) => info!(health_check_name = %health_check_name, "Health check deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(health_check_name = %health_check_name, "Health check already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete health check {}", health_check_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.health_check_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingGlobalAddress,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingGlobalAddress,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(address_name) = &self.global_address_name {
            info!(address_name = %address_name, "Deleting global address");

            match compute_client
                .delete_global_address(address_name.clone())
                .await
            {
                Ok(_) => info!(address_name = %address_name, "Global address deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(address_name = %address_name, "Global address already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete global address {}", address_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.global_address_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingPersistentDisks,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPersistentDisks,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_persistent_disks(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for disk_state in &self.persistent_disks {
            info!(disk_name = %disk_state.disk_name, "Deleting Persistent Disk");

            match compute_client
                .delete_disk(disk_state.zone.clone(), disk_state.disk_name.clone())
                .await
            {
                Ok(_) => info!(disk_name = %disk_state.disk_name, "Disk deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(disk_name = %disk_state.disk_name, "Disk already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete disk {}", disk_state.disk_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.persistent_disks.clear();

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ─────────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let container_name = self.container_name.as_ref()?;

        let status = self.horizon_status.unwrap_or(ContainerStatus::Pending);

        // Build load balancer endpoint for DNS controller
        let load_balancer_endpoint = self
            .forwarding_rule_name
            .as_ref()
            .and_then(|_| self.global_address_name.as_ref())
            .and_then(|_| self.public_url.as_ref())
            .and_then(|url| {
                // Extract IP or hostname from URL
                url.strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .map(|s| s.split('/').next().unwrap_or(s))
                    .map(|dns| alien_core::LoadBalancerEndpoint {
                        dns_name: dns.to_string(),
                        hosted_zone_id: None, // GCP doesn't use hosted zones
                    })
            });

        Some(ResourceOutputs::new(ContainerOutputs {
            name: container_name.clone(),
            status,
            current_replicas: self.current_replicas,
            desired_replicas: self.current_replicas,
            internal_dns: format!("{}.svc", container_name),
            url: self.public_url.clone(),
            replicas: vec![],
            load_balancer_endpoint,
        }))
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, ContainerBinding};

        self.container_name.as_ref().map(|name| {
            let internal_url = format!("http://{}.svc:8080", name);

            let binding = if let Some(url) = &self.public_url {
                ContainerBinding::horizon_with_public_url(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                    BindingValue::value(url.clone()),
                )
            } else {
                ContainerBinding::horizon(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                )
            };

            serde_json::to_value(binding).unwrap_or_default()
        })
    }
}

impl GcpContainerController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(container_name: &str, replicas: u32, public_url: Option<String>) -> Self {
        Self {
            state: GcpContainerState::Ready,
            container_name: Some(container_name.to_string()),
            horizon_status: Some(ContainerStatus::Running),
            current_replicas: replicas,
            public_url,
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            health_check_name: Some("test-hc".to_string()),
            neg_name: Some("test-neg".to_string()),
            backend_service_name: Some("test-bs".to_string()),
            url_map_name: Some("test-um".to_string()),
            target_http_proxy_name: Some("test-proxy".to_string()),
            forwarding_rule_name: Some("test-fwd".to_string()),
            global_address_name: Some("test-ip".to_string()),
            persistent_disks: vec![],
            wait_for_replicas_iterations: 0,
            _internal_stay_count: None,
        }
    }
}
