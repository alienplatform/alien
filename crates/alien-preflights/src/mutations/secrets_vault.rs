//! Adds the deployment secrets vault and grants runtime access only to Worker
//! wrappers that need vault-backed app or runtime secrets.

use crate::error::Result;
use crate::mutations::runs_on_platform_or_base;
use crate::StackMutation;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{
    ComputeCluster, ComputeKind, Container, Daemon, DeploymentConfig, ExposeProtocol, Platform,
    Postgres, RemoteStackManagement, ResourceEntry, ResourceLifecycle, ResourceRef, SecretDelivery,
    Stack, StackState, Vault, Worker,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Adds secrets vault for environment variable storage.
///
/// Creates the "secrets" vault for Worker secrets, Azure database passwords, or
/// Azure HTTP certificates (if missing). Worker wrappers receive the vault
/// link/read permission only when needed for app or runtime-owned secrets.
/// Runtime-less Containers and Daemons receive secrets from their hosting
/// layer and must not get vault data-plane access.
///
/// Steps:
/// 1. Add "secrets" vault resource (if not present)
/// 2. Link the vault to Worker runtimes that consume vault-backed secrets
/// 3. Add vault/data-read to those Worker profiles
/// 4. Add scoped management writes, and reads only when secret delivery requires them
pub struct SecretsVaultMutation;

/// Resource id this mutation reserves for the deployment secrets vault.
///
/// Owned by `alien_core::gateability` so the gating rules the generators
/// enforce agree with this mutation on one constant; re-exported here for the
/// preflight callers that always read it from this module.
pub use alien_core::SECRETS_VAULT_ID;

#[async_trait]
impl StackMutation for SecretsVaultMutation {
    fn description(&self) -> &'static str {
        "Add secrets vault for environment variable storage"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        if stack_state.platform == Platform::Machines {
            return stack.resources.contains_key(SECRETS_VAULT_ID)
                || config.external_bindings.has(SECRETS_VAULT_ID);
        }

        let explicitly_configured = stack.resources.contains_key(SECRETS_VAULT_ID)
            || config.external_bindings.has(SECRETS_VAULT_ID);
        let worker_needs_vault = stack.resources.values().any(|entry| {
            entry.config.resource_type() == Worker::RESOURCE_TYPE
                && (!SecretDelivery::resolve(stack_state.platform, ComputeKind::Worker)
                    .is_native_projection()
                    || config.monitoring.is_some())
        });

        // Vault-native deployer secrets live in this vault: the deployer
        // writes them there, and workloads read them from it.
        let deployer_secrets_need_vault = stack.inputs.iter().any(|input| {
            alien_core::is_deployer_secret_input(input)
                && input.platforms.as_ref().is_none_or(|platforms| {
                    platforms.is_empty() || platforms.contains(&stack_state.platform)
                })
        });

        explicitly_configured
            || worker_needs_vault
            || deployer_secrets_need_vault
            || azure_setup_needs_vault(stack, stack_state, config)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding deployment secrets vault and scoped runtime access");

        let secrets_vault_id = SECRETS_VAULT_ID;

        let infrastructure_only = azure_setup_needs_vault(&stack, stack_state, config)
            && !config.external_bindings.has(SECRETS_VAULT_ID)
            && !stack.resources.values().any(|entry| {
                entry.config.resource_type() == Worker::RESOURCE_TYPE
                    && (!SecretDelivery::resolve(stack_state.platform, ComputeKind::Worker)
                        .is_native_projection()
                        || config.monitoring.is_some())
            });

        // Step 1: Add vault resource if it doesn't already exist
        if !stack.resources.contains_key(secrets_vault_id) {
            let vault = Vault::new(secrets_vault_id.to_string()).build();
            let vault_entry = ResourceEntry {
                enabled_when: None,
                config: alien_core::Resource::new(vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                // This locator is synchronized for the manager's internal secret-delivery path.
                // It does not expose the vault through the Remote Bindings API.
                remote_access: false,
            };
            stack
                .resources
                .insert(secrets_vault_id.to_string(), vault_entry);
            debug!("Added secrets vault resource");
        } else {
            debug!("Secrets vault already exists");
        }

        // Native-projected Workers need the vault only when their wrapper has
        // runtime-owned monitoring credentials. Other Worker hosts consume the
        // vault-backed app-secret pointer regardless of monitoring.
        let worker_vault_access_required =
            !SecretDelivery::resolve(stack_state.platform, ComputeKind::Worker)
                .is_native_projection()
                || config.monitoring.is_some();
        if worker_vault_access_required {
            link_vault_to_worker_runtimes(&mut stack, secrets_vault_id)?;
            add_vault_read_permissions_to_worker_profiles(&mut stack, secrets_vault_id)?;
        }
        add_vault_dependency_to_compute_clusters(
            &mut stack,
            secrets_vault_id,
            stack_state.platform,
        );

        // Step 4: Add vault data permissions to management profile for the secrets vault.
        // This allows the control plane to sync secret environment variables without
        // granting access to user-declared vaults.
        add_vault_permissions_to_management(&mut stack, secrets_vault_id, !infrastructure_only)?;

        Ok(stack)
    }
}

/// Managed Azure database passwords need a setup vault even when workloads run
/// in Kubernetes. Kubernetes HTTP endpoints use their own ingress certificates.
fn azure_setup_needs_vault(stack: &Stack, state: &StackState, config: &DeploymentConfig) -> bool {
    runs_on_platform_or_base(state, config, Platform::Azure)
        && stack.resources.values().any(|entry| {
            if state.platform == Platform::Kubernetes {
                entry.lifecycle == ResourceLifecycle::Frozen
                    && entry.config.downcast_ref::<Postgres>().is_some()
            } else {
                azure_resource_needs_vault(entry)
            }
        })
}

/// Azure databases and HTTP endpoints store passwords or certificates in the shared vault.
/// TCP passthrough and private workloads do not need certificate storage.
pub(super) fn azure_resource_needs_vault(entry: &ResourceEntry) -> bool {
    if entry.config.downcast_ref::<Postgres>().is_some() {
        return true;
    }
    let endpoints = if let Some(container) = entry.config.downcast_ref::<Container>() {
        &container.public_endpoints
    } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
        &daemon.public_endpoints
    } else {
        return false;
    };
    endpoints
        .iter()
        .any(|endpoint| endpoint.protocol == ExposeProtocol::Http)
}

