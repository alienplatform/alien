use super::*;

impl AzureWorkerController {
    // ─────────────── HELPER METHODS ────────────────────────────

    /// Pre-create commands infrastructure (queue, Dapr component, role assignments)
    /// before the Container App is created. This ensures the Dapr sidecar starts
    /// with the component already defined and RBAC roles already propagating.
    pub(super) async fn setup_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &alien_azure_clients::AzureClientConfig,
        func_cfg: &alien_core::Worker,
        container_app_name: &str,
    ) -> Result<DaprComponentOperation> {
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let env_resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get the Service Bus namespace from the dependent resource
        let namespace_ref = alien_core::ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: namespace_ref.id.clone(),
                })
            })?
            .clone();
        let service_bus_resource_group = namespace_controller.resource_group_name(ctx)?;

        // Create commands queue
        let queue_name = format!("{}-rq", container_app_name);
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        info!(
            worker=%func_cfg.id,
            namespace=%namespace_name,
            queue=%queue_name,
            "Pre-creating commands Service Bus queue (before Container App)"
        );

        mgmt.create_or_update_queue(
            service_bus_resource_group.clone(),
            namespace_name.clone(),
            queue_name.clone(),
            alien_azure_clients::models::queue::SbQueueProperties::default(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        // Create Dapr component for commands queue
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let ns_fqdn = format!("{}.servicebus.windows.net", namespace_name);
        let component_name = format!("servicebus-{}-commands", container_app_name);

        let mut metadata = vec![
            DaprMetadata {
                name: Some("namespaceName".into()),
                value: Some(ns_fqdn),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("queueName".into()),
                value: Some(queue_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("direction".into()),
                value: Some("input".into()),
                secret_ref: None,
            },
        ];

        // Add client ID for user-assigned managed identity
        let service_account_id = format!("{}-sa", func_cfg.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        if let Ok(sa_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &sa_state.identity_client_id {
                metadata.push(DaprMetadata {
                    name: Some("azureClientId".into()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        info!(
            worker=%func_cfg.id,
            component=%component_name,
            "Pre-creating commands Dapr Service Bus component (before Container App)"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        match client
            .create_or_update_dapr_component(
                &env_resource_group_name,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create commands Dapr component '{}'",
                    component_name
                ),
                resource_id: Some(func_cfg.id.clone()),
            })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        self.commands_namespace_name = Some(namespace_name.clone());
        self.commands_queue_name = Some(queue_name);
        self.commands_dapr_component = Some(component_name);

        // Command transport RBAC is setup-applied before live worker provisioning.
        self.assign_commands_sender_role(
            ctx,
            azure_config,
            &service_bus_resource_group,
            &namespace_name,
            func_cfg,
        )
        .await?;

        info!(worker=%func_cfg.id, "Commands infrastructure pre-created successfully");
        Ok(DaprComponentOperation::Completed)
    }

    /// Ensure command transport permissions are represented in the management profile.
    ///
    /// Azure command transport uses Service Bus data-plane roles. Those grants are
    /// setup-owned because both Terraform setup and runtime setup know the stack
    /// management and execution identities before live workers are created.
    pub(super) async fn assign_commands_sender_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        azure_config: &alien_azure_clients::AzureClientConfig,
        resource_group_name: &str,
        namespace_name: &str,
        func_cfg: &alien_core::Worker,
    ) -> Result<()> {
        if !management_profile_dispatches_commands(ctx, &func_cfg.id) {
            info!(
                worker = %func_cfg.id,
                "Skipping management command sender role because worker/dispatch-command is not granted"
            );
            return Ok(());
        }

        if AzurePermissionsHelper::get_management_uami_principal_id(ctx)?.is_none() {
            if self.commands_sender_role_assignment_id.is_some() {
                return Ok(());
            }

            let queue_name = self.commands_queue_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: "commands-queue".to_string(),
                })
            })?;
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;
            let principal_id = ctx
                .service_provider
                .get_azure_caller_principal_id(azure_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to resolve Azure command sender principal".to_string(),
                    resource_id: Some(func_cfg.id.clone()),
                })?;
            let queue_scope = alien_azure_clients::authorization::Scope::Resource {
                resource_group_name: resource_group_name.to_string(),
                resource_provider: "Microsoft.ServiceBus".to_string(),
                parent_resource_path: Some(format!("namespaces/{namespace_name}")),
                resource_type: "queues".to_string(),
                resource_name: queue_name.clone(),
            };
            let role_assignment_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!(
                    "deployment:azure:commands-sender:{}:{}:{}:{}:{}",
                    ctx.resource_prefix, func_cfg.id, principal_id, namespace_name, queue_name
                )
                .as_bytes(),
            )
            .to_string();
            let full_assignment_id = authorization_client
                .build_role_assignment_id(&queue_scope, role_assignment_id.clone());
            let role_definition_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/69a216fc-b8fb-44d8-bc22-1f3c2cd27a39",
                azure_config.subscription_id
            );

            AzurePermissionsHelper::create_role_assignment(
                &authorization_client,
                azure_config,
                &queue_scope,
                &role_assignment_id,
                &principal_id,
                &role_definition_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to grant Azure command sender role".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

            self.commands_sender_role_assignment_id = Some(full_assignment_id);
            info!(
                worker = %func_cfg.id,
                principal_id = %principal_id,
                namespace = %namespace_name,
                queue = %queue_name,
                "Granted direct-manager Azure Service Bus command sender role"
            );
            return Ok(());
        }

        info!(
            worker = %func_cfg.id,
            "Using setup-applied Azure Service Bus command permissions"
        );

        Ok(())
    }

    /// Resolve domain information for a public worker.
    /// Returns either custom domain config or auto-generated domain from metadata.
    pub(super) fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;

        // Check for custom domain configuration
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let keyvault_cert_id = custom
                .certificate
                .azure
                .as_ref()
                .map(|cert| cert.key_vault_certificate_id.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an Azure Key Vault certificate ID"
                            .to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                keyvault_cert_id: Some(keyvault_cert_id),
                container_apps_certificate_id: None,
                uses_custom_domain: true,
            });
        }

        // Use auto-generated domain from domain metadata
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
        })
    }

    pub(super) fn ensure_domain_info(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        if self.fqdn.is_some()
            && (self.certificate_id.is_some()
                || self.keyvault_cert_id.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id) {
            Ok(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.keyvault_cert_id = domain_info.keyvault_cert_id;
                self.container_apps_certificate_id = domain_info.container_apps_certificate_id;
                self.uses_custom_domain = domain_info.uses_custom_domain;
                if self.url.is_none() {
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(resource_id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    pub(super) fn clear_all(&mut self) {
        self.container_app_name = None;
        self.resource_id = None;
        self.url = None;
        self.container_app_url = None;
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        self.dapr_components.clear();
        self.storage_trigger_infrastructure.clear();
    }

    /// Called whenever provisioning *succeeds* and we have the live resource.
    pub(super) fn handle_creation_completed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        app: &ContainerApp,
    ) {
        self.resource_id = app.id.clone();

        let container_app_url = self.extract_url_from_container_app(app);

        // Capture the ingress host (DNS CNAME target) before `url` is overridden below.
        self.container_app_url = container_app_url.clone();

        // Check for URL override in deployment config, otherwise use Container App URL
        if let Ok(config) = ctx.desired_resource_config::<Worker>() {
            self.url = ctx
                .deployment_config
                .public_endpoints
                .as_ref()
                .and_then(|resources| resources.get(&config.id))
                .and_then(|endpoints| endpoints.values().next().cloned())
                .or(container_app_url);
        } else {
            self.url = container_app_url;
        }

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
    }

    pub(super) fn set_custom_domain(app: &mut ContainerApp, fqdn: String, certificate_id: String) {
        if let Some(props) = &mut app.properties {
            if let Some(config) = &mut props.configuration {
                if let Some(ingress) = &mut config.ingress {
                    ingress.custom_domains = vec![CustomDomain {
                        name: fqdn,
                        binding_type: Some(CustomDomainBindingType::SniEnabled),
                        certificate_id: Some(certificate_id),
                    }];
                }
            }
        }
    }

    pub(super) fn extract_url_from_container_app(&self, app: &ContainerApp) -> Option<String> {
        let fqdn = app
            .properties
            .as_ref()?
            .configuration
            .as_ref()?
            .ingress
            .as_ref()?
            .fqdn
            .clone()?;

        if fqdn.starts_with("http://") || fqdn.starts_with("https://") {
            Some(fqdn)
        } else {
            Some(format!("https://{}", fqdn))
        }
    }

    /// Prepare environment variables using the shared logic, then convert to Azure's EnvironmentVar format
    pub(super) async fn prepare_environment_variables_azure(
        &self,
        func: &Worker,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<EnvironmentVar>> {
        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        // Build complete environment using shared logic
        // IMPORTANT: Start with func.environment which includes injected vars from DeploymentConfig
        let complete_env = EnvironmentVariableBuilder::try_new(&func.environment)?
            .add_worker_runtime_env_vars(ctx, &func.id, func.timeout_seconds)?
            .add_linked_resources(&func.links, ctx, &func.id)
            .await?
            .add_self_worker_binding(&func.id, self_binding_params.as_ref())?
            .build();

        // Add managed identity environment variable from ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let mut env_vars = Vec::new();

        // Convert all environment variables to Azure format
        for (name, value) in complete_env {
            env_vars.push(EnvironmentVar {
                name: Some(name),
                value: Some(value),
                secret_ref: None,
            });
        }

        // Add Azure-specific managed identity client ID
        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &service_account_state.identity_client_id {
                env_vars.push(EnvironmentVar {
                    name: Some(ENV_AZURE_CLIENT_ID.to_string()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        Ok(env_vars)
    }

    /// Build the full ContainerApps ARM spec for *desired* state.
    pub(super) async fn build_container_app(
        &self,
        func: &Worker,
        _environment_name: &str,
        container_app_name: &str,
        azure_cfg: &AzureClientConfig,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<ContainerApp> {
        let location = azure_cfg.region.as_deref().unwrap_or("East US");

        let image = match &func.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Worker '{}' uses source code, but only pre‑built images are supported on Azure",
                        func.id
                    ),
                    resource_id: Some(func.id.clone()),
                }));
            }
        };

        // Prepare environment variables using shared logic
        let env_vars = self.prepare_environment_variables_azure(func, ctx).await?;

        // Note: Dapr input bindings (bindings.azure.servicebusqueues) auto-deliver
        // messages without requiring GET /dapr/subscribe. No subscription env var needed.

        // Azure Container Apps requires specific CPU/memory combinations.
        // The ratio is 0.5 Gi per 0.25 CPU (2 Gi per 1 CPU).
        let memory_gi = func.memory_mb as f64 / 1024.0;
        // Azure Container Apps requires specific CPU/memory pairs where CPU = memory_gi / 2.
        // The WorkerMemoryCheck preflight validates that memory_mb is a valid Azure value
        // (512, 1024, 1536, 2048, 2560, 3072, 3584, 4096).
        let cpu = memory_gi / 2.0;

        let container = Container {
            name: Some("main".to_string()),
            image: Some(image.clone()),
            resources: Some(ContainerResources {
                cpu: Some(cpu),
                memory: Some(format!("{}Gi", memory_gi)),
                ephemeral_storage: None,
            }),
            env: env_vars,
            args: vec![],
            command: vec![],
            probes: vec![],
            volume_mounts: vec![],
        };

        // Tags for traceability
        let mut tags = HashMap::new();
        tags.insert("resource-type".to_string(), "worker".to_string());
        tags.insert("resource".to_string(), func.id.clone());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

        let _resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_id = azure_utils::get_container_apps_environment_resource_id(ctx.state)?;

        let ingress_cfg = if !func.public_endpoints.is_empty() {
            Some(alien_azure_clients::models::container_apps::Ingress {
                external: true,
                target_port: Some(8080),
                traffic: vec![TrafficWeight {
                    weight: Some(100),
                    latest_revision: true,
                    revision_name: None,
                    label: None,
                }],
                transport: IngressTransport::Auto,
                allow_insecure: false,
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
                fqdn: None,
            })
        } else {
            None
        };

        let mut registries = vec![];
        let mut secrets = vec![];

        // Managed identity support from ServiceAccounts
        // Collect all ServiceAccounts:
        // 1. Permission-based ServiceAccount (from permission profile)
        // 2. Linked ServiceAccounts (from worker.links)
        use alien_azure_clients::models::container_apps::{
            ManagedServiceIdentity, ManagedServiceIdentityType, UserAssignedIdentities,
            UserAssignedIdentity,
        };

        let mut identity_map = HashMap::new();

        // Add permission-based ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(identity_id) = &service_account_state.identity_resource_id {
                identity_map.insert(
                    identity_id.clone(),
                    UserAssignedIdentity {
                        client_id: None,
                        principal_id: None,
                    },
                );
            }
        }

        // Add linked ServiceAccounts
        for link in &func.links {
            if link.resource_type() == &alien_core::ServiceAccount::RESOURCE_TYPE {
                if let Ok(linked_sa_state) = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                    link,
                ) {
                    if let Some(identity_id) = &linked_sa_state.identity_resource_id {
                        identity_map.insert(
                            identity_id.clone(),
                            UserAssignedIdentity {
                                client_id: None,
                                principal_id: None,
                            },
                        );
                    }
                }
            }
        }

        // Configure registry credentials for image pull.
        // The image URI points at the manager's registry (proxy URI from release).
        // Add Basic auth with the deployment token so the Container App can pull.
        let registry_token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "deployment_token is required for Azure Container Apps to pull images from the registry proxy".to_string(),
                resource_id: Some(func.id.clone()),
            })
        })?;
        let registry_server = image.split('/').next().unwrap_or_default().to_string();
        let secret_name = "registry-proxy-password";
        secrets.push(Secret {
            name: Some(secret_name.to_string()),
            value: Some(registry_token.clone()),
            identity: None,
            key_vault_url: None,
        });
        registries.push(RegistryCredentials {
            identity: None,
            password_secret_ref: Some(secret_name.to_string()),
            server: Some(registry_server),
            username: Some("deployment".to_string()),
        });

        // Create managed identity spec if we have any identities
        let identity_resource_ids: Vec<String> = identity_map.keys().cloned().collect();

        let managed_identity = if !identity_map.is_empty() {
            Some(ManagedServiceIdentity {
                principal_id: None,
                tenant_id: None,
                type_: ManagedServiceIdentityType::UserAssigned,
                user_assigned_identities: Some(UserAssignedIdentities(identity_map)),
            })
        } else {
            None
        };

        // Configure Dapr if the worker uses any triggers or commands.
        // Dapr handles delivery for queue (Service Bus), storage (blob), and cron triggers.
        let needs_dapr = func.commands_enabled || !func.triggers.is_empty();
        let dapr_config = if needs_dapr {
            use alien_azure_clients::models::container_apps::{Dapr, DaprAppProtocol};

            Some(Dapr {
                app_id: Some(container_app_name.to_string()),
                app_port: Some(8080), // Port that alien-worker-runtime listens on
                app_protocol: DaprAppProtocol::Http,
                enable_api_logging: Some(false),
                enabled: true,
                http_max_request_size: None,
                http_read_buffer_size: None,
                log_level: None,
            })
        } else {
            None
        };

        let configuration = Configuration {
            active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
            dapr: dapr_config,
            identity_settings: identity_resource_ids
                .iter()
                .map(|identity_id| IdentitySettings {
                    identity: identity_id.clone(),
                    lifecycle: IdentitySettingsLifecycle::All,
                })
                .collect(),
            ingress: ingress_cfg,
            max_inactive_revisions: None,
            registries,
            runtime: None,
            secrets,
            service: None,
        };

        let template = Template {
            containers: vec![container],
            init_containers: vec![],
            revision_suffix: None,
            scale: Some(Scale {
                cooldown_period: None,
                max_replicas: func.concurrency_limit.map(|c| c as i32).unwrap_or(10),
                min_replicas: Some(if func.public_endpoints.is_empty() {
                    0
                } else {
                    1
                }),
                polling_interval: None,
                rules: vec![],
            }),
            service_binds: vec![],
            termination_grace_period_seconds: None,
            volumes: vec![],
        };

        Ok(ContainerApp {
            extended_location: None,
            id: None,
            identity: managed_identity,
            location: location.to_string(),
            managed_by: None,
            name: None,
            properties: Some(ContainerAppProperties {
                configuration: Some(configuration),
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: Some(environment_id),
                outbound_ip_addresses: vec![],
                provisioning_state: None,
                running_status: None,
                template: Some(template),
                workload_profile_name: None,
            }),
            system_data: None,
            tags,
            type_: None,
        })
    }

    /// Creates a Dapr Service Bus component for a queue trigger
    pub(super) async fn create_dapr_service_bus_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<DaprComponentOperation> {
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        // Get queue controller to access Service Bus namespace
        let queue_controller =
            ctx.require_dependency::<crate::queue::azure::AzureQueueController>(queue_ref)?;
        let namespace = queue_controller.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let ns_fqdn = format!("{}.servicebus.windows.net", namespace);

        // Generate component name: servicebus-{containerAppName}-{queueId}
        let component_name = format!("servicebus-{}-{}", container_app_name, queue_ref.id);

        // Use Dapr input binding — the manager/user code sends directly to Service Bus
        // via Azure SDK, not through Dapr pubsub. Input bindings auto-deliver from the
        // named queue without requiring GET /dapr/subscribe subscriptions.
        let queue_name = queue_controller.queue_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: {
                    let mut metadata = vec![
                        DaprMetadata {
                            name: Some("namespaceName".into()),
                            value: Some(ns_fqdn),
                            secret_ref: None,
                        },
                        DaprMetadata {
                            name: Some("queueName".into()),
                            value: Some(queue_name.clone()),
                            secret_ref: None,
                        },
                        DaprMetadata {
                            name: Some("direction".into()),
                            value: Some("input".into()),
                            secret_ref: None,
                        },
                    ];

                    // Add client ID for user-assigned managed identity
                    let service_account_id = format!("{}-sa", worker_config.get_permissions());
                    let service_account_ref = alien_core::ResourceRef::new(
                        alien_core::ServiceAccount::RESOURCE_TYPE,
                        service_account_id.to_string(),
                    );

                    if let Ok(service_account_state) = ctx.require_dependency::<crate::service_account::AzureServiceAccountController>(&service_account_ref) {
                        if let Some(client_id) = &service_account_state.identity_client_id {
                            metadata.push(DaprMetadata {
                                name: Some("azureClientId".into()),
                                value: Some(client_id.clone()),
                                secret_ref: None
                            });
                        }
                    }

                    metadata
                },
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        info!(
            worker=%worker_config.id,
            queue=%queue_ref.id,
            component=%component_name,
            environment=%environment_name,
            "Creating Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match client
            .create_or_update_dapr_component(
                &resource_group_name,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Dapr component '{}' for queue '{}'",
                    component_name, queue_ref.id
                ),
                resource_id: Some(worker_config.id.clone()),
            })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            "Successfully created Dapr Service Bus component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates supported Azure storage-trigger delivery:
    /// Event Grid -> dedicated Service Bus queue -> Dapr Service Bus input binding.
    pub(super) async fn create_azure_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<DaprComponentOperation> {
        use alien_azure_clients::authorization::Scope;
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let environment_resource_group = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let storage_controller =
            ctx.require_dependency::<crate::storage::azure::AzureStorageController>(storage_ref)?;
        let storage_account_name = storage_controller
            .storage_account_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: storage_ref.id.clone(),
                })
            })?
            .clone();
        let container_name = storage_controller
            .container_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: storage_ref.id.clone(),
                })
            })?
            .clone();

        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: namespace_ref.id.clone(),
                })
            })?
            .clone();
        let service_bus_resource_group = namespace_controller.resource_group_name(ctx)?;

        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.clone(),
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let execution_client_id = service_account
            .identity_client_id
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: service_account_id.clone(),
                })
            })?
            .clone();
        let execution_principal_id = service_account
            .identity_principal_id
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: service_account_id,
                })
            })?
            .clone();

        let queue_name = format!("{}-storage-{}", container_app_name, storage_ref.id);
        let component_name = format!("blobstorage-{}-{}", container_app_name, storage_ref.id);
        let event_subscription_name =
            get_azure_storage_event_subscription_name(&worker_config.id, &storage_ref.id);
        let deployment_resource_group = get_resource_group_name(ctx.state)?;
        let source_resource_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
            azure_config.subscription_id, deployment_resource_group, storage_account_name
        );
        let queue_resource_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
            azure_config.subscription_id,
            service_bus_resource_group,
            namespace_name,
            queue_name
        );

        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;
        mgmt.create_or_update_queue(
            service_bus_resource_group.clone(),
            namespace_name.clone(),
            queue_name.clone(),
            alien_azure_clients::models::queue::SbQueueProperties::default(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create storage-trigger Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(worker_config.id.clone()),
        })?;

        let tracker_index = self
            .storage_trigger_infrastructure
            .iter()
            .position(|item| item.event_subscription_name == event_subscription_name);
        let tracker_index = match tracker_index {
            Some(index) => {
                let tracker = &mut self.storage_trigger_infrastructure[index];
                tracker.source_resource_id = source_resource_id.clone();
                tracker.service_bus_resource_group = service_bus_resource_group.clone();
                tracker.namespace_name = namespace_name.clone();
                tracker.queue_name = queue_name.clone();
                index
            }
            None => {
                self.storage_trigger_infrastructure
                    .push(AzureStorageTriggerInfrastructure {
                        source_resource_id: source_resource_id.clone(),
                        event_subscription_name: event_subscription_name.clone(),
                        service_bus_resource_group: service_bus_resource_group.clone(),
                        namespace_name: namespace_name.clone(),
                        queue_name: queue_name.clone(),
                        receiver_role_assignment_id: None,
                    });
                self.storage_trigger_infrastructure.len() - 1
            }
        };

        if self.storage_trigger_infrastructure[tracker_index]
            .receiver_role_assignment_id
            .is_none()
        {
            let queue_scope = Scope::Resource {
                resource_group_name: service_bus_resource_group.clone(),
                resource_provider: "Microsoft.ServiceBus".to_string(),
                parent_resource_path: Some(format!("namespaces/{namespace_name}")),
                resource_type: "queues".to_string(),
                resource_name: queue_name.clone(),
            };
            let role_assignment_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!(
                    "deployment:azure:storage-trigger-receiver:{}:{}:{}:{}",
                    ctx.resource_prefix, worker_config.id, storage_ref.id, execution_principal_id
                )
                .as_bytes(),
            )
            .to_string();
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;
            let full_assignment_id = authorization_client
                .build_role_assignment_id(&queue_scope, role_assignment_id.clone());
            let role_definition_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0",
                azure_config.subscription_id
            );
            AzurePermissionsHelper::create_role_assignment(
                &authorization_client,
                azure_config,
                &queue_scope,
                &role_assignment_id,
                &execution_principal_id,
                &role_definition_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to grant the worker access to storage-trigger queue '{}'",
                    queue_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;
            self.storage_trigger_infrastructure[tracker_index].receiver_role_assignment_id =
                Some(full_assignment_id);
        }

        let metadata = vec![
            DaprMetadata {
                name: Some("namespaceName".into()),
                value: Some(format!("{}.servicebus.windows.net", namespace_name)),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("queueName".into()),
                value: Some(queue_name.clone()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("direction".into()),
                value: Some("input".into()),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("azureClientId".into()),
                value: Some(execution_client_id),
                secret_ref: None,
            },
        ];

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.azure.servicebusqueues".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match container_apps_client
            .create_or_update_dapr_component(
                &environment_resource_group,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Dapr component '{}' for storage trigger '{}'",
                    component_name, storage_ref.id
                ),
                resource_id: Some(worker_config.id.clone()),
            })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        let event_grid_client = ctx
            .service_provider
            .get_azure_event_grid_client(azure_config)?;
        let event_subscription = match event_grid_client
            .get_event_subscription(source_resource_id.clone(), event_subscription_name.clone())
            .await
        {
            Ok(event_subscription) => event_subscription,
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                let included_event_types = azure_storage_event_types(events, &worker_config.id)?;
                event_grid_client
                    .create_or_update_event_subscription(
                        source_resource_id,
                        event_subscription_name.clone(),
                        EventSubscriptionRequest {
                            properties: EventSubscriptionRequestProperties {
                                destination: ServiceBusQueueDestination {
                                    endpoint_type: "ServiceBusQueue".to_string(),
                                    properties: ServiceBusQueueDestinationProperties {
                                        resource_id: queue_resource_id,
                                    },
                                },
                                filter: EventSubscriptionFilter {
                                    included_event_types,
                                    subject_begins_with: format!(
                                        "/blobServices/default/containers/{}/blobs/",
                                        container_name
                                    ),
                                    is_subject_case_sensitive: false,
                                },
                                event_delivery_schema: "CloudEventSchemaV1_0".to_string(),
                            },
                        },
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create Event Grid subscription '{}' for storage '{}'",
                            event_subscription_name, storage_ref.id
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    })?
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to inspect Event Grid subscription '{}' for storage '{}'",
                        event_subscription_name, storage_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        };

        if let Some(provisioning_state) = event_subscription
            .properties
            .and_then(|properties| properties.provisioning_state)
        {
            if !provisioning_state.eq_ignore_ascii_case("Succeeded") {
                info!(
                    worker=%worker_config.id,
                    subscription=%event_subscription_name,
                    state=%provisioning_state,
                    "Waiting for Event Grid storage subscription"
                );
                return Ok(DaprComponentOperation::Pending(Duration::from_secs(5)));
            }
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            subscription=%event_subscription_name,
            "Azure storage trigger delivery is ready"
        );

        Ok(DaprComponentOperation::Completed)
    }

    /// Creates a Dapr cron input binding for a schedule trigger
    pub(super) async fn create_dapr_cron_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        container_app_name: &str,
        worker_config: &alien_core::Worker,
        cron: &str,
        index: usize,
    ) -> Result<DaprComponentOperation> {
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();

        let component_name = format!("cron-{}-{}", container_app_name, index);

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("bindings.cron".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: vec![
                    DaprMetadata {
                        name: Some("schedule".into()),
                        value: Some(cron.to_string()),
                        secret_ref: None,
                    },
                    DaprMetadata {
                        name: Some("direction".into()),
                        value: Some("input".into()),
                        secret_ref: None,
                    },
                ],
                scopes: vec![container_app_name.to_string()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        match client
            .create_or_update_dapr_component(
                &resource_group_name,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Dapr cron component '{}' with schedule '{}'",
                    component_name, cron
                ),
                resource_id: Some(worker_config.id.clone()),
            })? {
            OperationResult::Completed(_) => {}
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                return Ok(DaprComponentOperation::LongRunning(
                    lro.retry_after.unwrap_or(Duration::from_secs(15)),
                ));
            }
        }

        if !self.dapr_components.contains(&component_name) {
            self.dapr_components.push(component_name.clone());
        }

        info!(
            worker=%worker_config.id,
            component=%component_name,
            schedule=%cron,
            "Successfully created Dapr cron component"
        );

        Ok(DaprComponentOperation::Completed)
    }

    pub(super) async fn delete_storage_trigger_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if self.storage_trigger_infrastructure.is_empty() {
            return Ok(());
        }

        let azure_config = ctx.get_azure_config()?;
        let event_grid_client = ctx
            .service_provider
            .get_azure_event_grid_client(azure_config)?;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        let service_bus_client = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        for infrastructure in self.storage_trigger_infrastructure.clone() {
            match event_grid_client
                .delete_event_subscription(
                    infrastructure.source_resource_id.clone(),
                    infrastructure.event_subscription_name.clone(),
                )
                .await
            {
                Ok(()) => info!(
                    subscription=%infrastructure.event_subscription_name,
                    "Deleted Event Grid storage subscription"
                ),
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete Event Grid storage subscription '{}'",
                            infrastructure.event_subscription_name
                        ),
                        resource_id: Some(ctx.desired_config.id().to_string()),
                    }));
                }
            }

            if let Some(assignment_id) = infrastructure.receiver_role_assignment_id {
                match authorization_client
                    .delete_role_assignment_by_id(assignment_id.clone())
                    .await
                {
                    Ok(_) => info!(
                        assignment_id=%assignment_id,
                        "Deleted storage-trigger receiver role assignment"
                    ),
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(
                            assignment_id=%assignment_id,
                            "Storage-trigger receiver role assignment was already deleted"
                        );
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete storage-trigger receiver role assignment '{}'",
                                assignment_id
                            ),
                            resource_id: Some(ctx.desired_config.id().to_string()),
                        }));
                    }
                }
            }

            match service_bus_client
                .delete_queue(
                    infrastructure.service_bus_resource_group,
                    infrastructure.namespace_name,
                    infrastructure.queue_name.clone(),
                )
                .await
            {
                Ok(()) => info!(
                    queue=%infrastructure.queue_name,
                    "Deleted storage-trigger Service Bus queue"
                ),
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        queue=%infrastructure.queue_name,
                        "Storage-trigger Service Bus queue was already deleted"
                    );
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete storage-trigger Service Bus queue '{}'",
                            infrastructure.queue_name
                        ),
                        resource_id: Some(ctx.desired_config.id().to_string()),
                    }));
                }
            }
        }

        self.storage_trigger_infrastructure.clear();
        Ok(())
    }

    /// Deletes all Dapr components using best-effort approach
    pub(super) async fn delete_all_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if self.dapr_components.is_empty() {
            return Ok(());
        }

        let azure_config = ctx.get_azure_config()?;
        // Dapr components live on the Container Apps Environment, which may be in a
        // different resource group than the deployment (shared/external environments).
        let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
        let resource_group_name = env_outputs.resource_group_name.clone();
        let environment_name = env_outputs.environment_name.clone();
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        for component_name in &self.dapr_components.clone() {
            match client
                .delete_dapr_component(&resource_group_name, &environment_name, component_name)
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        component=%component_name,
                        "Dapr component delete requested"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        worker=%worker_config.id,
                        component=%component_name,
                        "Dapr component was already deleted or doesn't exist"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        component=%component_name,
                        error=%e,
                        "Failed to delete Dapr component during deletion"
                    );
                }
            }
        }

        self.dapr_components.clear();
        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: AzureWorkerState::Ready,
            container_app_name: Some(function_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                function_name
            )),
            url: Some(format!("https://{}.azurecontainerapps.io", function_name)),
            container_app_url: None,
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            storage_trigger_infrastructure: Vec::new(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_sender_role_assignment_id: None,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
            _internal_stay_count: None,
        }
    }
}
