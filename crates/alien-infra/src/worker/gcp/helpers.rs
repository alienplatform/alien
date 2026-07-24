use std::collections::HashMap;
use tracing::info;

use crate::core::{EnvironmentVariableBuilder, ResourcePermissionsHelper};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Network, ResourceDefinition, ResourceRef, Worker};
use alien_error::{AlienError, Context};
use alien_gcp_clients::cloudrun::{
    Ingress as CloudRunIngress, NetworkInterface, RevisionTemplate, Service, TrafficTarget,
    TrafficTargetAllocationType, VpcAccess, VpcEgress,
};
use alien_gcp_clients::iam::IamPolicy;

use super::support::*;
use super::GcpWorkerController;
#[cfg(feature = "test-utils")]
use super::GcpWorkerState;

// Separate impl block for helper methods
impl GcpWorkerController {
    // ─────────────── HELPER METHODS ────────────────────────────

    pub(super) async fn apply_command_topic_management_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        topic_name: &str,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let command_refs: Vec<_> = ctx
            .desired_stack
            .management()
            .profile()
            .and_then(|management_profile| management_profile.0.get(&config.id))
            .into_iter()
            .flat_map(|refs| refs.iter())
            .filter(|permission_set_ref| permission_set_ref.id() == "worker/dispatch-command")
            .cloned()
            .collect();

