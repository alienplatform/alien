//! Comprehensive E2E tests for the GCP Compute Engine client.
//!
//! These tests create real VPC resources in GCP and verify all operations work correctly.
//! Since VPC resources are expensive to set up and take time, we use a single comprehensive
//! test that exercises all APIs in sequence.

use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::compute::{
    Address, AttachedDisk, AttachedDiskInitializeParams, AttachedDiskType, Backend, BackendService,
    BackendServiceProtocol, BalancingMode, ComputeApi, ComputeClient, Disk, DiskMode, Firewall,
    FirewallAllowed, FirewallDirection, FixedOrPercent, ForwardingRule, ForwardingRuleProtocol,
    HealthCheck, HealthCheckType, HttpHealthCheck, InstanceGroupManager,
    InstanceGroupManagerUpdatePolicy, InstanceProperties, InstanceTemplate, LoadBalancingScheme,
    ManagedInstance, ManagedInstanceCurrentAction, ManagedInstanceStatus, MinimalAction,
    NatIpAllocateOption, Network, NetworkEndpointGroup, NetworkEndpointType, NetworkInterface,
    NetworkRoutingConfig, Router, RouterNat, RoutingMode, ServiceAccount,
    SourceSubnetworkIpRangesToNat, SslCertificate, SslCertificateSelfManaged, Subnetwork,
    TargetHttpProxy, TargetHttpsProxy, UpdatePolicyType, UrlMap,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

const TEST_REGION: &str = "us-central1";
const TEST_ZONE: &str = "us-central1-a";
const NETWORK_DELETE_TIMEOUT_SECONDS: u64 = 300;
const NETWORK_DELETE_RETRY_INTERVAL_SECONDS: u64 = 10;

struct ComputeTestContext {
    client: ComputeClient,
    project_id: String,
    region: String,
    zone: String,
    /// Resources to clean up, in reverse order of creation
    created_networks: Mutex<HashSet<String>>,
    created_subnetworks: Mutex<HashSet<(String, String)>>, // (region, name)
    created_routers: Mutex<HashSet<(String, String)>>,     // (region, name)
    created_firewalls: Mutex<HashSet<String>>,
    // Load Balancing resources
    created_health_checks: Mutex<HashSet<String>>,
    created_backend_services: Mutex<HashSet<String>>,
    created_url_maps: Mutex<HashSet<String>>,
    created_target_http_proxies: Mutex<HashSet<String>>,
    created_global_addresses: Mutex<HashSet<String>>,
    created_global_forwarding_rules: Mutex<HashSet<String>>,
    created_negs: Mutex<HashSet<(String, String)>>, // (zone, name)
    // Instance Management resources
    created_instance_templates: Mutex<HashSet<String>>,
    created_instance_group_managers: Mutex<HashSet<(String, String)>>, // (zone, name)
    // Disk resources
    created_disks: Mutex<HashSet<(String, String)>>, // (zone, name)
}

impl AsyncTestContext for ComputeTestContext {
    async fn setup() -> ComputeTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: TEST_REGION.to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = ComputeClient::new(Client::new(), config);

        ComputeTestContext {
            client,
            project_id,
            region: TEST_REGION.to_string(),
            zone: TEST_ZONE.to_string(),
            created_networks: Mutex::new(HashSet::new()),
            created_subnetworks: Mutex::new(HashSet::new()),
            created_routers: Mutex::new(HashSet::new()),
            created_firewalls: Mutex::new(HashSet::new()),
            created_health_checks: Mutex::new(HashSet::new()),
            created_backend_services: Mutex::new(HashSet::new()),
            created_url_maps: Mutex::new(HashSet::new()),
            created_target_http_proxies: Mutex::new(HashSet::new()),
            created_global_addresses: Mutex::new(HashSet::new()),
            created_global_forwarding_rules: Mutex::new(HashSet::new()),
            created_negs: Mutex::new(HashSet::new()),
            created_instance_templates: Mutex::new(HashSet::new()),
            created_instance_group_managers: Mutex::new(HashSet::new()),
            created_disks: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Compute Engine test cleanup...");

        // Clean up in reverse dependency order

        // Delete disks (must be detached first)
        let disks_to_cleanup = {
            let disks = self.created_disks.lock().unwrap();
            disks.clone()
        };
        for (zone, disk_name) in disks_to_cleanup {
            self.cleanup_disk(&zone, &disk_name).await;
        }

        // Delete instance group managers (must be done before instance templates)
        let igms_to_cleanup = {
            let igms = self.created_instance_group_managers.lock().unwrap();
            igms.clone()
        };
        for (zone, igm_name) in igms_to_cleanup {
            self.cleanup_instance_group_manager(&zone, &igm_name).await;
        }

        // Delete instance templates
        let templates_to_cleanup = {
            let templates = self.created_instance_templates.lock().unwrap();
            templates.clone()
        };
        for template_name in templates_to_cleanup {
            self.cleanup_instance_template(&template_name).await;
        }

        // Delete forwarding rules (must be before target proxies and addresses)
        let fwds_to_cleanup = {
            let fwds = self.created_global_forwarding_rules.lock().unwrap();
            fwds.clone()
        };
        for fwd_name in fwds_to_cleanup {
            self.cleanup_global_forwarding_rule(&fwd_name).await;
        }

        // Delete target HTTP proxies (must be before URL maps)
        let proxies_to_cleanup = {
            let proxies = self.created_target_http_proxies.lock().unwrap();
            proxies.clone()
        };
        for proxy_name in proxies_to_cleanup {
            self.cleanup_target_http_proxy(&proxy_name).await;
        }

        // Delete URL maps (must be before backend services)
        let url_maps_to_cleanup = {
            let url_maps = self.created_url_maps.lock().unwrap();
            url_maps.clone()
        };
        for url_map_name in url_maps_to_cleanup {
            self.cleanup_url_map(&url_map_name).await;
        }

        // Delete backend services (must be before health checks and NEGs)
        let bs_to_cleanup = {
            let bs = self.created_backend_services.lock().unwrap();
            bs.clone()
        };
        for bs_name in bs_to_cleanup {
            self.cleanup_backend_service(&bs_name).await;
        }

        // Delete NEGs
        let negs_to_cleanup = {
            let negs = self.created_negs.lock().unwrap();
            negs.clone()
        };
        for (zone, neg_name) in negs_to_cleanup {
            self.cleanup_neg(&zone, &neg_name).await;
        }

        // Delete health checks
        let hc_to_cleanup = {
            let hc = self.created_health_checks.lock().unwrap();
            hc.clone()
        };
        for hc_name in hc_to_cleanup {
            self.cleanup_health_check(&hc_name).await;
        }

        // Delete global addresses
        let addrs_to_cleanup = {
            let addrs = self.created_global_addresses.lock().unwrap();
            addrs.clone()
        };
        for addr_name in addrs_to_cleanup {
            self.cleanup_global_address(&addr_name).await;
        }

        // Delete firewalls
        let firewalls_to_cleanup = {
            let firewalls = self.created_firewalls.lock().unwrap();
            firewalls.clone()
        };
        for firewall_name in firewalls_to_cleanup {
            self.cleanup_firewall(&firewall_name).await;
        }

        // Delete routers
        let routers_to_cleanup = {
            let routers = self.created_routers.lock().unwrap();
            routers.clone()
        };
        for (region, router_name) in routers_to_cleanup {
            self.cleanup_router(&region, &router_name).await;
        }

        // Delete subnetworks
        let subnetworks_to_cleanup = {
            let subnetworks = self.created_subnetworks.lock().unwrap();
            subnetworks.clone()
        };
        for (region, subnetwork_name) in subnetworks_to_cleanup {
            self.cleanup_subnetwork(&region, &subnetwork_name).await;
        }

        // Delete networks
        let networks_to_cleanup = {
            let networks = self.created_networks.lock().unwrap();
            networks.clone()
        };
        for network_name in networks_to_cleanup {
            self.cleanup_network(&network_name).await;
        }

        info!("✅ Compute Engine test cleanup completed");
    }
}

impl ComputeTestContext {
    fn is_resource_not_ready_error(err: &Error) -> bool {
        let msg = format!("{:?}", err).to_lowercase();
        msg.contains("resourcenotready") || msg.contains("not ready")
    }

    async fn delete_network_with_retry(
        &self,
        network_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);
        let mut attempt = 0u64;

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(format!(
                    "Timed out deleting network {} after {}s",
                    network_name, timeout_seconds
                )
                .into());
            }

            attempt += 1;
            info!(
                "🧹 Attempting network deletion (attempt {}): {}",
                attempt, network_name
            );

            match self.client.delete_network(network_name.to_string()).await {
                Ok(operation) => {
                    let op_name = operation.name.as_deref().ok_or_else(|| {
                        format!("Delete network {} returned no operation name", network_name)
                    })?;

                    let remaining = timeout_duration
                        .saturating_sub(start_time.elapsed())
                        .as_secs()
                        .max(1);

                    match self.wait_for_global_operation(op_name, remaining).await {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            warn!(
                                "Network delete operation for {} did not complete yet: {}",
                                network_name, e
                            );
                        }
                    }
                }
                Err(e) => match &e.error {
                    Some(ErrorData::RemoteResourceNotFound { .. }) => return Ok(()),
                    _ if Self::is_resource_not_ready_error(&e) => {
                        info!(
                            "Network {} is not ready for deletion yet (attempt {}), retrying...",
                            network_name, attempt
                        );
                    }
                    _ => {
                        return Err(
                            format!("Failed to delete network {}: {:?}", network_name, e).into(),
                        )
                    }
                },
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(
                NETWORK_DELETE_RETRY_INTERVAL_SECONDS,
            ))
            .await;
        }
    }

    // --- Tracking methods ---

    fn track_network(&self, network_name: &str) {
        let mut networks = self.created_networks.lock().unwrap();
        networks.insert(network_name.to_string());
        info!("📝 Tracking network for cleanup: {}", network_name);
    }

    fn untrack_network(&self, network_name: &str) {
        let mut networks = self.created_networks.lock().unwrap();
        networks.remove(network_name);
        info!("✅ Network {} untracked", network_name);
    }

    fn track_subnetwork(&self, region: &str, subnetwork_name: &str) {
        let mut subnetworks = self.created_subnetworks.lock().unwrap();
        subnetworks.insert((region.to_string(), subnetwork_name.to_string()));
        info!(
            "📝 Tracking subnetwork for cleanup: {}/{}",
            region, subnetwork_name
        );
    }

    fn untrack_subnetwork(&self, region: &str, subnetwork_name: &str) {
        let mut subnetworks = self.created_subnetworks.lock().unwrap();
        subnetworks.remove(&(region.to_string(), subnetwork_name.to_string()));
        info!("✅ Subnetwork {}/{} untracked", region, subnetwork_name);
    }

    fn track_router(&self, region: &str, router_name: &str) {
        let mut routers = self.created_routers.lock().unwrap();
        routers.insert((region.to_string(), router_name.to_string()));
        info!("📝 Tracking router for cleanup: {}/{}", region, router_name);
    }

    fn untrack_router(&self, region: &str, router_name: &str) {
        let mut routers = self.created_routers.lock().unwrap();
        routers.remove(&(region.to_string(), router_name.to_string()));
        info!("✅ Router {}/{} untracked", region, router_name);
    }

    fn track_firewall(&self, firewall_name: &str) {
        let mut firewalls = self.created_firewalls.lock().unwrap();
        firewalls.insert(firewall_name.to_string());
        info!("📝 Tracking firewall for cleanup: {}", firewall_name);
    }

    fn untrack_firewall(&self, firewall_name: &str) {
        let mut firewalls = self.created_firewalls.lock().unwrap();
        firewalls.remove(firewall_name);
        info!("✅ Firewall {} untracked", firewall_name);
    }

    // --- Load Balancing Tracking Methods ---

    fn track_health_check(&self, name: &str) {
        let mut hc = self.created_health_checks.lock().unwrap();
        hc.insert(name.to_string());
        info!("📝 Tracking health check for cleanup: {}", name);
    }

    fn untrack_health_check(&self, name: &str) {
        let mut hc = self.created_health_checks.lock().unwrap();
        hc.remove(name);
        info!("✅ Health check {} untracked", name);
    }

    fn track_backend_service(&self, name: &str) {
        let mut bs = self.created_backend_services.lock().unwrap();
        bs.insert(name.to_string());
        info!("📝 Tracking backend service for cleanup: {}", name);
    }

    fn untrack_backend_service(&self, name: &str) {
        let mut bs = self.created_backend_services.lock().unwrap();
        bs.remove(name);
        info!("✅ Backend service {} untracked", name);
    }

    fn track_url_map(&self, name: &str) {
        let mut um = self.created_url_maps.lock().unwrap();
        um.insert(name.to_string());
        info!("📝 Tracking URL map for cleanup: {}", name);
    }

    fn untrack_url_map(&self, name: &str) {
        let mut um = self.created_url_maps.lock().unwrap();
        um.remove(name);
        info!("✅ URL map {} untracked", name);
    }

    fn track_target_http_proxy(&self, name: &str) {
        let mut proxies = self.created_target_http_proxies.lock().unwrap();
        proxies.insert(name.to_string());
        info!("📝 Tracking target HTTP proxy for cleanup: {}", name);
    }

    fn untrack_target_http_proxy(&self, name: &str) {
        let mut proxies = self.created_target_http_proxies.lock().unwrap();
        proxies.remove(name);
        info!("✅ Target HTTP proxy {} untracked", name);
    }

    fn track_global_address(&self, name: &str) {
        let mut addrs = self.created_global_addresses.lock().unwrap();
        addrs.insert(name.to_string());
        info!("📝 Tracking global address for cleanup: {}", name);
    }

    fn untrack_global_address(&self, name: &str) {
        let mut addrs = self.created_global_addresses.lock().unwrap();
        addrs.remove(name);
        info!("✅ Global address {} untracked", name);
    }

    fn track_global_forwarding_rule(&self, name: &str) {
        let mut fwds = self.created_global_forwarding_rules.lock().unwrap();
        fwds.insert(name.to_string());
        info!("📝 Tracking global forwarding rule for cleanup: {}", name);
    }

    fn untrack_global_forwarding_rule(&self, name: &str) {
        let mut fwds = self.created_global_forwarding_rules.lock().unwrap();
        fwds.remove(name);
        info!("✅ Global forwarding rule {} untracked", name);
    }

    fn track_neg(&self, zone: &str, name: &str) {
        let mut negs = self.created_negs.lock().unwrap();
        negs.insert((zone.to_string(), name.to_string()));
        info!("📝 Tracking NEG for cleanup: {}/{}", zone, name);
    }

    fn untrack_neg(&self, zone: &str, name: &str) {
        let mut negs = self.created_negs.lock().unwrap();
        negs.remove(&(zone.to_string(), name.to_string()));
        info!("✅ NEG {}/{} untracked", zone, name);
    }

    // --- Instance Management Tracking Methods ---

    fn track_instance_template(&self, name: &str) {
        let mut templates = self.created_instance_templates.lock().unwrap();
        templates.insert(name.to_string());
        info!("📝 Tracking instance template for cleanup: {}", name);
    }

    fn untrack_instance_template(&self, name: &str) {
        let mut templates = self.created_instance_templates.lock().unwrap();
        templates.remove(name);
        info!("✅ Instance template {} untracked", name);
    }

    fn track_instance_group_manager(&self, zone: &str, name: &str) {
        let mut igms = self.created_instance_group_managers.lock().unwrap();
        igms.insert((zone.to_string(), name.to_string()));
        info!(
            "📝 Tracking instance group manager for cleanup: {}/{}",
            zone, name
        );
    }

    fn untrack_instance_group_manager(&self, zone: &str, name: &str) {
        let mut igms = self.created_instance_group_managers.lock().unwrap();
        igms.remove(&(zone.to_string(), name.to_string()));
        info!("✅ Instance group manager {}/{} untracked", zone, name);
    }

    // --- Disk Tracking Methods ---

    fn track_disk(&self, zone: &str, name: &str) {
        let mut disks = self.created_disks.lock().unwrap();
        disks.insert((zone.to_string(), name.to_string()));
        info!("📝 Tracking disk for cleanup: {}/{}", zone, name);
    }

    fn untrack_disk(&self, zone: &str, name: &str) {
        let mut disks = self.created_disks.lock().unwrap();
        disks.remove(&(zone.to_string(), name.to_string()));
        info!("✅ Disk {}/{} untracked", zone, name);
    }

    // --- Cleanup methods ---

    async fn cleanup_firewall(&self, firewall_name: &str) {
        info!("🧹 Cleaning up firewall: {}", firewall_name);
        match self.client.delete_firewall(firewall_name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Firewall {} deleted", firewall_name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Firewall {} was already deleted", firewall_name);
                }
                _ => warn!("Failed to delete firewall {}: {:?}", firewall_name, e),
            },
        }
    }

    async fn cleanup_router(&self, region: &str, router_name: &str) {
        info!("🧹 Cleaning up router: {}/{}", region, router_name);
        match self
            .client
            .delete_router(region.to_string(), router_name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_region_operation(region, op_name, 120).await;
                }
                info!("✅ Router {}/{} deleted", region, router_name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Router {}/{} was already deleted", region, router_name);
                }
                _ => warn!(
                    "Failed to delete router {}/{}: {:?}",
                    region, router_name, e
                ),
            },
        }
    }

    async fn cleanup_subnetwork(&self, region: &str, subnetwork_name: &str) {
        info!("🧹 Cleaning up subnetwork: {}/{}", region, subnetwork_name);
        match self
            .client
            .delete_subnetwork(region.to_string(), subnetwork_name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_region_operation(region, op_name, 120).await;
                }
                info!("✅ Subnetwork {}/{} deleted", region, subnetwork_name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!(
                        "🔍 Subnetwork {}/{} was already deleted",
                        region, subnetwork_name
                    );
                }
                _ => warn!(
                    "Failed to delete subnetwork {}/{}: {:?}",
                    region, subnetwork_name, e
                ),
            },
        }
    }

    async fn cleanup_network(&self, network_name: &str) {
        info!("🧹 Cleaning up network: {}", network_name);
        match self
            .delete_network_with_retry(network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
            .await
        {
            Ok(()) => {
                info!("✅ Network {} deleted", network_name);
            }
            Err(e) => warn!("Failed to delete network {}: {:?}", network_name, e),
        }
    }

    // --- Load Balancing Cleanup Methods ---

    async fn cleanup_health_check(&self, name: &str) {
        info!("🧹 Cleaning up health check: {}", name);
        match self.client.delete_health_check(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Health check {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Health check {} was already deleted", name);
                }
                _ => warn!("Failed to delete health check {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_backend_service(&self, name: &str) {
        info!("🧹 Cleaning up backend service: {}", name);
        match self.client.delete_backend_service(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Backend service {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Backend service {} was already deleted", name);
                }
                _ => warn!("Failed to delete backend service {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_url_map(&self, name: &str) {
        info!("🧹 Cleaning up URL map: {}", name);
        match self.client.delete_url_map(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ URL map {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 URL map {} was already deleted", name);
                }
                _ => warn!("Failed to delete URL map {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_target_http_proxy(&self, name: &str) {
        info!("🧹 Cleaning up target HTTP proxy: {}", name);
        match self.client.delete_target_http_proxy(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Target HTTP proxy {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Target HTTP proxy {} was already deleted", name);
                }
                _ => warn!("Failed to delete target HTTP proxy {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_global_address(&self, name: &str) {
        info!("🧹 Cleaning up global address: {}", name);
        match self.client.delete_global_address(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Global address {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Global address {} was already deleted", name);
                }
                _ => warn!("Failed to delete global address {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_global_forwarding_rule(&self, name: &str) {
        info!("🧹 Cleaning up global forwarding rule: {}", name);
        match self
            .client
            .delete_global_forwarding_rule(name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Global forwarding rule {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Global forwarding rule {} was already deleted", name);
                }
                _ => warn!("Failed to delete global forwarding rule {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_neg(&self, zone: &str, name: &str) {
        info!("🧹 Cleaning up NEG: {}/{}", zone, name);
        match self
            .client
            .delete_network_endpoint_group(zone.to_string(), name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_zone_operation(zone, op_name, 120).await;
                }
                info!("✅ NEG {}/{} deleted", zone, name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 NEG {}/{} was already deleted", zone, name);
                }
                _ => warn!("Failed to delete NEG {}/{}: {:?}", zone, name, e),
            },
        }
    }

    // --- Instance Management Cleanup Methods ---

    async fn cleanup_instance_template(&self, name: &str) {
        info!("🧹 Cleaning up instance template: {}", name);
        match self.client.delete_instance_template(name.to_string()).await {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_global_operation(op_name, 120).await;
                }
                info!("✅ Instance template {} deleted", name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Instance template {} was already deleted", name);
                }
                _ => warn!("Failed to delete instance template {}: {:?}", name, e),
            },
        }
    }

    async fn cleanup_instance_group_manager(&self, zone: &str, name: &str) {
        info!("🧹 Cleaning up instance group manager: {}/{}", zone, name);
        match self
            .client
            .delete_instance_group_manager(zone.to_string(), name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_zone_operation(zone, op_name, 300).await;
                }
                info!("✅ Instance group manager {}/{} deleted", zone, name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!(
                        "🔍 Instance group manager {}/{} was already deleted",
                        zone, name
                    );
                }
                _ => warn!(
                    "Failed to delete instance group manager {}/{}: {:?}",
                    zone, name, e
                ),
            },
        }
    }

    // --- Disk Cleanup Methods ---

    async fn cleanup_disk(&self, zone: &str, name: &str) {
        info!("🧹 Cleaning up disk: {}/{}", zone, name);
        match self
            .client
            .delete_disk(zone.to_string(), name.to_string())
            .await
        {
            Ok(operation) => {
                if let Some(op_name) = &operation.name {
                    let _ = self.wait_for_zone_operation(zone, op_name, 120).await;
                }
                info!("✅ Disk {}/{} deleted", zone, name);
            }
            Err(e) => match &e.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Disk {}/{} was already deleted", zone, name);
                }
                _ => warn!("Failed to delete disk {}/{}: {:?}", zone, name, e),
            },
        }
    }

    // --- Helper methods ---

    fn generate_unique_name(&self, prefix: &str) -> String {
        format!(
            "alien-test-{}-{}",
            prefix,
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..8].to_lowercase()
        )
    }

    fn extract_operation_name(&self, operation_name: &str) -> String {
        operation_name
            .split('/')
            .last()
            .unwrap_or(operation_name)
            .to_string()
    }

    async fn wait_for_global_operation(
        &self,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let op_name = self.extract_operation_name(operation_name);
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            match self.client.get_global_operation(op_name.clone()).await {
                Ok(operation) => {
                    if operation.is_done() {
                        if operation.has_error() {
                            return Err(format!(
                                "Operation {} failed: {:?}",
                                op_name, operation.error
                            )
                            .into());
                        }
                        info!("✅ Global operation {} completed!", op_name);
                        return Ok(());
                    }

                    if start_time.elapsed() > timeout_duration {
                        return Err(format!(
                            "Timeout waiting for global operation {} to complete",
                            op_name
                        )
                        .into());
                    }

                    info!("⏳ Global operation {} still running...", op_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    if start_time.elapsed() > timeout_duration {
                        return Err(format!(
                            "Timeout waiting for global operation {} to complete (last error: {:?})",
                            op_name, e
                        )
                        .into());
                    }
                    warn!("Error checking global operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn wait_for_region_operation(
        &self,
        region: &str,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let op_name = self.extract_operation_name(operation_name);
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(format!(
                    "Timeout waiting for region operation {} to complete",
                    op_name
                )
                .into());
            }

            match self
                .client
                .get_region_operation(region.to_string(), op_name.clone())
                .await
            {
                Ok(operation) => {
                    if operation.is_done() {
                        if operation.has_error() {
                            return Err(format!(
                                "Operation {} failed: {:?}",
                                op_name, operation.error
                            )
                            .into());
                        }
                        info!("✅ Region operation {} completed!", op_name);
                        return Ok(());
                    }
                    info!("⏳ Region operation {} still running...", op_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    warn!("Error checking region operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn wait_for_zone_operation(
        &self,
        zone: &str,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let op_name = self.extract_operation_name(operation_name);
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(
                    format!("Timeout waiting for zone operation {} to complete", op_name).into(),
                );
            }

            match self
                .client
                .get_zone_operation(zone.to_string(), op_name.clone())
                .await
            {
                Ok(operation) => {
                    if operation.is_done() {
                        if operation.has_error() {
                            return Err(format!(
                                "Operation {} failed: {:?}",
                                op_name, operation.error
                            )
                            .into());
                        }
                        info!("✅ Zone operation {} completed!", op_name);
                        return Ok(());
                    }
                    info!("⏳ Zone operation {} still running...", op_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    warn!("Error checking zone operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn wait_for_stable_managed_instance(
        &self,
        zone: &str,
        igm_name: &str,
        expected_template_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<ManagedInstance, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(format!(
                    "Timeout waiting for IGM {}/{} to reach a stable managed instance on template {}",
                    zone, igm_name, expected_template_name
                )
                .into());
            }

            let igm = match self
                .client
                .get_instance_group_manager(zone.to_string(), igm_name.to_string())
                .await
            {
                Ok(igm) => igm,
                Err(e) => {
                    warn!(
                        "Error checking instance group manager {}/{} status: {:?}",
                        zone, igm_name, e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let managed_instances = match self
                .client
                .list_managed_instances(zone.to_string(), igm_name.to_string())
                .await
            {
                Ok(instances) => instances,
                Err(e) => {
                    warn!(
                        "Error listing managed instances for {}/{}: {:?}",
                        zone, igm_name, e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let is_stable = igm
                .status
                .as_ref()
                .and_then(|status| status.is_stable)
                .unwrap_or(false);
            let version_reached = igm
                .status
                .as_ref()
                .and_then(|status| status.version_target.as_ref())
                .and_then(|target| target.is_reached)
                .unwrap_or(false);

            if is_stable && version_reached {
                if let Some(instance) =
                    managed_instances.managed_instances.iter().find(|instance| {
                        matches!(
                            instance.instance_status,
                            Some(ManagedInstanceStatus::Running)
                        ) && matches!(
                            instance.current_action,
                            Some(ManagedInstanceCurrentAction::None)
                        ) && instance
                            .version
                            .as_ref()
                            .and_then(|version| version.instance_template.as_deref())
                            .is_some_and(|template| template.contains(expected_template_name))
                    })
                {
                    let instance_name = instance
                        .instance
                        .as_deref()
                        .and_then(|url| url.split('/').last())
                        .unwrap_or("unknown");
                    info!(
                        "✅ IGM {}/{} is stable with managed instance {} on template {}",
                        zone, igm_name, instance_name, expected_template_name
                    );
                    return Ok(instance.clone());
                }
            }

            let instance_summaries: Vec<String> = managed_instances
                .managed_instances
                .iter()
                .map(|instance| {
                    let instance_name = instance
                        .instance
                        .as_deref()
                        .and_then(|url| url.split('/').last())
                        .unwrap_or("unknown");
                    let template = instance
                        .version
                        .as_ref()
                        .and_then(|version| version.instance_template.as_deref())
                        .unwrap_or("unknown-template");
                    format!(
                        "{} status={:?} action={:?} template={}",
                        instance_name, instance.instance_status, instance.current_action, template
                    )
                })
                .collect();

            info!(
                "⏳ Waiting for IGM {}/{} to stabilize after rolling update (stable={}, version_reached={}, instances=[{}])",
                zone,
                igm_name,
                is_stable,
                version_reached,
                instance_summaries.join("; ")
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    fn create_invalid_client(&self) -> ComputeClient {
        let invalid_config = GcpClientConfig {
            project_id: "fake-project".to_string(),
            region: self.region.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
            },
            service_overrides: None,
            project_number: None,
        };
        ComputeClient::new(Client::new(), invalid_config)
    }
}

// =============================================================================================
// Basic Framework Test
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_framework_setup_compute(ctx: &mut ComputeTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    assert!(!ctx.region.is_empty(), "Region should not be empty");

    println!(
        "Successfully connected to Compute Engine in project: {} region: {}",
        ctx.project_id, ctx.region
    );
}

// =============================================================================================
// Comprehensive E2E Test - VPC with all components
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_vpc_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive VPC lifecycle test");

    // Generate unique names for all resources
    let network_name = ctx.generate_unique_name("vpc");
    let subnetwork_name = ctx.generate_unique_name("subnet");
    let router_name = ctx.generate_unique_name("router");
    let firewall_name = ctx.generate_unique_name("fw");

    // =========================================================================
    // Step 1: Create VPC Network
    // =========================================================================
    println!("\n📦 Step 1: Creating VPC network: {}", network_name);

    let network = Network::builder()
        .name(network_name.clone())
        .description("Alien test VPC network".to_string())
        .auto_create_subnetworks(false) // We'll create subnets manually
        .routing_config(
            NetworkRoutingConfig::builder()
                .routing_mode(RoutingMode::Regional)
                .build(),
        )
        .mtu(1460)
        .build();

    let create_network_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network");

    ctx.track_network(&network_name);
    assert!(
        create_network_op.name.is_some(),
        "Create network operation should have a name"
    );
    println!("✅ Network creation initiated");

    // Wait for network creation to complete
    ctx.wait_for_global_operation(create_network_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Network creation operation timed out");

    // Verify network was created
    let fetched_network = ctx
        .client
        .get_network(network_name.clone())
        .await
        .expect("Failed to get network");

    assert_eq!(
        fetched_network.name.as_ref().unwrap(),
        &network_name,
        "Network name should match"
    );
    assert_eq!(
        fetched_network.auto_create_subnetworks,
        Some(false),
        "autoCreateSubnetworks should be false"
    );
    println!("✅ Network verified: {}", network_name);

    // =========================================================================
    // Step 2: Create Subnetwork
    // =========================================================================
    println!("\n📦 Step 2: Creating subnetwork: {}", subnetwork_name);

    let network_url = format!(
        "projects/{}/global/networks/{}",
        ctx.project_id, network_name
    );

    let subnetwork = Subnetwork::builder()
        .name(subnetwork_name.clone())
        .description("Alien test subnetwork".to_string())
        .network(network_url.clone())
        .ip_cidr_range("10.128.0.0/20".to_string())
        .private_ip_google_access(true)
        .build();

    let create_subnet_op = ctx
        .client
        .insert_subnetwork(ctx.region.clone(), subnetwork)
        .await
        .expect("Failed to create subnetwork");

    ctx.track_subnetwork(&ctx.region, &subnetwork_name);
    assert!(
        create_subnet_op.name.is_some(),
        "Create subnetwork operation should have a name"
    );
    println!("✅ Subnetwork creation initiated");

    // Wait for subnetwork creation to complete
    ctx.wait_for_region_operation(&ctx.region, create_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork creation operation timed out");

    // Verify subnetwork was created
    let fetched_subnetwork = ctx
        .client
        .get_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to get subnetwork");

    assert_eq!(
        fetched_subnetwork.name.as_ref().unwrap(),
        &subnetwork_name,
        "Subnetwork name should match"
    );
    assert_eq!(
        fetched_subnetwork.private_ip_google_access,
        Some(true),
        "Private Google Access should be enabled"
    );
    println!("✅ Subnetwork verified: {}", subnetwork_name);

    // =========================================================================
    // Step 3: Create Router with Cloud NAT
    // =========================================================================
    println!(
        "\n📦 Step 3: Creating router with Cloud NAT: {}",
        router_name
    );

    let router = Router::builder()
        .name(router_name.clone())
        .description("Alien test router with NAT".to_string())
        .network(network_url.clone())
        .nats(vec![RouterNat::builder()
            .name("alien-test-nat".to_string())
            .source_subnetwork_ip_ranges_to_nat(
                SourceSubnetworkIpRangesToNat::AllSubnetworksAllIpRanges,
            )
            .nat_ip_allocate_option(NatIpAllocateOption::AutoOnly)
            .enable_endpoint_independent_mapping(false)
            .enable_dynamic_port_allocation(true)
            .min_ports_per_vm(64)
            .build()])
        .build();

    let create_router_op = ctx
        .client
        .insert_router(ctx.region.clone(), router)
        .await
        .expect("Failed to create router");

    ctx.track_router(&ctx.region, &router_name);
    assert!(
        create_router_op.name.is_some(),
        "Create router operation should have a name"
    );
    println!("✅ Router creation initiated");

    // Wait for router creation to complete
    ctx.wait_for_region_operation(&ctx.region, create_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router creation operation timed out");

    // Verify router was created
    let fetched_router = ctx
        .client
        .get_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to get router");

    assert_eq!(
        fetched_router.name.as_ref().unwrap(),
        &router_name,
        "Router name should match"
    );
    assert!(
        !fetched_router.nats.is_empty(),
        "Router should have NAT configuration"
    );
    println!("✅ Router verified: {}", router_name);

    // Test list_routers
    println!("\n📋 Testing list_routers...");
    let router_list = ctx
        .client
        .list_routers(ctx.region.clone())
        .await
        .expect("Failed to list routers");

    let found_router = router_list
        .items
        .iter()
        .find(|r| r.name.as_ref() == Some(&router_name));
    assert!(found_router.is_some(), "Router should be in list");
    println!("✅ Router found in list");

    // =========================================================================
    // Step 4: Create Firewall Rule
    // =========================================================================
    println!("\n📦 Step 4: Creating firewall rule: {}", firewall_name);

    let firewall = Firewall::builder()
        .name(firewall_name.clone())
        .description("Alien test firewall rule".to_string())
        .network(network_url.clone())
        .direction(FirewallDirection::Ingress)
        .priority(1000)
        .allowed(vec![
            FirewallAllowed::builder()
                .ip_protocol("tcp".to_string())
                .ports(vec!["22".to_string(), "80".to_string(), "443".to_string()])
                .build(),
            FirewallAllowed::builder()
                .ip_protocol("icmp".to_string())
                .build(),
        ])
        .source_ranges(vec!["0.0.0.0/0".to_string()])
        .build();

    let create_firewall_op = ctx
        .client
        .insert_firewall(firewall)
        .await
        .expect("Failed to create firewall");

    ctx.track_firewall(&firewall_name);
    assert!(
        create_firewall_op.name.is_some(),
        "Create firewall operation should have a name"
    );
    println!("✅ Firewall creation initiated");

    // Wait for firewall creation to complete
    ctx.wait_for_global_operation(create_firewall_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Firewall creation operation timed out");

    // Verify firewall was created
    let fetched_firewall = ctx
        .client
        .get_firewall(firewall_name.clone())
        .await
        .expect("Failed to get firewall");

    assert_eq!(
        fetched_firewall.name.as_ref().unwrap(),
        &firewall_name,
        "Firewall name should match"
    );
    assert_eq!(
        fetched_firewall.direction,
        Some(FirewallDirection::Ingress),
        "Firewall direction should be INGRESS"
    );
    println!("✅ Firewall verified: {}", firewall_name);

    // Test list_firewalls
    println!("\n📋 Testing list_firewalls...");
    let firewall_list = ctx
        .client
        .list_firewalls()
        .await
        .expect("Failed to list firewalls");

    let found_firewall = firewall_list
        .items
        .iter()
        .find(|f| f.name.as_ref() == Some(&firewall_name));
    assert!(found_firewall.is_some(), "Firewall should be in list");
    println!("✅ Firewall found in list");

    // =========================================================================
    // Step 5: Patch Router (update NAT config)
    // =========================================================================
    println!("\n📦 Step 5: Patching router NAT configuration");

    let updated_router = Router::builder()
        .nats(vec![RouterNat::builder()
            .name("alien-test-nat".to_string())
            .source_subnetwork_ip_ranges_to_nat(
                SourceSubnetworkIpRangesToNat::AllSubnetworksAllIpRanges,
            )
            .nat_ip_allocate_option(NatIpAllocateOption::AutoOnly)
            .enable_endpoint_independent_mapping(false)
            .enable_dynamic_port_allocation(true)
            .min_ports_per_vm(128) // Changed from 64 to 128
            .max_ports_per_vm(65536)
            .build()])
        .build();

    let patch_router_op = ctx
        .client
        .patch_router(ctx.region.clone(), router_name.clone(), updated_router)
        .await
        .expect("Failed to patch router");

    assert!(
        patch_router_op.name.is_some(),
        "Patch router operation should have a name"
    );
    println!("✅ Router patch initiated");

    // Wait for patch to complete
    ctx.wait_for_region_operation(&ctx.region, patch_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router patch operation timed out");

    // Verify router was updated
    let updated_router_check = ctx
        .client
        .get_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to get updated router");

    let nat_config = &updated_router_check.nats[0];
    assert_eq!(
        nat_config.min_ports_per_vm,
        Some(128),
        "NAT min_ports_per_vm should be updated to 128"
    );
    println!("✅ Router NAT configuration updated successfully");

    // =========================================================================
    // Step 6: Clean up in reverse order
    // =========================================================================
    println!("\n🧹 Step 6: Cleaning up resources");

    // Delete firewall
    println!("  Deleting firewall...");
    let delete_firewall_op = ctx
        .client
        .delete_firewall(firewall_name.clone())
        .await
        .expect("Failed to delete firewall");

    ctx.wait_for_global_operation(delete_firewall_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Firewall deletion operation timed out");
    ctx.untrack_firewall(&firewall_name);
    println!("  ✅ Firewall deleted");

    // Delete router
    println!("  Deleting router...");
    let delete_router_op = ctx
        .client
        .delete_router(ctx.region.clone(), router_name.clone())
        .await
        .expect("Failed to delete router");

    ctx.wait_for_region_operation(&ctx.region, delete_router_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Router deletion operation timed out");
    ctx.untrack_router(&ctx.region, &router_name);
    println!("  ✅ Router deleted");

    // Delete subnetwork
    println!("  Deleting subnetwork...");
    let delete_subnet_op = ctx
        .client
        .delete_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to delete subnetwork");

    ctx.wait_for_region_operation(&ctx.region, delete_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork deletion operation timed out");
    ctx.untrack_subnetwork(&ctx.region, &subnetwork_name);
    println!("  ✅ Subnetwork deleted");

    // Delete network
    println!("  Deleting network...");
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
    println!("  ✅ Network deleted");

    println!("\n🎉 Comprehensive VPC lifecycle test completed successfully!");
    println!("   - VPC network created and deleted: ✅");
    println!("   - Subnetwork created and deleted: ✅");
    println!("   - Router with NAT created, patched, and deleted: ✅");
    println!("   - Firewall rule created and deleted: ✅");
    println!("   - List operations verified: ✅");
}

// =============================================================================================
// Error Handling Tests
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_network_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-network-does-not-exist-12345";

    let result = ctx.client.get_network(non_existent.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent network");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for network");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_subnetwork_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-subnet-does-not-exist-12345";

    let result = ctx
        .client
        .get_subnetwork(ctx.region.clone(), non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent subnetwork"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for subnetwork");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_router_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-router-does-not-exist-12345";

    let result = ctx
        .client
        .get_router(ctx.region.clone(), non_existent.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent router");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for router");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_firewall_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-firewall-does-not-exist-12345";

    let result = ctx.client.get_firewall(non_existent.to_string()).await;
    assert!(result.is_err(), "Expected error for non-existent firewall");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for firewall");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_access_denied(ctx: &mut ComputeTestContext) {
    let invalid_client = ctx.create_invalid_client();

    let result = invalid_client.get_network("any-network".to_string()).await;
    assert!(result.is_err(), "Expected error with invalid credentials");

    let err = result.unwrap_err();
    match &err.error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        _ => println!("Got error (acceptable for invalid creds): {:?}", err),
    }
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_delete_non_existent_network(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-network-does-not-exist-67890";

    let result = ctx.client.delete_network(non_existent.to_string()).await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent network"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Compute Engine");
            assert_eq!(resource_name, non_existent);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for network deletion");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent network deletion, got: {:?}",
            err
        ),
    }
}

// =============================================================================================
// Operation Status Tests
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_wait_global_operation(ctx: &mut ComputeTestContext) {
    // Create a simple network to get an operation
    let network_name = ctx.generate_unique_name("op-test");

    let network = Network::builder()
        .name(network_name.clone())
        .description("Operation test network".to_string())
        .auto_create_subnetworks(false)
        .build();

    let create_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network for operation test");

    ctx.track_network(&network_name);
    let op_name = create_op.name.as_ref().unwrap();

    // Test wait_global_operation
    println!("Testing wait_global_operation...");
    let wait_result = ctx
        .client
        .wait_global_operation(ctx.extract_operation_name(op_name))
        .await
        .expect("Failed to wait for global operation");

    assert!(
        wait_result.is_done(),
        "Operation should be done after waiting"
    );
    println!("✅ wait_global_operation completed successfully");

    // Clean up
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
}

// =============================================================================================
// Comprehensive E2E Test - Load Balancing
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_load_balancing_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Load Balancing lifecycle test");

    // Generate unique names for all resources
    let network_name = ctx.generate_unique_name("lb-vpc");
    let subnetwork_name = ctx.generate_unique_name("lb-subnet");
    let health_check_name = ctx.generate_unique_name("hc");
    let neg_name = ctx.generate_unique_name("neg");
    let backend_service_name = ctx.generate_unique_name("bs");
    let url_map_name = ctx.generate_unique_name("urlmap");
    let proxy_name = ctx.generate_unique_name("proxy");
    let address_name = ctx.generate_unique_name("addr");
    let forwarding_rule_name = ctx.generate_unique_name("fwd");

    // =========================================================================
    // Step 1: Create VPC Network and Subnetwork (prerequisites)
    // =========================================================================
    println!("\n📦 Step 1: Creating VPC network and subnetwork for load balancing");

    let network = Network::builder()
        .name(network_name.clone())
        .description("Alien test VPC for load balancing".to_string())
        .auto_create_subnetworks(false)
        .build();

    let create_network_op = ctx
        .client
        .insert_network(network)
        .await
        .expect("Failed to create network");

    ctx.track_network(&network_name);
    ctx.wait_for_global_operation(create_network_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Network creation timed out");
    println!("✅ Network created: {}", network_name);

    let network_url = format!(
        "projects/{}/global/networks/{}",
        ctx.project_id, network_name
    );

    let subnetwork = Subnetwork::builder()
        .name(subnetwork_name.clone())
        .network(network_url.clone())
        .ip_cidr_range("10.0.0.0/24".to_string())
        .build();

    let create_subnet_op = ctx
        .client
        .insert_subnetwork(ctx.region.clone(), subnetwork)
        .await
        .expect("Failed to create subnetwork");

    ctx.track_subnetwork(&ctx.region, &subnetwork_name);
    ctx.wait_for_region_operation(&ctx.region, create_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork creation timed out");
    println!("✅ Subnetwork created: {}", subnetwork_name);

    // =========================================================================
    // Step 2: Create Health Check
    // =========================================================================
    println!("\n📦 Step 2: Creating health check: {}", health_check_name);

    let health_check = HealthCheck::builder()
        .name(health_check_name.clone())
        .description("Alien test health check".to_string())
        .r#type(HealthCheckType::Http)
        .check_interval_sec(10)
        .timeout_sec(5)
        .healthy_threshold(2)
        .unhealthy_threshold(3)
        .http_health_check(
            HttpHealthCheck::builder()
                .port(80)
                .request_path("/health".to_string())
                .build(),
        )
        .build();

    let create_hc_op = ctx
        .client
        .insert_health_check(health_check)
        .await
        .expect("Failed to create health check");

    ctx.track_health_check(&health_check_name);
    assert!(
        create_hc_op.name.is_some(),
        "Create health check operation should have a name"
    );
    println!("✅ Health check creation initiated");

    ctx.wait_for_global_operation(create_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check creation timed out");

    // Verify health check was created
    let fetched_hc = ctx
        .client
        .get_health_check(health_check_name.clone())
        .await
        .expect("Failed to get health check");

    assert_eq!(fetched_hc.name.as_ref().unwrap(), &health_check_name);
    assert_eq!(fetched_hc.r#type, Some(HealthCheckType::Http));
    println!("✅ Health check verified: {}", health_check_name);

    // =========================================================================
    // Step 3: Create Network Endpoint Group (NEG)
    // =========================================================================
    println!("\n📦 Step 3: Creating NEG: {}", neg_name);

    let subnetwork_url = format!(
        "projects/{}/regions/{}/subnetworks/{}",
        ctx.project_id, ctx.region, subnetwork_name
    );

    let neg = NetworkEndpointGroup::builder()
        .name(neg_name.clone())
        .description("Alien test NEG".to_string())
        .network_endpoint_type(NetworkEndpointType::GceVmIpPort)
        .network(network_url.clone())
        .subnetwork(subnetwork_url)
        .default_port(80)
        .build();

    let create_neg_op = ctx
        .client
        .insert_network_endpoint_group(ctx.zone.clone(), neg)
        .await
        .expect("Failed to create NEG");

    ctx.track_neg(&ctx.zone, &neg_name);
    assert!(
        create_neg_op.name.is_some(),
        "Create NEG operation should have a name"
    );
    println!("✅ NEG creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_neg_op.name.as_ref().unwrap(), 120)
        .await
        .expect("NEG creation timed out");

    // Verify NEG was created
    let fetched_neg = ctx
        .client
        .get_network_endpoint_group(ctx.zone.clone(), neg_name.clone())
        .await
        .expect("Failed to get NEG");

    assert_eq!(fetched_neg.name.as_ref().unwrap(), &neg_name);
    assert_eq!(
        fetched_neg.network_endpoint_type,
        Some(NetworkEndpointType::GceVmIpPort)
    );
    println!("✅ NEG verified: {}", neg_name);

    // =========================================================================
    // Step 4: Create Backend Service
    // =========================================================================
    println!(
        "\n📦 Step 4: Creating backend service: {}",
        backend_service_name
    );

    let health_check_url = format!(
        "projects/{}/global/healthChecks/{}",
        ctx.project_id, health_check_name
    );

    let neg_url = format!(
        "projects/{}/zones/{}/networkEndpointGroups/{}",
        ctx.project_id, ctx.zone, neg_name
    );

    let backend_service = BackendService::builder()
        .name(backend_service_name.clone())
        .description("Alien test backend service".to_string())
        .protocol(BackendServiceProtocol::Http)
        .port_name("http".to_string())
        .timeout_sec(30)
        .health_checks(vec![health_check_url])
        .load_balancing_scheme(LoadBalancingScheme::External)
        .backends(vec![Backend::builder()
            .group(neg_url)
            .balancing_mode(BalancingMode::Rate)
            .max_rate_per_endpoint(100.0)
            .build()])
        .build();

    let create_bs_op = ctx
        .client
        .insert_backend_service(backend_service)
        .await
        .expect("Failed to create backend service");

    ctx.track_backend_service(&backend_service_name);
    assert!(
        create_bs_op.name.is_some(),
        "Create backend service operation should have a name"
    );
    println!("✅ Backend service creation initiated");

    ctx.wait_for_global_operation(create_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service creation timed out");

    // Verify backend service was created
    let fetched_bs = ctx
        .client
        .get_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to get backend service");

    assert_eq!(fetched_bs.name.as_ref().unwrap(), &backend_service_name);
    assert_eq!(fetched_bs.protocol, Some(BackendServiceProtocol::Http));
    println!("✅ Backend service verified: {}", backend_service_name);

    // =========================================================================
    // Step 5: Create URL Map
    // =========================================================================
    println!("\n📦 Step 5: Creating URL map: {}", url_map_name);

    let backend_service_url = format!(
        "projects/{}/global/backendServices/{}",
        ctx.project_id, backend_service_name
    );

    let url_map = UrlMap::builder()
        .name(url_map_name.clone())
        .description("Alien test URL map".to_string())
        .default_service(backend_service_url)
        .build();

    let create_um_op = ctx
        .client
        .insert_url_map(url_map)
        .await
        .expect("Failed to create URL map");

    ctx.track_url_map(&url_map_name);
    assert!(
        create_um_op.name.is_some(),
        "Create URL map operation should have a name"
    );
    println!("✅ URL map creation initiated");

    ctx.wait_for_global_operation(create_um_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map creation timed out");

    // Verify URL map was created
    let fetched_um = ctx
        .client
        .get_url_map(url_map_name.clone())
        .await
        .expect("Failed to get URL map");

    assert_eq!(fetched_um.name.as_ref().unwrap(), &url_map_name);
    println!("✅ URL map verified: {}", url_map_name);

    // =========================================================================
    // Step 6: Create Target HTTP Proxy
    // =========================================================================
    println!("\n📦 Step 6: Creating target HTTP proxy: {}", proxy_name);

    let url_map_url = format!(
        "projects/{}/global/urlMaps/{}",
        ctx.project_id, url_map_name
    );

    let target_http_proxy = TargetHttpProxy::builder()
        .name(proxy_name.clone())
        .description("Alien test target HTTP proxy".to_string())
        .url_map(url_map_url)
        .build();

    let create_proxy_op = ctx
        .client
        .insert_target_http_proxy(target_http_proxy)
        .await
        .expect("Failed to create target HTTP proxy");

    ctx.track_target_http_proxy(&proxy_name);
    assert!(
        create_proxy_op.name.is_some(),
        "Create proxy operation should have a name"
    );
    println!("✅ Target HTTP proxy creation initiated");

    ctx.wait_for_global_operation(create_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTP proxy creation timed out");

    // Verify proxy was created
    let fetched_proxy = ctx
        .client
        .get_target_http_proxy(proxy_name.clone())
        .await
        .expect("Failed to get target HTTP proxy");

    assert_eq!(fetched_proxy.name.as_ref().unwrap(), &proxy_name);
    println!("✅ Target HTTP proxy verified: {}", proxy_name);

    // =========================================================================
    // Step 7: Create Global Address
    // =========================================================================
    println!("\n📦 Step 7: Creating global address: {}", address_name);

    let address = Address::builder()
        .name(address_name.clone())
        .description("Alien test global address".to_string())
        .build();

    let create_addr_op = ctx
        .client
        .insert_global_address(address)
        .await
        .expect("Failed to create global address");

    ctx.track_global_address(&address_name);
    assert!(
        create_addr_op.name.is_some(),
        "Create address operation should have a name"
    );
    println!("✅ Global address creation initiated");

    ctx.wait_for_global_operation(create_addr_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Global address creation timed out");

    // Verify address was created
    let fetched_addr = ctx
        .client
        .get_global_address(address_name.clone())
        .await
        .expect("Failed to get global address");

    assert_eq!(fetched_addr.name.as_ref().unwrap(), &address_name);
    assert!(fetched_addr.address.is_some(), "Address should have an IP");
    println!(
        "✅ Global address verified: {} (IP: {})",
        address_name,
        fetched_addr.address.as_ref().unwrap()
    );

    // =========================================================================
    // Step 8: Create Global Forwarding Rule
    // =========================================================================
    println!(
        "\n📦 Step 8: Creating global forwarding rule: {}",
        forwarding_rule_name
    );

    let proxy_url = format!(
        "projects/{}/global/targetHttpProxies/{}",
        ctx.project_id, proxy_name
    );

    let forwarding_rule = ForwardingRule::builder()
        .name(forwarding_rule_name.clone())
        .description("Alien test forwarding rule".to_string())
        .ip_address(fetched_addr.address.clone().unwrap())
        .ip_protocol(ForwardingRuleProtocol::Tcp)
        .port_range("80-80".to_string())
        .target(proxy_url)
        .load_balancing_scheme(LoadBalancingScheme::External)
        .build();

    let create_fwd_op = ctx
        .client
        .insert_global_forwarding_rule(forwarding_rule)
        .await
        .expect("Failed to create forwarding rule");

    ctx.track_global_forwarding_rule(&forwarding_rule_name);
    assert!(
        create_fwd_op.name.is_some(),
        "Create forwarding rule operation should have a name"
    );
    println!("✅ Global forwarding rule creation initiated");

    ctx.wait_for_global_operation(create_fwd_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Global forwarding rule creation timed out");

    // Verify forwarding rule was created
    let fetched_fwd = ctx
        .client
        .get_global_forwarding_rule(forwarding_rule_name.clone())
        .await
        .expect("Failed to get forwarding rule");

    assert_eq!(fetched_fwd.name.as_ref().unwrap(), &forwarding_rule_name);
    println!(
        "✅ Global forwarding rule verified: {}",
        forwarding_rule_name
    );

    // =========================================================================
    // Step 9: Test patch backend service
    // =========================================================================
    println!("\n📦 Step 9: Patching backend service");

    let patched_bs = BackendService::builder()
        .timeout_sec(60) // Changed from 30 to 60
        .build();

    let patch_bs_op = ctx
        .client
        .patch_backend_service(backend_service_name.clone(), patched_bs)
        .await
        .expect("Failed to patch backend service");

    ctx.wait_for_global_operation(patch_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service patch timed out");

    let updated_bs = ctx
        .client
        .get_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to get updated backend service");

    assert_eq!(updated_bs.timeout_sec, Some(60));
    println!("✅ Backend service patched successfully");

    // =========================================================================
    // Step 10: Cleanup in reverse dependency order
    // =========================================================================
    println!("\n🧹 Step 10: Cleaning up load balancing resources");

    // Delete forwarding rule
    let delete_fwd_op = ctx
        .client
        .delete_global_forwarding_rule(forwarding_rule_name.clone())
        .await
        .expect("Failed to delete forwarding rule");
    ctx.wait_for_global_operation(delete_fwd_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Forwarding rule deletion timed out");
    ctx.untrack_global_forwarding_rule(&forwarding_rule_name);
    println!("  ✅ Forwarding rule deleted");

    // Delete target HTTP proxy
    let delete_proxy_op = ctx
        .client
        .delete_target_http_proxy(proxy_name.clone())
        .await
        .expect("Failed to delete proxy");
    ctx.wait_for_global_operation(delete_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Proxy deletion timed out");
    ctx.untrack_target_http_proxy(&proxy_name);
    println!("  ✅ Target HTTP proxy deleted");

    // Delete URL map
    let delete_um_op = ctx
        .client
        .delete_url_map(url_map_name.clone())
        .await
        .expect("Failed to delete URL map");
    ctx.wait_for_global_operation(delete_um_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map deletion timed out");
    ctx.untrack_url_map(&url_map_name);
    println!("  ✅ URL map deleted");

    // Delete backend service
    let delete_bs_op = ctx
        .client
        .delete_backend_service(backend_service_name.clone())
        .await
        .expect("Failed to delete backend service");
    ctx.wait_for_global_operation(delete_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service deletion timed out");
    ctx.untrack_backend_service(&backend_service_name);
    println!("  ✅ Backend service deleted");

    // Delete NEG
    let delete_neg_op = ctx
        .client
        .delete_network_endpoint_group(ctx.zone.clone(), neg_name.clone())
        .await
        .expect("Failed to delete NEG");
    ctx.wait_for_zone_operation(&ctx.zone, delete_neg_op.name.as_ref().unwrap(), 120)
        .await
        .expect("NEG deletion timed out");
    ctx.untrack_neg(&ctx.zone, &neg_name);
    println!("  ✅ NEG deleted");

    // Delete health check
    let delete_hc_op = ctx
        .client
        .delete_health_check(health_check_name.clone())
        .await
        .expect("Failed to delete health check");
    ctx.wait_for_global_operation(delete_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check deletion timed out");
    ctx.untrack_health_check(&health_check_name);
    println!("  ✅ Health check deleted");

    // Delete global address
    let delete_addr_op = ctx
        .client
        .delete_global_address(address_name.clone())
        .await
        .expect("Failed to delete address");
    ctx.wait_for_global_operation(delete_addr_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Address deletion timed out");
    ctx.untrack_global_address(&address_name);
    println!("  ✅ Global address deleted");

    // Delete subnetwork
    let delete_subnet_op = ctx
        .client
        .delete_subnetwork(ctx.region.clone(), subnetwork_name.clone())
        .await
        .expect("Failed to delete subnetwork");
    ctx.wait_for_region_operation(&ctx.region, delete_subnet_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Subnetwork deletion timed out");
    ctx.untrack_subnetwork(&ctx.region, &subnetwork_name);
    println!("  ✅ Subnetwork deleted");

    // Delete network
    ctx.delete_network_with_retry(&network_name, NETWORK_DELETE_TIMEOUT_SECONDS)
        .await
        .expect("Failed to delete network");
    ctx.untrack_network(&network_name);
    println!("  ✅ Network deleted");

    println!("\n🎉 Comprehensive Load Balancing lifecycle test completed successfully!");
    println!("   - Health check created and deleted: ✅");
    println!("   - Network Endpoint Group created and deleted: ✅");
    println!("   - Backend service created, patched, and deleted: ✅");
    println!("   - URL map created and deleted: ✅");
    println!("   - Target HTTP proxy created and deleted: ✅");
    println!("   - Global address created and deleted: ✅");
    println!("   - Global forwarding rule created and deleted: ✅");
}

// =============================================================================================
// Comprehensive E2E Test - Persistent Disk
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_disk_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Persistent Disk lifecycle test");

    let disk_name = ctx.generate_unique_name("disk");

    // =========================================================================
    // Step 1: Create Disk
    // =========================================================================
    println!("\n📦 Step 1: Creating disk: {}", disk_name);

    let disk = Disk::builder()
        .name(disk_name.clone())
        .description("Alien test persistent disk".to_string())
        .size_gb("10".to_string())
        .r#type(format!(
            "projects/{}/zones/{}/diskTypes/pd-standard",
            ctx.project_id, ctx.zone
        ))
        .build();

    let create_disk_op = ctx
        .client
        .insert_disk(ctx.zone.clone(), disk)
        .await
        .expect("Failed to create disk");

    ctx.track_disk(&ctx.zone, &disk_name);
    assert!(
        create_disk_op.name.is_some(),
        "Create disk operation should have a name"
    );
    println!("✅ Disk creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk creation timed out");

    // Verify disk was created
    let fetched_disk = ctx
        .client
        .get_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to get disk");

    assert_eq!(fetched_disk.name.as_ref().unwrap(), &disk_name);
    assert_eq!(fetched_disk.size_gb, Some("10".to_string()));
    println!("✅ Disk verified: {}", disk_name);

    // =========================================================================
    // Step 2: Delete Disk
    // =========================================================================
    println!("\n🧹 Step 2: Deleting disk");

    let delete_disk_op = ctx
        .client
        .delete_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to delete disk");

    ctx.wait_for_zone_operation(&ctx.zone, delete_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk deletion timed out");
    ctx.untrack_disk(&ctx.zone, &disk_name);
    println!("✅ Disk deleted");

    // Verify disk was deleted (should return 404)
    let result = ctx
        .client
        .get_disk(ctx.zone.clone(), disk_name.clone())
        .await;
    assert!(result.is_err(), "Disk should be deleted");

    println!("\n🎉 Comprehensive Persistent Disk lifecycle test completed successfully!");
    println!("   - Disk created and deleted: ✅");
}

// =============================================================================================
// Comprehensive E2E Test - Instance Management
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_instance_management_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Instance Management lifecycle test");

    let template_name = ctx.generate_unique_name("template");
    let igm_name = ctx.generate_unique_name("igm");

    // =========================================================================
    // Step 1: Create Instance Template
    // =========================================================================
    println!("\n📦 Step 1: Creating instance template: {}", template_name);

    let instance_template = InstanceTemplate::builder()
        .name(template_name.clone())
        .description("Alien test instance template".to_string())
        .properties(
            InstanceProperties::builder()
                .machine_type("e2-micro".to_string())
                .disks(vec![AttachedDisk::builder()
                    .r#type(AttachedDiskType::Persistent)
                    .boot(true)
                    .mode(DiskMode::ReadWrite)
                    .auto_delete(true)
                    .initialize_params(
                        AttachedDiskInitializeParams::builder()
                            .source_image(
                                "projects/debian-cloud/global/images/family/debian-11".to_string(),
                            )
                            .disk_size_gb("10".to_string())
                            .build(),
                    )
                    .build()])
                .network_interfaces(vec![NetworkInterface::builder()
                    .network("global/networks/default".to_string())
                    .build()])
                .service_accounts(vec![ServiceAccount::builder()
                    .email("default".to_string())
                    .scopes(vec![
                        "https://www.googleapis.com/auth/cloud-platform".to_string()
                    ])
                    .build()])
                .build(),
        )
        .build();

    let create_template_op = ctx
        .client
        .insert_instance_template(instance_template)
        .await
        .expect("Failed to create instance template");

    ctx.track_instance_template(&template_name);
    assert!(
        create_template_op.name.is_some(),
        "Create template operation should have a name"
    );
    println!("✅ Instance template creation initiated");

    ctx.wait_for_global_operation(create_template_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Instance template creation timed out");

    // Verify template was created
    let fetched_template = ctx
        .client
        .get_instance_template(template_name.clone())
        .await
        .expect("Failed to get instance template");

    assert_eq!(fetched_template.name.as_ref().unwrap(), &template_name);
    println!("✅ Instance template verified: {}", template_name);

    // =========================================================================
    // Step 2: Create Instance Group Manager
    // =========================================================================
    println!("\n📦 Step 2: Creating instance group manager: {}", igm_name);

    let template_url = format!(
        "projects/{}/global/instanceTemplates/{}",
        ctx.project_id, template_name
    );

    let igm = InstanceGroupManager::builder()
        .name(igm_name.clone())
        .description("Alien test instance group manager".to_string())
        .instance_template(template_url)
        .base_instance_name(format!("alien-test-{}", &igm_name[..8.min(igm_name.len())]))
        .target_size(0) // Start with 0 instances
        .update_policy(
            InstanceGroupManagerUpdatePolicy::builder()
                .r#type(UpdatePolicyType::Proactive)
                .build(),
        )
        .build();

    let create_igm_op = ctx
        .client
        .insert_instance_group_manager(ctx.zone.clone(), igm)
        .await
        .expect("Failed to create instance group manager");

    ctx.track_instance_group_manager(&ctx.zone, &igm_name);
    assert!(
        create_igm_op.name.is_some(),
        "Create IGM operation should have a name"
    );
    println!("✅ Instance group manager creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_igm_op.name.as_ref().unwrap(), 180)
        .await
        .expect("Instance group manager creation timed out");

    // Verify IGM was created
    let fetched_igm = ctx
        .client
        .get_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to get instance group manager");

    assert_eq!(fetched_igm.name.as_ref().unwrap(), &igm_name);
    assert_eq!(fetched_igm.target_size, Some(0));
    println!("✅ Instance group manager verified: {}", igm_name);

    // =========================================================================
    // Step 3: Resize Instance Group Manager
    // =========================================================================
    println!("\n📦 Step 3: Resizing instance group manager to 1 instance");

    let resize_op = ctx
        .client
        .resize_instance_group_manager(ctx.zone.clone(), igm_name.clone(), 1)
        .await
        .expect("Failed to resize IGM");

    assert!(
        resize_op.name.is_some(),
        "Resize operation should have a name"
    );
    println!("✅ Resize operation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, resize_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Resize operation timed out");

    // Wait a bit for instances to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // =========================================================================
    // Step 3.5: Create a second instance template for patch test
    // =========================================================================
    println!("\n📦 Step 3.5: Creating second instance template for patch test");

    let template_v2_name = ctx.generate_unique_name("template-v2");

    let instance_template_v2 = InstanceTemplate::builder()
        .name(template_v2_name.clone())
        .description(
            "Alien test instance template v2 (for patch_instance_group_manager)".to_string(),
        )
        .properties(
            InstanceProperties::builder()
                .machine_type("e2-micro".to_string())
                .disks(vec![AttachedDisk::builder()
                    .r#type(AttachedDiskType::Persistent)
                    .boot(true)
                    .mode(DiskMode::ReadWrite)
                    .auto_delete(true)
                    .initialize_params(
                        AttachedDiskInitializeParams::builder()
                            .source_image(
                                "projects/debian-cloud/global/images/family/debian-11".to_string(),
                            )
                            .disk_size_gb("10".to_string())
                            .build(),
                    )
                    .build()])
                .network_interfaces(vec![NetworkInterface::builder()
                    .network("global/networks/default".to_string())
                    .build()])
                .service_accounts(vec![ServiceAccount::builder()
                    .email("default".to_string())
                    .scopes(vec![
                        "https://www.googleapis.com/auth/cloud-platform".to_string()
                    ])
                    .build()])
                .build(),
        )
        .build();

    let create_template_v2_op = ctx
        .client
        .insert_instance_template(instance_template_v2)
        .await
        .expect("Failed to create second instance template");

    ctx.track_instance_template(&template_v2_name);
    ctx.wait_for_global_operation(create_template_v2_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Second instance template creation timed out");
    println!("✅ Second instance template created: {}", template_v2_name);

    // =========================================================================
    // Step 3.6: Patch the IGM to use the new template with PROACTIVE update policy
    // =========================================================================
    println!("\n📦 Step 3.6: Patching IGM to use new template with PROACTIVE rolling update");

    let template_v2_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/global/instanceTemplates/{}",
        ctx.project_id, template_v2_name
    );

    let igm_patch = InstanceGroupManager::builder()
        .instance_template(template_v2_url.clone())
        .update_policy(InstanceGroupManagerUpdatePolicy {
            r#type: Some(UpdatePolicyType::Proactive),
            minimal_action: Some(MinimalAction::Replace),
            most_disruptive_allowed_action: None,
            // maxSurge: 1 — create 1 extra VM before terminating old (works with target_size=1)
            max_surge: Some(FixedOrPercent {
                fixed: Some(1),
                percent: None,
                calculated: None,
            }),
            // maxUnavailable: 0 — never reduce capacity below target_size
            max_unavailable: Some(FixedOrPercent {
                fixed: Some(0),
                percent: None,
                calculated: None,
            }),
            replacement_method: None,
        })
        .build();

    let patch_op = ctx
        .client
        .patch_instance_group_manager(ctx.zone.clone(), igm_name.clone(), igm_patch)
        .await
        .expect("Failed to patch instance group manager");

    assert!(
        patch_op.name.is_some(),
        "Patch operation should have a name"
    );
    println!("✅ Patch operation initiated: {:?}", patch_op.name);

    ctx.wait_for_zone_operation(&ctx.zone, patch_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Patch operation timed out");

    // =========================================================================
    // Step 3.7: Verify the IGM now references the new template
    // =========================================================================
    println!("\n📦 Step 3.7: Verifying IGM references new template after patch");

    let patched_igm = ctx
        .client
        .get_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to get patched IGM");

    let current_template = patched_igm.instance_template.as_deref().unwrap_or("");
    assert!(
        current_template.contains(&template_v2_name),
        "IGM should now reference the new template '{}', but has '{}'",
        template_v2_name,
        current_template
    );
    println!(
        "✅ IGM correctly references new template: {}",
        template_v2_name
    );

    let stable_managed_instance = ctx
        .wait_for_stable_managed_instance(&ctx.zone, &igm_name, &template_v2_name, 300)
        .await
        .expect("Managed instance group never converged on the patched template");

    // =========================================================================
    // Step 4: List Managed Instances
    // =========================================================================
    println!("\n📦 Step 4: Listing managed instances");

    let managed_instances = ctx
        .client
        .list_managed_instances(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to list managed instances");

    println!(
        "  Found {} managed instances",
        managed_instances.managed_instances.len()
    );
    for mi in &managed_instances.managed_instances {
        if let Some(instance_url) = &mi.instance {
            let instance_name = instance_url.split('/').last().unwrap_or("unknown");
            println!(
                "    - Instance: {}, Status: {:?}",
                instance_name, mi.instance_status
            );
        }
    }
    println!("✅ Managed instances listed");

    // =========================================================================
    // Step 4.1: Get serial port output from the managed instance
    // =========================================================================
    println!("\n📦 Step 4.1: Reading serial port output from first instance");

    let stable_instance_url = stable_managed_instance
        .instance
        .as_ref()
        .expect("Stable managed instance should have an instance URL");
    let stable_instance_name = stable_instance_url
        .split('/')
        .last()
        .unwrap_or("unknown")
        .to_string();

    // Retry because the instance may not be ready immediately after creation or replacement.
    let mut serial_output = None;
    for attempt in 1..=12 {
        match ctx
            .client
            .get_serial_port_output(ctx.zone.clone(), stable_instance_name.clone())
            .await
        {
            Ok(output) => {
                serial_output = Some(output);
                break;
            }
            Err(e) => {
                let msg = format!("{:?}", e);
                if msg.contains("resourceNotReady") || msg.contains("not ready") {
                    println!(
                        "  Instance not ready for serial port (attempt {}/12), waiting 10s...",
                        attempt
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                } else {
                    panic!("Failed to get serial port output: {:?}", e);
                }
            }
        }
    }

    let serial_output =
        serial_output.expect("Instance never became ready for serial port output after 120s");
    println!(
        "  Serial port output length: {} bytes",
        serial_output.contents.as_deref().unwrap_or("").len()
    );
    assert!(
        serial_output.contents.is_some(),
        "Serial port output should have a contents field"
    );
    println!(
        "✅ Serial port output retrieved successfully from {}",
        stable_instance_name
    );

    // =========================================================================
    // Step 4.5: Attach and detach a persistent disk to a managed instance
    // =========================================================================
    println!("\n📦 Step 4.5: Attaching and detaching a disk to a managed instance");

    let instance_name = stable_instance_name.clone();

    let disk_name = ctx.generate_unique_name("attach-disk");
    let device_name = format!("dev-{}", &disk_name[..8.min(disk_name.len())]);
    let disk = Disk::builder()
        .name(disk_name.clone())
        .description("Alien test attached disk".to_string())
        .size_gb("10".to_string())
        .r#type(format!(
            "projects/{}/zones/{}/diskTypes/pd-standard",
            ctx.project_id, ctx.zone
        ))
        .build();

    let create_disk_op = ctx
        .client
        .insert_disk(ctx.zone.clone(), disk)
        .await
        .expect("Failed to create disk for attachment");
    ctx.track_disk(&ctx.zone, &disk_name);

    ctx.wait_for_zone_operation(&ctx.zone, create_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk creation timed out");

    let attached_disk = AttachedDisk::builder()
        .r#type(AttachedDiskType::Persistent)
        .mode(DiskMode::ReadWrite)
        .source(format!(
            "projects/{}/zones/{}/disks/{}",
            ctx.project_id, ctx.zone, disk_name
        ))
        .device_name(device_name.clone())
        .auto_delete(false)
        .build();

    let attach_op = ctx
        .client
        .attach_disk(ctx.zone.clone(), instance_name.clone(), attached_disk)
        .await
        .expect("Failed to attach disk");

    ctx.wait_for_zone_operation(&ctx.zone, attach_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Disk attach operation failed or timed out");
    println!("✅ Disk attached to instance {}", instance_name);

    let detach_op = ctx
        .client
        .detach_disk(ctx.zone.clone(), instance_name.clone(), device_name.clone())
        .await
        .expect("Failed to detach disk");

    ctx.wait_for_zone_operation(&ctx.zone, detach_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Disk detach operation failed or timed out");
    println!("✅ Disk detached from instance {}", instance_name);

    let delete_disk_op = ctx
        .client
        .delete_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to delete attached disk");
    ctx.wait_for_zone_operation(&ctx.zone, delete_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk deletion failed or timed out");
    ctx.untrack_disk(&ctx.zone, &disk_name);
    println!("✅ Attached disk deleted");

    // =========================================================================
    // Step 5: Resize back to 0 before cleanup
    // =========================================================================
    println!("\n📦 Step 5: Resizing instance group manager back to 0");

    let resize_down_op = ctx
        .client
        .resize_instance_group_manager(ctx.zone.clone(), igm_name.clone(), 0)
        .await
        .expect("Failed to resize IGM down");

    ctx.wait_for_zone_operation(&ctx.zone, resize_down_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Resize down operation timed out");
    println!("✅ Instance group manager resized to 0");

    // Wait for instances to be deleted
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // =========================================================================
    // Step 6: Cleanup
    // =========================================================================
    println!("\n🧹 Step 6: Cleaning up instance management resources");

    // Delete IGM
    let delete_igm_op = ctx
        .client
        .delete_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to delete IGM");

    ctx.wait_for_zone_operation(&ctx.zone, delete_igm_op.name.as_ref().unwrap(), 300)
        .await
        .expect("IGM deletion timed out");
    ctx.untrack_instance_group_manager(&ctx.zone, &igm_name);
    println!("  ✅ Instance group manager deleted");

    // Delete instance template
    let delete_template_op = ctx
        .client
        .delete_instance_template(template_name.clone())
        .await
        .expect("Failed to delete instance template");

    ctx.wait_for_global_operation(delete_template_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Instance template deletion timed out");
    ctx.untrack_instance_template(&template_name);
    println!("  ✅ Instance template deleted");

    println!("\n🎉 Comprehensive Instance Management lifecycle test completed successfully!");
    println!("   - Instance template created and deleted: ✅");
    println!("   - Instance group manager created, resized, and deleted: ✅");
    println!("   - Managed instances listed: ✅");
}

// =============================================================================================
// Error Handling Tests for New APIs
// =============================================================================================

// -------------------------------------------------------------------------
// HTTPS Load Balancing Tests (SSL Certificates + Target HTTPS Proxies)
// -------------------------------------------------------------------------

/// This test covers the full lifecycle of SSL certificates and HTTPS proxies:
/// 1. Create an SSL certificate (self-managed)
/// 2. Verify the certificate was created
/// 3. Create a Target HTTPS proxy referencing the certificate
/// 4. Verify the HTTPS proxy was created
/// 5. Delete the HTTPS proxy
/// 6. Delete the SSL certificate
#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_ssl_https_proxy_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive SSL certificate and HTTPS proxy lifecycle test");

    // Generate unique names
    let ssl_cert_name = ctx.generate_unique_name("ssl-cert");
    let https_proxy_name = ctx.generate_unique_name("https-proxy");
    let url_map_name = ctx.generate_unique_name("urlmap-for-https");
    let backend_service_name = ctx.generate_unique_name("bs-for-https");
    let health_check_name = ctx.generate_unique_name("hc-for-https");

    // =========================================================================
    // Step 1: Create prerequisites (health check, backend service, URL map)
    // =========================================================================
    println!("\n📦 Step 1: Creating prerequisites for HTTPS proxy");

    // Create health check
    let health_check = HealthCheck::builder()
        .name(health_check_name.clone())
        .r#type(HealthCheckType::Http)
        .check_interval_sec(10)
        .timeout_sec(5)
        .http_health_check(HttpHealthCheck::builder().port(80).build())
        .build();

    let create_hc_op = ctx
        .client
        .insert_health_check(health_check)
        .await
        .expect("Failed to create health check");
    ctx.track_health_check(&health_check_name);
    ctx.wait_for_global_operation(create_hc_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Health check creation timed out");

    let health_check_url = format!(
        "projects/{}/global/healthChecks/{}",
        ctx.project_id, health_check_name
    );

    // Create backend service
    let backend_service = BackendService::builder()
        .name(backend_service_name.clone())
        .protocol(BackendServiceProtocol::Http)
        .health_checks(vec![health_check_url])
        .build();

    let create_bs_op = ctx
        .client
        .insert_backend_service(backend_service)
        .await
        .expect("Failed to create backend service");
    ctx.track_backend_service(&backend_service_name);
    ctx.wait_for_global_operation(create_bs_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Backend service creation timed out");

    let backend_service_url = format!(
        "projects/{}/global/backendServices/{}",
        ctx.project_id, backend_service_name
    );

    // Create URL map
    let url_map = UrlMap::builder()
        .name(url_map_name.clone())
        .default_service(backend_service_url)
        .build();

    let create_urlmap_op = ctx
        .client
        .insert_url_map(url_map)
        .await
        .expect("Failed to create URL map");
    ctx.track_url_map(&url_map_name);
    ctx.wait_for_global_operation(create_urlmap_op.name.as_ref().unwrap(), 120)
        .await
        .expect("URL map creation timed out");

    let url_map_url = format!(
        "projects/{}/global/urlMaps/{}",
        ctx.project_id, url_map_name
    );

    println!("✅ Prerequisites created");

    // =========================================================================
    // Step 2: Create SSL Certificate
    // =========================================================================
    println!("\n📦 Step 2: Creating SSL certificate: {}", ssl_cert_name);

    // Generate a valid self-signed certificate with CN and SAN using rcgen
    use rcgen::{CertificateParams, DistinguishedName, DnType};

    let mut params = CertificateParams::new(vec!["example.com".to_string()])
        .expect("Failed to create certificate params");

    // Set distinguished name with Common Name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "example.com");
    dn.push(DnType::OrganizationName, "Alien Test");
    dn.push(DnType::CountryName, "US");
    params.distinguished_name = dn;

    // Add Subject Alternative Names (required by GCP)
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName(rcgen::Ia5String::try_from("example.com").unwrap()),
        rcgen::SanType::DnsName(rcgen::Ia5String::try_from("*.example.com").unwrap()),
    ];

    let key_pair = rcgen::KeyPair::generate().expect("Failed to generate key pair");
    let cert = params
        .self_signed(&key_pair)
        .expect("Failed to generate certificate");

    let certificate_pem = cert.pem();
    let private_key_pem = key_pair.serialize_pem();

    let ssl_certificate = SslCertificate::builder()
        .name(ssl_cert_name.clone())
        .description("Alien test SSL certificate".to_string())
        .r#type("SELF_MANAGED".to_string())
        .self_managed(
            SslCertificateSelfManaged::builder()
                .certificate(certificate_pem.to_string())
                .private_key(private_key_pem.to_string())
                .build(),
        )
        .build();

    let create_ssl_op = ctx
        .client
        .insert_ssl_certificate(ssl_certificate)
        .await
        .expect("Failed to create SSL certificate");

    // Track for cleanup (we'll add tracking helper)
    ctx.wait_for_global_operation(create_ssl_op.name.as_ref().unwrap(), 120)
        .await
        .expect("SSL certificate creation timed out");

    println!("✅ SSL certificate created");

    // Verify certificate was created
    let fetched_cert = ctx
        .client
        .get_ssl_certificate(ssl_cert_name.clone())
        .await
        .expect("Failed to get SSL certificate");

    assert_eq!(fetched_cert.name.as_ref().unwrap(), &ssl_cert_name);
    assert!(fetched_cert.id.is_some(), "Certificate should have an ID");
    println!("✅ Verified SSL certificate: {}", ssl_cert_name);

    // =========================================================================
    // Step 3: Create Target HTTPS Proxy
    // =========================================================================
    println!(
        "\n📦 Step 3: Creating Target HTTPS proxy: {}",
        https_proxy_name
    );

    let ssl_cert_url = format!(
        "projects/{}/global/sslCertificates/{}",
        ctx.project_id, ssl_cert_name
    );

    let https_proxy = TargetHttpsProxy::builder()
        .name(https_proxy_name.clone())
        .description("Alien test HTTPS proxy".to_string())
        .url_map(url_map_url)
        .ssl_certificates(vec![ssl_cert_url])
        .build();

    let create_proxy_op = ctx
        .client
        .insert_target_https_proxy(https_proxy)
        .await
        .expect("Failed to create Target HTTPS proxy");

    ctx.wait_for_global_operation(create_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTPS proxy creation timed out");

    println!("✅ Target HTTPS proxy created");

    // Verify HTTPS proxy was created
    let fetched_proxy = ctx
        .client
        .get_target_https_proxy(https_proxy_name.clone())
        .await
        .expect("Failed to get Target HTTPS proxy");

    assert_eq!(fetched_proxy.name.as_ref().unwrap(), &https_proxy_name);
    assert!(fetched_proxy.id.is_some(), "Proxy should have an ID");
    assert!(
        fetched_proxy.ssl_certificates.is_some(),
        "Proxy should have SSL certificates"
    );
    println!("✅ Verified Target HTTPS proxy: {}", https_proxy_name);

    // =========================================================================
    // Step 4: Delete Target HTTPS Proxy
    // =========================================================================
    println!("\n🗑️  Step 4: Deleting Target HTTPS proxy");

    let delete_proxy_op = ctx
        .client
        .delete_target_https_proxy(https_proxy_name.clone())
        .await
        .expect("Failed to delete Target HTTPS proxy");

    ctx.wait_for_global_operation(delete_proxy_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Target HTTPS proxy deletion timed out");

    // Verify deletion
    let get_deleted_result = ctx
        .client
        .get_target_https_proxy(https_proxy_name.clone())
        .await;
    assert!(
        get_deleted_result.is_err(),
        "Target HTTPS proxy should be deleted"
    );
    println!("✅ Target HTTPS proxy deleted");

    // =========================================================================
    // Step 5: Delete SSL Certificate
    // =========================================================================
    println!("\n🗑️  Step 5: Deleting SSL certificate");

    let delete_ssl_op = ctx
        .client
        .delete_ssl_certificate(ssl_cert_name.clone())
        .await
        .expect("Failed to delete SSL certificate");

    ctx.wait_for_global_operation(delete_ssl_op.name.as_ref().unwrap(), 120)
        .await
        .expect("SSL certificate deletion timed out");

    // Verify deletion
    let get_deleted_cert_result = ctx.client.get_ssl_certificate(ssl_cert_name.clone()).await;
    assert!(
        get_deleted_cert_result.is_err(),
        "SSL certificate should be deleted"
    );
    println!("✅ SSL certificate deleted");

    // Clean up prerequisites
    ctx.cleanup_url_map(&url_map_name).await;
    ctx.untrack_url_map(&url_map_name);
    ctx.cleanup_backend_service(&backend_service_name).await;
    ctx.untrack_backend_service(&backend_service_name);
    ctx.cleanup_health_check(&health_check_name).await;
    ctx.untrack_health_check(&health_check_name);

    println!("\n🎉 SSL certificate and HTTPS proxy lifecycle test completed successfully!");
}

// -------------------------------------------------------------------------
// Not Found Error Tests
// -------------------------------------------------------------------------

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_health_check_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-hc-does-not-exist-12345";

    let result = ctx.client.get_health_check(non_existent.to_string()).await;
    assert!(
        result.is_err(),
        "Expected error for non-existent health check"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for health check");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_backend_service_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-bs-does-not-exist-12345";

    let result = ctx
        .client
        .get_backend_service(non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent backend service"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for backend service");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_disk_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-disk-does-not-exist-12345";

    let result = ctx
        .client
        .get_disk(ctx.zone.clone(), non_existent.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent disk");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for disk");
}

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_error_instance_template_not_found(ctx: &mut ComputeTestContext) {
    let non_existent = "alien-test-template-does-not-exist-12345";

    let result = ctx
        .client
        .get_instance_template(non_existent.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error for non-existent instance template"
    );

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND for instance template");
}
