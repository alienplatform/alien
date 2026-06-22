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

use google_cloud_compute_v1::model::{
    operation::Status as OperationStatus, Address, BackendService, Firewall, ForwardingRule,
    Network, NetworkEndpointGroup, Operation, Router, SslCertificate, Subnetwork, TargetHttpsProxy,
    UrlMap,
};

pub fn operation_is_done(operation: &Operation) -> bool {
    matches!(operation.status, Some(OperationStatus::Done))
}

pub fn operation_has_error(operation: &Operation) -> bool {
    operation
        .error
        .as_ref()
        .is_some_and(|error| !error.errors.is_empty())
}

pub(crate) async fn compute_client_from_alien_config<T, B>(
    config: &GcpClientConfig,
    builder: impl FnOnce() -> B,
) -> crate::error::Result<T>
where
    B: ComputeClientBuilder<T>,
{
    build_compute_client(config, builder)
        .await
        .map_err(|error| {
            AlienError::new(crate::error::ErrorData::CloudPlatformError {
                message: error.to_string(),
                resource_id: None,
            })
        })
}

pub(crate) async fn get_network(
    client: &Networks,
    project_id: &str,
    network_name: &str,
) -> CloudClientResult<Network> {
    client
        .get()
        .set_project(project_id)
        .set_network(network_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "network", network_name))
}

pub(crate) async fn insert_network(
    client: &Networks,
    project_id: &str,
    network: Network,
) -> CloudClientResult<Operation> {
    let name = resource_name(&network.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(network)
        .send()
        .await
        .map_err(|error| compute_error(error, "network", &name))
}

pub(crate) async fn delete_network(
    client: &Networks,
    project_id: &str,
    network_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_network(network_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "network", network_name))
}

pub(crate) async fn get_subnetwork(
    client: &Subnetworks,
    project_id: &str,
    region: &str,
    subnetwork_name: &str,
) -> CloudClientResult<Subnetwork> {
    client
        .get()
        .set_project(project_id)
        .set_region(region)
        .set_subnetwork(subnetwork_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "subnetwork", subnetwork_name))
}

pub(crate) async fn insert_subnetwork(
    client: &Subnetworks,
    project_id: &str,
    region: &str,
    subnetwork: Subnetwork,
) -> CloudClientResult<Operation> {
    let name = resource_name(&subnetwork.name);
    client
        .insert()
        .set_project(project_id)
        .set_region(region)
        .set_body(subnetwork)
        .send()
        .await
        .map_err(|error| compute_error(error, "subnetwork", &name))
}

pub(crate) async fn delete_subnetwork(
    client: &Subnetworks,
    project_id: &str,
    region: &str,
    subnetwork_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_region(region)
        .set_subnetwork(subnetwork_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "subnetwork", subnetwork_name))
}

pub(crate) async fn get_router(
    client: &Routers,
    project_id: &str,
    region: &str,
    router_name: &str,
) -> CloudClientResult<Router> {
    client
        .get()
        .set_project(project_id)
        .set_region(region)
        .set_router(router_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "router", router_name))
}

pub(crate) async fn insert_router(
    client: &Routers,
    project_id: &str,
    region: &str,
    router: Router,
) -> CloudClientResult<Operation> {
    let name = resource_name(&router.name);
    client
        .insert()
        .set_project(project_id)
        .set_region(region)
        .set_body(router)
        .send()
        .await
        .map_err(|error| compute_error(error, "router", &name))
}

pub(crate) async fn patch_router(
    client: &Routers,
    project_id: &str,
    region: &str,
    router_name: &str,
    router: Router,
) -> CloudClientResult<Operation> {
    client
        .patch()
        .set_project(project_id)
        .set_region(region)
        .set_router(router_name)
        .set_body(router)
        .send()
        .await
        .map_err(|error| compute_error(error, "router", router_name))
}

pub(crate) async fn delete_router(
    client: &Routers,
    project_id: &str,
    region: &str,
    router_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_region(region)
        .set_router(router_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "router", router_name))
}

pub(crate) async fn insert_firewall(
    client: &Firewalls,
    project_id: &str,
    firewall: Firewall,
) -> CloudClientResult<Operation> {
    let name = resource_name(&firewall.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(firewall)
        .send()
        .await
        .map_err(|error| compute_error(error, "firewall", &name))
}

pub(crate) async fn delete_firewall(
    client: &Firewalls,
    project_id: &str,
    firewall_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_firewall(firewall_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "firewall", firewall_name))
}

pub(crate) async fn get_global_operation(
    client: &GlobalOperations,
    project_id: &str,
    operation_name: &str,
) -> CloudClientResult<Operation> {
    client
        .get()
        .set_project(project_id)
        .set_operation(operation_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalOperation", operation_name))
}

pub(crate) async fn get_region_operation(
    client: &RegionOperations,
    project_id: &str,
    region: &str,
    operation_name: &str,
) -> CloudClientResult<Operation> {
    client
        .get()
        .set_project(project_id)
        .set_region(region)
        .set_operation(operation_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "regionOperation", operation_name))
}