        let gcp_config = ctx.get_gcp_config()?;
        let mut permission_context = alien_permissions::PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(topic_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = alien_permissions::generators::GcpRuntimePermissionsGenerator::new();
        let mut all_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_management_bindings_for(
            ctx,
            &config.id,
            topic_name,
            &command_refs,
            &generator,
            &permission_context,
            alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
            &mut all_bindings,
        )
        .await?;

        let iam_policy = IamPolicy {
            version: Some(3),
            bindings: all_bindings,
            etag: None,
            kind: None,
            resource_id: None,
        };
        let bindings_count = iam_policy.bindings.len();

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        pubsub_client
            .set_topic_iam_policy(topic_name.to_string(), iam_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply management command permissions to Pub/Sub topic '{}'",
                    topic_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            worker = %config.id,
            topic = %topic_name,
            bindings_count,
            "Reconciled management command permissions on Pub/Sub topic"
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
            let ssl_cert_name = custom
                .certificate
                .gcp
                .as_ref()
                .map(|cert| cert.certificate_name.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires a GCP SSL certificate name".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                ssl_certificate_name: Some(ssl_cert_name),
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
            ssl_certificate_name: None,
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
                || self.ssl_certificate_name.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id) {
            Ok(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.ssl_certificate_name = domain_info.ssl_certificate_name;
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

    pub(super) fn unexpected_update_wrapper_state(
        resource_id: &str,
        handler: &str,
        state: GcpWorkerState,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("{handler} returned unexpected state during update: {state:?}"),
        })
    }

    pub(super) async fn ensure_global_address_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        address_name: &str,
    ) -> Result<String> {
        if let Some(ip_address) = &self.global_address_ip {
            return Ok(ip_address.clone());
        }

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;
        let address = compute_client
            .get_global_address(address_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get global address".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        let ip_address = address.address.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Global address has no IP".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        self.global_address_ip = Some(ip_address.clone());
        Ok(ip_address)
    }

    pub(super) async fn build_cloud_run_service(
        &self,
        service_name: &str,
        cfg: &Worker,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Service> {
        use alien_gcp_clients::cloudrun::{
            Container, ContainerPort, EnvVar, ResourceRequirements, Service,
        };

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's email
        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        let service_account = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();
        let service_account = Some(service_account);

        // Extract container image
        let image = match &cfg.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Worker '{}' is configured with source code, but only pre-built images are supported in alien-infra.",
                        cfg.id
                    ),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Resolve proxy URIs to native GAR URIs. Cloud Run can only pull from GAR.
        let image = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
            alien_core::image_rewrite::resolve_native_image_uri(&image, native_host)
                .unwrap_or(image)
        } else {
            image
        };

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, service_name)
            .await?;

        let env: Vec<EnvVar> = env_vars
            .into_iter()
            .map(|(name, value)| EnvVar {
                name,
                value: Some(value),
                value_source: None,
            })
            .collect();

        // Cloud Run gen2 requires `memory >= 512 Mi`; that's enforced by the
        // `WorkerMemoryCheck` preflight, so we trust cfg.memory_mb here.
        let mut limits = HashMap::new();
        limits.insert("memory".to_string(), format!("{}Mi", cfg.memory_mb));
        // Cloud Run automatically allocates CPU based on memory

        let resources = ResourceRequirements {
            limits: Some(limits),
            cpu_idle: Some(true),          // Allow CPU throttling when idle
            startup_cpu_boost: Some(true), // Boost CPU during startup
        };

        // Build container port
        let ports = vec![ContainerPort {
            name: Some("http1".to_string()),
            // NOTE: This must match the alien-worker-runtime port on alien-build/src/lib.rs
            container_port: Some(8080),
        }];

        // Build container
        let container = Container::builder()
            .name("worker".to_string())
            .image(image)
            .env(env)
            .resources(resources)
            .ports(ports)
            .build();

        let ingress = if cfg.public_endpoints.is_empty() {
            CloudRunIngress::IngressTrafficInternal
        } else {
            CloudRunIngress::IngressTrafficAll
        };

        // Get VPC access configuration if a Network resource exists
        let vpc_access = self.get_vpc_access(ctx)?;
        if vpc_access.is_some() {
            info!(name=%service_name, "Configuring Cloud Run service with Direct VPC Egress");
        }

        // Build revision template
        let mut revision_labels = HashMap::from([("worker".to_string(), cfg.id.clone())]);
        if self.image_pull_permission_retries > 0 {
            revision_labels.insert(
                "alien-image-pull-retry".to_string(),
                self.image_pull_permission_retries.to_string(),
            );
        }

        let template = RevisionTemplate::builder()
            .labels(revision_labels)
            .scaling(
                alien_gcp_clients::cloudrun::RevisionScaling::builder()
                    .min_instance_count(0) // Scale to zero
                    .maybe_max_instance_count(cfg.concurrency_limit.map(|c| c as i32))
                    .build(),
            )
            .timeout(format!("{}s", cfg.timeout_seconds))
            .maybe_service_account(service_account)
            .containers(vec![container])
            .execution_environment(
                alien_gcp_clients::cloudrun::ExecutionEnvironment::ExecutionEnvironmentGen2,
            )
            .max_instance_request_concurrency(1000) // Cloud Run default
            .maybe_vpc_access(vpc_access)
            .build();

        // Build traffic target
        let traffic = vec![TrafficTarget::builder()
            .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
            .percent(100)
            .build()];

        // Build service
        // When ingress is public, disable the IAM invoker check instead of adding
        // allUsers to IAM policy. This works even when the GCP organization has
        // domain-restricted sharing enabled (which blocks allUsers in IAM).
        let is_public = !cfg.public_endpoints.is_empty();
        let service = Service::builder()
            .description(format!("Runtime worker: {}", cfg.id))
            .labels(HashMap::from([
                ("resource-type".to_string(), "worker".to_string()),
                ("resource".to_string(), cfg.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
            ]))
            .ingress(ingress)
            .template(template)
            .traffic(traffic)
            .invoker_iam_disabled(is_public)
            .build();

        Ok(service)
    }

    /// Gets VPC access configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the network name and subnetwork name from the Network controller to configure
    /// the Cloud Run service with Direct VPC Egress.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_access(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcAccess>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::GcpNetworkController>(&network_ref)?;

        // Only configure VPC access if we have network and subnetwork names
        let network_name = match &network_state.network_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        let subnetwork_name = match &network_state.subnetwork_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        // Build Direct VPC Egress configuration using network interfaces
        let network_interface = NetworkInterface::builder()
            .network(network_name)
            .subnetwork(subnetwork_name)
            .build();

        Ok(Some(
            VpcAccess::builder()
                .egress(VpcEgress::AllTraffic)
                .network_interfaces(vec![network_interface])
                .build(),
        ))
    }

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        function_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        use crate::core::ResourceController;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let env_vars = EnvironmentVariableBuilder::try_new(initial_env)?
            .add_worker_runtime_env_vars(ctx, &worker_config.id, worker_config.timeout_seconds)?
            .add_linked_resources(links, ctx, function_name_for_error_logging)
            .await?
            .add_self_worker_binding(&worker_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Applies consolidated IAM policy (resource-scoped permissions + public access) in a single operation
    pub(super) async fn apply_consolidated_iam_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        enable_public_access: bool,
    ) -> Result<()> {
        use alien_gcp_clients::iam::Binding;

        let config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;

        // Get existing IAM policy to preserve any existing bindings
        let mut policy = client
            .get_service_iam_policy(gcp_config.region.clone(), service_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for Cloud Run service '{}' before applying bindings. Refusing to proceed to avoid overwriting existing bindings.", service_name),
                resource_id: Some(config.id.clone()),
            })?;

        // Step 1: Apply resource-scoped permissions from the stack
        let mut resource_bindings = Vec::new();
        self.collect_resource_scoped_bindings(ctx, service_name, &mut resource_bindings)
            .await?;

