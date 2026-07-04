//! Commands dispatch loop - polls for pending commands and dispatches to functions
//!
//! For pull-model deployments on cloud platforms (AWS, GCP, Azure), serverless
//! functions receive commands via platform-native push (InvokeFunction, Pub/Sub,
//! Service Bus). The manager creates a per-target pending index for these
//! commands, and this loop polls the manager's lease API — once per push-capable
//! target — to pick them up and dispatch.
//!
//! This loop only runs for cloud function platforms. K8s/Local deployments use
//! runtime-level polling instead (via ALIEN_COMMANDS_POLLING_* env vars).

use crate::AgentState;
use alien_commands::dispatchers::{
    CommandDispatcher, LambdaCommandDispatcher, PubSubCommandDispatcher,
    ServiceBusCommandDispatcher,
};
use alien_commands::{CommandTarget, CommandTargetType, LeaseRequest, LeaseResponse};
use alien_core::{ClientConfig, Platform, StackState, WorkerOutputs};
use alien_infra::ClientConfigExt;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// A push-capable command target discovered in stack state: a Worker resource
/// whose `WorkerOutputs.commands_push_target` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PushTarget {
    /// The Worker's resource id within the deployment's stack.
    resource_id: String,
    /// The platform-specific push target string (Lambda name, Pub/Sub topic, …).
    push_target: String,
}

/// A dispatcher cached for a specific target, tagged with the `push_target`
/// string it was built for so we can detect when it needs rebuilding.
struct CachedDispatcher<D> {
    push_target: String,
    dispatcher: D,
}

/// Per-target dispatcher cache, keyed by the target's resource id.
type DispatcherCache = HashMap<String, CachedDispatcher<Box<dyn CommandDispatcher>>>;

