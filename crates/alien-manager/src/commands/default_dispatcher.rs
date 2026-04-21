use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use alien_commands::{
    error::{ErrorData as CmdErrorData, Result as CmdResult},
    server::CommandDispatcher,
    Envelope,
};
use alien_core::{Function, FunctionOutputs, Platform};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use tracing::{debug, info};

use crate::traits::{CredentialResolver, DeploymentStore, ReleaseStore};

/// Default command dispatcher for standalone alien-manager.
///
/// Looks up the deployment's stack state, finds the function with `commands_enabled=true`,
/// reads its `commands_push_target` from outputs, resolves credentials for the target
/// environment, and dispatches via the platform-specific mechanism.
pub struct DefaultCommandDispatcher {
    deployment_store: Arc<dyn DeploymentStore>,
    release_store: Arc<dyn ReleaseStore>,
    credential_resolver: Arc<dyn CredentialResolver>,
    http_client: reqwest::Client,
}

impl Debug for DefaultCommandDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultCommandDispatcher")
            .finish_non_exhaustive()
    }
}

impl DefaultCommandDispatcher {
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        release_store: Arc<dyn ReleaseStore>,
        credential_resolver: Arc<dyn CredentialResolver>,
    ) -> Self {
        Self {
            deployment_store,
            release_store,
            credential_resolver,
            http_client: reqwest::Client::new(),
        }
    }

    /// Find the function with `commands_enabled=true` in the release stack for the given platform.
    fn find_commands_function(
        &self,
        release: &crate::traits::ReleaseRecord,
        platform: &Platform,
    ) -> CmdResult<String> {
        let stack = release.stacks.get(platform).ok_or_else(|| {
            AlienError::new(CmdErrorData::Other {
                message: format!(
                    "Release {} does not contain a stack for platform {}",
                    release.id, platform
                ),
            })
        })?;

        for (resource_id, entry) in stack.resources() {
            if let Some(function) = entry.config.downcast_ref::<Function>() {
                if function.commands_enabled {
                    return Ok(resource_id.clone());
                }
            }
        }
        Err(AlienError::new(CmdErrorData::Other {
            message: "No function with commands_enabled=true found in release stack".to_string(),
        }))
    }
}

#[async_trait]
impl CommandDispatcher for DefaultCommandDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> CmdResult<()> {
        let deployment_id = &envelope.deployment_id;

        debug!(
            command_id = %envelope.command_id,
            deployment_id = %deployment_id,
            command = %envelope.command,
            "DefaultCommandDispatcher: looking up deployment for push dispatch"
        );

        // 1. Get deployment record
        let deployment = self
            .deployment_store
            .get_deployment(deployment_id)
            .await
            .context(CmdErrorData::Other {
                message: format!("Failed to get deployment {}", deployment_id),
            })?
            .ok_or_else(|| {
                AlienError::new(CmdErrorData::Other {
                    message: format!("Deployment {} not found", deployment_id),
                })
            })?;

        // 2. Reject platforms that use polling (K8s/Local), not push dispatch.
        // Note: we don't check deployment_model here — the CommandServer already
        // routes Pull deployments to create_pending_index, so this dispatcher
        // only gets called for Push deployments.
        if matches!(deployment.platform, Platform::Kubernetes | Platform::Local) {
            return Err(AlienError::new(CmdErrorData::OperationNotSupported {
                message: format!(
                    "Deployment {} is on {:?} which uses polling, not push dispatch",
                    deployment_id, deployment.platform
                ),
                operation: Some("dispatch".to_string()),
            }));
        }

        // 3. Get release to find the commands-enabled function
        let release_id = deployment.current_release_id.as_ref().ok_or_else(|| {
            AlienError::new(CmdErrorData::Other {
                message: format!("Deployment {} has no current_release_id", deployment_id),
            })
        })?;

        let release = self
            .release_store
            .get_release(release_id)
            .await
            .context(CmdErrorData::Other {
                message: format!("Failed to get release {}", release_id),
            })?
            .ok_or_else(|| {
                AlienError::new(CmdErrorData::Other {
                    message: format!("Release {} not found", release_id),
                })
            })?;

        // 4. Find function with commands_enabled and get its push target from stack state
        let function_id = self.find_commands_function(&release, &deployment.platform)?;

        let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(CmdErrorData::Other {
                message: format!("Deployment {} has no stack_state", deployment_id),
            })
        })?;

        let function_outputs: &FunctionOutputs = stack_state
            .get_resource_outputs(&function_id)
            .context(CmdErrorData::Other {
                message: format!("Failed to get function outputs for '{}'", function_id),
            })?;

        let push_target = function_outputs
            .commands_push_target
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(CmdErrorData::Other {
                    message: format!(
                        "Function '{}' has no commands_push_target in outputs",
                        function_id
                    ),
                })
            })?;

        // 5. Resolve credentials for the target environment
        let client_config = self
            .credential_resolver
            .resolve(&deployment)
            .await
            .context(CmdErrorData::Other {
                message: "Failed to resolve credentials".to_string(),
            })?;

        // 6. Create platform-specific dispatcher and dispatch
        info!(
            command_id = %envelope.command_id,
            deployment_id = %deployment_id,
            platform = ?deployment.platform,
            push_target = %push_target,
            "Dispatching command via platform-specific push"
        );

        match client_config {
            alien_core::ClientConfig::Aws(aws_config) => {
                use alien_commands::server::dispatchers::LambdaCommandDispatcher;
                let dispatcher = LambdaCommandDispatcher::new(
                    self.http_client.clone(),
                    *aws_config,
                    push_target.clone(),
                )
                .await
                .context(CmdErrorData::Other {
                    message: "Failed to create Lambda dispatcher".to_string(),
                })?;
                dispatcher.dispatch(envelope).await
            }
            alien_core::ClientConfig::Gcp(gcp_config) => {
                use alien_commands::server::dispatchers::PubSubCommandDispatcher;
                let dispatcher = PubSubCommandDispatcher::new(
                    self.http_client.clone(),
                    *gcp_config,
                    push_target.clone(),
                );
                dispatcher.dispatch(envelope).await
            }
            alien_core::ClientConfig::Azure(azure_config) => {
                use alien_commands::server::dispatchers::ServiceBusCommandDispatcher;
                let (namespace, queue) = push_target.split_once('/').ok_or_else(|| {
                    AlienError::new(CmdErrorData::Other {
                        message: format!(
                            "Invalid Azure push target '{}': expected 'namespace/queue'",
                            push_target
                        ),
                    })
                })?;
                let dispatcher = ServiceBusCommandDispatcher::new(
                    self.http_client.clone(),
                    *azure_config,
                    namespace.to_string(),
                    queue.to_string(),
                );
                dispatcher.dispatch(envelope).await
            }
            _ => Err(AlienError::new(CmdErrorData::OperationNotSupported {
                message: format!(
                    "Platform {:?} does not support push dispatch",
                    deployment.platform
                ),
                operation: Some("dispatch".to_string()),
            })),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
