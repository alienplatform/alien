//! Kubernetes Sandbox controller.
//!
//! Alien owns the lifecycle here, unlike Postgres or KV on Kubernetes which connect to
//! something the operator already runs. So this is a real controller rather than an
//! external-binding shim.
//!
//! The Frozen parent — namespace, ServiceAccount, NetworkPolicy, pod template — is emitted by
//! Helm at setup. What the controller owns is refusing an ineligible cluster before anything is
//! created, and reaping the Live pods.

use std::time::Duration;

use tracing::{debug, info, warn};

use crate::core::{
    kubernetes_cleanup_resource_labels, kubernetes_deployment_scope_label,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::sandbox::{
    idle_pool_pod_with_labels, idle_selector, pool_deficit, require_sandboxed_runtime_class,
    session_deadline_passed, SandboxPlacement, LABEL_POOL_STATE, LABEL_SANDBOX, LABEL_SESSION,
};
use alien_core::{ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox, SandboxOutputs};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// Runtime class of an install whose chart predates `sandbox.runtimeClassName` and so does not
/// pass `ALIEN_SANDBOX_RUNTIME_CLASS`. Those installs have always run under it; a chart upgrade
/// sets the variable and this is no longer consulted. The Operator never picks a class otherwise.
const LEGACY_RUNTIME_CLASS: &str = "gvisor";

/// The class the chart chose, which the Operator only checks. `None` for a chart that predates it.
fn chart_runtime_class() -> Option<String> {
    std::env::var(alien_core::ENV_ALIEN_SANDBOX_RUNTIME_CLASS)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Idle pods the chart asks for (`sandbox.warmPoolSize`); `None` for a chart that predates it.
fn chart_warm_pool_size() -> Option<usize> {
    std::env::var(alien_core::ENV_ALIEN_SANDBOX_WARM_POOL_SIZE)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

/// Kubernetes Sandbox controller.
#[controller]
pub struct KubernetesSandboxController {
    /// Bound installed with the parent and published to runtime bindings.
    #[serde(default)]
    pub(crate) egress: Option<alien_core::bindings::SandboxEgressBinding>,
    /// Public signing material; the private key stays in the secrets vault and is not read
    /// back. Other internet sandboxes take the key from the owner's copy of this field.
    #[serde(default)]
    pub(crate) egress_public_key: Option<super::egress::GatewayPublicKey>,
    /// Digest of the gateway configuration this owner last published. An unchanged
    /// configuration is not read or written again.
    #[serde(default)]
    pub(crate) egress_config_digest: Option<String>,
    /// The owner has published a configuration that names the mounted replica secret. From
    /// then on it never embeds one again, whatever the gateway's state.
    #[serde(default)]
    pub(crate) egress_replica_secret_sourced: bool,
    /// Default policy supplied to newly created sessions.
    #[serde(default)]
    pub(crate) default_policy: Option<alien_core::EgressPolicy>,
    /// Sandbox this controller owns, and the enumeration scope for its pods.
    pub(crate) sandbox_id: Option<String>,
    /// Namespace the Helm chart created for this sandbox's pods.
    pub(crate) namespace: Option<String>,
    /// Runtime class every session pod carries.
    pub(crate) runtime_class: Option<String>,
    /// Idle pods kept ready. 79s cold against 2.7s warm is the reason this exists.
    pub(crate) warm_pool_size: Option<usize>,
    /// Public half of the sandbox's capability keypair, base64. Only the public half: a pod's
    /// environment is readable by the untrusted code inside it.
    pub(crate) capability_public_key: Option<String>,
    /// Where the application reaches the session broker, which the operator serves. Read on
    /// every pass from the address the chart gives the operator.
    pub(crate) broker_url: Option<String>,
    /// Deployment ownership selector used by the broker and deletion paths.
    #[serde(default)]
    pub(crate) deployment_label_key: Option<String>,
    #[serde(default)]
    pub(crate) deployment_label_value: Option<String>,
}

impl super::egress::RecordsEgressKey for KubernetesSandboxController {
    fn egress_public_key(&self) -> Option<&super::egress::GatewayPublicKey> {
        self.egress_public_key.as_ref()
    }
}

#[controller]
impl KubernetesSandboxController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = VerifyCluster,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn verify_cluster(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        // The chart's value wins over what an earlier run recorded, so changing
        // `sandbox.runtimeClassName` and upgrading takes effect.
        let runtime_class = chart_runtime_class()
            .or_else(|| self.runtime_class.clone())
            .unwrap_or_else(|| LEGACY_RUNTIME_CLASS.to_string());

        let kubernetes_config = ctx.get_kubernetes_config()?;
        let client = ctx
            .service_provider
            .get_kubernetes_runtime_class_client(kubernetes_config)
            .await?;

        let available =
            client
                .list_runtime_classes()
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to list RuntimeClasses".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

        // Before any pod exists. On Autopilot an unschedulable pod is not rejected — node
        // auto-provisioning picks it up and it sits in Pending while nodes are billed.
        require_sandboxed_runtime_class(&config.id, &runtime_class, &available.items)?;

        self.runtime_class = Some(runtime_class);
        self.sandbox_id = Some(config.id.clone());
        self.default_policy = config.default_policy.clone();
        self.broker_url = Some(broker_url(&config.id)?);

        // Recorded as soon as it is known, because the binding and the outputs both read it
        // directly rather than through the fallback: leaving it unset publishes no binding at
        // all, and the sandbox comes up Running with nothing able to reach it.
        let namespace = deployment_namespace(ctx.get_kubernetes_config()?)?;
        self.namespace = Some(namespace.clone());
        let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
        self.deployment_label_key = Some(deployment_label_key);
        self.deployment_label_value = Some(deployment_label_value);

        if self.capability_public_key.is_none() {
            self.capability_public_key =
                Some(ensure_capability_keypair(ctx, &config.id, &namespace).await?);
        }

        self.egress = Some(
            super::kubernetes_egress::configure(
                ctx,
                config,
                &namespace,
                &mut self.egress_public_key,
                &mut self.egress_config_digest,
                &mut self.egress_replica_secret_sourced,
            )
            .await?,
        );

        info!(sandbox_id = %config.id, "Cluster can run sandboxes");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let namespace = deployment_namespace(ctx.get_kubernetes_config()?)?;
        self.broker_url = Some(broker_url(&config.id)?);
        let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
        self.deployment_label_key = Some(deployment_label_key);
        self.deployment_label_value = Some(deployment_label_value);
        self.capability_public_key =
            Some(ensure_capability_keypair(ctx, &config.id, &namespace).await?);
        self.egress = Some(
            super::kubernetes_egress::configure(
                ctx,
                config,
                &namespace,
                &mut self.egress_public_key,
                &mut self.egress_config_digest,
                &mut self.egress_replica_secret_sourced,
            )
            .await?,
        );
        let sessions = list_session_pods(ctx, &namespace, &config.id).await?;

        let pool = replenish_warm_pool(
            ctx,
            &config,
            &namespace,
            self.runtime_class
                .as_deref()
                .unwrap_or(LEGACY_RUNTIME_CLASS),
            self.warm_pool_size
                .or_else(chart_warm_pool_size)
                .unwrap_or(DEFAULT_WARM_POOL_SIZE),
            self.capability_public_key.as_deref(),
        )
        .await?;

        let created = pool.created;
        debug!(sandbox_id = %config.id, sessions, created, "Sandbox health check passed");

        // Counts of pods, never anything inside one. `sessions` covers every pod carrying the
        // sandbox label, the idle pool included, so what an application is actually running is
        // what is left once the idle pods are taken out.
        ctx.emit_heartbeat(alien_core::ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Sandbox::RESOURCE_TYPE,
            controller_platform: alien_core::Platform::Kubernetes,
            backend: alien_core::HeartbeatBackend::Kubernetes,
            observed_at: chrono::Utc::now(),
            data: alien_core::ResourceHeartbeatData::Sandbox(
                alien_core::SandboxHeartbeatData::KubernetesPods(
                    alien_core::KubernetesSandboxHeartbeatData {
                        status: alien_core::SandboxHeartbeatStatus::default(),
                        namespace,
                        active_sessions: sessions.saturating_sub(pool.idle) as u32,
                        idle_pods: (pool.idle + pool.created) as u32,
                    },
                ),
            ),
            raw: vec![],
        });

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdatingSandbox,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn updating_sandbox(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;
        self.default_policy = config.default_policy.clone();
        self.broker_url = Some(broker_url(&config.id)?);
        let namespace = deployment_namespace(ctx.get_kubernetes_config()?)?;
        self.egress = Some(
            super::kubernetes_egress::configure(
                ctx,
                config,
                &namespace,
                &mut self.egress_public_key,
                &mut self.egress_config_digest,
                &mut self.egress_replica_secret_sourced,
            )
            .await?,
        );

        // Config applies to pods created after it. Running sessions are not rolled: a session
        // is a unit of work someone is waiting on, not a replica.
        info!(sandbox_id = %config.id, "Updated Kubernetes sandbox configuration");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Sandbox>()?;

        let namespace = deployment_namespace(ctx.get_kubernetes_config()?)?;
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let client = ctx
            .service_provider
            .get_kubernetes_pod_client(kubernetes_config)
            .await?;

        let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
        let pods = client
            .list_pods(
                &namespace,
                Some(format!(
                    "{LABEL_SANDBOX}={},{}={}",
                    config.id, deployment_label_key, deployment_label_value
                )),
                None,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list sandbox pods for deletion".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Children before the parent, and best-effort per pod: one already-gone pod must not
        // strand the rest. But only *already-gone* — a delete refused for any other reason
        // (forbidden, a webhook, the apiserver unavailable) leaves a sandbox running, and
        // reporting Deleted over it is a teardown that lies.
        let mut removed = 0;
        let mut unreachable = Vec::new();
        for pod in &pods.items {
            let Some(name) = pod.metadata.name.as_deref() else {
                continue;
            };

            match client.delete_pod(&namespace, name).await {
                Ok(()) => removed += 1,
                Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => removed += 1,
                Err(error) => unreachable.push(format!("{name}: {error}")),
            }
        }

        if !unreachable.is_empty() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "{} sandbox pod(s) could not be deleted and are still running: {}",
                    unreachable.len(),
                    unreachable.join("; ")
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // The capability key outlives its pods otherwise, and a signing key nobody can use is
        // still a signing key sitting in the cluster. Best effort: one already gone is the
        // desired end state, and it must not strand the rest of the teardown.
        let secrets = ctx
            .service_provider
            .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
            .await?;
        let capability_name = capability_secret_name(&config.id);
        match secrets.get_secret(&namespace, &capability_name).await {
            Ok(secret) => {
                let is_owned = secret.metadata.labels.as_ref().is_some_and(|labels| {
                    labels.get(&deployment_label_key) == Some(&deployment_label_value)
                });
                if !is_owned {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "refusing to delete ambiguous sandbox capability Secret '{capability_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                secrets
                    .delete_secret(&namespace, &capability_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "failed to delete sandbox capability Secret '{capability_name}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
            Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "failed to inspect sandbox capability Secret '{capability_name}' before deletion"
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
        }

        info!(sandbox_id = %config.id, removed, "Removed Kubernetes sandbox pods and capability key");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, SandboxBinding};

        // Every field is recorded by the first provisioning step, which fails rather than
        // proceed without a broker address, so this is only `None` before that step ran.
        let (
            Some(sandbox_id),
            Some(namespace),
            Some(runtime_class),
            Some(broker_url),
            Some(deployment_label_key),
            Some(deployment_label_value),
        ) = (
            self.sandbox_id.clone(),
            self.namespace.clone(),
            self.runtime_class.clone(),
            self.broker_url.clone(),
            self.deployment_label_key.clone(),
            self.deployment_label_value.clone(),
        )
        else {
            return Ok(None);
        };

        let mut binding = SandboxBinding::kubernetes(
            BindingValue::value(namespace),
            BindingValue::value(runtime_class),
            BindingValue::value(format!(
                "{},{}={}",
                idle_selector(&sandbox_id),
                deployment_label_key,
                deployment_label_value
            )),
            BindingValue::value(broker_url),
            BindingValue::value(capability_secret_name(&sandbox_id)),
            BindingValue::value(SERVICE_ACCOUNT_TOKEN_PATH.to_string()),
            self.egress
                .as_ref()
                .map(|value| value.declaration().clone()),
        );

        if let SandboxBinding::Kubernetes(value) = &mut binding {
            value.egress = self.egress.clone();
            value.default_policy = self.default_policy.clone();
        }

        Ok(Some(
            serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize the sandbox binding".to_string(),
                },
            )?,
        ))
    }

    // ─────────────── TERMINAL STATES ──────────────────────────────────────

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    // ─────────────── HELPER METHODS ──────────────────────────────────────

    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.namespace.as_ref().map(|namespace| {
            CoreResourceOutputs::new(SandboxOutputs {
                parent_name: namespace.clone(),
                identifier: self.runtime_class.clone(),
                // Sessions are reached through the in-pod agent; there is no provider endpoint.
                endpoint: None,
            })
        })
    }
}

/// Idle pods kept ready when the operator has not chosen a size.
const DEFAULT_WARM_POOL_SIZE: usize = 2;

/// What one replenish pass found and did.
struct WarmPool {
    /// Idle pods already waiting when the pass ran.
    idle: usize,
    /// Pods the pass added.
    created: usize,
}

/// Tops the idle pool back up to its target.
///
/// Runs on the health tick rather than on session create: a create that had to wait for a pod
/// to be built would be the 79s cold path this pool exists to avoid.
async fn replenish_warm_pool(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
    runtime_class: &str,
    target: usize,
    capability_public_key: Option<&str>,
) -> Result<WarmPool> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let client = ctx
        .service_provider
        .get_kubernetes_pod_client(kubernetes_config)
        .await?;

    let placement = SandboxPlacement::from_env().map_err(|message| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: sandbox.id.clone(),
            message,
        })
    })?;

    let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
    let selector_scope = format!("{deployment_label_key}={deployment_label_value}");
    let idle = client
        .list_pods(
            namespace,
            Some(format!(
                "{},{}={}",
                idle_selector(&sandbox.id),
                deployment_label_key,
                deployment_label_value
            )),
            None,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to list idle sandbox pods".to_string(),
            resource_id: Some(sandbox.id.clone()),
        })?;

    reap_expired_sessions(client.as_ref(), sandbox, namespace, &selector_scope).await;

    let mut created = 0;
    let Some(egress_environment) =
        super::kubernetes_egress::bootstrap_environment(ctx, sandbox, namespace).await?
    else {
        return Ok(WarmPool {
            idle: idle.items.len(),
            created,
        });
    };

    let mut pod = idle_pool_pod_with_labels(
        sandbox,
        namespace,
        runtime_class,
        None,
        capability_public_key,
        kubernetes_cleanup_resource_labels(ctx, &sandbox.id),
    );
    placement.apply(&mut pod);
    if let Some(secret_name) = ensure_image_pull_secret(ctx, sandbox, namespace).await? {
        pod.spec
            .as_mut()
            .expect("generated pod has a spec")
            .image_pull_secrets = Some(vec![k8s_openapi::api::core::v1::LocalObjectReference {
            name: secret_name,
        }]);
    }
    pod.spec
        .as_mut()
        .expect("generated pod has a spec")
        .containers[0]
        .env
        .get_or_insert_default()
        .extend(egress_environment);

    // A pod is only as new as the template it was built from. A release that changes the image,
    // limits or lifecycle of a Live sandbox changes the template, and the idle pods built from
    // the old one are replaced; a pod already claimed by a session is left to run until it ends.
    let template_hash = template_hash(&pod);
    pod.metadata
        .annotations
        .get_or_insert_default()
        .insert(ANNOTATION_TEMPLATE_HASH.to_string(), template_hash.clone());
    let mut waiting = 0;
    for mut stale in stale_idle_pods(idle.items, &template_hash, &mut waiting) {
        let name = stale.metadata.name.clone().unwrap_or_default();
        // A session may claim the pod between the listing and the delete. The write that takes it
        // out of the idle pool carries the resourceVersion that was listed, so it loses to a
        // claim, and only a pod this controller retired is deleted.
        stale
            .metadata
            .labels
            .get_or_insert_default()
            .insert(LABEL_POOL_STATE.to_string(), "retired".to_string());
        let retired = client.update_pod(namespace, &name, &stale).await;
        let outcome = match retired {
            Ok(_) => client.delete_pod(namespace, &name).await,
            Err(error) => Err(error),
        };
        match outcome {
            Ok(()) => {
                debug!(sandbox_id = %sandbox.id, pod = %name, "Replaced an idle pool pod built from an older template")
            }
            Err(error) => {
                warn!(sandbox_id = %sandbox.id, pod = %name, error = %error, "Left a stale idle pool pod; it may have been claimed")
            }
        }
    }
    let deficit = pool_deficit(target, waiting);

    // The broker builds a pod from this when a create finds the pool empty, so a create never
    // fails for want of an idle pod. It is rewritten every tick and so follows chart upgrades.
    // Best effort: without it a create still claims from the pool, it just cannot go cold.
    if let Err(error) = publish_pod_template(
        ctx,
        &sandbox.id,
        namespace,
        &pod,
        matches!(sandbox.egress, alien_core::SandboxEgress::Allow),
    )
    .await
    {
        warn!(sandbox_id = %sandbox.id, %namespace, error = %error, "Failed to publish the sandbox pod template");
    }

    for _ in 0..deficit {
        // Best effort per pod: a pool that is one short is slower, not broken, and failing the
        // health tick over it would take a working sandbox out of Running. The reason is logged
        // rather than swallowed, because a pool that is always empty is a bug and silence makes
        // it look like a slow cluster.
        match client.create_pod(namespace, &pod).await {
            Ok(_) => created += 1,
            Err(error) => {
                warn!(sandbox_id = %sandbox.id, %namespace, error = %error, "Failed to create a pool pod")
            }
        }
    }

    Ok(WarmPool {
        idle: waiting,
        created,
    })
}

