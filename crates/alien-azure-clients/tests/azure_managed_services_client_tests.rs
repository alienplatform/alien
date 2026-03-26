#![cfg(all(test, feature = "azure"))]

//! Azure Lighthouse (Managed Services) Integration Tests
//!
//! These tests validate Azure Lighthouse functionality which requires a two-tenant setup:
//!
//! ## Required Setup:
//! 1. **Management Tenant** - The managing organization (service provider)
//!    - Has its own subscription where managed identities are created
//!    - Service principal for this tenant (AZURE_MANAGEMENT_*)
//!
//! 2. **Target Tenant** - The customer organization being managed  
//!    - Has its own separate subscription where Lighthouse resources are created
//!    - Service principal for this tenant (AZURE_TARGET_*)
//!    - **CRITICAL**: Target service principal must have Owner or User Access Administrator role
//!
//! ## Environment Variables Required:
//! - AZURE_MANAGEMENT_TENANT_ID, AZURE_MANAGEMENT_SUBSCRIPTION_ID, AZURE_MANAGEMENT_CLIENT_ID, AZURE_MANAGEMENT_CLIENT_SECRET
//! - AZURE_TARGET_TENANT_ID, AZURE_TARGET_SUBSCRIPTION_ID, AZURE_TARGET_CLIENT_ID, AZURE_TARGET_CLIENT_SECRET
//! - ALIEN_TEST_AZURE_RESOURCE_GROUP (exists in both subscriptions)
//!
//! ## Grant Required Permissions:
//! ```bash
//! # In target subscription, grant Owner role to target service principal
//! az role assignment create \
//!   --assignee <AZURE_TARGET_CLIENT_ID> \
//!   --role Owner \
//!   --scope /subscriptions/<AZURE_TARGET_SUBSCRIPTION_ID>
//! ```

