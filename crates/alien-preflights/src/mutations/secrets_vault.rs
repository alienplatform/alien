//! Adds the deployment secrets vault and grants runtime read access only to
//! workloads that read it: Worker wrappers that need vault-backed app or
//! runtime secrets, and cloud-hosted Containers and Daemons whose hosting
//! layer reads their deployer secrets with the workload's own identity.

use crate::error::{ErrorData, Result};
use crate::mutations::runs_on_platform_or_base;
use crate::StackMutation;
use alien_core::permissions::{PermissionProfile, PermissionSet, PermissionSetReference};
use alien_core::vault_naming;
use alien_core::{
    ComputeCluster, ComputeKind, Container, Daemon, DeploymentConfig, ExposeProtocol, Platform,
    Postgres, RemoteStackManagement, ResourceEntry, ResourceLifecycle, ResourceRef, SecretDelivery,
    Stack, StackState, Vault, Worker,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tracing::{debug, info};

/// Adds secrets vault for environment variable storage.
///
/// Creates the "secrets" vault for Worker secrets, Azure database passwords, or
/// Azure HTTP certificates (if missing). Worker wrappers receive the vault
/// link/read permission only when needed for app or runtime-owned secrets.
/// Runtime-less Containers and Daemons receive secrets from their hosting
/// layer and get no vault data-plane access, except on AWS, GCP and Azure
/// when a deployer secret maps into them: their hosting layer then reads
/// that slot with the workload's own identity, so the workload's profile
/// needs to read it.
///
/// Steps:
/// 1. Add "secrets" vault resource (if not present)
/// 2. Link the vault to Worker runtimes that consume vault-backed secrets
/// 3. Add vault/data-read to those Worker profiles, and to the profiles of
///    cloud-hosted Containers and Daemons that receive a deployer secret
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
        let deployer_secrets_need_vault = alien_core::deployer_secret_slots(
            &stack.inputs,
            &config.input_values,
            &config.stored_secret_input_ids,
            stack_state.platform,
        )
        .iter()
        .any(|slot| {
            !config
                .input_values
                .get(&slot.input.id)
                .is_some_and(|value| !value.is_null())
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

        // Validate workload identities before deriving a vault or changing grants.
        // Kubernetes on a cloud base uses its own projection path, but an
        // identityless daemon still cannot consume deployer-vault slots.
        let deployer_keys = if [Platform::Aws, Platform::Gcp, Platform::Azure]
            .into_iter()
            .any(|platform| runs_on_platform_or_base(stack_state, config, platform))
        {
            deployer_secret_keys_by_profile(&stack, config, stack_state.platform)?
        } else {
            BTreeMap::new()
        };

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
            let worker_profiles = compute_profiles(&stack, |entry| {
                entry
                    .config
                    .downcast_ref::<Worker>()
                    .map(|w| &w.permissions)
            });
            add_vault_read_permissions_to_profiles(&mut stack, secrets_vault_id, worker_profiles);
        }
        // On these platforms the hosting layer resolves a Container's or
        // Daemon's deployer secrets as it starts the workload, with the
        // workload's own cloud identity. That identity may read exactly those
        // slots.
        if matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            add_deployer_secret_read_permissions(&mut stack, secrets_vault_id, deployer_keys)?;
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

/// The distinct permission profiles of the resources `profile` selects.
fn compute_profiles(
    stack: &Stack,
    profile: impl Fn(&ResourceEntry) -> Option<&String>,
) -> BTreeSet<String> {
    stack
        .resources
        .values()
        .filter_map(profile)
        .cloned()
        .collect()
}

/// For each permission profile of a Container or Daemon, the vault keys of the
/// deployer secret slots on `platform` that map into it.
///
/// Every workload that uses a profile runs with that profile's cloud identity,
/// so a profile can only read the secrets all of its workloads receive. Two
/// Containers or Daemons that share a profile but receive different deployer
/// secrets are refused rather than letting one read the other's.
fn deployer_secret_keys_by_profile(
    stack: &Stack,
    config: &DeploymentConfig,
    platform: Platform,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    // The same slots delivery reads: a secret the developer may also provide
    // and has a stored developer value is not a slot.
    let slots = alien_core::deployer_secret_slots(
        &stack.inputs,
        &config.input_values,
        &config.stored_secret_input_ids,
        platform,
    );
    if slots.is_empty() {
        return Ok(BTreeMap::new());
    }

    // profile -> (first workload seen with it, the keys that workload receives)
    let mut by_profile: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for (resource_id, entry) in &stack.resources {
        let profile = if let Some(container) = entry.config.downcast_ref::<Container>() {
            Some(&container.permissions)
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            daemon.permissions.as_ref()
        } else {
            continue;
        };
        // Concrete pre-vault values retain their delivery until removed.
        // Presence IDs alone never stand in for such a value.
        let keys: BTreeSet<String> = slots
            .iter()
            .filter(|slot| {
                !config
                    .input_values
                    .get(&slot.input.id)
                    .is_some_and(|value| !value.is_null())
            })
            .filter(|slot| {
                slot.input
                    .env
                    .iter()
                    .any(|mapping| mapping.targets(resource_id))
            })
            .map(|slot| slot.vault_key.clone())
            .collect();
        let Some(profile) = profile else {
            if !keys.is_empty() {
                return Err(AlienError::new(ErrorData::ResourceValidationFailed {
                    resource_id: resource_id.clone(),
                    message: "A cloud daemon receiving deployer-vault secrets requires an explicit workload permission profile. Set permissions to a declared profile, or remove its deployer-secret mappings.".to_string(),
                }));
            }
            continue;
        };
        // Cloud-base Kubernetes keeps its existing secret projection and grant
        // behavior; only the missing workload identity is rejected here.
        if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
            continue;
        }
        match by_profile.get(profile) {
            None => {
                by_profile.insert(profile.clone(), (resource_id.clone(), keys));
            }
            Some((other, other_keys)) if *other_keys != keys => {
                return Err(AlienError::new(ErrorData::ResourceValidationFailed {
                    resource_id: resource_id.clone(),
                    message: format!(
                        "'{resource_id}' and '{other}' share permission profile '{profile}' \
                         but receive different deployer secrets. Workloads that share a \
                         profile share one cloud identity, so either could read the other's \
                         secrets: give them separate permission profiles, or map the same \
                         deployer secrets into both."
                    ),
                }));
            }
            Some(_) => {}
        }
    }

    Ok(by_profile
        .into_iter()
        .filter(|(_, (_, keys))| !keys.is_empty())
        .map(|(profile, (_, keys))| (profile, keys))
        .collect())
}

/// Id of the inline permission set that reads a profile's deployer secret slots.
const DEPLOYER_SECRETS_READ_ID: &str = "vault/deployer-secrets-read";

/// Lets each profile read exactly its deployer secret slots in `vault_name`.
fn add_deployer_secret_read_permissions(
    stack: &mut Stack,
    vault_name: &str,
    keys_by_profile: BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    for (profile_name, keys) in keys_by_profile {
        let Some(profile) = stack.permissions.profiles.get_mut(&profile_name) else {
            // PermissionProfilesExistCheck reports a missing profile.
            debug!(
                "Skipping deployer secret read for nonexistent profile '{}'",
                profile_name
            );
            continue;
        };
        let vault_permissions = profile.0.entry(vault_name.to_string()).or_default();
        vault_permissions.retain(|set| set.id() != DEPLOYER_SECRETS_READ_ID);
        vault_permissions.push(PermissionSetReference::from_inline(
            deployer_secrets_read_permission_set(&keys)?,
        ));
        debug!(
            "Profile '{}' may read {} deployer secret slot(s) in vault '{}'",
            profile_name,
            keys.len(),
            vault_name
        );
    }
    Ok(())
}

/// Read access to exactly these `secrets` vault keys, with `${resourceName}`
/// standing for the vault (its prefix on AWS and GCP, its name on Azure).
fn deployer_secrets_read_permission_set(keys: &BTreeSet<String>) -> Result<PermissionSet> {
    const VAULT: &str = "${resourceName}";

    let aws_parameters: Vec<String> = keys
        .iter()
        .map(|key| {
            format!(
                "arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/{}",
                vault_naming::parameter_store_parameter_name(VAULT, key)
            )
        })
        .collect();
    let gcp_condition = keys
        .iter()
        .map(|key| {
            let secret = format!(
                "projects/${{projectNumber}}/secrets/{}",
                vault_naming::secret_manager_secret_id(VAULT, key)
            );
            format!("resource.name == \"{secret}\" || resource.name.startsWith(\"{secret}/\")")
        })
        .collect::<Vec<_>>()
        .join(" || ");
    let azure: Vec<serde_json::Value> = keys
        .iter()
        .map(|key| {
            serde_json::json!({
                "grant": { "predefinedRoles": ["Key Vault Secrets User"] },
                "binding": { "resource": { "scope": format!(
                    "/subscriptions/${{subscriptionId}}/resourceGroups/${{resourceGroup}}/providers/Microsoft.KeyVault/vaults/{VAULT}/secrets/{}",
                    vault_naming::key_vault_secret_name(key)
                ) } }
            })
        })
        .collect();

    serde_json::from_value(serde_json::json!({
        "id": DEPLOYER_SECRETS_READ_ID,
        "description": "Reads the deployer secrets mapped into this workload",
        "platforms": {
            "aws": [{
                "grant": { "actions": ["ssm:GetParameter"] },
                "binding": { "resource": { "resources": aws_parameters } }
            }],
            "gcp": [{
                "grant": { "predefinedRoles": ["roles/secretmanager.secretAccessor"] },
                "binding": { "resource": {
                    "scope": "projects/${projectName}",
                    "condition": {
                        "title": "DeployerSecretsRead",
                        "expression": gcp_condition
                    }
                } }
            }],
            "azure": azure
        }
    }))
    .into_alien_error()
    .context(ErrorData::StackMutationFailed {
        mutation_name: "SecretsVaultMutation".to_string(),
        message: "Failed to build the deployer secrets read permission set".to_string(),
        resource_id: None,
    })
}

/// Add vault/data-read on `vault_name` to each of `profile_names`.
fn add_vault_read_permissions_to_profiles(
    stack: &mut Stack,
    vault_name: &str,
    profile_names: BTreeSet<String>,
) {
    let vault_permission = PermissionSetReference::from_name("vault/data-read");

    for profile_name in profile_names {
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
    use crate::compatibility::PermissionProfilesUnchangedCheck;
    use crate::StackCompatibilityCheck;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceEntry, ResourceLifecycle, ResourceSpec, StackInputDefinition,
        StackInputEnvironmentMapping, StackInputKind, StackInputProvider, StackSettings,
        StackState, Worker, WorkerCode,
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
            generate: None,
            env: Vec::new(),
        };
        stack.inputs = vec![input.clone()];
        assert!(SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        input.platforms = Some(vec![Platform::Aws]);
        stack.inputs = vec![input.clone()];
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        input.platforms = None;
        input.provided_by = vec![StackInputProvider::Developer];
        stack.inputs = vec![input.clone()];
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &config));

        // Only trusted input presence, not a coincident mapped variable, keeps
        // a dual-provider developer answer off the vault path.
        input.provided_by = vec![StackInputProvider::Developer, StackInputProvider::Deployer];
        input.env = vec![StackInputEnvironmentMapping {
            name: "DATABASE_PASSWORD".to_string(),
            target_resources: None,
            var_type: None,
        }];
        stack.inputs = vec![input];
        assert!(SecretsVaultMutation.should_run(&stack, &stack_state, &config));
        let mut delivered = config.clone();
        delivered.environment_variables.variables = vec![alien_core::EnvironmentVariable {
            name: "DATABASE_PASSWORD".to_string(),
            value: "developer-value".to_string(),
            var_type: alien_core::EnvironmentVariableType::Secret,
            target_resources: None,
        }];
        // A mapped variable alone is not authoritative stored-input presence.
        assert!(SecretsVaultMutation.should_run(&stack, &stack_state, &delivered));
        delivered.stored_secret_input_ids = vec![stack.inputs[0].id.clone()];
        assert!(!SecretsVaultMutation.should_run(&stack, &stack_state, &delivered));
    }

    #[test]
    fn stored_dual_secret_presence_avoids_an_unneeded_vault() {
        let mut stack = deployer_secret_stack(&[("api", "api-profile")], &["api"]);
        stack.inputs[0]
            .provided_by
            .push(StackInputProvider::Developer);
        let state = StackState::new(Platform::Gcp);
        let mut config = deployer_secret_config();
        assert!(config.input_values.is_empty());
        assert!(SecretsVaultMutation.should_run(&stack, &state, &config));
        config.stored_secret_input_ids = vec![stack.inputs[0].id.clone()];
        assert!(!SecretsVaultMutation.should_run(&stack, &state, &config));
        assert!(
            deployer_secret_keys_by_profile(&stack, &config, Platform::Gcp)
                .unwrap()
                .is_empty()
        );
        stack.inputs[0].provided_by = vec![StackInputProvider::Deployer];
        assert!(SecretsVaultMutation.should_run(&stack, &state, &config));
        assert!(
            !deployer_secret_keys_by_profile(&stack, &config, Platform::Gcp)
                .unwrap()
                .is_empty()
        );
        stack.inputs[0]
            .provided_by
            .push(StackInputProvider::Developer);
        config.stored_secret_input_ids.clear();
        assert!(SecretsVaultMutation.should_run(&stack, &state, &config));
    }

    fn secret_daemon_stack() -> Stack {
        let mut stack = deployer_secret_stack(&[], &["daemon"]);
        let daemon = Daemon::new("daemon".to_string())
            .code(alien_core::DaemonCode::Image {
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
            .build();
        stack.resources.insert(
            "daemon".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(daemon),
                lifecycle: ResourceLifecycle::Live,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );
        stack
    }

    #[tokio::test]
    async fn identityless_cloud_daemon_refuses_actual_vault_slots() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
        ] {
            let stack = secret_daemon_stack();
            let mut config = deployer_secret_config();
            if platform == Platform::Kubernetes {
                config.base_platform = Some(Platform::Gcp);
            }
            // Presence cannot turn a pure deployer secret into a developer answer.
            config.stored_secret_input_ids = vec!["apiKey".to_string()];
            config.input_values.insert(
                "unrelated".to_string(),
                serde_json::json!("generic-canary-value"),
            );
            let state = StackState::new(platform);
            assert!(SecretsVaultMutation.should_run(&stack, &state, &config));
            let error = SecretsVaultMutation
                .mutate(stack, &state, &config)
                .await
                .expect_err("a vault slot requires workload identity");
            assert_eq!(error.code, "RESOURCE_VALIDATION_FAILED");
            let diagnostic = format!("{error} {error:?}");
            assert!(diagnostic.contains("explicit workload permission profile"));
            assert!(!diagnostic.contains("generic-canary-value"));
            assert!(!diagnostic.contains("apiKey"));
        }
        let mut dual = secret_daemon_stack();
        dual.inputs[0]
            .provided_by
            .push(StackInputProvider::Developer);
        let mut config = deployer_secret_config();
        config
            .input_values
            .insert("apiKey".to_string(), serde_json::Value::Null);
        config.stored_secret_input_ids = vec!["unrelated".to_string()];
        config
            .environment_variables
            .variables
            .push(alien_core::EnvironmentVariable {
                name: "API_KEY".to_string(),
                value: "generic-mapped-value".to_string(),
                var_type: alien_core::EnvironmentVariableType::Secret,
                target_resources: None,
            });
        assert!(
            SecretsVaultMutation
                .mutate(dual.clone(), &StackState::new(Platform::Gcp), &config)
                .await
                .is_err(),
            "mapped environment names must not bypass workload identity validation"
        );
        config.stored_secret_input_ids = vec!["apiKey".to_string()];
        let prepared = SecretsVaultMutation
            .mutate(dual, &StackState::new(Platform::Gcp), &config)
            .await
            .unwrap();
        assert!(prepared.permissions.profiles.is_empty());
        assert_eq!(config.input_values["apiKey"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn identityless_daemon_without_actual_vault_slots_remains_valid() {
        for case in [
            "none",
            "developer",
            "stored-dual",
            "concrete-dual",
            "other-platform",
            "other-target",
        ] {
            let mut stack = secret_daemon_stack();
            let mut config = deployer_secret_config();
            match case {
                "none" => stack.inputs.clear(),
                "developer" => stack.inputs[0].provided_by = vec![StackInputProvider::Developer],
                "stored-dual" | "concrete-dual" => {
                    stack.inputs[0]
                        .provided_by
                        .push(StackInputProvider::Developer);
                    if case == "stored-dual" {
                        config.stored_secret_input_ids = vec!["apiKey".to_string()];
                    } else {
                        config.input_values.insert(
                            "apiKey".to_string(),
                            serde_json::json!("generic-developer-value"),
                        );
                    }
                }
                "other-platform" => stack.inputs[0].platforms = Some(vec![Platform::Aws]),
                "other-target" => {
                    stack.inputs[0].env[0].target_resources = Some(vec!["other".to_string()])
                }
                _ => unreachable!(),
            }
            let result = SecretsVaultMutation
                .mutate(stack, &StackState::new(Platform::Gcp), &config)
                .await
                .unwrap();
            assert!(result.permissions.profiles.is_empty(), "{case}");
            assert!(result.resources["daemon"]
                .config
                .downcast_ref::<Daemon>()
                .unwrap()
                .permissions
                .is_none());
        }
        // Non-cloud boundaries retain their existing delivery/refusal handling.
        for platform in [Platform::Machines, Platform::Local, Platform::Kubernetes] {
            let result = SecretsVaultMutation
                .mutate(
                    secret_daemon_stack(),
                    &StackState::new(platform),
                    &deployer_secret_config(),
                )
                .await
                .unwrap();
            assert!(result.permissions.profiles.is_empty());
        }
    }

    #[tokio::test]
    async fn explicit_daemon_profile_receives_only_its_vault_keys() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let mut stack = secret_daemon_stack();
            stack
                .resources
                .get_mut("daemon")
                .unwrap()
                .config
                .downcast_mut::<Daemon>()
                .unwrap()
                .permissions = Some("execution".to_string());
            stack
                .permissions
                .profiles
                .insert("execution".to_string(), PermissionProfile::new());
            let mut other = stack.inputs[0].clone();
            other.id = "otherCredential".to_string();
            other.env[0].target_resources = Some(vec!["other".to_string()]);
            stack.inputs.push(other);
            let result = SecretsVaultMutation
                .mutate(stack, &StackState::new(platform), &deployer_secret_config())
                .await
                .unwrap();
            let expected = deployer_secrets_read_permission_set(&BTreeSet::from([
                alien_core::deployer_secret_vault_key("apiKey"),
            ]))
            .unwrap();
            assert_eq!(
                deployer_secrets_read_set(&result, "execution").unwrap(),
                expected
            );
            assert_eq!(
                result.permissions.profiles["execution"].0[SECRETS_VAULT_ID].len(),
                1
            );
        }
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

    fn deployer_secret_container(id: &str, profile: &str) -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                Container::new(id.to_string())
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
                    .permissions(profile.to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    /// A stack with containers `(id, profile)` and an `apiKey` deployer secret
    /// mapped into `targets`.
    fn deployer_secret_stack(containers: &[(&str, &str)], targets: &[&str]) -> Stack {
        let mut resources = IndexMap::new();
        let mut profiles = IndexMap::new();
        for (id, profile) in containers {
            resources.insert(id.to_string(), deployer_secret_container(id, profile));
            profiles.insert(profile.to_string(), PermissionProfile::new());
        }
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![StackInputDefinition {
                id: "apiKey".to_string(),
                kind: StackInputKind::Secret,
                provided_by: vec![StackInputProvider::Deployer],
                required: true,
                label: "API key".to_string(),
                description: String::new(),
                placeholder: None,
                default: None,
                platforms: None,
                validation: None,
                generate: None,
                env: vec![StackInputEnvironmentMapping {
                    name: "API_KEY".to_string(),
                    target_resources: Some(targets.iter().map(|t| t.to_string()).collect()),
                    var_type: None,
                }],
            }],
        }
    }

    fn deployer_secret_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    /// Every string value under a `key` field anywhere in `value`.
    fn collect_strings_under(value: &serde_json::Value, key: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    match v {
                        serde_json::Value::String(text) if k == key => out.push(text.clone()),
                        _ => collect_strings_under(v, key, out),
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect_strings_under(item, key, out);
                }
            }
            _ => {}
        }
    }

    /// The inline read set a profile got on the secrets vault, if any.
    fn deployer_secrets_read_set(stack: &Stack, profile: &str) -> Option<PermissionSet> {
        stack.permissions.profiles[profile]
            .0
            .get(SECRETS_VAULT_ID)?
            .iter()
            .find(|set| set.id() == DEPLOYER_SECRETS_READ_ID)
            .and_then(|set| set.resolve(|_| None))
    }

    /// The hosting layer reads a cloud-hosted workload's deployer secrets with
    /// the workload's own identity. That identity may read exactly the slots
    /// mapped into it: rendered for each cloud, the grant names those slots,
    /// spelled as the location the deployer writes to, and nothing else in
    /// the vault. Kubernetes and Local deliver them without the workload
    /// reading the vault.
    #[tokio::test]
    async fn cloud_workloads_may_read_exactly_their_deployer_secret_slots() {
        let stack =
            || deployer_secret_stack(&[("api", "api-profile"), ("web", "web-profile")], &["api"]);
        let config = deployer_secret_config();
        let key = alien_core::deployer_secret_vault_key("apiKey");
        let location_context = alien_core::DeployerSecretLocationContext::default();

        // AWS: ssm:GetParameter on exactly the slot's parameter.
        let aws = SecretsVaultMutation
            .mutate(stack(), &StackState::new(Platform::Aws), &config)
            .await
            .unwrap();
        assert!(deployer_secrets_read_set(&aws, "web-profile").is_none());
        let set = deployer_secrets_read_set(&aws, "api-profile").expect("api reads its slot");
        let context = alien_permissions::PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_resource_name("acme-secrets");
        let policy = alien_permissions::generators::AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, alien_permissions::BindingTarget::Resource, &context)
            .unwrap();
        let location = alien_core::deployer_secret_location(
            &alien_core::VaultBinding::parameter_store("acme-secrets"),
            &key,
            &location_context,
        )
        .unwrap();
        let policy = serde_json::to_value(&policy).unwrap();
        let statement = &policy["Statement"][0];
        assert_eq!(statement["Action"], serde_json::json!(["ssm:GetParameter"]));
        assert_eq!(
            statement["Resource"],
            serde_json::json!([format!(
                "arn:aws:ssm:us-east-1:123456789012:parameter/{}",
                location.name
            )])
        );

        // GCP: Secret Manager accessor conditioned on exactly the slot.
        let gcp = SecretsVaultMutation
            .mutate(stack(), &StackState::new(Platform::Gcp), &config)
            .await
            .unwrap();
        let set = deployer_secrets_read_set(&gcp, "api-profile").expect("api reads its slot");
        let context = alien_permissions::PermissionContext::new()
            .with_project_name("acme-prod")
            .with_project_number("424242")
            .with_resource_name("acme-secrets");
        let plan = alien_permissions::generators::GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(&set, alien_permissions::BindingTarget::Resource, &context)
            .unwrap();
        let secret = format!(
            "projects/424242/secrets/{}",
            alien_core::deployer_secret_location(
                &alien_core::VaultBinding::secret_manager("acme-secrets"),
                &key,
                &location_context,
            )
            .unwrap()
            .name
        );
        let plan = serde_json::to_value(&plan).unwrap();
        let mut expressions = Vec::new();
        collect_strings_under(&plan, "expression", &mut expressions);
        assert_eq!(
            expressions,
            vec![format!(
                "resource.name == \"{secret}\" || resource.name.startsWith(\"{secret}/\")"
            )],
            "the condition names exactly the slot: {plan}"
        );
        let plan = plan.to_string();
        assert!(
            plan.contains("roles/secretmanager.secretAccessor"),
            "{plan}"
        );
        assert!(!plan.contains("roles/secretmanager.viewer"), "{plan}");

        // Azure: Key Vault Secrets User on exactly the slot's secret.
        let azure = SecretsVaultMutation
            .mutate(stack(), &StackState::new(Platform::Azure), &config)
            .await
            .unwrap();
        let set = deployer_secrets_read_set(&azure, "api-profile").expect("api reads its slot");
        let context = alien_permissions::PermissionContext::new()
            .with_subscription_id("sub-1")
            .with_resource_group("rg-1")
            .with_resource_name("acmesecrets7f3a");
        let plan = alien_permissions::generators::AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(&set, alien_permissions::BindingTarget::Resource, &context)
            .unwrap();
        let secret_name = alien_core::deployer_secret_location(
            &alien_core::VaultBinding::key_vault("acmesecrets7f3a"),
            &key,
            &location_context,
        )
        .unwrap()
        .name;
        let plan = serde_json::to_string(&plan).unwrap();
        assert!(
            plan.contains(&format!(
                "/subscriptions/sub-1/resourceGroups/rg-1/providers/Microsoft.KeyVault/vaults/acmesecrets7f3a/secrets/{secret_name}\""
            )),
            "{plan}"
        );

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = SecretsVaultMutation
                .mutate(stack(), &StackState::new(platform), &config)
                .await
                .unwrap();
            let api = result.resources["api"]
                .config
                .downcast_ref::<Container>()
                .unwrap();
            assert!(
                api.links.iter().all(|link| link.id() != SECRETS_VAULT_ID),
                "the hosting layer reads the slot; the container needs no vault binding"
            );
            assert!(
                result.permissions.profiles["api-profile"].0[SECRETS_VAULT_ID]
                    .iter()
                    .all(|set| set.id() != "vault/data-read"),
                "{platform:?}: no whole-vault read"
            );
        }

        for platform in [Platform::Kubernetes, Platform::Local] {
            let result = SecretsVaultMutation
                .mutate(stack(), &StackState::new(platform), &config)
                .await
                .unwrap();
            assert!(
                deployer_secrets_read_set(&result, "api-profile").is_none(),
                "{platform:?}"
            );
        }
    }

    /// Workloads that share a permission profile share one cloud identity, so
    /// two that receive different deployer secrets cannot share a profile.
    #[tokio::test]
    async fn workloads_sharing_a_profile_must_receive_the_same_deployer_secrets() {
        let config = deployer_secret_config();
        let state = StackState::new(Platform::Aws);

        let mixed = deployer_secret_stack(&[("api", "shared"), ("web", "shared")], &["api"]);
        let error = SecretsVaultMutation
            .mutate(mixed, &state, &config)
            .await
            .expect_err("a profile shared with a workload that gets no secret must be refused");
        assert_eq!(error.code, "RESOURCE_VALIDATION_FAILED");
        let message = error.to_string();
        assert!(
            message.contains("share permission profile 'shared'"),
            "{message}"
        );

        // A secret the developer may also provide, with a stored developer
        // value, is not a slot: nothing reads it from the vault, so it neither
        // blocks the shared profile nor grants anything.
        let mut developer_valued =
            deployer_secret_stack(&[("api", "shared"), ("web", "shared")], &["api"]);
        developer_valued.inputs[0].provided_by =
            vec![StackInputProvider::Developer, StackInputProvider::Deployer];
        let mut with_value = deployer_secret_config();
        with_value.input_values.insert(
            "apiKey".to_string(),
            serde_json::json!("from-the-developer"),
        );
        let result = SecretsVaultMutation
            .mutate(developer_valued.clone(), &state, &with_value)
            .await
            .expect("a developer-valued secret is no slot and cannot block the profile");
        assert!(deployer_secrets_read_set(&result, "shared").is_none());
        // Without the developer value it is a slot again, so it is refused.
        assert!(SecretsVaultMutation
            .mutate(developer_valued, &state, &config)
            .await
            .is_err());

        let same = deployer_secret_stack(&[("api", "shared"), ("web", "shared")], &["api", "web"]);
        let result = SecretsVaultMutation
            .mutate(same, &state, &config)
            .await
            .unwrap();
        assert!(deployer_secrets_read_set(&result, "shared").is_some());
    }

    /// A deployment installed before deployer secrets were vault-native still
    /// delivers the value it stored, so its workload's setup-owned profile
    /// must not gain a vault read grant on the next update: the profile
    /// compatibility check would refuse that update until setup reran. Once
    /// the stored value is gone the slot is read from the vault and granted.
    #[tokio::test]
    async fn a_slot_with_a_stored_value_is_not_granted_until_it_is_vault_native() {
        let state = StackState::new(Platform::Aws);
        let stack = deployer_secret_stack(&[("api", "api-profile")], &["api"]);
        let mut legacy = deployer_secret_config();
        legacy.input_values.insert(
            "apiKey".to_string(),
            serde_json::json!("stored-before-slots"),
        );

        let updated = SecretsVaultMutation
            .mutate(stack.clone(), &state, &legacy)
            .await
            .expect("a stored deployer secret value prepares");
        assert!(deployer_secrets_read_set(&updated, "api-profile").is_none());
        assert_eq!(
            updated.permissions.profiles["api-profile"], stack.permissions.profiles["api-profile"],
            "the profile must stay as the earlier release installed it"
        );
        let compatibility = PermissionProfilesUnchangedCheck
            .check_with_config(&stack, &updated, &legacy)
            .await
            .expect("check runs");
        assert!(
            !compatibility
                .errors
                .iter()
                .any(|error| error.contains("api-profile")),
            "{:?}",
            compatibility.errors
        );

        let vault_native = SecretsVaultMutation
            .mutate(stack, &state, &deployer_secret_config())
            .await
            .expect("an empty slot prepares");
        assert!(deployer_secrets_read_set(&vault_native, "api-profile").is_some());
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