/// Whether `image` is served by the manager's registry, whose pull needs the deployment's token.
/// An image from any other registry is pulled with whatever access the cluster already has: the
/// token is never offered to a registry that is not the manager's.
fn image_is_from_manager(image: &str, manager_url: Option<&str>) -> bool {
    let Some(manager_url) = manager_url else {
        return false;
    };
    let manager_host = alien_core::image_rewrite::strip_url_scheme(manager_url);
    image.split('/').next() == Some(manager_host)
}

/// Creates or refreshes the pull Secret for an image `alien release` pushed to the manager's
/// registry (a sandbox built from source), and returns its name. `None` for every other image.
///
/// The pod only references the Secret: the kubelet reads it, and the pod mounts no Secret and no
/// ServiceAccount token, so the sandbox's code cannot.
async fn ensure_image_pull_secret(
    ctx: &ResourceControllerContext<'_>,
    sandbox: &Sandbox,
    namespace: &str,
) -> Result<Option<String>> {
    let alien_core::SandboxCode::Image { image, .. } = &sandbox.code else {
        return Ok(None);
    };
    if !image_is_from_manager(image, ctx.deployment_config.manager_url.as_deref()) {
        return Ok(None);
    }
    let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: sandbox.id.clone(),
            message: "deployment_token is required to pull the sandbox image from the manager's registry".to_string(),
        })
    })?;
    let name = format!("alien-sbx-{}-registry", sandbox.id);
    let secrets = ctx
        .service_provider
        .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
        .await?;
    crate::kubernetes_registry::ensure_registry_pull_secret(
        &secrets,
        namespace,
        &name,
        image,
        token,
        kubernetes_cleanup_resource_labels(ctx, &sandbox.id),
        false,
    )
    .await?;
    Ok(Some(name))
}

