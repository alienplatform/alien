/*!
# Azure Service Bus Client Integration Tests

Tests Azure Service Bus operations: namespace and queue management, message send/receive operations.

## Prerequisites
Set up `.env.test` with Azure credentials:
```
AZURE_MANAGEMENT_SUBSCRIPTION_ID=your_subscription_id
AZURE_MANAGEMENT_TENANT_ID=your_tenant_id
AZURE_MANAGEMENT_CLIENT_ID=your_client_id
AZURE_MANAGEMENT_CLIENT_SECRET=your_client_secret
ALIEN_TEST_AZURE_RESOURCE_GROUP=your_test_resource_group
```

Note: The service principal object ID is automatically resolved by decoding the JWT token from Azure authentication.

Note: The test creates Service Bus namespaces and queues in your Azure subscription. These resources incur charges.
*/

use alien_azure_clients::authorization::{AuthorizationApi, AzureAuthorizationClient, Scope};
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::queue::SbQueueProperties;
use alien_azure_clients::models::queue_namespace::{
    SbNamespaceProperties, SbNamespacePropertiesPublicNetworkAccess,
};
use alien_azure_clients::service_bus::{
    AzureServiceBusDataPlaneClient, AzureServiceBusManagementClient, BrokerProperties,
    SendMessageParameters, ServiceBusDataPlaneApi, ServiceBusManagementApi,
};
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt as _, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use alien_error::{AlienError, Context};
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct ServiceBusTestContext {
    management_client: AzureServiceBusManagementClient,
    data_plane_client: AzureServiceBusDataPlaneClient,
    authorization_client: AzureAuthorizationClient,
    subscription_id: String,
    resource_group_name: String,
    created_namespaces: Mutex<HashSet<String>>,
    created_queues: Mutex<HashMap<String, HashSet<String>>>, // namespace_name -> set of queue names
    created_role_assignments: Mutex<HashSet<String>>,
}

impl AsyncTestContext for ServiceBusTestContext {
    async fn setup() -> ServiceBusTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

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

        let config = AzureClientConfig {
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
            "🔧 Using subscription: {} and resource group: {} for Service Bus testing",
            subscription_id, resource_group_name
        );

        let management_client = AzureServiceBusManagementClient::new(Client::new(), config.clone());
        let data_plane_client = AzureServiceBusDataPlaneClient::new(Client::new(), config.clone());
        let authorization_client = AzureAuthorizationClient::new(Client::new(), config);

        ServiceBusTestContext {
            management_client,
            data_plane_client,
            authorization_client,
            subscription_id,
            resource_group_name,
            created_namespaces: Mutex::new(HashSet::new()),
            created_queues: Mutex::new(HashMap::new()),
            created_role_assignments: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Service Bus test cleanup...");

        // Cleanup role assignments first
        let role_assignments_to_cleanup = {
            let assignments = self.created_role_assignments.lock().unwrap();
            assignments.clone()
        };

        for assignment_id in role_assignments_to_cleanup {
            self.cleanup_role_assignment(&assignment_id).await;
        }

        // Cleanup queues
        let queues_to_cleanup = {
            let queues = self.created_queues.lock().unwrap();
            queues.clone()
        };

        for (namespace_name, queue_names) in queues_to_cleanup {
            for queue_name in queue_names {
                self.cleanup_queue(&namespace_name, &queue_name).await;
            }
        }

        // Cleanup namespaces
        let namespaces_to_cleanup = {
            let namespaces = self.created_namespaces.lock().unwrap();
            namespaces.clone()
        };

        for namespace_name in namespaces_to_cleanup {
            self.cleanup_namespace(&namespace_name).await;
        }

        info!("✅ Service Bus test cleanup completed");
    }
}

impl ServiceBusTestContext {
    fn track_namespace(&self, namespace_name: &str) {
        let mut created_namespaces = self.created_namespaces.lock().unwrap();
        created_namespaces.insert(namespace_name.to_string());
    }

    fn track_queue(&self, namespace_name: &str, queue_name: &str) {
        let mut created_queues = self.created_queues.lock().unwrap();
        created_queues
            .entry(namespace_name.to_string())
            .or_insert_with(HashSet::new)
            .insert(queue_name.to_string());
    }

