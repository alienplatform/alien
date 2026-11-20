use alien_extension_sdk::prelude::*;

/// A DNS zone the deployment owns.
#[alien_resource(type = "acme/dns-zone", outputs = DnsZoneOutputs, lifecycles = [Frozen, Live], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema, bon::Builder)]
#[serde(rename_all = "camelCase")]
#[builder(start_fn = new)]
pub struct DnsZone {
    /// Lowercase letters, digits and dashes, at most 22 characters, ending in a letter or digit.
    #[builder(start_fn)]
    pub id: String,
    /// The zone's domain, for example "internal.acme.dev".
    pub domain: String,
}

/// What a linked workload sees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DnsZoneOutputs {
    pub zone_id: String,
}

/// What the AWS template returns after initial setup.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsDnsZoneImport {
    pub zone_id: String,
}

/// The fields the AWS controller saves. The importer fills them after the admin applies the template.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsDnsZoneFields {
    pub zone_id: Option<String>,
}

/// What the Google Cloud template returns after initial setup.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpDnsZoneImport {
    pub zone_name: String,
}

/// The fields the Google Cloud controller saves.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpDnsZoneFields {
    pub zone_name: Option<String>,
}

/// What the Azure template returns after initial setup.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureDnsZoneImport {
    pub zone_id: String,
}

/// The fields the Azure controller saves.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureDnsZoneFields {
    pub zone_id: Option<String>,
}
