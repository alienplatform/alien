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
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

// =============================================================================================
// Service Configuration
// =============================================================================================

/// Compute Engine service configuration
#[derive(Debug)]
pub struct ComputeServiceConfig;

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

    /// Deletes a target HTTPS proxy.
    /// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies/delete
    async fn delete_target_https_proxy(&self, target_https_proxy_name: String)
        -> Result<Operation>;

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

// =============================================================================================
// Data Structures - Operation
// =============================================================================================

/// Represents a Compute Engine operation.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalOperations
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Type of the operation (e.g., "insert", "delete").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_type: Option<String>,

    /// URL of the resource the operation modifies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_link: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// User who requested the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Status of the operation: PENDING, RUNNING, or DONE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,

    /// Optional progress indicator (0-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,

    /// Time the operation was started (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// Time the operation was completed (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,

    /// Time the operation was requested (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_time: Option<String>,

    /// URL of the zone where the operation resides (for zonal operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// URL of the region where the operation resides (for regional operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Description of the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// HTTP error status code returned if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_status_code: Option<i32>,

    /// HTTP error message returned if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_message: Option<String>,

    /// Error information if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OperationError>,

    /// Type of resource (always "compute#operation").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Operation {
    /// Returns true if the operation has completed (status == DONE).
    pub fn is_done(&self) -> bool {
        matches!(self.status, Some(OperationStatus::Done))
    }

    /// Returns true if the operation completed with an error.
    pub fn has_error(&self) -> bool {
        self.error.is_some() && !self.error.as_ref().unwrap().errors.is_empty()
    }
}

/// Status of an operation.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    /// Operation is pending.
    Pending,
    /// Operation is running.
    Running,
    /// Operation is complete.
    Done,
}

/// Error information for a failed operation.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    /// Array of errors.
    #[builder(default)]
    #[serde(default)]
    pub errors: Vec<OperationErrorItem>,
}

/// Individual error item in an operation error.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationErrorItem {
    /// Error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Location in the request that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Human-readable error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// =============================================================================================
// Data Structures - Network
// =============================================================================================

/// Represents a VPC network resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/networks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource. Must be 1-63 characters, lowercase letters, numbers, or hyphens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// When true, VMs in this network without external IPs can access Google APIs using Private Google Access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_create_subnetworks: Option<bool>,

    /// Server-defined list of subnetwork URLs for this VPC network.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnetworks: Vec<String>,

    /// The network routing mode (REGIONAL or GLOBAL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_config: Option<NetworkRoutingConfig>,

    /// Maximum Transmission Unit in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtu: Option<i32>,

    /// Firewall policy enforced on the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_policy: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Gateway IPv4 address (output only, for legacy networks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_i_pv4: Option<String>,

    /// Internal IPv6 range for this network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_ipv6_range: Option<String>,

    /// Type of resource (always "compute#network").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Network firewall policy enforcement order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_firewall_policy_enforcement_order: Option<NetworkFirewallPolicyEnforcementOrder>,
}

/// Routing configuration for a network.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRoutingConfig {
    /// The network-wide routing mode: REGIONAL or GLOBAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<RoutingMode>,
}

/// Network routing mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoutingMode {
    /// Regional routing: routes are only advertised to routers in the same region.
    Regional,
    /// Global routing: routes are advertised to all routers in the network.
    Global,
}

/// Network firewall policy enforcement order.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkFirewallPolicyEnforcementOrder {
    /// Evaluate firewall policy before VPC firewall rules.
    BeforeClassicFirewall,
    /// Evaluate firewall policy after VPC firewall rules.
    AfterClassicFirewall,
}

// =============================================================================================
// Data Structures - Subnetwork
// =============================================================================================

/// Represents a subnetwork resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Subnetwork {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the network this subnetwork belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// IP CIDR range for this subnetwork (e.g., "10.0.0.0/24").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,

    /// URL of the region this subnetwork belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Gateway address for default routes to IPs within this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_address: Option<String>,

    /// Whether VMs in this subnetwork can access Google services without external IPs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_google_access: Option<bool>,

    /// Purpose of the subnetwork (PRIVATE, INTERNAL_HTTPS_LOAD_BALANCER, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<SubnetworkPurpose>,

    /// Role of the subnetwork (ACTIVE or BACKUP for INTERNAL_HTTPS_LOAD_BALANCER).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<SubnetworkRole>,

    /// Secondary IP ranges for this subnetwork.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_ip_ranges: Vec<SubnetworkSecondaryRange>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether flow logs are enabled for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_flow_logs: Option<bool>,

    /// Log configuration for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<SubnetworkLogConfig>,

    /// Stack type for this subnetwork (IPV4_ONLY or IPV4_IPV6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_type: Option<StackType>,

    /// IPv6 access type for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_access_type: Option<Ipv6AccessType>,

    /// IPv6 CIDR range for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_cidr_range: Option<String>,

    /// External IPv6 prefix for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ipv6_prefix: Option<String>,

    /// Internal IPv6 prefix for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_ipv6_prefix: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#subnetwork").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Private IPv6 Google access type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ipv6_google_access: Option<PrivateIpv6GoogleAccess>,
}

/// Purpose of a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkPurpose {
    /// Regular user-created subnetwork.
    Private,
    /// Reserved for Internal HTTP(S) Load Balancer.
    InternalHttpsLoadBalancer,
    /// Reserved for Regional Internal HTTP(S) Load Balancer.
    RegionalManagedProxy,
    /// Reserved for Global Internal HTTP(S) Load Balancer.
    GlobalManagedProxy,
    /// Reserved for Private Service Connect.
    PrivateServiceConnect,
    /// Reserved for Private NAT.
    PrivateNat,
}