    fn track_role_assignment(&self, assignment_id: &str) {
        let mut created_role_assignments = self.created_role_assignments.lock().unwrap();
        created_role_assignments.insert(assignment_id.to_string());
        info!("📝 Tracking role assignment for cleanup: {}", assignment_id);
    }

    async fn cleanup_namespace(&self, namespace_name: &str) {
        info!("🗑️ Cleaning up namespace: {}", namespace_name);
        match self
            .management_client
            .delete_namespace(self.resource_group_name.clone(), namespace_name.to_string())
            .await
        {
            Ok(_) => info!("✅ Successfully deleted namespace: {}", namespace_name),
            Err(e) => {
                warn!("⚠️ Failed to delete namespace {}: {:?}", namespace_name, e);
                // Check if it's a "not found" error - if so, it's already cleaned up
                if let Some(ErrorData::RemoteResourceNotFound { .. }) = &e.error {
                    info!("✅ Namespace {} was already deleted", namespace_name);
                    return;
                }
                // Log other errors but don't fail the test
                warn!("⚠️ Error during namespace cleanup will be ignored: {:?}", e);
            }
        }
    }

    async fn cleanup_queue(&self, namespace_name: &str, queue_name: &str) {
        info!(
            "🗑️ Cleaning up queue: {} in namespace: {}",
            queue_name, namespace_name
        );
        match self
            .management_client
            .delete_queue(
                self.resource_group_name.clone(),
                namespace_name.to_string(),
                queue_name.to_string(),
            )
            .await
        {
            Ok(_) => info!(
                "✅ Successfully deleted queue: {} in namespace: {}",
                queue_name, namespace_name
            ),
            Err(e) => {
                warn!(
                    "⚠️ Failed to delete queue {}/{}: {:?}",
                    namespace_name, queue_name, e
                );
                // Check if it's a "not found" error - if so, it's already cleaned up
                if let Some(ErrorData::RemoteResourceNotFound { .. }) = &e.error {
                    info!(
                        "✅ Queue {}/{} was already deleted",
                        namespace_name, queue_name
                    );
                    return;
                }
                // Log other errors but don't fail the test
                warn!("⚠️ Error during queue cleanup will be ignored: {:?}", e);
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

    fn generate_unique_namespace_name(&self) -> String {
        format!(
            "alien-test-sb-ns-{}",
            Uuid::new_v4().to_string().replace('-', "")[..8].to_lowercase()
        )
    }

    fn generate_unique_queue_name(&self) -> String {
        format!(
            "alien-test-queue-{}",
            Uuid::new_v4().to_string().replace('-', "")[..8].to_lowercase()
        )
    }

    async fn create_test_namespace(&self, namespace_name: &str) -> Result<(), Error> {
        info!("🔧 Creating test namespace: {}", namespace_name);

        let namespace_properties = SbNamespaceProperties {
            private_endpoint_connections: Vec::new(),
            public_network_access: SbNamespacePropertiesPublicNetworkAccess::Enabled,
            created_at: None,
            disable_local_auth: None,
            encryption: None,
            metric_id: None,
            provisioning_state: None,
            service_bus_endpoint: None,
            status: None,
            updated_at: None,
            zone_redundant: None,
            alternate_name: None,
            minimum_tls_version: None,
            premium_messaging_partitions: None,
        };

        self.management_client
            .create_or_update_namespace(
                self.resource_group_name.clone(),
                namespace_name.to_string(),
                namespace_properties,
            )
            .await?;

        self.track_namespace(namespace_name);
        info!("✅ Created test namespace: {}", namespace_name);

        // Assign Service Bus Data Owner role for data plane operations
        self.assign_service_bus_data_owner_role(namespace_name)
            .await?;

        Ok(())
    }

    async fn create_test_queue(&self, namespace_name: &str, queue_name: &str) -> Result<(), Error> {
        info!(
            "🔧 Creating test queue: {} in namespace: {}",
            queue_name, namespace_name
        );

        let queue_properties = SbQueueProperties {
            max_size_in_megabytes: Some(1024),
            enable_batched_operations: Some(true),
            auto_delete_on_idle: None,
            count_details: None,
            created_at: None,
            dead_lettering_on_message_expiration: None,
            default_message_time_to_live: None,
            duplicate_detection_history_time_window: None,
            enable_express: None,
            enable_partitioning: None,
            forward_dead_lettered_messages_to: None,
            forward_to: None,
            lock_duration: None,
            max_delivery_count: None,
            message_count: None,
            requires_duplicate_detection: None,
            requires_session: None,
            size_in_bytes: None,
            status: None,
            updated_at: None,
            accessed_at: None,
            max_message_size_in_kilobytes: None,
        };

        self.management_client
            .create_or_update_queue(
                self.resource_group_name.clone(),
                namespace_name.to_string(),
                queue_name.to_string(),
                queue_properties,
            )
            .await?;

        self.track_queue(namespace_name, queue_name);
        info!(
            "✅ Created test queue: {} in namespace: {}",
            queue_name, namespace_name
        );
        Ok(())
    }

    /// Automatically resolve the service principal's object ID by decoding the JWT token
    async fn resolve_service_principal_object_id(&self) -> Result<String, Error> {
        info!("🔍 Auto-resolving object ID from JWT token...");

        // Get a bearer token for Azure Resource Manager (this will contain the oid claim)
        let bearer_token = self
            .management_client
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to get bearer token".to_string(),
            })?;

        // Parse the JWT token to extract the payload (claims)
        let parts: Vec<&str> = bearer_token.split('.').collect();
        if parts.len() != 3 {
            return Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Invalid JWT token format - expected 3 parts".to_string(),
                errors: None,
            }));
        }

        // Decode the payload (claims) part
        let claims_b64 = parts[1];
        let claims_bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(claims_b64)
            .map_err(|e| {
                AlienError::new(ErrorData::DataLoadError {
                    message: format!("Failed to decode JWT payload: {}", e),
                })
            })?;

        // Parse the claims as JSON
        let claims_json: serde_json::Value =
            serde_json::from_slice(&claims_bytes).map_err(|e| {
                AlienError::new(ErrorData::DataLoadError {
                    message: format!("Failed to parse JWT claims JSON: {}", e),
                })
            })?;

        // Extract the oid (object ID) claim from the token
        let object_id = claims_json
            .get("oid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "JWT token does not contain 'oid' claim (object ID)".to_string(),
                    errors: Some(format!("Available claims: {}", claims_json)),
                })
            })?;

        info!("✅ Auto-resolved object ID from JWT: {}", object_id);
        Ok(object_id.to_string())
    }

    async fn assign_service_bus_data_owner_role(&self, namespace_name: &str) -> Result<(), Error> {
        info!(
            "🔐 Assigning Service Bus Data Owner role to service principal for namespace: {}",
            namespace_name
        );

        // Get the service principal object ID by decoding JWT token
        let service_principal_object_id = self.resolve_service_principal_object_id().await?;

        // Build Service Bus namespace resource scope
        let service_bus_scope = Scope::Resource {
            resource_group_name: self.resource_group_name.clone(),
            resource_provider: "Microsoft.ServiceBus".to_string(),
            parent_resource_path: None,
            resource_type: "namespaces".to_string(),
            resource_name: namespace_name.to_string(),
        };

        let scope_string =
            service_bus_scope.to_scope_string(&self.authorization_client.client_config);
        info!("   Role assignment scope: {}", scope_string);

        // Create role assignment with Service Bus Data Owner role
        let assignment_id = Uuid::new_v4().to_string();
        let service_bus_data_owner_role_id = "090c5cfd-751d-490a-894a-3ce6f1109419"; // Azure Service Bus Data Owner built-in role
        let role_definition_full_id = format!(
            "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            self.authorization_client.client_config.subscription_id, service_bus_data_owner_role_id
        );

        info!("   Assignment ID: {}", assignment_id);
        info!("   Principal ID: {}", service_principal_object_id);
        info!("   Role definition ID: {}", role_definition_full_id);

        let role_assignment = RoleAssignment {
            properties: Some(RoleAssignmentProperties {
                principal_id: service_principal_object_id.clone(),
                role_definition_id: role_definition_full_id,
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                scope: Some(
                    service_bus_scope.to_scope_string(&self.authorization_client.client_config),
                ),
                condition: None,
                condition_version: None,
                delegated_managed_identity_resource_id: None,
                description: Some(
                    "Service Bus Data Owner role for data plane operations".to_string(),
                ),
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
            id: None,
            name: None,
            type_: None,
        };

        let full_assignment_id = self
            .authorization_client
            .build_role_assignment_id(&service_bus_scope, assignment_id);
        info!("   Full assignment ID: {}", full_assignment_id);

        let _role_assignment_result = self
            .authorization_client
            .create_or_update_role_assignment_by_id(full_assignment_id.clone(), &role_assignment)
            .await?;

        self.track_role_assignment(&full_assignment_id);
        info!(
            "✅ Assigned Service Bus Data Owner role: {}",
            full_assignment_id
        );

        // Wait for role assignment to propagate
        self.wait_for_role_assignment_propagation(&full_assignment_id)
            .await?;

        Ok(())
    }

    /// Wait for role assignment propagation by polling until it's accessible
    async fn wait_for_role_assignment_propagation(
        &self,
        role_assignment_id: &str,
    ) -> Result<(), Error> {
        info!("⏳ Waiting for role assignment to propagate...");

        let max_attempts = 12; // Maximum 12 attempts
        let mut attempt = 0;
        let mut delay_seconds: u64 = 5; // Start with 5 seconds

        while attempt < max_attempts {
            attempt += 1;

            // Try to get the role assignment to verify it's accessible
            match self
                .authorization_client
                .get_role_assignment_by_id(role_assignment_id.to_string())
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Role assignment propagated successfully after {} attempts",
                        attempt
                    );
                    return Ok(());
                }
                Err(e) => {
                    if attempt == max_attempts {
                        return Err(Error::new(ErrorData::AuthenticationError {
                            message: format!(
                                "Role assignment failed to propagate after {} attempts (maximum {}s wait): {}",
                                max_attempts,
                                5 + (max_attempts - 1) * 10,
                                e
                            ),
                        }));
                    }

                    info!(
                        "⏳ Role assignment not yet accessible (attempt {}/{}). Waiting {} seconds before retry. Error: {}",
                        attempt, max_attempts, delay_seconds, e
                    );
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;

            // Increase delay for next attempt (exponential backoff with cap)
            delay_seconds = std::cmp::min(delay_seconds + 5, 20);
        }

        Err(Error::new(ErrorData::AuthenticationError {
            message: format!(
                "Role assignment propagation timeout after {} attempts",
                max_attempts
            ),
        }))
    }
}

