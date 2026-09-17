//! GCP Compute Engine client for VPC, networking, load balancing, instances, and disk operations.
//!
//! This module provides APIs for managing:
//! - VPC networks, subnetworks, routers, and firewalls
//! - Load balancing: health checks, backend services, URL maps, proxies, forwarding rules, NEGs
//! - Instance management: instance templates, instance group managers, instances
//! - Persistent disks
//!
//! See:
//! - Networks: https://cloud.google.com/compute/docs/reference/rest/v1/networks
//! - Subnetworks: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks
//! - Routers: https://cloud.google.com/compute/docs/reference/rest/v1/routers
//! - Firewalls: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls
//! - Health Checks: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks
//! - Backend Services: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices
//! - URL Maps: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps
//! - Target HTTP Proxies: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies
//! - Global Addresses: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses
//! - Global Forwarding Rules: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules
//! - Network Endpoint Groups: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
//! - Instance Templates: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates
//! - Instance Group Managers: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers
//! - Instances: https://cloud.google.com/compute/docs/reference/rest/v1/instances
//! - Disks: https://cloud.google.com/compute/docs/reference/rest/v1/disks

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
pub use alien_gcp_compute_types::*;
use reqwest::{Client, Method};
use serde::Serialize;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

// =============================================================================================
// Service Configuration
// =============================================================================================

/// Compute Engine service configuration
#[derive(Debug)]
pub struct ComputeServiceConfig;

#[derive(Debug, Serialize, Clone)]
struct ResourceGroupReference {
    group: String,
}

impl GcpServiceConfig for ComputeServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://compute.googleapis.com/compute/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://compute.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Compute Engine"
    }

    fn service_key(&self) -> &'static str {
        "compute"
    }
}

