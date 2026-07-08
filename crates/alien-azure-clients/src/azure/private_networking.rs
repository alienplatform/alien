//! Azure Private Endpoint + Private DNS client (the private path to a Postgres
//! Flexible Server). ARM REST/JSON, mirroring the other Azure clients.
//!
//! The Flexible Server has public network access disabled, so the controller
//! reaches it through a Private Endpoint placed in the VNet, a Private DNS Zone
//! (`privatelink.postgres.database.azure.com`) linked to that VNet, and a DNS
//! zone group that auto-registers the endpoint's record in the zone.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// The Private DNS zone Postgres Flexible Server private endpoints resolve through.
pub const POSTGRES_PRIVATE_DNS_ZONE: &str = "privatelink.postgres.database.azure.com";

/// The `groupIds` value that selects the Postgres Flexible Server sub-resource
/// when wiring a Private Endpoint to it.
pub const POSTGRES_PRIVATE_LINK_GROUP_ID: &str = "postgresqlServer";

// ─────────────────────────── Private Endpoint models ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEndpoint {
    pub location: String,
    pub properties: PrivateEndpointProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEndpointProperties {
    pub subnet: SubnetReference,
    pub private_link_service_connections: Vec<PrivateLinkServiceConnection>,
    /// Response-only: the private IP/FQDN entries Azure assigns once the endpoint
    /// is provisioned. The controller reads these to learn the private address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_dns_configs: Option<Vec<CustomDnsConfig>>,
    /// Lifecycle state (response only): Succeeded / Provisioning / ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
}

/// A reference to an existing ARM resource by its full resource id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubnetReference {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateLinkServiceConnection {
    pub name: String,
    pub properties: PrivateLinkServiceConnectionProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateLinkServiceConnectionProperties {
    /// Resource id of the target — for Postgres, the Flexible Server's id.
    pub private_link_service_id: String,
    /// Sub-resource selector — `["postgresqlServer"]` for Postgres Flexible Server.
    pub group_ids: Vec<String>,
}

/// Response-only: the FQDN and private IP addresses Azure assigns to the endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomDnsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
    #[serde(default)]
    pub ip_addresses: Vec<String>,
}

// ─────────────────────────── Private DNS Zone models ───────────────────────────

/// A Private DNS Zone is a global resource: `location = "global"` and empty properties.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZone {
    pub location: String,
    #[serde(default)]
    pub properties: PrivateDnsZoneProperties,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZoneProperties {}

impl PrivateDnsZone {
    /// Builds the global-scoped zone body Azure expects.
    pub fn global() -> Self {
        Self {
            location: "global".to_string(),
            properties: PrivateDnsZoneProperties::default(),
        }
    }
}

// ─────────────────────────── Virtual Network Link models ───────────────────────────

