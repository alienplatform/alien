//! Read-only AWS Service Quotas client.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{RequestBuilderExt, Result};
use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServiceQuota {
    pub quota_code: Option<String>,
    pub service_code: Option<String>,
    pub quota_name: Option<String>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetServiceQuotaResponse {
    pub quota: Option<ServiceQuota>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ServiceQuotasApi: Send + Sync + Debug {
    async fn get_service_quota(
        &self,
        service_code: &str,
        quota_code: &str,
    ) -> Result<GetServiceQuotaResponse>;
}

#[derive(Debug, Clone)]
pub struct ServiceQuotasClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl ServiceQuotasClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "servicequotas".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn base_url(&self) -> String {
        self.credentials
            .get_service_endpoint_option("servicequotas")
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                format!(
                    "https://servicequotas.{}.amazonaws.com",
                    self.credentials.region()
                )
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ServiceQuotasApi for ServiceQuotasClient {
    async fn get_service_quota(
        &self,
        service_code: &str,
        quota_code: &str,
    ) -> Result<GetServiceQuotaResponse> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::json!({
            "ServiceCode": service_code,
            "QuotaCode": quota_code,
        });
        let builder = self
            .client
            .post(format!("{}/", self.base_url().trim_end_matches('/')))
            .host(&format!(
                "servicequotas.{}.amazonaws.com",
                self.credentials.region()
            ))
            .header("X-Amz-Target", "ServiceQuotasV20190624.GetServiceQuota")
            .header("Content-Type", "application/x-amz-json-1.1")
            .json(&body);
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
    fn parses_applied_quota() {
        let response: GetServiceQuotaResponse = serde_json::from_str(
            r#"{"Quota":{"QuotaCode":"L-0263D0A3","ServiceCode":"ec2","QuotaName":"EC2-VPC Elastic IPs","Value":12.0}}"#,
        )
        .unwrap();
        assert_eq!(response.quota.and_then(|quota| quota.value), Some(12.0));
    }
}
