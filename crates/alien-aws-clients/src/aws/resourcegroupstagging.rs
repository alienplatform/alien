use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use bon::Builder;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

pub const GET_RESOURCES_TARGET: &str = "ResourceGroupsTaggingAPI_20170126.GetResources";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourceGroupsTaggingApi: Send + Sync + std::fmt::Debug {
    async fn get_resources(&self, request: GetResourcesRequest) -> Result<GetResourcesResponse>;
}

#[derive(Debug, Clone)]
pub struct ResourceGroupsTaggingClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl ResourceGroupsTaggingClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "tagging".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn host(&self) -> String {
        format!("tagging.{}.amazonaws.com", self.credentials.region())
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self
            .credentials
            .get_service_endpoint_option("resourcegroupstagging")
            .or_else(|| self.credentials.get_service_endpoint_option("tagging"))
        {
            override_url.to_string()
        } else {
            format!("https://{}", self.host())
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = format!("{}/", self.get_base_url().trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&self.host())
            .header("X-Amz-Target", target)
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&body))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &error.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_error(status, text, operation, resource, request_body)
                    {
                        return Err(error.context(mapped));
                    }
                }
                Err(error)
            }
        }
    }

    fn map_error(
        status: StatusCode,
        body: &str,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: Option<ResourceGroupsTaggingErrorResponse> = serde_json::from_str(body).ok();
        let code = parsed
            .as_ref()
            .map(|error| error.code.trim_start_matches('#'))
            .unwrap_or_default();
        let message = parsed
            .as_ref()
            .and_then(|error| error.message.clone())
            .unwrap_or_else(|| body.to_string());

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Some(ErrorData::AuthenticationError { message })
            }
            StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded { message }),
            StatusCode::BAD_REQUEST if code == "InvalidParameterException" => {
                Some(ErrorData::InvalidClientConfig {
                    message,
                    errors: None,
                })
            }
            _ if !body.trim().is_empty() => Some(ErrorData::HttpResponseError {
                message: format!("{} failed for '{}': {}", operation, resource, message),
                url: String::new(),
                http_status: status.as_u16(),
                http_request_text: request_body.map(ToOwned::to_owned),
                http_response_text: Some(body.to_string()),
            }),
            _ => None,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ResourceGroupsTaggingApi for ResourceGroupsTaggingClient {
    async fn get_resources(&self, request: GetResourcesRequest) -> Result<GetResourcesResponse> {
        let body = serde_json::to_string(&request).map_err(|error| {
            alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Failed to serialize GetResources request: {}", error),
                errors: None,
            })
        })?;

        self.send_json(
            GET_RESOURCES_TARGET,
            body,
            "GetResources",
            "resource inventory",
        )
        .await
    }
}

#[derive(Debug, Clone, Default, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetResourcesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources_per_page: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub tag_filters: Vec<TagFilter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub resource_type_filters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_compliance_details: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_compliant_resources: Option<bool>,
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TagFilter {
    pub key: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetResourcesResponse {
    #[serde(default)]
    pub pagination_token: Option<String>,
    #[serde(default)]
    pub resource_tag_mapping_list: Vec<ResourceTagMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourceTagMapping {
    #[serde(rename = "ResourceARN")]
    pub resource_arn: String,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub compliance_details: Option<ComplianceDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ComplianceDetails {
    #[serde(default)]
    pub noncompliant_keys: Vec<String>,
    #[serde(default)]
    pub keys_with_noncompliant_values: Vec<String>,
    #[serde(default)]
    pub compliance_status: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ResourceGroupsTaggingErrorResponse {
    #[serde(rename = "__type", alias = "Code")]
    code: String,
    #[serde(rename = "Message", alias = "message")]
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_resources_request_matches_aws_json_shape() {
        let request = GetResourcesRequest::builder()
            .resources_per_page(1)
            .tag_filters(vec![TagFilter::builder()
                .key("Environment".to_string())
                .values(vec!["prod".to_string()])
                .build()])
            .resource_type_filters(vec!["ec2:instance".to_string()])
            .build();

        let encoded = serde_json::to_value(request).unwrap();

        assert_eq!(
            encoded,
            json!({
                "ResourcesPerPage": 1,
                "TagFilters": [{ "Key": "Environment", "Values": ["prod"] }],
                "ResourceTypeFilters": ["ec2:instance"]
            })
        );
    }

    #[test]
    fn get_resources_response_parses_resource_arn_and_tags() {
        let response: GetResourcesResponse = serde_json::from_value(json!({
            "PaginationToken": "next",
            "ResourceTagMappingList": [{
                "ResourceARN": "arn:aws:s3:::example",
                "Tags": [{ "Key": "Name", "Value": "example" }]
            }]
        }))
        .unwrap();

        assert_eq!(response.pagination_token.as_deref(), Some("next"));
        assert_eq!(
            response.resource_tag_mapping_list[0].resource_arn,
            "arn:aws:s3:::example"
        );
        assert_eq!(response.resource_tag_mapping_list[0].tags[0].key, "Name");
        assert_eq!(
            response.resource_tag_mapping_list[0].tags[0].value,
            "example"
        );
    }
}