// =============================================================================================
// API Trait
// =============================================================================================

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ComputeApi: Send + Sync + Debug {
    // --- Zone Operations ---

    /// Lists zones in the project.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/zones/list
    async fn list_zones(&self, filter: Option<String>) -> Result<ZoneList>;

    // --- Network Operations ---

    /// Gets a VPC network.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networks/get
    async fn get_network(&self, network_name: String) -> Result<Network>;

    /// Creates a VPC network.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networks/insert
    async fn insert_network(&self, network: Network) -> Result<Operation>;

    /// Deletes a VPC network.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networks/delete
    async fn delete_network(&self, network_name: String) -> Result<Operation>;

    // --- Subnetwork Operations ---

    /// Gets a subnetwork.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks/get
    async fn get_subnetwork(&self, region: String, subnetwork_name: String) -> Result<Subnetwork>;

    /// Creates a subnetwork.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks/insert
    async fn insert_subnetwork(&self, region: String, subnetwork: Subnetwork) -> Result<Operation>;

    /// Deletes a subnetwork.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks/delete
    async fn delete_subnetwork(&self, region: String, subnetwork_name: String)
        -> Result<Operation>;

    // --- Router Operations ---

    /// Lists routers in a region.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers/list
    async fn list_routers(&self, region: String) -> Result<RouterList>;

    /// Gets a router.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers/get
    async fn get_router(&self, region: String, router_name: String) -> Result<Router>;

    /// Creates a router.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers/insert
    async fn insert_router(&self, region: String, router: Router) -> Result<Operation>;

    /// Updates a router (PATCH).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers/patch
    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> Result<Operation>;

    /// Deletes a router.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers/delete
    async fn delete_router(&self, region: String, router_name: String) -> Result<Operation>;

    // --- Firewall Operations ---

    /// Lists firewall rules.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls/list
    async fn list_firewalls(&self) -> Result<FirewallList>;

    /// Gets a firewall rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls/get
    async fn get_firewall(&self, firewall_name: String) -> Result<Firewall>;

    /// Creates a firewall rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls/insert
    async fn insert_firewall(&self, firewall: Firewall) -> Result<Operation>;

    /// Deletes a firewall rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls/delete
    async fn delete_firewall(&self, firewall_name: String) -> Result<Operation>;

    // --- Operation Operations ---

    /// Gets the status of an operation (global).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalOperations/get
    async fn get_global_operation(&self, operation_name: String) -> Result<Operation>;

    /// Gets the status of a regional operation.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/regionOperations/get
    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> Result<Operation>;

    /// Waits for a global operation to complete.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalOperations/wait
    async fn wait_global_operation(&self, operation_name: String) -> Result<Operation>;

    /// Waits for a regional operation to complete.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/regionOperations/wait
    async fn wait_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> Result<Operation>;

    /// Gets the status of a zonal operation.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/zoneOperations/get
    async fn get_zone_operation(&self, zone: String, operation_name: String) -> Result<Operation>;

    /// Waits for a zonal operation to complete.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/zoneOperations/wait
    async fn wait_zone_operation(&self, zone: String, operation_name: String) -> Result<Operation>;

    // --- Health Check Operations ---

    /// Gets a health check.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks/get
    async fn get_health_check(&self, health_check_name: String) -> Result<HealthCheck>;

    /// Creates a health check.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks/insert
    async fn insert_health_check(&self, health_check: HealthCheck) -> Result<Operation>;

    /// Deletes a health check.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks/delete
    async fn delete_health_check(&self, health_check_name: String) -> Result<Operation>;

    /// Patches a health check (PATCH update).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks/patch
    async fn patch_health_check(
        &self,
        health_check_name: String,
        health_check: HealthCheck,
    ) -> Result<Operation>;

    // --- Backend Service Operations ---

    /// Gets a backend service.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices/get
    async fn get_backend_service(&self, backend_service_name: String) -> Result<BackendService>;

    /// Gets the health of a backend group attached to a backend service.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices/getHealth
    async fn get_backend_service_health(
        &self,
        backend_service_name: String,
        group: String,
    ) -> Result<BackendServiceGroupHealth>;

    /// Creates a backend service.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices/insert
    async fn insert_backend_service(&self, backend_service: BackendService) -> Result<Operation>;

    /// Deletes a backend service.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices/delete
    async fn delete_backend_service(&self, backend_service_name: String) -> Result<Operation>;

    /// Updates a backend service (PATCH).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices/patch
    async fn patch_backend_service(
        &self,
        backend_service_name: String,
        backend_service: BackendService,
    ) -> Result<Operation>;

    // --- URL Map Operations ---

    /// Gets a URL map.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps/get
    async fn get_url_map(&self, url_map_name: String) -> Result<UrlMap>;

    /// Creates a URL map.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps/insert
    async fn insert_url_map(&self, url_map: UrlMap) -> Result<Operation>;

    /// Deletes a URL map.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps/delete
    async fn delete_url_map(&self, url_map_name: String) -> Result<Operation>;

    // --- Target HTTP Proxy Operations ---

    /// Gets a target HTTP proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies/get
    async fn get_target_http_proxy(
        &self,
        target_http_proxy_name: String,
    ) -> Result<TargetHttpProxy>;

    /// Creates a target HTTP proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies/insert
    async fn insert_target_http_proxy(
        &self,
        target_http_proxy: TargetHttpProxy,
    ) -> Result<Operation>;

    /// Deletes a target HTTP proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies/delete
    async fn delete_target_http_proxy(&self, target_http_proxy_name: String) -> Result<Operation>;

    // --- Target HTTPS Proxy Operations ---

    /// Gets a target HTTPS proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies/get
    async fn get_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> Result<TargetHttpsProxy>;

    /// Creates a target HTTPS proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies/insert
    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> Result<Operation>;

    /// Replaces the SSL certificates associated with a target HTTPS proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies/setSslCertificates
    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> Result<Operation>;

    /// Deletes a target HTTPS proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies/delete
    async fn delete_target_https_proxy(&self, target_https_proxy_name: String)
        -> Result<Operation>;

    // --- Target TCP Proxy Operations ---

    async fn insert_target_tcp_proxy(&self, target_tcp_proxy: TargetTcpProxy) -> Result<Operation>;
    async fn delete_target_tcp_proxy(&self, target_tcp_proxy_name: String) -> Result<Operation>;

    // --- SSL Certificate Operations ---

    /// Gets an SSL certificate.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates/get
    async fn get_ssl_certificate(&self, ssl_certificate_name: String) -> Result<SslCertificate>;

    /// Creates an SSL certificate.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates/insert
    async fn insert_ssl_certificate(&self, ssl_certificate: SslCertificate) -> Result<Operation>;

    /// Deletes an SSL certificate.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates/delete
    async fn delete_ssl_certificate(&self, ssl_certificate_name: String) -> Result<Operation>;

    // --- Global Address Operations ---

    /// Gets a global address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses/get
    async fn get_global_address(&self, address_name: String) -> Result<Address>;

    /// Creates a global address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses/insert
    async fn insert_global_address(&self, address: Address) -> Result<Operation>;

    /// Deletes a global address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses/delete
    async fn delete_global_address(&self, address_name: String) -> Result<Operation>;

    // --- Global Forwarding Rule Operations ---

    /// Gets a global forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules/get
    async fn get_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule>;

    /// Creates a global forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules/insert
    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation>;

    /// Deletes a global forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules/delete
    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<Operation>;

    // --- Regional Address Operations ---
    // Private Service Connect consumer endpoints are regional; the global address
    // methods above don't cover the regional internal address they need.

    /// Gets a regional address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/addresses/get
    async fn get_address(&self, region: String, address_name: String) -> Result<Address>;

    /// Creates a regional address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/addresses/insert
    async fn insert_address(&self, region: String, address: Address) -> Result<Operation>;

    /// Deletes a regional address.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/addresses/delete
    async fn delete_address(&self, region: String, address_name: String) -> Result<Operation>;

    // --- Regional Forwarding Rule Operations ---
    // Private Service Connect consumer endpoints are regional; the global
    // forwarding-rule methods above don't cover them.

    /// Gets a regional forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/forwardingRules/get
    async fn get_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule>;

    /// Creates a regional forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/forwardingRules/insert
    async fn insert_forwarding_rule(
        &self,
        region: String,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation>;

    /// Deletes a regional forwarding rule.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/forwardingRules/delete
    async fn delete_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<Operation>;

    // --- Network Endpoint Group (NEG) Operations ---

    /// Gets a network endpoint group.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups/get
    async fn get_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup>;

    /// Creates a network endpoint group.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups/insert
    async fn insert_network_endpoint_group(
        &self,
        zone: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation>;

    /// Deletes a network endpoint group.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups/delete
    async fn delete_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<Operation>;

    /// Attaches network endpoints to a NEG.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups/attachNetworkEndpoints
    async fn attach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsAttachEndpointsRequest,
    ) -> Result<Operation>;

    /// Detaches network endpoints from a NEG.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups/detachNetworkEndpoints
    async fn detach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsDetachEndpointsRequest,
    ) -> Result<Operation>;

    // --- Regional Network Endpoint Group (NEG) Operations ---

    /// Gets a regional network endpoint group (for serverless workloads).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/regionNetworkEndpointGroups/get
    async fn get_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup>;

    /// Creates a regional network endpoint group (for serverless workloads).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/regionNetworkEndpointGroups/insert
    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation>;

    /// Deletes a regional network endpoint group.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/regionNetworkEndpointGroups/delete
    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<Operation>;

    // --- Instance Template Operations ---

    /// Gets an instance template.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates/get
    async fn get_instance_template(
        &self,
        instance_template_name: String,
    ) -> Result<InstanceTemplate>;

    /// Creates an instance template.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates/insert
    async fn insert_instance_template(
        &self,
        instance_template: InstanceTemplate,
    ) -> Result<Operation>;

    /// Deletes an instance template.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates/delete
    async fn delete_instance_template(&self, instance_template_name: String) -> Result<Operation>;

    // --- Instance Group Manager Operations ---

    /// Gets an instance group manager.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/get
    async fn get_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManager>;

    /// Creates an instance group manager.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/insert
    async fn insert_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager: InstanceGroupManager,
    ) -> Result<Operation>;

    /// Deletes an instance group manager.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/delete
    async fn delete_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<Operation>;

    /// Resizes an instance group manager.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/resize
    async fn resize_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        size: i32,
    ) -> Result<Operation>;

    /// Deletes selected managed instances and reduces the instance group manager target size.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/deleteInstances
    async fn delete_instance_group_manager_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
        request: InstanceGroupManagersDeleteInstancesRequest,
    ) -> Result<Operation>;

    /// Lists managed instances in an instance group manager.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/listManagedInstances
    async fn list_managed_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManagersListManagedInstancesResponse>;

    /// Patches an instance group manager using merge-patch semantics.
    /// Used for rolling updates: set instanceTemplate + updatePolicy to trigger PROACTIVE replacement.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers/patch
    async fn patch_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        patch: InstanceGroupManager,
    ) -> Result<Operation>;

    // --- Instance Operations ---

    /// Gets an instance.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/get
    async fn get_instance(&self, zone: String, instance_name: String) -> Result<Instance>;

    /// Deletes an instance.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/delete
    async fn delete_instance(&self, zone: String, instance_name: String) -> Result<Operation>;

    /// Attaches a disk to an instance.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/attachDisk
    async fn attach_disk(
        &self,
        zone: String,
        instance_name: String,
        attached_disk: AttachedDisk,
    ) -> Result<Operation>;

    /// Detaches a disk from an instance.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/detachDisk
    async fn detach_disk(
        &self,
        zone: String,
        instance_name: String,
        device_name: String,
    ) -> Result<Operation>;

    // --- Disk Operations ---

    /// Gets a disk.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/disks/get
    async fn get_disk(&self, zone: String, disk_name: String) -> Result<Disk>;

    /// Creates a disk.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/disks/insert
    async fn insert_disk(&self, zone: String, disk: Disk) -> Result<Operation>;

    /// Deletes a disk.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/disks/delete
    async fn delete_disk(&self, zone: String, disk_name: String) -> Result<Operation>;

    // --- Serial Port Operations ---

    /// Gets the serial port output of an instance (port 1 = main console).
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/getSerialPortOutput
    async fn get_serial_port_output(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<SerialPortOutput>;
}

