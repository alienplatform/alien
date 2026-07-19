use std::collections::HashMap;
use tracing::{info, warn};

use crate::core::{EnvironmentVariableBuilder, ResourcePermissionsHelper};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Network, ResourceDefinition, ResourceRef, Worker};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::cloudrun::{
    Ingress as CloudRunIngress, NetworkInterface, RevisionTemplate, Service, TrafficTarget,
    TrafficTargetAllocationType, VpcAccess, VpcEgress,
};
use alien_gcp_clients::gcs::GcsNotification;
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription, Topic};

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

    /// Creates a Pub/Sub push subscription for a queue trigger
    pub(super) async fn create_push_subscription(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        // Get queue controller to access the topic name
        let queue_controller =
            ctx.require_dependency::<crate::queue::gcp::GcpQueueController>(queue_ref)?;
        let topic_name = queue_controller.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let topic_full_name = format!("projects/{}/topics/{}", gcp_config.project_id, topic_name);

        // Generate push subscription name: stack-prefix-worker-id-queue-id
        let subscription_name = format!(
            "{}-{}-{}",
            ctx.resource_prefix, worker_config.id, queue_ref.id
        );

        // Get the service URL for push endpoint
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for push subscription".to_string(),
            })
        })?;

        // Build push endpoint URL (Cloud Run service URL)
        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;
        let service_account_email = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Create push config with OIDC authentication
        let oidc_token = OidcToken {
            service_account_email: service_account_email.clone(),
            audience: Some(push_endpoint.clone()),
        };

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint.clone()),
            attributes: Some(std::collections::HashMap::new()),
            oidc_token: Some(oidc_token),
            pubsub_wrapper: None,
            no_wrapper: None,
        };

        let subscription = Subscription {
            name: Some(subscription_name.clone()),
            topic: Some(topic_full_name.clone()),
            push_config: Some(push_config),
            ack_deadline_seconds: Some(worker_config.timeout_seconds as i32),
            retain_acked_messages: Some(false),
            message_retention_duration: None,
            labels: Some(std::collections::HashMap::from([
                ("worker".to_string(), worker_config.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
            ])),
            enable_message_ordering: Some(false),
            expiration_policy: None,
            filter: None,
            dead_letter_policy: None,
            retry_policy: None,
            detached: Some(false),
            state: None,
            analytics_hub_subscription_info: None,
            bigquery_config: None,
            cloud_storage_config: None,
        };

        info!(
            worker=%worker_config.id,
            topic=%topic_full_name,
            subscription=%subscription_name,
            endpoint=%push_endpoint,
            "Creating Pub/Sub push subscription"
        );

        match pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    subscription=%subscription_name,
                    "Pub/Sub push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for queue '{}'",
                        subscription_name, queue_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name.clone());
        }

        info!(
            worker=%worker_config.id,
            subscription=%subscription_name,
            "Successfully created Pub/Sub push subscription"
        );

        Ok(())
    }

    /// Deletes all push subscriptions using best-effort approach
    pub(super) async fn delete_all_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.push_subscriptions.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for subscription_name in &self.push_subscriptions.clone() {
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        subscription=%subscription_name,
                        "Push subscription deleted successfully"
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
                        subscription=%subscription_name,
                        "Push subscription was already deleted (not found)"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }

        self.push_subscriptions.clear();
        Ok(())
    }

    /// Gets the service account email for the worker's permission profile.
    pub(super) fn get_service_account_email(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker_config: &alien_core::Worker,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        service_account_state.service_account_email.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id().to_string(),
                dependency_id: service_account_id,
            })
        })
    }

    /// Creates storage trigger infrastructure: Pub/Sub topic, GCS notification, and push subscription.
    pub(super) async fn create_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Get bucket name from the storage controller dependency
        let storage_controller =
            ctx.require_dependency::<crate::storage::GcpStorageController>(storage_ref)?;
        let bucket_name = storage_controller.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: storage_ref.id.clone(),
            })
        })?;

        // 1. Create a dedicated Pub/Sub topic for this storage notification
        let topic_short_name = format!(
            "{}-{}-{}-notif",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );
        let topic_full_name = format!(
            "projects/{}/topics/{}",
            gcp_config.project_id, topic_short_name
        );

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            topic=%topic_full_name,
            "Creating Pub/Sub topic for storage notifications"
        );

        match pubsub_client
            .create_topic(topic_short_name.clone(), Topic::default())
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    topic=%topic_short_name,
                    "Storage notification topic already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create storage notification topic '{}'",
                        topic_short_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.storage_notification_topics.contains(&topic_short_name) {
            self.storage_notification_topics
                .push(topic_short_name.clone());
        }

        // 2. Ask Cloud Storage for its managed service account before granting it
        //    publish permissions. Deriving the email from the project number does
        //    not ensure that the service account has been provisioned yet.
        let gcs_project_service_account = gcs_client.get_project_service_account().await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get the Cloud Storage service account for project '{}'",
                    gcp_config.project_id
                ),
                resource_id: Some(worker_config.id.clone()),
            },
        )?;
        let gcs_service_agent = format!(
            "serviceAccount:{}",
            gcs_project_service_account.email_address
        );

        let iam_policy = alien_gcp_clients::iam::IamPolicy::builder()
            .version(1)
            .bindings(vec![Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec![gcs_service_agent],
                condition: None,
            }])
            .build();

        pubsub_client
            .set_topic_iam_policy(topic_short_name.clone(), iam_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set IAM policy on storage notification topic '{}'",
                    topic_short_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // 3. Create GCS notification on the bucket pointing to the topic
        let gcs_event_types: Vec<String> = events
            .iter()
            .map(|event| {
                match event.as_str() {
                    "created" => "OBJECT_FINALIZE".to_string(),
                    "deleted" => "OBJECT_DELETE".to_string(),
                    "archived" => "OBJECT_ARCHIVE".to_string(),
                    "metadataUpdated" => "OBJECT_METADATA_UPDATE".to_string(),
                    other => other.to_string(), // Pass through unknown events as-is
                }
            })
            .collect();

        let notification = GcsNotification {
            id: None,
            topic: Some(topic_full_name.clone()),
            event_types: gcs_event_types,
            payload_format: Some("JSON_API_V1".to_string()),
            object_name_prefix: None,
            custom_attributes: std::collections::HashMap::new(),
        };

        let existing_notification = gcs_client
            .list_notifications(bucket_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list GCS notifications on bucket '{}' for worker '{}'",
                    bucket_name, worker_config.id
                ),
                resource_id: Some(worker_config.id.clone()),
            })?
            .items
            .into_iter()
            .find(|existing| gcs_notification_matches_existing(existing, &notification));

        let created_notification = if let Some(existing_notification) = existing_notification {
            info!(
                worker=%worker_config.id,
                storage=%storage_ref.id,
                bucket=%bucket_name,
                notification_id=?existing_notification.id,
                "GCS notification already exists; treating as created"
            );
            existing_notification
        } else {
            gcs_client
                .insert_notification(bucket_name.clone(), notification)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create GCS notification on bucket '{}' for worker '{}'",
                        bucket_name, worker_config.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                })?
        };

        if let Some(notification_id) = &created_notification.id {
            if !self.gcs_notification_ids.iter().any(|tracker| {
                tracker.bucket_name == *bucket_name && tracker.notification_id == *notification_id
            }) {
                self.gcs_notification_ids.push(GcsNotificationTracker {
                    bucket_name: bucket_name.clone(),
                    notification_id: notification_id.clone(),
                });
            }
        }

        // 4. Create a push subscription to the Cloud Run URL
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for storage trigger push subscription"
                    .to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_email = self.get_service_account_email(ctx, worker_config)?;

        let oidc_token = OidcToken {
            service_account_email,
            audience: Some(push_endpoint.clone()),
        };

        let subscription_name = format!(
            "{}-{}-{}-notif-sub",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint),
            attributes: Some(std::collections::HashMap::new()),
            oidc_token: Some(oidc_token),
            pubsub_wrapper: None,
            no_wrapper: None,
        };

        let subscription = Subscription {
            name: Some(subscription_name.clone()),
            topic: Some(topic_full_name.clone()),
            push_config: Some(push_config),
            ack_deadline_seconds: Some(worker_config.timeout_seconds as i32),
            retain_acked_messages: Some(false),
            message_retention_duration: None,
            labels: Some(std::collections::HashMap::from([
                ("worker".to_string(), worker_config.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
                ("storage".to_string(), storage_ref.id.clone()),
            ])),
            enable_message_ordering: Some(false),
            expiration_policy: None,
            filter: None,
            dead_letter_policy: None,
            retry_policy: None,
            detached: Some(false),
            state: None,
            analytics_hub_subscription_info: None,
            bigquery_config: None,
            cloud_storage_config: None,
        };

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            subscription=%subscription_name,
            "Creating Pub/Sub push subscription for storage trigger"
        );

        match pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    subscription=%subscription_name,
                    "Storage trigger push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for storage trigger '{}'",
                        subscription_name, storage_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name);
        }

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            "Successfully created storage trigger infrastructure"
        );

        Ok(())
    }

    /// Deletes all GCS notifications (best-effort)
    pub(super) async fn delete_all_storage_notifications(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.gcs_notification_ids.is_empty() {
            return Ok(());
        }

        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for tracker in &self.gcs_notification_ids.clone() {
            match gcs_client
                .delete_notification(tracker.bucket_name.clone(), tracker.notification_id.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        "GCS notification deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        error=%e,
                        "Failed to delete GCS notification (best-effort, continuing)"
                    );
                }
            }
        }

        self.gcs_notification_ids.clear();
        Ok(())
    }

    /// Deletes all storage notification Pub/Sub topics (best-effort)
    pub(super) async fn delete_all_storage_notification_topics(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.storage_notification_topics.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for topic_name in &self.storage_notification_topics.clone() {
            match pubsub_client.delete_topic(topic_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        "Storage notification topic deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        error=%e,
                        "Failed to delete storage notification topic (best-effort, continuing)"
                    );
                }
            }
        }

        self.storage_notification_topics.clear();
        Ok(())
    }

    /// Deletes all Cloud Scheduler jobs (best-effort)
    pub(super) async fn delete_all_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.scheduler_job_names.is_empty() {
            return Ok(());
        }

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        job=%job_name,
                        error=%e,
                        "Failed to delete Cloud Scheduler job (best-effort, continuing)"
                    );
                }
            }
        }

        self.scheduler_job_names.clear();
        Ok(())
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
