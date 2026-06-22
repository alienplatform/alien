use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use google_cloud_compute_v1::{
    client::{
        BackendServices, Firewalls, GlobalAddresses, GlobalForwardingRules, GlobalOperations,
        Networks, RegionNetworkEndpointGroups, RegionOperations, Routers, SslCertificates,
        Subnetworks, TargetHttpsProxies, UrlMaps,
    },
    model::TargetHttpsProxiesSetSslCertificatesRequest,
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use http::StatusCode;
use tokio::sync::OnceCell;

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

use google_cloud_compute_v1::model::{
    operation::Status as OperationStatus, Address, BackendService, Firewall, ForwardingRule,
    Network, NetworkEndpointGroup, Operation, Router, SslCertificate, Subnetwork, TargetHttpsProxy,
    UrlMap,
};

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcpComputeApi: Send + Sync + std::fmt::Debug {
    async fn get_network(&self, network_name: String) -> CloudClientResult<Network>;
    async fn insert_network(&self, network: Network) -> CloudClientResult<Operation>;
    async fn delete_network(&self, network_name: String) -> CloudClientResult<Operation>;

    async fn get_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Subnetwork>;
    async fn insert_subnetwork(
        &self,
        region: String,
        subnetwork: Subnetwork,
    ) -> CloudClientResult<Operation>;
    async fn delete_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Operation>;

    async fn get_router(&self, region: String, router_name: String) -> CloudClientResult<Router>;
    async fn insert_router(&self, region: String, router: Router) -> CloudClientResult<Operation>;
    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> CloudClientResult<Operation>;
    async fn delete_router(
        &self,
        region: String,
        router_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_firewall(&self, firewall: Firewall) -> CloudClientResult<Operation>;
    async fn delete_firewall(&self, firewall_name: String) -> CloudClientResult<Operation>;

    async fn get_global_operation(&self, operation_name: String) -> CloudClientResult<Operation>;
    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_backend_service(
        &self,
        backend_service: BackendService,
    ) -> CloudClientResult<Operation>;
    async fn delete_backend_service(
        &self,
        backend_service_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_url_map(&self, url_map: UrlMap) -> CloudClientResult<Operation>;
    async fn delete_url_map(&self, url_map_name: String) -> CloudClientResult<Operation>;

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> CloudClientResult<Operation>;
    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> CloudClientResult<Operation>;
    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_ssl_certificate(
        &self,
        ssl_certificate: SslCertificate,
    ) -> CloudClientResult<Operation>;
    async fn delete_ssl_certificate(
        &self,
        ssl_certificate_name: String,
    ) -> CloudClientResult<Operation>;

    async fn get_global_address(&self, address_name: String) -> CloudClientResult<Address>;
    async fn insert_global_address(&self, address: Address) -> CloudClientResult<Operation>;
    async fn delete_global_address(&self, address_name: String) -> CloudClientResult<Operation>;

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> CloudClientResult<Operation>;
    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group: NetworkEndpointGroup,
    ) -> CloudClientResult<Operation>;
    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group_name: String,
    ) -> CloudClientResult<Operation>;
}

pub fn operation_is_done(operation: &Operation) -> bool {
    matches!(operation.status, Some(OperationStatus::Done))
}

pub fn operation_has_error(operation: &Operation) -> bool {
    operation
        .error
        .as_ref()
        .is_some_and(|error| !error.errors.is_empty())
}

pub struct OfficialGcpComputeClient {
    config: GcpClientConfig,
    networks: OnceCell<Networks>,
    subnetworks: OnceCell<Subnetworks>,
    routers: OnceCell<Routers>,
    firewalls: OnceCell<Firewalls>,
    global_operations: OnceCell<GlobalOperations>,
    region_operations: OnceCell<RegionOperations>,
    backend_services: OnceCell<BackendServices>,
    url_maps: OnceCell<UrlMaps>,
    target_https_proxies: OnceCell<TargetHttpsProxies>,
    ssl_certificates: OnceCell<SslCertificates>,
    global_addresses: OnceCell<GlobalAddresses>,
    global_forwarding_rules: OnceCell<GlobalForwardingRules>,
    region_network_endpoint_groups: OnceCell<RegionNetworkEndpointGroups>,
}

impl std::fmt::Debug for OfficialGcpComputeClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpComputeClient")
            .field("project_id", &self.config.project_id)
            .field("region", &self.config.region)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpComputeClient {
    pub fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            networks: OnceCell::new(),
            subnetworks: OnceCell::new(),
            routers: OnceCell::new(),
            firewalls: OnceCell::new(),
            global_operations: OnceCell::new(),
            region_operations: OnceCell::new(),
            backend_services: OnceCell::new(),
            url_maps: OnceCell::new(),
            target_https_proxies: OnceCell::new(),
            ssl_certificates: OnceCell::new(),
            global_addresses: OnceCell::new(),
            global_forwarding_rules: OnceCell::new(),
            region_network_endpoint_groups: OnceCell::new(),
        }
    }

    async fn networks(&self) -> CloudClientResult<&Networks> {
        self.networks
            .get_or_try_init(|| async {
                build_compute_client(&self.config, Networks::builder).await
            })
            .await
    }

    async fn subnetworks(&self) -> CloudClientResult<&Subnetworks> {
        self.subnetworks
            .get_or_try_init(|| async {
                build_compute_client(&self.config, Subnetworks::builder).await
            })
            .await
    }

    async fn routers(&self) -> CloudClientResult<&Routers> {
        self.routers
            .get_or_try_init(|| async {
                build_compute_client(&self.config, Routers::builder).await
            })
            .await
    }

    async fn firewalls(&self) -> CloudClientResult<&Firewalls> {
        self.firewalls
            .get_or_try_init(|| async {
                build_compute_client(&self.config, Firewalls::builder).await
            })
            .await
    }

    async fn global_operations(&self) -> CloudClientResult<&GlobalOperations> {
        self.global_operations
            .get_or_try_init(|| async {
                build_compute_client(&self.config, GlobalOperations::builder).await
            })
            .await
    }

    async fn region_operations(&self) -> CloudClientResult<&RegionOperations> {
        self.region_operations
            .get_or_try_init(|| async {
                build_compute_client(&self.config, RegionOperations::builder).await
            })
            .await
    }

    async fn backend_services(&self) -> CloudClientResult<&BackendServices> {
        self.backend_services
            .get_or_try_init(|| async {
                build_compute_client(&self.config, BackendServices::builder).await
            })
            .await
    }

    async fn url_maps(&self) -> CloudClientResult<&UrlMaps> {
        self.url_maps
            .get_or_try_init(|| async {
                build_compute_client(&self.config, UrlMaps::builder).await
            })
            .await
    }

    async fn target_https_proxies(&self) -> CloudClientResult<&TargetHttpsProxies> {
        self.target_https_proxies
            .get_or_try_init(|| async {
                build_compute_client(&self.config, TargetHttpsProxies::builder).await
            })
            .await
    }

    async fn ssl_certificates(&self) -> CloudClientResult<&SslCertificates> {
        self.ssl_certificates
            .get_or_try_init(|| async {
                build_compute_client(&self.config, SslCertificates::builder).await
            })
            .await
    }

    async fn global_addresses(&self) -> CloudClientResult<&GlobalAddresses> {
        self.global_addresses
            .get_or_try_init(|| async {
                build_compute_client(&self.config, GlobalAddresses::builder).await
            })
            .await
    }

    async fn global_forwarding_rules(&self) -> CloudClientResult<&GlobalForwardingRules> {
        self.global_forwarding_rules
            .get_or_try_init(|| async {
                build_compute_client(&self.config, GlobalForwardingRules::builder).await
            })
            .await
    }

    async fn region_network_endpoint_groups(
        &self,
    ) -> CloudClientResult<&RegionNetworkEndpointGroups> {
        self.region_network_endpoint_groups
            .get_or_try_init(|| async {
                build_compute_client(&self.config, RegionNetworkEndpointGroups::builder).await
            })
            .await
    }
}

