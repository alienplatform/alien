use super::types::*;
use alien_client_core::Result;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

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
