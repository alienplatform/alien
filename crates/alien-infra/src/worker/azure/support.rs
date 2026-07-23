use super::*;

/// Generates a deterministic Azure Container Apps name for a worker.
pub(super) fn get_azure_container_app_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[cfg(not(test))]
pub(super) const AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS: u64 = 60;
#[cfg(test)]
pub(super) const AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
pub(super) const AZURE_READY_RBAC_WAIT_SECS: u64 = 120;
#[cfg(test)]
pub(super) const AZURE_READY_RBAC_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
pub(super) const AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS: u64 = 900;
#[cfg(test)]
pub(super) const AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
pub(super) const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS: u64 = 600;
#[cfg(test)]
pub(super) const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS: u64 = 0;

pub(super) const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;
pub(super) const AZURE_RBAC_WAIT_MAX_ATTEMPTS: u32 = 1_000;
pub(super) const AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_POLL_SECS: u64 = 30;

pub(super) fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn ensure_rbac_wait_deadline(
    wait_until_epoch_secs: &mut Option<u64>,
    wait_secs: u64,
) -> u64 {
    let now = current_unix_timestamp_secs();
    *wait_until_epoch_secs.get_or_insert_with(|| now.saturating_add(wait_secs))
}

pub(super) fn rbac_wait_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_RBAC_WAIT_POLL_SECS),
        ))
    }
}

pub(super) fn container_apps_environment_wake_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_POLL_SECS),
        ))
    }
}

pub(super) fn retry_after_delay(retry_after_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = retry_after_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(remaining))
    }
}

pub(super) fn dns_name_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('.')
        .to_string()
}

pub(super) fn management_profile_dispatches_commands(
    ctx: &ResourceControllerContext<'_>,
    worker_id: &str,
) -> bool {
    ctx.desired_stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(worker_id))
        .is_some_and(|refs| refs.iter().any(|r| r.id() == "worker/dispatch-command"))
}

pub(super) fn is_azure_container_apps_environment_waking_error(
    error: &AlienError<ErrorData>,
) -> bool {
    fn matches_layer(message: &str, context: Option<&serde_json::Value>) -> bool {
        let context_text = context.map(|value| value.to_string()).unwrap_or_default();
        message.contains("ContainerAppEnvironmentDisabled")
            || message.contains("environment is stopped due to a long period of inactivity")
            || context_text.contains("ContainerAppEnvironmentDisabled")
            || context_text.contains("environment is stopped due to a long period of inactivity")
    }

    if matches_layer(&error.message, error.context.as_ref()) {
        return true;
    }

    let mut source = error.source.as_deref();
    while let Some(layer) = source {
        if matches_layer(&layer.message, layer.context.as_ref()) {
            return true;
        }
        source = layer.source.as_deref();
    }

    false
}

pub(super) fn get_container_apps_certificate_name(prefix: &str, worker_id: &str) -> String {
    format!("{}-{}", prefix, worker_id)
        .replace('_', "-")
        .to_lowercase()
}

/// Domain information for a worker.
pub(super) struct DomainInfo {
    pub(super) fqdn: String,
    pub(super) certificate_id: Option<String>,
    pub(super) keyvault_cert_id: Option<String>,
    pub(super) container_apps_certificate_id: Option<String>,
    pub(super) uses_custom_domain: bool,
}

pub(super) enum DaprComponentOperation {
    Completed,
    Creating(Duration),
    Deleting(Duration),
    Pending(Duration),
}

