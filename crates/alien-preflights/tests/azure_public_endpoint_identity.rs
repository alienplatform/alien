//! The identity for an HTTPS public endpoint, driven through the built-in preflight registry:
//! later mutations (service accounts, vault grants, management, dependency wiring) must accept
//! it as a service account without permission sets and must not add grants to it.

use alien_core::{
    public_endpoint_identity_id, AzureManagementConfig, ComputeCluster, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExposeProtocol, ExternalBindings,
    ManagementConfig, PermissionProfile, PermissionsConfig, Platform, PublicEndpoint,
    ResourceLifecycle, ResourceRef, ResourceSpec, ServiceAccount, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;

fn container(id: &str, endpoints: &[ExposeProtocol]) -> Container {
    let mut container = Container::new(id.to_string())
        .cluster("pool".to_string())
        .permissions("execution".to_string())
        .code(ContainerCode::Image {
            image: "app:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.25".into(),
            desired: "0.25".into(),
        })
        .memory(ResourceSpec {
            min: "256Mi".into(),
            desired: "256Mi".into(),
        })
        .port(8080)
        .build();
    container.public_endpoints = endpoints
        .iter()
        .enumerate()
        .map(|(index, protocol)| PublicEndpoint {
            name: format!("endpoint-{index}"),
            port: 8080,
            protocol: *protocol,
            host_label: None,
            wildcard_subdomains: false,
        })
        .collect();
    container
}

async fn prepared() -> Stack {
    let stack = Stack::new("endpoint-identity".to_string())
        .permissions(
            PermissionsConfig::new()
                .with_profile("execution", PermissionProfile::new().global(["storage/data-read"])),
        )
        .add(
            ComputeCluster::new("pool".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            container("api", &[ExposeProtocol::Tcp, ExposeProtocol::Http]),
            ResourceLifecycle::Live,
        )
        .add(
            container("db", &[ExposeProtocol::Tcp]),
            ResourceLifecycle::Live,
        )
        .build();
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "management-tenant".to_string(),
            oidc_issuer: "https://issuer.example".to_string(),
            oidc_subject: "system:serviceaccount:alien:manager".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    PreflightRunner::new()
        .apply_mutations(stack, &StackState::new(Platform::Azure), &config)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_endpoint_identity_survives_the_full_registry_without_grants() {
    let stack = prepared().await;
    let id = public_endpoint_identity_id("api");
    let entry = stack.resources.get(&id).expect("identity in prepared stack");
    assert_eq!(entry.lifecycle, ResourceLifecycle::Frozen);
    let account = entry.config.downcast_ref::<ServiceAccount>().unwrap();
    assert!(account.stack_permission_sets.is_empty());
    assert!(account.resource_permission_sets.is_empty());
    // A TCP-first container with an HTTP endpoint behind it still gets one; a TCP-only one does not.
    assert!(!stack.resources.contains_key(&public_endpoint_identity_id("db")));

    // No permission profile, management grant or other resource's grant is aimed at it.
    for profile in stack.permissions.profiles.values() {
        assert!(!profile.0.contains_key(&id), "profile grants to {id}");
    }
    if let Some(profile) = stack.management().profile() {
        assert!(!profile.0.contains_key(&id), "management grants on {id}");
    }
    for (other_id, other) in stack.resources() {
        if let Some(account) = other.config.downcast_ref::<ServiceAccount>() {
            assert!(
                !account.resource_permission_sets.contains_key(&id),
                "{other_id} carries a grant on {id}"
            );
        }
    }
    // The cluster and container wait for it; it waits for the resource group.
    let reference = ResourceRef::new(ServiceAccount::RESOURCE_TYPE, id.clone());
    assert!(stack.resources["api"].dependencies.contains(&reference));
    assert!(stack.resources["pool"].dependencies.contains(&reference));
    assert!(entry
        .dependencies
        .iter()
        .any(|dependency| dependency.id() == "default-resource-group"));
}

#[tokio::test]
async fn preparing_the_prepared_stack_again_is_stable() {
    let first = prepared().await;
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "management-tenant".to_string(),
            oidc_issuer: "https://issuer.example".to_string(),
            oidc_subject: "system:serviceaccount:alien:manager".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let second = PreflightRunner::new()
        .apply_mutations(first.clone(), &StackState::new(Platform::Azure), &config)
        .await
        .unwrap();
    assert_eq!(first, second);
}
