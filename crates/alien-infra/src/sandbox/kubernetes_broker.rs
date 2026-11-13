//! Claiming a warm pod and minting the capability that reaches its agent.
//!
//! This is the whole reason a broker exists. Claiming is a `PATCH` on pods, and putting that in
//! the binding would mean the customer's application holds pod-write on the namespace. That is
//! too much: `pods/exec` reaches every pod in the namespace, and the Docker socket is
//! root-equivalent on the host. A narrower credential is worth a process.
//!
//! The application gets back a pod address and a capability scoped to one session. It never gets
//! a cluster credential.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use k8s_openapi::api::core::v1::Pod;

use crate::error::{ErrorData, Result};
use crate::sandbox::{claim_idle_pod, idle_selector};
use alien_core::sandbox_capability::{SandboxCapabilityClaims, SandboxOperationClass};
use alien_core::sandbox_capability_token;
use alien_core::sandbox_image::AGENT_PORT;
use alien_error::{AlienError, Context};
use alien_k8s_clients::kubernetes::pods::PodApi;
use alien_k8s_clients::kubernetes::secrets::SecretsApi;

/// How long a minted capability lives.
///
/// Short because a session is a unit of work someone is waiting on, and a capability that
/// outlives the turn it was minted for is a capability somebody can replay.
const CAPABILITY_LIFETIME_SECONDS: i64 = 900;

/// What the application needs to reach its session, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSession {
    /// Immutable identity used by the pod agent and egress gateway.
    pub agent_session_id: String,
    /// Session id, which the pod now carries as a label
    pub session_id: String,
    /// `http://<pod ip>:<agent port>`
    pub endpoint: String,
    /// Bearer capability, scoped to this session and this operation class
    pub capability: String,
    /// Unix seconds after which the capability is void
    pub expires_at: i64,
    /// Unix seconds at which the session was created, as the pod records it
    pub created_at: i64,
    /// Generation the pod records
    pub generation: u64,
    /// Tenant key the session was created for
    pub tenant_key: Option<String>,
    /// Whether this call created the session, as opposed to finding it
    pub created: bool,
    /// Unix seconds at which the session ends, when it has a lifetime. Anything minted for the
    /// session (the egress gateway's identity token) lives exactly this long, not as long as one
    /// capability.
    pub deadline: Option<i64>,
    /// Whether the creator is still setting the session up (its egress policy). Another caller
    /// that finds the session waits for this to clear before it uses it.
    pub setup_pending: bool,
}

/// What a pod records about its session.
fn recorded(pod: &Pod, fallback_created_at: i64) -> (i64, u64, Option<String>) {
    let annotations = pod.metadata.annotations.as_ref();
    let read = |key: &str| annotations.and_then(|a| a.get(key));
    (
        read(crate::sandbox::ANNOTATION_SESSION_CREATED_AT)
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback_created_at),
        read(crate::sandbox::ANNOTATION_SESSION_GENERATION)
            .and_then(|value| value.parse().ok())
            .unwrap_or(1),
        read(crate::sandbox::ANNOTATION_SESSION_TENANT).cloned(),
    )
}

/// Claims an idle pod for `session_id` and mints a capability addressed to it.
///
/// **The claim is won by the apiserver, not here.** `claim_idle_pod` mutates a pod in memory and
/// the write is what decides it: a conflicting write loses on `resourceVersion` and the loser
/// moves to the next candidate. Deciding locally would hand two callers the same pod.
pub async fn claim_session(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
) -> Result<ClaimedSession> {
    claim_session_with_scope(
        pods,
        secrets,
        sandbox_id,
        namespace,
        session_id,
        secret_name,
        now_unix,
        None,
        None,
        None,
    )
    .await
    .map_err(SessionRequestError::into_alien)
}

pub(crate) async fn claim_session_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    deployment_label_key: &str,
    deployment_label_value: &str,
    tenant_key: Option<&str>,
    timeout_seconds: Option<u64>,
) -> std::result::Result<ClaimedSession, SessionRequestError> {
    claim_session_with_scope(
        pods,
        secrets,
        sandbox_id,
        namespace,
        session_id,
        secret_name,
        now_unix,
        Some((deployment_label_key, deployment_label_value)),
        tenant_key,
        timeout_seconds,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn claim_session_with_scope(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    expected_scope: Option<(&str, &str)>,
    tenant_key: Option<&str>,
    timeout_seconds: Option<u64>,
) -> std::result::Result<ClaimedSession, SessionRequestError> {
    let (signing_key, deployment_label_key, deployment_label_value) =
        signing_key(secrets, namespace, secret_name, sandbox_id, expected_scope).await?;
    require_cluster_dns(secrets, namespace, secret_name, sandbox_id).await?;
    let selector = format!(
        "{},{}={}",
        idle_selector(sandbox_id),
        deployment_label_key,
        deployment_label_value
    );
    let idle = pods
        .list_pods(namespace, Some(selector), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: "failed to list idle sandbox pods".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;

    for pod in idle.items {
        let Some(name) = pod.metadata.name.clone() else {
            continue;
        };
        // A pod that is not Ready has not proven its NetworkPolicy is in force yet (see
        // `require_enforced_policy`): on a fresh node its egress may still be unrestricted.
        // It stays in the pool until it is, and this create takes another pod or goes cold.
        if !pod_is_ready(&pod) {
            continue;
        }

        // The agent starts while the pod is still idle. Its authorization identity is the
        // immutable pod UID, supplied by the downward API, rather than this later claim label.
        let Some(agent_session_id) = pod.metadata.uid.clone().filter(|uid| !uid.is_empty()) else {
            continue;
        };
        let mut candidate = pod;
        if !claim_idle_pod(&mut candidate, session_id) {
            continue;
        }
        crate::sandbox::annotate_claim(&mut candidate, now_unix, tenant_key, timeout_seconds)?;
        mark_policy_verified(&mut candidate);

        // A conflict here is another caller winning the same pod, not an error to report: try
        // the next one rather than failing a create that a warm pool can still satisfy.
        let Ok(claimed) = pods.update_pod(namespace, &name, &candidate).await else {
            continue;
        };

        // A pool pod is labelled idle when it is created, before the kubelet assigns an address,
        // so a claim arriving in that window can win a pod it cannot use. The label write has
        // already committed, and nothing would ever release a session the caller never received —
        // so put it back and try the next candidate, the same as losing the race on one.
        let Some(address) = claimed
            .status
            .as_ref()
            .and_then(|status| status.pod_ip.clone())
        else {
            release_claim(pods, namespace, &name, &claimed).await;
            continue;
        };

        let (capability, expires_at) = mint_capability(
            &signing_key,
            &agent_session_id,
            secret_name,
            sandbox_id,
            now_unix,
        )?;

        return Ok(ClaimedSession {
            agent_session_id,
            session_id: session_id.to_string(),
            endpoint: format!("http://{address}:{AGENT_PORT}"),
            capability,
            expires_at,
            created_at: now_unix,
            generation: 1,
            tenant_key: tenant_key.map(str::to_string),
            created: true,
            deadline: crate::sandbox::session_deadline(&claimed),
            setup_pending: false,
        });
    }

    // The pool is a speed-up, not a limit: with no idle pod the broker builds one from the
    // template the controller published and waits for it to start.
    let template = pod_template(secrets, namespace, secret_name).await?;
    cold_create(
        pods,
        &signing_key,
        namespace,
        sandbox_id,
        session_id,
        secret_name,
        now_unix,
        template,
        tenant_key,
        timeout_seconds,
    )
    .await
}

/// Refuses a new session on an allow-egress sandbox when `sandbox.clusterDNS` does not include the
/// resolver this cluster hands its pods. Only creation is gated: a session already running keeps
/// its pod and its policy, and a lookup by id never comes here.
async fn require_cluster_dns(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    secret_name: &str,
    sandbox_id: &str,
) -> Result<()> {
    require_cluster_dns_with(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        std::env::var(alien_core::ENV_ALIEN_SANDBOX_CLUSTER_DNS)
            .ok()
            .as_deref(),
        &crate::sandbox::kubernetes_dns::read_resolv_conf(),
    )
    .await
}

async fn require_cluster_dns_with(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    secret_name: &str,
    sandbox_id: &str,
    configured: Option<&str>,
    resolv_conf: &str,
) -> Result<()> {
    let secret = secrets.get_secret(namespace, secret_name).await.context(
        ErrorData::CloudPlatformError {
            message: format!("the capability Secret for '{sandbox_id}' is unreadable"),
            resource_id: Some(sandbox_id.to_string()),
        },
    )?;
    let required = secret
        .data
        .as_ref()
        .and_then(|data| data.get(crate::sandbox::REQUIRES_CLUSTER_DNS_SECRET_KEY))
        .is_some_and(|value| value.0 == b"true");
    if !required {
        return Ok(());
    }
    crate::sandbox::kubernetes_dns::check(configured, resolv_conf).map_err(|message| {
        AlienError::new(ErrorData::CloudPlatformError {
            message,
            resource_id: Some(sandbox_id.to_string()),
        })
    })
}

/// How long a cold create waits for the pod to start and receive an address.
///
/// Image pull on a node that has to be provisioned first is the slow part: 79 s measured on a
/// scaled-to-zero Autopilot pool, and about 4 minutes for a 4 GB image on an EKS Auto Mode
/// sandbox node (measured). Ten minutes covers that with room, and a pod that is still not
/// running then is deleted and the caller gets a retryable 503 rather than a leaked pod.
const COLD_CREATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const COLD_CREATE_POLL: std::time::Duration = std::time::Duration::from_secs(2);

async fn pod_template(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    secret_name: &str,
) -> Result<Pod> {
    let secret = secrets.get_secret(namespace, secret_name).await.context(
        ErrorData::CloudPlatformError {
            message: "the sandbox pod template is unreadable".to_string(),
            resource_id: None,
        },
    )?;
    let bytes = secret
        .data
        .as_ref()
        .and_then(|data| data.get(crate::sandbox::POD_TEMPLATE_SECRET_KEY))
        .map(|value| value.0.clone())
        .ok_or_else(|| {
            // Only before the controller's first health tick has published it.
            AlienError::new(ErrorData::CloudPlatformError {
                message: "the sandbox has no pod template yet; it is published on the first \
                          health tick"
                    .to_string(),
                resource_id: None,
            })
        })?;
    let pod = serde_json::from_slice::<Pod>(&bytes).map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("the sandbox pod template is unusable: {error}"),
            resource_id: None,
        })
    })?;
    Ok(pod)
}

#[allow(clippy::too_many_arguments)]
async fn cold_create(
    pods: &Arc<dyn PodApi>,
    signing_key: &ed25519_compact::SecretKey,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    mut pod: Pod,
    tenant_key: Option<&str>,
    timeout_seconds: Option<u64>,
) -> std::result::Result<ClaimedSession, SessionRequestError> {
    // Born claimed: it never appears in the idle selector, so no one else can take it.
    if !claim_idle_pod(&mut pod, session_id) {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "the sandbox pod template is not an idle pool pod".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })
        .into());
    }
    crate::sandbox::annotate_claim(&mut pod, now_unix, tenant_key, timeout_seconds)?;
    let created =
        pods.create_pod(namespace, &pod)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "failed to create a sandbox pod".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            })?;
    let name = created.metadata.name.clone().unwrap_or_default();
    let agent_session_id = created.metadata.uid.clone().unwrap_or_default();

    let started = std::time::Instant::now();
    loop {
        let current = pods.get_pod(namespace, &name).await.ok();
        let address = current
            .as_ref()
            .and_then(|pod| pod.status.as_ref())
            .filter(|status| status.phase.as_deref() == Some("Running"))
            .filter(|_| current.as_ref().is_some_and(pod_is_ready))
            .and_then(|status| status.pod_ip.clone());
        // The deadline started at the claim. It is checked after the pod is read, so a pod that
        // is Running only after it is not returned: the Operator would end it at once, and the
        // caller's first command would fail.
        if crate::sandbox::session_deadline_passed(
            &pod,
            now_unix + started.elapsed().as_secs() as i64,
        ) {
            let _ = pods.delete_pod(namespace, &name).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "the sandbox pod for '{sandbox_id}' was not running before the session's lifetime ended; create it again with a longer timeout"
                ),
                resource_id: Some(sandbox_id.to_string()),
            })
            .into());
        }
        if let Some(address) = address {
            // Record the proof so a retry after an Operator restart can tell this pod from one
            // that became Ready against a dead address. Failing to write it is safe: reattach
            // then falls back to the address comparison.
            if let Some(mut verified) = current {
                mark_policy_verified(&mut verified);
                let _ = pods.update_pod(namespace, &name, &verified).await;
            }
            let (capability, expires_at) = mint_capability(
                signing_key,
                &agent_session_id,
                secret_name,
                sandbox_id,
                now_unix + started.elapsed().as_secs() as i64,
            )?;
            return Ok(ClaimedSession {
                agent_session_id,
                session_id: session_id.to_string(),
                endpoint: format!("http://{address}:{AGENT_PORT}"),
                capability,
                expires_at,
                created_at: now_unix,
                generation: 1,
                tenant_key: tenant_key.map(str::to_string),
                created: true,
                deadline: crate::sandbox::session_deadline(&pod),
                setup_pending: false,
            });
        }
        if started.elapsed() >= COLD_CREATE_TIMEOUT {
            // Nobody holds this session yet, so leaving the pod would leak it.
            let _ = pods.delete_pod(namespace, &name).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "a new sandbox pod for '{sandbox_id}' did not start within {} seconds{}",
                    COLD_CREATE_TIMEOUT.as_secs(),
                    pod_not_ready_reason(current.as_ref())
                ),
                resource_id: Some(sandbox_id.to_string()),
            })
            .into());
        }
        tokio::time::sleep(COLD_CREATE_POLL).await;
    }
}

