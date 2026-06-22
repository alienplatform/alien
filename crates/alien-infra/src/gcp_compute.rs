use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use google_cloud_compute_v1::client::{
    BackendServices, Firewalls, GlobalAddresses, GlobalForwardingRules, GlobalOperations, Networks,
    RegionNetworkEndpointGroups, RegionOperations, Routers, SslCertificates, Subnetworks,
    TargetHttpsProxies, UrlMaps,
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use http::StatusCode;

use google_cloud_compute_v1::model::{operation::Status as OperationStatus, Operation};

pub(crate) fn operation_is_done(operation: &Operation) -> bool {
    matches!(operation.status, Some(OperationStatus::Done))
}

pub(crate) fn operation_has_error(operation: &Operation) -> bool {
    operation
        .error
        .as_ref()
        .is_some_and(|error| !error.errors.is_empty())
}

macro_rules! official_compute_client_constructor {
    ($fn_name:ident, $client:path, $builder:expr, $display_name:literal) => {
        pub(crate) async fn $fn_name(config: &GcpClientConfig) -> crate::error::Result<$client> {
            let credentials =
                crate::core::gcp_credentials_from_alien_config(config).map_err(|error| {
                    AlienError::new(crate::error::ErrorData::CloudPlatformError {
                        message: error.to_string(),
                        resource_id: None,
                    })
                })?;
            let mut builder = $builder().with_credentials(credentials);

            if let Some(endpoint) = config
                .service_overrides
                .as_ref()
                .and_then(|overrides| overrides.endpoints.get("compute"))
            {
                builder = builder.with_endpoint(compute_endpoint(endpoint));
            }

            builder.build().await.map_err(|error| {
                AlienError::new(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to build official GCP Compute {} client: {error}",
                        $display_name
                    ),
                    resource_id: None,
                })
            })
        }
    };
}

official_compute_client_constructor!(
    networks_client_from_alien_config,
    Networks,
    Networks::builder,
    "Networks"
);
official_compute_client_constructor!(
    subnetworks_client_from_alien_config,
    Subnetworks,
    Subnetworks::builder,
    "Subnetworks"
);
official_compute_client_constructor!(
    routers_client_from_alien_config,
    Routers,
    Routers::builder,
    "Routers"
);
official_compute_client_constructor!(
    firewalls_client_from_alien_config,
    Firewalls,
    Firewalls::builder,
    "Firewalls"
);
official_compute_client_constructor!(
    global_operations_client_from_alien_config,
    GlobalOperations,
    GlobalOperations::builder,
    "GlobalOperations"
);
official_compute_client_constructor!(
    region_operations_client_from_alien_config,
    RegionOperations,
    RegionOperations::builder,
    "RegionOperations"
);
official_compute_client_constructor!(
    backend_services_client_from_alien_config,
    BackendServices,
    BackendServices::builder,
    "BackendServices"
);
official_compute_client_constructor!(
    url_maps_client_from_alien_config,
    UrlMaps,
    UrlMaps::builder,
    "UrlMaps"
);
official_compute_client_constructor!(
    target_https_proxies_client_from_alien_config,
    TargetHttpsProxies,
    TargetHttpsProxies::builder,
    "TargetHttpsProxies"
);
official_compute_client_constructor!(
    ssl_certificates_client_from_alien_config,
    SslCertificates,
    SslCertificates::builder,
    "SslCertificates"
);
official_compute_client_constructor!(
    global_addresses_client_from_alien_config,
    GlobalAddresses,
    GlobalAddresses::builder,
    "GlobalAddresses"
);
official_compute_client_constructor!(
    global_forwarding_rules_client_from_alien_config,
    GlobalForwardingRules,
    GlobalForwardingRules::builder,
    "GlobalForwardingRules"
);
official_compute_client_constructor!(
    region_network_endpoint_groups_client_from_alien_config,
    RegionNetworkEndpointGroups,
    RegionNetworkEndpointGroups::builder,
    "RegionNetworkEndpointGroups"
);

pub(crate) fn compute_error(
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
