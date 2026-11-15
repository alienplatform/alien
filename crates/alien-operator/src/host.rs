//! The machine or Docker host an operator runs on: the Docker Engine client
//! shared by log collection and inventory.

use alien_error::{Context, IntoAlienError};
use bollard::Docker;

use crate::error::{ErrorData, Result};

const DEFAULT_DOCKER_SOCKET: &str = "/var/run/docker.sock";

/// The Docker socket path: `OPERATOR_DOCKER_SOCKET` or the standard path.
pub fn docker_socket_path() -> String {
    std::env::var("OPERATOR_DOCKER_SOCKET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DOCKER_SOCKET.to_string())
}

/// A Docker Engine client on the socket. Cheap; built per use so a daemon
/// restart needs no reconnect logic.
#[cfg(unix)]
pub fn docker_client() -> Result<Docker> {
    Docker::connect_with_unix(&docker_socket_path(), 30, bollard::API_DEFAULT_VERSION)
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "could not open the Docker socket".to_string(),
        })
}

/// Host logs and host inventory read a Unix socket, journald and systemd. On
/// other platforms they are not available.
#[cfg(not(unix))]
pub fn docker_client() -> Result<Docker> {
    Err(alien_error::AlienError::new(ErrorData::PodLogReadFailed {
        message: "Docker host collection is only supported on Unix platforms".to_string(),
    }))
}

/// Whether host logs and host inventory can run on this platform.
pub const HOST_COLLECTION_SUPPORTED: bool = cfg!(unix);

// ---------------------------------------------------------------- inventory

use std::collections::BTreeMap;

use alien_core::{
    ContainerImageIdentity, HeartbeatBackend, ObservedHealth, ObservedInventoryBatch,
    ObservedResourceSample, Platform, ProviderLifecycleState,
};
use chrono::Utc;
use serde_json::{json, Value};
use tracing::warn;

use crate::host_logs::HostLogsConfig;

const INVENTORY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const INVENTORY_SCOPE: &str = "host";
/// Most service units and containers reported in one pass.
const MAX_UNITS: usize = 500;
const MAX_CONTAINERS: usize = 200;

fn sample(
    raw_identity: String,
    provider_kind: &str,
    display_name: String,
    health: ObservedHealth,
    lifecycle: ProviderLifecycleState,
    attributes: BTreeMap<String, Value>,
) -> ObservedResourceSample {
    ObservedResourceSample {
        deployment_id: None,
        raw_identity,
        provider_kind: provider_kind.to_string(),
        display_name,
        namespace: None,
        region: None,
        scope: None,
        resource_type_hint: None,
        version: None,
        alien_resource_id: None,
        health,
        lifecycle,
        message: None,
        partial: false,
        provider_stale: false,
        counts: None,
        collection_issues: Vec::new(),
        labels: BTreeMap::new(),
        attributes,
        raw: Vec::new(),
        images: Vec::new(),
    }
}

/// The machine: OS, kernel, CPU count and memory.
pub(crate) fn host_sample(
    hostname: &str,
    os: &str,
    kernel: &str,
    cpus: u64,
    memory_bytes: u64,
) -> ObservedResourceSample {
    sample(
        format!("host:{hostname}"),
        "host/Machine",
        hostname.to_string(),
        ObservedHealth::Healthy,
        ProviderLifecycleState::Running,
        BTreeMap::from([
            ("os".to_string(), json!(os)),
            ("kernel".to_string(), json!(kernel)),
            ("cpus".to_string(), json!(cpus)),
            ("memoryBytes".to_string(), json!(memory_bytes)),
        ]),
    )
}