/// A link binding a Private DNS Zone to a VNet (child of the zone). Global resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkLink {
    pub location: String,
    pub properties: VirtualNetworkLinkProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkLinkProperties {
    pub virtual_network: VirtualNetworkReference,
    /// Auto-registration of VM records — kept `false`; the endpoint registers via
    /// its DNS zone group, not auto-registration.
    pub registration_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkReference {
    pub id: String,
}

impl VirtualNetworkLink {
    /// Builds the global-scoped link body for the given VNet.
    pub fn new(vnet_id: impl Into<String>, registration_enabled: bool) -> Self {
        Self {
            location: "global".to_string(),
            properties: VirtualNetworkLinkProperties {
                virtual_network: VirtualNetworkReference { id: vnet_id.into() },
                registration_enabled,
            },
        }
    }
}

// ─────────────────────────── Private DNS Zone Group models ───────────────────────────

/// A DNS zone group on a Private Endpoint (child of the PE). Wiring this is what
/// auto-registers the endpoint's address as an A record in the zone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZoneGroup {
    pub properties: PrivateDnsZoneGroupProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZoneGroupProperties {
    pub private_dns_zone_configs: Vec<PrivateDnsZoneConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZoneConfig {
    pub name: String,
    pub properties: PrivateDnsZoneConfigProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrivateDnsZoneConfigProperties {
    /// Resource id of the Private DNS Zone the endpoint's record is written into.
    pub private_dns_zone_id: String,
}

impl PrivateDnsZoneGroup {
    /// Builds a single-zone group pointing at the given Private DNS Zone id.
    pub fn single(config_name: impl Into<String>, private_dns_zone_id: impl Into<String>) -> Self {
        Self {
            properties: PrivateDnsZoneGroupProperties {
                private_dns_zone_configs: vec![PrivateDnsZoneConfig {
                    name: config_name.into(),
                    properties: PrivateDnsZoneConfigProperties {
                        private_dns_zone_id: private_dns_zone_id.into(),
                    },
                }],
            },
        }
    }
}

/// Result of a Private Endpoint create or update operation.
pub type PrivateEndpointOperationResult = OperationResult<PrivateEndpoint>;

// ─────────────────────────── trait + client ───────────────────────────

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait PrivateNetworkingApi: Send + Sync + std::fmt::Debug {
    // ─── Private Endpoint ───
    async fn create_or_update_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
        private_endpoint: &PrivateEndpoint,
    ) -> Result<PrivateEndpointOperationResult>;

    async fn get_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
    ) -> Result<PrivateEndpoint>;

    async fn delete_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
    ) -> Result<OperationResult<()>>;

    // ─── Private DNS Zone (global resource) ───
    async fn create_or_update_private_dns_zone(
        &self,
        resource_group: &str,
        zone_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn delete_private_dns_zone(
        &self,
        resource_group: &str,
        zone_name: &str,
    ) -> Result<OperationResult<()>>;

    // ─── Virtual Network Link (child of the zone) ───
    async fn create_or_update_vnet_link(
        &self,
        resource_group: &str,
        zone_name: &str,
        link_name: &str,
        vnet_id: &str,
        registration_enabled: bool,
    ) -> Result<OperationResult<()>>;

    /// Deletes a zone's Virtual Network Link. Azure refuses to delete a private DNS zone that
    /// still has links, so zone teardown removes the link first.
    async fn delete_vnet_link(
        &self,
        resource_group: &str,
        zone_name: &str,
        link_name: &str,
    ) -> Result<OperationResult<()>>;

    // ─── Private DNS Zone Group (child of the PE) ───
    async fn create_or_update_dns_zone_group(
        &self,
        resource_group: &str,
        private_endpoint_name: &str,
        group_name: &str,
        private_dns_zone_id: &str,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzurePrivateNetworkingClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzurePrivateNetworkingClient {
    /// Private Endpoint + DNS-zone-group operations share the Network RP api-version.
    const NETWORK_API_VERSION: &'static str = "2024-05-01";
    /// Private DNS Zone, virtual network links, and zone configs use the DNS RP api-version.
    const PRIVATE_DNS_API_VERSION: &'static str = "2018-09-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        let endpoint = token_cache.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    fn subscription_id(&self) -> &str {
        &self.token_cache.config().subscription_id
    }

    fn private_endpoint_path(&self, resource_group: &str, name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/privateEndpoints/{}",
            self.subscription_id(),
            resource_group,
            name
        )
    }

    fn private_dns_zone_path(&self, resource_group: &str, zone_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/privateDnsZones/{}",
            self.subscription_id(),
            resource_group,
            zone_name
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl PrivateNetworkingApi for AzurePrivateNetworkingClient {
    // ─── Private Endpoint ───

    async fn create_or_update_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
        private_endpoint: &PrivateEndpoint,
    ) -> Result<PrivateEndpointOperationResult> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.private_endpoint_path(resource_group, name),
            Some(vec![("api-version", Self::NETWORK_API_VERSION.into())]),
        );
        let body = serde_json::to_string(private_endpoint)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize Private Endpoint: {name}"),
            })?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdatePrivateEndpoint",
                name,
            )
            .await
    }

    async fn get_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
    ) -> Result<PrivateEndpoint> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.private_endpoint_path(resource_group, name),
            Some(vec![("api-version", Self::NETWORK_API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::GET, url.clone())
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetPrivateEndpoint", name)
            .await?;
        // The response already succeeded (execute_request returns Ok only on 2xx), so a body-read /
        // parse failure is a serialization problem, not an HTTP failure — don't fabricate a 502.
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure GetPrivateEndpoint: failed to read body for {name}"),
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure GetPrivateEndpoint: JSON parse error for {name}"),
            })
    }

    async fn delete_private_endpoint(
        &self,
        resource_group: &str,
        name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.private_endpoint_path(resource_group, name),
            Some(vec![("api-version", Self::NETWORK_API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::DELETE, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "DeletePrivateEndpoint", name)
            .await
    }

    // ─── Private DNS Zone ───

    async fn create_or_update_private_dns_zone(
        &self,
        resource_group: &str,
        zone_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.private_dns_zone_path(resource_group, zone_name),
            Some(vec![("api-version", Self::PRIVATE_DNS_API_VERSION.into())]),
        );
        let zone = PrivateDnsZone::global();
        let body = serde_json::to_string(&zone).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Private DNS Zone: {zone_name}"),
            },
        )?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdatePrivateDnsZone",
                zone_name,
            )
            .await
    }

    async fn delete_private_dns_zone(
        &self,
        resource_group: &str,
        zone_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.private_dns_zone_path(resource_group, zone_name),
            Some(vec![("api-version", Self::PRIVATE_DNS_API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::DELETE, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "DeletePrivateDnsZone", zone_name)
            .await
    }

    // ─── Virtual Network Link ───

    async fn create_or_update_vnet_link(
        &self,
        resource_group: &str,
        zone_name: &str,
        link_name: &str,
        vnet_id: &str,
        registration_enabled: bool,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/virtualNetworkLinks/{}",
                self.private_dns_zone_path(resource_group, zone_name),
                link_name
            ),
            Some(vec![("api-version", Self::PRIVATE_DNS_API_VERSION.into())]),
        );
        let link = VirtualNetworkLink::new(vnet_id, registration_enabled);
        let body = serde_json::to_string(&link).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Virtual Network Link: {link_name}"),
            },
        )?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateVirtualNetworkLink",
                link_name,
            )
            .await
    }

    async fn delete_vnet_link(
        &self,
        resource_group: &str,
        zone_name: &str,
        link_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/virtualNetworkLinks/{}",
                self.private_dns_zone_path(resource_group, zone_name),
                link_name
            ),
            Some(vec![("api-version", Self::PRIVATE_DNS_API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::DELETE, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "DeleteVirtualNetworkLink", link_name)
            .await
    }

    // ─── Private DNS Zone Group ───

    async fn create_or_update_dns_zone_group(
        &self,
        resource_group: &str,
        private_endpoint_name: &str,
        group_name: &str,
        private_dns_zone_id: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/privateDnsZoneGroups/{}",
                self.private_endpoint_path(resource_group, private_endpoint_name),
                group_name
            ),
            Some(vec![("api-version", Self::NETWORK_API_VERSION.into())]),
        );
        let zone_group = PrivateDnsZoneGroup::single(group_name, private_dns_zone_id);
        let body = serde_json::to_string(&zone_group)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize Private DNS Zone Group: {group_name}"),
            })?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdatePrivateDnsZoneGroup",
                group_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn postgres_private_endpoint() -> PrivateEndpoint {
        PrivateEndpoint {
            location: "eastus".into(),
            properties: PrivateEndpointProperties {
                subnet: SubnetReference {
                    id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/pe".into(),
                },
                private_link_service_connections: vec![PrivateLinkServiceConnection {
                    name: "pg-connection".into(),
                    properties: PrivateLinkServiceConnectionProperties {
                        private_link_service_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/pg".into(),
                        group_ids: vec![POSTGRES_PRIVATE_LINK_GROUP_ID.into()],
                    },
                }],
                custom_dns_configs: None,
                provisioning_state: None,
            },
        }
    }

    #[test]
    fn private_endpoint_serializes_subnet_and_postgres_group() {
        let json = serde_json::to_value(postgres_private_endpoint()).unwrap();
        assert_eq!(
            json["properties"]["subnet"]["id"],
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/pe"
        );
        let connection = &json["properties"]["privateLinkServiceConnections"][0];
        assert_eq!(connection["name"], "pg-connection");
        assert_eq!(
            connection["properties"]["privateLinkServiceId"],
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/pg"
        );
        assert_eq!(connection["properties"]["groupIds"][0], "postgresqlServer");
        assert_eq!(
            connection["properties"]["groupIds"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // Request body must not carry response-only fields.
        assert!(json["properties"].get("customDnsConfigs").is_none());
        assert!(json["properties"].get("provisioningState").is_none());
    }

    #[test]
    fn private_endpoint_deserializes_assigned_ip_and_fqdn() {
        let body = r#"{"location":"eastus","properties":{
            "subnet":{"id":"/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/pe"},
            "privateLinkServiceConnections":[{"name":"pg-connection","properties":{
                "privateLinkServiceId":"/subscriptions/sub/.../flexibleServers/pg",
                "groupIds":["postgresqlServer"]}}],
            "customDnsConfigs":[{"fqdn":"pg.privatelink.postgres.database.azure.com","ipAddresses":["10.0.1.4"]}],
            "provisioningState":"Succeeded"}}"#;
        let endpoint: PrivateEndpoint = serde_json::from_str(body).unwrap();
        assert_eq!(
            endpoint.properties.provisioning_state.as_deref(),
            Some("Succeeded")
        );
        let dns_configs = endpoint
            .properties
            .custom_dns_configs
            .expect("customDnsConfigs should be present");
        assert_eq!(dns_configs.len(), 1);
        assert_eq!(
            dns_configs[0].fqdn.as_deref(),
            Some("pg.privatelink.postgres.database.azure.com")
        );
        assert_eq!(dns_configs[0].ip_addresses, vec!["10.0.1.4".to_string()]);
    }

    #[test]
    fn private_dns_zone_serializes_as_global() {
        let json = serde_json::to_value(PrivateDnsZone::global()).unwrap();
        assert_eq!(json["location"], "global");
        // Properties is an empty object, not absent.
        assert!(json["properties"].is_object());
        assert_eq!(json["properties"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn vnet_link_serializes_global_with_registration_disabled() {
        let json = serde_json::to_value(VirtualNetworkLink::new(
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet",
            false,
        ))
        .unwrap();
        assert_eq!(json["location"], "global");
        assert_eq!(
            json["properties"]["virtualNetwork"]["id"],
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet"
        );
        assert_eq!(json["properties"]["registrationEnabled"], false);
    }

    #[test]
    fn dns_zone_group_nests_private_dns_zone_id() {
        let zone_id = "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/privateDnsZones/privatelink.postgres.database.azure.com";
        let json = serde_json::to_value(PrivateDnsZoneGroup::single("pg-config", zone_id)).unwrap();
        let config = &json["properties"]["privateDnsZoneConfigs"][0];
        assert_eq!(config["name"], "pg-config");
        assert_eq!(config["properties"]["privateDnsZoneId"], zone_id);
        assert_eq!(
            json["properties"]["privateDnsZoneConfigs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