pub(super) enum CommandsSetupOperation {
    Completed,
    Creating(Duration),
    Deleting(Duration),
    Pending(Duration),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageTriggerInfrastructure {
    #[serde(default)]
    pub storage_id: Option<String>,
    pub source_resource_id: String,
    #[serde(default)]
    pub source_container_name: Option<String>,
    pub event_subscription_name: String,
    pub service_bus_resource_group: String,
    pub namespace_name: String,
    pub queue_name: String,
    #[serde(default)]
    pub queue_applied: bool,
    pub receiver_role_assignment_id: Option<String>,
    #[serde(default)]
    pub delivery_reconciled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AzureStorageTriggerTeardownProgress {
    #[default]
    EventSubscription,
    ReceiverRoleAssignment,
    Queue,
}

pub(super) enum StorageTriggerTeardownResult {
    Complete,
    Mutated,
}

pub(super) enum CommandsTeardownResult {
    Complete,
    Mutated,
    LongRunning(Duration),
}

pub(super) fn emit_azure_container_apps_worker_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    worker_config: &Worker,
    container_app_name: &str,
    container_app: &ContainerApp,
) {
    let properties = container_app.properties.as_ref();
    let template = properties.and_then(|properties| properties.template.as_ref());
    let container = template.and_then(|template| template.containers.first());
    let resources = container.and_then(|container| container.resources.as_ref());
    let scale = template.and_then(|template| template.scale.as_ref());
    let ingress = properties
        .and_then(|properties| properties.configuration.as_ref())
        .and_then(|configuration| configuration.ingress.as_ref());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: worker_config.id.clone(),
        resource_type: Worker::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Worker(WorkerHeartbeatData::AzureContainerApps(
            AzureContainerAppsWorkerHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "Azure Container App '{container_app_name}' is reachable"
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                app_name: container_app_name.to_string(),
                revision: properties.and_then(|properties| properties.latest_revision_name.clone()),
                environment_name: properties.and_then(|properties| {
                    properties
                        .managed_environment_id
                        .clone()
                        .or_else(|| properties.environment_id.clone())
                }),
                provisioning_state: properties
                    .and_then(|properties| properties.provisioning_state.as_ref())
                    .map(|state| format!("{state:?}")),
                running_status: properties
                    .and_then(|properties| properties.running_status.as_ref())
                    .map(|status| format!("{status:?}")),
                ingress_fqdn: ingress.and_then(|ingress| ingress.fqdn.clone()),
                min_replicas: scale.and_then(|scale| scale.min_replicas),
                max_replicas: scale.map(|scale| scale.max_replicas),
                cpu: resources.and_then(|resources| resources.cpu),
                memory: resources.and_then(|resources| resources.memory.clone()),
            },
        )),
        raw: vec![],
    });
}

/// Converts PEM-encoded private key and certificate chain to PKCS#12 format for Azure Key Vault.
/// Azure Key Vault requires certificates in PKCS#12 (PFX) format.
pub(super) fn pem_to_pkcs12(private_key_pem: &str, certificate_chain_pem: &str) -> Result<Vec<u8>> {
    use alien_error::IntoAlienError;

    // Parse private key PEM
    let key_blocks = pem::parse_many(private_key_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse private key PEM".to_string(),
            resource_id: None,
        })?;

    let key_block = key_blocks
        .into_iter()
        .find(|p| p.tag().ends_with("PRIVATE KEY"))
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message:
                    "No PRIVATE KEY block found in private key PEM (expected BEGIN PRIVATE KEY)"
                        .to_string(),
                resource_id: None,
            })
        })?;

    // p12 expects PKCS#8 PrivateKeyInfo DER bytes (BEGIN PRIVATE KEY)
    if key_block.tag() != "PRIVATE KEY" {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Unsupported key type '{}'. Expected 'PRIVATE KEY' (PKCS#8). Convert to PKCS#8 first.",
                key_block.tag()
            ),
            resource_id: None,
        }));
    }
    let key_der = key_block.contents().to_vec();

    // Parse certificate chain PEM
    let cert_blocks = pem::parse_many(certificate_chain_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse certificate chain PEM".to_string(),
            resource_id: None,
        })?;

    let mut certs: Vec<pem::Pem> = cert_blocks
        .into_iter()
        .filter(|p| p.tag().contains("CERTIFICATE"))
        .collect();

    if certs.is_empty() {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "No CERTIFICATE blocks found in PEM".to_string(),
            resource_id: None,
        }));
    }

    // Leaf is first, rest are intermediates
    let leaf_pem = certs.remove(0);
    let leaf_der = leaf_pem.contents().to_vec();

    let intermediate_ders: Vec<Vec<u8>> =
        certs.into_iter().map(|p| p.contents().to_vec()).collect();
    let intermediate_refs: Vec<&[u8]> = intermediate_ders.iter().map(|v| v.as_slice()).collect();

    // Build PKCS#12 with empty password
    let pfx = p12::PFX::new_with_cas(
        &leaf_der,
        &key_der,
        &intermediate_refs,
        "",
        "Alien Worker Certificate",
    )
    .ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "Failed to build PKCS#12 (p12::PFX::new_with_cas returned None)".to_string(),
            resource_id: None,
        })
    })?;

    Ok(pfx.to_der())
}