/// Deletes the claimed pods of this sandbox whose session deadline has passed.
///
/// A session's deadline starts at its claim, which a pod-level deadline cannot express, so the
/// Operator ends the session instead. Best effort per pod, retried on the next tick.
async fn reap_expired_sessions(
    client: &dyn alien_k8s_clients::PodApi,
    sandbox: &Sandbox,
    namespace: &str,
    scope: &str,
) {
    let selector = format!("{LABEL_SANDBOX}={},{LABEL_SESSION},{scope}", sandbox.id);
    let Ok(claimed) = client.list_pods(namespace, Some(selector), None).await else {
        return;
    };
    let now = chrono::Utc::now().timestamp();
    for pod in claimed.items {
        if !session_deadline_passed(&pod, now) {
            continue;
        }
        let (Some(name), Some(uid)) = (pod.metadata.name.as_deref(), pod.metadata.uid.as_deref())
        else {
            continue;
        };
        // Only this pod: a name another session reuses later is a different uid.
        match client.delete_pod_if_uid(namespace, name, uid).await {
            Ok(()) => {
                info!(sandbox_id = %sandbox.id, pod = %name, "Ended a session at its deadline")
            }
            Err(error) => {
                warn!(sandbox_id = %sandbox.id, pod = %name, error = %error, "Failed to end an expired session")
            }
        }
    }
}

