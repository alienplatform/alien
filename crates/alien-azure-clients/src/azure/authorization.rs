use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::authorization_role_assignments::RoleAssignment;
use crate::azure::models::authorization_role_definitions::RoleDefinition;
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Authorization API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait AuthorizationApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
        role_definition: &RoleDefinition,
    ) -> Result<RoleDefinition>;

    async fn delete_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<Option<RoleDefinition>>;

    async fn get_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<RoleDefinition>;

    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignment,
    ) -> Result<RoleAssignment>;

    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<Option<RoleAssignment>>;

    async fn get_role_assignment_by_id(&self, role_assignment_id: String)
        -> Result<RoleAssignment>;

    async fn list_role_assignments(
        &self,
        scope: &Scope,
        role_definition_id: Option<String>,
    ) -> Result<Vec<RoleAssignment>>;

    fn build_role_assignment_id(&self, scope: &Scope, role_assignment_name: String) -> String;
    fn build_resource_group_role_assignment_id(
        &self,
        resource_group_name: String,
        role_assignment_name: String,
    ) -> String;
    fn build_resource_role_assignment_id(
        &self,
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
        role_assignment_name: String,
    ) -> String;
}

// -----------------------------------------------------------------------------
// Scope enum for role definitions and assignments
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Scope {
    /// Resource group scope
    ResourceGroup { resource_group_name: String },
    /// Individual resource scope
    Resource {
        resource_group_name: String,
        resource_provider: String,
        /// Optional parent resource path (e.g., "sites/mysite" for a slot under a web app)
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
    },
}