/// Compute-cluster machine identities receive narrowly scoped read access to the
/// deployment secrets vault. Make the Frozen vault an explicit dependency so
/// provider-specific ComputeCluster controllers never assign that access
/// against a vault whose outputs or cloud resource do not exist yet.
///
/// AWS Parameter Store vaults are namespaces rather than provisioned resources.
/// Their setup emitters only produce IAM policy attachments, so treating those
/// attachments as the vault dependency target creates a cycle between the
/// compute role, vault policies, and execution role. The compute role already
/// carries its scoped Parameter Store access and has no vault resource to await.
fn add_vault_dependency_to_compute_clusters(stack: &mut Stack, vault_id: &str, platform: Platform) {
    if matches!(platform, Platform::Aws | Platform::Kubernetes) {
        return;
    }

    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);

    for (resource_id, entry) in &mut stack.resources {
        if entry.config.resource_type() != ComputeCluster::RESOURCE_TYPE {
            continue;
        }
        if entry
            .dependencies
            .iter()
            .any(|dependency| dependency == &vault_ref)
        {
            continue;
        }

        entry.dependencies.push(vault_ref.clone());
        debug!(
            compute_cluster = %resource_id,
            vault = %vault_id,
            "Made the compute cluster depend on its deployment secrets vault"
        );
    }
}

/// Link the secrets vault only to Worker wrappers selected by the caller.
fn link_vault_to_worker_runtimes(stack: &mut Stack, vault_id: &str) -> Result<()> {
    let vault_ref = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);
    let mut linked_count = 0;

    for (resource_id, entry) in &mut stack.resources {
        let resource_type = entry.config.resource_type();

        if resource_type != Worker::RESOURCE_TYPE {
            continue;
        }

        let Some(worker) = entry.config.downcast_mut::<Worker>() else {
            continue;
        };

        // Add vault link if not already present
        if !worker.links.iter().any(|link| link.id() == vault_id) {
            worker.links.push(vault_ref.clone());
            linked_count += 1;
            debug!("Linked secrets vault to compute resource '{}'", resource_id);
        }
    }

    if linked_count > 0 {
        info!("Linked secrets vault to {linked_count} Worker runtimes");
    }

    Ok(())
}

