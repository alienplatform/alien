use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::compute::{
    ComputeApi, ComputeClient, ManagedInstance, ManagedInstanceCurrentAction, ManagedInstanceStatus,
};
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::AsyncTestContext;
use tracing::{info, warn};
use uuid::Uuid;

mod lifecycle;

const TEST_REGION: &str = "us-central1";
const TEST_ZONE: &str = "us-central1-a";
pub(crate) const NETWORK_DELETE_TIMEOUT_SECONDS: u64 = 300;
const NETWORK_DELETE_RETRY_INTERVAL_SECONDS: u64 = 10;

pub(crate) struct ComputeTestContext {
    pub(crate) client: ComputeClient,
    pub(crate) project_id: String,
    pub(crate) region: String,
    pub(crate) zone: String,
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

impl ComputeTestContext {
    fn is_resource_not_ready_error(err: &Error) -> bool {
        let msg = format!("{:?}", err).to_lowercase();
        msg.contains("resourcenotready") || msg.contains("not ready")
    }

    pub(crate) async fn delete_network_with_retry(
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

    pub(crate) fn track_network(&self, network_name: &str) {
        let mut networks = self.created_networks.lock().unwrap();
        networks.insert(network_name.to_string());
        info!("📝 Tracking network for cleanup: {}", network_name);
    }

    pub(crate) fn untrack_network(&self, network_name: &str) {
        let mut networks = self.created_networks.lock().unwrap();
        networks.remove(network_name);
        info!("✅ Network {} untracked", network_name);
    }

    pub(crate) fn track_subnetwork(&self, region: &str, subnetwork_name: &str) {
        let mut subnetworks = self.created_subnetworks.lock().unwrap();
        subnetworks.insert((region.to_string(), subnetwork_name.to_string()));
        info!(
            "📝 Tracking subnetwork for cleanup: {}/{}",
            region, subnetwork_name
        );
    }

    pub(crate) fn untrack_subnetwork(&self, region: &str, subnetwork_name: &str) {
        let mut subnetworks = self.created_subnetworks.lock().unwrap();
        subnetworks.remove(&(region.to_string(), subnetwork_name.to_string()));
        info!("✅ Subnetwork {}/{} untracked", region, subnetwork_name);
    }

    pub(crate) fn track_router(&self, region: &str, router_name: &str) {
        let mut routers = self.created_routers.lock().unwrap();
        routers.insert((region.to_string(), router_name.to_string()));
        info!("📝 Tracking router for cleanup: {}/{}", region, router_name);
    }

    pub(crate) fn untrack_router(&self, region: &str, router_name: &str) {
        let mut routers = self.created_routers.lock().unwrap();
        routers.remove(&(region.to_string(), router_name.to_string()));
        info!("✅ Router {}/{} untracked", region, router_name);
    }

    pub(crate) fn track_firewall(&self, firewall_name: &str) {
        let mut firewalls = self.created_firewalls.lock().unwrap();
        firewalls.insert(firewall_name.to_string());
        info!("📝 Tracking firewall for cleanup: {}", firewall_name);
    }

    pub(crate) fn untrack_firewall(&self, firewall_name: &str) {
        let mut firewalls = self.created_firewalls.lock().unwrap();
        firewalls.remove(firewall_name);
        info!("✅ Firewall {} untracked", firewall_name);
    }

    // --- Load Balancing Tracking Methods ---

    pub(crate) fn track_health_check(&self, name: &str) {
        let mut hc = self.created_health_checks.lock().unwrap();
        hc.insert(name.to_string());
        info!("📝 Tracking health check for cleanup: {}", name);
    }

    pub(crate) fn untrack_health_check(&self, name: &str) {
        let mut hc = self.created_health_checks.lock().unwrap();
        hc.remove(name);
        info!("✅ Health check {} untracked", name);
    }

    pub(crate) fn track_backend_service(&self, name: &str) {
        let mut bs = self.created_backend_services.lock().unwrap();
        bs.insert(name.to_string());
        info!("📝 Tracking backend service for cleanup: {}", name);
    }

    pub(crate) fn untrack_backend_service(&self, name: &str) {
        let mut bs = self.created_backend_services.lock().unwrap();
        bs.remove(name);
        info!("✅ Backend service {} untracked", name);
    }

    pub(crate) fn track_url_map(&self, name: &str) {
        let mut um = self.created_url_maps.lock().unwrap();
        um.insert(name.to_string());
        info!("📝 Tracking URL map for cleanup: {}", name);
    }

    pub(crate) fn untrack_url_map(&self, name: &str) {
        let mut um = self.created_url_maps.lock().unwrap();
        um.remove(name);
        info!("✅ URL map {} untracked", name);
    }

    pub(crate) fn track_target_http_proxy(&self, name: &str) {
        let mut proxies = self.created_target_http_proxies.lock().unwrap();
        proxies.insert(name.to_string());
        info!("📝 Tracking target HTTP proxy for cleanup: {}", name);
    }

    pub(crate) fn untrack_target_http_proxy(&self, name: &str) {
        let mut proxies = self.created_target_http_proxies.lock().unwrap();
        proxies.remove(name);
        info!("✅ Target HTTP proxy {} untracked", name);
    }

    pub(crate) fn track_global_address(&self, name: &str) {
        let mut addrs = self.created_global_addresses.lock().unwrap();
        addrs.insert(name.to_string());
        info!("📝 Tracking global address for cleanup: {}", name);
    }

    pub(crate) fn untrack_global_address(&self, name: &str) {
        let mut addrs = self.created_global_addresses.lock().unwrap();
        addrs.remove(name);
        info!("✅ Global address {} untracked", name);
    }

    pub(crate) fn track_global_forwarding_rule(&self, name: &str) {
        let mut fwds = self.created_global_forwarding_rules.lock().unwrap();
        fwds.insert(name.to_string());
        info!("📝 Tracking global forwarding rule for cleanup: {}", name);
    }

    pub(crate) fn untrack_global_forwarding_rule(&self, name: &str) {
        let mut fwds = self.created_global_forwarding_rules.lock().unwrap();
        fwds.remove(name);
        info!("✅ Global forwarding rule {} untracked", name);
    }

    pub(crate) fn track_neg(&self, zone: &str, name: &str) {
        let mut negs = self.created_negs.lock().unwrap();
        negs.insert((zone.to_string(), name.to_string()));
        info!("📝 Tracking NEG for cleanup: {}/{}", zone, name);
    }

    pub(crate) fn untrack_neg(&self, zone: &str, name: &str) {
        let mut negs = self.created_negs.lock().unwrap();
        negs.remove(&(zone.to_string(), name.to_string()));
        info!("✅ NEG {}/{} untracked", zone, name);
    }

    // --- Instance Management Tracking Methods ---

    pub(crate) fn track_instance_template(&self, name: &str) {
        let mut templates = self.created_instance_templates.lock().unwrap();
        templates.insert(name.to_string());
        info!("📝 Tracking instance template for cleanup: {}", name);
    }

    pub(crate) fn untrack_instance_template(&self, name: &str) {
        let mut templates = self.created_instance_templates.lock().unwrap();
        templates.remove(name);
        info!("✅ Instance template {} untracked", name);
    }

    pub(crate) fn track_instance_group_manager(&self, zone: &str, name: &str) {
        let mut igms = self.created_instance_group_managers.lock().unwrap();
        igms.insert((zone.to_string(), name.to_string()));
        info!(
            "📝 Tracking instance group manager for cleanup: {}/{}",
            zone, name
        );
    }

    pub(crate) fn untrack_instance_group_manager(&self, zone: &str, name: &str) {
        let mut igms = self.created_instance_group_managers.lock().unwrap();
        igms.remove(&(zone.to_string(), name.to_string()));
        info!("✅ Instance group manager {}/{} untracked", zone, name);
    }

    // --- Disk Tracking Methods ---

    pub(crate) fn track_disk(&self, zone: &str, name: &str) {
        let mut disks = self.created_disks.lock().unwrap();
        disks.insert((zone.to_string(), name.to_string()));
        info!("📝 Tracking disk for cleanup: {}/{}", zone, name);
    }

    pub(crate) fn untrack_disk(&self, zone: &str, name: &str) {
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

    pub(crate) async fn cleanup_health_check(&self, name: &str) {
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

    pub(crate) async fn cleanup_backend_service(&self, name: &str) {
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

    pub(crate) async fn cleanup_url_map(&self, name: &str) {
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

    pub(crate) fn generate_unique_name(&self, prefix: &str) -> String {
        format!(
            "alien-test-{}-{}",
            prefix,
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..8].to_lowercase()
        )
    }

    pub(crate) fn extract_operation_name(&self, operation_name: &str) -> String {
        operation_name
            .split('/')
            .last()
            .unwrap_or(operation_name)
            .to_string()
    }

    pub(crate) async fn wait_for_global_operation(
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

    pub(crate) async fn wait_for_region_operation(
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

    pub(crate) async fn wait_for_zone_operation(
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

    pub(crate) async fn wait_for_stable_managed_instance(
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

    pub(crate) fn create_invalid_client(&self) -> ComputeClient {
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
