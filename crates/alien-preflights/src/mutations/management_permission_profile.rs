use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::{
    ownership_policy_for_resource_type, Container, DeploymentConfig, ExposeProtocol, Ingress,
    KubernetesCertificateMode, KubernetesCluster, KubernetesExposureSettings,
    KubernetesHeartbeatMode, KubernetesIngressRouteProfile, KubernetesRouteProfile,
    KubernetesRouteProviderOptions, Platform, ResourceLifecycle, Stack, StackState, Storage,
    Worker, WorkerTrigger,
};
use alien_permissions::get_permission_set;
use indexmap::IndexMap;
use std::collections::BTreeSet;

const OBSERVE_PERMISSION_SET_ID: &str = "observe/observe";

/// Automatically adds management permission profile with necessary permissions for all resources in the stack.
///
/// This mutation generates management permissions based on resource lifecycles and feature policies:
/// - Live resources get `<resourceType>/provision`.
/// - Frozen resources get management only when their ownership policy allows
///   Alien to operate part of an existing setup-owned resource.
/// - Resources get `<resourceType>/heartbeat` permission sets (when heartbeat is not Disabled)
/// - Resources get `<resourceType>/telemetry` permission sets (when telemetry is not Disabled)
pub struct ManagementPermissionProfileMutation;

#[async_trait::async_trait]
impl StackMutation for ManagementPermissionProfileMutation {
    fn description(&self) -> &'static str {
        "Automatically add management permission profile with necessary permissions for all resources in the stack"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only run if management is set to Auto or if we need to extend existing permissions
        match stack.management() {
            ManagementPermissions::Auto => true,
            ManagementPermissions::Extend(_) => true,
            ManagementPermissions::Override(_) => true,
        }
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let current_management = stack.management().clone();

        match current_management {
            ManagementPermissions::Auto => {
                // Auto-generate management permissions based on resource lifecycles
                let management_profile =
                    generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(profile) = management_profile {
                    stack.permissions.management = ManagementPermissions::Extend(profile);
                }
            }
            ManagementPermissions::Extend(extend_profile) => {
                // Generate auto permissions first, then merge with extend profile
                let auto_profile = generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(mut auto_profile) = auto_profile {
                    // Merge the extend profile into the auto-generated profile
                    for (scope, permission_sets) in &extend_profile.0 {
                        if let Some(existing_permissions) = auto_profile.0.get_mut(scope) {
                            // Add new permission sets, avoiding duplicates
                            for permission_set in permission_sets {
                                if !existing_permissions.contains(permission_set) {
                                    existing_permissions.push(permission_set.clone());
                                }
                            }
                        } else {
                            // Add new scope if it doesn't exist
                            auto_profile
                                .0
                                .insert(scope.clone(), permission_sets.clone());
                        }
                    }
                    stack.permissions.management = ManagementPermissions::Extend(auto_profile);
                } else {
                    // No auto permissions generated, keep the extend profile as is
                    stack.permissions.management = ManagementPermissions::Extend(extend_profile);
                }
            }
            ManagementPermissions::Override(mut override_profile) => {
                ensure_observe_permission(&mut override_profile);
                stack.permissions.management = ManagementPermissions::Override(override_profile);
            }
        }

        Ok(stack)
    }
}

fn ensure_observe_permission(profile: &mut PermissionProfile) {
    let global_permissions = profile.0.entry("*".to_string()).or_default();
    let observe_ref = PermissionSetReference::from_name(OBSERVE_PERMISSION_SET_ID);
    if !global_permissions.contains(&observe_ref) {
        // TODO: move observe permissions to a dedicated read-only role, or gate
        // this mandatory management grant once observe-only deployments have
        // their own role model.
        global_permissions.push(observe_ref);
    }
}