/// Mints a capability for one pod's agent, valid for [`CAPABILITY_LIFETIME_SECONDS`].
fn mint_capability(
    signing_key: &ed25519_compact::SecretKey,
    agent_session_id: &str,
    secret_name: &str,
    sandbox_id: &str,
    now_unix: i64,
) -> Result<(String, i64)> {
    let expires_at = now_unix + CAPABILITY_LIFETIME_SECONDS;
    let capability = sandbox_capability_token::mint(
        &SandboxCapabilityClaims {
            session_id: agent_session_id.to_string(),
            operation: SandboxOperationClass::Execute,
            generation: 1,
            expires_at,
            key_id: secret_name.to_string(),
            port: None,
        },
        signing_key,
    )
    .context(ErrorData::CloudPlatformError {
        message: "failed to mint a sandbox capability".to_string(),
        resource_id: Some(sandbox_id.to_string()),
    })?;
    Ok((capability, expires_at))
}

/// Finds the live pod of a claimed session of this deployment, if there is one.
async fn claimed_pods(
    pods: &Arc<dyn PodApi>,
    namespace: &str,
    sandbox_id: &str,
    now_unix: i64,
    session_id: Option<&str>,
    deployment_label_key: &str,
    deployment_label_value: &str,
) -> Result<Vec<Pod>> {
    let session = match session_id {
        Some(session_id) => format!("{}={session_id}", crate::sandbox::LABEL_SESSION),
        // Any value: only a claimed pod carries the session label.
        None => crate::sandbox::LABEL_SESSION.to_string(),
    };
    let selector = format!(
        "{}={sandbox_id},{session},{deployment_label_key}={deployment_label_value}",
        crate::sandbox::LABEL_SANDBOX,
    );
    let found = pods
        .list_pods(namespace, Some(selector), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("could not look up the sessions of '{sandbox_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    Ok(found
        .items
        .into_iter()
        .filter(|pod| session_is_live(pod, now_unix))
        .collect())
}

/// Whether a claimed pod can still be a session: not being deleted, not finished (a pod keeps its
/// address after it succeeded or failed) and not past its deadline (the Operator ends it on its
/// next tick, but until then it must not be handed out).
fn session_is_live(pod: &Pod, now_unix: i64) -> bool {
    pod.metadata.deletion_timestamp.is_none()
        && !matches!(
            pod.status
                .as_ref()
                .and_then(|status| status.phase.as_deref()),
            Some("Succeeded" | "Failed")
        )
        && !crate::sandbox::session_deadline_passed(pod, now_unix)
}

/// Returns a session the caller already holds, with a fresh capability, or `None` when no pod
/// carries that session id.
///
/// The pod's labels are the only record of a session, so any process of the deployment can
/// address a session another one claimed, and a capability that is about to expire is replaced by
/// asking again. A pod that exists but has no address yet is an error rather than `None`: the
/// session exists and the caller should retry, not create a second one.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn reattach_session_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    deployment_label_key: &str,
    deployment_label_value: &str,
) -> Result<Option<ClaimedSession>> {
    let (signing_key, key, value) = signing_key(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        Some((deployment_label_key, deployment_label_value)),
    )
    .await?;
    let mut found = claimed_pods(
        pods,
        namespace,
        sandbox_id,
        now_unix,
        Some(session_id),
        &key,
        &value,
    )
    .await?;
    // Two processes creating one new id at the same moment can each end up with a pod. Every
    // lookup answers with the same one (the earliest claim, then the name), so the callers
    // converge on it and the other ends at its deadline or at terminate.
    found.sort_by_key(|pod| {
        (
            recorded(pod, i64::MAX).0,
            pod.metadata.name.clone().unwrap_or_default(),
        )
    });
    let Some(pod) = found.into_iter().next() else {
        return Ok(None);
    };
    let agent_session_id = pod
        .metadata
        .uid
        .clone()
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("session '{session_id}' has a pod with no identity"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;
    let address = pod
        .status
        .as_ref()
        .and_then(|status| status.pod_ip.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("session '{session_id}' has no address yet"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;
    // A pod that is not Ready has not proven its NetworkPolicy is in force (see `cold_create`):
    // a retry for a session whose pod is still starting must not receive it early. Only the
    // `Ready` condition counts, together with either the verified mark (set when the session was
    // handed out against the then-current Operator, so a restart does not strand it) or a check
    // that still points at this Operator. A claimed pod with neither (a cold pod that became
    // Ready against a dead address) is not handed out.
    if !reattachable_for(
        &pod,
        std::env::var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_ADDRESS)
            .ok()
            .as_deref(),
    ) {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!("session '{session_id}' is not ready yet"),
            resource_id: Some(sandbox_id.to_string()),
        }));
    }
    let (capability, expires_at) = mint_capability(
        &signing_key,
        &agent_session_id,
        secret_name,
        sandbox_id,
        now_unix,
    )?;
    let (created_at, generation, tenant_key) = recorded(&pod, now_unix);
    Ok(Some(ClaimedSession {
        agent_session_id,
        session_id: session_id.to_string(),
        endpoint: format!("http://{address}:{AGENT_PORT}"),
        capability,
        expires_at,
        created_at,
        generation,
        tenant_key,
        created: false,
        deadline: crate::sandbox::session_deadline(&pod),
        setup_pending: setup_pending(secrets, namespace, sandbox_id, session_id, now_unix).await?,
    }))
}

/// Whether the pod's `Ready` condition is true and its policy check still points at this
/// Operator. A pod built with the policy gate becomes Ready only after its NetworkPolicy is
/// proven in force (connections to the Operator's address are dropped); that proof means nothing
/// if the address is no longer the Operator's (it was rescheduled), because a dropped packet to a
/// dead address says nothing about the policy. Such a pod is not handed out and the controller
/// replaces it. One built without the gate is Ready when its container runs.
pub(crate) fn pod_is_ready(pod: &Pod) -> bool {
    pod_is_ready_for(
        pod,
        std::env::var(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_ADDRESS)
            .ok()
            .as_deref(),
    )
}

/// Why a running pod is still not Ready, when the cause is the policy gate: its container runs
/// but its network check never passed, so the cluster is not enforcing the pod's NetworkPolicy.
/// Empty when the pod is not in that state (it may simply still be pulling its image).
pub(crate) fn pod_not_ready_reason(pod: Option<&Pod>) -> &'static str {
    let Some(pod) = pod else { return "" };
    let running = pod
        .status
        .as_ref()
        .is_some_and(|status| status.phase.as_deref() == Some("Running"));
    let gated = pod
        .spec
        .as_ref()
        .and_then(|spec| spec.containers.first())
        .and_then(|container| container.env.as_ref())
        .is_some_and(|env| {
            env.iter()
                .any(|var| var.name == "ALIEN_SANDBOX_POLICY_CANARY")
        });
    if running && gated && !pod_condition_ready(pod) {
        ": the pod is running but its network policy is not enforced by this cluster, so it is never marked Ready; use a cluster network plugin that enforces NetworkPolicy (on EKS Auto Mode, enable network policy in the vpc-cni ConfigMap)"
    } else {
        ""
    }
}

fn mark_policy_verified(pod: &mut Pod) {
    pod.metadata
        .annotations
        .get_or_insert_with(Default::default)
        .insert(ANNOTATION_POLICY_VERIFIED.to_string(), "true".to_string());
}

fn policy_verified(pod: &Pod) -> bool {
    pod.metadata
        .annotations
        .as_ref()
        .is_some_and(|annotations| annotations.contains_key(ANNOTATION_POLICY_VERIFIED))
}

/// Whether a claimed pod may be handed to a retry: Ready, and either marked verified or checked
/// against the current Operator address.
fn reattachable_for(pod: &Pod, operator_address: Option<&str>) -> bool {
    pod_condition_ready(pod) && (policy_verified(pod) || pod_is_ready_for(pod, operator_address))
}

fn pod_condition_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|condition| condition.type_ == "Ready" && condition.status == "True")
        })
}

fn pod_is_ready_for(pod: &Pod, operator_address: Option<&str>) -> bool {
    let ready = pod_condition_ready(pod);
    let canary = pod
        .spec
        .as_ref()
        .and_then(|spec| spec.containers.first())
        .and_then(|container| container.env.as_ref())
        .and_then(|env| {
            env.iter()
                .find(|var| var.name == "ALIEN_SANDBOX_POLICY_CANARY")
        })
        .and_then(|var| var.value.as_deref());
    match (
        canary,
        operator_address.filter(|address| !address.is_empty()),
    ) {
        (Some(canary), Some(current)) => ready && canary == current,
        _ => ready,
    }
}

/// The longest a preview capability lives. A caller asks again before it ends.
const PREVIEW_LIFETIME_SECONDS: i64 = 900;

/// What a caller needs to reach one port of a session's own server.
#[derive(Debug, Clone)]
pub struct PreviewGrant {
    /// The agent's address and the preview route for the port
    pub endpoint: String,
    /// A preview capability for this pod and this port
    pub capability: String,
    /// Unix seconds after which the capability is void. Never later than the session's deadline.
    pub expires_at: i64,
}

/// Why a preview capability was not minted.
#[derive(Debug)]
pub enum PreviewError {
    /// The port is not one the sandbox declared for preview.
    PortNotDeclared,
    /// No such session for this deployment, or its deadline has passed.
    NotFound,
    /// The cluster could not answer; worth retrying.
    Unavailable(AlienError<ErrorData>),
}

/// Mints a preview capability for `port` of the pod that serves `session_id`.
///
/// The deployment label check is the same one a reattach makes, so a caller can preview only a
/// session of its own deployment. The port is checked against the declaration the controller
/// published, not against anything in the request.
pub(crate) async fn preview_session_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    deployment_label_key: &str,
    deployment_label_value: &str,
    port: u16,
) -> std::result::Result<PreviewGrant, PreviewError> {
    let unavailable = PreviewError::Unavailable;
    let secret = secrets
        .get_secret(namespace, secret_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("the capability Secret for '{sandbox_id}' is unreadable"),
            resource_id: Some(sandbox_id.to_string()),
        })
        .map_err(unavailable)?;
    let declared: Vec<u16> = secret
        .data
        .as_ref()
        .and_then(|data| data.get(crate::sandbox::PREVIEW_PORTS_SECRET_KEY))
        .and_then(|value| serde_json::from_slice(&value.0).ok())
        .unwrap_or_default();
    if port == AGENT_PORT || !declared.contains(&port) {
        return Err(PreviewError::PortNotDeclared);
    }
    let (signing_key, key, value) = signing_key(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        Some((deployment_label_key, deployment_label_value)),
    )
    .await
    .map_err(unavailable)?;
    let mut found = claimed_pods(
        pods,
        namespace,
        sandbox_id,
        now_unix,
        Some(session_id),
        &key,
        &value,
    )
    .await
    .map_err(unavailable)?;
    found.sort_by_key(|pod| {
        (
            recorded(pod, i64::MAX).0,
            pod.metadata.name.clone().unwrap_or_default(),
        )
    });
    let Some(pod) = found.into_iter().next() else {
        return Err(PreviewError::NotFound);
    };
    if crate::sandbox::session_deadline_passed(&pod, now_unix) {
        return Err(PreviewError::NotFound);
    }
    let (Some(uid), Some(address)) = (
        pod.metadata.uid.clone().filter(|uid| !uid.is_empty()),
        pod.status.as_ref().and_then(|status| status.pod_ip.clone()),
    ) else {
        return Err(PreviewError::NotFound);
    };
    let deadline = pod
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(crate::sandbox::ANNOTATION_SESSION_DEADLINE))
        .and_then(|value| value.parse::<i64>().ok());
    let expires_at = (now_unix + PREVIEW_LIFETIME_SECONDS).min(deadline.unwrap_or(i64::MAX));
    let (_, generation, _) = recorded(&pod, now_unix);
    let capability = sandbox_capability_token::mint(
        &SandboxCapabilityClaims {
            session_id: uid,
            operation: SandboxOperationClass::Preview,
            generation: if generation == 0 { 1 } else { 1 },
            expires_at,
            key_id: secret_name.to_string(),
            port: Some(port),
        },
        &signing_key,
    )
    .context(ErrorData::CloudPlatformError {
        message: "failed to mint a preview capability".to_string(),
        resource_id: Some(sandbox_id.to_string()),
    })
    .map_err(unavailable)?;
    Ok(PreviewGrant {
        endpoint: format!("http://{address}:{AGENT_PORT}/v1/preview/{port}"),
        capability,
        expires_at,
    })
}

/// Why a session request could not be answered by finding or creating the session.
#[derive(Debug)]
pub(crate) enum SessionRequestError {
    /// The session exists for a different tenant than the request names.
    TenantMismatch,
    /// The requested timeout is longer than the sandbox's declared lifetime.
    LifetimeTooLong(crate::sandbox::LifetimeTooLong),
    /// Anything else; retryable from the caller's point of view.
    Unavailable(AlienError<ErrorData>),
}

impl From<AlienError<ErrorData>> for SessionRequestError {
    fn from(error: AlienError<ErrorData>) -> Self {
        Self::Unavailable(error)
    }
}

impl From<crate::sandbox::LifetimeTooLong> for SessionRequestError {
    fn from(error: crate::sandbox::LifetimeTooLong) -> Self {
        Self::LifetimeTooLong(error)
    }
}