/// Annotation holding the digest of the template an idle pool pod was built from.
const ANNOTATION_TEMPLATE_HASH: &str = "alien.dev/pod-template-hash";

/// Digest of the pod the controller would create. The pod is serialized in a fixed field order,
/// so the same template always gives the same digest.
fn template_hash(pod: &k8s_openapi::api::core::v1::Pod) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(pod).unwrap_or_default();
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Idle pods built from a template other than `current`. Counts the others in `fresh`.
fn stale_idle_pods(
    idle: Vec<k8s_openapi::api::core::v1::Pod>,
    current: &str,
    fresh: &mut usize,
) -> Vec<k8s_openapi::api::core::v1::Pod> {
    let mut stale = Vec::new();
    for pod in idle {
        let built_from = pod
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get(ANNOTATION_TEMPLATE_HASH));
        match built_from {
            Some(hash) if hash == current => *fresh += 1,
            _ if pod.metadata.name.is_some() => stale.push(pod),
            _ => {}
        }
    }
    stale
}

/// Stores the idle pod the controller would create in the capability Secret, for the broker's
/// cold create. Holds no secret: the environment carries only the public key and gateway address.
async fn publish_pod_template(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    namespace: &str,
    pod: &k8s_openapi::api::core::v1::Pod,
    requires_cluster_dns: bool,
) -> Result<()> {
    let client = ctx
        .service_provider
        .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
        .await?;
    let name = capability_secret_name(sandbox_id);
    let mut secret =
        client
            .get_secret(namespace, &name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("failed to read capability Secret '{name}'"),
                resource_id: Some(sandbox_id.to_string()),
            })?;
    let template = serde_json::to_vec(pod).map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("failed to serialize the pod template: {error}"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })?;
    let data = secret.data.get_or_insert_default();
    let dns_flag = if requires_cluster_dns {
        b"true".to_vec()
    } else {
        b"false".to_vec()
    };
    if data
        .get(POD_TEMPLATE_SECRET_KEY)
        .is_some_and(|existing| existing.0 == template)
        && data
            .get(REQUIRES_CLUSTER_DNS_SECRET_KEY)
            .is_some_and(|existing| existing.0 == dns_flag)
    {
        return Ok(());
    }
    data.insert(
        REQUIRES_CLUSTER_DNS_SECRET_KEY.to_string(),
        k8s_openapi::ByteString(dns_flag),
    );
    data.insert(
        POD_TEMPLATE_SECRET_KEY.to_string(),
        k8s_openapi::ByteString(template),
    );
    client
        .update_secret(namespace, &name, &secret)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("failed to store the pod template in '{name}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    Ok(())
}