/// Role of a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkRole {
    /// Active role.
    Active,
    /// Backup role.
    Backup,
}

/// Secondary IP range for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworkSecondaryRange {
    /// Name of the secondary range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_name: Option<String>,

    /// IP CIDR range for the secondary range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,
}

/// Log configuration for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworkLogConfig {
    /// Whether to enable flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Aggregation interval for flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregation_interval: Option<AggregationInterval>,

    /// Sampling rate for flow logs (0.0-1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_sampling: Option<f64>,

    /// Metadata to include in flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SubnetworkLogConfigMetadata>,

    /// Custom metadata fields to include.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata_fields: Vec<String>,

    /// Filter expression for flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_expr: Option<String>,
}

/// Aggregation interval for flow logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AggregationInterval {
    /// 5 second interval.
    Interval5Sec,
    /// 30 second interval.
    Interval30Sec,
    /// 1 minute interval.
    Interval1Min,
    /// 5 minute interval.
    Interval5Min,
    /// 10 minute interval.
    Interval10Min,
    /// 15 minute interval.
    Interval15Min,
}

/// Metadata configuration for subnetwork logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkLogConfigMetadata {
    /// Exclude all metadata.
    ExcludeAllMetadata,
    /// Include all metadata.
    IncludeAllMetadata,
    /// Include custom metadata only.
    CustomMetadata,
}

/// Stack type for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StackType {
    /// IPv4 only.
    Ipv4Only,
    /// Dual-stack (IPv4 and IPv6).
    Ipv4Ipv6,
}

/// IPv6 access type for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ipv6AccessType {
    /// External IPv6 access.
    External,
    /// Internal IPv6 access.
    Internal,
}

/// Private IPv6 Google access type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivateIpv6GoogleAccess {
    /// Disable private IPv6 Google access.
    DisableGoogleAccess,
    /// Enable outbound VM access to Google services via IPv6.
    EnableOutboundVmAccessToGoogle,
    /// Enable bidirectional access to Google services via IPv6.
    EnableBidirectionalAccessToGoogle,
}

// =============================================================================================
// Data Structures - Router
// =============================================================================================

/// Represents a Cloud Router resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Router {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the region this router belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// URL of the network this router belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// BGP information for this router.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bgp: Option<RouterBgp>,

    /// BGP peers for this router.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bgp_peers: Vec<RouterBgpPeer>,

    /// NAT configurations for this router.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nats: Vec<RouterNat>,

    /// Router interfaces.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<RouterInterface>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#router").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Encrypted interconnect router flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_interconnect_router: Option<bool>,
}

/// BGP configuration for a router.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterBgp {
    /// Local BGP Autonomous System Number (ASN).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,

    /// Advertise mode for this BGP speaker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertise_mode: Option<AdvertiseMode>,

    /// Groups of prefixes to be advertised.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_groups: Vec<AdvertisedGroup>,

    /// Individual prefixes to be advertised.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_ip_ranges: Vec<RouterAdvertisedIpRange>,

    /// Keepalive interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keepalive_interval: Option<u32>,
}

/// BGP advertise mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvertiseMode {
    /// Advertise default routes.
    Default,
    /// Advertise custom routes.
    Custom,
}

/// Groups of prefixes to advertise.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvertisedGroup {
    /// Advertise all subnets.
    AllSubnets,
}

/// Individual IP range to advertise.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterAdvertisedIpRange {
    /// IP range to advertise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,

    /// Description of this IP range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// BGP peer configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterBgpPeer {
    /// Name of this BGP peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Name of the interface the BGP peer is associated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_name: Option<String>,

    /// IP address of the peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_ip_address: Option<String>,

    /// Peer BGP ASN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_asn: Option<u32>,

    /// Advertise mode for this BGP peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertise_mode: Option<AdvertiseMode>,

    /// Advertised groups for this peer.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_groups: Vec<AdvertisedGroup>,

    /// Advertised IP ranges for this peer.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_ip_ranges: Vec<RouterAdvertisedIpRange>,

    /// BGP peer status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_type: Option<ManagementType>,

    /// Whether this peer is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Advertised route priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertised_route_priority: Option<u32>,
}

/// Management type for a BGP peer.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagementType {
    /// Peer is managed by the user.
    ManagedByUser,
    /// Peer is managed by an attachment.
    ManagedByAttachment,
}

/// Router interface configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterInterface {
    /// Name of this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// IP range for this interface (CIDR format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_range: Option<String>,

    /// URL of the linked VPN tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_vpn_tunnel: Option<String>,

    /// URL of the linked interconnect attachment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_interconnect_attachment: Option<String>,

    /// Management type for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_type: Option<ManagementType>,

    /// Subnetwork this interface is attached to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Private IP address for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_address: Option<String>,

    /// Redundant interface for this router interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redundant_interface: Option<String>,
}