#[test_context(ServiceBusTestContext)]
#[tokio::test]
async fn test_namespace_management_lifecycle(ctx: &ServiceBusTestContext) -> Result<(), Error> {
    let namespace_name = ctx.generate_unique_namespace_name();
    info!(
        "🧪 Testing namespace management lifecycle with namespace: {}",
        namespace_name
    );

    // 1. Create namespace
    ctx.create_test_namespace(&namespace_name).await?;

    // 2. Get namespace
    let retrieved_namespace = ctx
        .management_client
        .get_namespace(ctx.resource_group_name.clone(), namespace_name.clone())
        .await?;

    assert!(retrieved_namespace.name.is_some());
    assert_eq!(retrieved_namespace.name.unwrap(), namespace_name);
    info!("✅ Successfully retrieved namespace: {}", namespace_name);

    // 3. List namespaces in resource group
    let namespaces_list = ctx
        .management_client
        .list_namespaces_by_resource_group(ctx.resource_group_name.clone())
        .await?;

    let found_namespace = namespaces_list
        .value
        .iter()
        .find(|ns| ns.name.as_ref() == Some(&namespace_name));
    assert!(
        found_namespace.is_some(),
        "Created namespace should appear in list"
    );
    info!("✅ Namespace appears in resource group list");

    // 4. Update namespace (add tags or other properties)
    let updated_properties = SbNamespaceProperties {
        private_endpoint_connections: Vec::new(),
        public_network_access: SbNamespacePropertiesPublicNetworkAccess::Enabled,
        created_at: None,
        disable_local_auth: None,
        encryption: None,
        metric_id: None,
        provisioning_state: None,
        service_bus_endpoint: None,
        status: None,
        updated_at: None,
        zone_redundant: None,
        alternate_name: None,
        minimum_tls_version: None,
        premium_messaging_partitions: None,
    };

    let updated_namespace = ctx
        .management_client
        .create_or_update_namespace(
            ctx.resource_group_name.clone(),
            namespace_name.clone(),
            updated_properties,
        )
        .await?;

    assert!(updated_namespace.name.is_some());
    assert_eq!(updated_namespace.name.unwrap(), namespace_name);
    info!("✅ Successfully updated namespace: {}", namespace_name);

    info!("✅ Namespace management lifecycle test completed");
    Ok(())
}