/// The deployment's namespace, which is where a sandbox's pods go.
///
/// Not a namespace of the sandbox's own: the operator is namespace-scoped and holds no
/// cluster-admin, so it can neither create a namespace nor act in one it was not installed into.
/// Every resource lives in the one namespace Helm created, and a sandbox's pods are separated
/// from everything else there by the `alien.dev/sandbox` label their NetworkPolicy selects on.
///
/// This is also the namespace the broker checks a caller's ServiceAccount against, so reading it
/// from anywhere else would put the two halves in different places.
fn deployment_namespace(config: &alien_core::KubernetesClientConfig) -> Result<String> {
    use alien_core::KubernetesClientConfig as Config;

    let namespace = match config {
        Config::InCluster { namespace, .. }
        | Config::Kubeconfig { namespace, .. }
        | Config::Manual { namespace, .. } => namespace.clone(),
    };

    namespace.ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "the Kubernetes client config names no namespace, so there is nowhere to \
                      put a sandbox's pods"
                .to_string(),
            resource_id: None,
        })
    })
}

/// Counts the Live pods belonging to one sandbox.
async fn list_session_pods(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
    sandbox_id: &str,
) -> Result<usize> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let client = ctx
        .service_provider
        .get_kubernetes_pod_client(kubernetes_config)
        .await?;

    let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
    let pods = client
        .list_pods(
            namespace,
            Some(format!(
                "{LABEL_SANDBOX}={sandbox_id},{deployment_label_key}={deployment_label_value}"
            )),
            None,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to list sandbox pods".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;

    Ok(pods.items.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::Pod;

    fn idle_with_hash(name: &str, hash: Option<&str>) -> Pod {
        let mut pod = Pod::default();
        pod.metadata.name = Some(name.to_string());
        if let Some(hash) = hash {
            pod.metadata.annotations =
                Some([(ANNOTATION_TEMPLATE_HASH.to_string(), hash.to_string())].into());
        }
        pod
    }

    /// A release that changes a Live sandbox's image or limits changes the pod template, and the
    /// idle pods built from the old template are the ones replaced.
    #[test]
    fn the_deployment_token_is_offered_only_to_the_managers_registry() {
        let manager = Some("https://manager.example.com");
        assert!(image_is_from_manager(
            "manager.example.com/app/sbx:1",
            manager
        ));
        assert!(!image_is_from_manager(
            "docker.io/library/ubuntu:24.04",
            manager
        ));
        assert!(!image_is_from_manager("ubuntu:24.04", manager));
        assert!(!image_is_from_manager(
            "manager.example.com/app/sbx:1",
            None
        ));
        assert!(
            !image_is_from_manager("manager.example.com.evil.test/x:1", manager),
            "a host that merely starts with the manager's is another registry"
        );
    }

    #[test]
    fn idle_pods_built_from_an_older_template_are_the_ones_replaced() {
        let mut fresh = 0;
        let stale = stale_idle_pods(
            vec![
                idle_with_hash("current", Some("new")),
                idle_with_hash("old", Some("old")),
                idle_with_hash("from-before-the-hash-existed", None),
            ],
            "new",
            &mut fresh,
        );
        assert_eq!(fresh, 1);
        let names: Vec<_> = stale
            .iter()
            .filter_map(|p| p.metadata.name.as_deref())
            .collect();
        assert_eq!(names, ["old", "from-before-the-hash-existed"]);
    }

    fn pool_pod(image: &str, memory: &str) -> Pod {
        use alien_core::{SandboxCode, SandboxEgress, SandboxLimits};
        let sandbox = Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
                bundled_from: None,
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: memory.to_string(),
                disk: "10Gi".to_string(),
                max_processes: None,
            })
            .egress(SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(3600),
                idle_pause_seconds: None,
            })
            .build();
        let mut pod =
            idle_pool_pod_with_labels(&sandbox, "ns", "gvisor", None, None, Default::default());
        // Created by the API server with a random suffix, so never part of the template.
        pod.metadata.generate_name = None;
        pod
    }

    /// A new release changes the image or the limits of a Live sandbox; either must change the
    /// digest the idle pool is compared against, and an unchanged template must not.
    #[test]
    fn the_template_digest_follows_the_image_and_limits() {
        let base = template_hash(&pool_pod("registry/app:1", "2Gi"));
        assert_eq!(base, template_hash(&pool_pod("registry/app:1", "2Gi")));
        assert_ne!(
            base,
            template_hash(&pool_pod("registry/app:2", "2Gi")),
            "image"
        );
        assert_ne!(
            base,
            template_hash(&pool_pod("registry/app:1", "4Gi")),
            "limits"
        );
    }

    /// An operator without the chart's broker address must fail the sandbox rather than let it
    /// reach Running with no binding: a linked workload would then start without
    /// `ALIEN_<SANDBOX>_BINDING`, which is the failure this guards against.
    #[test]
    fn a_missing_broker_address_fails_instead_of_withholding_the_binding() {
        for value in [None, Some(String::new()), Some("  ".to_string())] {
            let error = broker_url_from("sbx", value.clone()).expect_err("no address is an error");
            assert_eq!(error.code, "RESOURCE_CONTROLLER_CONFIG_ERROR", "{value:?}");
            assert!(
                error
                    .message
                    .contains(alien_core::ENV_ALIEN_SANDBOX_BROKER_URL),
                "the error names the missing setting: {}",
                error.message
            );
        }
        assert_eq!(
            broker_url_from(
                "sbx",
                Some("http://alien-operator.alien.svc:8080".to_string())
            )
            .expect("an address"),
            "http://alien-operator.alien.svc:8080"
        );
    }

    #[test]
    fn the_binding_carries_the_broker_address_and_no_key_material() {
        use crate::core::ResourceController;

        let controller = KubernetesSandboxController {
            egress: None,
            egress_public_key: None,
            egress_config_digest: None,
            egress_replica_secret_sourced: false,
            default_policy: None,
            state: KubernetesSandboxState::Ready,
            sandbox_id: Some("sbx".to_string()),
            namespace: Some("alien-sandbox-sbx".to_string()),
            runtime_class: Some("gvisor".to_string()),
            warm_pool_size: None,
            capability_public_key: Some("cHVibGlj".to_string()),
            broker_url: Some("http://alien-operator.alien:8080".to_string()),
            deployment_label_key: Some("alien.dev/deployment".to_string()),
            deployment_label_value: Some("release-a".to_string()),
            _internal_stay_count: None,
        };

        let params = controller
            .get_binding_params()
            .expect("params")
            .expect("a ready sandbox publishes a binding");

        assert_eq!(params["brokerUrl"], "http://alien-operator.alien:8080");
        assert_eq!(params["keyName"], "alien-sandbox-sbx-capability");
        assert_eq!(
            params["selector"],
            "alien.dev/sandbox=sbx,alien.dev/sandbox-pool=idle,alien.dev/deployment=release-a"
        );
        assert_eq!(
            params["tokenPath"],
            "/var/run/secrets/kubernetes.io/serviceaccount/token"
        );
        // A path Kubernetes already wrote, never a secret of ours.
        assert!(
            !params.to_string().contains("cHVibGlj"),
            "no key material belongs in a binding: {params}"
        );
    }

    use super::*;
    use crate::core::{deserialize_controller, serialize_controller, ResourceController};

    #[test]
    fn gateway_binding_keeps_its_descriptor_and_creation_default() {
        use alien_core::bindings::{
            SandboxEgressBinding, SandboxEgressGateway, SandboxEgressSigningKey,
        };
        let gateway = SandboxEgressBinding::Gateway(SandboxEgressGateway {
            container: alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into(),
            audience: "deployment/subject".into(),
            signing_key: SandboxEgressSigningKey {
                vault: "secrets".into(),
                key: "sandbox-egress-signing-key".into(),
            },
            kid: "one".into(),
            admin_ca_pem: None,
            replicas: None,
        });
        let policy = serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
        let controller = KubernetesSandboxController {
            egress: Some(gateway.clone()),
            default_policy: Some(policy),
            sandbox_id: Some("subject".into()),
            namespace: Some("deployment".into()),
            runtime_class: Some("gvisor".into()),
            broker_url: Some("http://broker:8080".into()),
            deployment_label_key: Some("alien.dev/deployment".into()),
            deployment_label_value: Some("deployment".into()),
            ..Default::default()
        };
        let binding: alien_core::bindings::SandboxBinding =
            serde_json::from_value(controller.get_binding_params().unwrap().unwrap()).unwrap();
        let alien_core::bindings::SandboxBinding::Kubernetes(binding) = binding else {
            panic!("Kubernetes binding")
        };
        assert_eq!(binding.egress, Some(gateway));
        assert_eq!(binding.default_policy, controller.default_policy);
        let restored = deserialize_controller(serialize_controller(&controller).unwrap()).unwrap();
        assert_eq!(
            restored.get_binding_params().unwrap(),
            controller.get_binding_params().unwrap()
        );
    }

    #[test]
    fn controller_round_trips_by_tag() {
        let controller = KubernetesSandboxController {
            namespace: Some("alien-sandbox-agent".to_string()),
            runtime_class: Some("gvisor".to_string()),
            ..Default::default()
        };

        let value = serialize_controller(&controller).expect("serializes with its tag");
        assert_eq!(value["type"], "KubernetesSandboxController");

        let restored = deserialize_controller(value).expect("a registered tag must deserialize");
        assert_eq!(restored.controller_type(), controller.controller_type());
    }

    #[test]
    fn the_registry_resolves_a_kubernetes_sandbox_controller() {
        let registry = crate::core::ResourceRegistry::with_built_ins();

        let controller = registry
            .get_controller(
                alien_core::Sandbox::RESOURCE_TYPE,
                alien_core::Platform::Kubernetes,
            )
            .expect("Kubernetes must have a registered Sandbox controller");
        assert_eq!(controller.controller_type(), "KubernetesSandboxController");
    }

    #[cfg(feature = "kubernetes")]
    fn expected_capability_labels() -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([
            ("alien.dev/label-domain".to_string(), "acme".to_string()),
            (
                "alien.dev/legacy-label-domain-0".to_string(),
                "acme.example".to_string(),
            ),
            ("acme/deployment".to_string(), "release-a".to_string()),
            ("acme/resource".to_string(), "sbx".to_string()),
            (LABEL_SANDBOX.to_string(), "sbx".to_string()),
        ])
    }

    #[cfg(feature = "kubernetes")]
    #[test]
    fn capability_secret_with_the_explicit_legacy_scope_can_be_migrated() {
        let labels = std::collections::BTreeMap::from([
            (
                "acme.example/deployment".to_string(),
                "release-a".to_string(),
            ),
            ("acme.example/resource".to_string(), "sbx".to_string()),
            (LABEL_SANDBOX.to_string(), "sbx".to_string()),
        ]);

        assert!(capability_secret_labels_match_deployment(
            Some(&labels),
            &expected_capability_labels(),
            "sbx",
        ));
    }

    #[cfg(feature = "kubernetes")]
    #[test]
    fn unscoped_capability_secret_is_not_adopted() {
        let labels =
            std::collections::BTreeMap::from([(LABEL_SANDBOX.to_string(), "sbx".to_string())]);

        assert!(!capability_secret_labels_match_deployment(
            Some(&labels),
            &expected_capability_labels(),
            "sbx",
        ));
    }
}

