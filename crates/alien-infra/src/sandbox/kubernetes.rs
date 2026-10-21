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
    LABEL_SANDBOX,
};
use alien_core::{ResourceOutputs as CoreResourceOutputs, ResourceStatus, Sandbox, SandboxOutputs};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// Runtime class a sandbox pod runs under when the operator has not chosen one.
const DEFAULT_RUNTIME_CLASS: &str = "gvisor";

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
        let runtime_class = self
            .runtime_class
            .clone()
            .unwrap_or_else(|| DEFAULT_RUNTIME_CLASS.to_string());

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
                .unwrap_or(DEFAULT_RUNTIME_CLASS),
            self.warm_pool_size.unwrap_or(DEFAULT_WARM_POOL_SIZE),
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

    let (deployment_label_key, deployment_label_value) = kubernetes_deployment_scope_label(ctx);
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

    let waiting = idle.items.len();
    let deficit = pool_deficit(target, waiting);
    let mut created = 0;
    if deficit == 0 {
        return Ok(WarmPool {
            idle: waiting,
            created,
        });
    }
    let Some(egress_environment) =
        super::kubernetes_egress::bootstrap_environment(ctx, sandbox, namespace).await?
    else {
        return Ok(WarmPool {
            idle: waiting,
            created,
        });
    };

    for _ in 0..deficit {
        let mut pod = idle_pool_pod_with_labels(
            sandbox,
            namespace,
            runtime_class,
            None,
            capability_public_key,
            kubernetes_cleanup_resource_labels(ctx, &sandbox.id),
        );

        pod.spec
            .as_mut()
            .expect("generated pod has a spec")
            .containers[0]
            .env
            .get_or_insert_default()
            .extend(egress_environment.clone());

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
    /// An operator without the chart's broker address must fail the sandbox rather than let it
    /// reach Running with no binding: a linked workload would then start without
    /// `ALIEN_<SANDBOX>_BINDING`, which is the failure this guards against.
    #[test]
    fn a_missing_broker_address_fails_instead_of_withholding_the_binding() {
        for value in [None, Some(String::new()), Some("  ".to_string())] {
            let error = broker_url_from("sbx", value.clone()).expect_err("no address is an error");
            assert_eq!(error.code, "RESOURCE_CONTROLLER_CONFIG_ERROR", "{value:?}");
            assert!(
                error.message.contains(alien_core::ENV_ALIEN_SANDBOX_BROKER_URL),
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
