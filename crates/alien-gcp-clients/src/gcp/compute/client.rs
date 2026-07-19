use super::types::*;
use super::{ComputeApi, ComputeServiceConfig};
use crate::gcp::api_client::GcpClientBase;
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use reqwest::{Client, Method};

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