/// Name of the Secret holding a sandbox's capability signing key.
pub fn capability_secret_name(sandbox_id: &str) -> String {
    format!("alien-sandbox-{sandbox_id}-capability")
}

/// Key within that Secret.
const CAPABILITY_SECRET_KEY: &str = "signingKey";

/// Key holding the idle pod the broker creates when no warm pod is free.
pub(crate) const POD_TEMPLATE_SECRET_KEY: &str = "podTemplate";

/// Key marking a sandbox whose network policy admits DNS only to `sandbox.clusterDNS`, so the
/// broker checks that value before it starts a new session.
pub(crate) const REQUIRES_CLUSTER_DNS_SECRET_KEY: &str = "requiresClusterDNS";

#[cfg(feature = "kubernetes")]
fn capability_secret_labels_match_deployment(
    labels: Option<&std::collections::BTreeMap<String, String>>,
    expected_labels: &std::collections::BTreeMap<String, String>,
    sandbox_id: &str,
) -> bool {
    labels
        .and_then(|labels| labels.get(LABEL_SANDBOX))
        .map(String::as_str)
        == Some(sandbox_id)
        && crate::core::kubernetes_labels_have_recognized_scope(labels, expected_labels)
        && crate::core::kubernetes_labels_have_compatible_scope(labels, expected_labels)
}