impl SessionRequestError {
    fn into_alien(self) -> AlienError<ErrorData> {
        match self {
            Self::Unavailable(error) => error,
            Self::TenantMismatch => AlienError::new(ErrorData::CloudPlatformError {
                message: "the session belongs to another tenant".to_string(),
                resource_id: None,
            }),
            Self::LifetimeTooLong(too_long) => AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "the sandbox allows a session of at most {} seconds",
                    too_long.max_seconds
                ),
                resource_id: None,
            }),
        }
    }
}

/// An existing session is answered only to the tenant key it was created with. A request that
/// names another key, none for a keyed session, or a key for an unkeyed one is a mismatch: it is
/// refused, never answered with the session.
fn require_same_tenant(
    found: ClaimedSession,
    requested: Option<&str>,
) -> std::result::Result<ClaimedSession, SessionRequestError> {
    if found.tenant_key.as_deref() == requested {
        Ok(found)
    } else {
        Err(SessionRequestError::TenantMismatch)
    }
}

/// How long a creator's reservation holds the id before another creator may take it over: a cold
/// create's wait plus a margin. Only a creator that died mid-claim leaves one this old.
const RESERVATION_TTL_SECONDS: i64 = COLD_CREATE_TIMEOUT.as_secs() as i64 + 60;
/// How often a caller that lost the reservation looks for the session the winner is creating.
#[cfg(not(test))]
const RESERVATION_POLL: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(test)]
const RESERVATION_POLL: std::time::Duration = std::time::Duration::from_millis(5);
const ANNOTATION_RESERVED_AT: &str = "alien.dev/reserved-at";
/// Set on a reservation the creator keeps until it reports that the session's setup is done.
const ANNOTATION_SETUP_PENDING: &str = "alien.dev/setup-pending";
/// Set on a reservation once its creator has claimed the session's pod. Ending a session or
/// reporting its setup only lets go of a reservation in this state: one without it belongs to a
/// creator that is still claiming, and a second pod for the id would follow if it were cleared.
/// The value is the unix second of the claim, where the setup window starts.
const ANNOTATION_CLAIMED: &str = "alien.dev/session-claimed";
/// Set on a pod once it was Ready with its policy check pointing at the Operator that handed
/// the session out. A retry or a later Operator (new address) trusts the pod only with this
/// mark, or while the check still points at the current Operator.
const ANNOTATION_POLICY_VERIFIED: &str = "alien.dev/policy-verified";
/// How long a creator may take over a session's setup. Past it a session that is still marked is
/// reported as ready: a creator that died mid-setup must not leave every caller waiting for a
/// session that is only missing its egress policy, which each of them can set.
const SETUP_WINDOW_SECONDS: i64 = 120;

/// Name of the object that serializes creation of one session id.
fn reservation_name(sandbox_id: &str, session_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{sandbox_id}\0{session_id}"));
    let hex: String = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("alien-session-claim-{hex}")
}

/// What `reserve` found.
enum Reservation {
    /// This caller holds the id and is the only one creating its session. `uid` names the object
    /// it created, so it only ever lets go of its own.
    Held { uid: Option<String> },
    /// Another creator holds it, since this unix second.
    Taken {
        reserved_at: i64,
        uid: Option<String>,
    },
}

/// Creates the per-id reservation if no one holds it. Kubernetes refuses a second create of the
/// same name, so exactly one of any number of concurrent creators holds it.
async fn reserve(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    now_unix: i64,
    hold_for_setup: bool,
) -> Result<Reservation> {
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    let name = reservation_name(sandbox_id, session_id);
    let reservation = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(std::collections::BTreeMap::from([(
                crate::sandbox::LABEL_SANDBOX.to_string(),
                sandbox_id.to_string(),
            )])),
            annotations: Some({
                let mut annotations = std::collections::BTreeMap::from([(
                    ANNOTATION_RESERVED_AT.to_string(),
                    now_unix.to_string(),
                )]);
                if hold_for_setup {
                    annotations.insert(ANNOTATION_SETUP_PENDING.to_string(), "true".to_string());
                }
                annotations
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    match secrets.create_secret(namespace, &reservation).await {
        Ok(created) => Ok(Reservation::Held {
            uid: created.metadata.uid,
        }),
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
            ) =>
        {
            let held = secrets.get_secret_metadata(namespace, &name).await;
            let (reserved_at, uid) = match held {
                Ok(meta) => (
                    meta.annotations
                        .and_then(|annotations| annotations.get(ANNOTATION_RESERVED_AT).cloned())
                        .and_then(|value| value.parse::<i64>().ok())
                        // A reservation nobody can date is as old as it can be.
                        .unwrap_or(0),
                    meta.uid,
                ),
                // Released between the two calls: the next attempt decides again.
                Err(_) => (now_unix, None),
            };
            Ok(Reservation::Taken { reserved_at, uid })
        }
        Err(error) => Err(Err::<(), _>(error)
            .context(ErrorData::CloudPlatformError {
                message: format!("could not reserve session '{session_id}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
            .unwrap_err()),
    }
}

/// Whether the creator of `session_id` still holds the reservation it keeps while it sets the
/// session up. A missing reservation (the usual case) is a session that is ready.
async fn setup_pending(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    now_unix: i64,
) -> Result<bool> {
    let name = reservation_name(sandbox_id, session_id);
    let meta = match secrets.get_secret_metadata(namespace, &name).await {
        Ok(meta) => meta,
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(false)
        }
        Err(error) => {
            return Err(Err::<(), _>(error)
                .context(ErrorData::CloudPlatformError {
                    message: format!("could not read the setup state of session '{session_id}'"),
                    resource_id: Some(sandbox_id.to_string()),
                })
                .unwrap_err())
        }
    };
    let annotations = meta.annotations.unwrap_or_default();
    if annotations
        .get(ANNOTATION_SETUP_PENDING)
        .map(String::as_str)
        != Some("true")
    {
        return Ok(false);
    }
    let reserved_at = annotations
        .get(ANNOTATION_RESERVED_AT)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    // The setup window starts when the creator's claim finished, not when it reserved the id: a
    // cold create can take much longer than the window. Before the claim is recorded the creator
    // is still claiming, which the reservation's own lifetime bounds.
    Ok(match claimed_at(&annotations, reserved_at) {
        Some(claimed_at) => now_unix - claimed_at <= SETUP_WINDOW_SECONDS,
        None => now_unix - reserved_at <= RESERVATION_TTL_SECONDS,
    })
}

/// Unix second at which the creator recorded its claim, if it has. A record without a readable
/// time counts from the reservation.
fn claimed_at(
    annotations: &std::collections::BTreeMap<String, String>,
    reserved_at: i64,
) -> Option<i64> {
    annotations
        .get(ANNOTATION_CLAIMED)
        .map(|value| value.parse::<i64>().unwrap_or(reserved_at))
}

/// The creator reports that its setup of the session is done (or that it ended the session):
/// lets go of the reservation it kept.
pub(crate) async fn finish_session_setup_for_deployment(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    secret_name: &str,
    deployment_label_key: &str,
    deployment_label_value: &str,
) -> Result<()> {
    signing_key(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        Some((deployment_label_key, deployment_label_value)),
    )
    .await?;
    release_claimed_reservation(secrets, namespace, sandbox_id, session_id).await;
    Ok(())
}

/// Lets go of the reservation with this uid, and of no other: the name is reused by the next
/// creator of the id. Best effort: one that is left behind is deleted by the Operator's sweep.
async fn release_reservation(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    uid: Option<&str>,
) {
    let Some(uid) = uid else { return };
    let _ = secrets
        .delete_secret_if_uid(namespace, &reservation_name(sandbox_id, session_id), uid)
        .await;
}

/// Lets go of the reservation, if it is one whose creator has claimed the session's pod. A
/// reservation of a creator that is still claiming is left alone, so a concurrent create of the
/// same id keeps its hold and cannot be joined by a second pod. Deleted by uid: a reservation
/// another creator took in between is a different object.
async fn release_claimed_reservation(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
) {
    let name = reservation_name(sandbox_id, session_id);
    let Ok(meta) = secrets.get_secret_metadata(namespace, &name).await else {
        return;
    };
    let claimed = meta
        .annotations
        .as_ref()
        .is_some_and(|annotations| annotations.contains_key(ANNOTATION_CLAIMED));
    if claimed {
        release_reservation(
            secrets,
            namespace,
            sandbox_id,
            session_id,
            meta.uid.as_deref(),
        )
        .await;
    }
}

/// Marks the reservation this creator holds as one whose session is claimed, at `now_unix`, which
/// is when the setup window starts. An error leaves the reservation setup-pending but unclaimed,
/// which neither the ready report nor ending the session lets go of, so the caller fails the
/// create rather than continue.
async fn mark_reservation_claimed(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    uid: Option<&str>,
    now_unix: i64,
) -> Result<()> {
    let name = reservation_name(sandbox_id, session_id);
    let mut secret =
        secrets
            .get_secret(namespace, &name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("could not read the reservation of session '{session_id}'"),
                resource_id: Some(sandbox_id.to_string()),
            })?;
    // Only the object this creator created; the update carries its resourceVersion, so a
    // concurrent change fails it.
    if secret.metadata.uid.as_deref() != uid {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "the reservation of session '{session_id}' is no longer this creator's"
            ),
            resource_id: Some(sandbox_id.to_string()),
        }));
    }
    secret
        .metadata
        .annotations
        .get_or_insert_with(Default::default)
        .insert(ANNOTATION_CLAIMED.to_string(), now_unix.to_string());
    secrets
        .update_secret(namespace, &name, &secret)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("could not record the claim of session '{session_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    Ok(())
}