impl Scope {
    /// Convert the scope to the Azure Resource Manager scope string format
    pub fn to_scope_string(&self, client_config: &AzureClientConfig) -> String {
        match self {
            Scope::ResourceGroup {
                resource_group_name,
            } => {
                format!(
                    "subscriptions/{}/resourceGroups/{}",
                    client_config.subscription_id, resource_group_name
                )
            }
            Scope::Resource {
                resource_group_name,
                resource_provider,
                parent_resource_path,
                resource_type,
                resource_name,
            } => {
                let base = format!(
                    "subscriptions/{}/resourceGroups/{}/providers/{}",
                    client_config.subscription_id, resource_group_name, resource_provider
                );

                if let Some(parent_path) = parent_resource_path {
                    format!(
                        "{}/{}/{}/{}",
                        base, parent_path, resource_type, resource_name
                    )
                } else {
                    format!("{}/{}/{}", base, resource_type, resource_name)
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Authorization client struct
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureAuthorizationClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl AzureAuthorizationClient {
    pub fn new(client: Client, client_config: AzureClientConfig) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = client_config.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, client_config.clone()),
            client_config,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AuthorizationApi for AzureAuthorizationClient {
    /// Create or update a role definition
    async fn create_or_update_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
        role_definition: &RoleDefinition,
    ) -> Result<RoleDefinition> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(&self.client_config);
        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                scope_string, role_definition_id
            ),
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let body = serde_json::to_string(role_definition)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize role definition".to_string(),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateRoleDefinition", &role_definition_id)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Azure CreateOrUpdateRoleDefinition: failed to read response body"
                    ),
                })?;

        let role_definition: RoleDefinition = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateRoleDefinition: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(role_definition)
    }

    /// Delete a role definition
    async fn delete_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<Option<RoleDefinition>> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(&self.client_config);
        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                scope_string, role_definition_id
            ),
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "DeleteRoleDefinition", &role_definition_id)
            .await?;

        let status = resp.status();
        let role_definition = if status == StatusCode::NO_CONTENT {
            None
        } else {
            let response_body =
                resp.text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: format!(
                            "Azure DeleteRoleDefinition: failed to read response body"
                        ),
                    })?;

            if response_body.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(&response_body)
                        .into_alien_error()
                        .context(ErrorData::HttpResponseError {
                            message: format!(
                                "Azure DeleteRoleDefinition: JSON parse error. Body: {}",
                                response_body
                            ),
                            url: url.to_string(),
                            http_status: status.as_u16(),
                            http_request_text: None,
                            http_response_text: Some(response_body),
                        })?,
                )
            }
        };

        Ok(role_definition)
    }

    /// Get a role definition by ID
    async fn get_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<RoleDefinition> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(&self.client_config);
        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                scope_string, role_definition_id
            ),
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetRoleDefinition", &role_definition_id)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure GetRoleDefinition: failed to read response body"),
                })?;

        let role_definition: RoleDefinition = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetRoleDefinition: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(role_definition)
    }

    /// Create or update a role assignment by ID
    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignment,
    ) -> Result<RoleAssignment> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &role_assignment_id,
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let body = serde_json::to_string(role_assignment)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize role assignment: {}",
                    role_assignment_id
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateRoleAssignment", &role_assignment_id)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Azure CreateOrUpdateRoleAssignment: failed to read response body"
                    ),
                })?;

        let role_assignment: RoleAssignment = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateOrUpdateRoleAssignment: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(role_assignment)
    }

    /// Delete a role assignment by ID
    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<Option<RoleAssignment>> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &role_assignment_id,
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "DeleteRoleAssignment", &role_assignment_id)
            .await?;

        let status = resp.status();
        let role_assignment = if status == StatusCode::NO_CONTENT {
            None
        } else {
            let response_body =
                resp.text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: format!(
                            "Azure DeleteRoleAssignment: failed to read response body"
                        ),
                    })?;

            if response_body.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(&response_body)
                        .into_alien_error()
                        .context(ErrorData::HttpResponseError {
                            message: format!(
                                "Azure DeleteRoleAssignment: JSON parse error. Body: {}",
                                response_body
                            ),
                            url: url.to_string(),
                            http_status: status.as_u16(),
                            http_request_text: None,
                            http_response_text: Some(response_body),
                        })?,
                )
            }
        };

        Ok(role_assignment)
    }

    /// Get a role assignment by ID
    async fn get_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<RoleAssignment> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &role_assignment_id,
            Some(vec![("api-version", "2022-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetRoleAssignment", &role_assignment_id)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure GetRoleAssignment: failed to read response body"),
                })?;

        let role_assignment: RoleAssignment = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetRoleAssignment: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(role_assignment)
    }

    /// List role assignments at a scope, optionally filtered by role definition ID
    ///
    /// # Arguments
    /// * `scope` - The scope to list role assignments for
    /// * `role_definition_id` - Optional role definition ID to filter by
    ///
    /// # Returns
    /// Vector of role assignments, filtered by role definition ID if provided
    async fn list_role_assignments(
        &self,
        scope: &Scope,
        role_definition_id: Option<String>,
    ) -> Result<Vec<RoleAssignment>> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(&self.client_config);

        // Build query parameters
        let query_params = vec![
            ("api-version", "2022-04-01".into()),
            ("$filter", "atScope()".into()),
        ];

        let url = self.base.build_url(
            &format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments",
                scope_string
            ),
            Some(query_params),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListRoleAssignments", &scope_string)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Azure ListRoleAssignments: failed to read response body"),
                })?;

        #[derive(Deserialize)]
        struct RoleAssignmentListResponse {
            value: Vec<RoleAssignment>,
        }

        let response: RoleAssignmentListResponse = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListRoleAssignments: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        // Filter by role definition ID if provided
        let assignments = if let Some(role_def_id) = role_definition_id {
            response
                .value
                .into_iter()
                .filter(|assignment| {
                    assignment
                        .properties
                        .as_ref()
                        .map_or(false, |props| props.role_definition_id == role_def_id)
                })
                .collect()
        } else {
            response.value
        };

        Ok(assignments)
    }

    /// Helper method to construct a full role assignment ID from a scope
    ///
    /// # Arguments
    /// * `scope` - The scope (ResourceGroup or Resource)
    /// * `role_assignment_name` - The name/ID of the role assignment (usually a GUID)
    ///
    /// # Returns
    /// Full role assignment ID in the format: /{scope}/providers/Microsoft.Authorization/roleAssignments/{roleAssignmentName}
    fn build_role_assignment_id(&self, scope: &Scope, role_assignment_name: String) -> String {
        let scope_string = scope.to_scope_string(&self.client_config);
        format!(
            "/{}/providers/Microsoft.Authorization/roleAssignments/{}",
            scope_string, role_assignment_name
        )
    }

    /// Helper method to construct a resource group-scoped role assignment ID
    ///
    /// # Arguments
    /// * `resource_group_name` - The resource group name
    /// * `role_assignment_name` - The name/ID of the role assignment (usually a GUID)
    fn build_resource_group_role_assignment_id(
        &self,
        resource_group_name: String,
        role_assignment_name: String,
    ) -> String {
        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.to_string(),
        };
        self.build_role_assignment_id(&scope, role_assignment_name)
    }

    /// Helper method to construct a resource-scoped role assignment ID
    ///
    /// # Arguments
    /// * `resource_group_name` - The resource group name
    /// * `resource_provider` - The resource provider (e.g., "Microsoft.Storage")
    /// * `parent_resource_path` - Optional parent resource path (e.g., "sites/mysite" for a slot under a web app)
    /// * `resource_type` - The resource type (e.g., "storageAccounts")
    /// * `resource_name` - The resource name
    /// * `role_assignment_name` - The name/ID of the role assignment (usually a GUID)
    fn build_resource_role_assignment_id(
        &self,
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
        role_assignment_name: String,
    ) -> String {
        let scope = Scope::Resource {
            resource_group_name: resource_group_name.to_string(),
            resource_provider: resource_provider.to_string(),
            parent_resource_path: parent_resource_path.map(|s| s.to_string()),
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        };
        self.build_role_assignment_id(&scope, role_assignment_name)
    }
}