/// Creates the sandbox's capability keypair if it has none, returning the public half.
///
/// The private half goes into a Kubernetes Secret that sandbox pods never mount: the broker
/// reads it to mint, the agent only ever sees the public half. Storing it in controller state
/// would put a signing key in deployment state, which is the one thing state must not carry.
///
/// Reads before it writes, so a controller restart adopts the existing key rather than minting
/// a second one that would invalidate every capability already handed out.
#[cfg(feature = "kubernetes")]
async fn ensure_capability_keypair(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    namespace: &str,
) -> Result<String> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    let kubernetes_config = ctx.get_kubernetes_config()?;
    let client = ctx
        .service_provider
        .get_kubernetes_secrets_client(kubernetes_config)
        .await?;

    let name = capability_secret_name(sandbox_id);

    let existing = match client.get_secret(namespace, &name).await {
        Ok(existing) => Some(existing),
        Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => None,
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("failed to inspect capability Secret '{name}'"),
                resource_id: Some(sandbox_id.to_string()),
            }))
        }
    };
    if let Some(mut existing) = existing {
        if let Some(encoded) = existing
            .data
            .as_ref()
            .and_then(|data| data.get(CAPABILITY_SECRET_KEY))
        {
            let pair = ed25519_compact::KeyPair::from_slice(&encoded.0).map_err(|error| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("the stored capability key is unusable: {error}"),
                    resource_id: Some(sandbox_id.to_string()),
                })
            })?;
            let mut expected_labels = kubernetes_cleanup_resource_labels(ctx, sandbox_id);
            expected_labels.insert(LABEL_SANDBOX.to_string(), sandbox_id.to_string());
            let labels = existing.metadata.labels.as_ref();
            if !capability_secret_labels_match_deployment(labels, &expected_labels, sandbox_id) {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "the capability Secret for '{sandbox_id}' lacks this deployment's cleanup ownership labels; refusing to adopt an ambiguous signing key"
                    ),
                    resource_id: Some(sandbox_id.to_string()),
                }));
            }
            if labels != Some(&expected_labels) {
                existing.metadata.labels = Some(expected_labels);
                client
                    .update_secret(namespace, &name, &existing)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "failed to migrate cleanup ownership for capability Secret '{name}'"
                        ),
                        resource_id: Some(sandbox_id.to_string()),
                    })?;
            }
            return Ok(BASE64.encode(pair.pk.as_ref()));
        }
    }

    let pair = ed25519_compact::KeyPair::generate();

    let mut labels = kubernetes_cleanup_resource_labels(ctx, sandbox_id);
    labels.insert(LABEL_SANDBOX.to_string(), sandbox_id.to_string());
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(std::collections::BTreeMap::from([(
            CAPABILITY_SECRET_KEY.to_string(),
            k8s_openapi::ByteString(pair.as_ref().to_vec()),
        )])),
        ..Default::default()
    };

    if client.create_secret(namespace, &secret).await.is_err() {
        // Losing this write means somebody else created the key first, which is the outcome the
        // read above is for — adopt theirs. Propagating instead would put a sandbox whose key is
        // live and usable into a terminal ProvisionFailed, for a race that already resolved.
        let mut existing =
            client
                .get_secret(namespace, &name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("failed to store the capability key for '{sandbox_id}'"),
                    resource_id: Some(sandbox_id.to_string()),
                })?;

        let mut expected_labels = kubernetes_cleanup_resource_labels(ctx, sandbox_id);
        expected_labels.insert(LABEL_SANDBOX.to_string(), sandbox_id.to_string());
        let labels = existing.metadata.labels.as_ref();
        if !capability_secret_labels_match_deployment(labels, &expected_labels, sandbox_id) {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "the capability Secret for '{sandbox_id}' is not owned by this deployment"
                ),
                resource_id: Some(sandbox_id.to_string()),
            }));
        }
        if labels != Some(&expected_labels) {
            existing.metadata.labels = Some(expected_labels);
            client
                .update_secret(namespace, &name, &existing)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "failed to migrate cleanup ownership for capability Secret '{name}'"
                    ),
                    resource_id: Some(sandbox_id.to_string()),
                })?;
        }

        let encoded = existing
            .data
            .as_ref()
            .and_then(|data| data.get(CAPABILITY_SECRET_KEY))
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("the capability Secret for '{sandbox_id}' carries no key"),
                    resource_id: Some(sandbox_id.to_string()),
                })
            })?;

        let adopted = ed25519_compact::KeyPair::from_slice(&encoded.0).map_err(|error| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("the stored capability key is unusable: {error}"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;
        return Ok(BASE64.encode(adopted.pk.as_ref()));
    }

    Ok(BASE64.encode(pair.pk.as_ref()))
}

