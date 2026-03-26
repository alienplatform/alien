use std::fmt::Debug;

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, ContextError};
use async_trait::async_trait;
use bon::Builder;
use form_urlencoded;

#[cfg(feature = "test-utils")]
use mockall::automock;
use quick_xml;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait IamApi: Send + Sync + Debug {
    // Role Operations
    async fn create_role(&self, request: CreateRoleRequest) -> Result<CreateRoleResponse>;
    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse>;
    async fn delete_role(&self, role_name: &str) -> Result<()>;
    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()>;
    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse>;
    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()>;
    async fn update_assume_role_policy(&self, role_name: &str, policy_document: &str)
        -> Result<()>;
    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse>;
    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse>;

    // Instance Profile Operations
    async fn create_instance_profile(
        &self,
        request: CreateInstanceProfileRequest,
    ) -> Result<CreateInstanceProfileResponse>;
    async fn get_instance_profile(
        &self,
        instance_profile_name: &str,
    ) -> Result<GetInstanceProfileResponse>;
    async fn delete_instance_profile(&self, instance_profile_name: &str) -> Result<()>;
    async fn add_role_to_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()>;
    async fn remove_role_from_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()>;
    async fn list_instance_profiles(
        &self,
        request: ListInstanceProfilesRequest,
    ) -> Result<ListInstanceProfilesResponse>;
}