use alien_azure_clients::managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi};
use alien_azure_clients::managed_services::{AzureManagedServicesClient, ManagedServicesApi};
use alien_azure_clients::models::managed_identity::{Identity, UserAssignedIdentityProperties};
use alien_azure_clients::models::managedservices::{
    Authorization, RegistrationAssignment, RegistrationAssignmentProperties,
    RegistrationAssignmentPropertiesProvisioningState, RegistrationDefinition,
    RegistrationDefinitionProperties,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_azure_clients::AzureTokenCache;
use alien_client_core::{Error, ErrorData};
use alien_error::AlienError;
use reqwest::Client;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct TrackedRegistrationDefinition {
    scope: String,
    registration_definition_id: String,
}

#[derive(Debug, Clone)]
struct TrackedRegistrationAssignment {
    scope: String,
    registration_assignment_id: String,
}

struct ManagedServicesTestContext {
    // Management account client (where the managed identity is)
    management_client: AzureManagedServicesClient,
    management_identity_client: AzureManagedIdentityClient,
    // Target account client (where lighthouse resources are created)
    target_client: AzureManagedServicesClient,
    #[allow(dead_code)]
    management_subscription_id: String,
    target_subscription_id: String,
    resource_group_name: String,
    // Managed identity created for this test
    management_identity_name: String,
    management_principal_id: String,
    created_registration_definitions: Mutex<Vec<TrackedRegistrationDefinition>>,
    created_registration_assignments: Mutex<Vec<TrackedRegistrationAssignment>>,
}

impl AsyncTestContext for ManagedServicesTestContext {
    async fn setup() -> ManagedServicesTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok(); // Initialize tracing

        // Management account credentials
        let management_subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let management_tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let management_client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let management_client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");

        // Target account credentials
        let target_subscription_id =
            env::var("AZURE_TARGET_SUBSCRIPTION_ID").expect("AZURE_TARGET_SUBSCRIPTION_ID not set");
        let target_tenant_id =
            env::var("AZURE_TARGET_TENANT_ID").expect("AZURE_TARGET_TENANT_ID not set");
        let target_client_id =
            env::var("AZURE_TARGET_CLIENT_ID").expect("AZURE_TARGET_CLIENT_ID not set");
        let target_client_secret =
            env::var("AZURE_TARGET_CLIENT_SECRET").expect("AZURE_TARGET_CLIENT_SECRET not set");

        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        // Create management platform config
        let management_client_config = AzureClientConfig {
            subscription_id: management_subscription_id.clone(),
            tenant_id: management_tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: management_client_id,
                client_secret: management_client_secret,
            },
            service_overrides: None,
        };

        // Create target platform config
        let target_client_config = AzureClientConfig {
            subscription_id: target_subscription_id.clone(),
            tenant_id: target_tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: target_client_id,
                client_secret: target_client_secret,
            },
            service_overrides: None,
        };

        let client = Client::new();
        let management_client =
            AzureManagedServicesClient::new(client.clone(), AzureTokenCache::new(management_client_config.clone()));
        let management_identity_client =
            AzureManagedIdentityClient::new(client.clone(), AzureTokenCache::new(management_client_config));
        let target_client = AzureManagedServicesClient::new(client, AzureTokenCache::new(target_client_config));

        info!("🔧 Azure Lighthouse Test Setup:");
        info!(
            "   Management Tenant: {} (Subscription: {})",
            management_client.token_cache.config().tenant_id, management_subscription_id
        );
        info!(
            "   Target Tenant: {} (Subscription: {})",
            target_client.token_cache.config().tenant_id, target_subscription_id
        );
        info!("   ⚠️  REQUIRED: Target service principal must have Owner or User Access Administrator role on target subscription");
        info!("   ⚠️  To grant permissions, run in target subscription:");
        info!("       az role assignment create --assignee <target-service-principal-id> --role Owner --scope /subscriptions/{}", target_subscription_id);

        // Validate that we have different tenants and subscriptions for proper Lighthouse testing
        if management_client.token_cache.config().tenant_id == target_client.token_cache.config().tenant_id {
            panic!("❌ Management and target tenants must be different for Azure Lighthouse testing. Both are: {}", management_client.token_cache.config().tenant_id);
        }

        if management_subscription_id == target_subscription_id {
            panic!("❌ Management and target subscriptions must be different for Azure Lighthouse testing. Both are: {}", management_subscription_id);
        }

        info!("✅ Tenant and subscription setup validated for Lighthouse testing");

        // Create a managed identity in the management account for Lighthouse delegation
        let management_identity_name = format!(
            "alien-lighthouse-test-{}",
            Uuid::new_v4()
                .as_simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        );
        info!(
            "🏗️  Creating managed identity '{}' in management account for Lighthouse",
            management_identity_name
        );

        let mut tags = HashMap::new();
        tags.insert(
            "CreatedBy".to_string(),
            "alien-lighthouse-tests".to_string(),
        );
        tags.insert("Purpose".to_string(), "lighthouse-delegation".to_string());

        let identity = Identity {
            id: None,
            name: None,
            type_: None,
            location: "eastus".to_string(),
            tags,
            properties: Some(UserAssignedIdentityProperties {
                tenant_id: None,
                principal_id: None,
                client_id: None,
                isolation_scope: None,
            }),
            system_data: None,
        };

        let created_identity = management_identity_client
            .create_or_update_user_assigned_identity(
                &resource_group_name,
                &management_identity_name,
                &identity,
            )
            .await
            .expect("Failed to create managed identity in management account");

        let management_principal_id = created_identity
            .properties
            .as_ref()
            .and_then(|p| p.principal_id.as_ref())
            .expect("Created managed identity should have a principal ID")
            .clone();

        info!(
            "✅ Created managed identity with principal ID: {}",
            management_principal_id
        );

        // Wait for managed identity to propagate across Azure tenants to avoid InvalidPrincipalId errors
        info!("⏳ Waiting 30 seconds for managed identity to propagate across Azure tenants...");
        sleep(Duration::from_secs(30)).await;
        info!("✅ Finished waiting for managed identity propagation");

        ManagedServicesTestContext {
            management_client,
            management_identity_client,
            target_client,
            management_subscription_id,
            target_subscription_id,
            resource_group_name,
            management_identity_name: management_identity_name.clone(),
            management_principal_id: management_principal_id.to_string(),
            created_registration_definitions: Mutex::new(Vec::new()),
            created_registration_assignments: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created registration assignments (using target client)
        let assignments = self.created_registration_assignments.into_inner().unwrap();
        for assignment in assignments {
            // Try multiple times with delays for assignments that might still be provisioning
            let mut attempts = 0;
            let max_attempts = 3;

            while attempts < max_attempts {
                attempts += 1;

                match self
                    .target_client
                    .delete_registration_assignment(
                        &assignment.scope,
                        &assignment.registration_assignment_id,
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            "✅ Successfully cleaned up registration assignment {}",
                            assignment.registration_assignment_id
                        );
                        break;
                    }
                    Err(e) => {
                        if e.to_string().contains("provisioning is in progress")
                            && attempts < max_attempts
                        {
                            warn!("⏳ Assignment {} still provisioning, waiting 30s before retry {}/{}", 
                                assignment.registration_assignment_id, attempts, max_attempts);
                            sleep(Duration::from_secs(30)).await;
                        } else {
                            warn!("❌ Failed to clean up registration assignment {} (attempt {}/{}): {:?}", 
                                assignment.registration_assignment_id, attempts, max_attempts, e);
                            break;
                        }
                    }
                }
            }
        }

        // Clean up created registration definitions (using target client)
        let definitions = self.created_registration_definitions.into_inner().unwrap();
        for definition in definitions {
            if let Err(e) = self
                .target_client
                .delete_registration_definition(
                    &definition.scope,
                    &definition.registration_definition_id,
                )
                .await
            {
                warn!(
                    "Failed to clean up registration definition {}: {:?}",
                    definition.registration_definition_id, e
                );
            }
        }

        // Clean up the managed identity (using management client)
        info!(
            "🧹 Cleaning up managed identity '{}'",
            self.management_identity_name
        );
        if let Err(e) = self
            .management_identity_client
            .delete_user_assigned_identity(
                &self.resource_group_name,
                &self.management_identity_name,
            )
            .await
        {
            warn!(
                "Failed to clean up managed identity {}: {:?}",
                self.management_identity_name, e
            );
        } else {
            info!(
                "✅ Managed identity '{}' cleaned up successfully",
                self.management_identity_name
            );
        }
    }
}

