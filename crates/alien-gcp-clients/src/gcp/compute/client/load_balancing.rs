use super::*;

impl ComputeClient {
    pub(super) async fn get_health_check_impl(
        &self,
        health_check_name: String,
    ) -> Result<HealthCheck> {
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

    pub(super) async fn insert_health_check_impl(
        &self,
        health_check: HealthCheck,
    ) -> Result<Operation> {
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

    pub(super) async fn delete_health_check_impl(
        &self,
        health_check_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn patch_health_check_impl(
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

    pub(super) async fn get_backend_service_impl(
        &self,
        backend_service_name: String,
    ) -> Result<BackendService> {
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

    pub(super) async fn insert_backend_service_impl(
        &self,
        backend_service: BackendService,
    ) -> Result<Operation> {
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

    pub(super) async fn delete_backend_service_impl(
        &self,
        backend_service_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn patch_backend_service_impl(
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

    pub(super) async fn get_url_map_impl(&self, url_map_name: String) -> Result<UrlMap> {
        let path = format!(
            "projects/{}/global/urlMaps/{}",
            self.project_id, url_map_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &url_map_name)
            .await
    }

    pub(super) async fn insert_url_map_impl(&self, url_map: UrlMap) -> Result<Operation> {
        let path = format!("projects/{}/global/urlMaps", self.project_id);
        let resource_name = url_map.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(url_map), &resource_name)
            .await
    }

    pub(super) async fn delete_url_map_impl(&self, url_map_name: String) -> Result<Operation> {
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

    pub(super) async fn get_target_http_proxy_impl(
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

    pub(super) async fn insert_target_http_proxy_impl(
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

    pub(super) async fn delete_target_http_proxy_impl(
        &self,
        target_http_proxy_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn get_target_https_proxy_impl(
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

    pub(super) async fn insert_target_https_proxy_impl(
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

    pub(super) async fn set_target_https_proxy_ssl_certificates_impl(
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

    pub(super) async fn delete_target_https_proxy_impl(
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

    pub(super) async fn insert_target_tcp_proxy_impl(
        &self,
        target_tcp_proxy: TargetTcpProxy,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/targetTcpProxies", self.project_id);
        let name = target_tcp_proxy
            .name
            .clone()
            .unwrap_or_else(|| "targetTcpProxy".to_string());
        self.base
            .execute_request(Method::POST, &path, None, Some(target_tcp_proxy), &name)
            .await
    }

    pub(super) async fn delete_target_tcp_proxy_impl(
        &self,
        target_tcp_proxy_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn get_ssl_certificate_impl(
        &self,
        ssl_certificate_name: String,
    ) -> Result<SslCertificate> {
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

    pub(super) async fn insert_ssl_certificate_impl(
        &self,
        ssl_certificate: SslCertificate,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/sslCertificates", self.project_id);
        let name = ssl_certificate
            .name
            .clone()
            .unwrap_or_else(|| "sslCertificate".to_string());
        self.base
            .execute_request(Method::POST, &path, None, Some(ssl_certificate), &name)
            .await
    }

    pub(super) async fn delete_ssl_certificate_impl(
        &self,
        ssl_certificate_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn get_global_address_impl(&self, address_name: String) -> Result<Address> {
        let path = format!(
            "projects/{}/global/addresses/{}",
            self.project_id, address_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &address_name)
            .await
    }

    pub(super) async fn insert_global_address_impl(&self, address: Address) -> Result<Operation> {
        let path = format!("projects/{}/global/addresses", self.project_id);
        let resource_name = address.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(address), &resource_name)
            .await
    }

    pub(super) async fn delete_global_address_impl(
        &self,
        address_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn get_global_forwarding_rule_impl(
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

    pub(super) async fn insert_global_forwarding_rule_impl(
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

    pub(super) async fn delete_global_forwarding_rule_impl(
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

    pub(super) async fn get_address_impl(
        &self,
        region: String,
        address_name: String,
    ) -> Result<Address> {
        let path = format!(
            "projects/{}/regions/{}/addresses/{}",
            self.project_id, region, address_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &address_name)
            .await
    }

    pub(super) async fn insert_address_impl(
        &self,
        region: String,
        address: Address,
    ) -> Result<Operation> {
        let path = format!("projects/{}/regions/{}/addresses", self.project_id, region);
        let resource_name = address.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(address), &resource_name)
            .await
    }

    pub(super) async fn delete_address_impl(
        &self,
        region: String,
        address_name: String,
    ) -> Result<Operation> {
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

    pub(super) async fn get_forwarding_rule_impl(
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

    pub(super) async fn insert_forwarding_rule_impl(
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

    pub(super) async fn delete_forwarding_rule_impl(
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

    pub(super) async fn get_network_endpoint_group_impl(
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

    pub(super) async fn insert_network_endpoint_group_impl(
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

    pub(super) async fn delete_network_endpoint_group_impl(
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

    pub(super) async fn attach_network_endpoints_impl(
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

    pub(super) async fn detach_network_endpoints_impl(
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

    pub(super) async fn get_region_network_endpoint_group_impl(
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

    pub(super) async fn insert_region_network_endpoint_group_impl(
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

    pub(super) async fn delete_region_network_endpoint_group_impl(
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
}