/// AWS IAM client using the new request/​error abstractions.
#[derive(Debug, Clone)]
pub struct IamClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl IamClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "iam".into(),
            // IAM is *always* signed in us-east-1 regardless of the target region.
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: Some("us-east-1".into()),
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("iam") {
            override_url.to_string()
        } else {
            "https://iam.amazonaws.com".to_string()
        }
    }

    fn build_form_body(action: &str, version: &str, params: Vec<(String, String)>) -> String {
        let mut all = vec![
            ("Action".to_string(), action.to_string()),
            ("Version".to_string(), version.to_string()),
        ];
        all.extend(params);

        all.into_iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    k,
                    form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                )
            })
            .collect::<Vec<String>>()
            .join("&")
    }

    // ---- Internal helpers ------------------------------------------------
    async fn post_xml<T: DeserializeOwned + Send + 'static>(
        &self,
        body: String,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));
        let builder = self
            .client
            .post(&url)
            .host("iam.amazonaws.com")
            .content_type_form()
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_iam_error(status, text, operation_name, resource_name, &body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse IAM error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn post_no_response(
        &self,
        body: String,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));
        let builder = self
            .client
            .post(&url)
            .host("iam.amazonaws.com")
            .content_type_form()
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_iam_error(status, text, operation_name, resource_name, &body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse IAM error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_iam_error(
        status: StatusCode,
        error_body: &str,
        operation: &str,
        resource_name: &str,
        request_body: &str,
    ) -> Option<ErrorData> {
        // Attempt to parse the canonical AWS error XML.
        let parsed_error: std::result::Result<IamErrorResponse, _> =
            quick_xml::de::from_str(error_body);

        let (error_code, error_message) = match parsed_error {
            Ok(e) => (
                e.error.code.unwrap_or_else(|| "UnknownErrorCode".into()),
                e.error.message.unwrap_or_else(|| "Unknown error".into()),
            ),
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match error_code.as_str() {
            // Access & auth
            "AccessDenied"
            | "AccessDeniedException"
            | "UnauthorizedOperation"
            | "InvalidUserID.NotFound"
            | "AuthFailure"
            | "SignatureDoesNotMatch"
            | "TokenRefreshRequired"
            | "NotAuthorized"
            | "InvalidClientTokenId"
            | "MissingAuthenticationToken"
            | "OptInRequired" => ErrorData::RemoteAccessDenied {
                resource_type: "IAM Resource".into(),
                resource_name: resource_name.into(),
            },

            // Rate limiting / throttling
            "Throttling" | "ThrottlingException" | "RequestLimitExceeded" => {
                ErrorData::RateLimitExceeded {
                    message: error_message,
                }
            }

            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "ServiceFailure" => {
                ErrorData::RemoteServiceUnavailable {
                    message: error_message,
                }
            }

            // Not found
            "NoSuchEntity" | "NoSuchRole" | "NoSuchPolicy" => ErrorData::RemoteResourceNotFound {
                resource_type: "IAM Resource".into(),
                resource_name: resource_name.into(),
            },

            // Already exists
            "EntityAlreadyExists" | "RoleAlreadyExists" => ErrorData::RemoteResourceConflict {
                message: error_message,
                resource_type: "IAM Resource".into(),
                resource_name: resource_name.into(),
            },

            // Quota / limit exceeded
            "LimitExceeded"
            | "LimitExceededException"
            | "PolicySizeLimitExceeded"
            | "RoleSizeLimitExceeded" => ErrorData::QuotaExceeded {
                message: error_message,
            },

            // Generic fallback categories
            _ => match status {
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message: error_message,
                    resource_type: "IAM Resource".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Resource".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "IAM Resource".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
                    message: error_message,
                },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable {
                    message: error_message,
                },
                _ => ErrorData::HttpResponseError {
                    message: format!("IAM operation failed: {}", error_message),
                    url: "iam.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(error_body.into()),
                    http_request_text: Some(request_body.into()),
                },
            },
        })
    }

    // ---- Public API ------------------------------------------------------
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl IamApi for IamClient {
    async fn create_role(&self, request: CreateRoleRequest) -> Result<CreateRoleResponse> {
        let mut params: Vec<(String, String)> = vec![
            ("RoleName".to_string(), request.role_name.clone()),
            (
                "AssumeRolePolicyDocument".to_string(),
                request.assume_role_policy_document.clone(),
            ),
        ];

        if let Some(ref path) = request.path {
            params.push(("Path".to_string(), path.clone()));
        }
        if let Some(ref description) = request.description {
            params.push(("Description".to_string(), description.clone()));
        }
        if let Some(max_session) = request.max_session_duration {
            params.push(("MaxSessionDuration".to_string(), max_session.to_string()));
        }

        // Handle tags array
        if let Some(ref tags) = request.tags {
            for (i, tag) in tags.iter().enumerate() {
                params.push((format!("Tags.member.{}.Key", i + 1), tag.key.clone()));
                params.push((format!("Tags.member.{}.Value", i + 1), tag.value.clone()));
            }
        }

        let body = Self::build_form_body("CreateRole", "2010-05-08", params);
        self.post_xml(body, "CreateRole", &request.role_name).await
    }

    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse> {
        let params = vec![("RoleName".to_string(), role_name.to_string())];
        let body = Self::build_form_body("GetRole", "2010-05-08", params);
        self.post_xml(body, "GetRole", role_name).await
    }

    async fn delete_role(&self, role_name: &str) -> Result<()> {
        let params = vec![("RoleName".to_string(), role_name.to_string())];
        let body = Self::build_form_body("DeleteRole", "2010-05-08", params);
        self.post_no_response(body, "DeleteRole", role_name).await
    }

    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyName".to_string(), policy_name.to_string()),
            ("PolicyDocument".to_string(), policy_document.to_string()),
        ];
        let body = Self::build_form_body("PutRolePolicy", "2010-05-08", params);
        let resource = format!("{}:{}", role_name, policy_name);
        self.post_no_response(body, "PutRolePolicy", &resource)
            .await
    }

    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyName".to_string(), policy_name.to_string()),
        ];
        let body = Self::build_form_body("GetRolePolicy", "2010-05-08", params);
        let resource = format!("{}:{}", role_name, policy_name);
        self.post_xml(body, "GetRolePolicy", &resource).await
    }

    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyName".to_string(), policy_name.to_string()),
        ];
        let body = Self::build_form_body("DeleteRolePolicy", "2010-05-08", params);
        let resource = format!("{}:{}", role_name, policy_name);
        self.post_no_response(body, "DeleteRolePolicy", &resource)
            .await
    }

    async fn update_assume_role_policy(
        &self,
        role_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyDocument".to_string(), policy_document.to_string()),
        ];
        let body = Self::build_form_body("UpdateAssumeRolePolicy", "2010-05-08", params);
        self.post_no_response(body, "UpdateAssumeRolePolicy", role_name)
            .await
    }

    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse> {
        let params = vec![("RoleName".to_string(), role_name.to_string())];
        let body = Self::build_form_body("ListAttachedRolePolicies", "2010-05-08", params);
        self.post_xml(body, "ListAttachedRolePolicies", role_name)
            .await
    }

    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyArn".to_string(), policy_arn.to_string()),
        ];
        let body = Self::build_form_body("DetachRolePolicy", "2010-05-08", params);
        let resource = format!("{}:{}", role_name, policy_arn);
        self.post_no_response(body, "DetachRolePolicy", &resource)
            .await
    }

    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse> {
        let params = vec![("RoleName".to_string(), role_name.to_string())];
        let body = Self::build_form_body("ListRolePolicies", "2010-05-08", params);
        self.post_xml(body, "ListRolePolicies", role_name).await
    }

    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let params = vec![
            ("RoleName".to_string(), role_name.to_string()),
            ("PolicyArn".to_string(), policy_arn.to_string()),
        ];
        let body = Self::build_form_body("AttachRolePolicy", "2010-05-08", params);
        let resource = format!("{}:{}", role_name, policy_arn);
        self.post_no_response(body, "AttachRolePolicy", &resource)
            .await
    }

    // ---------------------------------------------------------------------------
    // Instance Profile Operations
    // ---------------------------------------------------------------------------

    async fn create_instance_profile(
        &self,
        request: CreateInstanceProfileRequest,
    ) -> Result<CreateInstanceProfileResponse> {
        let mut params: Vec<(String, String)> = vec![(
            "InstanceProfileName".to_string(),
            request.instance_profile_name.clone(),
        )];

        if let Some(ref path) = request.path {
            params.push(("Path".to_string(), path.clone()));
        }

        if let Some(ref tags) = request.tags {
            for (i, tag) in tags.iter().enumerate() {
                params.push((format!("Tags.member.{}.Key", i + 1), tag.key.clone()));
                params.push((format!("Tags.member.{}.Value", i + 1), tag.value.clone()));
            }
        }

        let body = Self::build_form_body("CreateInstanceProfile", "2010-05-08", params);
        self.post_xml(
            body,
            "CreateInstanceProfile",
            &request.instance_profile_name,
        )
        .await
    }

    async fn get_instance_profile(
        &self,
        instance_profile_name: &str,
    ) -> Result<GetInstanceProfileResponse> {
        let params = vec![(
            "InstanceProfileName".to_string(),
            instance_profile_name.to_string(),
        )];
        let body = Self::build_form_body("GetInstanceProfile", "2010-05-08", params);
        self.post_xml(body, "GetInstanceProfile", instance_profile_name)
            .await
    }

    async fn delete_instance_profile(&self, instance_profile_name: &str) -> Result<()> {
        let params = vec![(
            "InstanceProfileName".to_string(),
            instance_profile_name.to_string(),
        )];
        let body = Self::build_form_body("DeleteInstanceProfile", "2010-05-08", params);
        self.post_no_response(body, "DeleteInstanceProfile", instance_profile_name)
            .await
    }

    async fn add_role_to_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()> {
        let params = vec![
            (
                "InstanceProfileName".to_string(),
                instance_profile_name.to_string(),
            ),
            ("RoleName".to_string(), role_name.to_string()),
        ];
        let body = Self::build_form_body("AddRoleToInstanceProfile", "2010-05-08", params);
        let resource = format!("{}:{}", instance_profile_name, role_name);
        self.post_no_response(body, "AddRoleToInstanceProfile", &resource)
            .await
    }

    async fn remove_role_from_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()> {
        let params = vec![
            (
                "InstanceProfileName".to_string(),
                instance_profile_name.to_string(),
            ),
            ("RoleName".to_string(), role_name.to_string()),
        ];
        let body = Self::build_form_body("RemoveRoleFromInstanceProfile", "2010-05-08", params);
        let resource = format!("{}:{}", instance_profile_name, role_name);
        self.post_no_response(body, "RemoveRoleFromInstanceProfile", &resource)
            .await
    }

    async fn list_instance_profiles(
        &self,
        request: ListInstanceProfilesRequest,
    ) -> Result<ListInstanceProfilesResponse> {
        let mut params: Vec<(String, String)> = Vec::new();

        if let Some(ref path_prefix) = request.path_prefix {
            params.push(("PathPrefix".to_string(), path_prefix.clone()));
        }
        if let Some(ref marker) = request.marker {
            params.push(("Marker".to_string(), marker.clone()));
        }
        if let Some(max_items) = request.max_items {
            params.push(("MaxItems".to_string(), max_items.to_string()));
        }

        let body = Self::build_form_body("ListInstanceProfiles", "2010-05-08", params);
        self.post_xml(body, "ListInstanceProfiles", "instance-profiles")
            .await
    }
}