#[async_trait::async_trait]
impl GcpComputeApi for OfficialGcpComputeClient {
    async fn get_network(&self, network_name: String) -> CloudClientResult<Network> {
        self.networks()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_network(network_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &network_name))
    }

    async fn insert_network(&self, network: Network) -> CloudClientResult<Operation> {
        let name = resource_name(&network.name);
        self.networks()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(network)
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &name))
    }

    async fn delete_network(&self, network_name: String) -> CloudClientResult<Operation> {
        self.networks()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_network(network_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &network_name))
    }

    async fn get_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Subnetwork> {
        self.subnetworks()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_subnetwork(subnetwork_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &subnetwork_name))
    }

    async fn insert_subnetwork(
        &self,
        region: String,
        subnetwork: Subnetwork,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&subnetwork.name);
        self.subnetworks()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(subnetwork)
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &name))
    }

    async fn delete_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Operation> {
        self.subnetworks()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_subnetwork(subnetwork_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &subnetwork_name))
    }

    async fn get_router(&self, region: String, router_name: String) -> CloudClientResult<Router> {
        self.routers()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
    }

    async fn insert_router(&self, region: String, router: Router) -> CloudClientResult<Operation> {
        let name = resource_name(&router.name);
        self.routers()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(router)
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &name))
    }

    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> CloudClientResult<Operation> {
        self.routers()
            .await?
            .patch()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .set_body(router)
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
    }

    async fn delete_router(
        &self,
        region: String,
        router_name: String,
    ) -> CloudClientResult<Operation> {
        self.routers()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
    }

    async fn insert_firewall(&self, firewall: Firewall) -> CloudClientResult<Operation> {
        let name = resource_name(&firewall.name);
        self.firewalls()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(firewall)
            .send()
            .await
            .map_err(|error| compute_error(error, "firewall", &name))
    }

    async fn delete_firewall(&self, firewall_name: String) -> CloudClientResult<Operation> {
        self.firewalls()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_firewall(firewall_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "firewall", &firewall_name))
    }

    async fn get_global_operation(&self, operation_name: String) -> CloudClientResult<Operation> {
        self.global_operations()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_operation(operation_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalOperation", &operation_name))
    }

    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> CloudClientResult<Operation> {
        self.region_operations()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_operation(operation_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "regionOperation", &operation_name))
    }

    async fn insert_backend_service(
        &self,
        backend_service: BackendService,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&backend_service.name);
        self.backend_services()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(backend_service)
            .send()
            .await
            .map_err(|error| compute_error(error, "backendService", &name))
    }

    async fn delete_backend_service(
        &self,
        backend_service_name: String,
    ) -> CloudClientResult<Operation> {
        self.backend_services()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_backend_service(backend_service_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "backendService", &backend_service_name))
    }

    async fn insert_url_map(&self, url_map: UrlMap) -> CloudClientResult<Operation> {
        let name = resource_name(&url_map.name);
        self.url_maps()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(url_map)
            .send()
            .await
            .map_err(|error| compute_error(error, "urlMap", &name))
    }

    async fn delete_url_map(&self, url_map_name: String) -> CloudClientResult<Operation> {
        self.url_maps()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_url_map(url_map_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "urlMap", &url_map_name))
    }

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&target_https_proxy.name);
        self.target_https_proxies()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(target_https_proxy)
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &name))
    }

    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> CloudClientResult<Operation> {
        let request = TargetHttpsProxiesSetSslCertificatesRequest::new()
            .set_ssl_certificates(ssl_certificates);
        self.target_https_proxies()
            .await?
            .set_ssl_certificates()
            .set_project(self.config.project_id.clone())
            .set_target_https_proxy(target_https_proxy_name.clone())
            .set_body(request)
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &target_https_proxy_name))
    }

    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> CloudClientResult<Operation> {
        self.target_https_proxies()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_target_https_proxy(target_https_proxy_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &target_https_proxy_name))
    }

    async fn insert_ssl_certificate(
        &self,
        ssl_certificate: SslCertificate,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&ssl_certificate.name);
        self.ssl_certificates()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(ssl_certificate)
            .send()
            .await
            .map_err(|error| compute_error(error, "sslCertificate", &name))
    }

    async fn delete_ssl_certificate(
        &self,
        ssl_certificate_name: String,
    ) -> CloudClientResult<Operation> {
        self.ssl_certificates()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_ssl_certificate(ssl_certificate_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "sslCertificate", &ssl_certificate_name))
    }

    async fn get_global_address(&self, address_name: String) -> CloudClientResult<Address> {
        self.global_addresses()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_address(address_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &address_name))
    }

    async fn insert_global_address(&self, address: Address) -> CloudClientResult<Operation> {
        let name = resource_name(&address.name);
        self.global_addresses()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(address)
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &name))
    }

    async fn delete_global_address(&self, address_name: String) -> CloudClientResult<Operation> {
        self.global_addresses()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_address(address_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &address_name))
    }

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&forwarding_rule.name);
        self.global_forwarding_rules()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(forwarding_rule)
            .send()
            .await
            .map_err(|error| compute_error(error, "globalForwardingRule", &name))
    }

    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> CloudClientResult<Operation> {
        self.global_forwarding_rules()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_forwarding_rule(forwarding_rule_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalForwardingRule", &forwarding_rule_name))
    }

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group: NetworkEndpointGroup,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&network_endpoint_group.name);
        self.region_network_endpoint_groups()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(network_endpoint_group)
            .send()
            .await
            .map_err(|error| compute_error(error, "regionNetworkEndpointGroup", &name))
    }

    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group_name: String,
    ) -> CloudClientResult<Operation> {
        self.region_network_endpoint_groups()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_network_endpoint_group(network_endpoint_group_name.clone())
            .send()
            .await
            .map_err(|error| {
                compute_error(
                    error,
                    "regionNetworkEndpointGroup",
                    &network_endpoint_group_name,
                )
            })
    }
}

