use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use urlencoding;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// IAM service configuration
#[derive(Debug)]
pub struct IamServiceConfig;

impl GcpServiceConfig for IamServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://iam.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://iam.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "IAM"
    }

    fn service_key(&self) -> &'static str {
        "iam"
    }
}

/// IAM Credentials service configuration for impersonation
#[derive(Debug)]
pub struct IamCredentialsServiceConfig;

impl GcpServiceConfig for IamCredentialsServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://iamcredentials.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://iamcredentials.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "IAM Service Account Credentials"
    }

    fn service_key(&self) -> &'static str {
        "iamcredentials"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait IamApi: Send + Sync + Debug {
    async fn create_service_account(
        &self,
        account_id: String,
        service_account: CreateServiceAccountRequest,
    ) -> Result<ServiceAccount>;

    async fn delete_service_account(&self, service_account_name: String) -> Result<()>;

    async fn get_service_account(&self, service_account_name: String) -> Result<ServiceAccount>;

    async fn patch_service_account(
        &self,
        service_account_name: String,
        service_account: ServiceAccount,
        update_mask: Option<String>,
    ) -> Result<ServiceAccount>;

    async fn get_service_account_iam_policy(
        &self,
        service_account_name: String,
    ) -> Result<IamPolicy>;

    async fn set_service_account_iam_policy(
        &self,
        service_account_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    async fn create_role(&self, role_id: String, role: CreateRoleRequest) -> Result<Role>;

    async fn delete_role(&self, role_name: String) -> Result<Role>;

    async fn get_role(&self, role_name: String) -> Result<Role>;

    async fn patch_role(
        &self,
        role_name: String,
        role: Role,
        update_mask: Option<String>,
    ) -> Result<Role>;

    async fn generate_access_token(
        &self,
        service_account_name: String,
        request: GenerateAccessTokenRequest,
    ) -> Result<GenerateAccessTokenResponse>;
}

// --- IAM Client ---
#[derive(Debug)]
pub struct IamClient {
    base: GcpClientBase,
    credentials_base: GcpClientBase,
    project_id: String,
}

impl IamClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client.clone(), config.clone(), Box::new(IamServiceConfig)),
            credentials_base: GcpClientBase::new(
                client,
                config,
                Box::new(IamCredentialsServiceConfig),
            ),
            project_id,
        }
    }

    /// Helper function to construct the correct path for role operations.
    /// Handles both full resource names and just role IDs.
    fn build_role_path(&self, role_name: String) -> String {
        if role_name.starts_with("projects/") || role_name.starts_with("organizations/") {
            // role_name is already a full resource name, use it directly
            role_name.to_string()
        } else {
            // role_name is just the role ID, construct the full path
            format!("projects/{}/roles/{}", self.project_id, role_name)
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl IamApi for IamClient {
    /// Creates a new service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/create
    async fn create_service_account(
        &self,
        account_id: String,
        service_account: CreateServiceAccountRequest,
    ) -> Result<ServiceAccount> {
        let path = format!("projects/{}/serviceAccounts", self.project_id);
        let query_params = vec![("accountId", account_id.to_string())];

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(service_account),
                &account_id,
            )
            .await
    }

    /// Deletes a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/delete
    async fn delete_service_account(&self, service_account_name: String) -> Result<()> {
        let encoded_name = urlencoding::encode(service_account_name.as_str());
        let path = format!(
            "projects/{}/serviceAccounts/{}",
            self.project_id, encoded_name
        );

        self.base
            .execute_request_no_response(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &service_account_name,
            )
            .await
    }

    /// Gets a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/get
    async fn get_service_account(&self, service_account_name: String) -> Result<ServiceAccount> {
        let encoded_name = urlencoding::encode(&service_account_name).into_owned();
        let path = format!(
            "projects/{}/serviceAccounts/{}",
            self.project_id, encoded_name
        );

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &service_account_name,
            )
            .await
    }

    /// Updates (patches) a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/patch
    async fn patch_service_account(
        &self,
        service_account_name: String,
        service_account: ServiceAccount,
        update_mask: Option<String>,
    ) -> Result<ServiceAccount> {
        let encoded_name = urlencoding::encode(&service_account_name).into_owned();
        let path = format!(
            "projects/{}/serviceAccounts/{}",
            self.project_id, encoded_name
        );

        let request = PatchServiceAccountRequest {
            service_account,
            update_mask: update_mask.map(|s| s.to_string()),
        };

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                None,
                Some(request),
                &service_account_name,
            )
            .await
    }

    /// Gets the IAM policy for a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/getIamPolicy
    async fn get_service_account_iam_policy(
        &self,
        service_account_name: String,
    ) -> Result<IamPolicy> {
        let encoded_name = urlencoding::encode(&service_account_name).into_owned();
        let path = format!(
            "projects/{}/serviceAccounts/{}:getIamPolicy",
            self.project_id, encoded_name
        );

        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &service_account_name,
            )
            .await
    }

    /// Sets the IAM policy for a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/setIamPolicy
    async fn set_service_account_iam_policy(
        &self,
        service_account_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let encoded_name = urlencoding::encode(&service_account_name).into_owned();
        let path = format!(
            "projects/{}/serviceAccounts/{}:setIamPolicy",
            self.project_id, encoded_name
        );
        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request),
                &service_account_name,
            )
            .await
    }

    /// Creates a new custom role.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.roles/create
    async fn create_role(&self, role_id: String, role: CreateRoleRequest) -> Result<Role> {
        let path = format!("projects/{}/roles", self.project_id);
        let query_params = vec![("roleId", role_id.to_string())];

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(role),
                &role_id,
            )
            .await
    }

    /// Deletes a custom role.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.roles/delete
    async fn delete_role(&self, role_name: String) -> Result<Role> {
        let path = self.build_role_path(role_name.clone());

        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &role_name)
            .await
    }

    /// Gets a role.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.roles/get
    async fn get_role(&self, role_name: String) -> Result<Role> {
        let path = self.build_role_path(role_name.clone());

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &role_name)
            .await
    }

    /// Updates (patches) a custom role.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.roles/patch
    async fn patch_role(
        &self,
        role_name: String,
        role: Role,
        update_mask: Option<String>,
    ) -> Result<Role> {
        let path = self.build_role_path(role_name.clone());
        let mut query_params = Vec::new();
        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask.to_string()));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(role),
                &role_name,
            )
            .await
    }

    /// Generates an access token for a service account.
    /// See: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts/generateAccessToken
    async fn generate_access_token(
        &self,
        service_account_name: String,
        request: GenerateAccessTokenRequest,
    ) -> Result<GenerateAccessTokenResponse> {
        let encoded_name = urlencoding::encode(&service_account_name).into_owned();
        let path = format!(
            "projects/-/serviceAccounts/{}:generateAccessToken",
            encoded_name
        );

        self.credentials_base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request),
                &service_account_name,
            )
            .await
    }
}