/// Add vault/data-read only to Worker profiles selected by the caller.
fn add_vault_read_permissions_to_worker_profiles(
    stack: &mut Stack,
    vault_name: &str,
) -> Result<()> {
    // Get Worker permission profile names.
    let profile_names: Vec<String> = stack
        .resources
        .iter()
        .filter_map(|(_, entry)| {
            let resource_type = entry.config.resource_type();

            if resource_type != Worker::RESOURCE_TYPE {
                return None;
            }

            entry
                .config
                .downcast_ref::<Worker>()
                .map(|worker| worker.permissions.clone())
        })
        .collect();

    // Deduplicate profile names (multiple resources might use the same profile)
    let unique_profiles: std::collections::HashSet<String> = profile_names.into_iter().collect();

    // Add vault/data-read to each profile
    let vault_permission = PermissionSetReference::from_name("vault/data-read");

    for profile_name in unique_profiles {
        if let Some(profile) = stack.permissions.profiles.get_mut(&profile_name) {
            let vault_permissions = profile.0.entry(vault_name.to_string()).or_default();
            if !vault_permissions
                .iter()
                .any(|p| p.id() == "vault/data-read")
            {
                vault_permissions.push(vault_permission.clone());
                debug!(
                    "Added vault/data-read to profile '{}' for vault '{}'",
                    profile_name, vault_name
                );
            }
        } else {
            // Profile doesn't exist - PermissionProfilesExistCheck should have caught this
            // Don't create the profile (fail fast on configuration errors)
            debug!(
                "Skipping vault permission for nonexistent profile '{}' (validation issue)",
                profile_name
            );
        }
    }

    Ok(())
}

