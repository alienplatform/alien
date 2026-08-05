//! Read-only Bedrock model availability client.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{RequestBuilderExt, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BedrockAvailabilityStatus {
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationModelAvailability {
    pub agreement_availability: Option<BedrockAvailabilityStatus>,
    pub authorization_status: Option<String>,
    pub entitlement_availability: Option<String>,
    pub region_availability: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait BedrockApi: Send + Sync + Debug {
    /// Inspect availability without invoking the model or accepting an agreement.
    async fn get_foundation_model_availability(
        &self,
        model_id: &str,
    ) -> Result<FoundationModelAvailability>;
}

#[derive(Debug, Clone)]
pub struct BedrockClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl BedrockClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "bedrock".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn base_url(&self) -> String {
        self.credentials
            .get_service_endpoint_option("bedrock")
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                format!(
                    "https://bedrock.{}.amazonaws.com",
                    self.credentials.region()
                )
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl BedrockApi for BedrockClient {
    async fn get_foundation_model_availability(
        &self,
        model_id: &str,
    ) -> Result<FoundationModelAvailability> {
        self.credentials.ensure_fresh().await?;
        let url = format!(
            "{}/foundation-model-availability/{}",
            self.base_url().trim_end_matches('/'),
            urlencoding::encode(model_id)
        );
        let builder = self.client.get(url).host(&format!(
            "bedrock.{}.amazonaws.com",
            self.credentials.region()
        ));
        builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_json()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_availability_response() {
        let parsed: FoundationModelAvailability = serde_json::from_str(
            r#"{
                "agreementAvailability":{"status":"AVAILABLE"},
                "authorizationStatus":"AUTHORIZED",
                "entitlementAvailability":"AVAILABLE",
                "regionAvailability":"AVAILABLE"
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.authorization_status.as_deref(), Some("AUTHORIZED"));
        assert_eq!(
            parsed.agreement_availability.and_then(|value| value.status),
            Some("AVAILABLE".to_string())
        );
        assert_eq!(
            parsed.entitlement_availability.as_deref(),
            Some("AVAILABLE")
        );
    }
}
