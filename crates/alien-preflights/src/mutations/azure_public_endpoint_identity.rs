//! Add the setup-owned identity that each container's HTTPS public endpoint runs as.

use alien_core::{
    public_endpoint_identity_cluster, public_endpoint_identity_id, Container, DeploymentConfig,
    Platform, Resource, ResourceEntry, ResourceLifecycle, ResourceRef, ServiceAccount, Stack,
    StackState,
};
use async_trait::async_trait;

use alien_error::AlienError;

use crate::{error::{ErrorData, Result}, StackMutation};

const IDENTITY_DESCRIPTION: &str = "The identity the container's HTTPS public endpoint runs as. \
     The cluster's machines are allowed to attach it.";

/// Whether `entry` is an identity like the one this mutation adds: a setup-owned service
/// account that holds no permissions. One declared by hand in that shape is the same thing; one
/// that holds permissions would be attachable by the cluster's machines, so it is refused.
fn is_public_endpoint_identity(entry: &ResourceEntry) -> bool {
    entry.lifecycle == ResourceLifecycle::Frozen
        && entry.config.downcast_ref::<ServiceAccount>().is_some_and(|account| {
            account.stack_permission_sets.is_empty()
                && account.resource_permission_sets.values().all(|sets| sets.is_empty())
        })
}

const RESOURCE_GROUP_ID: &str = "default-resource-group";

/// On Azure an HTTPS public endpoint's load balancer is created at runtime with a
/// user-assigned identity, and the cluster's machines must be allowed to attach it. Role
/// assignments belong to setup, so the identity exists before the container does: setup creates
/// it, lets the cluster's machines attach it, and the container only uses it.
///
/// The identity is a [`ServiceAccount`] without permission sets. The cluster and the container
/// both depend on it, so it exists before the machines are granted access and before the
/// container is created. Adding an HTTPS public endpoint to a release adds a frozen resource,
/// which the update check reports as needing a setup update.
pub struct AzurePublicEndpointIdentityMutation;

