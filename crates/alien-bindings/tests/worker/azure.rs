use super::*;

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
pub(super) struct AzureProviderTestContext {
    function: Arc<dyn Worker>,
    resource_group_name: String,
    container_app_name: String,
    container_apps_client: AzureContainerAppsClient,
    authorization_client: AzureAuthorizationClient,
    managed_identity_client: AzureManagedIdentityClient,
    long_running_operation_client: LongRunningOperationClient,
    managed_environment_id: String,
    location: String,
    container_image: String,
    created_container_apps: Mutex<HashSet<String>>,
    created_managed_identities: Mutex<HashSet<String>>,
    created_role_assignments: Mutex<HashSet<String>>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-azure-function";

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID must be set in .env.test");
        let tenant_id = env::var("AZURE_MANAGEMENT_TENANT_ID")
            .expect("AZURE_MANAGEMENT_TENANT_ID must be set in .env.test");
        let client_id = env::var("AZURE_MANAGEMENT_CLIENT_ID")
            .expect("AZURE_MANAGEMENT_CLIENT_ID must be set in .env.test");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET must be set in .env.test");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP must be set in .env.test");
        let managed_environment_name = env::var("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME")
            .expect("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME must be set in .env.test");
        let default_container_image =
            "mcr.microsoft.com/azuredocs/containerapps-helloworld:latest".to_string();
        let mut container_image = env::var("ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE")
            .unwrap_or_else(|_| default_container_image.clone());

        let client_config = alien_azure_clients::AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: alien_azure_clients::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        let container_apps_client = AzureContainerAppsClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(client_config.clone()),
        );

        let authorization_client = AzureAuthorizationClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(client_config.clone()),
        );

        let managed_identity_client = AzureManagedIdentityClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(client_config.clone()),
        );

        let long_running_operation_client = LongRunningOperationClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(client_config.clone()),
        );

        // Get the existing managed environment to retrieve its ID
        let managed_environment = container_apps_client.get_managed_environment(&resource_group_name, &managed_environment_name).await
            .expect("Failed to get existing managed environment. Make sure ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME points to an existing managed environment.");

        let managed_environment_id = managed_environment
            .id
            .expect("Managed environment should have an ID");

        // Create a unique container app name
        let container_app_name = format!(
            "alien-test-app-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        );

        // Initialize tracking collections
        let mut created_managed_identities = HashSet::new();
        let mut created_role_assignments = HashSet::new();

        // Create managed identity with ACR access if needed
        let (registries, identity) = if container_image.contains(".azurecr.io") {
            info!(
                "🔐 Setting up ACR authentication for container image: {}",
                container_image
            );
            let identity_name = format!("{}-identity", container_app_name);

            // Create managed identity
            let managed_identity = Identity {
                location: "eastus".to_string(),
                tags: Default::default(),
                properties: None,
                id: None,
                name: None,
                type_: None,
                system_data: None,
            };

            let created_identity = managed_identity_client
                .create_or_update_user_assigned_identity(
                    &resource_group_name,
                    &identity_name,
                    &managed_identity,
                )
                .await
                .expect("Failed to create managed identity");

            let principal_id = created_identity
                .properties
                .as_ref()
                .and_then(|p| p.principal_id.clone())
                .expect("Managed identity should have a principal ID");

            let identity_resource_id = created_identity
                .id
                .as_ref()
                .expect("Managed identity should have a resource ID")
                .clone();

            info!(
                "✅ Created managed identity with principal ID: {}",
                principal_id
            );

            // Track the managed identity for cleanup
            created_managed_identities.insert(identity_name.clone());

            // Extract ACR name from container image and assign AcrPull role
            let acr_server = container_image.split('/').next().unwrap_or_default();
            let acr_name = acr_server.split('.').next().unwrap_or_default();

            info!(
                "🏷️ Assigning AcrPull role to managed identity for ACR: {}",
                acr_name
            );

            // Build ACR resource scope
            let acr_scope = Scope::Resource {
                resource_group_name: resource_group_name.clone(),
                resource_provider: "Microsoft.ContainerRegistry".to_string(),
                parent_resource_path: None,
                resource_type: "registries".to_string(),
                resource_name: acr_name.to_string(),
            };

            // Create role assignment
            let assignment_id = Uuid::new_v4().to_string();
            let acr_pull_role_definition_id = "7f951dda-4ed3-4680-a7ca-43fe172d538d"; // AcrPull built-in role
            let role_definition_full_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                subscription_id, acr_pull_role_definition_id
            );

            let role_assignment = RoleAssignment {
                properties: Some(RoleAssignmentProperties {
                    principal_id: principal_id.to_string(),
                    role_definition_id: role_definition_full_id,
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    scope: Some(
                        acr_scope.to_scope_string(authorization_client.token_cache.config()),
                    ),
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some(
                        "AcrPull role for Container App managed identity".to_string(),
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

            let full_assignment_id =
                authorization_client.build_role_assignment_id(&acr_scope, assignment_id);

            let role_assignment_result = authorization_client
                .create_or_update_role_assignment_by_id(
                    full_assignment_id.clone(),
                    &role_assignment,
                )
                .await;

            if let Err(e) = role_assignment_result {
                if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                    warn!(
                        "ACR registry not found for image {}, falling back to public image",
                        container_image
                    );
                    container_image = default_container_image.clone();
                    (vec![], None)
                } else {
                    panic!("Failed to create role assignment: {:?}", e);
                }
            } else {
                info!("✅ Assigned AcrPull role to managed identity");

                // Track the role assignment for cleanup
                created_role_assignments.insert(full_assignment_id.clone());

                // Wait for role assignment to propagate
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                let registries = vec![RegistryCredentials {
                    server: Some(acr_server.to_string()),
                    identity: Some(identity_resource_id.clone()),
                    ..Default::default()
                }];

                // Create managed identity configuration for the container app
                let mut user_assigned_identities = std::collections::HashMap::new();
                user_assigned_identities.insert(
                    identity_resource_id.clone(),
                    UserAssignedIdentity::default(),
                );

                let identity = Some(ManagedServiceIdentity {
                    type_: ManagedServiceIdentityType::UserAssigned,
                    user_assigned_identities: Some(UserAssignedIdentities(
                        user_assigned_identities,
                    )),
                    principal_id: None,
                    tenant_id: None,
                });

                info!("✅ Configured managed identity and registry credentials");

                (registries, identity)
            }
        } else {
            info!("ℹ️ Using public container image, no ACR authentication needed");
            (vec![], None)
        };

        // Create the Container App
        let container_app = ContainerApp {
            location: "eastus".to_string(),
            identity,
            properties: Some(ContainerAppProperties {
                environment_id: Some(managed_environment_id.clone()),
                template: Some(Template {
                    containers: vec![
                        AzureContainer {
                            name: Some("main".to_string()),
                            image: Some(container_image.clone()),
                            env: vec![],
                            resources: Some(alien_azure_clients::models::container_apps::ContainerResources {
                                cpu: Some(0.5),
                                memory: Some("1Gi".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }
                    ],
                    scale: Some(Scale {
                        min_replicas: Some(1),
                        max_replicas: 10,
                        rules: vec![],
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                configuration: Some(Configuration {
                    ingress: Some(Ingress {
                        external: true,
                        target_port: Some(8080),
                        traffic: vec![
                            TrafficWeight {
                                latest_revision: true,
                                weight: Some(100),
                                ..Default::default()
                            }
                        ],
                        transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
                        ..Default::default()
                    }),
                    registries,
                    active_revisions_mode: alien_azure_clients::models::container_apps::ConfigurationActiveRevisionsMode::Single,
                    ..Default::default()
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: Some(managed_environment_id.clone()),
                running_status: None,
                workload_profile_name: None,
                provisioning_state: None,
                event_stream_endpoint: None,
            }),
            tags: Default::default(),
            id: None,
            name: None,
            type_: None,
            managed_by: None,
            system_data: None,
            extended_location: None,
        };

        let create_result = container_apps_client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .expect("Failed to create test Container App");

        // Wait for the ARM operation to complete
        create_result
            .wait_for_operation_completion(
                &long_running_operation_client,
                "CreateContainerApp",
                &container_app_name,
            )
            .await
            .expect("Failed to wait for Container App creation");

        info!("✅ Created Container App: {}", container_app_name);

        // Wait for container app to be ready
        let mut attempts = 0;
        let max_attempts = 12; // Increased from 6 to 12
        loop {
            attempts += 1;

            match container_apps_client
                .get_container_app(&resource_group_name, &container_app_name)
                .await
            {
                Ok(app) => {
                    if let Some(props) = &app.properties {
                        if let Some(state) = &props.provisioning_state {
                            info!(
                                "📊 Container app provisioning state: {:?} (attempt {}/{})",
                                state, attempts, max_attempts
                            );

                            if *state == alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState::Succeeded {
                                info!("✅ Container app is ready!");
                                break;
                            }

                            if *state == alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState::Failed {
                                panic!("❌ Container app provisioning failed");
                            }
                        }
                    }

                    if attempts >= max_attempts {
                        panic!("⚠️  Container app didn't become ready within timeout");
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    // Increased from 10 to 15 seconds
                }
                Err(e) => {
                    panic!("Failed to get container app status: {:?}", e);
                }
            }
        }

        // Additional wait time for the container to start responding to HTTP requests
        info!("⏳ Waiting additional time for container to be ready for HTTP requests...");
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        // Get the created app to get its URL
        let created_app = container_apps_client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
            .expect("Failed to get created container app");

        let app_url = created_app
            .properties
            .and_then(|props| props.configuration)
            .and_then(|config| config.ingress)
            .and_then(|ingress| ingress.fqdn)
            .map(|fqdn| format!("https://{}", fqdn))
            .expect("Container app should have a valid FQDN after creation");

        let binding = WorkerBinding::container_app(
            subscription_id.clone(),
            resource_group_name.clone(),
            container_app_name.clone(),
            app_url,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AZURE_TENANT_ID".to_string(), client_config.tenant_id);

        // Extract credentials based on the type
        let (azure_client_id, azure_client_secret) = match &client_config.credentials {
            alien_azure_clients::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } => (client_id.clone(), client_secret.clone()),
            alien_azure_clients::AzureCredentials::AccessToken { .. } => {
                panic!("AccessToken credentials not supported in worker binding tests")
            }
            alien_azure_clients::AzureCredentials::ScopedAccessTokens { .. } => {
                panic!("ScopedAccessTokens credentials not supported in worker binding tests")
            }
            alien_azure_clients::AzureCredentials::SasToken { .. } => {
                panic!("SasToken credentials not supported in worker binding tests")
            }
            alien_azure_clients::AzureCredentials::WorkloadIdentity { client_id, .. } => {
                panic!("WorkloadIdentity credentials not fully supported in worker binding tests, client_id: {}", client_id)
            }
            alien_azure_clients::AzureCredentials::ManagedIdentity { client_id, .. } => {
                panic!("ManagedIdentity credentials not supported in worker binding tests, client_id: {}", client_id)
            }
            alien_azure_clients::AzureCredentials::VmManagedIdentity { .. } => {
                panic!("VmManagedIdentity credentials not supported in worker binding tests")
            }
        };

        env_map.insert("AZURE_CLIENT_ID".to_string(), azure_client_id);
        env_map.insert("AZURE_CLIENT_SECRET".to_string(), azure_client_secret);
        env_map.insert("AZURE_SUBSCRIPTION_ID".to_string(), subscription_id);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load Azure bindings provider"),
        );
        let function = provider
            .load_worker(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Azure function for binding '{}' using container app '{}': {:?}",
                    binding_name, container_app_name, e
                )
            });

        let mut created_container_apps = HashSet::new();
        created_container_apps.insert(container_app_name.clone());

        Self {
            function,
            resource_group_name,
            container_app_name,
            container_apps_client,
            authorization_client,
            managed_identity_client,
            long_running_operation_client,
            managed_environment_id,
            location: "eastus".to_string(),
            container_image,
            created_container_apps: Mutex::new(created_container_apps),
            created_managed_identities: Mutex::new(created_managed_identities),
            created_role_assignments: Mutex::new(created_role_assignments),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Container Apps test cleanup...");

        // Cleanup role assignments first
        let role_assignments_to_cleanup = {
            let assignments = self.created_role_assignments.lock().unwrap();
            assignments.clone()
        };

        for assignment_id in role_assignments_to_cleanup {
            match self
                .authorization_client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => info!("✅ Role assignment {} deleted successfully", assignment_id),
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

        // Cleanup managed identities
        let identities_to_cleanup = {
            let identities = self.created_managed_identities.lock().unwrap();
            identities.clone()
        };

        for identity_name in identities_to_cleanup {
            match self
                .managed_identity_client
                .delete_user_assigned_identity(&self.resource_group_name, &identity_name)
                .await
            {
                Ok(_) => info!("✅ Managed identity {} deleted successfully", identity_name),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!("🔍 Managed identity {} was already deleted", identity_name);
                }
                Err(e) => {
                    warn!(
                        "Failed to delete managed identity {} during cleanup: {:?}",
                        identity_name, e
                    );
                }
            }
        }

        // Cleanup container apps
        let container_apps_to_cleanup = {
            let apps = self.created_container_apps.lock().unwrap();
            apps.clone()
        };

        for container_app_name in container_apps_to_cleanup {
            match self
                .container_apps_client
                .delete_container_app(&self.resource_group_name, &container_app_name)
                .await
            {
                Ok(_) => info!(
                    "✅ Container app {} deleted successfully",
                    container_app_name
                ),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!(
                        "🔍 Container app {} was already deleted",
                        container_app_name
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to delete container app {} during cleanup: {:?}",
                        container_app_name, e
                    );
                }
            }
        }

        info!("✅ Container Apps test cleanup completed");
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl FunctionTestContext for AzureProviderTestContext {
    async fn get_function(&self) -> Arc<dyn Worker> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    fn get_test_endpoint(&self) -> String {
        format!("{}/{}", self.resource_group_name, self.container_app_name)
    }
}
