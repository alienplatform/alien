//! Commands dispatch loop - polls for pending commands and dispatches to functions
//!
//! For pull-model deployments on cloud platforms (AWS, GCP, Azure), serverless
//! functions receive commands via platform-native push (InvokeFunction, Pub/Sub,
//! Service Bus). The manager creates a pending index for these commands, and
//! this loop polls the manager's lease API to pick them up and dispatch.
//!
//! This loop only runs for cloud function platforms. K8s/Local deployments use
//! runtime-level polling instead (via ALIEN_COMMANDS_POLLING_* env vars).

use crate::AgentState;
use alien_commands::dispatchers::{
    CommandDispatcher, LambdaCommandDispatcher, PubSubCommandDispatcher,
    ServiceBusCommandDispatcher,
};
use alien_commands::{LeaseRequest, LeaseResponse};
use alien_core::{ClientConfig, FunctionOutputs, Platform};
use alien_infra::ClientConfigExt;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Run the commands dispatch loop.
///
/// Polls the manager's lease API for pending commands and dispatches them
/// to the deployed function via platform-native push mechanisms.
pub async fn run_commands_loop(state: Arc<AgentState>) {
    let interval = Duration::from_secs(state.config.commands_interval_seconds);

    let sync_config = match &state.config.sync {
        Some(config) => config,
        None => {
            error!("Sync configuration not provided, commands loop cannot run");
            return;
        }
    };

    if !matches!(
        state.config.platform,
        Platform::Aws | Platform::Gcp | Platform::Azure
    ) {
        debug!(
            platform = ?state.config.platform,
            "Commands dispatch loop not needed for this platform (uses runtime polling)"
        );
        return;
    }

    // Create authenticated client with the agent's sync token
    let client = match create_commands_client(&sync_config.token) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create commands HTTP client");
            return;
        }
    };

    info!(
        interval_seconds = state.config.commands_interval_seconds,
        platform = ?state.config.platform,
        "Starting commands dispatch loop"
    );

    // Cache: last known push_target and its dispatcher
    let mut cached_push_target: Option<String> = None;
    let mut cached_dispatcher: Option<Box<dyn CommandDispatcher>> = None;

    loop {
        match poll_and_dispatch(
            &state,
            &client,
            &mut cached_push_target,
            &mut cached_dispatcher,
        )
        .await
        {
            Ok(0) => {
                debug!("No pending commands");
            }
            Ok(n) => {
                info!(dispatched = n, "Dispatched commands");
            }
            Err(e) => {
                warn!(error = %e, "Commands poll/dispatch failed");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Commands dispatch loop shutting down");
                return;
            }
        }
    }
}

