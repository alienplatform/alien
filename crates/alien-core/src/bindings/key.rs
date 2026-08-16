use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Provider key reference used for native encrypt and decrypt operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service")]
pub enum KeyBinding {
    /// AWS Key Management Service.
    #[serde(rename = "kms")]
    AwsKms(AwsKmsKeyBinding),
    /// GCP Cloud Key Management Service.
    #[serde(rename = "cloud-kms")]
    GcpCloudKms(GcpCloudKmsKeyBinding),
    /// Azure Key Vault Keys.
    #[serde(rename = "key-vault-key")]
    AzureKeyVault(AzureKeyVaultKeyBinding),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsKmsKeyBinding {
    pub key_arn: BindingValue<String>,
    pub region: Option<BindingValue<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudKmsKeyBinding {
    pub crypto_key_name: BindingValue<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureKeyVaultKeyBinding {
    pub key_id: BindingValue<String>,
}

impl KeyBinding {
    pub fn aws_kms(key_arn: impl Into<String>, region: Option<impl Into<String>>) -> Self {
        Self::AwsKms(AwsKmsKeyBinding {
            key_arn: key_arn.into().into(),
            region: region.map(|value| value.into().into()),
        })
    }

    pub fn gcp_cloud_kms(crypto_key_name: impl Into<String>) -> Self {
        Self::GcpCloudKms(GcpCloudKmsKeyBinding {
            crypto_key_name: crypto_key_name.into().into(),
        })
    }

    pub fn azure_key_vault(key_id: impl Into<String>) -> Self {
        Self::AzureKeyVault(AzureKeyVaultKeyBinding {
            key_id: key_id.into().into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_tags_round_trip_without_ambiguity() {
        let bindings = [
            KeyBinding::aws_kms("arn:aws:kms:us-east-1:123:key/abc", Some("us-east-1")),
            KeyBinding::gcp_cloud_kms(
                "projects/example/locations/us/keyRings/data/cryptoKeys/customer",
            ),
            KeyBinding::azure_key_vault("https://example.vault.azure.net/keys/customer/version"),
        ];

        for binding in bindings {
            let json = serde_json::to_value(&binding).unwrap();
            assert_eq!(serde_json::from_value::<KeyBinding>(json).unwrap(), binding);
        }
    }
}