#[test_context(ServiceBusTestContext)]
#[tokio::test]
async fn test_queue_management_lifecycle(ctx: &ServiceBusTestContext) -> Result<(), Error> {
    let namespace_name = ctx.generate_unique_namespace_name();
    let queue_name = ctx.generate_unique_queue_name();
    info!(
        "🧪 Testing queue management lifecycle with namespace: {} and queue: {}",
        namespace_name, queue_name
    );

    // 1. Create namespace first
    ctx.create_test_namespace(&namespace_name).await?;

    // 2. Create queue
    ctx.create_test_queue(&namespace_name, &queue_name).await?;

    // 3. Get queue
    let retrieved_queue = ctx
        .management_client
        .get_queue(
            ctx.resource_group_name.clone(),
            namespace_name.clone(),
            queue_name.clone(),
        )
        .await?;

    assert!(retrieved_queue.name.is_some());
    assert_eq!(retrieved_queue.name.unwrap(), queue_name);
    info!("✅ Successfully retrieved queue: {}", queue_name);

    // 4. List queues in namespace
    let queues_list = ctx
        .management_client
        .list_queues(ctx.resource_group_name.clone(), namespace_name.clone())
        .await?;

    let found_queue = queues_list
        .value
        .iter()
        .find(|q| q.name.as_ref() == Some(&queue_name));
    assert!(found_queue.is_some(), "Created queue should appear in list");
    info!("✅ Queue appears in namespace list");

    // 5. Update queue properties
    let updated_properties = SbQueueProperties {
        max_size_in_megabytes: Some(2048),
        enable_batched_operations: Some(false),
        auto_delete_on_idle: None,
        count_details: None,
        created_at: None,
        dead_lettering_on_message_expiration: None,
        default_message_time_to_live: None,
        duplicate_detection_history_time_window: None,
        enable_express: None,
        enable_partitioning: None,
        forward_dead_lettered_messages_to: None,
        forward_to: None,
        lock_duration: None,
        max_delivery_count: None,
        message_count: None,
        requires_duplicate_detection: None,
        requires_session: None,
        size_in_bytes: None,
        status: None,
        updated_at: None,
        accessed_at: None,
        max_message_size_in_kilobytes: None,
    };

    let updated_queue = ctx
        .management_client
        .create_or_update_queue(
            ctx.resource_group_name.clone(),
            namespace_name.clone(),
            queue_name.clone(),
            updated_properties,
        )
        .await?;

    assert!(updated_queue.name.is_some());
    assert_eq!(updated_queue.name.unwrap(), queue_name);
    info!("✅ Successfully updated queue: {}", queue_name);

    info!("✅ Queue management lifecycle test completed");
    Ok(())
}

