use crate::azure::{
    common::{AzureClientBase, AzureRequestBuilder},
    token_cache::AzureTokenCache,
};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// One Microsoft.Compute SKU returned for the current subscription.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSku {
    /// Provider SKU name, for example `Standard_D4s_v5`.
    pub name: Option<String>,
    /// Compute resource type to which the SKU applies.
    pub resource_type: Option<String>,
    /// Locations and availability zones in which the SKU is offered.
    #[serde(default)]
    pub location_info: Vec<ResourceSkuLocationInfo>,
    /// Subscription- or capacity-specific restrictions on the SKU.
    #[serde(default)]
    pub restrictions: Vec<ResourceSkuRestriction>,
}

impl ResourceSku {
    /// Returns the zones in which this subscription can use the SKU in `location`.
    ///
    /// Azure reports offered zones in `locationInfo` and separately reports
    /// subscription/capacity restrictions. Location restrictions make the SKU
    /// unavailable; zone restrictions are subtracted from the offered set.
    pub fn available_zones_in(&self, location: &str) -> Vec<String> {
        let location_is_restricted = self.restrictions.iter().any(|restriction| {
            restriction
                .restriction_type
                .eq_ignore_ascii_case("Location")
                && restriction.applies_to_location(location)
        });
        if location_is_restricted {
            return Vec::new();
        }

        let mut zones = self
            .location_info
            .iter()
            .filter(|info| info.location.eq_ignore_ascii_case(location))
            .flat_map(|info| info.zones.iter().cloned())
            .collect::<Vec<_>>();
        zones.sort();
        zones.dedup();

        zones.retain(|zone| {
            !self.restrictions.iter().any(|restriction| {
                restriction.restriction_type.eq_ignore_ascii_case("Zone")
                    && restriction.applies_to_location(location)
                    && restriction
                        .restriction_info
                        .as_ref()
                        .is_some_and(|info| info.zones.iter().any(|value| value == zone))
            })
        });
        zones
    }
}

/// Location-specific availability information for a compute SKU.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSkuLocationInfo {
    /// Azure location name.
    pub location: String,
    /// Logical availability zone names offered in this location.
    #[serde(default)]
    pub zones: Vec<String>,
}

/// A restriction that prevents use of a compute SKU.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSkuRestriction {
    /// Restriction type (`Location` or `Zone`).
    #[serde(rename = "type")]
    pub restriction_type: String,
    /// Restricted locations for location restrictions.
    #[serde(default)]
    pub values: Vec<String>,
    /// Structured location and zone restriction details.
    pub restriction_info: Option<ResourceSkuRestrictionInfo>,
    /// Azure reason code such as `QuotaId` or `NotAvailableForSubscription`.
    pub reason_code: Option<String>,
}

impl ResourceSkuRestriction {
    fn applies_to_location(&self, location: &str) -> bool {
        let structured_locations = self
            .restriction_info
            .as_ref()
            .map(|info| info.locations.as_slice())
            .unwrap_or_default();
        let locations = if structured_locations.is_empty() {
            self.values.as_slice()
        } else {
            structured_locations
        };
        locations.is_empty()
            || locations
                .iter()
                .any(|value| value.eq_ignore_ascii_case(location))
    }
}

/// Structured scope for a compute SKU restriction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSkuRestrictionInfo {
    /// Locations to which the restriction applies.
    #[serde(default)]
    pub locations: Vec<String>,
    /// Availability zones to which the restriction applies.
    #[serde(default)]
    pub zones: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSkusPage {
    #[serde(default)]
    value: Vec<ResourceSku>,
    next_link: Option<String>,
}

/// Read-only Microsoft.Compute Resource SKUs operations.
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourceSkusApi: Send + Sync + std::fmt::Debug {
    /// Lists all SKU pages returned for an Azure location.
    async fn list_resource_skus(&self, location: &str) -> Result<Vec<ResourceSku>>;

    /// Finds an exact virtual-machine SKU in a location.
    async fn get_virtual_machine_sku(
        &self,
        location: &str,
        sku_name: &str,
    ) -> Result<Option<ResourceSku>> {
        Ok(self
            .list_resource_skus(location)
            .await?
            .into_iter()
            .find(|sku| {
                sku.resource_type
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("virtualMachines"))
                    && sku
                        .name
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(sku_name))
            }))
    }
}

/// Client for subscription-aware Microsoft.Compute SKU discovery.
#[derive(Debug)]
pub struct AzureResourceSkusClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
}