// --- Data Structures ---

/// Represents an IAM policy.
/// https://cloud.google.com/iam/docs/reference/rest/v1/Policy
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IamPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // Typically "storage#iamPolicy" for GCS, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub role: String,
    #[builder(default)]
    pub members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<Expr>,
}

/// Represents a CEL expression.
/// https://cloud.google.com/iam/docs/reference/rest/v1/Expr
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Expr {
    pub expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// Represents a Service Account resource.
/// Based on: https://cloud.google.com/iam/docs/reference/rest/v1/projects.serviceAccounts#ServiceAccount
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    /// The resource name of the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Output only. The ID of the project that owns the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    /// Output only. The unique, stable numeric ID for the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,

    /// Output only. The email address of the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Optional. A user-specified, human-readable name for the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Optional. A user-specified, human-readable description of the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Output only. The OAuth 2.0 client ID for the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth2_client_id: Option<String>,

    /// Output only. Whether the service account is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Deprecated. Do not use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// Request message for creating a service account.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "snake_case")]
pub struct CreateServiceAccountRequest {
    /// The service account to create.
    pub service_account: ServiceAccount,
}

/// Request message for setting IAM policy.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetIamPolicyRequest {
    /// The policy to be applied.
    pub policy: IamPolicy,
}

/// Request message for updating a service account.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PatchServiceAccountRequest {
    /// The service account to update.
    pub service_account: ServiceAccount,

    /// The update mask for the service account.
    pub update_mask: Option<String>,
}

/// Represents the launch stage of a role.
/// Based on: https://cloud.google.com/iam/docs/reference/rest/v1/organizations.roles#Role.RoleLaunchStage
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoleLaunchStage {
    /// The user has indicated this role is currently in an Alpha phase.
    Alpha,
    /// The user has indicated this role is currently in a Beta phase.
    Beta,
    /// The user has indicated this role is generally available.
    Ga,
    /// The user has indicated this role is being deprecated.
    Deprecated,
    /// This role is disabled and will not contribute permissions to any principals.
    Disabled,
    /// The user has indicated this role is currently in an EAP phase.
    Eap,
}

/// Represents a Role resource.
/// Based on: https://cloud.google.com/iam/docs/reference/rest/v1/projects.roles#Role
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    /// The name of the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional. A human-readable title for the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Optional. A human-readable description for the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The names of the permissions this role grants when bound in an IAM policy.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub included_permissions: Vec<String>,

    /// The current launch stage of the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<RoleLaunchStage>,

    /// Used to perform a consistent read-modify-write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Output only. The current deleted state of the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
}

/// Request message for creating a role.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    /// The role to create.
    pub role: Role,
}

/// Request message for generating an access token.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAccessTokenRequest {
    /// The OAuth 2.0 scopes that define the access token's permissions.
    pub scope: Vec<String>,

    /// Optional. The sequence of service accounts in a delegation chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegates: Option<Vec<String>>,

    /// Optional. The desired lifetime duration of the access token (max 3600s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<String>,
}

/// Response message for generating an access token.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAccessTokenResponse {
    /// The generated access token.
    pub access_token: String,

    /// The expiration time of the access token in RFC3339 format.
    pub expire_time: String,
}