/// Run the commands dispatch loop.
///
/// Each tick, enumerates every push-capable target in stack state and leases
/// commands for each in turn (see [`poll_and_dispatch`]). Dispatchers are cached
/// per target and rebuilt only when a target's push target changes.
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

    // Per-target dispatcher cache, keyed by target resource id.
    let mut dispatcher_cache: DispatcherCache = HashMap::new();

    loop {
        match poll_and_dispatch(&state, &client, &mut dispatcher_cache).await {
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

/// Poll the manager's lease API for every push-capable target and dispatch any
/// leased commands.
///
/// Returns the total number of successfully dispatched commands across targets.
///
/// **Starvation / fairness:** targets are enumerated in a stable (resource-id
/// sorted) order and leased sequentially — one lease request per target per
/// tick, each bounded by `max_leases`. A slow or erroring target only affects
/// its own iteration: per-target lease/dispatch failures are logged and skipped
/// so the remaining targets in the same tick still make progress. A single
/// sequential round per tick is deliberately simple; no cross-tick scheduling
/// state is kept.
async fn poll_and_dispatch(
    state: &AgentState,
    client: &Client,
    dispatcher_cache: &mut DispatcherCache,
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

    // 3. Get deployment state to find the push targets
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

    // 4. Enumerate every push-capable target from stack state.
    let targets = enumerate_push_targets(stack_state);
    if targets.is_empty() {
        debug!("No push-capable command targets in stack state");
        // Drop any stale cached dispatchers.
        dispatcher_cache.clear();
        return Ok(0);
    }

    // Prune cached dispatchers for targets that no longer exist.
    let live_ids: std::collections::HashSet<&str> =
        targets.iter().map(|t| t.resource_id.as_str()).collect();
    dispatcher_cache.retain(|id, _| live_ids.contains(id.as_str()));

    let lease_url = format!("{}/commands/leases", commands_url.trim_end_matches('/'));
    let platform = state.config.platform;

    // 5. Lease + dispatch for each target in turn.
    let mut dispatched = 0;
    for target in &targets {
        // Ensure a dispatcher for this target (create or reuse cached) and take
        // the reference to it directly — no second lookup, so no `.expect`.
        let push_target = target.push_target.clone();
        let config = &state.config;
        let (cached, rebuilt) = match ensure_dispatcher(
            dispatcher_cache,
            &target.resource_id,
            &target.push_target,
            || async { create_dispatcher(platform, &push_target, config).await },
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                warn!(
                    resource_id = %target.resource_id,
                    error = %e,
                    "Failed to create dispatcher for target, skipping this tick"
                );
                continue;
            }
        };
        if rebuilt {
            debug!(resource_id = %target.resource_id, "Rebuilt command dispatcher");
        }

        match lease_and_dispatch_target(
            client,
            &lease_url,
            &deployment_id,
            target,
            cached.dispatcher.as_ref(),
        )
        .await
        {
            Ok(n) => dispatched += n,
            Err(e) => {
                warn!(
                    resource_id = %target.resource_id,
                    error = %e,
                    "Lease/dispatch failed for target, continuing with other targets"
                );
            }
        }
    }

    Ok(dispatched)
}

/// Acquire leases for a single target and dispatch each leased command via that
/// target's cached dispatcher. Returns the number dispatched for this target.
async fn lease_and_dispatch_target(
    client: &Client,
    lease_url: &str,
    deployment_id: &str,
    target: &PushTarget,
    dispatcher: &dyn CommandDispatcher,
) -> Result<usize, String> {
    // ALIEN-219: `LeaseRequest.target` names the specific push-capable Worker
    // this lease is for; the manager scans only that target's pending index.
    let lease_request = LeaseRequest {
        target: CommandTarget::new(target.resource_id.clone(), CommandTargetType::Worker),
        deployment_id: deployment_id.to_string(),
        max_leases: 10,
        lease_seconds: 60,
    };

    let response = client
        .post(lease_url)
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

    let mut dispatched = 0;
    for lease in &lease_response.leases {
        match dispatcher.dispatch(&lease.envelope).await {
            Ok(()) => {
                info!(
                    resource_id = %target.resource_id,
                    command_id = %lease.command_id,
                    command = %lease.envelope.command,
                    "Dispatched command to function"
                );
                dispatched += 1;
            }
            Err(e) => {
                error!(
                    resource_id = %target.resource_id,
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

/// Enumerate every push-capable command target in stack state.
///
/// A target is any resource whose outputs are `WorkerOutputs` with
/// `commands_push_target` set — i.e. a Worker provisioned with
/// `commands_enabled: true` on a platform that supports push. Results are sorted
/// by resource id so enumeration (and the per-tick lease round) is deterministic
/// and fair regardless of the underlying `HashMap` iteration order.
fn enumerate_push_targets(stack_state: &StackState) -> Vec<PushTarget> {
    let mut targets: Vec<PushTarget> = stack_state
        .resources
        .iter()
        .filter_map(|(resource_id, resource_state)| {
            let outputs = resource_state.outputs.as_ref()?;
            let worker_outputs = outputs.downcast_ref::<WorkerOutputs>()?;
            let push_target = worker_outputs.commands_push_target.clone()?;
            Some(PushTarget {
                resource_id: resource_id.clone(),
                push_target,
            })
        })
        .collect();
    targets.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    targets
}

/// Ensure the cache holds a dispatcher for `resource_id` built for the current
/// `push_target`, and return a reference to it. Reuses an existing entry when
/// the push target is unchanged; otherwise builds a new dispatcher via
/// `factory` and replaces it.
///
/// Returns `(&cached, rebuilt)` where `rebuilt` is `true` if a (re)build
/// happened. The reference is produced through the `Entry` API, so the caller
/// gets the dispatcher without a second fallible lookup — the removed `.expect`.
/// Generic over the dispatcher type so the caching logic is unit-testable
/// without constructing a real platform dispatcher.
async fn ensure_dispatcher<'c, D, F, Fut>(
    cache: &'c mut HashMap<String, CachedDispatcher<D>>,
    resource_id: &str,
    push_target: &str,
    factory: F,
) -> Result<(&'c CachedDispatcher<D>, bool), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<D, String>>,
{
    use std::collections::hash_map::Entry;
    match cache.entry(resource_id.to_string()) {
        Entry::Occupied(mut e) => {
            let rebuilt = e.get().push_target != push_target;
            if rebuilt {
                let dispatcher = factory().await?;
                e.insert(CachedDispatcher {
                    push_target: push_target.to_string(),
                    dispatcher,
                });
            }
            let cached: &CachedDispatcher<D> = e.into_mut();
            Ok((cached, rebuilt))
        }
        Entry::Vacant(e) => {
            let dispatcher = factory().await?;
            let cached: &CachedDispatcher<D> = e.insert(CachedDispatcher {
                push_target: push_target.to_string(),
                dispatcher,
            });
            Ok((cached, true))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Platform, Resource, ResourceOutputs, ResourceStatus, StackResourceState, Worker,
        WorkerCode, WorkerOutputs,
    };
    use std::collections::HashMap;

    /// Build a Worker resource state with the given optional push target.
    fn worker_state(name: &str, push_target: Option<&str>) -> StackResourceState {
        let worker = Worker::new(name.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();

        let outputs = WorkerOutputs {
            worker_name: name.to_string(),
            public_endpoints: HashMap::new(),
            identifier: None,
            commands_push_target: push_target.map(|s| s.to_string()),
        };

        StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(worker),
            None,
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(outputs));
        })
    }

    /// A Worker resource state that has no outputs yet (still provisioning).
    fn worker_state_no_outputs(name: &str) -> StackResourceState {
        let worker = Worker::new(name.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test-profile".to_string())
            .build();
        StackResourceState::new_pending(
            "worker".to_string(),
            Resource::new(worker),
            None,
            Vec::new(),
        )
    }

    #[test]
    fn enumerate_collects_all_workers_with_push_target_sorted() {
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "worker-b".to_string(),
            worker_state("worker-b", Some("lambda-b")),
        );
        stack_state.resources.insert(
            "worker-a".to_string(),
            worker_state("worker-a", Some("lambda-a")),
        );
        // Worker with commands disabled → no push target → excluded.
        stack_state
            .resources
            .insert("worker-c".to_string(), worker_state("worker-c", None));
        // Worker still provisioning (no outputs) → excluded.
        stack_state
            .resources
            .insert("worker-d".to_string(), worker_state_no_outputs("worker-d"));

        let targets = enumerate_push_targets(&stack_state);

        assert_eq!(
            targets,
            vec![
                PushTarget {
                    resource_id: "worker-a".to_string(),
                    push_target: "lambda-a".to_string(),
                },
                PushTarget {
                    resource_id: "worker-b".to_string(),
                    push_target: "lambda-b".to_string(),
                },
            ],
            "only workers with a push target, sorted by resource id"
        );
    }

    #[test]
    fn enumerate_empty_when_no_push_targets() {
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state
            .resources
            .insert("worker-a".to_string(), worker_state("worker-a", None));
        stack_state
            .resources
            .insert("worker-b".to_string(), worker_state_no_outputs("worker-b"));

        assert!(enumerate_push_targets(&stack_state).is_empty());
    }

    #[tokio::test]
    async fn ensure_dispatcher_caches_per_target_and_rebuilds_on_change() {
        // Use a plain counter as the "dispatcher" so we can assert build counts
        // without constructing a real platform dispatcher.
        let mut cache: HashMap<String, CachedDispatcher<u32>> = HashMap::new();
        let builds = std::cell::Cell::new(0u32);
        let bump = || async {
            builds.set(builds.get() + 1);
            Ok::<u32, String>(builds.get())
        };

        // First build for worker-a: returns the freshly built dispatcher ref.
        let (cached, rebuilt) = ensure_dispatcher(&mut cache, "worker-a", "lambda-a", bump)
            .await
            .unwrap();
        assert!(rebuilt);
        assert_eq!(cached.dispatcher, 1);
        assert_eq!(builds.get(), 1);

        // Same push target → reused, no rebuild, same dispatcher ref returned.
        let (cached, rebuilt) = ensure_dispatcher(&mut cache, "worker-a", "lambda-a", bump)
            .await
            .unwrap();
        assert!(!rebuilt);
        assert_eq!(cached.dispatcher, 1);
        assert_eq!(builds.get(), 1);

        // Changed push target → rebuild.
        let (cached, rebuilt) = ensure_dispatcher(&mut cache, "worker-a", "lambda-a-v2", bump)
            .await
            .unwrap();
        assert!(rebuilt);
        assert_eq!(cached.push_target, "lambda-a-v2");
        assert_eq!(builds.get(), 2);

        // Different target → independent cache entry.
        let (_, rebuilt) = ensure_dispatcher(&mut cache, "worker-b", "lambda-b", bump)
            .await
            .unwrap();
        assert!(rebuilt);
        assert_eq!(builds.get(), 3);
        assert_eq!(cache.len(), 2);
        // worker-a entry untouched by worker-b build.
        let (_, rebuilt) = ensure_dispatcher(&mut cache, "worker-a", "lambda-a-v2", bump)
            .await
            .unwrap();
        assert!(!rebuilt);
        assert_eq!(builds.get(), 3);
    }
}