/// Generates the default management permission profile from resource ownership
/// and feature settings.
fn generate_auto_management_profile(
    stack: &Stack,
    stack_state: &StackState,
    config: &DeploymentConfig,
) -> Result<Option<PermissionProfile>> {
    let mut permission_set_ids = BTreeSet::new();
    let mut resource_permission_set_ids: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    let platform = stack_state.platform;

    permission_set_ids.insert(OBSERVE_PERMISSION_SET_ID.to_string());

    // Iterate through all resources in the stack to determine required management permissions
    for (resource_id, resource_entry) in stack.resources() {
        let resource_type_value = resource_entry.config.resource_type();
        let resource_type = resource_type_value.0.as_ref();
        let permission_resource_type = permission_resource_type(resource_type);
        let policy = ownership_policy_for_resource_type(resource_type);

        match resource_entry.lifecycle {
            ResourceLifecycle::Live => {
                // Live resources are Alien-owned. Provision is required so
                // Alien can create, replace, and delete them after setup.
                permission_set_ids.insert(format!("{}/provision", permission_resource_type));
            }
            ResourceLifecycle::Frozen if policy.requires_management_permissions() => {
                permission_set_ids.insert(format!("{}/management", permission_resource_type));
            }
            ResourceLifecycle::Frozen => {
                // Frozen resources are setup-owned by default. Heartbeat,
                // telemetry, and explicit policy-granted management are added
                // independently.
            }
        }

        // Add heartbeat permissions if heartbeat is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.heartbeats.is_enabled() {
            add_cloud_heartbeat_permission(
                resource_id,
                resource_type,
                permission_resource_type,
                resource_entry,
                &mut permission_set_ids,
                &mut resource_permission_set_ids,
            );
        }

        // Add telemetry permissions if telemetry is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.telemetry.is_enabled() {
            permission_set_ids.insert(format!("{}/telemetry", permission_resource_type));
        }

        // Add command dispatch permissions for workers with commands_enabled = true.
        if resource_type == "worker" {
            if let Some(worker) = resource_entry.config.downcast_ref::<Worker>() {
                if worker.commands_enabled {
                    match platform {
                        Platform::Aws | Platform::Gcp | Platform::Azure => {
                            // Preflights author the explicit command dispatch grant for
                            // this concrete worker. Each cloud maps it to that worker's
                            // platform command transport.
                            resource_permission_set_ids
                                .entry(resource_id.clone())
                                .or_default()
                                .insert("worker/dispatch-command".to_string());
                        }
                        _ => {
                            // Other platforms like Kubernetes and Local use HTTP polling.
                        }
                    }
                }
            }
        }

        if platform == Platform::Kubernetes
            && kubernetes_exposure_needs_acm_import(config)
            && resource_needs_kubernetes_public_endpoint(resource_entry)
        {
            resource_permission_set_ids
                .entry(resource_id.clone())
                .or_default()
                .insert("kubernetes-public-endpoint/management".to_string());
        }

        add_storage_trigger_source_management_permissions(
            stack,
            platform,
            resource_entry,
            &mut resource_permission_set_ids,
        );
    }

    // Always include the observe grant. It is read-only and lets the management
    // role populate cloud inventory in Operate mode.
    if permission_set_ids.is_empty() && resource_permission_set_ids.is_empty() {
        return Ok(None);
    }

    fn resolve_permission_refs(
        permission_set_ids: BTreeSet<String>,
    ) -> Vec<PermissionSetReference> {
        let mut valid_permission_refs = Vec::new();
        for permission_set_id in permission_set_ids {
            if get_permission_set(&permission_set_id).is_some() {
                valid_permission_refs.push(PermissionSetReference::from_name(permission_set_id));
            } else {
                // Log warning but continue - allows system to work even if some permission sets are missing
                tracing::debug!(
                    permission_set_id = %permission_set_id,
                    "Management permission set not found in registry, skipping"
                );
            }
        }
        valid_permission_refs
    }

    // Validate permission sets exist in registry and filter out missing ones
    let valid_permission_refs = resolve_permission_refs(permission_set_ids);

    // Create the management permission profile. Auto lifecycle permissions are
    // wildcard-scoped. Permissions that address imported/existing physical
    // resources, such as KubernetesCluster cloud metadata heartbeats, are scoped
    // to the concrete resource so setup can use its actual provider identity.
    let mut management_permissions = IndexMap::new();
    if !valid_permission_refs.is_empty() {
        management_permissions.insert("*".to_string(), valid_permission_refs);
    }

    for (resource_id, permission_set_ids) in resource_permission_set_ids {
        let valid_resource_refs = resolve_permission_refs(permission_set_ids);
        if !valid_resource_refs.is_empty() {
            management_permissions.insert(resource_id, valid_resource_refs);
        }
    }

    if management_permissions.is_empty() {
        return Ok(None);
    }

    Ok(Some(PermissionProfile(management_permissions)))
}

