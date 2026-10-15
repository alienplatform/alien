use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::authorization_role_assignments::RoleAssignment;
use crate::azure::models::authorization_role_definitions::RoleDefinition;
use crate::azure::token_cache::AzureTokenCache;
use crate::azure::AzureClientConfig;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::{Client, Method, StatusCode, Url};
use serde::Deserialize;
use std::collections::HashSet;

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
    /// Subscription scope
    Subscription,
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
            Scope::Subscription => {
                format!("subscriptions/{}", client_config.subscription_id)
            }
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

    /// Convert the scope to a canonical Azure Resource Manager resource ID.
    ///
    /// ARM URLs in this client are built from a relative scope string, but role
    /// assignment payloads require the `scope` property to be the canonical ARM
    /// resource ID with a leading slash.
    pub fn to_resource_id_string(&self, client_config: &AzureClientConfig) -> String {
        format!(
            "/{}",
            self.to_scope_string(client_config).trim_start_matches('/')
        )
    }
}

// -----------------------------------------------------------------------------
// Authorization client struct
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureAuthorizationClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureAuthorizationClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
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
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(self.token_cache.config());
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
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(self.token_cache.config());
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
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(self.token_cache.config());
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
            .token_cache
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
            .token_cache
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
            .token_cache
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
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let scope_string = scope.to_scope_string(self.token_cache.config());

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

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RoleAssignmentListResponse {
            value: Vec<RoleAssignment>,
            next_link: Option<String>,
        }

        let origin = Url::parse(&url)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                message: "Invalid role-assignment endpoint".to_string(),
                field_name: None,
            })?
            .origin();
        let mut next_url = Some(url);
        let mut visited = HashSet::new();
        let mut assignments = Vec::new();
        while let Some(url) = next_url {
            let parsed = Url::parse(&url)
                .into_alien_error()
                .context(ErrorData::InvalidInput {
                    message: "Invalid role-assignment continuation URL".to_string(),
                    field_name: Some("nextLink".to_string()),
                })?;
            // Never send the bearer token to a different origin or follow a cycle.
            if parsed.origin() != origin
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.fragment().is_some()
                || !visited.insert(parsed.to_string())
            {
                return Err(AlienError::new(ErrorData::InvalidInput {
                    message: "Unsafe or repeated role-assignment continuation URL".to_string(),
                    field_name: Some("nextLink".to_string()),
                }));
            }
            let request = AzureRequestBuilder::new(Method::GET, url.clone())
                .content_length("")
                .build()?;
            let signed = self.base.sign_request(request, &bearer_token).await?;
            let response = self
                .base
                .execute_request(signed, "ListRoleAssignments", &scope_string)
                .await?;
            let page: RoleAssignmentListResponse = response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: "Azure ListRoleAssignments: invalid response page".to_string(),
                    url,
                    http_status: 200,
                    http_request_text: None,
                    http_response_text: None,
                })?;
            assignments.extend(page.value.into_iter().filter(|assignment| {
                role_definition_id.as_ref().is_none_or(|id| {
                    assignment
                        .properties
                        .as_ref()
                        .is_some_and(|props| &props.role_definition_id == id)
                })
            }));
            next_url = page.next_link;
        }
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
        let scope_string = scope.to_scope_string(self.token_cache.config());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::azure::{AzureCredentials, ServiceOverrides};
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;
    use std::collections::HashMap;

    const ASSIGNMENTS_PATH: &str = "/subscriptions/sub-123/resourceGroups/rg-1/providers/Microsoft.Authorization/roleAssignments";

    fn test_client(server: &MockServer) -> AzureAuthorizationClient {
        let mut config = test_config();
        config.service_overrides = Some(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureAuthorizationClient::new(Client::new(), AzureTokenCache::new(config))
    }

    #[tokio::test]
    async fn role_assignments_include_and_filter_all_pages() {
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(ASSIGNMENTS_PATH)
                    .query_param("api-version", "2022-04-01")
                    .query_param("$filter", "atScope()");
                then.status(200).body(
                    json!({"value": [{"id":"first", "properties": {
                "principalId":"principal", "roleDefinitionId":"other-role"
            }}], "nextLink": format!("{}/page2?cursor=next", server.base_url())})
                    .to_string(),
                );
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/page2")
                    .query_param("cursor", "next")
                    .header("authorization", "Bearer token");
                then.status(200).body(
                    json!({"value": [{"id":"second", "properties": {
                        "principalId":"principal", "roleDefinitionId":"wanted-role"
                    }}]})
                    .to_string(),
                );
            })
            .await;
        let client = test_client(&server);
        let scope = Scope::ResourceGroup {
            resource_group_name: "rg-1".into(),
        };
        let all = client.list_role_assignments(&scope, None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id.as_deref(), Some("first"));
        assert_eq!(all[1].id.as_deref(), Some("second"));
        let filtered = client
            .list_role_assignments(&scope, Some("wanted-role".into()))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id.as_deref(), Some("second"));
        first.assert_calls_async(2).await;
        second.assert_calls_async(2).await;
    }

    #[tokio::test]
    async fn role_assignment_invalid_later_page_does_not_return_partial_inventory() {
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method(GET).path(ASSIGNMENTS_PATH);
                then.status(200).body(
                    json!({"value": [{"id":"first", "properties": {
                "principalId":"principal", "roleDefinitionId":"wanted-role"
            }}], "nextLink": format!("{}/page2", server.base_url())})
                    .to_string(),
                );
            })
            .await;
        let invalid = server
            .mock_async(|when, then| {
                when.method(GET).path("/page2");
                then.status(200).body("{}");
            })
            .await;
        assert!(test_client(&server)
            .list_role_assignments(
                &Scope::ResourceGroup {
                    resource_group_name: "rg-1".into()
                },
                None
            )
            .await
            .is_err());
        first.assert_calls_async(1).await;
        invalid.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn role_assignment_pagination_rejects_foreign_origins_and_cycles() {
        for foreign in [false, true] {
            let server = MockServer::start_async().await;
            let other = MockServer::start_async().await;
            let next = format!(
                "{}/page2",
                if foreign {
                    other.base_url()
                } else {
                    server.base_url()
                }
            );
            let first = server
                .mock_async(|when, then| {
                    when.method(GET).path(ASSIGNMENTS_PATH);
                    then.status(200)
                        .body(json!({"value": [], "nextLink": next}).to_string());
                })
                .await;
            let repeated = server
                .mock_async(|when, then| {
                    when.method(GET).path("/page2");
                    then.status(200)
                        .body(json!({"value": [], "nextLink": next}).to_string());
                })
                .await;
            let forbidden = other
                .mock_async(|when, then| {
                    when.method(GET);
                    then.status(200).body(json!({"value": []}).to_string());
                })
                .await;
            let error = test_client(&server)
                .list_role_assignments(
                    &Scope::ResourceGroup {
                        resource_group_name: "rg-1".into(),
                    },
                    None,
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, "INVALID_INPUT");
            first.assert_calls_async(1).await;
            repeated
                .assert_calls_async(if foreign { 0 } else { 1 })
                .await;
            forbidden.assert_calls_async(0).await;
        }
    }

    fn test_config() -> AzureClientConfig {
        AzureClientConfig {
            subscription_id: "sub-123".to_string(),
            tenant_id: "tenant-123".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "token".to_string(),
            },
            service_overrides: None,
        }
    }

    #[test]
    fn scope_strings_distinguish_relative_paths_from_arm_resource_ids() {
        let config = test_config();

        let rg_scope = Scope::ResourceGroup {
            resource_group_name: "rg-1".to_string(),
        };
        assert_eq!(
            rg_scope.to_scope_string(&config),
            "subscriptions/sub-123/resourceGroups/rg-1"
        );
        assert_eq!(
            rg_scope.to_resource_id_string(&config),
            "/subscriptions/sub-123/resourceGroups/rg-1"
        );

        let resource_scope = Scope::Resource {
            resource_group_name: "rg-1".to_string(),
            resource_provider: "Microsoft.ServiceBus".to_string(),
            parent_resource_path: None,
            resource_type: "namespaces".to_string(),
            resource_name: "bus-1".to_string(),
        };
        assert_eq!(
            resource_scope.to_scope_string(&config),
            "subscriptions/sub-123/resourceGroups/rg-1/providers/Microsoft.ServiceBus/namespaces/bus-1"
        );
        assert_eq!(
            resource_scope.to_resource_id_string(&config),
            "/subscriptions/sub-123/resourceGroups/rg-1/providers/Microsoft.ServiceBus/namespaces/bus-1"
        );
    }
}