/// Deletes the session reservations of `sandbox_id` that were abandoned: a creator that never
/// reported its setup leaves one past the setup window, and one that died mid-claim leaves one
/// past the reservation's lifetime. Returns how many were deleted.
pub(crate) async fn sweep_abandoned_reservations(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    now_unix: i64,
) -> Result<usize> {
    let selector = format!("{}={sandbox_id}", crate::sandbox::LABEL_SANDBOX);
    let listed = secrets
        .list_secrets(namespace, Some(selector), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to list session reservations".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    let mut deleted = 0;
    for secret in listed.items {
        let (Some(name), Some(uid)) = (
            secret.metadata.name.as_deref(),
            secret.metadata.uid.as_deref(),
        ) else {
            continue;
        };
        if !name.starts_with("alien-session-claim-") {
            continue;
        }
        let annotations = secret.metadata.annotations.unwrap_or_default();
        let reserved_at = annotations
            .get(ANNOTATION_RESERVED_AT)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        // A claimed reservation is judged from the claim; one still being claimed from the
        // reservation, which a cold create may hold for as long as its lifetime.
        let (since, window) = match claimed_at(&annotations, reserved_at) {
            Some(claimed_at) => (claimed_at, SETUP_WINDOW_SECONDS),
            None => (reserved_at, RESERVATION_TTL_SECONDS),
        };
        if now_unix - since > window
            && secrets
                .delete_secret_if_uid(namespace, name, uid)
                .await
                .is_ok()
        {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Finds the session `session_id`, or creates it. Any process of the deployment may call this
/// with the same id: the pod's labels decide which call created it.
///
/// Creation of one id is serialized by a reservation that Kubernetes creates at most once, so two
/// processes racing on the same new id cannot each claim a pod. The one that holds the
/// reservation looks once more for the session (a holder that just finished has labelled its
/// pod before letting go), claims, and lets go; the others wait for that session to appear and
/// answer with it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn find_or_create_session_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
    deployment_label_key: &str,
    deployment_label_value: &str,
    tenant_key: Option<&str>,
    timeout_seconds: Option<u64>,
    hold_for_setup: bool,
) -> std::result::Result<ClaimedSession, SessionRequestError> {
    let unavailable = SessionRequestError::Unavailable;
    let started = std::time::Instant::now();
    let reattach = || {
        reattach_session_for_deployment(
            pods,
            secrets,
            sandbox_id,
            namespace,
            session_id,
            secret_name,
            now_unix,
            deployment_label_key,
            deployment_label_value,
        )
    };

    loop {
        if let Some(found) = reattach().await.map_err(unavailable)? {
            return require_same_tenant(found, tenant_key);
        }
        let elapsed = started.elapsed().as_secs() as i64;
        match reserve(
            secrets,
            namespace,
            sandbox_id,
            session_id,
            now_unix + elapsed,
            hold_for_setup,
        )
        .await
        .map_err(unavailable)?
        {
            Reservation::Held { uid } => {
                // The previous holder labels its pod before it lets go, so a session that was
                // created while this call was between its lookup and its reservation is visible now.
                let outcome = match reattach().await {
                    Ok(Some(found)) => require_same_tenant(found, tenant_key),
                    Ok(None) => {
                        claim_session_for_deployment(
                            pods,
                            secrets,
                            sandbox_id,
                            namespace,
                            session_id,
                            secret_name,
                            now_unix,
                            deployment_label_key,
                            deployment_label_value,
                            tenant_key,
                            timeout_seconds,
                        )
                        .await
                    }
                    Err(error) => Err(SessionRequestError::Unavailable(error)),
                };
                // The creator that asked to set the session up keeps the reservation until it says
                // the setup is done, so a caller that finds the session meanwhile can tell.
                let keep = hold_for_setup && matches!(&outcome, Ok(session) if session.created);
                if keep {
                    let marked = mark_reservation_claimed(
                        secrets,
                        namespace,
                        sandbox_id,
                        session_id,
                        uid.as_deref(),
                        now_unix + started.elapsed().as_secs() as i64,
                    )
                    .await;
                    if let Err(error) = marked {
                        // The session cannot be handed over with a setup window nobody can end:
                        // end the pod this call claimed and let go of its own reservation.
                        let selector = format!(
                            "{}={sandbox_id},{}={session_id}",
                            crate::sandbox::LABEL_SANDBOX,
                            crate::sandbox::LABEL_SESSION
                        );
                        let _ =
                            release_matching_session(pods, namespace, session_id, selector).await;
                        release_reservation(
                            secrets,
                            namespace,
                            sandbox_id,
                            session_id,
                            uid.as_deref(),
                        )
                        .await;
                        return Err(SessionRequestError::Unavailable(error));
                    }
                } else {
                    release_reservation(secrets, namespace, sandbox_id, session_id, uid.as_deref())
                        .await;
                }
                return outcome;
            }
            Reservation::Taken { reserved_at, uid } => {
                if now_unix + elapsed - reserved_at > RESERVATION_TTL_SECONDS {
                    // Its creator died before it could let go. Only the object seen is deleted.
                    release_reservation(secrets, namespace, sandbox_id, session_id, uid.as_deref())
                        .await;
                } else if started.elapsed() >= COLD_CREATE_TIMEOUT {
                    return Err(SessionRequestError::Unavailable(AlienError::new(
                        ErrorData::CloudPlatformError {
                            message: format!(
                                "session '{session_id}' is still being created by another caller"
                            ),
                            resource_id: Some(sandbox_id.to_string()),
                        },
                    )));
                } else {
                    tokio::time::sleep(RESERVATION_POLL).await;
                }
            }
        }
    }
}

/// Ids of the claimed sessions of a sandbox that belong to this deployment.
pub(crate) async fn list_sessions_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    secret_name: &str,
    now_unix: i64,
    deployment_label_key: &str,
    deployment_label_value: &str,
) -> Result<Vec<String>> {
    let (_, key, value) = signing_key(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        Some((deployment_label_key, deployment_label_value)),
    )
    .await?;
    let found = claimed_pods(pods, namespace, sandbox_id, now_unix, None, &key, &value).await?;
    Ok(found.iter().filter_map(session_name).collect())
}

/// Ends a pod claimed a moment ago that the caller will never receive.
///
/// Best effort on purpose: if this fails, the pod is a session nobody holds and it ends at its
/// own deadline (the Operator, or the kubelet's backstop); the health tick replaces the pool slot.
/// It is deleted rather than returned to the pool because the claim set the kubelet's
/// `activeDeadlineSeconds`, which cannot be removed again: an idle pod carrying it would be
/// killed with the pool never knowing why.
async fn release_claim(pods: &Arc<dyn PodApi>, namespace: &str, name: &str, claimed: &Pod) {
    match claimed.metadata.uid.as_deref() {
        Some(uid) => {
            let _ = pods.delete_pod_if_uid(namespace, name, uid).await;
        }
        None => {
            let _ = pods.delete_pod(namespace, name).await;
        }
    }
}

/// Reads the sandbox's signing key out of the Secret the controller provisioned.
async fn signing_key(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    secret_name: &str,
    sandbox_id: &str,
    expected_scope: Option<(&str, &str)>,
) -> Result<(ed25519_compact::SecretKey, String, String)> {
    let secret = secrets.get_secret(namespace, secret_name).await.context(
        ErrorData::CloudPlatformError {
            message: format!("the capability key for '{sandbox_id}' is unreadable"),
            resource_id: Some(sandbox_id.to_string()),
        },
    )?;

    let labels = secret.metadata.labels.as_ref();
    let owner_matches = labels
        .and_then(|labels| labels.get(crate::sandbox::LABEL_SANDBOX))
        .map(String::as_str)
        == Some(sandbox_id);
    let scopes = labels
        .into_iter()
        .flat_map(|labels| labels.iter())
        .filter(|(key, _)| key.ends_with("/deployment"))
        .collect::<Vec<_>>();
    let scope = match scopes.as_slice() {
        [(key, value)]
            if owner_matches
                && !value.is_empty()
                && expected_scope.is_none_or(|(expected_key, expected_value)| {
                    key.as_str() == expected_key && value.as_str() == expected_value
                }) =>
        {
            ((*key).clone(), (*value).clone())
        }
        _ => {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "the capability Secret for '{sandbox_id}' is not owned by this deployment"
                ),
                resource_id: Some(sandbox_id.to_string()),
            }));
        }
    };

    let bytes = secret
        .data
        .as_ref()
        .and_then(|data| data.get("signingKey"))
        .map(|value| value.0.clone())
        .or_else(|| {
            secret
                .string_data
                .as_ref()
                .and_then(|data| data.get("signingKey"))
                .and_then(|value| BASE64.decode(value).ok())
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("the capability Secret for '{sandbox_id}' carries no key"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;

    let pair = ed25519_compact::KeyPair::from_slice(&bytes).map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("the stored capability key is unusable: {error}"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })?;

    Ok((pair.sk, scope.0, scope.1))
}

/// Releases a claimed session by deleting its pod.
///
/// Deleting rather than returning it to the pool: a pod that ran untrusted code cannot be handed
/// to the next session, and the warm pool refills from a clean image.
///
/// Addressed by session rather than by pod name. A claim relabels a warm pool pod instead of
/// renaming it, so the caller never learns a pod name — and taking one from the caller would let
/// any workload in the namespace delete another tenant's session, which is the pod-write this
/// broker exists to withhold. The label selector answers "is this a session of this sandbox" and
/// "which pod is it" in one query.
pub async fn release_session(
    pods: &Arc<dyn PodApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
) -> Result<()> {
    let selector = format!(
        "{}={sandbox_id},{}={session_id}",
        crate::sandbox::LABEL_SANDBOX,
        crate::sandbox::LABEL_SESSION,
    );
    release_matching_session(pods, namespace, session_id, selector).await
}

pub(crate) async fn release_session_for_deployment(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
    secret_name: &str,
    deployment_label_key: &str,
    deployment_label_value: &str,
) -> Result<()> {
    let (_, deployment_label_key, deployment_label_value) = signing_key(
        secrets,
        namespace,
        secret_name,
        sandbox_id,
        Some((deployment_label_key, deployment_label_value)),
    )
    .await?;
    let selector = format!(
        "{}={sandbox_id},{}={session_id},{}={deployment_label_value}",
        crate::sandbox::LABEL_SANDBOX,
        crate::sandbox::LABEL_SESSION,
        deployment_label_key
    );
    let released = release_matching_session(pods, namespace, session_id, selector).await;
    // A session that ends while its creator was setting it up must not leave callers waiting.
    release_claimed_reservation(secrets, namespace, sandbox_id, session_id).await;
    released
}

async fn release_matching_session(
    pods: &Arc<dyn PodApi>,
    namespace: &str,
    session_id: &str,
    selector: String,
) -> Result<()> {
    let claimed = pods
        .list_pods(namespace, Some(selector), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("could not look up session '{session_id}'"),
            resource_id: Some(session_id.to_string()),
        })?;

    // Nothing matching is the desired end state, so release stays idempotent.
    for pod in claimed.items {
        let Some(name) = pod.metadata.name.clone() else {
            continue;
        };
        pods.delete_pod(namespace, &name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("could not release session '{session_id}'"),
                resource_id: Some(name),
            })?;
    }

    Ok(())
}