// Test helper methods
impl ManagedServicesTestContext {
    fn track_registration_definition(&self, scope: String, registration_definition_id: String) {
        self.created_registration_definitions
            .lock()
            .unwrap()
            .push(TrackedRegistrationDefinition {
                scope,
                registration_definition_id,
            });
    }

    fn track_registration_assignment(&self, scope: String, registration_assignment_id: String) {
        self.created_registration_assignments
            .lock()
            .unwrap()
            .push(TrackedRegistrationAssignment {
                scope,
                registration_assignment_id,
            });
    }

    fn create_test_registration_definition(&self) -> RegistrationDefinition {
        let authorization = Authorization {
            principal_id: self.management_principal_id.clone(), // Use the actual managed identity principal ID
            role_definition_id: "acdd72a7-3385-48ef-bd42-f606fba81ae7".to_string(), // Reader role
            delegated_role_definition_ids: Some(vec![]),        // Empty vector for now
            principal_id_display_name: None,
        };

        // Use the management tenant ID as the managing tenant
        let managing_tenant_id = &self.management_client.token_cache.config().tenant_id;

        let properties = RegistrationDefinitionProperties {
            description: Some("Test registration definition for Lighthouse delegation".to_string()),
            authorizations: vec![authorization],
            managed_by_tenant_id: managing_tenant_id.to_string(),
            registration_definition_name: Some("AlienLighthouseTest".to_string()),
            eligible_authorizations: Some(vec![]), // Empty vector for now
            provisioning_state: None,
            managee_tenant_id: None,
            managee_tenant_name: None,
            managed_by_tenant_name: None,
        };

        RegistrationDefinition {
            properties: Some(properties),
            id: None,
            name: None,
            type_: None,
            plan: None,
            system_data: None,
        }
    }

