#![cfg(all(test, feature = "azure"))]

use alien_azure_clients::authorization::{AuthorizationApi, AzureAuthorizationClient, Scope};
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct TrackedRoleDefinition {
    scope: Scope,
    role_definition_id: String,
}

struct AuthorizationTestContext {
    authorization_client: AzureAuthorizationClient,
    subscription_id: String,
    resource_group_name: String,
    created_role_definitions: Mutex<Vec<TrackedRoleDefinition>>,
    created_role_assignments: Mutex<HashSet<String>>,
}

impl AsyncTestContext for AuthorizationTestContext {
    async fn setup() -> AuthorizationTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok(); // Initialize tracing

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        // Create platform config with service principal credentials
        let client_config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        info!(
            "🔧 Using subscription: {} and resource group: {} for authorization testing",
            subscription_id, resource_group_name
        );

        let client = Client::new();
        AuthorizationTestContext {
            authorization_client: AzureAuthorizationClient::new(
                client,
                AzureTokenCache::new(client_config),
            ),
            subscription_id,
            resource_group_name,
            created_role_definitions: Mutex::new(Vec::new()),
            created_role_assignments: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Authorization test cleanup...");

        // Cleanup role assignments first (they depend on role definitions)
        let role_assignments_to_cleanup = {
            let assignments = self.created_role_assignments.lock().unwrap();
            assignments.clone()
        };

        for assignment_id in role_assignments_to_cleanup {
            self.cleanup_role_assignment(&assignment_id).await;
        }

        // Then cleanup role definitions
        let role_definitions_to_cleanup = {
            let definitions = self.created_role_definitions.lock().unwrap();
            definitions.clone()
        };

        for tracked_definition in role_definitions_to_cleanup {
            self.cleanup_role_definition(&tracked_definition).await;
        }

        info!("✅ Authorization test cleanup completed");
    }
}

impl AuthorizationTestContext {
    fn track_role_definition(&self, scope: &Scope, role_definition_id: &str) {
        let tracked = TrackedRoleDefinition {
            scope: scope.clone(),
            role_definition_id: role_definition_id.to_string(),
        };
        let mut definitions = self.created_role_definitions.lock().unwrap();
        definitions.push(tracked.clone());

        let scope_string = scope.to_scope_string(self.authorization_client.token_cache.config());
        let full_id = format!(
            "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            scope_string.trim_start_matches('/'),
            role_definition_id
        );
        info!("📝 Tracking role definition for cleanup: {}", full_id);
    }

    fn untrack_role_definition(&self, scope: &Scope, role_definition_id: &str) {
        let mut definitions = self.created_role_definitions.lock().unwrap();
        definitions.retain(|tracked| {
            !(tracked
                .scope
                .to_scope_string(self.authorization_client.token_cache.config())
                == scope.to_scope_string(self.authorization_client.token_cache.config())
                && tracked.role_definition_id == role_definition_id)
        });

        let scope_string = scope.to_scope_string(self.authorization_client.token_cache.config());
        let full_id = format!(
            "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            scope_string.trim_start_matches('/'),
            role_definition_id
        );
        info!(
            "✅ Role definition {} successfully cleaned up and untracked",
            full_id
        );
    }

    fn track_role_assignment(&self, role_assignment_id: &str) {
        let mut assignments = self.created_role_assignments.lock().unwrap();
        assignments.insert(role_assignment_id.to_string());
        info!(
            "📝 Tracking role assignment for cleanup: {}",
            role_assignment_id
        );
    }

    fn untrack_role_assignment(&self, role_assignment_id: &str) {
        let mut assignments = self.created_role_assignments.lock().unwrap();
        assignments.remove(role_assignment_id);
        info!(
            "✅ Role assignment {} successfully cleaned up and untracked",
            role_assignment_id
        );
    }

    async fn cleanup_role_definition(&self, tracked: &TrackedRoleDefinition) {
        let scope_string = tracked
            .scope
            .to_scope_string(self.authorization_client.token_cache.config());
        let full_id = format!(
            "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            scope_string.trim_start_matches('/'),
            tracked.role_definition_id
        );
        info!("🧹 Cleaning up role definition: {}", full_id);

        match self
            .authorization_client
            .delete_role_definition(&tracked.scope, tracked.role_definition_id.clone())
            .await
        {
            Ok(_) => {
                info!("✅ Role definition {} deleted successfully", full_id);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Role definition {} was already deleted", full_id);
            }
            Err(e) => {
                warn!(
                    "Failed to delete role definition {} during cleanup: {:?}",
                    full_id, e
                );
            }
        }
    }

