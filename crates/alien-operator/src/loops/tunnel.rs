//! Tunnel loop: keeps the operator's outbound tunnel connections open while
//! the manager accepts tunnels and the stack declares tunnel endpoints.
//!
//! Requests arriving on the tunnel are forwarded only to containers whose
//! stack config declares `tunnel`, at the address the container controller
//! reports (`ContainerOutputs.internal_dns`) and the declared port.

use std::{collections::HashMap, sync::Arc, time::Duration};

use alien_core::{Container, ContainerOutputs, DeploymentState};
use alien_tunnel::operator::{self, TunnelClientConfig, TunnelTargets};
use http::Uri;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use url::Url;

use crate::OperatorState;

/// How often the loop re-reads targets and the manager's tunnel URL.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// Parallel tunnel connections per operator.
const CONNECTIONS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Connection {
    manager_url: Url,
    token: String,
}

pub async fn run_tunnel_loop(state: Arc<OperatorState>) {
    let targets = TunnelTargets::new();
    let mut active: Option<(Connection, JoinHandle<()>)> = None;

    loop {
        match desired_connection(&state, &targets).await {
            Ok(desired) => {
                let current = active.as_ref().map(|(connection, _)| connection);
                if current != desired.as_ref() {
                    if let Some((_, task)) = active.take() {
                        task.abort();
                        info!("Tunnel stopped");
                    }
                    if let Some(connection) = desired {
                        info!(
                            manager_url = %connection.manager_url,
                            targets = ?targets.names(),
                            "Tunnel starting"
                        );
                        let config = TunnelClientConfig {
                            manager_url: connection.manager_url.clone(),
                            token: connection.token.clone(),
                            connections: CONNECTIONS,
                        };
                        let targets = targets.clone();
                        let task = tokio::spawn(async move {
                            if let Err(e) = operator::run(config, targets).await {
                                warn!(error = %e, "Tunnel client stopped");
                            }
                        });
                        active = Some((connection, task));
                    }
                }
            }
            Err(e) => warn!(error = %e, "Failed to refresh tunnel configuration"),
        }

        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = tokio::time::sleep(REFRESH_INTERVAL) => {}
        }
    }

    if let Some((_, task)) = active {
        task.abort();
    }
}

/// Refresh the target table and return the connection the operator should
/// hold, or `None` when it should hold none.
async fn desired_connection(
    state: &OperatorState,
    targets: &TunnelTargets,
) -> crate::error::Result<Option<Connection>> {
    let resolved = match state.db.get_deployment_state().await? {
        Some(deployment) => tunnel_targets(&deployment),
        None => HashMap::new(),
    };
    let has_targets = !resolved.is_empty();
    targets.replace(resolved);

    if !state.config.tunnel_enabled || !has_targets {
        return Ok(None);
    }
    let Some(manager_url) = state.db.get_tunnel_url().await? else {
        return Ok(None);
    };
    let Some(token) = state.db.get_sync_token().await? else {
        return Ok(None);
    };
    let manager_url = match Url::parse(&manager_url) {
        Ok(url) => url,
        Err(e) => {
            warn!(url = %manager_url, error = %e, "Manager advertised an invalid tunnel URL");
            return Ok(None);
        }
    };
    Ok(Some(Connection { manager_url, token }))
}

/// Tunnel endpoints declared by the deployed stack, keyed by container ID.
fn tunnel_targets(deployment: &DeploymentState) -> HashMap<String, Uri> {
    let Some(stack) = deployment.stack_state.as_ref() else {
        return HashMap::new();
    };
    stack
        .resources
        .iter()
        .filter_map(|(resource_id, resource)| {
            let tunnel = resource
                .config
                .downcast_ref::<Container>()?
                .tunnel
                .as_ref()?;
            let outputs = resource
                .outputs
                .as_ref()?
                .downcast_ref::<ContainerOutputs>()?;
            let uri = format!("http://{}:{}", outputs.internal_dns, tunnel.port)
                .parse::<Uri>()
                .map_err(|e| {
                    warn!(resource_id = %resource_id, error = %e, "Invalid tunnel target address");
                })
                .ok()?;
            Some((resource_id.clone(), uri))
        })
        .collect()
}