/// Cloud NAT configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNat {
    /// Name of this NAT configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Type of NAT (endpoint-independent or endpoint-dependent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<NatType>,

    /// Source subnetwork IP ranges to NAT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_subnetwork_ip_ranges_to_nat: Option<SourceSubnetworkIpRangesToNat>,

    /// Subnetworks to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnetworks: Vec<RouterNatSubnetworkToNat>,

    /// NAT IP allocation option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_ip_allocate_option: Option<NatIpAllocateOption>,

    /// NAT IPs to use.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nat_ips: Vec<String>,

    /// Drain NAT IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drain_nat_ips: Vec<String>,

    /// Minimum ports per VM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ports_per_vm: Option<i32>,

    /// Maximum ports per VM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ports_per_vm: Option<i32>,

    /// UDP idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udp_idle_timeout_sec: Option<i32>,

    /// ICMP idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icmp_idle_timeout_sec: Option<i32>,

    /// TCP established idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_established_idle_timeout_sec: Option<i32>,

    /// TCP transitory idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_transitory_idle_timeout_sec: Option<i32>,

    /// TCP time wait timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_time_wait_timeout_sec: Option<i32>,

    /// Log configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<RouterNatLogConfig>,

    /// Whether endpoint-independent mapping is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_endpoint_independent_mapping: Option<bool>,

    /// Whether dynamic port allocation is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dynamic_port_allocation: Option<bool>,

    /// NAT rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RouterNatRule>,

    /// Auto network tier for this NAT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_network_tier: Option<NetworkTier>,
}

/// NAT type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatType {
    /// Public NAT.
    Public,
    /// Private NAT.
    Private,
}

/// Source subnetwork IP ranges to NAT.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceSubnetworkIpRangesToNat {
    /// NAT all primary and secondary IP ranges of all subnetworks.
    AllSubnetworksAllIpRanges,
    /// NAT only primary IP ranges of all subnetworks.
    AllSubnetworksAllPrimaryIpRanges,
    /// NAT only specific subnetworks.
    ListOfSubnetworks,
}

/// NAT IP allocation option.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatIpAllocateOption {
    /// Allocate NAT IPs automatically.
    AutoOnly,
    /// Use manually specified NAT IPs.
    ManualOnly,
}

/// Subnetwork to NAT configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatSubnetworkToNat {
    /// Name of the subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Source IP ranges to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ip_ranges_to_nat: Vec<SourceIpRangesToNat>,

    /// Secondary IP range names to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_ip_range_names: Vec<String>,
}

/// Source IP ranges to NAT for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceIpRangesToNat {
    /// NAT all IP ranges.
    AllIpRanges,
    /// NAT primary IP range only.
    PrimaryIpRange,
    /// NAT only specified secondary IP ranges.
    ListOfSecondaryIpRanges,
}

/// NAT log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Log filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<NatLogFilter>,
}

/// NAT log filter.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatLogFilter {
    /// Log all events.
    All,
    /// Log errors only.
    ErrorsOnly,
    /// Log translations only.
    TranslationsOnly,
}

/// NAT rule.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatRule {
    /// Rule number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_number: Option<u32>,

    /// Description of this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Match condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#match: Option<String>,

    /// Action to take when the rule matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RouterNatRuleAction>,
}

/// NAT rule action.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatRuleAction {
    /// Source NAT active IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_active_ips: Vec<String>,

    /// Source NAT drain IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_drain_ips: Vec<String>,

    /// Source NAT active ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_active_ranges: Vec<String>,

    /// Source NAT drain ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_drain_ranges: Vec<String>,
}

/// Network tier.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkTier {
    /// Premium tier.
    Premium,
    /// Standard tier.
    Standard,
}

/// List of routers.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterList {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// List of routers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Router>,

    /// Server-defined URL for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Token for next page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    /// Type of resource (always "compute#routerList").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Firewall
// =============================================================================================

/// Represents a firewall rule resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Firewall {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the network this firewall applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Priority for this rule (0-65535, lower is higher priority).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,

    /// Direction of traffic (INGRESS or EGRESS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<FirewallDirection>,

    /// Action (allow or deny).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<FirewallAllowed>,

    /// Denied traffic specifications.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied: Vec<FirewallDenied>,

    /// Source IP ranges for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ranges: Vec<String>,

    /// Destination IP ranges for EGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_ranges: Vec<String>,

    /// Source tags for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_tags: Vec<String>,

    /// Target tags for this rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_tags: Vec<String>,

    /// Source service accounts for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_service_accounts: Vec<String>,

    /// Target service accounts for this rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_service_accounts: Vec<String>,

    /// Whether the rule is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Whether logging is enabled for this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<FirewallLogConfig>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#firewall").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Direction of a firewall rule.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirewallDirection {
    /// Incoming traffic.
    Ingress,
    /// Outgoing traffic.
    Egress,
}

/// Allowed traffic specification.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAllowed {
    /// IP protocol (tcp, udp, icmp, esp, ah, sctp, ipip, all).
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<String>,

    /// Ports to allow (e.g., "80", "8080-8090").
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

/// Denied traffic specification.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallDenied {
    /// IP protocol (tcp, udp, icmp, esp, ah, sctp, ipip, all).
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<String>,

    /// Ports to deny.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

/// Firewall log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Metadata to include in logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FirewallLogConfigMetadata>,
}

/// Metadata configuration for firewall logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirewallLogConfigMetadata {
    /// Exclude all metadata.
    ExcludeAllMetadata,
    /// Include all metadata.
    IncludeAllMetadata,
}

/// List of firewall rules.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallList {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// List of firewall rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Firewall>,

    /// Server-defined URL for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Token for next page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    /// Type of resource (always "compute#firewallList").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Health Check
// =============================================================================================