/// Loaded service units from `systemctl list-units --output=json`, and whether
/// the list is whole: `false` when the output could not be read or more than
/// `MAX_UNITS` services were cut off.
pub(crate) fn unit_samples(json_output: &str) -> (Vec<ObservedResourceSample>, bool) {
    let Ok(Value::Array(units)) = serde_json::from_str::<Value>(json_output) else {
        return (Vec::new(), false);
    };
    let samples: Vec<_> = units
        .iter()
        .filter_map(|unit| {
            let name = unit.get("unit")?.as_str()?;
            if !name.ends_with(".service") {
                return None;
            }
            let active = unit.get("active")?.as_str()?;
            let sub = unit.get("sub").and_then(Value::as_str).unwrap_or_default();
            let (health, lifecycle) = match active {
                "active" | "reloading" => {
                    (ObservedHealth::Healthy, ProviderLifecycleState::Running)
                }
                "failed" => (ObservedHealth::Unhealthy, ProviderLifecycleState::Failed),
                "activating" => (ObservedHealth::Unknown, ProviderLifecycleState::Creating),
                "deactivating" => (ObservedHealth::Unknown, ProviderLifecycleState::Stopping),
                _ => (ObservedHealth::Unknown, ProviderLifecycleState::Stopped),
            };
            Some(sample(
                format!("systemd:{name}"),
                "systemd/Service",
                name.to_string(),
                health,
                lifecycle,
                BTreeMap::from([
                    ("active".to_string(), json!(active)),
                    ("sub".to_string(), json!(sub)),
                    (
                        "description".to_string(),
                        json!(unit
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()),
                    ),
                ]),
            ))
        })
        .collect();
    let complete = samples.len() <= MAX_UNITS;
    (samples.into_iter().take(MAX_UNITS).collect(), complete)
}

/// One container: image, state, restarts and health. The environment is never
/// read, so no value can reach the report.
pub(crate) fn container_sample(
    id: &str,
    name: &str,
    image: &str,
    state: &str,
    restart_count: i64,
    health: Option<&str>,
) -> ObservedResourceSample {
    let (observed, lifecycle) = match (state, health) {
        (_, Some("unhealthy")) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Running),
        ("running", _) => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
        ("restarting", _) => (ObservedHealth::Degraded, ProviderLifecycleState::Updating),
        ("exited", _) | ("dead", _) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
        ("paused", _) | ("created", _) => {
            (ObservedHealth::Unknown, ProviderLifecycleState::Stopped)
        }
        _ => (ObservedHealth::Unknown, ProviderLifecycleState::Unknown),
    };
    let mut attributes = BTreeMap::from([
        ("state".to_string(), json!(state)),
        ("restartCount".to_string(), json!(restart_count)),
    ]);
    if let Some(health) = health {
        attributes.insert("health".to_string(), json!(health));
    }
    let mut item = sample(
        format!("docker:{id}"),
        "docker/Container",
        name.to_string(),
        observed,
        lifecycle,
        attributes,
    );
    item.images = vec![ContainerImageIdentity {
        name: name.to_string(),
        image: image.to_string(),
        digest: None,
    }];
    item
}

fn read_trimmed(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

/// `PRETTY_NAME` from an os-release file's text.
pub(crate) fn os_pretty_name(os_release: &str) -> String {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

/// `MemTotal` in bytes from /proc/meminfo's text.
pub(crate) fn mem_total_bytes(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse::<u64>().ok())
        .map_or(0, |kib| kib.saturating_mul(1024))
}

async fn docker_inventory(
    docker: &bollard::Docker,
) -> crate::error::Result<(
    Option<ObservedResourceSample>,
    Vec<ObservedResourceSample>,
    bool,
)> {
    use alien_error::{Context as _, IntoAlienError as _};
    let failed = |message: &str| crate::error::ErrorData::PodLogReadFailed {
        message: message.to_string(),
    };
    let info = docker
        .info()
        .await
        .into_alien_error()
        .context(failed("could not read Docker host information"))?;
    let host = host_sample(
        info.name.as_deref().unwrap_or("docker-host"),
        info.operating_system.as_deref().unwrap_or_default(),
        info.kernel_version.as_deref().unwrap_or_default(),
        info.ncpu.map_or(0, |n| u64::try_from(n).unwrap_or(0)),
        info.mem_total.map_or(0, |n| u64::try_from(n).unwrap_or(0)),
    );
    let listed = docker
        .list_containers(Some(bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        }))
        .await
        .into_alien_error()
        .context(failed("could not list Docker containers"))?;
    let complete = listed.len() <= MAX_CONTAINERS;
    let mut containers = Vec::new();
    for container in listed.into_iter().take(MAX_CONTAINERS) {
        let Some(id) = container.id else { continue };
        let name = container
            .names
            .and_then(|names| names.into_iter().next())
            .map(|name| name.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.chars().take(12).collect());
        let (restarts, health) = match docker
            .inspect_container(
                &id,
                Some(bollard::query_parameters::InspectContainerOptionsBuilder::default().build()),
            )
            .await
        {
            Ok(detail) => (
                detail.restart_count.unwrap_or(0),
                detail
                    .state
                    .and_then(|state| state.health)
                    .and_then(|health| health.status)
                    .map(|status| status.to_string()),
            ),
            Err(_) => (0, None),
        };
        containers.push(container_sample(
            &id.chars().take(12).collect::<String>(),
            &name,
            container.image.as_deref().unwrap_or_default(),
            &container
                .state
                .map(|state| state.to_string())
                .unwrap_or_default(),
            restarts,
            health
                .as_deref()
                .filter(|status| !status.is_empty() && *status != "none"),
        ));
    }
    Ok((Some(host), containers, complete))
}