#[test_context(ServiceBusTestContext)]
#[tokio::test]
async fn test_message_send_and_receive_delete(ctx: &ServiceBusTestContext) -> Result<(), Error> {
    let namespace_name = ctx.generate_unique_namespace_name();
    let queue_name = ctx.generate_unique_queue_name();
    info!(
        "🧪 Testing message send and receive-delete with namespace: {} and queue: {}",
        namespace_name, queue_name
    );

    // Setup namespace and queue
    ctx.create_test_namespace(&namespace_name).await?;

    // Wait a bit for namespace to be fully ready
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    ctx.create_test_queue(&namespace_name, &queue_name).await?;

    // Wait a bit for queue to be fully ready
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // 1. Send a simple message
    let message_body = "Hello, Service Bus!";
    let broker_properties = BrokerProperties {
        label: Some("test-message".to_string()),
        correlation_id: Some(Uuid::new_v4().to_string()),
        message_id: Some(Uuid::new_v4().to_string()),
        time_to_live: Some(300), // 5 minutes
        ..default_broker_properties()
    };

    let mut custom_properties = HashMap::new();
    custom_properties.insert("priority".to_string(), "High".to_string());
    custom_properties.insert("source".to_string(), "AlienTest".to_string());

    let send_params = SendMessageParameters {
        body: message_body.to_string(),
        broker_properties: Some(broker_properties.clone()),
        custom_properties,
    };

    ctx.data_plane_client
        .send_message(namespace_name.clone(), queue_name.clone(), send_params)
        .await?;
    info!("✅ Successfully sent message to queue");

    // 2. Receive and delete the message
    let received_message = ctx
        .data_plane_client
        .receive_and_delete(
            namespace_name.clone(),
            queue_name.clone(),
            Some(30), // 30 second timeout
        )
        .await?;

    assert!(
        received_message.is_some(),
        "Should receive the sent message"
    );
    let message = received_message.unwrap();
    assert_eq!(message.body, message_body);

    if let Some(props) = &message.broker_properties {
        assert_eq!(props.label, broker_properties.label);
        assert_eq!(props.correlation_id, broker_properties.correlation_id);
        assert_eq!(props.message_id, broker_properties.message_id);
    }

    assert!(message.custom_properties.get("priority").is_some());
    assert_eq!(message.custom_properties.get("priority").unwrap(), "High");
    info!("✅ Successfully received and validated message");

    // 3. Try to receive again - should get no message
    let no_message = ctx
        .data_plane_client
        .receive_and_delete(
            namespace_name.clone(),
            queue_name.clone(),
            Some(5), // Short timeout
        )
        .await?;

    assert!(
        no_message.is_none(),
        "Should not receive any message after destructive read"
    );
    info!("✅ Confirmed message was deleted after receive");

    info!("✅ Message send and receive-delete test completed");
    Ok(())
}