/// Author explicit vault data permissions into the management profile for this vault.
/// The generator treats these like any other management grant; the preflight is
/// the permission author.
fn add_vault_permissions_to_management(
    stack: &mut Stack,
    vault_name: &str,
    needs_read: bool,
) -> Result<()> {
    use alien_core::permissions::ManagementPermissions;

    if !stack
        .resources
        .values()
        .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    {
        debug!(
            vault_name = %vault_name,
            "Skipping concrete vault management permissions because remote stack management is not present"
        );
        return Ok(());
    }

    // Get current management permissions
    let current_management = stack.permissions.management.clone();

    match current_management {
        ManagementPermissions::Auto | ManagementPermissions::Extend(_) => {
            // For Auto or Extend, author data access on this concrete vault resource.
            // This will be merged with auto-generated permissions by ManagementPermissionProfileMutation
            let vault_read_permission = PermissionSetReference::from_name("vault/data-read");
            let vault_write_permission = PermissionSetReference::from_name("vault/data-write");

            let mut management_profile = match current_management {
                ManagementPermissions::Extend(profile) => profile,
                _ => PermissionProfile::new(),
            };

            let vault_permissions = management_profile
                .0
                .entry(vault_name.to_string())
                .or_default();
            if needs_read
                && !vault_permissions
                    .iter()
                    .any(|p| p.id() == "vault/data-read")
            {
                vault_permissions.push(vault_read_permission);
                debug!(
                    vault_name = %vault_name,
                    "Added vault/data-read to management profile for concrete vault resource"
                );
            }
            if !vault_permissions
                .iter()
                .any(|p| p.id() == "vault/data-write")
            {
                vault_permissions.push(vault_write_permission);
                debug!(
                    vault_name = %vault_name,
                    "Added vault/data-write to management profile for concrete vault resource"
                );
            }
            stack.permissions.management = ManagementPermissions::Extend(management_profile);
        }
        ManagementPermissions::Override(_) => {
            // Don't modify override - user has full control.
            debug!("Skipping concrete vault management permissions - management permissions are overridden");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceEntry, ResourceLifecycle, ResourceSpec, StackInputDefinition, StackInputKind,
        StackInputProvider, StackSettings, StackState, Worker, WorkerCode,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn remote_stack_management_entry() -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn compute_cluster_stack() -> Stack {
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources: IndexMap::from([(
                "compute".to_string(),
                ResourceEntry {
                    config: alien_core::Resource::new(
                        ComputeCluster::new("compute".to_string()).build(),
                    ),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    remote_access: false,
                    enabled_when: None,
                },
            )]),
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        }
    }

    #[tokio::test]
    async fn postgres_preflights_prepare_cloud_dependencies_without_secret_read_access() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let stack = Stack::new("database-only".to_string())
                .add(
                    Postgres::new("database".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    RemoteStackManagement::new("remote-stack-management".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(empty_env_snapshot())
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build();
            let state = StackState::new(platform);
            let runner = crate::runner::PreflightRunner::new();
            let prepared = runner
                .apply_mutations(stack, &state, &config)
                .await
                .expect("prepare Postgres");
            let repeated = runner
                .apply_mutations(prepared.clone(), &state, &config)
                .await
                .expect("repeat preflights");
            for stack in [prepared, repeated] {
                let database = stack.resources.get("database").expect("database");
                assert!(database.dependencies.contains(&ResourceRef::new(
                    alien_core::Network::RESOURCE_TYPE,
                    "default-network"
                )));
                if platform == Platform::Azure {
                    assert!(stack.resources.contains_key("default-resource-group"));
                    assert!(stack.resources.contains_key("enable-keyvault"));
                    assert_eq!(
                        stack.resources.get("secrets").expect("vault").lifecycle,
                        ResourceLifecycle::Frozen
                    );
                    assert!(database
                        .dependencies
                        .contains(&ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")));
                    assert!(database.dependencies.contains(&ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-postgresql"
                    )));
                    let ManagementPermissions::Extend(profile) = &stack.permissions.management
                    else {
                        panic!("expected explicit management permissions")
                    };
                    let permissions = &profile.0;
                    let vault_permissions = permissions.get("secrets").expect("vault permissions");
                    assert!(vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"));
                    assert!(!vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-read"));
                } else {
                    assert!(!stack.resources.contains_key("secrets"));
                    if platform == Platform::Gcp {
                        for id in [
                            "enable-cloud-sql",
                            "enable-compute-engine",
                            "enable-secret-manager",
                        ] {
                            assert!(database.dependencies.contains(&ResourceRef::new(
                                alien_core::ServiceActivation::RESOURCE_TYPE,
                                id
                            )));
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn azure_http_endpoints_need_vault_but_tcp_and_private_workloads_do_not() {
        for protocol in [None, Some(ExposeProtocol::Tcp), Some(ExposeProtocol::Http)] {
            let endpoints = protocol
                .map(|protocol| {
                    vec![alien_core::PublicEndpoint {
                        name: "web".to_string(),
                        port: 8080,
                        protocol,
                        host_label: None,
                        wildcard_subdomains: false,
                    }]
                })
                .unwrap_or_default();
            let mut container = Container::new("api".to_string())
                .code(ContainerCode::Image {
                    image: "example.com/api:latest".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "0.5".to_string(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "512Mi".to_string(),
                })
                .permissions("execution".to_string())
                .build();
            container.public_endpoints = endpoints.clone();
            let mut daemon = Daemon::new("daemon".to_string())
                .code(alien_core::DaemonCode::Image {
                    image: "example.com/api:latest".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "0.5".to_string(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "512Mi".to_string(),
                })
                .permissions("execution".to_string())
                .build();
            daemon.public_endpoints = endpoints;
            let stack = Stack::new("endpoints".to_string())
                .add(container, ResourceLifecycle::Live)
                .add(daemon, ResourceLifecycle::Live)
                .build();
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(empty_env_snapshot())
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build();
            for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
                let state = StackState::new(platform);
                let needed = platform == Platform::Azure && protocol == Some(ExposeProtocol::Http);
                assert_eq!(
                    SecretsVaultMutation.should_run(&stack, &state, &config),
                    needed
                );
                if needed {
                    let prepared = SecretsVaultMutation
                        .mutate(stack.clone(), &state, &config)
                        .await
                        .expect("inject endpoint Vault");
                    assert_eq!(
                        prepared.resources["secrets"].lifecycle,
                        ResourceLifecycle::Frozen
                    );
                    let wired = super::super::infrastructure_dependencies::InfrastructureDependenciesMutation.mutate(prepared, &state, &config).await.expect("wire dependencies");
                    for id in ["api", "daemon"] {
                        assert!(wired.resources[id]
                            .dependencies
                            .contains(&ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")));
                    }
                    assert!(
                        wired.permissions.profiles.is_empty(),
                        "certificate storage must not grant workloads Vault data access"
                    );
                }
            }
        }
    }

    #[test]
    fn aws_compute_cluster_skips_namespace_only_vault_dependency() {
        let mut stack = compute_cluster_stack();

        add_vault_dependency_to_compute_clusters(&mut stack, "secrets", Platform::Aws);

        assert!(stack
            .resources
            .get("compute")
            .expect("compute cluster")
            .dependencies
            .is_empty());
    }

    #[test]
    fn compute_cluster_depends_on_provisioned_vault_exactly_once() {
        let mut stack = compute_cluster_stack();

        add_vault_dependency_to_compute_clusters(&mut stack, "secrets", Platform::Azure);
        add_vault_dependency_to_compute_clusters(&mut stack, "secrets", Platform::Azure);

        let dependencies = &stack
            .resources
            .get("compute")
            .expect("compute cluster")
            .dependencies;
        assert_eq!(
            dependencies,
            &[ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")]
        );
    }

    #[tokio::test]
    async fn test_adds_secrets_vault_and_links_to_function() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation
            .mutate(stack.clone(), &stack_state, &config)
            .await
            .unwrap();

        // Check that secrets vault was added
        assert!(result_stack.resources.contains_key("secrets"));
        let vault_entry = result_stack.resources.get("secrets").unwrap();
        assert_eq!(vault_entry.lifecycle, ResourceLifecycle::Frozen);

        // Check that vault was linked to the worker
        let function_entry = result_stack.resources.get("test-worker").unwrap();
        let worker = function_entry.config.downcast_ref::<Worker>().unwrap();
        assert!(
            worker.links.iter().any(|link| link.id() == "secrets"),
            "Worker should be linked to secrets vault"
        );

        // Check that vault/data-read was added to worker profile
        let function_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        let vault_permissions = function_profile.0.get("secrets").unwrap();
        assert!(vault_permissions
            .iter()
            .any(|p| p.id() == "vault/data-read"));

        // Kubernetes projects app secrets natively. Without runtime monitoring,
        // its Worker wrapper has no reason to access the vault.
        let kubernetes_stack = mutation
            .mutate(
                stack.clone(),
                &StackState::new(Platform::Kubernetes),
                &config,
            )
            .await
            .unwrap();
        let worker = kubernetes_stack
            .resources
            .get("test-worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().all(|link| link.id() != "secrets"));
        assert!(kubernetes_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap()
            .0
            .get("secrets")
            .is_none());

        // Runtime monitoring adds a vault-backed ALIEN_RUNTIME_SECRETS pointer
        // for the Worker wrapper, so the link and read permission are required.
        let mut monitoring_config = config;
        monitoring_config.monitoring = Some(alien_core::OtlpConfig {
            logs_endpoint: "https://example.com/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer test".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: Default::default(),
        });
        let kubernetes_stack = mutation
            .mutate(
                stack,
                &StackState::new(Platform::Kubernetes),
                &monitoring_config,
            )
            .await
            .unwrap();
        let worker = kubernetes_stack
            .resources
            .get("test-worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().any(|link| link.id() == "secrets"));
        assert!(kubernetes_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap()
            .0
            .get("secrets")
            .unwrap()
            .iter()
            .any(|permission| permission.id() == "vault/data-read"));
    }

    #[tokio::test]
    async fn test_skips_machines_without_explicit_secrets_vault() {
        let container = Container::new("test-container".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-container".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Machines);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;

        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn deployer_secret_inputs_need_the_secrets_vault_on_their_platforms() {
        let mut stack = compute_cluster_stack();
        let stack_state = StackState::new(Platform::Gcp);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        let mut input = StackInputDefinition {
            id: "databasePassword".to_string(),
            kind: StackInputKind::Secret,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: "Database password".to_string(),
            description: String::new(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            env: Vec::new(),
        };
        stack.inputs = vec![input.clone()];
        assert!(SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        input.platforms = Some(vec![Platform::Aws]);
        stack.inputs = vec![input.clone()];
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        input.platforms = None;
        input.provided_by = vec![StackInputProvider::Developer];
        stack.inputs = vec![input];
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn runtime_less_container_gets_no_vault_link_or_read_permission() {
        let container = Container::new("test-container".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-container".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // The hosting layer projects Container secrets before process start.
        let container_entry = result_stack.resources.get("test-container").unwrap();
        let container = container_entry.config.downcast_ref::<Container>().unwrap();
        assert!(
            container.links.iter().all(|link| link.id() != "secrets"),
            "runtime-less Container must not receive the secrets vault binding"
        );

        let container_profile = result_stack
            .permissions
            .profiles
            .get("test-profile")
            .unwrap();
        assert!(
            !container_profile.0.contains_key("secrets"),
            "runtime-less Container profile must not get vault data-plane access"
        );
    }

    #[tokio::test]
    async fn test_does_not_add_duplicate_vault() {
        // Create a stack that already has a secrets vault
        let vault = Vault::new("secrets".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert(
            "secrets".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;

        // Mutation should always run (returns true)
        assert!(mutation.should_run(&stack, &stack_state, &config));

        // But when it runs, it should not add a duplicate vault
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Should still have exactly one vault
        assert_eq!(result_stack.resources.len(), 1);
        assert!(result_stack.resources.contains_key("secrets"));
    }

    #[tokio::test]
    async fn test_supports_mixed_functions_and_containers() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("api-profile".to_string())
            .build();

        let container = Container::new("worker".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("worker-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("api-profile".to_string(), PermissionProfile::new());
        profiles.insert("worker-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Only the Worker wrapper consumes vault pointers.
        let worker = result_stack
            .resources
            .get("api")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();
        assert!(worker.links.iter().any(|link| link.id() == "secrets"));

        let container = result_stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Container>()
            .unwrap();
        assert!(container.links.iter().all(|link| link.id() != "secrets"));

        let api_profile = result_stack
            .permissions
            .profiles
            .get("api-profile")
            .unwrap();
        assert!(api_profile
            .0
            .get("secrets")
            .expect("Worker vault grant")
            .iter()
            .any(|permission| permission.id() == "vault/data-read"));
        let container_profile = result_stack
            .permissions
            .profiles
            .get("worker-profile")
            .unwrap();
        assert!(!container_profile.0.contains_key("secrets"));
    }

    #[tokio::test]
    async fn test_adds_vault_data_permissions_to_management() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that management profile has vault data permissions scoped to the secrets vault.
        match result_stack.permissions.management {
            ManagementPermissions::Extend(profile) => {
                assert!(
                    profile.0.get("*").map_or(true, |permissions| !permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write")),
                    "Management profile should not have global vault/data-write"
                );
                let vault_permissions = profile.0.get("secrets").unwrap();
                assert!(
                    vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-read"),
                    "Management profile should have vault/data-read for secrets vault"
                );
                assert!(
                    vault_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Management profile should have vault/data-write for secrets vault"
                );
                assert!(
                    profile.0.get("alien-vault").is_none(),
                    "Management profile should not get vault/data-write for user vaults by default"
                );
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_preserves_explicit_global_vault_data_write_and_adds_secrets_scope() {
        let user_vault = Vault::new("alien-vault".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert(
            "alien-vault".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(user_vault),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert("management".to_string(), remote_stack_management_entry());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(
                    PermissionProfile::new().global(["vault/data-write", "storage/heartbeat"]),
                ),
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.permissions.management {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "storage/heartbeat"),
                    "Unrelated global permissions should be preserved"
                );
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Explicit global vault/data-write should be preserved"
                );
                assert!(
                    profile
                        .0
                        .get("secrets")
                        .unwrap()
                        .iter()
                        .any(|p| p.id() == "vault/data-read"),
                    "vault/data-read should be added to the secrets vault resource"
                );
                assert!(
                    profile
                        .0
                        .get("secrets")
                        .unwrap()
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "vault/data-write should be added to the secrets vault resource"
                );
                assert!(
                    profile.0.get("alien-vault").is_none(),
                    "user vaults should not receive resource-specific management vault/data-write by default"
                );
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_respects_override_management_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        // Create an override management profile without vault/data-write
        let override_profile = PermissionProfile::new().global(["storage/management"]);

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Override(override_profile.clone()),
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that override profile is unchanged (no vault/data-write added)
        match result_stack.permissions.management {
            ManagementPermissions::Override(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert_eq!(global_permissions.len(), 1);
                assert!(
                    global_permissions
                        .iter()
                        .any(|p| p.id() == "storage/management"),
                    "Override profile should keep original permissions"
                );
                assert!(
                    !global_permissions
                        .iter()
                        .any(|p| p.id() == "vault/data-write"),
                    "Override profile should not have vault/data-write added"
                );
            }
            _ => panic!("Expected Override management permissions"),
        }
    }

    #[tokio::test]
    async fn test_skips_management_permissions_without_remote_stack_management() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), PermissionProfile::new());

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Local);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = SecretsVaultMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        assert!(result_stack.resources.contains_key("secrets"));
        assert!(
            matches!(
                result_stack.permissions.management,
                ManagementPermissions::Auto
            ),
            "Local stacks without remote stack management should not get resource-scoped management permissions"
        );
    }
    #[tokio::test]
    async fn aks_database_setup_vault_does_not_grant_kubernetes_secret_reads() {
        let state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .base_platform(Platform::Azure)
            .build();
        let stack = Stack::new("database".to_string())
            .add(
                Postgres::new("database".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Worker::new("api".to_string())
                    .code(alien_core::WorkerCode::Image {
                        image: "example.test/api:1".to_string(),
                    })
                    .permissions("default".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                ComputeCluster::new("pool".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("remote-stack-management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        assert!(SecretsVaultMutation.should_run(&stack, &state, &config));
        let prepared = crate::runner::PreflightRunner::new()
            .apply_mutations(stack, &state, &config)
            .await
            .unwrap();
        assert_eq!(
            prepared.resources["secrets"].lifecycle,
            ResourceLifecycle::Frozen
        );
        assert!(prepared.resources["database"]
            .dependencies
            .contains(&ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")));
        assert!(!prepared.resources["pool"]
            .dependencies
            .contains(&ResourceRef::new(Vault::RESOURCE_TYPE, "secrets")));
        assert!(prepared.resources["api"]
            .config
            .downcast_ref::<Worker>()
            .unwrap()
            .links
            .iter()
            .all(|link| link.id() != "secrets"));
        let ManagementPermissions::Extend(profile) = &prepared.permissions.management else {
            panic!("management profile")
        };
        let permissions = profile.0.get("secrets").unwrap();
        assert!(permissions
            .iter()
            .any(|permission| permission.id() == "vault/data-write"));
        assert!(!permissions
            .iter()
            .any(|permission| permission.id() == "vault/data-read"));
    }

    #[test]
    fn pure_kubernetes_database_does_not_inject_cloud_vault() {
        let stack = Stack::new("database".to_string())
            .add(
                Postgres::new("database".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(!SecretsVaultMutation.should_run(
            &stack,
            &StackState::new(Platform::Kubernetes),
            &config
        ));
    }
    #[tokio::test]
    async fn azure_postgres_data_access_never_grants_shared_vault_reads() {
        for platform in [Platform::Azure, Platform::Kubernetes] {
            let mut stack = Stack::new("database".to_string())
                .add(
                    Postgres::new("database".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    Postgres::new("other".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            for (name, resource, permission) in [
                ("database-client", "database", "postgres/data-access"),
                ("all-databases", "*", "postgres/data-access"),
                ("observer", "database", "postgres/heartbeat"),
                ("unrelated", "missing", "postgres/data-access"),
                ("database-admin", "database", "postgres/management"),
            ] {
                let mut profile = PermissionProfile::new();
                profile.0.insert(
                    resource.to_string(),
                    vec![PermissionSetReference::from_name(permission)],
                );
                stack.permissions.profiles.insert(name.to_string(), profile);
            }
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(empty_env_snapshot())
                .external_bindings(ExternalBindings::default())
                .allow_frozen_changes(false)
                .base_platform(Platform::Azure)
                .build();
            let state = StackState::new(platform);
            let prepared = SecretsVaultMutation
                .mutate(stack, &state, &config)
                .await
                .unwrap();
            let repeated = SecretsVaultMutation
                .mutate(prepared, &state, &config)
                .await
                .unwrap();
            for name in [
                "database-client",
                "all-databases",
                "observer",
                "unrelated",
                "database-admin",
            ] {
                let count = repeated.permissions.profiles[name]
                    .0
                    .get("secrets")
                    .into_iter()
                    .flatten()
                    .filter(|permission| permission.id() == "vault/data-read")
                    .count();
                assert_eq!(count, 0, "profile {name}, platform {platform:?}");
            }
            let ManagementPermissions::Auto = repeated.permissions.management else {
                panic!("database-client access must not change management grants without remote management");
            };
        }
    }
}
