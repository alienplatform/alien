//! Amazon Data Lifecycle Manager client.
//!
//! Covers the lifecycle-policy operations needed to keep a scheduled EBS
//! snapshot policy in sync with a desired schedule. DLM is a REST/JSON API:
//! https://docs.aws.amazon.com/dlm/latest/APIReference/

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait DlmApi: Send + Sync + Debug {
    /// Creates a lifecycle policy. Sent once: DLM has no idempotency token, so a
    /// retried create after a lost response could create a second policy.
    async fn create_lifecycle_policy(
        &self,
        request: CreateLifecyclePolicyRequest,
    ) -> Result<CreateLifecyclePolicyResponse>;
    async fn get_lifecycle_policy(&self, policy_id: &str) -> Result<GetLifecyclePolicyResponse>;
    /// Lists policy summaries, optionally filtered by target tags (`key=value`).
    async fn get_lifecycle_policies(
        &self,
        request: GetLifecyclePoliciesRequest,
    ) -> Result<GetLifecyclePoliciesResponse>;
    async fn update_lifecycle_policy(
        &self,
        policy_id: &str,
        request: UpdateLifecyclePolicyRequest,
    ) -> Result<()>;
    async fn delete_lifecycle_policy(&self, policy_id: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct DlmClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

#[derive(Clone, Copy)]
enum Attempts {
    Retried,
    Once,
}

impl DlmClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "dlm".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("dlm") {
            override_url.to_string()
        } else {
            format!("https://dlm.{}.amazonaws.com", self.credentials.region())
        }
    }

    fn request(&self, method: Method, path_and_query: &str) -> reqwest::RequestBuilder {
        let url = format!(
            "{}{}",
            self.get_base_url().trim_end_matches('/'),
            path_and_query
        );
        self.client
            .request(method, &url)
            .host(&format!("dlm.{}.amazonaws.com", self.credentials.region()))
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        attempts: Attempts,
        method: Method,
        path_and_query: &str,
        body: Option<String>,
        operation: &str,
        resource_name: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let mut builder = self.request(method, path_and_query);
        if let Some(body) = body {
            builder = builder
                .header("Content-Type", "application/json")
                .content_sha256(&body)
                .body(body);
        }
        let result = match attempts {
            Attempts::Retried => {
                crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await
            }
            Attempts::Once => {
                crate::aws::aws_request_utils::sign_send_json_once(builder, &self.sign_config())
                    .await
            }
        };
        Self::map_result(result, operation, resource_name)
    }

    async fn send_no_response(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
        operation: &str,
        resource_name: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let mut builder = self.request(method, path);
        if let Some(body) = body {
            builder = builder
                .header("Content-Type", "application/json")
                .content_sha256(&body)
                .body(body);
        }
        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;
        Self::map_result(result, operation, resource_name)
    }

    fn map_result<T>(result: Result<T>, operation: &str, resource_name: &str) -> Result<T> {
        let error = match result {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let Some(ErrorData::HttpResponseError {
            http_status,
            http_response_text,
            ..
        }) = &error.error
        else {
            return Err(error);
        };
        let status =
            StatusCode::from_u16(*http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mapped = map_dlm_error(
            status,
            http_response_text.as_deref().unwrap_or_default(),
            operation,
            resource_name,
        );
        Err(error.context(mapped))
    }

    fn serialize<T: Serialize>(value: &T, operation: &str) -> Result<String> {
        serde_json::to_string(value)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize DLM {operation} request"),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl DlmApi for DlmClient {
    async fn create_lifecycle_policy(
        &self,
        request: CreateLifecyclePolicyRequest,
    ) -> Result<CreateLifecyclePolicyResponse> {
        let body = Self::serialize(&request, "CreateLifecyclePolicy")?;
        self.send_json(
            Attempts::Once,
            Method::POST,
            "/policies",
            Some(body),
            "CreateLifecyclePolicy",
            &request.description,
        )
        .await
    }

    async fn get_lifecycle_policy(&self, policy_id: &str) -> Result<GetLifecyclePolicyResponse> {
        self.send_json(
            Attempts::Retried,
            Method::GET,
            &format!("/policies/{}", urlencoding::encode(policy_id)),
            None,
            "GetLifecyclePolicy",
            policy_id,
        )
        .await
    }

    async fn get_lifecycle_policies(
        &self,
        request: GetLifecyclePoliciesRequest,
    ) -> Result<GetLifecyclePoliciesResponse> {
        let path = format!("/policies{}", request.query_string());
        self.send_json(
            Attempts::Retried,
            Method::GET,
            &path,
            None,
            "GetLifecyclePolicies",
            "lifecycle policies",
        )
        .await
    }

    async fn update_lifecycle_policy(
        &self,
        policy_id: &str,
        request: UpdateLifecyclePolicyRequest,
    ) -> Result<()> {
        let body = Self::serialize(&request, "UpdateLifecyclePolicy")?;
        self.send_no_response(
            Method::PATCH,
            &format!("/policies/{}", urlencoding::encode(policy_id)),
            Some(body),
            "UpdateLifecyclePolicy",
            policy_id,
        )
        .await
    }

    async fn delete_lifecycle_policy(&self, policy_id: &str) -> Result<()> {
        self.send_no_response(
            Method::DELETE,
            &format!("/policies/{}", urlencoding::encode(policy_id)),
            None,
            "DeleteLifecyclePolicy",
            policy_id,
        )
        .await
    }
}

/// DLM error body. The error type arrives in `x-amzn-ErrorType` and, for most
/// errors, in the body as `Code`/`code`.
#[derive(Debug, Deserialize)]
struct DlmErrorBody {
    #[serde(alias = "Message")]
    message: Option<String>,
    #[serde(alias = "Code", alias = "__type")]
    code: Option<String>,
}

fn map_dlm_error(status: StatusCode, body: &str, operation: &str, resource: &str) -> ErrorData {
    let parsed = serde_json::from_str::<DlmErrorBody>(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|error| error.code.clone())
        .unwrap_or_default();
    let message = parsed
        .and_then(|error| error.message)
        .unwrap_or_else(|| format!("DLM {operation} failed with HTTP {status}"));
    let message = if code.is_empty() {
        message
    } else {
        format!("{code}: {message}")
    };
    match status {
        StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
            resource_type: "DLM lifecycle policy".to_string(),
            resource_name: resource.to_string(),
        },
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorData::RemoteAccessDenied {
            resource_type: "DLM lifecycle policy".to_string(),
            resource_name: resource.to_string(),
        },
        StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
        StatusCode::BAD_REQUEST => ErrorData::InvalidInput {
            message,
            field_name: None,
        },
        _ => ErrorData::RemoteServiceUnavailable {
            message: format!("DLM {operation} failed for '{resource}': {message}"),
        },
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A DLM tag (`Key`/`Value`), used for target tags and tags added to snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DlmTag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct PolicyDetails {
    /// `EBS_SNAPSHOT_MANAGEMENT`, `IMAGE_MANAGEMENT` or `EVENT_BASED_POLICY`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_type: Option<String>,
    /// `VOLUME` or `INSTANCE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_types: Option<Vec<String>>,
    /// `CLOUD`, `OUTPOST` or `LOCAL_ZONE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_locations: Option<Vec<String>>,
    /// Resources with ANY of these tags are targeted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_tags: Option<Vec<DlmTag>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules: Option<Vec<Schedule>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Schedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Copy the source volume's tags onto each snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_tags: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags_to_add: Option<Vec<DlmTag>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_rule: Option<CreateRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_rule: Option<RetainRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRule {
    /// For `HOURS`: 1, 2, 3, 4, 6, 8, 12 or 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_unit: Option<String>,
    /// Start time in UTC, `hh:mm`. DLM accepts one value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expression: Option<String>,
}

/// Count-based (`count`) or age-based (`interval` + `interval_unit`) retention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct RetainRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLifecyclePolicyRequest {
    /// Allowed characters: `[0-9A-Za-z _-]`, at most 500.
    pub description: String,
    pub execution_role_arn: String,
    /// `ENABLED` or `DISABLED`.
    pub state: String,
    pub policy_details: PolicyDetails,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLifecyclePolicyResponse {
    pub policy_id: String,
}

#[derive(Debug, Clone, Serialize, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateLifecyclePolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_role_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_details: Option<PolicyDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetLifecyclePolicyResponse {
    pub policy: LifecyclePolicy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LifecyclePolicy {
    pub policy_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub status_message: Option<String>,
    #[serde(default)]
    pub execution_role_arn: Option<String>,
    #[serde(default)]
    pub policy_arn: Option<String>,
    #[serde(default)]
    pub policy_details: Option<PolicyDetails>,
    #[serde(default)]
    pub tags: Option<HashMap<String, String>>,
}

/// Filters for `GetLifecyclePolicies`. Tag filters use `key=value` strings.
#[derive(Debug, Clone, Builder, Default)]
pub struct GetLifecyclePoliciesRequest {
    pub policy_ids: Option<Vec<String>>,
    /// `ENABLED`, `DISABLED` or `ERROR`.
    pub state: Option<String>,
    pub resource_types: Option<Vec<String>>,
    pub target_tags: Option<Vec<String>>,
    pub tags_to_add: Option<Vec<String>>,
}

impl GetLifecyclePoliciesRequest {
    /// Lists are sent as repeated query parameters, as the REST/JSON protocol does.
    fn query_string(&self) -> String {
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (name, values) in [
            ("policyIds", &self.policy_ids),
            ("resourceTypes", &self.resource_types),
            ("targetTags", &self.target_tags),
            ("tagsToAdd", &self.tags_to_add),
        ] {
            for value in values.iter().flatten() {
                query.append_pair(name, value);
            }
        }
        if let Some(state) = &self.state {
            query.append_pair("state", state);
        }
        let query = query.finish();
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetLifecyclePoliciesResponse {
    #[serde(default)]
    pub policies: Vec<LifecyclePolicySummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LifecyclePolicySummary {
    pub policy_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub policy_type: Option<String>,
    #[serde(default)]
    pub tags: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServiceOverrides;
    use alien_core::{AwsClientConfig, AwsCredentials};
    use httpmock::prelude::*;

    fn client(server: &MockServer) -> DlmClient {
        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("dlm".to_string(), server.base_url())]),
            }),
        };
        DlmClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        )
    }

    fn snapshot_policy_details() -> PolicyDetails {
        PolicyDetails::builder()
            .policy_type("EBS_SNAPSHOT_MANAGEMENT".to_string())
            .resource_types(vec!["VOLUME".to_string()])
            .target_tags(vec![DlmTag::builder()
                .key("alien-backup-policy".to_string())
                .value("stack-db".to_string())
                .build()])
            .schedules(vec![Schedule::builder()
                .name("every-4h".to_string())
                .copy_tags(true)
                .tags_to_add(vec![DlmTag::builder()
                    .key("alien-snapshot-kind".to_string())
                    .value("scheduled".to_string())
                    .build()])
                .create_rule(
                    CreateRule::builder()
                        .interval(4)
                        .interval_unit("HOURS".to_string())
                        .times(vec!["00:00".to_string()])
                        .build(),
                )
                .retain_rule(
                    RetainRule::builder()
                        .interval(7)
                        .interval_unit("DAYS".to_string())
                        .build(),
                )
                .build()])
            .build()
    }

    #[tokio::test]
    async fn create_posts_the_documented_body_once_and_returns_the_policy_id() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/policies")
                    .header_exists("authorization")
                    .json_body(serde_json::json!({
                        "Description": "stack db volume backups",
                        "ExecutionRoleArn": "arn:aws:iam::123456789012:role/stack-db-dlm",
                        "State": "ENABLED",
                        "PolicyDetails": {
                            "PolicyType": "EBS_SNAPSHOT_MANAGEMENT",
                            "ResourceTypes": ["VOLUME"],
                            "TargetTags": [{"Key": "alien-backup-policy", "Value": "stack-db"}],
                            "Schedules": [{
                                "Name": "every-4h",
                                "CopyTags": true,
                                "TagsToAdd": [{"Key": "alien-snapshot-kind", "Value": "scheduled"}],
                                "CreateRule": {"Interval": 4, "IntervalUnit": "HOURS", "Times": ["00:00"]},
                                "RetainRule": {"Interval": 7, "IntervalUnit": "DAYS"}
                            }]
                        },
                        "Tags": {"alien-stack": "stack"}
                    }));
                then.status(200)
                    .json_body(serde_json::json!({"PolicyId": "policy-0abc"}));
            })
            .await;

        let response = client(&server)
            .create_lifecycle_policy(
                CreateLifecyclePolicyRequest::builder()
                    .description("stack db volume backups".to_string())
                    .execution_role_arn("arn:aws:iam::123456789012:role/stack-db-dlm".to_string())
                    .state("ENABLED".to_string())
                    .policy_details(snapshot_policy_details())
                    .tags(HashMap::from([(
                        "alien-stack".to_string(),
                        "stack".to_string(),
                    )]))
                    .build(),
            )
            .await
            .expect("create should succeed");

        assert_eq!(response.policy_id, "policy-0abc");
        assert_eq!(create.hits_async().await, 1);
    }

    #[tokio::test]
    async fn a_rejected_create_is_not_retried_and_keeps_the_service_message() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST).path("/policies");
                then.status(400)
                    .header("x-amzn-ErrorType", "InvalidRequestException")
                    .json_body(serde_json::json!({
                        "Code": "InvalidRequestException",
                        "Message": "Provided role arn:aws:iam::123456789012:role/stack-db-dlm cannot be assumed by principal 'dlm.amazonaws.com'."
                    }));
            })
            .await;

        let error = client(&server)
            .create_lifecycle_policy(
                CreateLifecyclePolicyRequest::builder()
                    .description("stack db volume backups".to_string())
                    .execution_role_arn("arn:aws:iam::123456789012:role/stack-db-dlm".to_string())
                    .state("ENABLED".to_string())
                    .policy_details(snapshot_policy_details())
                    .build(),
            )
            .await
            .expect_err("an unassumable role is rejected");

        assert_eq!(create.hits_async().await, 1);
        assert_eq!(error.code, "INVALID_INPUT");
        match error.error {
            Some(ErrorData::InvalidInput { message, .. }) => {
                assert!(message.contains("cannot be assumed"), "{message}");
                assert!(message.starts_with("InvalidRequestException"), "{message}");
            }
            other => panic!("unexpected error data: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_policies_sends_target_tags_as_repeated_query_parameters() {
        let server = MockServer::start_async().await;
        let list = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/policies")
                    .query_param("targetTags", "alien-backup-policy=stack-db")
                    .query_param("resourceTypes", "VOLUME");
                then.status(200).json_body(serde_json::json!({
                    "Policies": [{
                        "PolicyId": "policy-0abc",
                        "Description": "stack db volume backups",
                        "State": "ENABLED",
                        "PolicyType": "EBS_SNAPSHOT_MANAGEMENT",
                        "Tags": {"alien-stack": "stack"},
                        "DefaultPolicy": false
                    }]
                }));
            })
            .await;

        let response = client(&server)
            .get_lifecycle_policies(
                GetLifecyclePoliciesRequest::builder()
                    .target_tags(vec!["alien-backup-policy=stack-db".to_string()])
                    .resource_types(vec!["VOLUME".to_string()])
                    .build(),
            )
            .await
            .expect("list should succeed");

        assert_eq!(list.hits_async().await, 1);
        assert_eq!(response.policies.len(), 1);
        assert_eq!(response.policies[0].policy_id, "policy-0abc");
        assert_eq!(
            response.policies[0]
                .tags
                .as_ref()
                .and_then(|tags| tags.get("alien-stack"))
                .map(String::as_str),
            Some("stack")
        );
    }

    #[tokio::test]
    async fn get_policy_reads_the_schedule_back() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/policies/policy-0abc");
                then.status(200).json_body(serde_json::json!({
                    "Policy": {
                        "PolicyId": "policy-0abc",
                        "Description": "stack db volume backups",
                        "State": "ENABLED",
                        "StatusMessage": "ENABLED",
                        "ExecutionRoleArn": "arn:aws:iam::123456789012:role/stack-db-dlm",
                        "DateCreated": "2026-10-05T12:00:00.000Z",
                        "DateModified": "2026-10-05T12:00:00.000Z",
                        "PolicyArn": "arn:aws:dlm:us-east-1:123456789012:policy/policy-0abc",
                        "DefaultPolicy": false,
                        "PolicyDetails": {
                            "PolicyType": "EBS_SNAPSHOT_MANAGEMENT",
                            "PolicyLanguage": "STANDARD",
                            "ResourceTypes": ["VOLUME"],
                            "ResourceLocations": ["CLOUD"],
                            "TargetTags": [{"Key": "alien-backup-policy", "Value": "stack-db"}],
                            "Schedules": [{
                                "Name": "every-4h",
                                "CopyTags": true,
                                "TagsToAdd": [{"Key": "alien-snapshot-kind", "Value": "scheduled"}],
                                "CreateRule": {"Interval": 4, "IntervalUnit": "HOURS", "Times": ["00:00"], "Location": "CLOUD"},
                                "RetainRule": {"Interval": 7, "IntervalUnit": "DAYS"}
                            }]
                        },
                        "Tags": {"alien-stack": "stack"}
                    }
                }));
            })
            .await;

        let policy = client(&server)
            .get_lifecycle_policy("policy-0abc")
            .await
            .expect("get should succeed")
            .policy;

        let details = policy.policy_details.expect("policy details");
        let schedule = &details.schedules.expect("schedules")[0];
        assert_eq!(
            schedule.create_rule.as_ref().and_then(|rule| rule.interval),
            Some(4)
        );
        assert_eq!(
            schedule.retain_rule,
            Some(
                RetainRule::builder()
                    .interval(7)
                    .interval_unit("DAYS".to_string())
                    .build()
            )
        );
        assert_eq!(
            policy.execution_role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/stack-db-dlm")
        );
    }

    #[tokio::test]
    async fn update_patches_and_delete_maps_a_missing_policy_to_not_found() {
        let server = MockServer::start_async().await;
        let update = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PATCH)
                    .path("/policies/policy-0abc")
                    .json_body(serde_json::json!({"State": "ENABLED"}));
                then.status(200);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path("/policies/policy-gone");
                then.status(404).json_body(serde_json::json!({
                    "Code": "ResourceNotFoundException",
                    "Message": "Lifecycle policy not found",
                    "ResourceType": "LifecyclePolicy",
                    "ResourceIds": ["policy-gone"]
                }));
            })
            .await;
        let dlm = client(&server);

        dlm.update_lifecycle_policy(
            "policy-0abc",
            UpdateLifecyclePolicyRequest::builder()
                .state("ENABLED".to_string())
                .build(),
        )
        .await
        .expect("update should succeed");
        let error = dlm
            .delete_lifecycle_policy("policy-gone")
            .await
            .expect_err("missing policy");

        assert_eq!(update.hits_async().await, 1);
        assert_eq!(error.code, "REMOTE_RESOURCE_NOT_FOUND");
    }
}
