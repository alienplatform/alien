use super::types::*;
use super::{ComputeApi, ComputeServiceConfig};
use crate::gcp::api_client::GcpClientBase;
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use reqwest::{Client, Method};

mod instances;
mod load_balancing;

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
        self.get_health_check_impl(health_check_name).await
    }

    async fn insert_health_check(&self, health_check: HealthCheck) -> Result<Operation> {
        self.insert_health_check_impl(health_check).await
    }

    async fn delete_health_check(&self, health_check_name: String) -> Result<Operation> {
        self.delete_health_check_impl(health_check_name).await
    }

    async fn patch_health_check(
        &self,
        health_check_name: String,
        health_check: HealthCheck,
    ) -> Result<Operation> {
        self.patch_health_check_impl(health_check_name, health_check)
            .await
    }

    async fn get_backend_service(&self, backend_service_name: String) -> Result<BackendService> {
        self.get_backend_service_impl(backend_service_name).await
    }

    async fn insert_backend_service(&self, backend_service: BackendService) -> Result<Operation> {
        self.insert_backend_service_impl(backend_service).await
    }

    async fn delete_backend_service(&self, backend_service_name: String) -> Result<Operation> {
        self.delete_backend_service_impl(backend_service_name).await
    }

    async fn patch_backend_service(
        &self,
        backend_service_name: String,
        backend_service: BackendService,
    ) -> Result<Operation> {
        self.patch_backend_service_impl(backend_service_name, backend_service)
            .await
    }

    async fn get_url_map(&self, url_map_name: String) -> Result<UrlMap> {
        self.get_url_map_impl(url_map_name).await
    }

    async fn insert_url_map(&self, url_map: UrlMap) -> Result<Operation> {
        self.insert_url_map_impl(url_map).await
    }

    async fn delete_url_map(&self, url_map_name: String) -> Result<Operation> {
        self.delete_url_map_impl(url_map_name).await
    }

    async fn get_target_http_proxy(
        &self,
        target_http_proxy_name: String,
    ) -> Result<TargetHttpProxy> {
        self.get_target_http_proxy_impl(target_http_proxy_name)
            .await
    }

    async fn insert_target_http_proxy(
        &self,
        target_http_proxy: TargetHttpProxy,
    ) -> Result<Operation> {
        self.insert_target_http_proxy_impl(target_http_proxy).await
    }

    async fn delete_target_http_proxy(&self, target_http_proxy_name: String) -> Result<Operation> {
        self.delete_target_http_proxy_impl(target_http_proxy_name)
            .await
    }

    async fn get_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> Result<TargetHttpsProxy> {
        self.get_target_https_proxy_impl(target_https_proxy_name)
            .await
    }

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> Result<Operation> {
        self.insert_target_https_proxy_impl(target_https_proxy)
            .await
    }

    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> Result<Operation> {
        self.set_target_https_proxy_ssl_certificates_impl(target_https_proxy_name, ssl_certificates)
            .await
    }

    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> Result<Operation> {
        self.delete_target_https_proxy_impl(target_https_proxy_name)
            .await
    }

    async fn insert_target_tcp_proxy(&self, target_tcp_proxy: TargetTcpProxy) -> Result<Operation> {
        self.insert_target_tcp_proxy_impl(target_tcp_proxy).await
    }

    async fn delete_target_tcp_proxy(&self, target_tcp_proxy_name: String) -> Result<Operation> {
        self.delete_target_tcp_proxy_impl(target_tcp_proxy_name)
            .await
    }

    async fn get_ssl_certificate(&self, ssl_certificate_name: String) -> Result<SslCertificate> {
        self.get_ssl_certificate_impl(ssl_certificate_name).await
    }

    async fn insert_ssl_certificate(&self, ssl_certificate: SslCertificate) -> Result<Operation> {
        self.insert_ssl_certificate_impl(ssl_certificate).await
    }

    async fn delete_ssl_certificate(&self, ssl_certificate_name: String) -> Result<Operation> {
        self.delete_ssl_certificate_impl(ssl_certificate_name).await
    }

    async fn get_global_address(&self, address_name: String) -> Result<Address> {
        self.get_global_address_impl(address_name).await
    }

    async fn insert_global_address(&self, address: Address) -> Result<Operation> {
        self.insert_global_address_impl(address).await
    }

    async fn delete_global_address(&self, address_name: String) -> Result<Operation> {
        self.delete_global_address_impl(address_name).await
    }

    async fn get_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule> {
        self.get_global_forwarding_rule_impl(forwarding_rule_name)
            .await
    }

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation> {
        self.insert_global_forwarding_rule_impl(forwarding_rule)
            .await
    }

    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> Result<Operation> {
        self.delete_global_forwarding_rule_impl(forwarding_rule_name)
            .await
    }

    async fn get_address(&self, region: String, address_name: String) -> Result<Address> {
        self.get_address_impl(region, address_name).await
    }

    async fn insert_address(&self, region: String, address: Address) -> Result<Operation> {
        self.insert_address_impl(region, address).await
    }

    async fn delete_address(&self, region: String, address_name: String) -> Result<Operation> {
        self.delete_address_impl(region, address_name).await
    }

    async fn get_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<ForwardingRule> {
        self.get_forwarding_rule_impl(region, forwarding_rule_name)
            .await
    }

    async fn insert_forwarding_rule(
        &self,
        region: String,
        forwarding_rule: ForwardingRule,
    ) -> Result<Operation> {
        self.insert_forwarding_rule_impl(region, forwarding_rule)
            .await
    }

    async fn delete_forwarding_rule(
        &self,
        region: String,
        forwarding_rule_name: String,
    ) -> Result<Operation> {
        self.delete_forwarding_rule_impl(region, forwarding_rule_name)
            .await
    }

    async fn get_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup> {
        self.get_network_endpoint_group_impl(zone, neg_name).await
    }

    async fn insert_network_endpoint_group(
        &self,
        zone: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation> {
        self.insert_network_endpoint_group_impl(zone, neg).await
    }

    async fn delete_network_endpoint_group(
        &self,
        zone: String,
        neg_name: String,
    ) -> Result<Operation> {
        self.delete_network_endpoint_group_impl(zone, neg_name)
            .await
    }

    async fn attach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsAttachEndpointsRequest,
    ) -> Result<Operation> {
        self.attach_network_endpoints_impl(zone, neg_name, request)
            .await
    }

    async fn detach_network_endpoints(
        &self,
        zone: String,
        neg_name: String,
        request: NetworkEndpointGroupsDetachEndpointsRequest,
    ) -> Result<Operation> {
        self.detach_network_endpoints_impl(zone, neg_name, request)
            .await
    }

    async fn get_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<NetworkEndpointGroup> {
        self.get_region_network_endpoint_group_impl(region, neg_name)
            .await
    }

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        neg: NetworkEndpointGroup,
    ) -> Result<Operation> {
        self.insert_region_network_endpoint_group_impl(region, neg)
            .await
    }

    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        neg_name: String,
    ) -> Result<Operation> {
        self.delete_region_network_endpoint_group_impl(region, neg_name)
            .await
    }

    async fn get_instance_template(
        &self,
        instance_template_name: String,
    ) -> Result<InstanceTemplate> {
        self.get_instance_template_impl(instance_template_name)
            .await
    }

    async fn insert_instance_template(
        &self,
        instance_template: InstanceTemplate,
    ) -> Result<Operation> {
        self.insert_instance_template_impl(instance_template).await
    }

    async fn delete_instance_template(&self, instance_template_name: String) -> Result<Operation> {
        self.delete_instance_template_impl(instance_template_name)
            .await
    }

    async fn get_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManager> {
        self.get_instance_group_manager_impl(zone, instance_group_manager_name)
            .await
    }

    async fn insert_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager: InstanceGroupManager,
    ) -> Result<Operation> {
        self.insert_instance_group_manager_impl(zone, instance_group_manager)
            .await
    }

    async fn delete_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<Operation> {
        self.delete_instance_group_manager_impl(zone, instance_group_manager_name)
            .await
    }

    async fn resize_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        size: i32,
    ) -> Result<Operation> {
        self.resize_instance_group_manager_impl(zone, instance_group_manager_name, size)
            .await
    }

    async fn delete_instance_group_manager_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
        request: InstanceGroupManagersDeleteInstancesRequest,
    ) -> Result<Operation> {
        self.delete_instance_group_manager_instances_impl(
            zone,
            instance_group_manager_name,
            request,
        )
        .await
    }

    async fn list_managed_instances(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManagersListManagedInstancesResponse> {
        self.list_managed_instances_impl(zone, instance_group_manager_name)
            .await
    }

    async fn patch_instance_group_manager(
        &self,
        zone: String,
        instance_group_manager_name: String,
        patch: InstanceGroupManager,
    ) -> Result<Operation> {
        self.patch_instance_group_manager_impl(zone, instance_group_manager_name, patch)
            .await
    }

    async fn get_instance(&self, zone: String, instance_name: String) -> Result<Instance> {
        self.get_instance_impl(zone, instance_name).await
    }

    async fn delete_instance(&self, zone: String, instance_name: String) -> Result<Operation> {
        self.delete_instance_impl(zone, instance_name).await
    }

    async fn attach_disk(
        &self,
        zone: String,
        instance_name: String,
        attached_disk: AttachedDisk,
    ) -> Result<Operation> {
        self.attach_disk_impl(zone, instance_name, attached_disk)
            .await
    }

    async fn detach_disk(
        &self,
        zone: String,
        instance_name: String,
        device_name: String,
    ) -> Result<Operation> {
        self.detach_disk_impl(zone, instance_name, device_name)
            .await
    }

    async fn get_disk(&self, zone: String, disk_name: String) -> Result<Disk> {
        self.get_disk_impl(zone, disk_name).await
    }

    async fn insert_disk(&self, zone: String, disk: Disk) -> Result<Operation> {
        self.insert_disk_impl(zone, disk).await
    }

    async fn delete_disk(&self, zone: String, disk_name: String) -> Result<Operation> {
        self.delete_disk_impl(zone, disk_name).await
    }

    async fn get_serial_port_output(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<SerialPortOutput> {
        self.get_serial_port_output_impl(zone, instance_name).await
    }
}