/// Poll the manager's lease API and dispatch any leased commands.
///
/// Returns the number of successfully dispatched commands.
async fn poll_and_dispatch(
    state: &AgentState,
    client: &Client,
    cached_push_target: &mut Option<String>,
    cached_dispatcher: &mut Option<Box<dyn CommandDispatcher>>,
) -> Result<usize, String> {
    // 1. Get deployment_id
    let deployment_id = state
        .db
        .get_deployment_id()
        .await
        .map_err(|e| format!("Failed to get deployment_id: {}", e))?
        .ok_or_else(|| "No deployment_id yet".to_string())?;

    // 2. Get commands URL (set by sync loop from manager response)
    let commands_url = state
        .db
        .get_commands_url()
        .await
        .map_err(|e| format!("Failed to get commands_url: {}", e))?
        .ok_or_else(|| "No commands_url yet".to_string())?;

    // 3. Get deployment state to find the push target
    let deployment_state = state
        .db
        .get_deployment_state()
        .await
        .map_err(|e| format!("Failed to get deployment_state: {}", e))?
        .ok_or_else(|| "No deployment_state yet".to_string())?;

    let stack_state = deployment_state
        .stack_state
        .as_ref()
        .ok_or_else(|| "No stack_state yet (deployment not ready)".to_string())?;

    // 4. Find the commands push target from stack state
    let push_target = find_push_target(stack_state)
        .ok_or_else(|| "No commands_push_target in stack state".to_string())?;

    // 5. Ensure we have a dispatcher (create or reuse cached)
    if cached_push_target.as_deref() != Some(&push_target) {
        let dispatcher = create_dispatcher(state.config.platform, &push_target, &state.config)
            .await
            .map_err(|e| format!("Failed to create dispatcher: {}", e))?;
        *cached_push_target = Some(push_target.clone());
        *cached_dispatcher = Some(dispatcher);
    }

    let dispatcher = cached_dispatcher
        .as_ref()
        .expect("dispatcher should be cached after creation");

    // 6. Acquire leases from the manager
    let lease_url = format!("{}/commands/leases", commands_url.trim_end_matches('/'));
    let lease_request = LeaseRequest {
        deployment_id,
        max_leases: 10,
        lease_seconds: 60,
    };

    let response = client
        .post(&lease_url)
        .json(&lease_request)
        .send()
        .await
        .map_err(|e| format!("Lease request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Lease request returned {}: {}", status, body));
    }

    let lease_response: LeaseResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse lease response: {}", e))?;

    if lease_response.leases.is_empty() {
        return Ok(0);
    }

    // 7. Dispatch each leased command
    let mut dispatched = 0;
    for lease in &lease_response.leases {
        match dispatcher.dispatch(&lease.envelope).await {
            Ok(()) => {
                info!(
                    command_id = %lease.command_id,
                    command = %lease.envelope.command,
                    "Dispatched command to function"
                );
                dispatched += 1;
            }
            Err(e) => {
                error!(
                    command_id = %lease.command_id,
                    command = %lease.envelope.command,
                    error = %e,
                    "Failed to dispatch command (lease will expire and retry)"
                );
            }
        }
    }

    Ok(dispatched)
}

/// Find the commands push target from stack state.
///
/// Iterates all resources looking for a Function with `commands_push_target` set.
/// Only functions with `commands_enabled: true` get a push target during provisioning.
fn find_push_target(stack_state: &alien_core::StackState) -> Option<String> {
    for (_resource_id, resource_state) in &stack_state.resources {
        if let Some(ref outputs) = resource_state.outputs {
            if let Some(function_outputs) = outputs.downcast_ref::<FunctionOutputs>() {
                if let Some(ref push_target) = function_outputs.commands_push_target {
                    return Some(push_target.clone());
                }
            }
        }
    }
    None
}

/// Create a platform-specific command dispatcher.
async fn create_dispatcher(
    platform: Platform,
    push_target: &str,
    _config: &crate::config::AgentConfig,
) -> Result<Box<dyn CommandDispatcher>, String> {
    let http_client = Client::new();

    let client_config = ClientConfig::from_std_env(platform)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {}", e))?;

    match client_config {
        ClientConfig::Aws(aws_config) => {
            let dispatcher =
                LambdaCommandDispatcher::new(http_client, *aws_config, push_target.to_string())
                    .await
                    .map_err(|e| format!("Failed to create Lambda dispatcher: {}", e))?;
            Ok(Box::new(dispatcher))
        }
        ClientConfig::Gcp(gcp_config) => {
            let dispatcher =
                PubSubCommandDispatcher::new(http_client, *gcp_config, push_target.to_string());
            Ok(Box::new(dispatcher))
        }
        ClientConfig::Azure(azure_config) => {
            let (namespace, queue) = push_target.split_once('/').ok_or_else(|| {
                format!(
                    "Invalid Azure push target '{}': expected 'namespace/queue'",
                    push_target
                )
            })?;
            let dispatcher = ServiceBusCommandDispatcher::new(
                http_client,
                *azure_config,
                namespace.to_string(),
                queue.to_string(),
            );
            Ok(Box::new(dispatcher))
        }
        _ => Err(format!(
            "Platform {:?} does not support command dispatch",
            platform
        )),
    }
}

/// Create an authenticated HTTP client for the commands lease API.
fn create_commands_client(token: &str) -> Result<Client, String> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    let auth_value = format!("Bearer {}", token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value).map_err(|e| format!("Invalid auth token: {}", e))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-agent"));

    Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}