/// Where Kubernetes mounts a pod's own ServiceAccount token.
///
/// The binding carries this path rather than any secret of ours: the platform put the file
/// there, and the broker verifies it with a `TokenReview`.
const SERVICE_ACCOUNT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

/// Where the application reaches the session broker.
///
/// The broker is served by the operator, and the chart that installs the operator also renders
/// the Service in front of it, so the chart passes that Service's address in rather than this
/// controller guessing it from the operator's identity. An operator installed without it cannot
/// publish a usable binding, so the sandbox fails here instead of coming up Running with no
/// binding that any linked workload could load.
#[cfg(feature = "kubernetes")]
fn broker_url(sandbox_id: &str) -> Result<String> {
    broker_url_from(
        sandbox_id,
        std::env::var(alien_core::ENV_ALIEN_SANDBOX_BROKER_URL).ok(),
    )
}

#[cfg(feature = "kubernetes")]
fn broker_url_from(sandbox_id: &str, value: Option<String>) -> Result<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: sandbox_id.to_string(),
                message: format!(
                    "the operator has no sandbox broker address ({} is unset); upgrade the Helm \
                     chart, which renders the operator Service and passes its address",
                    alien_core::ENV_ALIEN_SANDBOX_BROKER_URL
                ),
            })
        })
}
