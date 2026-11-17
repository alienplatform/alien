//! The Azure egress gateway's `Container` declaration.
//!
//! Azure has no managed container backend. The gateway is two VMs that setup creates, and its
//! `Container` in the stack only names the image and ports setup renders. Nothing here deploys
//! anything: the declaration reaches Running at once so resources ordered after it proceed, and
//! it is never read back from the cloud. Gateway health is judged by the sandbox's own
//! network checks and the startup gate.

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, Container, ResourceStatus};
use alien_error::AlienError;
use alien_macros::controller;

/// Only the gateway declaration is a no-op. Any other Container on Azure would have nothing
/// deploying it, and reporting it Running would hide that.
fn require_gateway(ctx: &ResourceControllerContext<'_>) -> Result<()> {
    let config = ctx.desired_resource_config::<Container>()?;
    if config.id != SANDBOX_EGRESS_GATEWAY_ID {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "Container '{}' cannot run on Azure: only the managed sandbox egress gateway is a Container there, and setup creates it",
                config.id
            ),
            resource_id: Some(config.id.clone()),
        }));
    }
    Ok(())
}

/// Ports the gateway VMs serve: the sandbox proxy and the private policy administration port.
const PROXY_PORT: u16 = 3128;
const ADMIN_PORT: u16 = 3129;

/// The gateway VM addresses setup recorded on the deployment Network, empty when setup predates
/// them or the stack has no Network. Only the Internet sandboxes' own checks decide whether that
/// is an error, so the declaration records nothing rather than failing here.
fn setup_gateway_ips(ctx: &ResourceControllerContext<'_>) -> Result<Vec<String>> {
    use crate::core::state_utils::StackResourceStateExt;
    let Some(network_id) = ctx.desired_stack.resources().find_map(|(id, entry)| {
        entry
            .config
            .downcast_ref::<alien_core::Network>()
            .map(|_| id.to_string())
    }) else {
        return Ok(Vec::new());
    };
    let Some(state) = ctx.state.resources.get(&network_id) else {
        return Ok(Vec::new());
    };
    let network =
        state.get_internal_controller_typed::<crate::network::AzureNetworkController>()?;
    Ok(network
        .sandbox_network
        .map(|declared| declared.gateway_ips)
        .unwrap_or_default())
}

#[controller]
pub struct AzureSandboxGatewayDeclarationController {
    /// Static addresses of the setup-created gateway VMs. The first one is the proxy and admin
    /// address the Container binding names, as every replica serves both ports.
    #[serde(default)]
    pub(crate) gateway_ips: Vec<String>,
}