/// Represents a health check resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// How often (in seconds) to send a health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_interval_sec: Option<i32>,

    /// How long (in seconds) to wait before claiming failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<i32>,

    /// Number of consecutive failures before marking unhealthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold: Option<i32>,

    /// Number of consecutive successes before marking healthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold: Option<i32>,

    /// Type of health check (TCP, HTTP, HTTPS, HTTP2, GRPC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<HealthCheckType>,

    /// TCP health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_health_check: Option<TcpHealthCheck>,

    /// HTTP health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_health_check: Option<HttpHealthCheck>,

    /// HTTPS health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_health_check: Option<HttpsHealthCheck>,

    /// HTTP2 health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http2_health_check: Option<Http2HealthCheck>,

    /// GRPC health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_health_check: Option<GrpcHealthCheck>,

    /// Log configuration for this health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<HealthCheckLogConfig>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#healthCheck").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Health check type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthCheckType {
    /// TCP health check.
    Tcp,
    /// HTTP health check.
    Http,
    /// HTTPS health check.
    Https,
    /// HTTP/2 health check.
    Http2,
    /// gRPC health check.
    Grpc,
}

/// TCP health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TcpHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Request data to send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTP health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTPS health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpsHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTP/2 health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Http2HealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// gRPC health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GrpcHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// gRPC service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_service_name: Option<String>,
}

/// Port specification type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PortSpecification {
    /// Use a fixed port number.
    UseFixedPort,
    /// Use a named port.
    UseNamedPort,
    /// Use the serving port.
    UseServingPort,
}

/// Proxy header type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProxyHeader {
    /// No proxy header.
    None,
    /// PROXY_V1 header.
    ProxyV1,
}

/// Health check log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,
}

// =============================================================================================
// Data Structures - Backend Service
// =============================================================================================

/// Represents a backend service resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendService {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// List of backends.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<Backend>,

    /// Health check URLs for this backend service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub health_checks: Vec<String>,

    /// Timeout in seconds for backend responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<i32>,

    /// Port number used for communication with backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Protocol used to communicate with backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<BackendServiceProtocol>,

    /// Port name for backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Load balancing scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,

    /// Session affinity configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_affinity: Option<SessionAffinity>,

    /// Affinity cookie TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_cookie_ttl_sec: Option<i32>,

    /// Connection draining configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_draining: Option<ConnectionDraining>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to enable CDN for this backend service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_c_d_n: Option<bool>,

    /// CDN policy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdn_policy: Option<BackendServiceCdnPolicy>,

    /// Log configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<BackendServiceLogConfig>,

    /// Security policy URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_policy: Option<String>,

    /// Locality load balancing policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality_lb_policy: Option<LocalityLbPolicy>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#backendService").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Backend configuration for a backend service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Backend {
    /// URL of the backend group (instance group or NEG).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// Balancing mode (UTILIZATION, RATE, CONNECTION).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balancing_mode: Option<BalancingMode>,

    /// Capacity scaler (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_scaler: Option<f64>,

    /// Maximum connections for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<i32>,

    /// Maximum connections per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections_per_instance: Option<i32>,

    /// Maximum connections per endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections_per_endpoint: Option<i32>,

    /// Maximum rate for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate: Option<i32>,

    /// Maximum rate per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_per_instance: Option<f64>,

    /// Maximum rate per endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_per_endpoint: Option<f64>,

    /// Maximum CPU utilization for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_utilization: Option<f64>,

    /// Description of this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Balancing mode for a backend.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BalancingMode {
    /// Balance by CPU utilization.
    Utilization,
    /// Balance by request rate.
    Rate,
    /// Balance by connection count.
    Connection,
}

/// Backend service protocol.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BackendServiceProtocol {
    /// HTTP protocol.
    Http,
    /// HTTPS protocol.
    Https,
    /// HTTP/2 protocol.
    Http2,
    /// TCP protocol.
    Tcp,
    /// SSL protocol.
    Ssl,
    /// gRPC protocol.
    Grpc,
    /// Unspecified protocol.
    Unspecified,
}

/// Load balancing scheme.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoadBalancingScheme {
    /// External load balancing.
    External,
    /// Internal load balancing.
    Internal,
    /// Internal self-managed load balancing.
    InternalSelfManaged,
    /// Internal managed load balancing.
    InternalManaged,
    /// External managed load balancing.
    ExternalManaged,
}

/// Session affinity type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionAffinity {
    /// No session affinity.
    None,
    /// Client IP affinity.
    ClientIp,
    /// Generated cookie affinity.
    GeneratedCookie,
    /// Client IP with proto affinity.
    ClientIpProto,
    /// Client IP and port affinity.
    ClientIpPortProto,
    /// HTTP cookie affinity.
    HttpCookie,
    /// Header field affinity.
    HeaderField,
}

/// Connection draining configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDraining {
    /// Time in seconds to wait for connections to drain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draining_timeout_sec: Option<i32>,
}

/// Backend service CDN policy.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendServiceCdnPolicy {
    /// Cache mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,

    /// Signed URL cache max age in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_url_cache_max_age_sec: Option<i64>,

    /// Default TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_ttl: Option<i32>,

    /// Maximum TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ttl: Option<i32>,

    /// Client TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ttl: Option<i32>,

    /// Whether to serve stale content while revalidating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_while_stale: Option<i32>,

    /// Negative caching policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_caching: Option<bool>,
}

/// Cache mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CacheMode {
    /// Use origin headers.
    UseOriginHeaders,
    /// Force cache all.
    ForceCacheAll,
    /// Cache all static content.
    CacheAllStatic,
}

/// Backend service log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendServiceLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Sample rate (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
}