async fn build_compute_client<T, B>(
    config: &GcpClientConfig,
    builder: impl FnOnce() -> B,
) -> CloudClientResult<T>
where
    B: ComputeClientBuilder<T>,
{
    let credentials = crate::core::gcp_credentials_from_alien_config(config).map_err(|error| {
        AlienError::new(CloudClientErrorData::AuthenticationError {
            message: error.to_string(),
        })
    })?;
    let mut builder = builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("compute"))
    {
        builder = builder.with_endpoint(compute_endpoint(endpoint));
    }

    builder.build().await.map_err(|error| {
        AlienError::new(CloudClientErrorData::GenericError {
            message: format!("Failed to build official GCP Compute client: {error}"),
        })
    })
}

#[async_trait::async_trait]
trait ComputeClientBuilder<T>: Sized {
    fn with_credentials(self, credentials: google_cloud_auth::credentials::Credentials) -> Self;
    fn with_endpoint(self, endpoint: String) -> Self;
    async fn build(self) -> Result<T, google_cloud_gax::client_builder::Error>;
}

macro_rules! impl_compute_client_builder {
    ($builder:ty, $client:ty) => {
        #[async_trait::async_trait]
        impl ComputeClientBuilder<$client> for $builder {
            fn with_credentials(
                self,
                credentials: google_cloud_auth::credentials::Credentials,
            ) -> Self {
                self.with_credentials(credentials)
            }

            fn with_endpoint(self, endpoint: String) -> Self {
                self.with_endpoint(endpoint)
            }

            async fn build(self) -> Result<$client, google_cloud_gax::client_builder::Error> {
                self.build().await
            }
        }
    };
}