async fn collect(config: HostLogsConfig) -> Option<ObservedInventoryBatch> {
    let mut resources = Vec::new();
    let mut host = None;
    // A batch that is not whole must not replace the scope: what is missing
    // would be read as removed.
    let mut complete = true;
    if config.docker {
        match docker_client() {
            Ok(docker) => match docker_inventory(&docker).await {
                Ok((docker_host, containers, whole)) => {
                    host = docker_host;
                    resources.extend(containers);
                    complete &= whole;
                }
                Err(error) => {
                    complete = false;
                    warn!(error = %error, "Docker inventory failed");
                }
            },
            Err(error) => {
                complete = false;
                warn!(error = %error, "Docker inventory failed");
            }
        }
    }
    if config.journald {
        let output = tokio::process::Command::new("systemctl")
            .env_clear()
            .args([
                "list-units",
                "--type=service",
                "--all",
                "--no-pager",
                "--output=json",
            ])
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => {
                let (units, whole) = unit_samples(&String::from_utf8_lossy(&output.stdout));
                resources.extend(units);
                complete &= whole;
            }
            Ok(_) | Err(_) => {
                complete = false;
                warn!("Could not list systemd units for the inventory");
            }
        }
    }
    let host = host.unwrap_or_else(|| {
        let hostname = hostname::get()
            .ok()
            .and_then(|name| name.into_string().ok())
            .unwrap_or_else(|| "host".to_string());
        host_sample(
            &hostname,
            &os_pretty_name(&read_trimmed("/etc/os-release")),
            &read_trimmed("/proc/sys/kernel/osrelease"),
            std::thread::available_parallelism().map_or(0, |n| n.get() as u64),
            mem_total_bytes(&read_trimmed("/proc/meminfo")),
        )
    });
    resources.insert(0, host);
    Some(ObservedInventoryBatch {
        source_kind: "operator".to_string(),
        inventory_scope: INVENTORY_SCOPE.to_string(),
        controller_platform: Platform::Local,
        backend: HeartbeatBackend::Local,
        observed_at: Utc::now(),
        complete,
        resources,
    })
}

/// Scopes this operator writes itself, outside the deployment steps. A deployment
/// step that replaces the pending batches keeps these.
pub(crate) fn is_operator_scope(scope: &str) -> bool {
    scope == INVENTORY_SCOPE
}

/// Replace this scope's pending batch and keep any other scope's.
pub(crate) fn merge_batches(
    pending: Vec<ObservedInventoryBatch>,
    batch: ObservedInventoryBatch,
) -> Vec<ObservedInventoryBatch> {
    let mut merged: Vec<_> = pending
        .into_iter()
        .filter(|existing| existing.inventory_scope != batch.inventory_scope)
        .collect();
    merged.push(batch);
    merged
}