pub(crate) async fn insert_backend_service(
    client: &BackendServices,
    project_id: &str,
    backend_service: BackendService,
) -> CloudClientResult<Operation> {
    let name = resource_name(&backend_service.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(backend_service)
        .send()
        .await
        .map_err(|error| compute_error(error, "backendService", &name))
}

pub(crate) async fn delete_backend_service(
    client: &BackendServices,
    project_id: &str,
    backend_service_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_backend_service(backend_service_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "backendService", backend_service_name))
}

pub(crate) async fn insert_url_map(
    client: &UrlMaps,
    project_id: &str,
    url_map: UrlMap,
) -> CloudClientResult<Operation> {
    let name = resource_name(&url_map.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(url_map)
        .send()
        .await
        .map_err(|error| compute_error(error, "urlMap", &name))
}

pub(crate) async fn delete_url_map(
    client: &UrlMaps,
    project_id: &str,
    url_map_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_url_map(url_map_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "urlMap", url_map_name))
}

pub(crate) async fn insert_target_https_proxy(
    client: &TargetHttpsProxies,
    project_id: &str,
    target_https_proxy: TargetHttpsProxy,
) -> CloudClientResult<Operation> {
    let name = resource_name(&target_https_proxy.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(target_https_proxy)
        .send()
        .await
        .map_err(|error| compute_error(error, "targetHttpsProxy", &name))
}

pub(crate) async fn set_target_https_proxy_ssl_certificates(
    client: &TargetHttpsProxies,
    project_id: &str,
    target_https_proxy_name: &str,
    ssl_certificates: Vec<String>,
) -> CloudClientResult<Operation> {
    let request =
        TargetHttpsProxiesSetSslCertificatesRequest::new().set_ssl_certificates(ssl_certificates);
    client
        .set_ssl_certificates()
        .set_project(project_id)
        .set_target_https_proxy(target_https_proxy_name)
        .set_body(request)
        .send()
        .await
        .map_err(|error| compute_error(error, "targetHttpsProxy", target_https_proxy_name))
}

pub(crate) async fn delete_target_https_proxy(
    client: &TargetHttpsProxies,
    project_id: &str,
    target_https_proxy_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_target_https_proxy(target_https_proxy_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "targetHttpsProxy", target_https_proxy_name))
}

pub(crate) async fn insert_ssl_certificate(
    client: &SslCertificates,
    project_id: &str,
    ssl_certificate: SslCertificate,
) -> CloudClientResult<Operation> {
    let name = resource_name(&ssl_certificate.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(ssl_certificate)
        .send()
        .await
        .map_err(|error| compute_error(error, "sslCertificate", &name))
}

pub(crate) async fn delete_ssl_certificate(
    client: &SslCertificates,
    project_id: &str,
    ssl_certificate_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_ssl_certificate(ssl_certificate_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "sslCertificate", ssl_certificate_name))
}

pub(crate) async fn get_global_address(
    client: &GlobalAddresses,
    project_id: &str,
    address_name: &str,
) -> CloudClientResult<Address> {
    client
        .get()
        .set_project(project_id)
        .set_address(address_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalAddress", address_name))
}

pub(crate) async fn insert_global_address(
    client: &GlobalAddresses,
    project_id: &str,
    address: Address,
) -> CloudClientResult<Operation> {
    let name = resource_name(&address.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(address)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalAddress", &name))
}

pub(crate) async fn delete_global_address(
    client: &GlobalAddresses,
    project_id: &str,
    address_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_address(address_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalAddress", address_name))
}

pub(crate) async fn insert_global_forwarding_rule(
    client: &GlobalForwardingRules,
    project_id: &str,
    forwarding_rule: ForwardingRule,
) -> CloudClientResult<Operation> {
    let name = resource_name(&forwarding_rule.name);
    client
        .insert()
        .set_project(project_id)
        .set_body(forwarding_rule)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalForwardingRule", &name))
}

pub(crate) async fn delete_global_forwarding_rule(
    client: &GlobalForwardingRules,
    project_id: &str,
    forwarding_rule_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_forwarding_rule(forwarding_rule_name)
        .send()
        .await
        .map_err(|error| compute_error(error, "globalForwardingRule", forwarding_rule_name))
}

pub(crate) async fn insert_region_network_endpoint_group(
    client: &RegionNetworkEndpointGroups,
    project_id: &str,
    region: &str,
    network_endpoint_group: NetworkEndpointGroup,
) -> CloudClientResult<Operation> {
    let name = resource_name(&network_endpoint_group.name);
    client
        .insert()
        .set_project(project_id)
        .set_region(region)
        .set_body(network_endpoint_group)
        .send()
        .await
        .map_err(|error| compute_error(error, "regionNetworkEndpointGroup", &name))
}

pub(crate) async fn delete_region_network_endpoint_group(
    client: &RegionNetworkEndpointGroups,
    project_id: &str,
    region: &str,
    network_endpoint_group_name: &str,
) -> CloudClientResult<Operation> {
    client
        .delete()
        .set_project(project_id)
        .set_region(region)
        .set_network_endpoint_group(network_endpoint_group_name)
        .send()
        .await
        .map_err(|error| {
            compute_error(
                error,
                "regionNetworkEndpointGroup",
                network_endpoint_group_name,
            )
        })
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
pub(crate) trait ComputeClientBuilder<T>: Sized {
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
