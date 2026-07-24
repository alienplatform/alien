use serde::{Deserialize, Serialize};

/// Azure AI (AIServices) ImportData.
///
/// Carries the account name, endpoint, resource group, and location from an
/// externally-created AIServices account directly into the controller's ready
/// state so that heartbeat ticks and binding-param serialization work without
/// a cloud round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureAiImportData {
    /// Name of the Azure CognitiveServices / AIServices account.
    pub account_name: String,
    /// The endpoint URL of the AIServices account.
    pub endpoint: String,
    /// Azure resource group containing the account.
    pub resource_group: String,
    /// Azure region where the account lives (e.g. "eastus").
    pub location: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Azure AI Terraform emitter writes the import ref with camelCase keys
    /// (`crates/alien-terraform/src/emitters/azure/ai.rs`); it also emits an extra
    /// `subscriptionId` this struct does not read. This pins the four consumed keys so a
    /// future rename can't silently break the frozen/Terraform import round-trip.
    #[test]
    fn deserializes_the_emitter_import_ref_keys() {
        let json = serde_json::json!({
            "subscriptionId": "sub-ignored",
            "resourceGroup": "my-rg",
            "accountName": "my-account",
            "endpoint": "https://my-account.openai.azure.com/",
            "location": "eastus",
        });
        let data: AzureAiImportData =
            serde_json::from_value(json).expect("the emitter's import-ref keys must deserialize");
        assert_eq!(data.account_name, "my-account");
        assert_eq!(data.endpoint, "https://my-account.openai.azure.com/");
        assert_eq!(data.resource_group, "my-rg");
        assert_eq!(data.location, "eastus");
    }
}