/// Locality load balancing policy.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalityLbPolicy {
    /// Round robin.
    RoundRobin,
    /// Least request.
    LeastRequest,
    /// Ring hash.
    RingHash,
    /// Random.
    Random,
    /// Original destination.
    OriginalDestination,
    /// Maglev.
    Maglev,
}

// =============================================================================================
// Data Structures - URL Map
// =============================================================================================

/// Represents a URL map resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UrlMap {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Default backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,

    /// Host rules for this URL map.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_rules: Vec<HostRule>,

    /// Path matchers for this URL map.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_matchers: Vec<PathMatcher>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#urlMap").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Host rule for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HostRule {
    /// Description of this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// List of hosts to match.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,

    /// Name of the path matcher to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_matcher: Option<String>,
}

/// Path matcher for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PathMatcher {
    /// Name of this path matcher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description of this path matcher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,

    /// Path rules for this matcher.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_rules: Vec<PathRule>,
}

/// Path rule for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PathRule {
    /// Paths to match.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,

    /// Backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

// =============================================================================================
// Data Structures - Target HTTP Proxy
// =============================================================================================

/// Represents a target HTTP proxy resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetHttpProxy {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the URL map associated with this proxy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_map: Option<String>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to proxy WebSocket requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_bind: Option<bool>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#targetHttpProxy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Target HTTPS Proxy
// =============================================================================================

/// Represents a target HTTPS proxy resource (with SSL certificate support).
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetHttpsProxy {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the URL map associated with this proxy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_map: Option<String>,

    /// URLs of SSL certificates associated with this proxy.
    /// At least one SSL certificate must be specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_certificates: Option<Vec<String>>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to proxy WebSocket requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_bind: Option<bool>,

    /// Minimum TLS version (e.g., "TLS_1_2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#targetHttpsProxy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// QUIC protocol override (e.g., "NONE", "ENABLE", "DISABLE").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quic_override: Option<String>,
}

// =============================================================================================
// Data Structures - SSL Certificate
// =============================================================================================

/// Self-managed SSL certificate details.
/// Used when SslCertificate.type = "SELF_MANAGED".
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates#SslCertificateSelfManagedSslCertificate
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificateSelfManaged {
    /// PEM-encoded X.509 certificate chain.
    /// The chain must be no greater than 5 certificates long.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,

    /// PEM-encoded private key. Write-only; never returned in GET responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

/// Represents an SSL certificate resource for load balancers.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificate {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Type of certificate ("SELF_MANAGED" or "MANAGED").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Self-managed certificate details.
    /// Must be populated when type = "SELF_MANAGED".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_managed: Option<SslCertificateSelfManaged>,

    /// Domains covered by this certificate (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_alternative_names: Option<Vec<String>>,

    /// Expiration timestamp (RFC3339, output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// Creation timestamp (RFC3339, output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#sslCertificate").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Global Address
// =============================================================================================

/// Represents a global address resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// The static IP address represented by this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    /// The type of address (EXTERNAL or INTERNAL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<AddressType>,

    /// IP version (IPV4 or IPV6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_version: Option<IpVersion>,

    /// Purpose of the address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<AddressPurpose>,

    /// Network tier for this address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,

    /// Status of the address (RESERVED, IN_USE, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AddressStatus>,

    /// URL of the resource using this address.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<String>,

    /// Prefix length for IPv6 addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_length: Option<i32>,

    /// Network URL for internal addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Subnetwork URL for internal addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#address").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Address type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressType {
    /// External address.
    External,
    /// Internal address.
    Internal,
}

/// IP version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpVersion {
    /// IPv4.
    Ipv4,
    /// IPv6.
    Ipv6,
}

/// Address purpose.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressPurpose {
    /// GCE endpoint.
    GceEndpoint,
    /// VPC peering.
    VpcPeering,
    /// Private service connect.
    PrivateServiceConnect,
    /// NAT auto.
    NatAuto,
    /// Shared loadbalancer VIP.
    SharedLoadbalancerVip,
    /// DNS resolver.
    DnsResolver,
}

/// Address status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressStatus {
    /// Address is reserved.
    Reserved,
    /// Address is reserved but being used.
    Reserving,
    /// Address is in use.
    InUse,
}

// =============================================================================================
// Data Structures - Global Forwarding Rule
// =============================================================================================

/// Represents a forwarding rule resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRule {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// IP address for this forwarding rule.
    #[serde(rename = "IPAddress", skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// IP protocol for this forwarding rule.
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<ForwardingRuleProtocol>,

    /// Port range for this forwarding rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_range: Option<String>,

    /// List of ports for this forwarding rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,

    /// URL of the target resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// Load balancing scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,

    /// Network tier for this forwarding rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#forwardingRule").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Forwarding rule IP protocol.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForwardingRuleProtocol {
    /// TCP protocol.
    Tcp,
    /// UDP protocol.
    Udp,
    /// ESP protocol.
    Esp,
    /// AH protocol.
    Ah,
    /// SCTP protocol.
    Sctp,
    /// ICMP protocol.
    Icmp,
    /// L3 default protocol.
    L3Default,
}

// =============================================================================================
// Data Structures - Network Endpoint Group (NEG)
// =============================================================================================