#[test_context(ServiceBusTestContext)]
#[tokio::test]
async fn test_message_peek_lock_and_complete(ctx: &ServiceBusTestContext) -> Result<(), Error> {
    let namespace_name = ctx.generate_unique_namespace_name();
    let queue_name = ctx.generate_unique_queue_name();
    info!(
        "🧪 Testing message peek-lock and complete with namespace: {} and queue: {}",
        namespace_name, queue_name
    );

    // Setup namespace and queue
    ctx.create_test_namespace(&namespace_name).await?;

    // Wait a bit for namespace to be fully ready
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    ctx.create_test_queue(&namespace_name, &queue_name).await?;

    // Wait a bit for queue to be fully ready
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // 1. Send a message
    let message_body = "Test message for peek-lock";
    let message_id = Uuid::new_v4().to_string();
    let broker_properties = BrokerProperties {
        label: Some("peek-lock-test".to_string()),
        message_id: Some(message_id.clone()),
        time_to_live: Some(300),
        ..default_broker_properties()
    };

    let send_params = SendMessageParameters {
        body: message_body.to_string(),
        broker_properties: Some(broker_properties),
        custom_properties: HashMap::new(),
    };

    ctx.data_plane_client
        .send_message(namespace_name.clone(), queue_name.clone(), send_params)
        .await?;
    info!("✅ Successfully sent message for peek-lock test");

    // 2. Peek-lock the message
    let peeked_message = ctx
        .data_plane_client
        .peek_lock(namespace_name.clone(), queue_name.clone(), Some(30))
        .await?;

    assert!(
        peeked_message.is_some(),
        "Should peek-lock the sent message"
    );
    let message = peeked_message.unwrap();
    assert_eq!(message.body, message_body);

    // Extract lock token from broker properties
    let lock_token = message
        .broker_properties
        .as_ref()
        .and_then(|props| props.lock_token.clone())
        .expect("Peek-locked message should have a lock token");

    info!("✅ Successfully peek-locked message with lock token");

    // 3. Complete the message
    ctx.data_plane_client
        .complete_message(
            namespace_name.clone(),
            queue_name.clone(),
            message_id,
            lock_token,
        )
        .await?;
    info!("✅ Successfully completed message");

    // 4. Try to receive again - should get no message
    let no_message = ctx
        .data_plane_client
        .receive_and_delete(namespace_name.clone(), queue_name.clone(), Some(5))
        .await?;

    assert!(
        no_message.is_none(),
        "Should not receive any message after completion"
    );
    info!("✅ Confirmed message was completed and removed");

    info!("✅ Message peek-lock and complete test completed");
    Ok(())
}

fn default_broker_properties() -> BrokerProperties {
    BrokerProperties {
        label: None,
        correlation_id: None,
        session_id: None,
        message_id: None,
        reply_to: None,
        time_to_live: None,
        delivery_count: None,
        lock_token: None,
        sequence_number: None,
        enqueued_time_utc: None,
        scheduled_enqueue_time_utc: None,
    }
}
