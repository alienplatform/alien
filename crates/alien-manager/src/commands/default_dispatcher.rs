use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use alien_commands::{
    error::{ErrorData as CmdErrorData, Result as CmdResult},
    server::CommandDispatcher,
    Envelope,
};
use alien_core::{CommandTargetType, Platform, WorkerOutputs};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use tracing::{debug, info};

use crate::traits::{CredentialResolver, DeploymentStore};

/// Default command dispatcher for standalone alien-manager.
///
/// The command envelope already names the specific Worker the command is
/// addressed to (`envelope.target`, resolved server-side by the registry).
/// This dispatcher reads that worker's `commands_push_target` from the
/// deployment's stack state, resolves credentials for the target environment,
/// and dispatches via the platform-specific mechanism.
///
/// Only Worker targets ever reach this push dispatcher — Container/Daemon
/// targets are always Pull (per the ALIEN-219 delivery rule) and are served by
/// the pending-index poll path, never dispatched here.
pub struct DefaultCommandDispatcher {
    deployment_store: Arc<dyn DeploymentStore>,
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
        credential_resolver: Arc<dyn CredentialResolver>,
    ) -> Self {
        Self {
            deployment_store,
            credential_resolver,
            http_client: reqwest::Client::new(),
        }
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

        // 1. Get deployment record. Push dispatch runs from a `Subscriber`
        // task with no inbound caller — `Subject::system()` is the standard
        // synthetic subject for that case (empty bearer signals no
        // passthrough is available).
        let system = crate::auth::Subject::system();
        let deployment = self
            .deployment_store
            .get_deployment(&system, deployment_id)
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

        // 3. The envelope names the specific target resource. Push dispatch
        // only handles Worker targets — a Container/Daemon target here means a
        // Pull command was misrouted to the push path, which must never happen
        // (they are served by the pending-index poll path). Fail loudly.
        let target = &envelope.target;
        if target.resource_type != CommandTargetType::Worker {
            return Err(AlienError::new(CmdErrorData::OperationNotSupported {
                message: format!(
                    "Command {} targets a {:?} resource ('{}'), which uses pull delivery and \
                     cannot be push-dispatched",
                    envelope.command_id, target.resource_type, target.resource_id
                ),
                operation: Some("dispatch".to_string()),
            }));
        }
        let worker_id = &target.resource_id;

        // 4. Read the targeted worker's push target from stack state.
        let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(CmdErrorData::Other {
                message: format!("Deployment {} has no stack_state", deployment_id),
            })
        })?;

        let worker_outputs: &WorkerOutputs =
            stack_state
                .get_resource_outputs(worker_id)
                .context(CmdErrorData::Other {
                    message: format!("Failed to get worker outputs for '{}'", worker_id),
                })?;

        let push_target = worker_outputs
            .commands_push_target
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(CmdErrorData::Other {
                    message: format!(
                        "Worker '{}' has no commands_push_target in outputs",
                        worker_id
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