        // Step 2: Add public access binding if needed
        if enable_public_access {
            info!(service_name = %service_name, "Adding public access to IAM policy");
            let invoker_role = "roles/run.invoker".to_string();
            let all_users_member = "allUsers".to_string();

            // Check if binding already exists
            let binding_exists = policy
                .bindings
                .iter()
                .any(|b| b.role == invoker_role && b.members.contains(&all_users_member));

            if !binding_exists {
                // Find existing binding or create new one
                if let Some(binding) = policy.bindings.iter_mut().find(|b| b.role == invoker_role) {
                    if !binding.members.contains(&all_users_member) {
                        binding.members.push(all_users_member);
                    }
                } else {
                    policy.bindings.push(
                        Binding::builder()
                            .role(invoker_role)
                            .members(vec![all_users_member])
                            .build(),
                    );
                }
            }
        }

        // Step 3: Add resource-scoped bindings
        if !resource_bindings.is_empty() {
            info!(
                service_name = %service_name,
                bindings_count = resource_bindings.len(),
                "Adding resource-scoped permissions to IAM policy"
            );
            policy.bindings.extend(resource_bindings);
        }

        // Step 4: Apply the consolidated policy in one operation
        client
            .set_service_iam_policy(gcp_config.region.clone(), service_name.to_string(), policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply consolidated IAM policy to Cloud Run service '{}'",
                    service_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(service_name = %service_name, "Consolidated IAM policy applied successfully");
        Ok(())
    }

    /// Collect resource-scoped bindings without applying them
    async fn collect_resource_scoped_bindings(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        all_bindings: &mut Vec<alien_gcp_clients::iam::Binding>,
    ) -> Result<()> {
        use alien_permissions::{generators::GcpRuntimePermissionsGenerator, PermissionContext};

        let config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;

        // Build permission context for this specific worker resource
        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(service_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = GcpRuntimePermissionsGenerator::new();
        let type_prefix = "worker/";

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions
            let mut combined_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = profile.0.get(&config.id) {
                combined_refs.extend(
                    permission_set_refs
                        .iter()
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                combined_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if !combined_refs.is_empty() {
                info!(
                    service_name = %service_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing resource-scoped permissions for worker"
                );

                self.process_profile_permissions(
                    ctx,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to process permissions for profile '{}' on worker '{}'",
                        profile_name, service_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        // Process management SA permissions matching the worker resource type
        if let Some(management_profile) = ctx.desired_stack.management().profile() {
            let mut management_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = management_profile.0.get(&config.id) {
                management_refs.extend(
                    permission_set_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if let Some(wildcard_refs) = management_profile.0.get("*") {
                management_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .filter(|r| r.id() != "worker/dispatch-command")
                        .cloned(),
                );
            }

            if !management_refs.is_empty() {
                use crate::core::ResourcePermissionsHelper;
                ResourcePermissionsHelper::collect_gcp_management_bindings_for(
                    ctx,
                    &config.id,
                    service_name,
                    &management_refs,
                    &generator,
                    &permission_context,
                    alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
                    all_bindings,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &alien_permissions::generators::GcpRuntimePermissionsGenerator,
        permission_context: &alien_permissions::PermissionContext,
        all_bindings: &mut Vec<alien_gcp_clients::iam::Binding>,
    ) -> Result<()> {
        use alien_gcp_clients::iam::{Binding, Expr};
        use alien_permissions::BindingTarget;

        // Get the service account email for this profile
        let service_account_email =
            self.get_service_account_email_for_profile(ctx, profile_name)?;

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;
            let selected_bindings = grant_plan.bindings_for_target(
                alien_permissions::generators::GcpBindingTargetScope::CurrentResource,
            );

            // Convert and add bindings
            let member = format!("serviceAccount:{}", service_account_email);
            for binding in selected_bindings {
                all_bindings.push(Binding {
                    role: binding.role,
                    members: vec![member.clone()],
                    condition: binding.condition.map(|cond| Expr {
                        title: Some(cond.title),
                        description: Some(cond.description),
                        expression: cond.expression,
                        location: None,
                    }),
                });
            }
        }

        Ok(())
    }

    /// Get the service account email for a permission profile
    fn get_service_account_email_for_profile(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "worker".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: GcpWorkerState::Ready,
            service_name: Some(function_name.to_string()),
            url: Some(format!("https://{}-abcd1234-uc.a.run.app", function_name)),
            operation_name: None,
            image_pull_permission_retries: 0,
            compute_operation_name: None,
            compute_operation_region: None,
            push_subscriptions: Vec::new(),
            storage_notification_topics: Vec::new(),
            gcs_notification_ids: Vec::new(),
            scheduler_job_names: Vec::new(),
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            global_address_ip: None,
            forwarding_rule_name: None,
            project_id: Some("test-project".to_string()),
            region: Some("us-central1".to_string()),
            commands_topic_name: None,
            commands_subscription_name: None,
            _internal_stay_count: None,
        }
    }
}
