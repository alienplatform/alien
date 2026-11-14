//! The Azure egress gateway's `Container` declaration.
//!
//! Azure has no managed container backend. The gateway is two VMs that setup creates, and its
//! `Container` in the stack only names the image and ports setup renders. Nothing here deploys
//! anything: the declaration reaches Running at once so resources ordered after it proceed, and
//! it is never read back from the cloud. Gateway health is judged by the sandbox's own
//! network checks and the startup gate.

use crate::core::ResourceControllerContext;
use crate::error::Result;
use alien_core::{Container, ResourceStatus};
use alien_macros::controller;

#[controller]
pub struct AzureSandboxGatewayDeclarationController {}

#[controller]
impl AzureSandboxGatewayDeclarationController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        ctx.desired_resource_config::<Container>()?;
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
        ctx.desired_resource_config::<Container>()?;
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
}

#[cfg(test)]
mod tests {
    use alien_core::{
        sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, ClientConfig, Container, ContainerCode,
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
        ResourceLifecycle, ResourceSpec, ResourceStatus, StackSettings, StackState,
    };

    use crate::StackExecutor;
    use alien_azure_clients::AzureClientConfigExt;

    /// Preparation declares the gateway as a Container on Azure. The runtime executor must
    /// accept that stack and run the declaration to Running without a cloud call.
    #[tokio::test]
    async fn the_executor_accepts_the_gateway_declaration_on_azure() {
        let gateway = Container::new(SANDBOX_EGRESS_GATEWAY_ID.into())
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
            .port(3129)
            .permissions(SANDBOX_EGRESS_GATEWAY_ID.into())
            .build();
        let stack = alien_core::Stack::new("gw".to_string())
            .add(gateway, ResourceLifecycle::Live)
            .build();
        let deployment_config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let executor = StackExecutor::builder(
            &stack,
            ClientConfig::Azure(Box::new(alien_azure_clients::AzureClientConfig::mock())),
        )
        .deployment_config(&deployment_config)
        .build()
        .expect("the Azure gateway declaration has a controller");

        let mut state = StackState::new(Platform::Azure);
        for _ in 0..4 {
            state = executor.step(state).await.unwrap().next_state;
        }
        assert_eq!(
            state.resources[SANDBOX_EGRESS_GATEWAY_ID].status,
            ResourceStatus::Running
        );
    }
}