/// Report the host, its service units and its containers every minute.
pub async fn run_inventory_loop(
    state: std::sync::Arc<crate::OperatorState>,
    config: HostLogsConfig,
) {
    loop {
        if let Some(batch) = collect(config).await {
            let result = async {
                state
                    .db
                    .update_pending_observed_inventory_batches(|pending| {
                        merge_batches(pending, batch)
                    })
                    .await?;
                state.db.request_sync();
                crate::error::Result::Ok(())
            }
            .await;
            if let Err(error) = result {
                warn!(error = %error, "Could not queue the host inventory");
            }
        }
        tokio::select! {
            _ = state.cancel.cancelled() => return,
            _ = tokio::time::sleep(INVENTORY_INTERVAL) => {}
        }
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;

    #[test]
    fn units_report_state_and_only_services() {
        let json = r#"[
            {"unit":"nginx.service","load":"loaded","active":"active","sub":"running","description":"web"},
            {"unit":"broken.service","load":"loaded","active":"failed","sub":"failed","description":"x"},
            {"unit":"sshd.socket","load":"loaded","active":"active","sub":"listening","description":"s"}
        ]"#;
        let (units, complete) = unit_samples(json);
        assert!(complete);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].health, ObservedHealth::Healthy);
        assert_eq!(units[1].health, ObservedHealth::Unhealthy);
        assert_eq!(units[1].lifecycle, ProviderLifecycleState::Failed);
        // An unreadable list, or one cut at the limit, is not a whole list.
        let (none, complete) = unit_samples("not json");
        assert!(none.is_empty() && !complete);
        let many: Vec<_> = (0..MAX_UNITS + 1)
            .map(|i| json!({"unit": format!("u{i}.service"), "active": "active", "sub": "running"}))
            .collect();
        let (cut, complete) = unit_samples(&serde_json::to_string(&many).unwrap());
        assert_eq!(cut.len(), MAX_UNITS);
        assert!(!complete);
    }

    #[test]
    fn a_container_reports_image_state_restarts_and_health_and_no_environment() {
        let item = container_sample("abc123", "web", "nginx:1", "running", 3, Some("unhealthy"));
        assert_eq!(item.health, ObservedHealth::Unhealthy);
        assert_eq!(item.attributes["restartCount"], json!(3));
        assert_eq!(item.images[0].image, "nginx:1");
        let serialized = serde_json::to_string(&item).unwrap();
        assert!(!serialized.to_lowercase().contains("env"), "{serialized}");
        let stopped = container_sample("d", "db", "pg", "exited", 0, None);
        assert_eq!(stopped.lifecycle, ProviderLifecycleState::Stopped);
    }

    #[test]
    fn host_facts_parse_from_os_release_and_meminfo() {
        assert_eq!(
            os_pretty_name("NAME=x\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n"),
            "Ubuntu 24.04 LTS"
        );
        assert_eq!(
            mem_total_bytes("MemTotal:       16384 kB\nMemFree: 1 kB"),
            16384 * 1024
        );
        let host = host_sample("box", "Ubuntu", "6.8.0", 4, 1024);
        assert_eq!(host.raw_identity, "host:box");
        assert_eq!(host.attributes["cpus"], json!(4));
    }

    #[test]
    fn a_new_pass_replaces_its_own_scope_and_keeps_others() {
        let batch = |scope: &str, count: usize| ObservedInventoryBatch {
            source_kind: "operator".into(),
            inventory_scope: scope.into(),
            controller_platform: Platform::Local,
            backend: HeartbeatBackend::Local,
            observed_at: Utc::now(),
            complete: true,
            resources: (0..count)
                .map(|i| host_sample(&format!("h{i}"), "", "", 1, 1))
                .collect(),
        };
        let merged = merge_batches(vec![batch("host", 5), batch("other", 1)], batch("host", 2));
        assert_eq!(merged.len(), 2);
        let host = merged.iter().find(|b| b.inventory_scope == "host").unwrap();
        assert_eq!(host.resources.len(), 2);
    }
}