impl_compute_client_builder!(
    google_cloud_compute_v1::builder::networks::ClientBuilder,
    Networks
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::subnetworks::ClientBuilder,
    Subnetworks
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::routers::ClientBuilder,
    Routers
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::firewalls::ClientBuilder,
    Firewalls
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_operations::ClientBuilder,
    GlobalOperations
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::region_operations::ClientBuilder,
    RegionOperations
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::backend_services::ClientBuilder,
    BackendServices
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::url_maps::ClientBuilder,
    UrlMaps
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::target_https_proxies::ClientBuilder,
    TargetHttpsProxies
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::ssl_certificates::ClientBuilder,
    SslCertificates
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_addresses::ClientBuilder,
    GlobalAddresses
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_forwarding_rules::ClientBuilder,
    GlobalForwardingRules
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::region_network_endpoint_groups::ClientBuilder,
    RegionNetworkEndpointGroups
);

fn compute_error(
    error: google_cloud_gax::error::Error,
    resource_type: &str,
    resource_name: &str,
) -> AlienError<CloudClientErrorData> {
    if gax_error_is_not_found(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    if gax_error_is_conflict(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            message: error.to_string(),
        });
    }

    if gax_error_is_permission_denied(&error) {
        return AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    AlienError::new(CloudClientErrorData::GenericError {
        message: error.to_string(),
    })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::CONFLICT.as_u16())
}

fn gax_error_is_permission_denied(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::PermissionDenied)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::FORBIDDEN.as_u16())
}

fn compute_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/compute/v1")
        .to_string()
}

fn resource_name(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "<unnamed>".to_string())
}