// =============================================================================================
// Client Implementation
// =============================================================================================

/// Compute Engine client for managing VPC networks and related resources
#[derive(Debug)]
pub struct ComputeClient {
    base: GcpClientBase,
    project_id: String,
}

impl ComputeClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(ComputeServiceConfig)),
            project_id,
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ComputeApi for ComputeClient {
    // --- Zone Operations ---

    async fn list_zones(&self, filter: Option<String>) -> Result<ZoneList> {
        let path = format!("projects/{}/zones", self.project_id);
        let query_params = filter.map(|filter| vec![("filter", filter)]);
        self.base
            .execute_request(
                Method::GET,
                &path,
                query_params,
                Option::<()>::None,
                "zones",
            )
            .await
    }

    // --- Network Operations ---

    async fn get_network(&self, network_name: String) -> Result<Network> {
        let path = format!(
            "projects/{}/global/networks/{}",
            self.project_id, network_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &network_name)
            .await
    }

    async fn insert_network(&self, network: Network) -> Result<Operation> {
        let path = format!("projects/{}/global/networks", self.project_id);
        let resource_name = network.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(network), &resource_name)
            .await
    }

    async fn delete_network(&self, network_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/networks/{}",
            self.project_id, network_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &network_name,
            )
            .await
    }

    // --- Subnetwork Operations ---

    async fn get_subnetwork(&self, region: String, subnetwork_name: String) -> Result<Subnetwork> {
        let path = format!(
            "projects/{}/regions/{}/subnetworks/{}",
            self.project_id, region, subnetwork_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &subnetwork_name,
            )
            .await
    }

    async fn insert_subnetwork(&self, region: String, subnetwork: Subnetwork) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/subnetworks",
            self.project_id, region
        );
        let resource_name = subnetwork.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(subnetwork), &resource_name)
            .await
    }

    async fn delete_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/subnetworks/{}",
            self.project_id, region, subnetwork_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &subnetwork_name,
            )
            .await
    }

    // --- Router Operations ---

    async fn list_routers(&self, region: String) -> Result<RouterList> {
        let path = format!("projects/{}/regions/{}/routers", self.project_id, region);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, "routers")
            .await
    }

    async fn get_router(&self, region: String, router_name: String) -> Result<Router> {
        let path = format!(
            "projects/{}/regions/{}/routers/{}",
            self.project_id, region, router_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &router_name)
            .await
    }

    async fn insert_router(&self, region: String, router: Router) -> Result<Operation> {
        let path = format!("projects/{}/regions/{}/routers", self.project_id, region);
        let resource_name = router.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(router), &resource_name)
            .await
    }

    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/routers/{}",
            self.project_id, region, router_name
        );
        self.base
            .execute_request(Method::PATCH, &path, None, Some(router), &router_name)
            .await
    }

    async fn delete_router(&self, region: String, router_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/routers/{}",
            self.project_id, region, router_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &router_name,
            )
            .await
    }

    // --- Firewall Operations ---

    async fn list_firewalls(&self) -> Result<FirewallList> {
        let path = format!("projects/{}/global/firewalls", self.project_id);
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, "firewalls")
            .await
    }

    async fn get_firewall(&self, firewall_name: String) -> Result<Firewall> {
        let path = format!(
            "projects/{}/global/firewalls/{}",
            self.project_id, firewall_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &firewall_name)
            .await
    }

    async fn insert_firewall(&self, firewall: Firewall) -> Result<Operation> {
        let path = format!("projects/{}/global/firewalls", self.project_id);
        let resource_name = firewall.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(firewall), &resource_name)
            .await
    }

    async fn delete_firewall(&self, firewall_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/firewalls/{}",
            self.project_id, firewall_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &firewall_name,
            )
            .await
    }

    // --- Operation Operations ---

    async fn get_global_operation(&self, operation_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/operations/{}",
            self.project_id, operation_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/operations/{}",
            self.project_id, region, operation_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    async fn wait_global_operation(&self, operation_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/operations/{}/wait",
            self.project_id, operation_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    async fn wait_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/operations/{}/wait",
            self.project_id, region, operation_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    async fn get_zone_operation(&self, zone: String, operation_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/operations/{}",
            self.project_id, zone, operation_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    async fn wait_zone_operation(&self, zone: String, operation_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/operations/{}/wait",
            self.project_id, zone, operation_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }

    // --- Health Check Operations ---

    async fn get_health_check(&self, health_check_name: String) -> Result<HealthCheck> {
        let path = format!(
            "projects/{}/global/healthChecks/{}",
            self.project_id, health_check_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &health_check_name,
            )
            .await
    }

    async fn insert_health_check(&self, health_check: HealthCheck) -> Result<Operation> {
        let path = format!("projects/{}/global/healthChecks", self.project_id);
        let resource_name = health_check.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(health_check),
                &resource_name,
            )
            .await
    }

    async fn delete_health_check(&self, health_check_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/healthChecks/{}",
            self.project_id, health_check_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &health_check_name,
            )
            .await
    }

    async fn patch_health_check(
        &self,
        health_check_name: String,
        health_check: HealthCheck,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/healthChecks/{}",
            self.project_id, health_check_name
        );
        self.base
            .execute_request(
                Method::PATCH,
                &path,
                None,
                Some(health_check),
                &health_check_name,
            )
            .await
    }

    // --- Backend Service Operations ---

    async fn get_backend_service(&self, backend_service_name: String) -> Result<BackendService> {
        let path = format!(
            "projects/{}/global/backendServices/{}",
            self.project_id, backend_service_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &backend_service_name,
            )
            .await
    }

    async fn get_backend_service_health(
        &self,
        backend_service_name: String,
        group: String,
    ) -> Result<BackendServiceGroupHealth> {
        let path = format!(
            "projects/{}/global/backendServices/{}/getHealth",
            self.project_id, backend_service_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(ResourceGroupReference { group }),
                &backend_service_name,
            )
            .await
    }

    async fn insert_backend_service(&self, backend_service: BackendService) -> Result<Operation> {
        let path = format!("projects/{}/global/backendServices", self.project_id);
        let resource_name = backend_service.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(backend_service),
                &resource_name,
            )
            .await
    }

    async fn delete_backend_service(&self, backend_service_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/backendServices/{}",
            self.project_id, backend_service_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &backend_service_name,
            )
            .await
    }

    async fn patch_backend_service(
        &self,
        backend_service_name: String,
        backend_service: BackendService,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/backendServices/{}",
            self.project_id, backend_service_name
        );
        self.base
            .execute_request(
                Method::PATCH,
                &path,
                None,
                Some(backend_service),
                &backend_service_name,
            )
            .await
    }

    // --- URL Map Operations ---

    async fn get_url_map(&self, url_map_name: String) -> Result<UrlMap> {
        let path = format!(
            "projects/{}/global/urlMaps/{}",
            self.project_id, url_map_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &url_map_name)
            .await
    }

    async fn insert_url_map(&self, url_map: UrlMap) -> Result<Operation> {
        let path = format!("projects/{}/global/urlMaps", self.project_id);
        let resource_name = url_map.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(url_map), &resource_name)
            .await
    }

    async fn delete_url_map(&self, url_map_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/urlMaps/{}",
            self.project_id, url_map_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &url_map_name,
            )
            .await
    }

    // --- Target HTTP Proxy Operations ---

    async fn get_target_http_proxy(
        &self,
        target_http_proxy_name: String,
    ) -> Result<TargetHttpProxy> {
        let path = format!(
            "projects/{}/global/targetHttpProxies/{}",
            self.project_id, target_http_proxy_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &target_http_proxy_name,
            )
            .await
    }

    async fn insert_target_http_proxy(
        &self,
        target_http_proxy: TargetHttpProxy,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/targetHttpProxies", self.project_id);
        let resource_name = target_http_proxy.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(target_http_proxy),
                &resource_name,
            )
            .await
    }

    async fn delete_target_http_proxy(&self, target_http_proxy_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/targetHttpProxies/{}",
            self.project_id, target_http_proxy_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &target_http_proxy_name,
            )
            .await
    }

    // --- Target HTTPS Proxy Operations ---

    async fn get_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> Result<TargetHttpsProxy> {
        let path = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            self.project_id, target_https_proxy_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &target_https_proxy_name,
            )
            .await
    }

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/targetHttpsProxies", self.project_id);
        let name = target_https_proxy
            .name
            .clone()
            .unwrap_or_else(|| "targetHttpsProxy".to_string());
        self.base
            .execute_request(Method::POST, &path, None, Some(target_https_proxy), &name)
            .await
    }

    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/targetHttpsProxies/{}/setSslCertificates",
            self.project_id, target_https_proxy_name
        );
        let request = SetSslCertificatesRequest { ssl_certificates };
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request),
                &target_https_proxy_name,
            )
            .await
    }

    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            self.project_id, target_https_proxy_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &target_https_proxy_name,
            )
            .await
    }

    async fn insert_target_tcp_proxy(&self, target_tcp_proxy: TargetTcpProxy) -> Result<Operation> {
        let path = format!("projects/{}/global/targetTcpProxies", self.project_id);
        let name = target_tcp_proxy
            .name
            .clone()
            .unwrap_or_else(|| "targetTcpProxy".to_string());
        self.base
            .execute_request(Method::POST, &path, None, Some(target_tcp_proxy), &name)
            .await
    }

    async fn delete_target_tcp_proxy(&self, target_tcp_proxy_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/targetTcpProxies/{}",
            self.project_id, target_tcp_proxy_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &target_tcp_proxy_name,
            )
            .await
    }

    // --- SSL Certificate Operations ---

    async fn get_ssl_certificate(&self, ssl_certificate_name: String) -> Result<SslCertificate> {
        let path = format!(
            "projects/{}/global/sslCertificates/{}",
            self.project_id, ssl_certificate_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &ssl_certificate_name,
            )
            .await
    }

    async fn insert_ssl_certificate(&self, ssl_certificate: SslCertificate) -> Result<Operation> {
        let path = format!("projects/{}/global/sslCertificates", self.project_id);
        let name = ssl_certificate
            .name
            .clone()
            .unwrap_or_else(|| "sslCertificate".to_string());
        self.base
            .execute_request(Method::POST, &path, None, Some(ssl_certificate), &name)
            .await
    }

    async fn delete_ssl_certificate(&self, ssl_certificate_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/sslCertificates/{}",
            self.project_id, ssl_certificate_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &ssl_certificate_name,
            )
            .await
    }

    // --- Global Address Operations ---

    async fn get_global_address(&self, address_name: String) -> Result<Address> {
        let path = format!(
            "projects/{}/global/addresses/{}",
            self.project_id, address_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &address_name)
            .await
    }

    async fn insert_global_address(&self, address: Address) -> Result<Operation> {
        let path = format!("projects/{}/global/addresses", self.project_id);
        let resource_name = address.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(address), &resource_name)
            .await
    }

    async fn delete_global_address(&self, address_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/addresses/{}",
            self.project_id, address_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &address_name,
            )
            .await
    }

    // --- Global Forwarding Rule Operations ---

    async fn get_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule> {
        let path = format!(
            "projects/{}/global/forwardingRules/{}",
            self.project_id, forwarding_rule_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &forwarding_rule_name,
            )
            .await
    }

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/forwardingRules", self.project_id);
        let resource_name = forwarding_rule.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(forwarding_rule),
                &resource_name,
            )
            .await
    }

    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/forwardingRules/{}",
            self.project_id, forwarding_rule_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &forwarding_rule_name,
            )
            .await
    }

    // --- Regional Address Operations ---

    async fn get_address(&self, region: String, address_name: String) -> Result<Address> {
        let path = format!(
            "projects/{}/regions/{}/addresses/{}",
            self.project_id, region, address_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &address_name)
            .await
    }

    async fn insert_address(&self, region: String, address: Address) -> Result<Operation> {
        let path = format!("projects/{}/regions/{}/addresses", self.project_id, region);
        let resource_name = address.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(address), &resource_name)
            .await
    }

    async fn delete_address(&self, region: String, address_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/addresses/{}",
            self.project_id, region, address_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &address_name,
            )
            .await
    }

    // --- Regional Forwarding Rule Operations ---

    async fn get_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule> {
        let path = format!(
            "projects/{}/regions/{}/forwardingRules/{}",
            self.project_id, region, forwarding_rule_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &forwarding_rule_name,
            )
            .await
    }

    async fn insert_forwarding_rule(
        &self,
        region: String,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/forwardingRules",
            self.project_id, region
        );
        let resource_name = forwarding_rule.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(forwarding_rule),
                &resource_name,
            )
            .await
    }

    async fn delete_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/forwardingRules/{}",
            self.project_id, region, forwarding_rule_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &forwarding_rule_name,
            )
            .await
    }

    // --- Network Endpoint Group (NEG) Operations ---

    async fn get_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup> {
        let path = format!(
            "projects/{}/zones/{}/networkEndpointGroups/{}",
            self.project_id, zone, neg_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &neg_name)
            .await
    }

    async fn insert_network_endpoint_group(
        &self,
        zone: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/networkEndpointGroups",
            self.project_id, zone
        );
        let resource_name = neg.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(neg), &resource_name)
            .await
    }

    async fn delete_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/networkEndpointGroups/{}",
            self.project_id, zone, neg_name
        );
        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &neg_name)
            .await
    }

    async fn attach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsAttachEndpointsRequest,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/networkEndpointGroups/{}/attachNetworkEndpoints",
            self.project_id, zone, neg_name
        );
        self.base
            .execute_request(Method::POST, &path, None, Some(request), &neg_name)
            .await
    }

    async fn detach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsDetachEndpointsRequest,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/networkEndpointGroups/{}/detachNetworkEndpoints",
            self.project_id, zone, neg_name
        );
        self.base
            .execute_request(Method::POST, &path, None, Some(request), &neg_name)
            .await
    }

    // --- Regional Network Endpoint Group (NEG) Operations ---

    async fn get_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup> {
        let path = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            self.project_id, region, neg_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &neg_name)
            .await
    }

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/networkEndpointGroups",
            self.project_id, region
        );
        let resource_name = neg.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(neg), &resource_name)
            .await
    }

    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            self.project_id, region, neg_name
        );
        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &neg_name)
            .await
    }

    // --- Instance Template Operations ---

    async fn get_instance_template(
        &self,
        instance_template_name: String,
    ) -> Result<InstanceTemplate> {
        let path = format!(
            "projects/{}/global/instanceTemplates/{}",
            self.project_id, instance_template_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &instance_template_name,
            )
            .await
    }

    async fn insert_instance_template(
        &self,
        instance_template: InstanceTemplate,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/instanceTemplates", self.project_id);
        let resource_name = instance_template.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(instance_template),
                &resource_name,
            )
            .await
    }

    async fn delete_instance_template(&self, instance_template_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/instanceTemplates/{}",
            self.project_id, instance_template_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_template_name,
            )
            .await
    }

    // --- Instance Group Manager Operations ---

    async fn get_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManager> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    async fn insert_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager: InstanceGroupManager,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers",
            self.project_id, zone
        );
        let resource_name = instance_group_manager.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(instance_group_manager),
                &resource_name,
            )
            .await
    }

    async fn delete_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    async fn resize_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        size: i32,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/resize",
            self.project_id, zone, instance_group_manager_name
        );
        let query_params = vec![("size", size.to_string())];
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    async fn delete_instance_group_manager_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
        request: InstanceGroupManagersDeleteInstancesRequest,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/deleteInstances",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request),
                &instance_group_manager_name,
            )
            .await
    }

    async fn list_managed_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManagersListManagedInstancesResponse> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/listManagedInstances",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    async fn patch_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        patch: InstanceGroupManager,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::PATCH,
                &path,
                None,
                Some(patch),
                &instance_group_manager_name,
            )
            .await
    }

    // --- Instance Operations ---

    async fn get_instance(&self, zone: String, instance_name: String) -> Result<Instance> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &instance_name)
            .await
    }

    async fn delete_instance(&self, zone: String, instance_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_name,
            )
            .await
    }

    async fn attach_disk(
        &self,
        zone: String,
        instance_name: String,
        attached_disk: AttachedDisk,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/attachDisk",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(attached_disk),
                &instance_name,
            )
            .await
    }

    async fn detach_disk(
        &self,
        zone: String,
        instance_name: String,
        device_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/detachDisk",
            self.project_id, zone, instance_name
        );
        let query = vec![("deviceName", device_name)];
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query),
                Option::<()>::None,
                &instance_name,
            )
            .await
    }

    // --- Disk Operations ---

    async fn get_disk(&self, zone: String, disk_name: String) -> Result<Disk> {
        let path = format!(
            "projects/{}/zones/{}/disks/{}",
            self.project_id, zone, disk_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &disk_name)
            .await
    }

    async fn insert_disk(&self, zone: String, disk: Disk) -> Result<Operation> {
        let path = format!("projects/{}/zones/{}/disks", self.project_id, zone);
        let resource_name = disk.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(disk), &resource_name)
            .await
    }

    async fn delete_disk(&self, zone: String, disk_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/disks/{}",
            self.project_id, zone, disk_name
        );
        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &disk_name)
            .await
    }

    async fn get_serial_port_output(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<SerialPortOutput> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/serialPort",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(vec![("port", "1".to_string())]),
                Option::<()>::None,
                &instance_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE_ATTACHMENT: &str =
        "https://www.googleapis.com/compute/v1/projects/p-producer/regions/us-east1/serviceAttachments/sql-sa";
    const STACK_SUBNET: &str =
        "https://www.googleapis.com/compute/v1/projects/p-consumer/regions/us-east1/subnetworks/stack-subnet";
    const STACK_NETWORK: &str =
        "https://www.googleapis.com/compute/v1/projects/p-consumer/global/networks/stack-vpc";

    /// The forwarding rule half of a Private Service Connect consumer endpoint:
    /// it targets the producer's service attachment over an internal IP, in the
    /// consumer's network/subnet, and is *not* a load balancer.
    fn psc_consumer_forwarding_rule() -> ForwardingRule {
        ForwardingRule {
            name: Some("stack-psc-endpoint".into()),
            target: Some(SERVICE_ATTACHMENT.into()),
            ip_address: Some("10.0.0.42".into()),
            network: Some(STACK_NETWORK.into()),
            subnetwork: Some(STACK_SUBNET.into()),
            // PSC consumer endpoints are not load balancers: scheme stays unset.
            load_balancing_scheme: None,
            ..Default::default()
        }
    }

    /// The address half of a PSC consumer endpoint: a regional INTERNAL IP
    /// reserved from the consumer's subnet.
    fn psc_consumer_address() -> Address {
        Address {
            name: Some("stack-psc-ip".into()),
            address_type: Some(AddressType::Internal),
            address: Some("10.0.0.42".into()),
            subnetwork: Some(STACK_SUBNET.into()),
            ..Default::default()
        }
    }

    #[test]
    fn psc_forwarding_rule_serializes_for_consumer_endpoint() {
        let json = serde_json::to_value(psc_consumer_forwarding_rule())
            .expect("forwarding rule should serialize");

        assert_eq!(json["name"], "stack-psc-endpoint");
        // The target is the producer service attachment — this is what makes it PSC.
        assert_eq!(json["target"], SERVICE_ATTACHMENT);
        // Internal reachability: a fixed internal IP in the consumer subnet.
        assert_eq!(json["IPAddress"], "10.0.0.42");
        assert_eq!(json["network"], STACK_NETWORK);
        assert_eq!(json["subnetwork"], STACK_SUBNET);
        // A PSC consumer endpoint must NOT carry a load-balancing scheme.
        assert!(
            json.get("loadBalancingScheme").is_none(),
            "PSC consumer endpoint must not set loadBalancingScheme, got {json:?}"
        );
        // No global-only target-proxy ports leak in.
        assert!(json.get("portRange").is_none());
        // GCP rejects IPProtocol on a service-attachment-target (PSC) rule outright, so it
        // must be omitted entirely.
        assert!(
            json.get("IPProtocol").is_none(),
            "PSC consumer endpoint must not set IPProtocol, got {json:?}"
        );
    }

    #[test]
    fn psc_address_serializes_as_regional_internal() {
        let json = serde_json::to_value(psc_consumer_address()).expect("address should serialize");

        assert_eq!(json["name"], "stack-psc-ip");
        // Must be INTERNAL — an external address can't back a PSC endpoint.
        assert_eq!(json["addressType"], "INTERNAL");
        assert_eq!(json["address"], "10.0.0.42");
        // The internal IP is drawn from the consumer subnet.
        assert_eq!(json["subnetwork"], STACK_SUBNET);
        // No external-only fields should appear.
        assert!(json.get("networkTier").is_none());
    }

    #[test]
    fn forwarding_rule_round_trips_through_get_response() {
        // A GET on the rule returns the same identity fields we sent on insert.
        let rule: ForwardingRule =
            serde_json::from_value(serde_json::to_value(psc_consumer_forwarding_rule()).unwrap())
                .expect("forwarding rule should deserialize");

        assert_eq!(rule.name.as_deref(), Some("stack-psc-endpoint"));
        assert_eq!(rule.target.as_deref(), Some(SERVICE_ATTACHMENT));
        assert_eq!(rule.subnetwork.as_deref(), Some(STACK_SUBNET));
        assert!(rule.load_balancing_scheme.is_none());
    }

    #[tokio::test]
    async fn target_tcp_proxy_insert_and_delete_use_compute_rest_contract() {
        use alien_core::{GcpCredentials, GcpServiceOverrides};
        use std::{
            collections::HashMap,
            io::{Read, Write},
            net::TcpListener,
            sync::{Arc, Mutex},
        };

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured = observed.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                let (header_end, content_length) = loop {
                    let count = stream.read(&mut buffer).expect("read request");
                    assert!(count > 0, "request ended before headers");
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let header_end = end + 4;
                        let headers = String::from_utf8_lossy(&bytes[..header_end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|value| value.parse().ok())
                            })
                            .unwrap_or(0);
                        break (header_end, length);
                    }
                };
                while bytes.len() < header_end + content_length {
                    let count = stream.read(&mut buffer).expect("read body");
                    assert!(count > 0, "request ended before body");
                    bytes.extend_from_slice(&buffer[..count]);
                }
                captured.lock().expect("capture lock").push(
                    String::from_utf8(bytes[..header_end + content_length].to_vec())
                        .expect("request utf8"),
                );
                let body = r#"{"name":"operation-1","status":"DONE"}"#;
                write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body).expect("write response");
            }
        });
        let client = ComputeClient::new(
            Client::new(),
            GcpClientConfig {
                project_id: "example-project".to_string(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::AccessToken {
                    token: "test-token".to_string(),
                },
                service_overrides: Some(GcpServiceOverrides {
                    endpoints: HashMap::from([("compute".to_string(), endpoint)]),
                }),
                project_number: None,
            },
        );
        client
            .insert_target_tcp_proxy(
                TargetTcpProxy::builder()
                    .name("example-proxy".to_string())
                    .description("TCP proxy".to_string())
                    .service(
                        "projects/example-project/global/backendServices/example-backend"
                            .to_string(),
                    )
                    .proxy_header("NONE".to_string())
                    .build(),
            )
            .await
            .expect("insert should succeed");
        client
            .delete_target_tcp_proxy("example-proxy".to_string())
            .await
            .expect("delete should succeed");
        server.join().expect("server should finish");
        let requests = observed.lock().expect("capture lock");
        let (insert_headers, insert_body) =
            requests[0].split_once("\r\n\r\n").expect("insert request");
        assert!(insert_headers
            .starts_with("POST /projects/example-project/global/targetTcpProxies HTTP/1.1"));
        assert!(insert_headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer test-token")));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(insert_body).expect("insert JSON"),
            serde_json::json!({"name":"example-proxy","description":"TCP proxy","service":"projects/example-project/global/backendServices/example-backend","proxyHeader":"NONE"})
        );
        let (delete_headers, delete_body) =
            requests[1].split_once("\r\n\r\n").expect("delete request");
        assert!(delete_headers.starts_with(
            "DELETE /projects/example-project/global/targetTcpProxies/example-proxy HTTP/1.1"
        ));
        assert!(delete_headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer test-token")));
        assert!(delete_body.is_empty());
    }

    #[test]
    fn instance_properties_serialize_nested_virtualization() {
        let properties = InstanceProperties::builder()
            .machine_type("n2-standard-8".to_string())
            .advanced_machine_features(
                AdvancedMachineFeatures::builder()
                    .enable_nested_virtualization(true)
                    .build(),
            )
            .build();

        assert_eq!(
            serde_json::to_value(properties).expect("instance properties should serialize"),
            serde_json::json!({
                "machineType": "n2-standard-8",
                "advancedMachineFeatures": {
                    "enableNestedVirtualization": true
                }
            })
        );
    }
}