#[controller]
impl AzureSandboxGatewayDeclarationController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        require_gateway(ctx)?;
        self.gateway_ips = setup_gateway_ips(ctx)?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        require_gateway(ctx)?;
        self.gateway_ips = setup_gateway_ips(ctx)?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(60)),
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<alien_core::ResourceOutputs> {
        None
    }

    /// The sandbox binding and every consumer of the sandbox load the gateway as a linked
    /// Container. Azure has no managed container backend to report its URLs, so they come from
    /// the setup-created VM addresses.
    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{ContainerBinding, HorizonContainerBinding};
        use alien_core::BindingValue;
        use alien_error::{Context, IntoAlienError};

        let Some(address) = self.gateway_ips.first() else {
            return Ok(None);
        };
        let url = |port: u16| BindingValue::value(format!("http://{address}:{port}"));
        let binding = ContainerBinding::Horizon(HorizonContainerBinding {
            container_name: BindingValue::value(SANDBOX_EGRESS_GATEWAY_ID.to_string()),
            internal_url: url(PROXY_PORT),
            public_url: None,
            internal_urls: [(PROXY_PORT, url(PROXY_PORT)), (ADMIN_PORT, url(ADMIN_PORT))]
                .into_iter()
                .collect(),
        });
        serde_json::to_value(binding)
            .into_alien_error()
            .context(ErrorData::ResourceStateSerializationFailed {
                resource_id: SANDBOX_EGRESS_GATEWAY_ID.to_string(),
                message: "Failed to serialize binding parameters".to_string(),
            })
            .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ResourceController;
    use alien_azure_clients::AzureClientConfigExt;
    use alien_core::{
        sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, ClientConfig, Container, ContainerCode,
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceLifecycle, ResourceSpec, ResourceStatus, Stack, StackSettings, StackState,
    };

    use crate::StackExecutor;

    fn stack(id: &str, image: &str) -> Stack {
        let container = Container::new(id.into())
            .code(ContainerCode::Image {
                image: image.into(),
            })
            .cpu(ResourceSpec {
                min: "0.25".into(),
                desired: "0.25".into(),
            })
            .memory(ResourceSpec {
                min: "512Mi".into(),
                desired: "512Mi".into(),
            })
            .port(3129)
            .permissions(id.into())
            .build();
        Stack::new("gw".to_string())
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    fn executor(stack: &Stack, config: &DeploymentConfig) -> StackExecutor {
        StackExecutor::builder(
            stack,
            ClientConfig::Azure(Box::new(alien_azure_clients::AzureClientConfig::mock())),
        )
        .deployment_config(config)
        .build()
        .expect("a Container has a controller on Azure")
    }

    async fn steps(executor: &StackExecutor, mut state: StackState, n: usize) -> StackState {
        for _ in 0..n {
            state = executor.step(state).await.unwrap().next_state;
        }
        state
    }

    /// Preparation declares the gateway as a Container on Azure. The runtime executor must
    /// accept that stack and run the declaration to Running without a cloud call.
    #[tokio::test]
    async fn the_executor_accepts_the_gateway_declaration_on_azure() {
        let config = config();
        let stack = stack(
            SANDBOX_EGRESS_GATEWAY_ID,
            "example.invalid/gateway@sha256:0",
        );
        let state = steps(
            &executor(&stack, &config),
            StackState::new(Platform::Azure),
            4,
        )
        .await;
        assert_eq!(
            state.resources[SANDBOX_EGRESS_GATEWAY_ID].status,
            ResourceStatus::Running
        );
    }

    /// Nothing deploys another Container on Azure, so it must fail rather than look Running.
    #[tokio::test]
    async fn another_container_on_azure_fails_and_never_runs() {
        let config = config();
        let stack = stack("worker-api", "example.invalid/app:1");
        let state = steps(
            &executor(&stack, &config),
            StackState::new(Platform::Azure),
            4,
        )
        .await;
        let status = state.resources["worker-api"].status;
        assert_ne!(status, ResourceStatus::Running);
        assert_eq!(status, ResourceStatus::ProvisionFailed);
    }

    /// A new gateway image updates the declaration without a cloud call.
    #[tokio::test]
    async fn updating_the_gateway_declaration_makes_no_cloud_call() {
        let config = config();
        let first = stack(
            SANDBOX_EGRESS_GATEWAY_ID,
            "example.invalid/gateway@sha256:0",
        );
        let state = steps(
            &executor(&first, &config),
            StackState::new(Platform::Azure),
            4,
        )
        .await;
        let second = stack(
            SANDBOX_EGRESS_GATEWAY_ID,
            "example.invalid/gateway@sha256:1",
        );
        let state = steps(&executor(&second, &config), state, 4).await;
        let resource = &state.resources[SANDBOX_EGRESS_GATEWAY_ID];
        assert_eq!(resource.status, ResourceStatus::Running);
        assert_eq!(
            resource.config,
            second.resources[SANDBOX_EGRESS_GATEWAY_ID].config
        );
    }

    /// Removing the gateway deletes the declaration without a cloud call.
    #[tokio::test]
    async fn deleting_the_gateway_declaration_makes_no_cloud_call() {
        let config = config();
        let stack = stack(
            SANDBOX_EGRESS_GATEWAY_ID,
            "example.invalid/gateway@sha256:0",
        );
        let mut state = steps(
            &executor(&stack, &config),
            StackState::new(Platform::Azure),
            4,
        )
        .await;
        let empty = Stack::new("gw".to_string()).build();
        let remover = executor(&empty, &config);
        remover.prepare_for_deletion(&mut state).unwrap();
        let state = steps(&remover, state, 4).await;
        assert_eq!(
            state.resources[SANDBOX_EGRESS_GATEWAY_ID].status,
            ResourceStatus::Deleted
        );
    }

    fn gateway() -> Container {
        Container::new(SANDBOX_EGRESS_GATEWAY_ID.into())
            .code(ContainerCode::Image {
                image: "example.invalid/gateway@sha256:0".into(),
            })
            .cpu(ResourceSpec {
                min: "0.25".into(),
                desired: "0.25".into(),
            })
            .memory(ResourceSpec {
                min: "512Mi".into(),
                desired: "512Mi".into(),
            })
            .port(3128)
            .port(3129)
            .permissions(SANDBOX_EGRESS_GATEWAY_ID.into())
            .build()
    }

    async fn run_with_network(
        network: crate::network::AzureNetworkController,
    ) -> crate::core::controller_test::SingleControllerExecutor {
        let mut run = crate::core::controller_test::SingleControllerExecutor::builder()
            .resource(gateway())
            .controller(AzureSandboxGatewayDeclarationController::default())
            .platform(Platform::Azure)
            .resource_lifecycle(ResourceLifecycle::Live)
            .with_dependency(
                alien_core::Network::new("network".into())
                    .settings(alien_core::NetworkSettings::Create {
                        cidr: Some("10.0.0.0/16".into()),
                        availability_zones: 2,
                    })
                    .build(),
                network,
            )
            .build()
            .await
            .unwrap();
        run.run_until_status(ResourceStatus::Running).await.unwrap();
        run
    }

    /// The sandbox binding loads the gateway as a linked Container binding, and the sandbox's
    /// consumers receive it the same way. Azure has no managed container backend, so the
    /// declaration must publish the binding itself from the setup-created gateway VM addresses.
    #[tokio::test]
    async fn the_declaration_publishes_a_container_binding_for_the_setup_gateway_vms() {
        let (network, ..) = crate::network::azure_sandbox::tests::fixture();
        let run = run_with_network(network).await;
        let controller = run
            .internal_state::<AzureSandboxGatewayDeclarationController>()
            .unwrap();
        let binding = controller
            .get_binding_params()
            .unwrap()
            .expect("a binding for the linked gateway Container");
        let parsed: alien_core::bindings::ContainerBinding =
            serde_json::from_value(binding).unwrap();
        let alien_core::bindings::ContainerBinding::Horizon(horizon) = parsed else {
            panic!("the Azure gateway binding is a plain internal-URL container binding");
        };
        let url = |port: u16| match horizon.internal_urls.get(&port) {
            Some(alien_core::BindingValue::Value(url)) => url.clone(),
            other => panic!("port {port}: {other:?}"),
        };
        assert_eq!(url(3128), "http://10.0.6.4:3128");
        assert_eq!(url(3129), "http://10.0.6.4:3129");
    }

    /// A deployment whose setup predates the gateway VMs has no addresses to hand over; the
    /// declaration must not invent a binding, so the sandbox refuses to start.
    #[tokio::test]
    async fn without_gateway_vm_addresses_there_is_no_binding() {
        let (mut network, ..) = crate::network::azure_sandbox::tests::fixture();
        network
            .sandbox_network
            .as_mut()
            .unwrap()
            .gateway_ips
            .clear();
        let run = run_with_network(network).await;
        let controller = run
            .internal_state::<AzureSandboxGatewayDeclarationController>()
            .unwrap();
        assert!(controller.get_binding_params().unwrap().is_none());
    }
}