/// Represents a network endpoint group resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroup {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Type of network endpoint group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_endpoint_type: Option<NetworkEndpointType>,

    /// Size of the network endpoint group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,

    /// URL of the network to which this NEG belongs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// URL of the subnetwork to which this NEG belongs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// URL of the zone where the NEG is located.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Default port for endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_port: Option<i32>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#networkEndpointGroup").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Cloud Run service configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_run: Option<NetworkEndpointGroupCloudRun>,

    /// App Engine service configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_engine: Option<NetworkEndpointGroupAppEngine>,

    /// Cloud Function configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_function: Option<NetworkEndpointGroupCloudFunction>,
}

/// Cloud Run service configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupCloudRun {
    /// Cloud Run service name.
    /// Example: "my-service"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Cloud Run service tag (optional).
    /// Example: "v1", "production"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// URL mask for routing to multiple Cloud Run services.
    /// Example: "<tag>.domain.com/<service>" allows routing based on URL patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// App Engine service configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupAppEngine {
    /// App Engine service name (optional).
    /// The service name is case-sensitive and must be 1-63 characters long.
    /// Example: "default", "my-service"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// App Engine version (optional).
    /// The version name is case-sensitive and must be 1-100 characters long.
    /// Example: "v1", "v2"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// URL mask for routing to multiple App Engine services.
    /// Example: "<service>-dot-appname.appspot.com/<version>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// Cloud Function configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupCloudFunction {
    /// Cloud Function name.
    /// The function name is case-sensitive and must be 1-63 characters long.
    /// Example: "func1"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,

    /// URL mask for routing to multiple Cloud Functions.
    /// Example: "/<function>" allows routing based on URL patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// Network endpoint type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkEndpointType {
    /// GCE VM IP port endpoint.
    GceVmIpPort,
    /// Non-GCP private IP port endpoint.
    NonGcpPrivateIpPort,
    /// Internet IP port endpoint.
    InternetIpPort,
    /// Internet FQDN port endpoint.
    InternetFqdnPort,
    /// Serverless endpoint.
    Serverless,
    /// Private service connect endpoint.
    PrivateServiceConnect,
}

/// Request to attach network endpoints.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupsAttachEndpointsRequest {
    /// Network endpoints to attach.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_endpoints: Vec<NetworkEndpoint>,
}

/// Request to detach network endpoints.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupsDetachEndpointsRequest {
    /// Network endpoints to detach.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_endpoints: Vec<NetworkEndpoint>,
}

/// Network endpoint.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpoint {
    /// IP address of the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// Port number for the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Instance that the endpoint belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// FQDN of the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
}

// =============================================================================================
// Data Structures - Instance Template
// =============================================================================================

/// Represents an instance template resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTemplate {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Instance properties for instances created from this template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<InstanceProperties>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#instanceTemplate").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Properties for instances created from a template.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProperties {
    /// Machine type for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<String>,

    /// Description of instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Disks attached to instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<AttachedDisk>,

    /// Network interfaces for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<NetworkInterface>,

    /// Metadata for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service accounts for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_accounts: Vec<ServiceAccount>,

    /// Tags for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,

    /// Scheduling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<Scheduling>,

    /// Labels for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Whether to allow stopping for update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_ip_forward: Option<bool>,

    /// Guest accelerators for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guest_accelerators: Vec<AcceleratorConfig>,

    /// Shielded instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shielded_instance_config: Option<ShieldedInstanceConfig>,

    /// Confidential instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidential_instance_config: Option<ConfidentialInstanceConfig>,
}

/// Attached disk configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AttachedDisk {
    /// Type of attachment (PERSISTENT, SCRATCH).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AttachedDiskType>,

    /// Mode of disk (READ_WRITE, READ_ONLY).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<DiskMode>,

    /// Source disk URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,

    /// Boot disk indicator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot: Option<bool>,

    /// Initialize parameters for new disks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialize_params: Option<AttachedDiskInitializeParams>,

    /// Whether to auto-delete the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete: Option<bool>,

    /// Index of the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,

    /// Disk interface (SCSI, NVME).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<DiskInterface>,
}

/// Attached disk type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttachedDiskType {
    /// Persistent disk.
    Persistent,
    /// Scratch disk.
    Scratch,
}

/// Disk mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskMode {
    /// Read-write mode.
    ReadWrite,
    /// Read-only mode.
    ReadOnly,
}

/// Disk interface.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskInterface {
    /// SCSI interface.
    Scsi,
    /// NVMe interface.
    Nvme,
}

/// Parameters for initializing a new disk.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AttachedDiskInitializeParams {
    /// Name for the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_name: Option<String>,

    /// Source image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image: Option<String>,

    /// Disk size in GB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_size_gb: Option<String>,

    /// Disk type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_type: Option<String>,

    /// Source snapshot URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot: Option<String>,

    /// Labels for the disk.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,
}

/// Network interface configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// Network URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Subnetwork URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Network IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_i_p: Option<String>,

    /// Name of the interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Access configurations for external IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access_configs: Vec<AccessConfig>,

    /// Alias IP ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_ip_ranges: Vec<AliasIpRange>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Stack type for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_type: Option<StackType>,

    /// Network interface card type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nic_type: Option<NicType>,
}

/// Access configuration for external IP.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AccessConfig {
    /// Type of access config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AccessConfigType>,

    /// Name of the access config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// External IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_i_p: Option<String>,

    /// Network tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,
}

/// Access config type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccessConfigType {
    /// One-to-one NAT.
    OneToOneNat,
    /// Direct IPv6 access.
    DirectIpv6,
}

/// Alias IP range.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AliasIpRange {
    /// IP CIDR range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,

    /// Subnetwork range name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork_range_name: Option<String>,
}

