//! AWS Simple Email Service (SES) v1 client.

use std::fmt::Debug;

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Metadata for the active SES receipt rule set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveReceiptRuleSet {
    /// Name of the active rule set, or `None` when receiving is inactive.
    pub name: Option<String>,
}

/// Read-only SES operations used by resource health checks.
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait SesApi: Send + Sync + Debug {
    /// Return the account's active receipt rule set in the configured region.
    async fn describe_active_receipt_rule_set(&self) -> Result<ActiveReceiptRuleSet>;
}

/// AWS SES v1 Query API client.
#[derive(Debug, Clone)]
pub struct SesClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl SesClient {
    /// Create an SES client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "ses".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn base_url(&self) -> String {
        self.credentials
            .get_service_endpoint_option("ses")
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("https://email.{}.amazonaws.com", self.credentials.region()))
    }

    fn host(&self) -> String {
        format!("email.{}.amazonaws.com", self.credentials.region())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeActiveReceiptRuleSetResponse {
    describe_active_receipt_rule_set_result: DescribeActiveReceiptRuleSetResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeActiveReceiptRuleSetResult {
    metadata: Option<ReceiptRuleSetMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ReceiptRuleSetMetadata {
    name: String,
}

#[async_trait]
impl SesApi for SesClient {
    async fn describe_active_receipt_rule_set(&self) -> Result<ActiveReceiptRuleSet> {
        self.credentials.ensure_fresh().await?;
        let body = "Action=DescribeActiveReceiptRuleSet&Version=2010-12-01";
        let builder = self
            .client
            .post(self.base_url())
            .host(&self.host())
            .content_type_form()
            .body(body);
        let response: DescribeActiveReceiptRuleSetResponse =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await?;

        Ok(ActiveReceiptRuleSet {
            name: response
                .describe_active_receipt_rule_set_result
                .metadata
                .map(|metadata| metadata.name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_rule_set() {
        let response: DescribeActiveReceiptRuleSetResponse = quick_xml::de::from_str(
            r#"<DescribeActiveReceiptRuleSetResponse>
                <DescribeActiveReceiptRuleSetResult>
                    <Metadata><Name>example-inbound</Name></Metadata>
                </DescribeActiveReceiptRuleSetResult>
            </DescribeActiveReceiptRuleSetResponse>"#,
        )
        .expect("response should parse");

        assert_eq!(
            response
                .describe_active_receipt_rule_set_result
                .metadata
                .expect("metadata")
                .name,
            "example-inbound"
        );
    }

    #[test]
    fn parses_no_active_rule_set() {
        let response: DescribeActiveReceiptRuleSetResponse = quick_xml::de::from_str(
            r#"<DescribeActiveReceiptRuleSetResponse>
                <DescribeActiveReceiptRuleSetResult />
            </DescribeActiveReceiptRuleSetResponse>"#,
        )
        .expect("response should parse");

        assert!(response
            .describe_active_receipt_rule_set_result
            .metadata
            .is_none());
    }
}