    async fn cleanup_role_assignment(&self, assignment_id: &str) {
        info!("🧹 Cleaning up role assignment: {}", assignment_id);

        match self
            .authorization_client
            .delete_role_assignment_by_id(assignment_id.to_string())
            .await
        {
            Ok(_) => {
                info!("✅ Role assignment {} deleted successfully", assignment_id);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Role assignment {} was already deleted", assignment_id);
            }
            Err(e) => {
                warn!(
                    "Failed to delete role assignment {} during cleanup: {:?}",
                    assignment_id, e
                );
            }
        }
    }

    fn generate_unique_role_definition_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    fn generate_unique_role_assignment_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    fn resource_group_scope(&self) -> Scope {
        Scope::ResourceGroup {
            resource_group_name: self.resource_group_name.clone(),
        }
    }

    fn test_resource_scope(&self) -> Scope {
        Scope::Resource {
            resource_group_name: self.resource_group_name.clone(),
            resource_provider: "Microsoft.Storage".to_string(),
            parent_resource_path: None,
            resource_type: "storageAccounts".to_string(),
            resource_name: "alienteststorage".to_string(),
        }
    }

    async fn create_test_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: &str,
        role_name: &str,
    ) -> Result<RoleDefinition, Error> {
        let scope_string = scope.to_scope_string(self.authorization_client.token_cache.config());
        let role_definition = RoleDefinition {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleDefinitionProperties {
                role_name: Some(role_name.to_string()),
                type_: Some("CustomRole".to_string()),
                description: Some("Test role created by alien-infra tests".to_string()),
                assignable_scopes: vec![format!("/{}", scope_string.trim_start_matches('/'))],
                permissions: vec![Permission {
                    actions: vec!["Microsoft.Storage/storageAccounts/read".to_string()],
                    not_actions: vec![],
                    data_actions: vec![],
                    not_data_actions: vec![],
                }],
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        let result = self
            .authorization_client
            .create_or_update_role_definition(
                scope,
                role_definition_id.to_string(),
                &role_definition,
            )
            .await;

        if result.is_ok() {
            self.track_role_definition(scope, role_definition_id);
        }

        result
    }

    async fn create_test_role_assignment(
        &self,
        role_assignment_id: &str,
        principal_id: &str,
        role_definition_id: &str,
        scope: &Scope,
    ) -> Result<RoleAssignment, Error> {
        let scope_string = scope.to_scope_string(self.authorization_client.token_cache.config());
        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: format!(
                    "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                    scope_string.trim_start_matches('/'),
                    role_definition_id
                ),
                scope: Some(format!("/{}", scope_string.trim_start_matches('/'))),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                condition: None,
                condition_version: None,
                delegated_managed_identity_resource_id: None,
                description: Some("Test role assignment created by alien-infra tests".to_string()),
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        let full_assignment_id = self
            .authorization_client
            .build_role_assignment_id(scope, role_assignment_id.to_string());

        let result = self
            .authorization_client
            .create_or_update_role_assignment_by_id(full_assignment_id.clone(), &role_assignment)
            .await;

        if result.is_ok() {
            self.track_role_assignment(&full_assignment_id);
        }

        result
    }
}

// -------------------------------------------------------------------------
// Role Definition tests
// -------------------------------------------------------------------------

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_create_and_delete_role_definition(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let scope = ctx.resource_group_scope();

    // Create role definition
    let create_result = ctx
        .create_test_role_definition(&scope, &role_definition_id, &role_name)
        .await;
    assert!(
        create_result.is_ok(),
        "Failed to create role definition: {:?}",
        create_result.err()
    );

    let created_role = create_result.unwrap();
    let properties = created_role
        .properties
        .as_ref()
        .expect("Role definition should have properties");
    assert_eq!(properties.role_name.as_ref(), Some(&role_name));
    assert_eq!(properties.type_.as_ref(), Some(&"CustomRole".to_string()));

    // Delete role definition
    let delete_result = ctx
        .authorization_client
        .delete_role_definition(&scope, role_definition_id.clone())
        .await;
    assert!(
        delete_result.is_ok(),
        "Failed to delete role definition: {:?}",
        delete_result.err()
    );
    ctx.untrack_role_definition(&scope, &role_definition_id);

    // Verify role definition is deleted by trying to get it
    // Note: Azure has eventual consistency, so the resource might still be returned for some time after deletion
    let get_after_delete_result = ctx
        .authorization_client
        .get_role_definition(&scope, role_definition_id.clone())
        .await;
    match get_after_delete_result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            info!("✅ Role definition correctly returned RemoteResourceNotFound after deletion");
        }
        Ok(role_definition) => {
            // Azure still returns the role definition due to eventual consistency - this is acceptable
            info!("⚠️ Role definition still returned after deletion due to Azure eventual consistency");
            let properties = role_definition
                .properties
                .as_ref()
                .expect("Role definition should have properties");
            assert_eq!(
                properties.role_name.as_ref(),
                Some(&role_name),
                "Role definition content should be unchanged"
            );
        }
        Err(other_error) => {
            panic!("Expected either Ok or RemoteResourceNotFound after deleting role definition, got {:?}", other_error);
        }
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_get_role_definition(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let scope = ctx.resource_group_scope();

    // Create role definition first
    let created_role = ctx
        .create_test_role_definition(&scope, &role_definition_id, &role_name)
        .await
        .expect("Failed to create role definition for get test");

    // Get role definition
    let get_result = ctx
        .authorization_client
        .get_role_definition(&scope, role_definition_id.clone())
        .await;
    assert!(
        get_result.is_ok(),
        "Failed to get role definition: {:?}",
        get_result.err()
    );

    let retrieved_role = get_result.unwrap();
    let properties = retrieved_role
        .properties
        .as_ref()
        .expect("Role definition should have properties");

    assert_eq!(properties.role_name.as_ref(), Some(&role_name));
    assert_eq!(properties.type_.as_ref(), Some(&"CustomRole".to_string()));
    assert!(properties.description.is_some());
    assert!(!properties.assignable_scopes.is_empty());
    assert!(!properties.permissions.is_empty());
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_create_role_definition_already_exists(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let scope = ctx.resource_group_scope();

    // Create role definition first time
    let create_first_result = ctx
        .create_test_role_definition(&scope, &role_definition_id, &role_name)
        .await;
    assert!(
        create_first_result.is_ok(),
        "Failed to create role definition initially: {:?}",
        create_first_result.err()
    );

    // Attempt to create the same role definition again (should succeed as it's an update)
    let scope_string = scope.to_scope_string(ctx.authorization_client.token_cache.config());
    let role_definition = RoleDefinition {
        id: None,
        name: None,
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(format!("{}-updated", role_name)),
            type_: Some("CustomRole".to_string()),
            description: Some("Updated test role".to_string()),
            assignable_scopes: vec![format!("/{}", scope_string.trim_start_matches('/'))],
            permissions: vec![Permission {
                actions: vec!["Microsoft.Storage/storageAccounts/read".to_string()],
                not_actions: vec![],
                data_actions: vec![],
                not_data_actions: vec![],
            }],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let create_second_result = ctx
        .authorization_client
        .create_or_update_role_definition(&scope, role_definition_id.clone(), &role_definition)
        .await;

    // This should succeed because it's an update operation
    assert!(
        create_second_result.is_ok(),
        "Role definition update should succeed: {:?}",
        create_second_result.err()
    );

    if let Ok(role_definition) = create_second_result {
        let properties = role_definition
            .properties
            .as_ref()
            .expect("Role definition should have properties");
        assert_eq!(
            properties.role_name.as_ref(),
            Some(&format!("{}-updated", role_name))
        );
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_delete_non_existent_role_definition(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let scope = ctx.resource_group_scope();

    let result = ctx
        .authorization_client
        .delete_role_definition(&scope, role_definition_id.clone())
        .await;
    // Azure may return either Ok (idempotent delete) or RemoteResourceNotFound
    match result {
        Ok(role_definition) => {
            // Azure returned OK for idempotent delete - this is valid behavior
            assert!(
                role_definition.is_none(),
                "Role definition should be None for non-existent deletion"
            );
            info!(
                "✅ Delete non-existent role definition returned OK (idempotent delete behavior)"
            );
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Azure returned not found - also valid behavior
            info!("✅ Delete non-existent role definition returned RemoteResourceNotFound as expected");
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound after deleting non-existent role definition, got {:?}", other);
        }
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_get_non_existent_role_definition(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let scope = ctx.resource_group_scope();

    let result = ctx
        .authorization_client
        .get_role_definition(&scope, role_definition_id.clone())
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_role_definition_with_complex_permissions(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-complex-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let rg_scope = ctx.resource_group_scope();
    let resource_scope = ctx.test_resource_scope();

    let rg_scope_string = rg_scope.to_scope_string(ctx.authorization_client.token_cache.config());
    let resource_scope_string =
        resource_scope.to_scope_string(ctx.authorization_client.token_cache.config());

    let role_definition = RoleDefinition {
        id: None,
        name: None,
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(role_name.clone()),
            type_: Some("CustomRole".to_string()),
            description: Some("Complex test role with multiple permissions".to_string()),
            assignable_scopes: vec![
                format!("/{}", rg_scope_string.trim_start_matches('/')),
                format!("/{}", resource_scope_string.trim_start_matches('/')),
            ],
            permissions: vec![Permission {
                actions: vec![
                    "Microsoft.Storage/storageAccounts/read".to_string(),
                    "Microsoft.Storage/storageAccounts/listKeys/action".to_string(),
                ],
                not_actions: vec!["Microsoft.Storage/storageAccounts/delete".to_string()],
                data_actions: vec![
                    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"
                        .to_string(),
                ],
                not_data_actions: vec![
                    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/delete"
                        .to_string(),
                ],
            }],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let create_result = ctx
        .authorization_client
        .create_or_update_role_definition(&rg_scope, role_definition_id.clone(), &role_definition)
        .await;

    if create_result.is_ok() {
        ctx.track_role_definition(&rg_scope, &role_definition_id);
    }

    assert!(
        create_result.is_ok(),
        "Failed to create complex role definition: {:?}",
        create_result.err()
    );

    let created_role = create_result.unwrap();
    let properties = created_role
        .properties
        .as_ref()
        .expect("Role definition should have properties");

    // Verify complex permissions were preserved
    assert_eq!(properties.permissions.len(), 1);
    let permission = &properties.permissions[0];
    assert_eq!(permission.actions.len(), 2);
    assert_eq!(permission.not_actions.len(), 1);
    assert_eq!(permission.data_actions.len(), 1);
    assert_eq!(permission.not_data_actions.len(), 1);
    assert_eq!(properties.assignable_scopes.len(), 2);
}

// -------------------------------------------------------------------------
// Role Assignment tests
// -------------------------------------------------------------------------

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_role_assignment_helper_methods(ctx: &mut AuthorizationTestContext) {
    let assignment_name = "test-assignment";
    let resource_group = "my-resource-group";

    // Test general build_role_assignment_id with resource group scope
    let rg_scope = Scope::ResourceGroup {
        resource_group_name: resource_group.to_string(),
    };
    let general_id = ctx
        .authorization_client
        .build_role_assignment_id(&rg_scope, assignment_name.to_string());
    let expected_general = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Authorization/roleAssignments/{}",
        ctx.subscription_id, resource_group, assignment_name
    );
    assert_eq!(general_id, expected_general);

    // Test resource group-scoped helper
    let rg_id_result = ctx
        .authorization_client
        .build_resource_group_role_assignment_id(
            resource_group.to_string(),
            assignment_name.to_string(),
        );
    let expected_rg = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Authorization/roleAssignments/{}",
        ctx.subscription_id, resource_group, assignment_name
    );
    assert_eq!(rg_id_result, expected_rg);

    // Test resource-scoped helper
    let resource_id_result = ctx.authorization_client.build_resource_role_assignment_id(
        resource_group.to_string(),
        "Microsoft.Storage".to_string(),
        None,
        "storageAccounts".to_string(),
        "mystorageaccount".to_string(),
        assignment_name.to_string(),
    );
    let expected_resource = format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/mystorageaccount/providers/Microsoft.Authorization/roleAssignments/{}", 
        ctx.subscription_id, resource_group, assignment_name);
    assert_eq!(resource_id_result, expected_resource);
}

// TODO: Uncomment these tests

/*
#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_create_and_delete_role_assignment(ctx: &mut AuthorizationTestContext) {
    // Create a role definition first
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!("alien-test-role-{}", Uuid::new_v4().as_simple().to_string().chars().take(8).collect::<String>());
    let scope = ctx.resource_group_scope();

    let _created_role = ctx.create_test_role_definition(&scope, &role_definition_id, &role_name).await
        .expect("Failed to create role definition for assignment test");

    // Create role assignment
    let assignment_id = ctx.generate_unique_role_assignment_id();
    let principal_id = "YOUR_SERVICE_PRINCIPAL_OBJECT_ID"; // Replace with actual principal ID

    let create_result = ctx.create_test_role_assignment(&assignment_id, principal_id, &role_definition_id, &scope).await;
    assert!(create_result.is_ok(), "Failed to create role assignment: {:?}", create_result.err());

    let created_assignment = create_result.unwrap();
    assert_eq!(created_assignment.properties.principal_id, principal_id);

    // Delete role assignment
    let full_assignment_id = ctx.authorization_client.build_role_assignment_id(&scope, &assignment_id);
    let delete_result = ctx.authorization_client.delete_role_assignment_by_id(&full_assignment_id).await;
    assert!(delete_result.is_ok(), "Failed to delete role assignment: {:?}", delete_result.err());
    ctx.untrack_role_assignment(&full_assignment_id);

    // Verify role assignment is deleted
    let get_after_delete_result = ctx.authorization_client.get_role_assignment_by_id(&full_assignment_id).await;
    assert!(matches!(get_after_delete_result, Err(Error::RemoteResourceNotFound { .. })),
        "Expected RemoteResourceNotFound after deleting role assignment, got {:?}", get_after_delete_result);
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_get_role_assignment(ctx: &mut AuthorizationTestContext) {
    // Create a role definition and assignment first
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!("alien-test-role-{}", Uuid::new_v4().as_simple().to_string().chars().take(8).collect::<String>());
    let scope = ctx.resource_group_scope();

    let _created_role = ctx.create_test_role_definition(&scope, &role_definition_id, &role_name).await
        .expect("Failed to create role definition for get assignment test");

    let assignment_id = ctx.generate_unique_role_assignment_id();
    let principal_id = "YOUR_SERVICE_PRINCIPAL_OBJECT_ID"; // Replace with actual principal ID

    let _created_assignment = ctx.create_test_role_assignment(&assignment_id, principal_id, &role_definition_id, &scope).await
        .expect("Failed to create role assignment for get test");

    // Get role assignment
    let full_assignment_id = ctx.authorization_client.build_role_assignment_id(&scope, &assignment_id);
    let get_result = ctx.authorization_client.get_role_assignment_by_id(&full_assignment_id).await;
    assert!(get_result.is_ok(), "Failed to get role assignment: {:?}", get_result.err());

    let retrieved_assignment = get_result.unwrap();
    let retrieved_role_definition_id = retrieved_assignment.properties.role_definition_id.split('/').last().unwrap();

    assert_eq!(retrieved_assignment.properties.principal_id, principal_id);
    assert!(retrieved_assignment.properties.role_definition_id.contains(&retrieved_role_definition_id));
}
*/

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_get_non_existent_role_assignment(ctx: &mut AuthorizationTestContext) {
    let assignment_id = ctx.generate_unique_role_assignment_id();
    let scope = ctx.resource_group_scope();
    let full_assignment_id = ctx
        .authorization_client
        .build_role_assignment_id(&scope, assignment_id.clone());

    let result = ctx
        .authorization_client
        .get_role_assignment_by_id(full_assignment_id.clone())
        .await;
    match result {
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected result
        }
        other => panic!("Expected RemoteResourceNotFound, got {:?}", other),
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_delete_non_existent_role_assignment(ctx: &mut AuthorizationTestContext) {
    let assignment_id = ctx.generate_unique_role_assignment_id();
    let scope = ctx.resource_group_scope();
    let full_assignment_id = ctx
        .authorization_client
        .build_role_assignment_id(&scope, assignment_id.clone());

    let result = ctx
        .authorization_client
        .delete_role_assignment_by_id(full_assignment_id.clone())
        .await;
    // Azure may return either Ok (idempotent delete) or RemoteResourceNotFound
    match result {
        Ok(role_assignment) => {
            // Azure returned OK for idempotent delete - this is valid behavior
            assert!(
                role_assignment.is_none(),
                "Role assignment should be None for non-existent deletion"
            );
            info!(
                "✅ Delete non-existent role assignment returned OK (idempotent delete behavior)"
            );
        }
        Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Azure returned not found - also valid behavior
            info!("✅ Delete non-existent role assignment returned RemoteResourceNotFound as expected");
        }
        Err(other) => {
            panic!("Expected Ok or RemoteResourceNotFound after deleting non-existent role assignment, got {:?}", other);
        }
    }
}

// -------------------------------------------------------------------------
// Error scenario tests
// -------------------------------------------------------------------------

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_invalid_scope_format(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );

    // Create an invalid scope by using a resource group with invalid characters
    let invalid_scope = Scope::ResourceGroup {
        resource_group_name: "invalid-scope-format-with-invalid-chars!@#$%".to_string(),
    };

    let scope_string = invalid_scope.to_scope_string(ctx.authorization_client.token_cache.config());
    let role_definition = RoleDefinition {
        id: None,
        name: None,
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(role_name),
            type_: Some("CustomRole".to_string()),
            description: Some("Test role with invalid scope".to_string()),
            assignable_scopes: vec![format!("/{}", scope_string.trim_start_matches('/'))],
            permissions: vec![Permission {
                actions: vec!["Microsoft.Storage/storageAccounts/read".to_string()],
                not_actions: vec![],
                data_actions: vec![],
                not_data_actions: vec![],
            }],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let result = ctx
        .authorization_client
        .create_or_update_role_definition(
            &invalid_scope,
            role_definition_id.clone(),
            &role_definition,
        )
        .await;

    // This should fail with some kind of error (likely BadRequest or similar)
    assert!(
        result.is_err(),
        "Expected error for invalid scope format, got success"
    );
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_role_definition_with_invalid_permissions(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let scope = ctx.resource_group_scope();

    let scope_string = scope.to_scope_string(ctx.authorization_client.token_cache.config());
    let role_definition = RoleDefinition {
        id: None,
        name: None,
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(role_name),
            type_: Some("CustomRole".to_string()),
            description: Some("Test role with invalid permissions".to_string()),
            assignable_scopes: vec![format!("/{}", scope_string.trim_start_matches('/'))],
            permissions: vec![Permission {
                actions: vec!["InvalidAction/DoSomething".to_string()],
                not_actions: vec![],
                data_actions: vec![],
                not_data_actions: vec![],
            }],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let result = ctx
        .authorization_client
        .create_or_update_role_definition(&scope, role_definition_id.clone(), &role_definition)
        .await;

    // Azure might accept this (permissions are validated at assignment time),
    // but if it rejects invalid permissions, that's also acceptable
    match result {
        Ok(_) => {
            info!("Azure accepted role definition with invalid permissions (will be validated at assignment time)");
            ctx.track_role_definition(&scope, &role_definition_id);
        }
        Err(e) => {
            info!(
                "Azure rejected role definition with invalid permissions: {:?}",
                e
            );
        }
    }
}

#[test_context(AuthorizationTestContext)]
#[tokio::test]
async fn test_role_definition_options_validation(ctx: &mut AuthorizationTestContext) {
    let role_definition_id = ctx.generate_unique_role_definition_id();
    let role_name = format!(
        "alien-test-role-{}",
        Uuid::new_v4()
            .as_simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let scope = ctx.resource_group_scope();

    // This test is no longer applicable since we removed client_request_id validation
    // We can test some other validation scenario or remove this test
    let scope_string = scope.to_scope_string(ctx.authorization_client.token_cache.config());
    let role_definition = RoleDefinition {
        id: None,
        name: None,
        type_: None,
        properties: Some(RoleDefinitionProperties {
            role_name: Some(role_name),
            type_: Some("CustomRole".to_string()),
            description: Some("Test role".to_string()),
            assignable_scopes: vec![format!("/{}", scope_string.trim_start_matches('/'))],
            permissions: vec![Permission {
                actions: vec!["Microsoft.Storage/storageAccounts/read".to_string()],
                not_actions: vec![],
                data_actions: vec![],
                not_data_actions: vec![],
            }],
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        }),
    };

    let result = ctx
        .authorization_client
        .create_or_update_role_definition(&scope, role_definition_id.clone(), &role_definition)
        .await;

    // Since we removed client_request_id validation, this should succeed
    if result.is_ok() {
        ctx.track_role_definition(&scope, &role_definition_id);
    }

    assert!(
        result.is_ok(),
        "Role definition creation should succeed: {:?}",
        result.err()
    );
}