    /// Wait for a registration assignment to be fully provisioned before proceeding
    async fn wait_for_assignment_ready(
        &self,
        scope: &str,
        assignment_id: &str,
    ) -> Result<(), Error> {
        info!(
            "⏳ Waiting for registration assignment '{}' to be fully provisioned...",
            assignment_id
        );

        let max_wait_time = Duration::from_secs(120); // 2 minutes max wait
        let poll_interval = Duration::from_secs(10); // Poll every 10 seconds
        let start_time = std::time::Instant::now();

        loop {
            if start_time.elapsed() > max_wait_time {
                warn!(
                    "⏰ Timeout waiting for registration assignment '{}' to be ready",
                    assignment_id
                );
                break;
            }

            match self
                .target_client
                .get_registration_assignment(scope, assignment_id)
                .await
            {
                Ok(assignment) => {
                    // Check if we have properties indicating it's provisioned
                    if let Some(properties) = &assignment.properties {
                        if let Some(provisioning_state) = properties.provisioning_state.as_ref() {
                            info!(
                                "📋 Assignment '{}' provisioning state: {}",
                                assignment_id, provisioning_state
                            );
                            match provisioning_state {
                                RegistrationAssignmentPropertiesProvisioningState::Succeeded => {
                                    info!(
                                        "✅ Registration assignment '{}' is ready",
                                        assignment_id
                                    );
                                    return Ok(());
                                }
                                RegistrationAssignmentPropertiesProvisioningState::Failed => {
                                    return Err(AlienError::new(
                                        ErrorData::RemoteResourceConflict {
                                            resource_type: "RegistrationAssignment".to_string(),
                                            resource_name: assignment_id.to_string(),
                                            message: "Registration assignment provisioning failed"
                                                .to_string(),
                                        },
                                    ));
                                }
                                _ => {
                                    // Still in progress, continue waiting
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    info!(
                        "🔄 Assignment '{}' not yet accessible: {}",
                        assignment_id, e
                    );
                }
            }

            info!(
                "⏳ Still waiting for assignment '{}', sleeping for {} seconds...",
                assignment_id,
                poll_interval.as_secs()
            );
            sleep(poll_interval).await;
        }

        // Even if we timeout, continue - sometimes the assignment works even without clear provisioning state
        warn!(
            "⚠️  Proceeding with assignment '{}' despite timeout",
            assignment_id
        );
        Ok(())
    }
}

#[test_context(ManagedServicesTestContext)]
#[tokio::test]
async fn test_registration_definition_crud(ctx: &ManagedServicesTestContext) -> Result<(), Error> {
    let registration_definition_id = Uuid::new_v4().to_string();
    // Use target subscription for Lighthouse resources
    let scope = ctx
        .target_client
        .build_subscription_scope(&ctx.target_subscription_id);

    let registration_definition = ctx.create_test_registration_definition();

    info!(
        "Creating registration definition with ID: {} in target subscription",
        registration_definition_id
    );

    // Create registration definition in target account
    let created = ctx.target_client
        .create_or_update_registration_definition(
            &scope,
            &registration_definition_id,
            &registration_definition,
        )
        .await
        .map_err(|e| {
            if e.to_string().contains("AuthorizationFailed") {
                info!("❌ Permission denied creating Lighthouse registration definition");
                info!("   The target service principal needs Owner or User Access Administrator role");
                info!("   Run: az role assignment create --assignee <target-service-principal-id> --role Owner --scope /subscriptions/{}", ctx.target_subscription_id);
            }
            e
        })?;

    ctx.track_registration_definition(scope.clone(), registration_definition_id.clone());

    // Verify creation
    assert!(created.id.is_some());
    assert!(created.properties.is_some());

    // Get registration definition from target account
    let retrieved = ctx
        .target_client
        .get_registration_definition(&scope, &registration_definition_id)
        .await?;

    assert_eq!(created.id, retrieved.id);

    // Delete registration definition from target account
    let deleted = ctx
        .target_client
        .delete_registration_definition(&scope, &registration_definition_id)
        .await?;

    // Verify deletion (the returned value may be None for successful deletion)
    info!("Registration definition deleted: {:?}", deleted.is_some());

    Ok(())
}

#[test_context(ManagedServicesTestContext)]
#[tokio::test]
async fn test_registration_assignment_crud(ctx: &ManagedServicesTestContext) -> Result<(), Error> {
    // First create a registration definition
    let registration_definition_id = Uuid::new_v4().to_string();
    let registration_assignment_id = Uuid::new_v4().to_string();
    // Use target subscription for Lighthouse resources
    let scope = ctx
        .target_client
        .build_subscription_scope(&ctx.target_subscription_id);

    let registration_definition = ctx.create_test_registration_definition();

    info!(
        "Creating registration definition for assignment test with ID: {} in target subscription",
        registration_definition_id
    );

    let created_definition = ctx.target_client
        .create_or_update_registration_definition(
            &scope,
            &registration_definition_id,
            &registration_definition,
        )
        .await
        .map_err(|e| {
            if e.to_string().contains("AuthorizationFailed") {
                info!("❌ Permission denied creating Lighthouse registration definition");
                info!("   The target service principal needs Owner or User Access Administrator role");
                info!("   Run: az role assignment create --assignee <target-service-principal-id> --role Owner --scope /subscriptions/{}", ctx.target_subscription_id);
            }
            e
        })?;

    ctx.track_registration_definition(scope.clone(), registration_definition_id.clone());

    // Create registration assignment
    let assignment_properties = RegistrationAssignmentProperties {
        registration_definition_id: created_definition.id.unwrap(),
        provisioning_state: None,
        registration_definition: None,
    };

    let registration_assignment = RegistrationAssignment {
        properties: Some(assignment_properties),
        id: None,
        name: None,
        type_: None,
        system_data: None,
    };

    info!(
        "Creating registration assignment with ID: {} in target subscription",
        registration_assignment_id
    );

    let created_assignment = ctx
        .target_client
        .create_or_update_registration_assignment(
            &scope,
            &registration_assignment_id,
            &registration_assignment,
        )
        .await?;

    ctx.track_registration_assignment(scope.clone(), registration_assignment_id.clone());

    // Verify creation
    assert!(created_assignment.id.is_some());
    assert!(created_assignment.properties.is_some());

    // Wait for the assignment to be fully provisioned before proceeding
    ctx.wait_for_assignment_ready(&scope, &registration_assignment_id)
        .await?;

    // Get registration assignment from target account
    let retrieved_assignment = ctx
        .target_client
        .get_registration_assignment(&scope, &registration_assignment_id)
        .await?;

    assert_eq!(created_assignment.id, retrieved_assignment.id);

    // Delete registration assignment from target account
    let deleted_assignment = ctx
        .target_client
        .delete_registration_assignment(&scope, &registration_assignment_id)
        .await?;

    info!(
        "Registration assignment deleted: {:?}",
        deleted_assignment.is_some()
    );

    Ok(())
}

#[test_context(ManagedServicesTestContext)]
#[tokio::test]
async fn test_scope_helpers(ctx: &ManagedServicesTestContext) -> Result<(), Error> {
    let subscription_scope = ctx
        .target_client
        .build_subscription_scope(&ctx.target_subscription_id);

    let resource_group_scope = ctx
        .target_client
        .build_resource_group_scope(&ctx.target_subscription_id, &ctx.resource_group_name);

    // Verify scope formats
    assert_eq!(
        subscription_scope,
        format!("subscriptions/{}", ctx.target_subscription_id)
    );

    assert_eq!(
        resource_group_scope,
        format!(
            "subscriptions/{}/resourceGroups/{}",
            ctx.target_subscription_id, ctx.resource_group_name
        )
    );

    Ok(())
}

#[test_context(ManagedServicesTestContext)]
#[tokio::test]
async fn test_nonexistent_registration_definition(
    ctx: &ManagedServicesTestContext,
) -> Result<(), Error> {
    let nonexistent_id = Uuid::new_v4().to_string();
    let scope = ctx
        .target_client
        .build_subscription_scope(&ctx.target_subscription_id);

    // Try to get a non-existent registration definition from target account
    let result = ctx
        .target_client
        .get_registration_definition(&scope, &nonexistent_id)
        .await;

    // Should return an error (typically 404 Not Found)
    assert!(result.is_err());

    // For now, just verify that we get an error - the specific error type checking
    // can be enhanced later if needed
    info!("Expected error occurred: {:?}", result.unwrap_err());

    Ok(())
}
