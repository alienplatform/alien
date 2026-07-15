use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use alien_commands::{
    dispatchers::{
        CommandDispatcher, HttpCommandDispatcher, LambdaCommandDispatcher, PubSubCommandDispatcher,
        ServiceBusCommandDispatcher,
    },
    error::{ErrorData as CmdErrorData, Result as CmdResult},
    Envelope,
};
use alien_core::{CommandTargetType, Platform, ResourceStatus, WorkerOutputs};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tracing::{debug, info};

use crate::traits::{CredentialResolver, DeploymentStore};

const COMMAND_DISPATCH_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const COMMAND_DISPATCH_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn definitely_not_delivered(envelope: &Envelope, message: impl Into<String>) -> CmdErrorData {
    CmdErrorData::TransportDispatchRejected {
        message: message.into(),
        transport_type: None,
        target: Some(envelope.command_id.clone()),
    }
}

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
    ) -> CmdResult<Self> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(COMMAND_DISPATCH_CONNECT_TIMEOUT)
            .timeout(COMMAND_DISPATCH_REQUEST_TIMEOUT)
            .build()
            .map_err(reqwest::Error::without_url)
            .into_alien_error()
            .context(CmdErrorData::Other {
                message: "Failed to build bounded command dispatch HTTP client".to_string(),
            })?;
        Ok(Self {
            deployment_store,
            credential_resolver,
            http_client,
        })
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
            .context(definitely_not_delivered(
                envelope,
                format!("Failed to get deployment {deployment_id} before dispatch"),
            ))?
            .ok_or_else(|| {
                AlienError::new(definitely_not_delivered(
                    envelope,
                    format!("Deployment {deployment_id} not found before dispatch"),
                ))
            })?;

        // 2. The envelope names the specific target resource. Push dispatch
        // only handles Worker targets — a Container/Daemon target here means a
        // Pull command was misrouted to the push path, which must never happen
        // (they are served by the pending-index poll path). Fail loudly.
        let target = &envelope.target;
        if target.resource_type != CommandTargetType::Worker {
            return Err(AlienError::new(definitely_not_delivered(
                envelope,
                format!(
                    "Target '{}' is {:?} and cannot use push delivery",
                    target.resource_id, target.resource_type
                ),
            )));
        }
        let worker_id = &target.resource_id;

        if deployment.platform == Platform::Kubernetes {
            return Err(AlienError::new(definitely_not_delivered(
                envelope,
                "Kubernetes Worker commands must use the environment-local operator relay",
            )));
        }

        // 3. Read the targeted worker's push target from stack state.
        let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(definitely_not_delivered(
                envelope,
                format!("Deployment {deployment_id} has no stack state"),
            ))
        })?;

        let worker_state = stack_state.resources.get(worker_id).ok_or_else(|| {
            AlienError::new(definitely_not_delivered(
                envelope,
                format!("Worker '{worker_id}' is absent from stack state"),
            ))
        })?;
        if worker_state.status != ResourceStatus::Running {
            return Err(AlienError::new(definitely_not_delivered(
                envelope,
                format!(
                    "Worker '{worker_id}' is not Running (status: {:?})",
                    worker_state.status
                ),
            )));
        }

        let worker_outputs: &WorkerOutputs =
            stack_state
                .get_resource_outputs(worker_id)
                .context(definitely_not_delivered(
                    envelope,
                    format!("Failed to get Worker outputs for '{worker_id}'"),
                ))?;

        let push_target = worker_outputs
            .commands_push_target
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(definitely_not_delivered(
                    envelope,
                    format!("Worker '{worker_id}' has no command push target"),
                ))
            })?;

        if deployment.platform == Platform::Local {
            let token = deployment.deployment_token.clone().ok_or_else(|| {
                AlienError::new(definitely_not_delivered(
                    envelope,
                    format!("Deployment {deployment_id} has no command push token"),
                ))
            })?;
            let dispatcher =
                HttpCommandDispatcher::new(self.http_client.clone(), push_target.clone(), token);
            return dispatcher.dispatch(envelope).await;
        }

        // 4. Resolve credentials for the target environment.
        let client_config = self
            .credential_resolver
            .resolve(&deployment)
            .await
            .context(definitely_not_delivered(
                envelope,
                "Failed to resolve credentials before dispatch",
            ))?;

        // 5. Create platform-specific dispatcher and dispatch.
        info!(
            command_id = %envelope.command_id,
            deployment_id = %deployment_id,
            platform = ?deployment.platform,
            push_target = %push_target,
            "Dispatching command via platform-specific push"
        );

        match client_config {
            alien_core::ClientConfig::Aws(aws_config) => {
                let dispatcher = LambdaCommandDispatcher::new(
                    self.http_client.clone(),
                    *aws_config,
                    push_target.clone(),
                )
                .await
                .context(definitely_not_delivered(
                    envelope,
                    "Failed to create Lambda dispatcher before dispatch",
                ))?;
                dispatcher.dispatch(envelope).await
            }
            alien_core::ClientConfig::Gcp(gcp_config) => {
                let dispatcher = PubSubCommandDispatcher::new(
                    self.http_client.clone(),
                    *gcp_config,
                    push_target.clone(),
                );
                dispatcher.dispatch(envelope).await
            }
            alien_core::ClientConfig::Azure(azure_config) => {
                let (namespace, queue) = push_target.split_once('/').ok_or_else(|| {
                    AlienError::new(definitely_not_delivered(
                        envelope,
                        "Invalid Azure push target: expected 'namespace/queue'",
                    ))
                })?;
                let dispatcher = ServiceBusCommandDispatcher::new(
                    self.http_client.clone(),
                    *azure_config,
                    namespace.to_string(),
                    queue.to_string(),
                );
                dispatcher.dispatch(envelope).await
            }
            _ => Err(AlienError::new(definitely_not_delivered(
                envelope,
                format!(
                    "Platform {:?} does not support push dispatch",
                    deployment.platform
                ),
            ))),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