/// NIC type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NicType {
    /// Virtio NET.
    VirtioNet,
    /// gVNIC.
    Gvnic,
}

/// Metadata configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Metadata items.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MetadataItem>,

    /// Type of resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Metadata item.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MetadataItem {
    /// Key of the metadata item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// Value of the metadata item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Service account configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    /// Email address of the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// OAuth scopes.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

/// Tags configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Tags {
    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Tag items.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

/// Scheduling configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Scheduling {
    /// On host maintenance behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_host_maintenance: Option<OnHostMaintenance>,

    /// Automatic restart enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic_restart: Option<bool>,

    /// Whether this is a preemptible instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preemptible: Option<bool>,

    /// Provisioning model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_model: Option<ProvisioningModel>,
}

/// On host maintenance behavior.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnHostMaintenance {
    /// Migrate during maintenance.
    Migrate,
    /// Terminate during maintenance.
    Terminate,
}

/// Provisioning model.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProvisioningModel {
    /// Standard provisioning.
    Standard,
    /// Spot provisioning.
    Spot,
}

/// Accelerator configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorConfig {
    /// Type of accelerator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_type: Option<String>,

    /// Number of accelerators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_count: Option<i32>,
}

/// Shielded instance configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ShieldedInstanceConfig {
    /// Enable secure boot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_secure_boot: Option<bool>,

    /// Enable vTPM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_vtpm: Option<bool>,

    /// Enable integrity monitoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_integrity_monitoring: Option<bool>,
}

/// Confidential instance configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ConfidentialInstanceConfig {
    /// Enable confidential compute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_confidential_compute: Option<bool>,
}

// =============================================================================================
// Data Structures - Instance Group Manager
// =============================================================================================

/// Represents an instance group manager resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManager {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// URL of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_group: Option<String>,

    /// URL of the instance template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_template: Option<String>,

    /// Target size of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_size: Option<i32>,

    /// Base instance name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_instance_name: Option<String>,

    /// Current actions summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_actions: Option<InstanceGroupManagerActionsSummary>,

    /// Status of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InstanceGroupManagerStatus>,

    /// Target pools for this manager.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_pools: Vec<String>,

    /// Named ports for this manager.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_ports: Vec<NamedPort>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Auto healing policies.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_healing_policies: Vec<InstanceGroupManagerAutoHealingPolicy>,

    /// Update policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_policy: Option<InstanceGroupManagerUpdatePolicy>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#instanceGroupManager").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Summary of instance group manager actions.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerActionsSummary {
    /// Number of instances currently being created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creating: Option<i32>,

    /// Number of instances currently being deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleting: Option<i32>,

    /// Number of instances that exist and are running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<i32>,

    /// Number of instances currently being recreated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recreating: Option<i32>,

    /// Number of instances currently being refreshed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshing: Option<i32>,

    /// Number of instances currently being restarted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restarting: Option<i32>,

    /// Number of instances currently being verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifying: Option<i32>,

    /// Number of instances currently being abandoned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abandoning: Option<i32>,

    /// Number of instances in a creating without retries state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creating_without_retries: Option<i32>,
}

/// Status of an instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatus {
    /// Whether the group is stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stable: Option<bool>,

    /// Stateful status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stateful: Option<InstanceGroupManagerStatusStateful>,

    /// Version target status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_target: Option<InstanceGroupManagerStatusVersionTarget>,
}

/// Stateful status for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusStateful {
    /// Whether there are stateful instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_stateful_config: Option<bool>,

    /// Whether per-instance configs exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_instance_configs: Option<InstanceGroupManagerStatusStatefulPerInstanceConfigs>,
}

/// Per-instance configs status.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusStatefulPerInstanceConfigs {
    /// Whether all configs are effective.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_effective: Option<bool>,
}

/// Version target status.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusVersionTarget {
    /// Whether the version target has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_reached: Option<bool>,
}

/// Named port for instance group.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NamedPort {
    /// Name of the port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Port number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
}

/// Auto healing policy for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerAutoHealingPolicy {
    /// Health check URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<String>,

    /// Initial delay in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_sec: Option<i32>,
}

/// Update policy for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerUpdatePolicy {
    /// Type of update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<UpdatePolicyType>,

    /// Minimal action for updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimal_action: Option<MinimalAction>,

    /// Most disruptive action allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_disruptive_allowed_action: Option<MinimalAction>,

    /// Maximum surge instances (fixed or percent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_surge: Option<FixedOrPercent>,

    /// Maximum unavailable instances (fixed or percent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_unavailable: Option<FixedOrPercent>,

    /// Replacement method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_method: Option<ReplacementMethod>,
}

/// Update policy type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpdatePolicyType {
    /// Opportunistic update.
    Opportunistic,
    /// Proactive update.
    Proactive,
}

/// Minimal action for updates.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MinimalAction {
    /// No action.
    None,
    /// Refresh instance.
    Refresh,
    /// Restart instance.
    Restart,
    /// Replace instance.
    Replace,
}

/// Fixed or percent value.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FixedOrPercent {
    /// Fixed value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed: Option<i32>,

    /// Percentage value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,

    /// Calculated value (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculated: Option<i32>,
}

/// Replacement method.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplacementMethod {
    /// Substitute replacement.
    Substitute,
    /// Recreate replacement.
    Recreate,
}