// -------------------------------------------------------------------------
// Error XML structs (PascalCase matching AWS IAM)
// -------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct IamErrorResponse {
    pub error: IamErrorDetails,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct IamErrorDetails {
    pub code: Option<String>,
    pub message: Option<String>,
}

// -------------------------------------------------------------------------
// Request / response payloads (intact from previous implementation)
// -------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRoleRequest {
    pub role_name: String,
    pub assume_role_policy_document: String,
    pub path: Option<String>,
    pub description: Option<String>,
    pub max_session_duration: Option<i32>,
    #[serde(skip)]
    pub tags: Option<Vec<CreateRoleTag>>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CreateRoleTag {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRoleResponse {
    pub create_role_result: CreateRoleResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRoleResult {
    pub role: Role,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Role {
    pub path: String,
    pub role_name: String,
    pub role_id: String,
    pub arn: String,
    pub create_date: String,
    pub assume_role_policy_document: Option<String>,
    pub description: Option<String>,
    pub max_session_duration: Option<i32>,
    pub permissions_boundary: Option<AttachedPermissionsBoundary>,
    pub tags: Option<Tags>,
    pub role_last_used: Option<RoleLastUsed>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AttachedPermissionsBoundary {
    pub permissions_boundary_type: Option<String>,
    pub permissions_boundary_arn: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Tags {
    #[serde(rename = "member", default)]
    pub member: Vec<Tag>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct RoleLastUsed {
    pub last_used_date: Option<String>,
    pub region: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetRoleResponse {
    pub get_role_result: GetRoleResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetRoleResult {
    pub role: Role,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetRolePolicyResponse {
    pub get_role_policy_result: GetRolePolicyResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetRolePolicyResult {
    pub role_name: String,
    pub policy_name: String,
    pub policy_document: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListAttachedRolePoliciesResponse {
    pub list_attached_role_policies_result: ListAttachedRolePoliciesResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListAttachedRolePoliciesResult {
    pub attached_policies: Option<AttachedPolicies>,
    pub is_truncated: Option<bool>,
    pub marker: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AttachedPolicies {
    #[serde(rename = "member", default)]
    pub member: Vec<AttachedPolicy>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AttachedPolicy {
    pub policy_name: String,
    pub policy_arn: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListRolePoliciesResponse {
    pub list_role_policies_result: ListRolePoliciesResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListRolePoliciesResult {
    pub policy_names: Option<PolicyNames>,
    pub is_truncated: Option<bool>,
    pub marker: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct PolicyNames {
    #[serde(rename = "member", default)]
    pub member: Vec<String>,
}

// -------------------------------------------------------------------------
// Instance Profile structures
// -------------------------------------------------------------------------

/// Request to create an instance profile.
#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateInstanceProfileRequest {
    /// The name of the instance profile.
    pub instance_profile_name: String,
    /// The path for the instance profile.
    pub path: Option<String>,
    /// Tags for the instance profile.
    #[serde(skip)]
    pub tags: Option<Vec<CreateRoleTag>>,
}

/// Response from creating an instance profile.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateInstanceProfileResponse {
    pub create_instance_profile_result: CreateInstanceProfileResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct CreateInstanceProfileResult {
    pub instance_profile: InstanceProfile,
}

/// Response from getting an instance profile.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetInstanceProfileResponse {
    pub get_instance_profile_result: GetInstanceProfileResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetInstanceProfileResult {
    pub instance_profile: InstanceProfile,
}

/// Request to list instance profiles.
#[derive(Serialize, Debug, Clone, Builder, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ListInstanceProfilesRequest {
    /// The path prefix for filtering.
    pub path_prefix: Option<String>,
    /// The marker for pagination.
    pub marker: Option<String>,
    /// The maximum number of items to return.
    pub max_items: Option<i32>,
}

/// Response from listing instance profiles.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListInstanceProfilesResponse {
    pub list_instance_profiles_result: ListInstanceProfilesResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ListInstanceProfilesResult {
    pub instance_profiles: Option<InstanceProfiles>,
    pub is_truncated: Option<bool>,
    pub marker: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceProfiles {
    #[serde(rename = "member", default)]
    pub member: Vec<InstanceProfile>,
}

/// Represents an IAM instance profile.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceProfile {
    /// The path to the instance profile.
    pub path: String,
    /// The name of the instance profile.
    pub instance_profile_name: String,
    /// The ID of the instance profile.
    pub instance_profile_id: String,
    /// The ARN of the instance profile.
    pub arn: String,
    /// The date and time the instance profile was created.
    pub create_date: String,
    /// The roles associated with the instance profile.
    pub roles: Option<InstanceProfileRoles>,
    /// Tags associated with the instance profile.
    pub tags: Option<Tags>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceProfileRoles {
    #[serde(rename = "member", default)]
    pub member: Vec<Role>,
}

// -------------------------------------------------------------------------
// Trust policy structures for IAM role trust relationships
// -------------------------------------------------------------------------

/// Trust policy principal types
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TrustPolicyPrincipal {
    /// AWS service principal (e.g., lambda.amazonaws.com)
    Service {
        #[serde(rename = "Service")]
        service: TrustPolicyPrincipalValue,
    },
    /// AWS IAM principal (e.g., role ARN)
    Aws {
        #[serde(rename = "AWS")]
        aws: TrustPolicyPrincipalValue,
    },
}

/// Principal value can be a single string or array of strings
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TrustPolicyPrincipalValue {
    /// Single principal
    Single(String),
    /// Multiple principals
    Multiple(Vec<String>),
}

/// Trust policy statement
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrustPolicyStatement {
    /// Effect of the statement (Allow/Deny)
    pub effect: String,
    /// Principal that can assume the role
    pub principal: TrustPolicyPrincipal,
    /// Action being permitted (usually sts:AssumeRole)
    pub action: String,
}

/// Complete trust policy document
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrustPolicyDocument {
    /// Policy version (typically "2012-10-17")
    pub version: String,
    /// List of policy statements
    pub statement: Vec<TrustPolicyStatement>,
}