impl AzureResourceSkusClient {
    const API_VERSION: &'static str = "2021-07-01";

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
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ResourceSkusApi for AzureResourceSkusClient {
    async fn list_resource_skus(&self, location: &str) -> Result<Vec<ResourceSku>> {
        let escaped_location = location.replace('\'', "''");
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let mut next_url = Some(self.base.build_url(
            &format!(
                "/subscriptions/{}/providers/Microsoft.Compute/skus",
                self.token_cache.config().subscription_id
            ),
            Some(vec![
                ("api-version", Self::API_VERSION.to_string()),
                ("$filter", format!("location eq '{escaped_location}'")),
            ]),
        ));
        let mut skus = Vec::new();

        while let Some(url) = next_url {
            let request = AzureRequestBuilder::new(Method::GET, url.clone())
                .content_length("")
                .build()?;
            let signed = self.base.sign_request(request, &bearer_token).await?;
            let response = self
                .base
                .execute_request(signed, "ListResourceSkus", location)
                .await?;
            let status = response.status().as_u16();
            let body =
                response
                    .text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: format!(
                            "Azure ListResourceSkus: failed to read response body for {location}"
                        ),
                        url: url.clone(),
                        http_status: status,
                        http_response_text: None,
                        http_request_text: None,
                    })?;
            let page: ResourceSkusPage = serde_json::from_str(&body).into_alien_error().context(
                ErrorData::HttpResponseError {
                    message: format!("Azure ListResourceSkus: JSON parse error for {location}"),
                    url,
                    http_status: status,
                    http_response_text: Some(body),
                    http_request_text: None,
                },
            )?;
            skus.extend(page.value);
            next_url = page.next_link;
        }

        Ok(skus)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use httpmock::{Method::GET, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    const SUBSCRIPTION_ID: &str = "12345678-1234-1234-1234-123456789012";

    fn test_client(server: &MockServer) -> AzureResourceSkusClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureResourceSkusClient::new(Client::new(), AzureTokenCache::new(config))
    }

    #[tokio::test]
    async fn lists_real_response_shape_across_every_page_and_finds_vm_sku() {
        let server = MockServer::start_async().await;
        let next_link = format!("{}/sku-pages/2?continuation=next", server.base_url());
        let first = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!(
                        "/subscriptions/{SUBSCRIPTION_ID}/providers/Microsoft.Compute/skus"
                    ))
                    .query_param("api-version", "2021-07-01")
                    .query_param("$filter", "location eq 'eastus'");
                then.status(200).json_body(json!({
                    "value": [{
                        "resourceType": "disks",
                        "name": "Premium_LRS",
                        "locationInfo": [{"location": "eastus", "zones": ["1", "2", "3"]}],
                        "restrictions": []
                    }],
                    "nextLink": next_link
                }));
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/sku-pages/2")
                    .query_param("continuation", "next");
                then.status(200).json_body(json!({
                    "value": [{
                        "resourceType": "virtualMachines",
                        "name": "Standard_D4s_v5",
                        "locations": ["eastus"],
                        "locationInfo": [{
                            "location": "eastus",
                            "zones": ["1", "2", "3"],
                            "zoneDetails": [{"name": ["1"], "capabilities": []}]
                        }],
                        "restrictions": [{
                            "type": "Zone",
                            "reasonCode": "NotAvailableForSubscription",
                            "restrictionInfo": {"locations": ["eastus"], "zones": ["2"]},
                            "values": ["eastus"]
                        }],
                        "capabilities": [{"name": "vCPUs", "value": "4"}]
                    }],
                    "nextLink": null
                }));
            })
            .await;

        let sku = test_client(&server)
            .get_virtual_machine_sku("eastus", "Standard_D4s_v5")
            .await
            .expect("SKU pages should be readable")
            .expect("VM SKU should be present");

        first.assert_async().await;
        second.assert_async().await;
        assert_eq!(sku.available_zones_in("eastus"), ["1", "3"]);
    }

    #[test]
    fn location_restriction_makes_sku_unavailable() {
        let sku: ResourceSku = serde_json::from_value(json!({
            "resourceType": "virtualMachines",
            "name": "Standard_D4s_v5",
            "locationInfo": [{"location": "eastus", "zones": ["1", "2", "3"]}],
            "restrictions": [{
                "type": "Location",
                "reasonCode": "NotAvailableForSubscription",
                "values": ["eastus"]
            }]
        }))
        .expect("real Azure response shape should deserialize");

        assert!(sku.available_zones_in("eastus").is_empty());
    }
}