/// Pods belonging to a sandbox that carry a session label.
pub fn session_name(pod: &Pod) -> Option<String> {
    pod.metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(crate::sandbox::LABEL_SESSION))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_k8s_clients::kubernetes::pods::MockPodApi;
    use alien_k8s_clients::kubernetes::secrets::MockSecretsApi;

    /// A mock cluster in which no creator holds a reservation for the session.
    fn no_reservation(secrets: &mut MockSecretsApi) {
        secrets.expect_get_secret_metadata().returning(|_, name| {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "secret".to_string(),
                    resource_name: name.to_string(),
                },
            ))
        });
        secrets.expect_delete_secret().returning(|_, _| Ok(()));
        secrets
            .expect_delete_secret_if_uid()
            .returning(|_, _, _| Ok(()));
    }
    use k8s_openapi::api::core::v1::{PodStatus, Secret};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::List;
    use std::collections::BTreeMap;

    const DEPLOYMENT_LABEL_KEY: &str = "alien.dev/deployment";
    const DEPLOYMENT_LABEL_VALUE: &str = "release-a";

    fn sandbox_labels() -> BTreeMap<String, String> {
        let mut labels = crate::sandbox::idle_pod_labels("sbx");
        labels.insert(
            DEPLOYMENT_LABEL_KEY.to_string(),
            DEPLOYMENT_LABEL_VALUE.to_string(),
        );
        labels
    }

    fn idle_pod(name: &str, ip: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                uid: Some(format!("uid-{name}")),
                labels: Some(sandbox_labels()),
                ..Default::default()
            },
            status: Some(PodStatus {
                pod_ip: Some(ip.to_string()),
                conditions: Some(ready_condition(true)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// A policy proof made against an address that is no longer the Operator's (the Operator was
    /// rescheduled) proves nothing: packets to a dead address are dropped whatever the policy.
    #[test]
    fn a_ready_pod_whose_policy_check_targets_another_address_is_not_ready() {
        use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec};
        let mut pod = Pod {
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "p".into(),
                    env: Some(vec![EnvVar {
                        name: "ALIEN_SANDBOX_POLICY_CANARY".into(),
                        value: Some("10.0.0.5:8080".into()),
                        value_from: None,
                    }]),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        pod.status = Some(k8s_openapi::api::core::v1::PodStatus {
            conditions: Some(ready_condition(true)),
            ..Default::default()
        });
        assert!(pod_is_ready_for(&pod, Some("10.0.0.5:8080")));
        assert!(!pod_is_ready_for(&pod, Some("10.0.0.9:8080")));
        assert!(
            !reattachable_for(&pod, Some("10.0.0.9:8080")),
            "a Ready pod whose check targeted a dead address and was never verified is not handed to a retry"
        );
        mark_policy_verified(&mut pod);
        assert!(
            reattachable_for(&pod, Some("10.0.0.9:8080")),
            "a session verified when handed out stays reachable after an Operator restart"
        );
        assert!(
            pod_is_ready_for(&pod, None),
            "no Operator address: nothing to compare"
        );
    }

    /// A running pod with the policy gate that is not Ready says why: the cluster does not enforce
    /// its NetworkPolicy. A pod without the gate, or not yet running, gets no such claim.
    #[test]
    fn a_running_gated_pod_that_is_not_ready_names_the_missing_policy_enforcement() {
        use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec};
        let mut pod = idle_pod("p", "10.0.0.7");
        pod.status.as_mut().unwrap().phase = Some("Running".into());
        pod.status.as_mut().unwrap().conditions = Some(ready_condition(false));
        assert_eq!(pod_not_ready_reason(Some(&pod)), "", "no gate, no claim");
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "p".into(),
                env: Some(vec![EnvVar {
                    name: "ALIEN_SANDBOX_POLICY_CANARY".into(),
                    value: Some("10.0.0.5:8080".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert!(pod_not_ready_reason(Some(&pod)).contains("not enforced by this cluster"));
        pod.status.as_mut().unwrap().phase = Some("Pending".into());
        assert_eq!(pod_not_ready_reason(Some(&pod)), "");
        pod.status.as_mut().unwrap().phase = Some("Running".into());
        pod.status.as_mut().unwrap().conditions = Some(ready_condition(true));
        assert_eq!(pod_not_ready_reason(Some(&pod)), "");
        assert_eq!(pod_not_ready_reason(None), "");
    }

    fn ready_condition(ready: bool) -> Vec<k8s_openapi::api::core::v1::PodCondition> {
        vec![k8s_openapi::api::core::v1::PodCondition {
            type_: "Ready".to_string(),
            status: if ready { "True" } else { "False" }.to_string(),
            ..Default::default()
        }]
    }

    /// A pool pod that exists but has not been given an address yet.
    fn idle_pod_without_address(name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                uid: Some(format!("uid-{name}")),
                labels: Some(sandbox_labels()),
                ..Default::default()
            },
            // Ready but with no address: the window between the condition and the status write
            // that carries the IP. A pod not Ready is skipped before it is claimed.
            status: Some(PodStatus {
                conditions: Some(ready_condition(true)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn key_secret() -> Secret {
        let pair = ed25519_compact::KeyPair::generate();
        Secret {
            metadata: ObjectMeta {
                labels: Some(BTreeMap::from([
                    (
                        DEPLOYMENT_LABEL_KEY.to_string(),
                        DEPLOYMENT_LABEL_VALUE.to_string(),
                    ),
                    (crate::sandbox::LABEL_SANDBOX.to_string(), "sbx".to_string()),
                ])),
                ..Default::default()
            },
            data: Some(BTreeMap::from([(
                "signingKey".to_string(),
                k8s_openapi::ByteString(pair.as_ref().to_vec()),
            )])),
            ..Default::default()
        }
    }

    fn pods_returning(items: Vec<Pod>) -> MockPodApi {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().returning(move |_, selector, _| {
            assert_eq!(
                selector.as_deref(),
                Some(
                    "alien.dev/sandbox=sbx,alien.dev/sandbox-pool=idle,alien.dev/deployment=release-a"
                )
            );
            Ok(List {
                items: items.clone(),
                metadata: Default::default(),
            })
        });
        pods
    }

    fn dns_secret(required: Option<&str>) -> Secret {
        let mut secret = key_secret();
        if let Some(value) = required {
            secret.data.as_mut().unwrap().insert(
                crate::sandbox::REQUIRES_CLUSTER_DNS_SECRET_KEY.to_string(),
                k8s_openapi::ByteString(value.as_bytes().to_vec()),
            );
        }
        secret
    }

    /// A wrong `sandbox.clusterDNS` refuses a new session on an allow sandbox, and the message
    /// names the value and the address to set. A sandbox that does not need it, and a matching
    /// value, both pass.
    #[tokio::test]
    async fn a_new_allow_session_is_refused_when_cluster_dns_does_not_match_the_resolver() {
        let resolv = "nameserver 10.100.0.10\n";
        let check = |required: Option<&'static str>, configured: Option<&'static str>| async move {
            let mut secrets = MockSecretsApi::new();
            no_reservation(&mut secrets);
            secrets
                .expect_get_secret()
                .returning(move |_, _| Ok(dns_secret(required)));
            require_cluster_dns_with(
                &(Arc::new(secrets) as Arc<dyn SecretsApi>),
                "ns",
                "alien-sandbox-sbx-capability",
                "sbx",
                configured,
                resolv,
            )
            .await
        };
        let error = check(Some("true"), Some("10.96.0.10")).await.unwrap_err();
        assert!(error.to_string().contains("sandbox.clusterDNS"), "{error}");
        assert!(error.to_string().contains("10.100.0.10"), "{error}");
        assert!(check(Some("true"), None).await.is_err());
        assert!(check(Some("true"), Some("10.100.0.10")).await.is_ok());
        assert!(check(Some("false"), Some("10.96.0.10")).await.is_ok());
        assert!(check(None, None).await.is_ok());
    }

    fn claimed_pod_with_deadline(deadline: i64) -> Pod {
        let mut claimed = idle_pod("pool-a", "10.0.0.7");
        claimed
            .metadata
            .labels
            .as_mut()
            .unwrap()
            .insert(crate::sandbox::LABEL_SESSION.to_string(), "s1".to_string());
        claimed.metadata.annotations = Some(BTreeMap::from([(
            crate::sandbox::ANNOTATION_SESSION_DEADLINE.to_string(),
            deadline.to_string(),
        )]));
        claimed
    }

    fn preview_secret(ports: &[u16]) -> Secret {
        let mut secret = key_secret();
        secret.data.as_mut().unwrap().insert(
            crate::sandbox::PREVIEW_PORTS_SECRET_KEY.to_string(),
            k8s_openapi::ByteString(serde_json::to_vec(ports).unwrap()),
        );
        secret
    }

    async fn preview(
        pod: Pod,
        secret: Secret,
        port: u16,
        now: i64,
    ) -> std::result::Result<PreviewGrant, PreviewError> {
        let mut mock = MockPodApi::new();
        mock.expect_list_pods().returning(move |_, _, _| {
            Ok(k8s_openapi::List {
                items: vec![pod.clone()],
                ..Default::default()
            })
        });
        let pods: Arc<dyn PodApi> = Arc::new(mock);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        preview_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s1",
            "key",
            now,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
            port,
        )
        .await
    }

    /// A preview capability is for one pod and one port, expires within 15 minutes and never
    /// after the session's deadline, and is minted only for a port the declaration lists.
    #[tokio::test]
    async fn a_preview_capability_is_for_one_pod_and_one_declared_port() {
        let secret = preview_secret(&[8080]);
        let public = {
            let seed = secret.data.as_ref().unwrap()["signingKey"].0.clone();
            ed25519_compact::KeyPair::from_slice(&seed).unwrap().pk
        };
        let grant = preview(
            claimed_pod_with_deadline(10_000),
            secret.clone(),
            8080,
            1_000,
        )
        .await
        .ok()
        .expect("a declared port is previewable");
        assert_eq!(
            grant.endpoint,
            format!("http://10.0.0.7:{AGENT_PORT}/v1/preview/8080")
        );
        assert_eq!(grant.expires_at, 1_000 + 900);
        let identity = |uid: &str| alien_core::sandbox_capability::SandboxSessionIdentity {
            session_id: uid.to_string(),
            generation: 1,
        };
        let verified = sandbox_capability_token::verify(
            &grant.capability,
            &public,
            &identity("uid-pool-a"),
            SandboxOperationClass::Preview,
            1_001,
        )
        .expect("verifies for this pod and the preview class");
        assert!(verified.admits_port(8080) && !verified.admits_port(8971));
        // Not for another pod, not as a shell, not after it expires.
        assert!(sandbox_capability_token::verify(
            &grant.capability,
            &public,
            &identity("uid-another-pod"),
            SandboxOperationClass::Preview,
            1_001
        )
        .is_err());
        assert!(sandbox_capability_token::verify(
            &grant.capability,
            &public,
            &identity("uid-pool-a"),
            SandboxOperationClass::Execute,
            1_001
        )
        .is_err());
        assert!(sandbox_capability_token::verify(
            &grant.capability,
            &public,
            &identity("uid-pool-a"),
            SandboxOperationClass::Preview,
            1_000 + 900
        )
        .is_err());
        // The session's own deadline caps it.
        let near_deadline = preview(claimed_pod_with_deadline(1_120), secret, 8080, 1_000)
            .await
            .ok()
            .unwrap();
        assert_eq!(near_deadline.expires_at, 1_120);
    }

    #[tokio::test]
    async fn a_preview_for_an_undeclared_port_or_the_agent_port_is_refused() {
        for port in [8081, AGENT_PORT] {
            let refused = preview(
                claimed_pod_with_deadline(10_000),
                preview_secret(&[8080, AGENT_PORT]),
                port,
                1_000,
            )
            .await;
            assert!(
                matches!(refused, Err(PreviewError::PortNotDeclared)),
                "port {port} must be refused"
            );
        }
        // No declaration at all: nothing is previewable.
        let none = preview(claimed_pod_with_deadline(10_000), key_secret(), 8080, 1_000).await;
        assert!(matches!(none, Err(PreviewError::PortNotDeclared)));
    }

    #[tokio::test]
    async fn a_preview_for_an_ended_or_unknown_session_is_not_found() {
        let ended = preview(
            claimed_pod_with_deadline(999),
            preview_secret(&[8080]),
            8080,
            1_000,
        )
        .await;
        assert!(matches!(ended, Err(PreviewError::NotFound)));
        // The listing is already scoped to the caller's deployment, so another deployment's pod
        // is never returned and the answer is the same as for an unknown id.
        let mut mock = MockPodApi::new();
        mock.expect_list_pods()
            .returning(|_, _, _| Ok(k8s_openapi::List::default()));
        let pods: Arc<dyn PodApi> = Arc::new(mock);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = preview_secret(&[8080]);
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let unknown = preview_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "nobody",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
            8080,
        )
        .await;
        assert!(matches!(unknown, Err(PreviewError::NotFound)));
    }

    #[tokio::test]
    async fn a_claim_returns_an_address_and_a_capability_for_that_session() {
        let mut pods = pods_returning(vec![idle_pod("pool-0", "10.1.2.3")]);
        pods.expect_update_pod()
            .returning(|_, _, pod| Ok(pod.clone()));

        let mut secrets = MockSecretsApi::new();

        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "alien-sandbox-sbx-capability",
            1_000,
        )
        .await
        .expect("an idle pod is claimable");

        assert_eq!(claimed.endpoint, "http://10.1.2.3:8971");
        assert_eq!(claimed.session_id, "s1");
        assert_eq!(claimed.expires_at, 1_000 + CAPABILITY_LIFETIME_SECONDS);
        assert!(!claimed.capability.is_empty());
    }

    /// The apiserver decides the race. A caller that lost one pod must take the next rather than
    /// failing a create the pool can still satisfy.
    /// A session may ask for less than the sandbox's declared lifetime and never for more; the
    /// deadline counts from the claim, so time an idle pod spent waiting is not taken from it.
    #[test]
    fn a_session_deadline_starts_at_the_claim_and_never_exceeds_the_declared_lifetime() {
        let template = || {
            let mut pod = idle_pod("pool-0", "10.1.2.3");
            pod.metadata.annotations = Some(BTreeMap::from([(
                crate::sandbox::ANNOTATION_MAX_LIFETIME.to_string(),
                "3600".to_string(),
            )]));
            pod
        };
        let deadline = |pod: &Pod| {
            pod.metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(crate::sandbox::ANNOTATION_SESSION_DEADLINE))
                .cloned()
        };

        let mut by_default = template();
        crate::sandbox::annotate_claim(&mut by_default, 1_000, None, None).unwrap();
        assert_eq!(deadline(&by_default).as_deref(), Some("4600"));

        let mut shorter = template();
        crate::sandbox::annotate_claim(&mut shorter, 1_000, None, Some(60)).unwrap();
        assert_eq!(deadline(&shorter).as_deref(), Some("1060"));

        let mut longer = template();
        let refused = crate::sandbox::annotate_claim(&mut longer, 1_000, None, Some(3601))
            .expect_err("longer than the declared lifetime");
        assert_eq!(refused.max_seconds, 3600);
        assert_eq!(deadline(&longer), None, "a refused claim writes nothing");

        assert!(crate::sandbox::session_deadline_passed(&by_default, 4_600));
        assert!(!crate::sandbox::session_deadline_passed(&by_default, 4_599));
    }

    /// The kubelet's own deadline backstops the Operator's, counted from when the pod started: a
    /// pod that waited idle for an hour still gives its session the whole lifetime, and one
    /// without a lifetime gets none.
    #[test]
    fn a_claim_sets_the_kubelet_backstop_from_the_pods_start() {
        let started = |start: i64| {
            let mut pod = idle_pod("pool-0", "10.1.2.3");
            pod.metadata.annotations = Some(BTreeMap::from([(
                crate::sandbox::ANNOTATION_MAX_LIFETIME.to_string(),
                "3600".to_string(),
            )]));
            pod.spec = Some(Default::default());
            pod.status.get_or_insert_with(Default::default).start_time =
                Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                    chrono::DateTime::from_timestamp(start, 0).unwrap(),
                ));
            pod
        };
        let mut waited = started(1_000);
        crate::sandbox::annotate_claim(&mut waited, 4_600, None, None).unwrap();
        // deadline 8_200; the pod started at 1_000; plus the grace.
        assert_eq!(
            waited.spec.unwrap().active_deadline_seconds,
            Some(8_200 - 1_000 + crate::sandbox::KUBELET_BACKSTOP_GRACE_SECONDS)
        );

        let mut unlimited = started(1_000);
        unlimited.metadata.annotations = Some(BTreeMap::new());
        crate::sandbox::annotate_claim(&mut unlimited, 4_600, None, None).unwrap();
        assert_eq!(unlimited.spec.unwrap().active_deadline_seconds, None);
    }

    /// A pod a failed claim returned to the pool keeps that claim's tenant; the next claim, which
    /// names none, must not inherit it.
    #[test]
    fn a_claim_without_a_tenant_drops_the_tenant_of_an_earlier_claim() {
        let mut pod = idle_pod("pool-0", "10.1.2.3");
        crate::sandbox::annotate_claim(&mut pod, 1_000, Some("old"), None).unwrap();
        crate::sandbox::annotate_claim(&mut pod, 2_000, None, None).unwrap();
        let annotations = pod.metadata.annotations.as_ref().unwrap();
        assert_eq!(
            annotations.get(crate::sandbox::ANNOTATION_SESSION_TENANT),
            None
        );
    }

    /// The pod is the only record of a session: its creation time, generation and tenant come
    /// from annotations the claim wrote, so a process that did not create it reads the same.
    #[tokio::test]
    async fn the_claim_records_the_session_on_the_pod_and_another_process_reads_it_back() {
        let mut pods = pods_returning(vec![idle_pod("pool-0", "10.1.2.3")]);
        let written = Arc::new(std::sync::Mutex::new(None));
        let written_pod = Arc::clone(&written);
        pods.expect_update_pod().returning(move |_, _, pod| {
            *written_pod.lock().unwrap() = Some(pod.clone());
            Ok(pod.clone())
        });
        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = key_secret();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);

        let claimed = claim_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s1",
            "key",
            4_242,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
            Some("tenant-a"),
            Some(600),
        )
        .await
        .expect("claim");
        assert!(claimed.created);
        assert_eq!(claimed.created_at, 4_242);

        let pod = written
            .lock()
            .unwrap()
            .clone()
            .expect("the claim wrote the pod");
        let (created_at, generation, tenant) = recorded(&pod, 0);
        assert_eq!(
            (created_at, generation, tenant.as_deref()),
            (4_242, 1, Some("tenant-a"))
        );
        // The deadline starts at the claim, not when the pod was built.
        assert_eq!(
            pod.metadata.annotations.as_ref().unwrap()[crate::sandbox::ANNOTATION_SESSION_DEADLINE],
            (4_242 + 600).to_string()
        );
    }

    /// A retry for a session whose pod has not proven its policy yet (not Ready) must not be
    /// handed the pod: only the creating call waited for Ready.
    #[tokio::test]
    async fn a_session_whose_pod_is_not_ready_is_not_handed_out() {
        let mut claimed = idle_pod("pool-a", "10.0.0.7");
        claimed
            .metadata
            .labels
            .as_mut()
            .unwrap()
            .insert(crate::sandbox::LABEL_SESSION.to_string(), "s1".to_string());
        claimed.status.as_mut().unwrap().conditions = Some(ready_condition(false));
        crate::sandbox::annotate_claim(&mut claimed, 77, None, None).unwrap();
        let mut mock = MockPodApi::new();
        mock.expect_list_pods().returning(move |_, _, _| {
            Ok(k8s_openapi::List {
                items: vec![claimed.clone()],
                ..Default::default()
            })
        });
        let pods: Arc<dyn PodApi> = Arc::new(mock);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = key_secret();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let result = reattach_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s1",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await;
        assert!(result.is_err(), "a pod that is not Ready is not returned");
    }

    #[tokio::test]
    async fn a_session_claimed_elsewhere_is_reattached_with_a_fresh_capability() {
        let mut claimed = idle_pod("pool-a", "10.0.0.7");
        claimed
            .metadata
            .labels
            .as_mut()
            .unwrap()
            .insert(crate::sandbox::LABEL_SESSION.to_string(), "s1".to_string());
        crate::sandbox::annotate_claim(&mut claimed, 77, Some("t1"), None).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_selector = Arc::clone(&seen);
        let mut mock = MockPodApi::new();
        mock.expect_list_pods().returning(move |_, selector, _| {
            seen_selector.lock().unwrap().push(selector.unwrap());
            Ok(k8s_openapi::List {
                items: vec![claimed.clone()],
                ..Default::default()
            })
        });
        let pods: Arc<dyn PodApi> = Arc::new(mock);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = key_secret();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);

        let session = reattach_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s1",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await
        .expect("lookup works")
        .expect("the session exists");

        assert_eq!(session.session_id, "s1");
        assert_eq!(session.endpoint, format!("http://10.0.0.7:{AGENT_PORT}"));
        assert_eq!(session.expires_at, 1_000 + CAPABILITY_LIFETIME_SECONDS);
        assert_eq!(
            (session.created, session.created_at, session.generation),
            (false, 77, 1),
            "a reattach reports what the creating process recorded, and did not create it"
        );
        assert_eq!(session.tenant_key.as_deref(), Some("t1"));
        let selector = seen.lock().unwrap()[0].clone();
        assert!(
            selector.contains("alien.dev/sandbox-session=s1")
                && selector.contains(&format!("{DEPLOYMENT_LABEL_KEY}={DEPLOYMENT_LABEL_VALUE}")),
            "the lookup is scoped to the session and the deployment: {selector}"
        );
    }

    /// A cluster in memory that behaves like the API server where it matters here: a second create
    /// of a name is a conflict, and a write that carries an old resourceVersion is a conflict.
    /// Every call yields, so concurrent callers interleave.
    #[derive(Debug, Default)]
    struct FakeCluster {
        pods: std::sync::Mutex<Vec<Pod>>,
        secrets: std::sync::Mutex<BTreeMap<String, Secret>>,
        versions: std::sync::Mutex<u64>,
        uids: std::sync::atomic::AtomicUsize,
        /// Makes every Secret update fail, as an API server error would.
        fail_secret_updates: std::sync::atomic::AtomicBool,
    }

    fn conflict(name: &str) -> alien_error::AlienError<alien_client_core::ErrorData> {
        alien_error::AlienError::new(alien_client_core::ErrorData::RemoteResourceConflict {
            resource_type: "object".to_string(),
            resource_name: name.to_string(),
            message: "already exists or changed".to_string(),
        })
    }

    #[async_trait::async_trait]
    impl PodApi for FakeCluster {
        async fn create_pod(&self, _: &str, pod: &Pod) -> alien_client_core::Result<Pod> {
            tokio::task::yield_now().await;
            self.pods.lock().unwrap().push(pod.clone());
            Ok(pod.clone())
        }
        async fn get_pod(&self, _: &str, name: &str) -> alien_client_core::Result<Pod> {
            tokio::task::yield_now().await;
            self.pods
                .lock()
                .unwrap()
                .iter()
                .find(|pod| pod.metadata.name.as_deref() == Some(name))
                .cloned()
                .ok_or_else(|| conflict(name))
        }
        async fn list_pods(
            &self,
            _: &str,
            selector: Option<String>,
            _: Option<String>,
        ) -> alien_client_core::Result<List<Pod>> {
            tokio::task::yield_now().await;
            let wanted: Vec<(String, Option<String>)> = selector
                .unwrap_or_default()
                .split(',')
                .filter(|term| !term.is_empty())
                .map(|term| match term.split_once('=') {
                    Some((key, value)) => (key.to_string(), Some(value.to_string())),
                    None => (term.to_string(), None),
                })
                .collect();
            let items = self
                .pods
                .lock()
                .unwrap()
                .iter()
                .filter(|pod| {
                    let labels = pod.metadata.labels.clone().unwrap_or_default();
                    wanted.iter().all(|(key, value)| match value {
                        Some(value) => labels.get(key) == Some(value),
                        None => labels.contains_key(key),
                    })
                })
                .cloned()
                .collect();
            Ok(List {
                items,
                ..Default::default()
            })
        }
        async fn update_pod(
            &self,
            _: &str,
            name: &str,
            pod: &Pod,
        ) -> alien_client_core::Result<Pod> {
            tokio::task::yield_now().await;
            let mut pods = self.pods.lock().unwrap();
            let current = pods
                .iter_mut()
                .find(|current| current.metadata.name.as_deref() == Some(name))
                .ok_or_else(|| conflict(name))?;
            if current.metadata.resource_version != pod.metadata.resource_version {
                return Err(conflict(name));
            }
            let mut next = pod.clone();
            let mut version = self.versions.lock().unwrap();
            *version += 1;
            next.metadata.resource_version = Some(version.to_string());
            *current = next.clone();
            Ok(next)
        }
        async fn delete_pod(&self, _: &str, name: &str) -> alien_client_core::Result<()> {
            self.pods
                .lock()
                .unwrap()
                .retain(|pod| pod.metadata.name.as_deref() != Some(name));
            Ok(())
        }
        async fn delete_pod_if_uid(
            &self,
            namespace: &str,
            name: &str,
            _: &str,
        ) -> alien_client_core::Result<()> {
            self.delete_pod(namespace, name).await
        }
    }

    #[async_trait::async_trait]
    impl SecretsApi for FakeCluster {
        async fn create_secret(
            &self,
            _: &str,
            secret: &Secret,
        ) -> alien_client_core::Result<Secret> {
            tokio::task::yield_now().await;
            let name = secret.metadata.name.clone().unwrap_or_default();
            let mut secrets = self.secrets.lock().unwrap();
            if secrets.contains_key(&name) {
                return Err(conflict(&name));
            }
            let mut stored = secret.clone();
            stored.metadata.resource_version = Some("1".to_string());
            stored.metadata.uid = Some(format!(
                "uid-{}",
                self.uids.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
            secrets.insert(name, stored.clone());
            Ok(stored)
        }
        async fn get_secret(&self, _: &str, name: &str) -> alien_client_core::Result<Secret> {
            tokio::task::yield_now().await;
            if name == "key" {
                return Ok(key_secret());
            }
            self.secrets
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| conflict(name))
        }
        async fn get_secret_metadata(
            &self,
            _: &str,
            name: &str,
        ) -> alien_client_core::Result<ObjectMeta> {
            tokio::task::yield_now().await;
            self.secrets
                .lock()
                .unwrap()
                .get(name)
                .map(|secret| secret.metadata.clone())
                .ok_or_else(|| {
                    alien_error::AlienError::new(
                        alien_client_core::ErrorData::RemoteResourceNotFound {
                            resource_type: "secret".to_string(),
                            resource_name: name.to_string(),
                        },
                    )
                })
        }
        async fn list_secrets(
            &self,
            _: &str,
            _: Option<String>,
            _: Option<String>,
        ) -> alien_client_core::Result<List<Secret>> {
            Ok(List {
                items: self.secrets.lock().unwrap().values().cloned().collect(),
                ..Default::default()
            })
        }
        async fn update_secret(
            &self,
            _: &str,
            name: &str,
            secret: &Secret,
        ) -> alien_client_core::Result<Secret> {
            if self
                .fail_secret_updates
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(conflict(name));
            }
            let mut secrets = self.secrets.lock().unwrap();
            // Like the API server: an update carries the resourceVersion it read, and loses to
            // any change made since.
            let current = secrets
                .get(name)
                .and_then(|stored| stored.metadata.resource_version.clone());
            if current != secret.metadata.resource_version {
                return Err(conflict(name));
            }
            let mut stored = secret.clone();
            stored.metadata.resource_version = Some(format!(
                "{}",
                current.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1
            ));
            secrets.insert(name.to_string(), stored.clone());
            Ok(stored)
        }
        async fn delete_secret_if_uid(
            &self,
            _: &str,
            name: &str,
            uid: &str,
        ) -> alien_client_core::Result<()> {
            tokio::task::yield_now().await;
            let mut secrets = self.secrets.lock().unwrap();
            if secrets
                .get(name)
                .is_some_and(|secret| secret.metadata.uid.as_deref() == Some(uid))
            {
                secrets.remove(name);
            }
            Ok(())
        }
        async fn delete_secret(&self, _: &str, name: &str) -> alien_client_core::Result<()> {
            tokio::task::yield_now().await;
            self.secrets.lock().unwrap().remove(name);
            Ok(())
        }
    }

    fn stored_reservation(
        cluster: &FakeCluster,
        sandbox: &str,
        session: &str,
        uid: &str,
        reserved_at: i64,
        annotations: &[&str],
    ) -> String {
        let name = reservation_name(sandbox, session);
        let mut map =
            BTreeMap::from([(ANNOTATION_RESERVED_AT.to_string(), reserved_at.to_string())]);
        for key in annotations {
            map.insert(key.to_string(), "true".to_string());
        }
        cluster.secrets.lock().unwrap().insert(
            name.clone(),
            Secret {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    uid: Some(uid.to_string()),
                    labels: Some(BTreeMap::from([(
                        crate::sandbox::LABEL_SANDBOX.to_string(),
                        sandbox.to_string(),
                    )])),
                    annotations: Some(map),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        name
    }

    /// Ending a session must not clear the reservation of a concurrent create of the same id that
    /// is still claiming: its pod would be followed by a second one for the id.
    #[tokio::test]
    async fn ending_a_session_leaves_a_creator_that_is_still_claiming_its_reservation() {
        let cluster = Arc::new(FakeCluster::default());
        let still_claiming = stored_reservation(
            &cluster,
            "sbx",
            "s1",
            "uid-claiming",
            1_000,
            &[ANNOTATION_SETUP_PENDING],
        );
        let claimed = stored_reservation(
            &cluster,
            "sbx",
            "s2",
            "uid-claimed",
            1_000,
            &[ANNOTATION_SETUP_PENDING, ANNOTATION_CLAIMED],
        );
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        release_claimed_reservation(&secrets, "ns", "sbx", "s1").await;
        release_claimed_reservation(&secrets, "ns", "sbx", "s2").await;
        let held = cluster.secrets.lock().unwrap();
        assert!(
            held.contains_key(&still_claiming),
            "a claiming creator keeps its hold"
        );
        assert!(
            !held.contains_key(&claimed),
            "a claimed session's reservation is let go"
        );
    }

    /// A creator that lets go late must not delete the reservation a later creator took under the
    /// same name.
    #[tokio::test]
    async fn letting_go_deletes_only_the_object_the_creator_created() {
        let cluster = Arc::new(FakeCluster::default());
        let name = stored_reservation(&cluster, "sbx", "s1", "uid-new", 1_000, &[]);
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        release_reservation(&secrets, "ns", "sbx", "s1", Some("uid-old")).await;
        assert!(cluster.secrets.lock().unwrap().contains_key(&name));
        release_reservation(&secrets, "ns", "sbx", "s1", Some("uid-new")).await;
        assert!(!cluster.secrets.lock().unwrap().contains_key(&name));
    }

    /// A reservation a client never reported ready is deleted once its window has passed; a recent
    /// one, and a Secret that is not a reservation, are left alone.
    #[tokio::test]
    async fn the_sweep_deletes_abandoned_reservations_only() {
        let cluster = Arc::new(FakeCluster::default());
        let abandoned = stored_reservation(
            &cluster,
            "sbx",
            "old",
            "u1",
            1_000,
            &[ANNOTATION_SETUP_PENDING, ANNOTATION_CLAIMED],
        );
        let recent = stored_reservation(
            &cluster,
            "sbx",
            "new",
            "u2",
            1_000 + SETUP_WINDOW_SECONDS,
            &[ANNOTATION_SETUP_PENDING],
        );
        cluster.secrets.lock().unwrap().insert(
            "alien-sandbox-sbx-capability".to_string(),
            Secret {
                metadata: ObjectMeta {
                    name: Some("alien-sandbox-sbx-capability".to_string()),
                    uid: Some("u3".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let deleted =
            sweep_abandoned_reservations(&secrets, "ns", "sbx", 1_000 + SETUP_WINDOW_SECONDS + 1)
                .await
                .unwrap();
        assert_eq!(deleted, 1);
        let held = cluster.secrets.lock().unwrap();
        assert!(!held.contains_key(&abandoned));
        assert!(held.contains_key(&recent));
        assert!(held.contains_key("alien-sandbox-sbx-capability"));
    }

    /// A cold create can take longer than the setup window. The window counts from the claim, so
    /// a session whose claim finished at +300 s is still being set up at +301 s and the sweep
    /// leaves its reservation alone; counted from the reservation both would already be over.
    #[tokio::test]
    async fn the_setup_window_starts_at_the_claim_not_at_the_reservation() {
        let cluster = Arc::new(FakeCluster::default());
        let name = stored_reservation(
            &cluster,
            "sbx",
            "slow",
            "uid-slow",
            1_000,
            &[ANNOTATION_SETUP_PENDING],
        );
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let claimed_at = 1_000 + 300;
        mark_reservation_claimed(&secrets, "ns", "sbx", "slow", Some("uid-slow"), claimed_at)
            .await
            .unwrap();

        let soon = claimed_at + 1;
        assert!(setup_pending(&secrets, "ns", "sbx", "slow", soon)
            .await
            .unwrap());
        assert_eq!(
            sweep_abandoned_reservations(&secrets, "ns", "sbx", soon)
                .await
                .unwrap(),
            0
        );
        assert!(cluster.secrets.lock().unwrap().contains_key(&name));

        let late = claimed_at + SETUP_WINDOW_SECONDS + 1;
        assert!(!setup_pending(&secrets, "ns", "sbx", "slow", late)
            .await
            .unwrap());
        assert_eq!(
            sweep_abandoned_reservations(&secrets, "ns", "sbx", late)
                .await
                .unwrap(),
            1
        );
    }

    /// A reservation still being claimed is judged by the reservation's lifetime, not the setup
    /// window: a cold create holds it for as long as it takes.
    #[tokio::test]
    async fn an_unclaimed_reservation_is_swept_only_after_the_reservation_lifetime() {
        let cluster = Arc::new(FakeCluster::default());
        let name = stored_reservation(
            &cluster,
            "sbx",
            "claiming",
            "uid-c",
            1_000,
            &[ANNOTATION_SETUP_PENDING],
        );
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let during_cold_create = 1_000 + SETUP_WINDOW_SECONDS + 100;
        assert!(
            setup_pending(&secrets, "ns", "sbx", "claiming", during_cold_create)
                .await
                .unwrap()
        );
        assert_eq!(
            sweep_abandoned_reservations(&secrets, "ns", "sbx", during_cold_create)
                .await
                .unwrap(),
            0
        );
        assert!(cluster.secrets.lock().unwrap().contains_key(&name));
        assert_eq!(
            sweep_abandoned_reservations(
                &secrets,
                "ns",
                "sbx",
                1_000 + RESERVATION_TTL_SECONDS + 1
            )
            .await
            .unwrap(),
            1
        );
    }

    /// When the creator cannot record its claim, the create fails and leaves nothing behind: no
    /// pod nobody holds, no reservation that nothing releases.
    #[tokio::test]
    async fn a_create_that_cannot_record_its_claim_fails_and_leaves_nothing() {
        let cluster = Arc::new(FakeCluster::default());
        let mut pod = idle_pod("pool-0", "10.0.0.1");
        pod.metadata.resource_version = Some("1".to_string());
        cluster.pods.lock().unwrap().push(pod);
        cluster
            .fail_secret_updates
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let pods: Arc<dyn PodApi> = cluster.clone();
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let result = find_or_create_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s1",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
            None,
            None,
            true,
        )
        .await;
        assert!(result.is_err(), "the create must not succeed");
        assert!(
            cluster.secrets.lock().unwrap().is_empty(),
            "the reservation is let go"
        );
        let carrying_session = cluster
            .pods
            .lock()
            .unwrap()
            .iter()
            .filter(|pod| {
                pod.metadata
                    .labels
                    .as_ref()
                    .is_some_and(|labels| labels.contains_key(crate::sandbox::LABEL_SESSION))
            })
            .count();
        assert_eq!(carrying_session, 0, "the claimed pod was ended");
    }

    /// Several processes asking for one new id at the same moment all get one pod: the id is
    /// created once, by one of them, and the rest find it.
    #[tokio::test]
    async fn concurrent_creates_of_one_id_resolve_to_one_pod() {
        let cluster = Arc::new(FakeCluster::default());
        for index in 0..4 {
            let mut pod = idle_pod(&format!("pool-{index}"), &format!("10.0.0.{index}"));
            pod.metadata.resource_version = Some("1".to_string());
            cluster.pods.lock().unwrap().push(pod);
        }
        let pods: Arc<dyn PodApi> = cluster.clone();
        let secrets: Arc<dyn SecretsApi> = cluster.clone();

        let create = || {
            let (pods, secrets) = (pods.clone(), secrets.clone());
            async move {
                find_or_create_session_for_deployment(
                    &pods,
                    &secrets,
                    "sbx",
                    "alien",
                    "same-id",
                    "key",
                    1_000,
                    DEPLOYMENT_LABEL_KEY,
                    DEPLOYMENT_LABEL_VALUE,
                    None,
                    None,
                    false,
                )
                .await
                .expect("create or find")
            }
        };
        let results = futures::future::join_all((0..4).map(|_| create())).await;

        let uids: std::collections::BTreeSet<_> = results
            .iter()
            .map(|session| session.agent_session_id.clone())
            .collect();
        assert_eq!(uids.len(), 1, "one id, one pod: {uids:?}");
        assert_eq!(
            results.iter().filter(|session| session.created).count(),
            1,
            "exactly one caller created it"
        );
        let held = cluster
            .pods
            .lock()
            .unwrap()
            .iter()
            .filter(|pod| {
                pod.metadata
                    .labels
                    .as_ref()
                    .is_some_and(|labels| labels.get(crate::sandbox::LABEL_SESSION).is_some())
            })
            .count();
        assert_eq!(held, 1, "no second pod was claimed for the id");
        assert!(
            cluster.secrets.lock().unwrap().is_empty(),
            "the reservation is let go once the pod carries the session"
        );
    }

    /// An existing session answers only to the tenant key it was created with, including when
    /// the caller lost the create race.
    #[tokio::test]
    async fn a_session_with_another_tenant_key_is_refused_by_the_broker() {
        let cluster = Arc::new(FakeCluster::default());
        let mut pod = idle_pod("pool-0", "10.0.0.1");
        pod.metadata.resource_version = Some("1".to_string());
        cluster.pods.lock().unwrap().push(pod);
        let pods: Arc<dyn PodApi> = cluster.clone();
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let call = |tenant: Option<&'static str>| {
            let (pods, secrets) = (pods.clone(), secrets.clone());
            async move {
                find_or_create_session_for_deployment(
                    &pods,
                    &secrets,
                    "sbx",
                    "alien",
                    "s1",
                    "key",
                    1_000,
                    DEPLOYMENT_LABEL_KEY,
                    DEPLOYMENT_LABEL_VALUE,
                    tenant,
                    None,
                    false,
                )
                .await
            }
        };

        assert!(call(Some("a")).await.expect("creates").created);
        assert!(call(Some("a")).await.is_ok());
        for other in [Some("b"), None] {
            assert!(
                matches!(call(other).await, Err(SessionRequestError::TenantMismatch)),
                "{other:?} must be refused"
            );
        }
    }

    /// A caller that finds a session while its creator is still setting up its egress is told so,
    /// and is told it is ready once the creator reports it, or when the session ends.
    #[tokio::test]
    async fn a_session_is_pending_for_other_callers_until_its_creator_reports_the_setup() {
        let cluster = Arc::new(FakeCluster::default());
        let mut pod = idle_pod("pool-0", "10.0.0.1");
        pod.metadata.resource_version = Some("1".to_string());
        cluster.pods.lock().unwrap().push(pod);
        let pods: Arc<dyn PodApi> = cluster.clone();
        let secrets: Arc<dyn SecretsApi> = cluster.clone();
        let find = |hold: bool| {
            find_or_create_session_for_deployment(
                &pods,
                &secrets,
                "sbx",
                "alien",
                "s1",
                "key",
                1_000,
                DEPLOYMENT_LABEL_KEY,
                DEPLOYMENT_LABEL_VALUE,
                None,
                None,
                hold,
            )
        };

        let created = find(true).await.expect("creates");
        assert!(created.created);
        let waiting = find(false).await.expect("finds");
        assert!(!waiting.created);
        assert!(waiting.setup_pending, "the creator has not reported yet");

        finish_session_setup_for_deployment(
            &secrets,
            "alien",
            "sbx",
            "s1",
            "key",
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await
        .unwrap();
        assert!(!find(false).await.expect("finds").setup_pending);

        // A creator that never asked to set the session up leaves nothing to wait for.
        let mut other = idle_pod("pool-1", "10.0.0.2");
        other.metadata.resource_version = Some("1".to_string());
        cluster.pods.lock().unwrap().push(other);
        let plain = find_or_create_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "s2",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
            None,
            None,
            false,
        )
        .await
        .unwrap();
        assert!(plain.created && !plain.setup_pending);
    }

    /// A pod that finished keeps its address, and one past its deadline is only ended on the
    /// Operator's next tick; neither is a session a caller can be handed.
    #[tokio::test]
    async fn a_finished_or_expired_pod_is_not_a_session() {
        let claimed = |name: &str| {
            let mut pod = idle_pod(name, "10.0.0.7");
            pod.metadata
                .labels
                .as_mut()
                .unwrap()
                .insert(crate::sandbox::LABEL_SESSION.to_string(), "s1".to_string());
            crate::sandbox::annotate_claim(&mut pod, 100, None, Some(600)).unwrap();
            pod
        };
        let mut succeeded = claimed("done");
        succeeded.status.as_mut().unwrap().phase = Some("Succeeded".to_string());
        let mut failed = claimed("failed");
        failed.status.as_mut().unwrap().phase = Some("Failed".to_string());
        let expired = claimed("expired"); // deadline 700
        let mut running = claimed("alive");
        running.status.as_mut().unwrap().phase = Some("Running".to_string());

        let lookup = |items: Vec<Pod>, now: i64| async move {
            let mut mock = MockPodApi::new();
            mock.expect_list_pods().returning(move |_, _, _| {
                Ok(k8s_openapi::List {
                    items: items.clone(),
                    ..Default::default()
                })
            });
            let pods: Arc<dyn PodApi> = Arc::new(mock);
            let mut secrets = MockSecretsApi::new();
            no_reservation(&mut secrets);
            let secret = key_secret();
            secrets
                .expect_get_secret()
                .returning(move |_, _| Ok(secret.clone()));
            let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
            let reattached = reattach_session_for_deployment(
                &pods,
                &secrets,
                "sbx",
                "alien",
                "s1",
                "key",
                now,
                DEPLOYMENT_LABEL_KEY,
                DEPLOYMENT_LABEL_VALUE,
            )
            .await
            .expect("lookup works");
            let listed = list_sessions_for_deployment(
                &pods,
                &secrets,
                "sbx",
                "alien",
                "key",
                now,
                DEPLOYMENT_LABEL_KEY,
                DEPLOYMENT_LABEL_VALUE,
            )
            .await
            .expect("list works");
            (reattached.is_some(), listed.len())
        };

        assert_eq!(lookup(vec![succeeded, failed], 200).await, (false, 0));
        assert_eq!(lookup(vec![expired], 800).await, (false, 0));
        assert_eq!(lookup(vec![running], 200).await, (true, 1));
    }

    #[tokio::test]
    async fn an_unknown_session_is_not_found_rather_than_an_error() {
        let mut mock = MockPodApi::new();
        mock.expect_list_pods().returning(|_, _, _| {
            Ok(k8s_openapi::List {
                items: vec![],
                ..Default::default()
            })
        });
        let pods: Arc<dyn PodApi> = Arc::new(mock);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = key_secret();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);

        let session = reattach_session_for_deployment(
            &pods,
            &secrets,
            "sbx",
            "alien",
            "missing",
            "key",
            1_000,
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await
        .expect("an absent session is not an error");
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn losing_the_race_on_one_pod_moves_to_the_next() {
        let mut pods = pods_returning(vec![
            idle_pod("pool-0", "10.1.2.3"),
            idle_pod("pool-1", "10.1.2.4"),
        ]);
        pods.expect_update_pod().returning(|_, name, pod| {
            if name == "pool-0" {
                Err(AlienError::new(
                    alien_client_core::ErrorData::GenericError {
                        message: "conflict".to_string(),
                    },
                ))
            } else {
                Ok(pod.clone())
            }
        });

        let mut secrets = MockSecretsApi::new();

        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect("the second pod is claimable");

        assert_eq!(claimed.endpoint, "http://10.1.2.4:8971");
    }

    fn secret_with_template(template: &Pod) -> Secret {
        let mut secret = key_secret();
        secret.data.as_mut().unwrap().insert(
            crate::sandbox::POD_TEMPLATE_SECRET_KEY.to_string(),
            k8s_openapi::ByteString(serde_json::to_vec(template).unwrap()),
        );
        secret
    }

    /// A create never fails because the pool is empty: the broker builds the pod from the
    /// published template, already claimed, and returns once it has an address.
    #[tokio::test(start_paused = true)]
    async fn an_empty_pool_creates_a_pod_cold() {
        let mut template = idle_pod("ignored", "0.0.0.0");
        template.metadata.name = None;
        template.metadata.uid = None;
        template.status = None;
        template.metadata.labels = Some(BTreeMap::from([
            (crate::sandbox::LABEL_SANDBOX.to_string(), "sbx".to_string()),
            (
                crate::sandbox::LABEL_POOL_STATE.to_string(),
                crate::sandbox::POOL_STATE_IDLE.to_string(),
            ),
            (
                DEPLOYMENT_LABEL_KEY.to_string(),
                DEPLOYMENT_LABEL_VALUE.to_string(),
            ),
        ]));
        let mut pods = pods_returning(Vec::new());
        pods.expect_create_pod().times(1).returning(|_, pod| {
            let labels = pod.metadata.labels.clone().unwrap();
            assert_eq!(
                labels
                    .get(crate::sandbox::LABEL_POOL_STATE)
                    .map(String::as_str),
                Some(crate::sandbox::POOL_STATE_CLAIMED),
                "a cold pod is born claimed so no other caller can take it"
            );
            assert_eq!(
                labels
                    .get(crate::sandbox::LABEL_SESSION)
                    .map(String::as_str),
                Some("s1")
            );
            let mut created = pod.clone();
            created.metadata.name = Some("cold-0".to_string());
            created.metadata.uid = Some("uid-cold-0".to_string());
            Ok(created)
        });
        pods.expect_update_pod().times(1).returning(|_, _, pod| {
            assert!(
                policy_verified(pod),
                "a cold pod handed out is marked as verified"
            );
            Ok(pod.clone())
        });
        let mut polls = 0;
        pods.expect_get_pod().returning(move |_, _| {
            polls += 1;
            let mut pod = idle_pod("cold-0", "10.9.9.9");
            pod.status = Some(PodStatus {
                phase: Some(if polls < 3 { "Pending" } else { "Running" }.to_string()),
                pod_ip: (polls >= 3).then(|| "10.9.9.9".to_string()),
                conditions: Some(ready_condition(polls >= 3)),
                ..Default::default()
            });
            Ok(pod)
        });
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        let secret = secret_with_template(&template);
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(secret.clone()));

        let session = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect("an empty pool still yields a session");

        assert_eq!(session.endpoint, format!("http://10.9.9.9:{AGENT_PORT}"));
        assert_eq!(session.agent_session_id, "uid-cold-0");
    }

    /// A pod that comes up after the session's lifetime is not returned: the Operator would end
    /// it at once. The create reports it and removes the pod.
    #[tokio::test(start_paused = true)]
    async fn a_cold_create_that_outlives_its_lifetime_is_an_error_and_removes_the_pod() {
        let mut template = idle_pod("ignored", "0.0.0.0");
        template.metadata.name = None;
        template.metadata.uid = None;
        template.status = None;
        let mut pods = pods_returning(Vec::new());
        pods.expect_create_pod().times(1).returning(|_, pod| {
            let mut created = pod.clone();
            created.metadata.name = Some("cold-0".to_string());
            created.metadata.uid = Some("uid-cold-0".to_string());
            Ok(created)
        });
        // The pod reaches Running only after the 3 s lifetime is over.
        let started = std::time::Instant::now();
        pods.expect_get_pod().returning(move |_, _| {
            let mut pod = idle_pod("cold-0", "10.9.9.9");
            let up = started.elapsed() >= std::time::Duration::from_secs(10);
            pod.status = Some(PodStatus {
                phase: Some(if up { "Running" } else { "Pending" }.to_string()),
                pod_ip: up.then(|| "10.9.9.9".to_string()),
                conditions: Some(ready_condition(up)),
                ..Default::default()
            });
            Ok(pod)
        });
        pods.expect_delete_pod().times(1).returning(|_, _| Ok(()));
        let pods: Arc<dyn PodApi> = Arc::new(pods);

        let error = cold_create(
            &pods,
            &ed25519_compact::KeyPair::generate().sk,
            "ns",
            "sbx",
            "s1",
            "k",
            1_000,
            template,
            None,
            Some(3),
        )
        .await
        .expect_err("the pod was not running before the lifetime ended");
        assert!(format!("{error:?}").contains("lifetime ended"));
    }

    /// Before the controller's first tick there is no template; the caller is told to retry.
    #[tokio::test]
    async fn an_empty_pool_without_a_template_asks_to_retry() {
        let pods = pods_returning(Vec::new());
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let error = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect_err("no template means no pod to build");

        assert!(format!("{error:?}").contains("health tick"));
    }

    /// A pod whose NetworkPolicy is not proven in force yet is not Ready and is not handed out,
    /// even though it is idle, running and has an address: on a fresh node its egress may still
    /// be unrestricted. It is not claimed (no label write), so it stays in the pool.
    #[tokio::test]
    async fn an_idle_pod_that_is_not_ready_is_not_claimed() {
        let mut waiting = idle_pod("pool-0", "10.1.2.3");
        waiting.status.as_mut().unwrap().conditions = Some(ready_condition(false));
        let mut pods = pods_returning(vec![waiting]);
        pods.expect_update_pod().never();
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let error = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect_err("the only idle pod is not Ready, and there is no template to go cold");
        assert!(format!("{error:?}").contains("health tick"));
    }

    /// A cold pod that is Running with an address but not Ready (its policy is not proven yet)
    /// is waited for, not returned.
    #[tokio::test(start_paused = true)]
    async fn a_cold_pod_is_returned_only_once_it_is_ready() {
        let mut template = idle_pod("ignored", "0.0.0.0");
        template.metadata.name = None;
        template.metadata.uid = None;
        template.status = None;
        let mut pods = pods_returning(Vec::new());
        pods.expect_create_pod().times(1).returning(|_, pod| {
            let mut created = pod.clone();
            created.metadata.name = Some("cold-0".to_string());
            created.metadata.uid = Some("uid-cold-0".to_string());
            Ok(created)
        });
        pods.expect_update_pod()
            .returning(|_, _, pod| Ok(pod.clone()));
        let polls = Arc::new(std::sync::Mutex::new(0));
        let seen = Arc::clone(&polls);
        pods.expect_get_pod().returning(move |_, _| {
            let mut count = seen.lock().unwrap();
            *count += 1;
            let mut pod = idle_pod("cold-0", "10.9.9.9");
            pod.status = Some(PodStatus {
                phase: Some("Running".to_string()),
                pod_ip: Some("10.9.9.9".to_string()),
                conditions: Some(ready_condition(*count >= 4)),
                ..Default::default()
            });
            Ok(pod)
        });
        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let session = cold_create(
            &pods,
            &ed25519_compact::KeyPair::generate().sk,
            "ns",
            "sbx",
            "s1",
            "k",
            1_000,
            template,
            None,
            None,
        )
        .await
        .expect("returned once Ready");
        assert_eq!(session.endpoint, "http://10.9.9.9:8971");
        assert_eq!(*polls.lock().unwrap(), 4, "it kept polling while not Ready");
    }

    /// The capability the broker mints has to be the one the agent accepts. Verifying it here
    /// against the public half is what stops the two drifting into a working mint the agent
    /// refuses.
    #[tokio::test]
    async fn the_minted_capability_verifies_against_the_public_half() {
        let pair = ed25519_compact::KeyPair::generate();
        let stored = pair.as_ref().to_vec();

        let sandbox = alien_core::Sandbox::new("sbx".into())
            .code(alien_core::SandboxCode::Image {
                image: "python:3.13-slim".into(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
            })
            .build();
        let mut pod = idle_pod("pool-0", "10.1.2.3");
        pod.spec = crate::sandbox::kubernetes_spec::idle_pool_pod(
            &sandbox,
            "ns",
            "gvisor",
            None,
            Some(&BASE64.encode(*pair.pk)),
        )
        .spec;
        // Resolve the actual emitted environment as the kubelet does. A missing required
        // identity, a logical claim label, or the wrong public key must fail this exchange.
        let environment: BTreeMap<_, _> = pod.spec.as_ref().unwrap().containers[0]
            .env
            .as_ref()
            .unwrap()
            .iter()
            .map(|env| {
                let value = env.value.clone().unwrap_or_else(|| {
                    let field = &env
                        .value_from
                        .as_ref()
                        .unwrap()
                        .field_ref
                        .as_ref()
                        .unwrap()
                        .field_path;
                    match field.as_str() {
                        "metadata.uid" => pod.metadata.uid.clone().unwrap(),
                        _ => panic!("unexpected identity field {field}"),
                    }
                });
                (env.name.clone(), value)
            })
            .collect();
        let agent_identity = alien_core::sandbox_capability::SandboxSessionIdentity {
            session_id: environment["ALIEN_SANDBOX_SESSION_ID"].clone(),
            generation: environment["ALIEN_SANDBOX_GENERATION"].parse().unwrap(),
        };
        let agent_key = ed25519_compact::PublicKey::from_slice(
            &BASE64
                .decode(&environment["ALIEN_SANDBOX_PUBLIC_KEY"])
                .unwrap(),
        )
        .unwrap();
        let mut pods = pods_returning(vec![pod]);
        pods.expect_update_pod()
            .returning(|_, _, pod| Ok(pod.clone()));

        let mut secrets = MockSecretsApi::new();

        no_reservation(&mut secrets);
        secrets.expect_get_secret().returning(move |_, _| {
            Ok(Secret {
                metadata: ObjectMeta {
                    labels: Some(BTreeMap::from([
                        (
                            DEPLOYMENT_LABEL_KEY.to_string(),
                            DEPLOYMENT_LABEL_VALUE.to_string(),
                        ),
                        (crate::sandbox::LABEL_SANDBOX.to_string(), "sbx".to_string()),
                    ])),
                    ..Default::default()
                },
                data: Some(BTreeMap::from([(
                    "signingKey".to_string(),
                    k8s_openapi::ByteString(stored.clone()),
                )])),
                ..Default::default()
            })
        });

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect("claim succeeds");

        sandbox_capability_token::verify(
            &claimed.capability,
            &agent_key,
            &agent_identity,
            SandboxOperationClass::Execute,
            1_100,
        )
        .expect("the agent must accept what the broker mints");

        sandbox_capability_token::verify(
            &claimed.capability,
            &pair.pk,
            &alien_core::sandbox_capability::SandboxSessionIdentity {
                session_id: "uid-replacement-pod".to_string(),
                generation: 1,
            },
            SandboxOperationClass::Execute,
            1_100,
        )
        .expect_err("a capability for one session must not reach another");
    }

    /// Release addresses a session, and the pod it deletes is the one carrying that session.
    ///
    /// A claim relabels a pool pod rather than renaming it, so the pod is still called
    /// `pool-<n>` while the caller only ever knows a session id. Deleting by the caller's string
    /// matched nothing, and the swallowed error reported success — every session leaked until the
    /// parent was torn down.
    #[tokio::test]
    async fn release_deletes_the_pod_carrying_the_session_not_one_named_after_it() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().returning(|_, selector, _| {
            let selector = selector.expect("release must select by label");
            assert!(
                selector.contains("alien.dev/sandbox=sbx")
                    && selector.contains("alien.dev/sandbox-session=session-7")
                    && selector.contains("alien.dev/deployment=release-a"),
                "both the sandbox and the session must be in the selector: {selector}"
            );
            let mut labels = crate::sandbox::idle_pod_labels("sbx");
            labels.insert(
                crate::sandbox::LABEL_SESSION.to_string(),
                "session-7".to_string(),
            );
            Ok(List {
                items: vec![Pod {
                    metadata: ObjectMeta {
                        name: Some("pool-3".to_string()),
                        labels: Some(labels),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            })
        });
        pods.expect_delete_pod()
            .withf(|_, name| name == "pool-3")
            .times(1)
            .returning(|_, _| Ok(()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));
        release_session_for_deployment(
            &pods,
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "alien-sandbox-sbx",
            "sbx",
            "session-7",
            "alien-sandbox-sbx-capability",
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await
        .expect("the claimed pod is released");
    }

    /// A session that matches nothing is already in the desired end state.
    #[tokio::test]
    async fn releasing_an_unknown_session_is_idempotent_and_deletes_nothing() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods()
            .returning(|_, _, _| Ok(List::default()));
        pods.expect_delete_pod().never().returning(|_, _| Ok(()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let mut secrets = MockSecretsApi::new();
        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));
        release_session_for_deployment(
            &pods,
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "alien-sandbox-sbx",
            "sbx",
            "never-claimed",
            "alien-sandbox-sbx-capability",
            DEPLOYMENT_LABEL_KEY,
            DEPLOYMENT_LABEL_VALUE,
        )
        .await
        .expect("an unknown session is not an error");
    }

    /// A pod claimed before the kubelet gave it an address is ended, not abandoned.
    ///
    /// Pool pods are labelled idle at creation, so a claim can win one that has no `pod_ip` yet.
    /// The label write has already committed and no session exists to release it, so returning an
    /// error there strands the pod in `claimed` forever and fails a request a later candidate
    /// could have served. The claim also set the kubelet's deadline, which cannot be unset, so the
    /// pod is deleted (the pool refills) instead of going back to the idle pool.
    #[tokio::test]
    async fn a_pod_claimed_before_it_has_an_address_is_ended_and_the_next_one_is_used() {
        let mut pods = MockPodApi::new();
        pods.expect_delete_pod_if_uid()
            .withf(|_, name, _| name == "pool-0")
            .times(1)
            .returning(|_, _, _| Ok(()));
        pods.expect_delete_pod()
            .withf(|_, name| name == "pool-0")
            .times(0..=1)
            .returning(|_, _| Ok(()));
        pods.expect_list_pods().returning(|_, _, _| {
            Ok(List {
                items: vec![
                    idle_pod_without_address("pool-0"),
                    idle_pod("pool-1", "10.1.2.3"),
                ],
                ..Default::default()
            })
        });

        // pool-0 is claimed, then restored to idle; pool-1 is claimed and kept.
        pods.expect_update_pod().returning(|_, name, pod| {
            let state = pod
                .metadata
                .labels
                .as_ref()
                .and_then(|l| l.get(crate::sandbox::LABEL_POOL_STATE))
                .cloned()
                .unwrap_or_default();
            if name == "pool-0" && state == crate::sandbox::POOL_STATE_IDLE {
                return Ok(idle_pod_without_address("pool-0"));
            }
            Ok(if name == "pool-0" {
                idle_pod_without_address("pool-0")
            } else {
                idle_pod("pool-1", "10.1.2.3")
            })
        });

        let mut secrets = MockSecretsApi::new();

        no_reservation(&mut secrets);
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let claimed = claim_session(&pods, &secrets, "sbx", "ns", "session-1", "key", 1_000)
            .await
            .expect("the addressless pod must not fail the whole claim");

        assert_eq!(
            claimed.endpoint, "http://10.1.2.3:8971",
            "the claim must land on the pod that actually has an address"
        );
    }
}