fn add_storage_trigger_source_management_permissions(
    stack: &Stack,
    platform: Platform,
    resource_entry: &alien_core::ResourceEntry,
    resource_permission_set_ids: &mut IndexMap<String, BTreeSet<String>>,
) {
    if !matches!(platform, Platform::Aws | Platform::Gcp) {
        return;
    }

    if resource_entry.lifecycle != ResourceLifecycle::Live {
        return;
    }

    let Some(worker) = resource_entry.config.downcast_ref::<Worker>() else {
        return;
    };

    for trigger in &worker.triggers {
        let WorkerTrigger::Storage { storage, .. } = trigger else {
            continue;
        };
        if storage.resource_type != Storage::RESOURCE_TYPE {
            continue;
        }

        let Some(source_entry) = stack.resources.get(storage.id()) else {
            continue;
        };
        if source_entry.lifecycle != ResourceLifecycle::Frozen {
            continue;
        }

        resource_permission_set_ids
            .entry(storage.id().to_string())
            .or_default()
            .insert("storage/trigger-management".to_string());
    }
}

fn kubernetes_exposure_needs_acm_import(config: &DeploymentConfig) -> bool {
    let Some(KubernetesExposureSettings::Generated { route, certificate }) = config
        .stack_settings
        .kubernetes
        .as_ref()
        .and_then(|settings| settings.exposure.as_ref())
    else {
        return false;
    };

    matches!(
        certificate,
        KubernetesCertificateMode::ManagedAcmImport { .. }
    ) && matches!(
        route,
        KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            provider: Some(KubernetesRouteProviderOptions::AwsAlb { .. }),
            ..
        })
    )
}

fn resource_needs_kubernetes_public_endpoint(resource_entry: &alien_core::ResourceEntry) -> bool {
    resource_entry
        .config
        .downcast_ref::<Worker>()
        .is_some_and(|worker| worker.ingress == Ingress::Public)
        || resource_entry
            .config
            .downcast_ref::<Container>()
            .is_some_and(|container| {
                container
                    .ports
                    .iter()
                    .any(|port| port.expose == Some(ExposeProtocol::Http))
            })
}

fn resource_needs_cloud_heartbeat_permission(
    resource_type: &str,
    resource_entry: &alien_core::ResourceEntry,
) -> bool {
    if resource_type != KubernetesCluster::RESOURCE_TYPE.as_ref() {
        return true;
    }

    resource_entry
        .config
        .downcast_ref::<KubernetesCluster>()
        .is_some_and(|cluster| {
            cluster.heartbeat_mode == KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata
        })
}

fn add_cloud_heartbeat_permission(
    resource_id: &str,
    resource_type: &str,
    permission_resource_type: &str,
    resource_entry: &alien_core::ResourceEntry,
    permission_set_ids: &mut BTreeSet<String>,
    resource_permission_set_ids: &mut IndexMap<String, BTreeSet<String>>,
) {
    if !resource_needs_cloud_heartbeat_permission(resource_type, resource_entry) {
        return;
    }

    let permission_set_id = format!("{}/heartbeat", permission_resource_type);
    if resource_type == KubernetesCluster::RESOURCE_TYPE.as_ref() {
        resource_permission_set_ids
            .entry(resource_id.to_string())
            .or_default()
            .insert(permission_set_id);
    } else {
        permission_set_ids.insert(permission_set_id);
    }
}