/// Response for list managed instances.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagersListManagedInstancesResponse {
    /// List of managed instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_instances: Vec<ManagedInstance>,

    /// Next page token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Managed instance in an instance group.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstance {
    /// URL of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// Instance status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_status: Option<ManagedInstanceStatus>,

    /// Current action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_action: Option<ManagedInstanceCurrentAction>,

    /// Last attempt status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<ManagedInstanceLastAttempt>,

    /// Unique identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Version of the instance template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<ManagedInstanceVersion>,

    /// Health check results for this instance (populated when a health check is attached to the MIG).
    /// JSON field: instanceHealth
    #[serde(
        default,
        rename = "instanceHealth",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub instance_health: Vec<ManagedInstanceHealth>,
}

/// Health state of a managed instance as reported by a health check.
/// Returned in `ManagedInstance.instanceHealth[]` by listManagedInstances.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceHealth {
    /// URL of the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<String>,

    /// Detailed health state of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detailed_health_state: Option<ManagedInstanceDetailedHealthState>,
}

/// Detailed health state values for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceDetailedHealthState {
    /// The instance is reachable and health check responded with HEALTHY.
    Healthy,
    /// The health check responded with UNHEALTHY.
    Unhealthy,
    /// The instance is being drained and will not accept new connections.
    Draining,
    /// The health check timed out.
    Timeout,
    /// The health state is unknown (e.g., health check not yet run).
    Unknown,
}

/// Status of a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceStatus {
    /// Instance is running.
    Running,
    /// Instance is pending.
    Pending,
    /// Instance is provisioning.
    Provisioning,
    /// Instance is staging.
    Staging,
    /// Instance is stopped.
    Stopped,
    /// Instance is stopping.
    Stopping,
    /// Instance is suspended.
    Suspended,
    /// Instance is suspending.
    Suspending,
    /// Instance is terminated.
    Terminated,
}

/// Current action on a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceCurrentAction {
    /// No action.
    None,
    /// Creating instance.
    Creating,
    /// Creating without retries.
    CreatingWithoutRetries,
    /// Recreating instance.
    Recreating,
    /// Deleting instance.
    Deleting,
    /// Abandoning instance.
    Abandoning,
    /// Restarting instance.
    Restarting,
    /// Refreshing instance.
    Refreshing,
    /// Verifying instance.
    Verifying,
}

/// Last attempt status for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttempt {
    /// Errors from last attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<ManagedInstanceLastAttemptErrors>,
}

/// Errors from last attempt.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptErrors {
    /// List of errors.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ManagedInstanceLastAttemptErrorsErrors>,
}

/// Individual error from last attempt.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptErrorsErrors {
    /// Error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Version information for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceVersion {
    /// Instance template URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_template: Option<String>,

    /// Version name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// =============================================================================================
// Data Structures - Instance
// =============================================================================================

/// Represents a compute instance resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Machine type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<String>,

    /// Status of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InstanceStatus>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Disks attached to this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<AttachedDisk>,

    /// Network interfaces for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<NetworkInterface>,

    /// Metadata for this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service accounts for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_accounts: Vec<ServiceAccount>,

    /// Tags for this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,

    /// Scheduling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<Scheduling>,

    /// Labels for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Whether IP forwarding is allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_ip_forward: Option<bool>,

    /// CPU platform.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_platform: Option<String>,

    /// Guest accelerators for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guest_accelerators: Vec<AcceleratorConfig>,

    /// Shielded instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shielded_instance_config: Option<ShieldedInstanceConfig>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Last start timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_start_timestamp: Option<String>,

    /// Last stop timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stop_timestamp: Option<String>,

    /// Type of resource (always "compute#instance").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Instance status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstanceStatus {
    /// Instance is running.
    Running,
    /// Instance is provisioning.
    Provisioning,
    /// Instance is staging.
    Staging,
    /// Instance is stopped.
    Stopped,
    /// Instance is stopping.
    Stopping,
    /// Instance is suspended.
    Suspended,
    /// Instance is suspending.
    Suspending,
    /// Instance is terminated.
    Terminated,
    /// Instance is pending.
    Pending,
}

// =============================================================================================
// Data Structures - Disk
// =============================================================================================

/// Represents a persistent disk resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/disks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Size of the disk in GB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_gb: Option<String>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Status of the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DiskStatus>,

    /// Source snapshot URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot: Option<String>,

    /// Source snapshot ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,

    /// Source image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image: Option<String>,

    /// Source image ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image_id: Option<String>,

    /// Disk type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Users of this disk (instance URLs).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<String>,

    /// Labels for this disk.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Label fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fingerprint: Option<String>,

    /// Physical block size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_block_size_bytes: Option<String>,

    /// Provisioned IOPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_iops: Option<i64>,

    /// Provisioned throughput.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput: Option<i64>,

    /// Last attach timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attach_timestamp: Option<String>,

    /// Last detach timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_detach_timestamp: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#disk").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Disk status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskStatus {
    /// Disk is being created.
    Creating,
    /// Disk is restoring from snapshot.
    Restoring,
    /// Disk creation failed.
    Failed,
    /// Disk is ready.
    Ready,
    /// Disk is being deleted.
    Deleting,
}

// =============================================================================================
// Data Structures - Serial Port Output
// =============================================================================================

/// Serial port output from a GCP compute instance (port 1 = main console).
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/getSerialPortOutput
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortOutput {
    /// The contents of the serial port output.
    pub contents: Option<String>,
    /// The starting byte position of the output that was returned.
    pub start: Option<String>,
    /// The byte position of the next byte to read (pagination cursor).
    pub next: Option<String>,
}