#[async_trait]
impl StackMutation for AzurePublicEndpointIdentityMutation {
    fn description(&self) -> &'static str {
        "Add an identity for each container's HTTPS public endpoint"
    }

    fn should_run(&self, stack: &Stack, state: &StackState, _config: &DeploymentConfig) -> bool {
        state.platform == Platform::Azure
            && stack.resources().any(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .and_then(public_endpoint_identity_cluster)
                    .is_some()
            })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let endpoints: Vec<(String, String)> = stack
            .resources()
            .filter_map(|(id, entry)| {
                let container = entry.config.downcast_ref::<Container>()?;
                let cluster = public_endpoint_identity_cluster(container)?;
                Some((id.clone(), cluster.to_string()))
            })
            .collect();
        for (container_id, cluster_id) in endpoints {
            let identity_id = public_endpoint_identity_id(&container_id);
            let identity_ref = ResourceRef::new(ServiceAccount::RESOURCE_TYPE, identity_id.clone());
            // Preparing a stack again keeps the entry (and its display text) as it is. A
            // different resource under this id is refused: the cluster's machines are allowed
            // to attach whatever identity lives here.
            if let Some(existing) = stack.resources.get(&identity_id) {
                if !is_public_endpoint_identity(existing) {
                    return Err(AlienError::new(ErrorData::StackMutationFailed {
                        mutation_name: self.description().to_string(),
                        message: format!(
                            "the id '{identity_id}' is reserved for the identity of container \
                             '{container_id}''s HTTPS public endpoint; rename the resource"
                        ),
                        resource_id: Some(identity_id.clone()),
                    }));
                }
            } else {
                let mut dependencies = Vec::new();
                if stack.resources.contains_key(RESOURCE_GROUP_ID) {
                    dependencies.push(ResourceRef::new(
                        alien_core::AzureResourceGroup::RESOURCE_TYPE,
                        RESOURCE_GROUP_ID,
                    ));
                }
                stack.resources.insert(
                    identity_id.clone(),
                    ResourceEntry {
                        enabled_when: None,
                        config: Resource::new(ServiceAccount::new(identity_id.clone()).build()),
                        lifecycle: ResourceLifecycle::Frozen,
                        dependencies,
                        display_name: Some(format!("Public endpoint identity ({container_id})")),
                        description: Some(IDENTITY_DESCRIPTION.to_string()),
                        remote_access: false,
                    },
                );
            }
            for user in [&container_id, &cluster_id] {
                if let Some(entry) = stack.resources.get_mut(user) {
                    if !entry.dependencies.contains(&identity_ref) {
                        entry.dependencies.push(identity_ref.clone());
                    }
                }
            }
        }
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::frozen_resources_unchanged::FrozenResourcesUnchangedCheck;
    use crate::StackCompatibilityCheck;
    use alien_core::{
        ComputeCluster, ContainerCode, ExposeProtocol, ExternalBindings, PublicEndpoint,
        ResourceSpec, StackSettings,
    };

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    fn container(id: &str, protocol: Option<ExposeProtocol>) -> Container {
        let builder = Container::new(id.to_string())
            .permissions("execution".to_string())
            .cluster("pool".to_string())
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
            .port(8080);
        match protocol {
            Some(protocol) => builder
                .public_endpoint(PublicEndpoint {
                    name: "default".into(),
                    port: 8080,
                    protocol,
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .build(),
            None => builder.build(),
        }
    }

    fn stack(containers: Vec<Container>) -> Stack {
        let mut builder = Stack::new("test-stack".to_string())
            .add(ComputeCluster::new("pool".to_string()).build(), ResourceLifecycle::Frozen);
        for container in containers {
            builder = builder.add(container, ResourceLifecycle::Live);
        }
        builder.build()
    }

    #[tokio::test]
    async fn an_https_endpoint_gets_a_frozen_identity_the_cluster_and_container_wait_for() {
        let state = StackState::new(Platform::Azure);
        let stack = stack(vec![
            container("api", Some(ExposeProtocol::Http)),
            container("db", Some(ExposeProtocol::Tcp)),
            container("worker", None),
        ]);
        let mutation = AzurePublicEndpointIdentityMutation;
        assert!(mutation.should_run(&stack, &state, &config()));
        let stack = mutation.mutate(stack, &state, &config()).await.unwrap();

        let identity = stack.resources.get("api-appgw-id").expect("identity added");
        assert_eq!(identity.lifecycle, ResourceLifecycle::Frozen);
        let account = identity.config.downcast_ref::<ServiceAccount>().unwrap();
        assert!(account.stack_permission_sets.is_empty());
        assert!(account.resource_permission_sets.is_empty());
        let reference = ResourceRef::new(ServiceAccount::RESOURCE_TYPE, "api-appgw-id");
        for user in ["api", "pool"] {
            assert!(
                stack.resources[user].dependencies.contains(&reference),
                "{user} waits for the identity"
            );
        }
        // Only the HTTPS endpoint gets one.
        assert_eq!(
            stack
                .resources
                .keys()
                .filter(|id| id.ends_with("-appgw-id"))
                .count(),
            1
        );
        // Preparing the prepared stack again changes nothing.
        let again = mutation
            .mutate(stack.clone(), &state, &config())
            .await
            .unwrap();
        assert_eq!(again, stack);
    }

    #[tokio::test]
    async fn a_live_resource_under_the_identity_id_is_refused() {
        let state = StackState::new(Platform::Azure);
        let mut stack = stack(vec![container("api", Some(ExposeProtocol::Http))]);
        stack.resources.insert(
            "api-appgw-id".to_string(),
            ResourceEntry {
                enabled_when: None,
                config: Resource::new(ServiceAccount::new("api-appgw-id".to_string()).build()),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
            },
        );
        let error = AzurePublicEndpointIdentityMutation
            .mutate(stack, &state, &config())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("api-appgw-id"), "{error}");
    }

    #[tokio::test]
    async fn a_service_account_with_permissions_under_the_identity_id_is_refused() {
        let state = StackState::new(Platform::Azure);
        let mut stack = stack(vec![container("api", Some(ExposeProtocol::Http))]);
        let holder = ServiceAccount::new("api-appgw-id".to_string())
            .stack_permission_set(alien_core::PermissionSet {
                id: "storage/data-read".to_string(),
                description: "read".to_string(),
                platforms: alien_core::PlatformPermissions { aws: None, gcp: None, azure: None },
            })
            .build();
        stack.resources.insert(
            "api-appgw-id".to_string(),
            ResourceEntry {
                enabled_when: None,
                config: Resource::new(holder),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
            },
        );
        assert!(AzurePublicEndpointIdentityMutation
            .mutate(stack, &state, &config())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_tcp_endpoint_listed_first_does_not_hide_the_https_one() {
        let mut both = container("api", Some(ExposeProtocol::Tcp));
        both.public_endpoints.push(PublicEndpoint {
            name: "web".into(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        });
        let state = StackState::new(Platform::Azure);
        let stack = stack(vec![both]);
        assert!(AzurePublicEndpointIdentityMutation.should_run(&stack, &state, &config()));
        let stack = AzurePublicEndpointIdentityMutation
            .mutate(stack, &state, &config())
            .await
            .unwrap();
        assert!(stack.resources.contains_key("api-appgw-id"));
    }

    #[tokio::test]
    async fn only_azure_with_an_https_endpoint_runs() {
        let https = stack(vec![container("api", Some(ExposeProtocol::Http))]);
        let none = stack(vec![container("db", Some(ExposeProtocol::Tcp))]);
        let mutation = AzurePublicEndpointIdentityMutation;
        assert!(!mutation.should_run(&https, &StackState::new(Platform::Aws), &config()));
        assert!(!mutation.should_run(&none, &StackState::new(Platform::Azure), &config()));
    }

    /// An installed stack without the identity asks for a setup update once the identity is
    /// part of the release.
    #[tokio::test]
    async fn an_installed_stack_without_the_identity_needs_a_setup_update() {
        let state = StackState::new(Platform::Azure);
        let installed = stack(vec![container("api", Some(ExposeProtocol::Http))]);
        let target = AzurePublicEndpointIdentityMutation
            .mutate(installed.clone(), &state, &config())
            .await
            .unwrap();
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Azure,
        }
        .check(&installed, &target)
        .await
        .unwrap();
        assert!(!result.success, "{:?}", result.errors);
        assert!(
            result.errors[0].contains("Cannot add frozen resources during update")
                && result.errors[0].contains("api-appgw-id"),
            "{:?}",
            result.errors
        );
        // Once set up, the same release is unchanged.
        let result = FrozenResourcesUnchangedCheck {
            platform: Platform::Azure,
        }
        .check(&target, &target)
        .await
        .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }
}