fn permission_resource_type(resource_type: &str) -> &str {
    match resource_type {
        "azure_resource_group" => "azure-resource-group",
        "azure_storage_account" => "azure-storage-account",
        "azure_container_apps_environment" => "azure-container-apps-environment",
        "azure_service_bus_namespace" => "azure-service-bus-namespace",
        "service_activation" => "service-activation",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup,
        AzureServiceBusNamespace, AzureStorageAccount, CapacityGroup, ComputeCluster, Container,
        ContainerCode, DeploymentModel, EnvironmentVariablesSnapshot, ExternalBindings,
        HeartbeatsMode, Ingress, KubernetesCertificateMode, KubernetesCluster,
        KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesExposureSettings,
        KubernetesHeartbeatMode, KubernetesIngressRouteProfile, KubernetesRouteProfile,
        KubernetesRouteProviderOptions, KubernetesSettings, ResourceEntry, ResourceLifecycle,
        ResourceSpec, ServiceActivation, StackSettings, StackState, Storage, TelemetryMode, Worker,
        WorkerCode,
    };

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn kubernetes_generated_aws_alb_acm_settings() -> StackSettings {
        StackSettings {
            kubernetes: Some(KubernetesSettings {
                cluster: None,
                exposure: Some(KubernetesExposureSettings::Generated {
                    route: aws_alb_route_profile(),
                    certificate: KubernetesCertificateMode::ManagedAcmImport {
                        region: None,
                        tags: Default::default(),
                    },
                }),
            }),
            ..Default::default()
        }
    }

    fn kubernetes_custom_aws_alb_byo_acm_settings() -> StackSettings {
        StackSettings {
            kubernetes: Some(KubernetesSettings {
                cluster: None,
                exposure: Some(KubernetesExposureSettings::Custom {
                    domain: "api.example.com".to_string(),
                    route: aws_alb_route_profile(),
                    certificate: KubernetesCertificateMode::AwsAcmArn {
                        certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/customer"
                            .to_string(),
                    },
                }),
            }),
            ..Default::default()
        }
    }

    fn aws_alb_route_profile() -> KubernetesRouteProfile {
        KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            ingress_class_name: "alb".to_string(),
            provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                scheme: "internet-facing".to_string(),
                target_type: "ip".to_string(),
                ip_address_type: None,
                subnet_ids: Vec::new(),
            }),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn test_auto_management_profile_generation() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen, // Should get nothing (heartbeat only)
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that management permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                // Live worker gets provision; storage is frozen so no management.
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(permission_names.contains(&OBSERVE_PERMISSION_SET_ID.to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
                assert!(!permission_names.contains(&"aws/tag-tamper-protection".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn storage_trigger_from_frozen_storage_gets_source_management_permission() {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let global_permissions = profile.0.get("*").expect("global management grants");
        let global_permission_names: Vec<String> = global_permissions
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(global_permission_names.contains(&"worker/provision".to_string()));

        let storage_permissions = profile
            .0
            .get("uploads")
            .expect("storage trigger source management grants");
        let storage_permission_names: Vec<String> = storage_permissions
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(storage_permission_names.contains(&"storage/trigger-management".to_string()));
    }

    #[tokio::test]
    async fn azure_storage_trigger_from_frozen_storage_does_not_get_source_management_permission() {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Azure);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        assert!(
            !profile.0.contains_key("uploads"),
            "Azure Dapr trigger wiring should be covered by worker/container-app permissions"
        );
    }

    #[tokio::test]
    async fn kubernetes_cluster_cloud_metadata_heartbeat_is_explicit() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let global_permission_names: Vec<String> = profile
            .0
            .get("*")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(
            global_permission_names == vec![OBSERVE_PERMISSION_SET_ID.to_string()],
            "Only observe should be global; KubernetesCluster heartbeat should be resource-scoped because existing clusters do not necessarily use the deployment prefix"
        );
        let permission_names: Vec<String> = profile
            .0
            .get("kubernetes")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(permission_names.contains(&"kubernetes-cluster/heartbeat".to_string()));
        assert!(!permission_names.contains(&"compute-cluster/heartbeat".to_string()));
    }

    #[tokio::test]
    async fn kubernetes_api_only_heartbeat_gets_no_cloud_metadata_permission() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Generic)
            .ownership(KubernetesClusterOwnership::External)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let global_permission_names: Vec<String> = profile
            .0
            .get("*")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert_eq!(
            global_permission_names,
            vec![OBSERVE_PERMISSION_SET_ID.to_string()]
        );
        assert!(
            !profile.0.contains_key("kubernetes"),
            "API-only Kubernetes heartbeat should not author Kubernetes cloud metadata permissions"
        );
    }

    #[tokio::test]
    async fn azure_setup_resource_heartbeats_use_permission_set_names() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureContainerAppsEnvironment::new("container-apps-env".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureServiceBusNamespace::new("service-bus".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceActivation::new("enable-storage".to_string())
                    .service_name("Microsoft.Storage".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .management(ManagementPermissions::Auto)
            .build();

        let stack_state = StackState::new(Platform::Azure);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let permission_names: Vec<String> = profile
            .0
            .get("*")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();

        for permission_set in [
            "azure-resource-group/heartbeat",
            "azure-storage-account/heartbeat",
            "azure-container-apps-environment/heartbeat",
            "azure-service-bus-namespace/heartbeat",
            "service-activation/heartbeat",
        ] {
            assert!(
                permission_names.contains(&permission_set.to_string()),
                "expected generated management profile to include {permission_set}"
            );
        }
        assert!(!permission_names.iter().any(|permission| {
            permission.contains("azure_") || permission.contains("service_activation")
        }));
    }

    #[tokio::test]
    async fn kubernetes_generated_aws_alb_public_worker_gets_acm_permission() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .ingress(Ingress::Public)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(kubernetes_generated_aws_alb_acm_settings())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let permission_names: Vec<String> = profile
            .0
            .get("api")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();

        assert!(permission_names.contains(&"kubernetes-public-endpoint/management".to_string()));
    }

    #[tokio::test]
    async fn kubernetes_byo_acm_public_worker_gets_no_managed_acm_permission() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .ingress(Ingress::Public)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(kubernetes_custom_aws_alb_byo_acm_settings())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        assert!(
            !profile.0.contains_key("api"),
            "BYO ACM should not add Kubernetes managed ACM permissions"
        );
    }

    #[tokio::test]
    async fn test_extend_management_profile_merge() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        // Create an extend profile with additional permissions
        let extend_profile = PermissionProfile::new().global(["storage/data-write"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(extend_profile),
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both auto-generated and extended permissions are present
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have auto-generated live provision and extended storage/data-write.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(permission_names.contains(&OBSERVE_PERMISSION_SET_ID.to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(permission_names.contains(&"storage/data-write".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_override_management_profile_keeps_user_grants_and_adds_observe() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        // Create an override profile
        let override_profile =
            PermissionProfile::new().global(["storage/management", "worker/management"]);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Override(override_profile.clone()),
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that override profile keeps user grants, gets mandatory observe,
        // and does not receive other auto-generated permissions.
        match result_stack.management() {
            ManagementPermissions::Override(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert_eq!(global_permissions.len(), 3);

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"storage/management".to_string()));
                assert!(permission_names.contains(&"worker/management".to_string()));
                assert!(permission_names.contains(&OBSERVE_PERMISSION_SET_ID.to_string()));
                // Should NOT have auto-generated worker/provision
                assert!(!permission_names.contains(&"worker/provision".to_string()));
            }
            _ => panic!("Expected Override management permissions"),
        }
    }

    #[tokio::test]
    async fn test_pull_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Create stack state with Pull model. Permission derivation is model
        // independent: the credentials are attached differently, but the
        // resource operations required by Alien are the same.
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions for both resources
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"storage/heartbeat".to_string()));

                // Should contain live resource mutation permissions in both
                // Pull and Push models.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_live_container_and_frozen_cluster_permissions() {
        let container = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test".to_string())
            .build();
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.large".to_string()),
                profile: None,
                min_size: 1,
                max_size: 3,
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "web".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"container/provision".to_string()));
                assert!(!permission_names.contains(&"container/management".to_string()));
                assert!(permission_names.contains(&"compute-cluster/management".to_string()));
                assert!(!permission_names.contains(&"compute-cluster/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_frozen_artifact_registry_gets_management_permissions() {
        let registry = ArtifactRegistry::new("registry".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"artifact-registry/management".to_string()));
                assert!(!permission_names.contains(&"artifact-registry/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_push_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Create stack state with Push model (Manager deploys remotely)
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Push,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both heartbeat and provision permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat plus live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_commands_enabled_function_permissions() {
        // Test AWS platform - should add resource-scoped worker/dispatch-command.
        let arc_function = Worker::new("arc-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "arc-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(arc_function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that Commands permissions were added
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }

        // Test GCP platform - should author an explicit command dispatch grant.
        let stack_state_gcp = StackState::new(Platform::Gcp);
        let config_gcp = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let stack_gcp = Stack {
            id: "test-stack-gcp".to_string(),
            resources: {
                let mut resources = IndexMap::new();
                let arc_function = Worker::new("arc-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("test".to_string())
                    .commands_enabled(true)
                    .build();
                resources.insert(
                    "arc-worker".to_string(),
                    ResourceEntry {
                        config: alien_core::Resource::new(arc_function),
                        lifecycle: ResourceLifecycle::Live,
                        dependencies: Vec::new(),
                        remote_access: false,
                    },
                );
                resources
            },
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let result_stack_gcp = mutation
            .mutate(stack_gcp, &stack_state_gcp, &config_gcp)
            .await
            .unwrap();

        match result_stack_gcp.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_heartbeat_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Disable heartbeat
        let stack_settings = StackSettings {
            heartbeats: HeartbeatsMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain heartbeat permissions since heartbeat is disabled
                assert!(!permission_names.contains(&"worker/heartbeat".to_string()));
                // Should still have live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_telemetry_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Disable telemetry
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain telemetry permissions since telemetry is disabled
                assert!(!permission_names.contains(&"worker/telemetry".to_string()));
                // Should still have heartbeat and live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_requires_approval_still_creates_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        // Require approval for telemetry - permissions should still be created
        // Note: ApprovalRequired is a runtime check, permissions are infrastructure.
        // When telemetry is ApprovalRequired (not Off), permissions should be created
        // for telemetry collection. The approval gate happens at runtime.
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::ApprovalRequired,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions (heartbeat is On by default)
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                // Should contain live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                // Note: worker/telemetry permission set may not exist in registry,
                // but the code attempts to add it. If it exists, it would be added.
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }
}
