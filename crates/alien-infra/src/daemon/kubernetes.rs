use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::container::kubernetes::is_already_exists;
use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_runtime_pod_labels, projected_env_vars,
    reconcile_environment_secret, EnvSecretRotationTracker, EnvironmentVariableBuilder,
    KubernetesEnvSecretPlan, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_workload_heartbeat::{
    emit_kubernetes_workload_heartbeat, label_selector, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    branded_tag_key, kubernetes_resource_name, kubernetes_service_account_name, Daemon, DaemonCode,
    DaemonOutputs, ResourceOutputs, ResourceStatus, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_MANAGED_BY_TAG_VALUE, DEFAULT_ALIEN_LABEL_DOMAIN,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetSpec};
use k8s_openapi::api::core::v1::{
    Container, LocalObjectReference, PodSpec, PodTemplateSpec, ResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

#[controller]
pub struct KubernetesDaemonController {
    /// The name of the created Kubernetes DaemonSet.
    pub(crate) daemon_set_name: Option<String>,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
    /// Tracks the env-Secret checksum so `needs_update` can detect secret
    /// rotations that config diffing cannot see (secrets are projected via
    /// secretKeyRef, never into the resource config).
    #[serde(default)]
    pub(crate) env_secret: EnvSecretRotationTracker,
}

#[controller]
impl KubernetesDaemonController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;

        let daemon_set_name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;
        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());

        let image_pull_secret_name = if let DaemonCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let secret_name = format!("{}-registry", daemon_set_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, &namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };

        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;

        // Reconcile the per-resource env Secret (creates/updates `{daemon}-env`)
        // for any Secret-kind env var scoped to this Daemon — notably the
        // command receiver's `ALIEN_COMMANDS_TOKEN`. Matches the container path:
        // each key is rendered as a `secretKeyRef` in the DaemonSet manifest.
        let env_secret_plan =
            reconcile_environment_secret("daemon", &config.id, &daemon_set_name, &namespace, ctx)
                .await?;
        self.env_secret.record(env_secret_plan.as_ref());

        let daemonset = self
            .build_daemonset(
                config,
                &daemon_set_name,
                &namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;

        match workload_client
            .create_daemonset(&namespace, &daemonset)
            .await
        {
            Ok(_) => {}
            // A retry after a transient failure between a successful create
            // and state persistence (or an orphan from a prior deploy) hits
            // AlreadyExists. Adopt the existing DaemonSet when it carries our
            // managed-by labels — mirrors the container controller.
            Err(err) if is_already_exists(&err) => {
                let existing = workload_client
                    .get_daemonset(&namespace, &daemon_set_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to read existing daemonset '{}' before adoption.",
                            daemon_set_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                if !self.is_managed_daemonset(
                    ctx,
                    existing.metadata.labels.as_ref(),
                    &daemon_set_name,
                ) {
                    return Err(err.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Refusing to adopt unmanaged daemonset '{}'.",
                            daemon_set_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                info!(daemon_set_name=%daemon_set_name, namespace=%namespace, "Adopting existing Kubernetes DaemonSet");
            }
            Err(err) => {
                return Err(err.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create daemonset '{}'.", daemon_set_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.daemon_set_name = Some(daemon_set_name.clone());
        self.namespace = Some(namespace.clone());

        info!(daemon_set_name=%daemon_set_name, namespace=%namespace, "DaemonSet creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForDaemonSet,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDaemonSet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_daemonset(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.daemonset_ready(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }

        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if !self.daemonset_ready(ctx).await? {
            let config = ctx.desired_resource_config::<Daemon>()?;
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Daemon daemonset is not fully ready".to_string(),
            }));
        }

        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        if let (Some(daemon_set_name), Some(namespace)) = (&self.daemon_set_name, &self.namespace) {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            let daemonset = workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get daemonset '{}'", daemon_set_name),
                    resource_id: Some(config.id.clone()),
                })?;
            let labels = self.build_labels(daemon_set_name);
            emit_kubernetes_workload_heartbeat(
                ctx,
                KubernetesWorkloadHeartbeatInput {
                    deployment_id: None,
                    resource_id: config.id.clone(),
                    resource_type: Daemon::RESOURCE_TYPE,
                    data_kind: KubernetesWorkloadDataKind::Daemon,
                    command_supported: config.commands_enabled,
                    namespace: namespace.clone(),
                    workload_name: daemon_set_name.clone(),
                    workload_kind: alien_core::KubernetesWorkloadKind::DaemonSet,
                    workload: KubernetesWorkload::DaemonSet(daemonset),
                    label_selector: label_selector(&labels),
                },
            )
            .await?;
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let daemon_set_name = self.daemon_set_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "DaemonSet name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let existing = workload_client
            .get_daemonset(namespace, daemon_set_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get daemonset '{}' before update",
                    daemon_set_name
                ),
                resource_id: Some(config.id.clone()),
            })?;
        let resource_version = existing.metadata.resource_version.clone();

        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        let image_pull_secret_name = if let DaemonCode::Image { image } = &config.code {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let secret_name = format!("{}-registry", daemon_set_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(&secrets_client, namespace, &secret_name, image, token)
                .await?;
            Some(secret_name)
        } else {
            None
        };

        // Reconcile the env Secret so token/secret changes propagate on update
        // and the pod-template checksum annotation rolls the DaemonSet.
        let env_secret_plan =
            reconcile_environment_secret("daemon", &config.id, daemon_set_name, namespace, ctx)
                .await?;
        self.env_secret.record(env_secret_plan.as_ref());

        let mut new_daemonset = self
            .build_daemonset(
                config,
                daemon_set_name,
                namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                env_secret_plan.as_ref(),
                ctx,
            )
            .await?;
        new_daemonset.metadata.resource_version = resource_version;

        workload_client
            .update_daemonset(namespace, daemon_set_name, &new_daemonset)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update daemonset '{}'.", daemon_set_name),
                resource_id: Some(config.id.clone()),
            })?;

        Ok(HandlerAction::Continue {
            state: WaitingForUpdate,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.daemonset_ready(ctx).await? {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }

        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        if let Some(daemon_set_name) = &self.daemon_set_name {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match workload_client
                .delete_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.daemon_set_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete daemonset '{}'.", daemon_set_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDeletion,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        if let Some(daemon_set_name) = &self.daemon_set_name {
            let workload_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match workload_client
                .get_daemonset(namespace, daemon_set_name)
                .await
            {
                Ok(_) => {
                    debug!(daemon_set_name=%daemon_set_name, "Daemon daemonset still exists");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.daemon_set_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get daemonset '{}'.", daemon_set_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    /// Secret-typed env vars never enter the resource config on Kubernetes
    /// (they're projected via secretKeyRef), so config diffing alone cannot
    /// see secret rotations. Schedule an update when the snapshot-derived
    /// env-secret checksum drifts from the one applied last; the update
    /// re-reconciles the Secret and rolls pods via the checksum annotation.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let Some(daemon_set_name) = self.daemon_set_name.as_ref() else {
            return Ok(false);
        };
        let config = ctx.desired_resource_config::<Daemon>()?;
        Ok(self.env_secret.drifted(
            &config.id,
            daemon_set_name,
            &ctx.deployment_config.environment_variables.variables,
        ))
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.daemon_set_name.as_ref().map(|daemon_set_name| {
            ResourceOutputs::new(DaemonOutputs {
                daemon_name: daemon_set_name.clone(),
                running: true,
                public_endpoints: std::collections::HashMap::new(),
            })
        })
    }
}

impl KubernetesDaemonController {
    async fn daemonset_ready(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let daemon_set_name = self.daemon_set_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "DaemonSet name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let workload_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        match workload_client
            .get_daemonset(namespace, daemon_set_name)
            .await
        {
            Ok(daemonset) => {
                if let Some(status) = &daemonset.status {
                    return Ok(status.number_ready >= status.desired_number_scheduled
                        && status.desired_number_scheduled > 0);
                }
                Ok(false)
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to get daemonset '{}'.", daemon_set_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    async fn build_daemonset(
        &self,
        config: &Daemon,
        daemon_set_name: &str,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        env_secret_plan: Option<&KubernetesEnvSecretPlan>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<DaemonSet> {
        let selector_labels = self.build_labels(daemon_set_name);
        let labels = self.workload_labels(ctx, &config.id, selector_labels.clone());
        // Source-built daemons are supported: `alien build` compiles the
        // source into an image whose compiled binary is the direct
        // entrypoint, deployed here as a DaemonSet. Reaching here with
        // unbuilt source means the build step was skipped.
        let image = match &config.code {
            DaemonCode::Image { image } => image.clone(),
            DaemonCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message:
                        "Daemon still has unbuilt source code. Run 'alien build' first; it compiles the source into an image the controller can deploy."
                            .to_string(),
                }));
            }
        };

        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_daemon_runtime_env_vars(ctx)?
            // Cross-target parity with the local controller: apps read
            // ALIEN_PUBLIC_ENDPOINTS_JSON to build their own absolute URLs.
            .add_current_resource_public_endpoint(ctx, &config.id)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        // Command-enabled Daemons no longer get `ALIEN_TRANSPORT=passthrough`.
        // Their receiver config (`ALIEN_COMMANDS_*`) is injected per-resource into
        // `config.environment` by the manager/operator snapshot (ALIEN-222); the
        // `ALIEN_COMMANDS_TOKEN` Secret is projected via secretKeyRef like any
        // other resource secret (handled below).

        let (env_map, bindings) = env_builder.build_with_bindings();

        // Daemons project Secret-kind env vars (e.g. ALIEN_COMMANDS_TOKEN) as
        // secretKeyRefs and never load secrets at runtime, so the ALIEN_SECRETS
        // vault-load pointer is stripped from the manifest.
        let env_vars = projected_env_vars(env_secret_plan, bindings, env_map, true)?;

        let container = Container {
            name: "daemon".to_string(),
            image: Some(image),
            // The daemon contract: `command` overrides the image default
            // entrypoint, and the declared ResourceSpecs become real
            // requests/limits — without them the pod schedules as BestEffort
            // and is first in line for eviction. Mirrors the container
            // controller.
            command: config.command.clone(),
            env: Some(env_vars),
            resources: Some(ResourceRequirements {
                requests: Some(BTreeMap::from([
                    ("cpu".to_string(), Quantity(config.cpu.min.clone())),
                    ("memory".to_string(), Quantity(config.memory.min.clone())),
                ])),
                limits: Some(BTreeMap::from([
                    ("cpu".to_string(), Quantity(config.cpu.desired.clone())),
                    (
                        "memory".to_string(),
                        Quantity(config.memory.desired.clone()),
                    ),
                ])),
                ..Default::default()
            }),
            ..Default::default()
        };

        let image_pull_secrets = image_pull_secret_name.map(|name| {
            vec![LocalObjectReference {
                name: name.to_string(),
            }]
        });

        let pod_spec = PodSpec {
            service_account_name: Some(service_account_name.to_string()),
            containers: vec![container],
            restart_policy: Some("Always".to_string()),
            image_pull_secrets,
            // Honor the configured stop grace period during pod shutdown
            // (K8s default is 30s when unset).
            termination_grace_period_seconds: config.stop_grace_period_seconds.map(i64::from),
            ..Default::default()
        };

        let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());
        // Roll pods when the env Secret changes (e.g. token rotation) by stamping
        // its checksum onto the pod template — matches the container controller.
        let pod_annotations = env_secret_plan.map(|plan| {
            BTreeMap::from([("env-secret-checksum".to_string(), plan.checksum.clone())])
        });

        Ok(DaemonSet {
            metadata: ObjectMeta {
                name: Some(daemon_set_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DaemonSetSpec {
                selector: LabelSelector {
                    match_labels: Some(selector_labels),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(pod_labels),
                        annotations: pod_annotations,
                        ..Default::default()
                    }),
                    spec: Some(pod_spec),
                },
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn build_labels(&self, daemon_set_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), daemon_set_name.to_string());
        labels.insert("managed-by".to_string(), "runtime".to_string());
        labels.insert("component".to_string(), "daemon".to_string());
        labels
    }

    fn workload_labels(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        mut labels: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        labels.extend(kubernetes_branded_resource_labels(ctx, resource_id));
        labels
    }

    /// Whether an existing DaemonSet carries our managed-by labels and may be
    /// adopted on an AlreadyExists create. Twin of the container controller's
    /// `is_managed_workload`.
    fn is_managed_daemonset(
        &self,
        ctx: &ResourceControllerContext<'_>,
        labels: Option<&BTreeMap<String, String>>,
        daemon_set_name: &str,
    ) -> bool {
        let label_domain = ctx
            .deployment_config
            .label_domain
            .as_deref()
            .unwrap_or(DEFAULT_ALIEN_LABEL_DOMAIN);
        let managed_by_key = branded_tag_key(label_domain, ALIEN_MANAGED_BY_TAG_KEY);
        let default_managed_by_key =
            branded_tag_key(DEFAULT_ALIEN_LABEL_DOMAIN, ALIEN_MANAGED_BY_TAG_KEY);
        labels.is_some_and(|labels| {
            labels.get(&managed_by_key).map(String::as_str) == Some(ALIEN_MANAGED_BY_TAG_VALUE)
                || labels.get(&default_managed_by_key).map(String::as_str)
                    == Some(ALIEN_MANAGED_BY_TAG_VALUE)
                || (labels.get("managed-by").map(String::as_str) == Some("runtime")
                    && labels.get("component").map(String::as_str) == Some("daemon")
                    && labels.get("app").map(String::as_str) == Some(daemon_set_name))
        })
    }

    fn get_kubernetes_namespace(&self, ctx: &ResourceControllerContext<'_>) -> Result<String> {
        let k8s_config = ctx.get_kubernetes_config()?;
        match k8s_config {
            alien_core::KubernetesClientConfig::InCluster { namespace, .. }
            | alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. }
            | alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
                namespace.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: "kubernetes".to_string(),
                        message: "Kubernetes namespace is not configured".to_string(),
                    })
                })
            }
        }
    }
}

async fn create_registry_pull_secret(
    secrets_client: &std::sync::Arc<dyn alien_k8s_clients::SecretsApi>,
    namespace: &str,
    secret_name: &str,
    proxy_host: &str,
    deployment_token: &str,
) -> Result<()> {
    crate::kubernetes_registry::ensure_registry_pull_secret(
        secrets_client,
        namespace,
        secret_name,
        proxy_host,
        deployment_token,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kubernetes_manifest_test_support::{
        assert_secret_key_ref, daemonset_env, pod_template_checksum_annotation, secret_env_var,
        KubernetesManifestTestHarness,
    };
    use crate::core::{environment_secret_plan, ResourceController};
    use alien_core::{
        Resource, ENV_ALIEN_COMMANDS_POLLING_ENABLED, ENV_ALIEN_COMMANDS_POLLING_URL,
        ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_LAMBDA_MODE, ENV_ALIEN_RUNTIME_SEND_OTLP,
        ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT, ENV_ALIEN_WORKER_GRPC_ADDRESS,
    };

    fn manifest_test_daemon(environment: &[(&str, &str)]) -> Daemon {
        let mut config = Daemon::new("agent".to_string())
            .code(DaemonCode::Image {
                image: "registry.example.com/agent:1".to_string(),
            })
            .permissions("default".to_string())
            .build();
        for (name, value) in environment {
            config
                .environment
                .insert(name.to_string(), value.to_string());
        }
        config
    }

    fn manifest_test_controller() -> KubernetesDaemonController {
        KubernetesDaemonController {
            daemon_set_name: Some("agent".to_string()),
            namespace: Some("test-ns".to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn daemonset_manifest_projects_secrets_and_never_carries_alien_secrets() {
        let variables = vec![
            secret_env_var("APP_SECRET", "s3cret", None),
            secret_env_var(
                ENV_ALIEN_COMMANDS_TOKEN,
                "receiver-token",
                Some(vec!["agent"]),
            ),
        ];
        // Simulate a config injected by an older manager that still collapsed
        // secrets: the pointer must be stripped from the manifest regardless.
        let config = manifest_test_daemon(&[
            ("APP_ENV", "prod"),
            (
                ENV_ALIEN_SECRETS,
                "{\"keys\":[\"APP_SECRET\"],\"hash\":\"legacy\"}",
            ),
        ]);
        let plan = environment_secret_plan("agent", "agent", &variables).expect("plan");
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
        let controller = manifest_test_controller();

        let daemonset = controller
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                "agent-sa",
                None,
                Some(&plan),
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");

        let env = daemonset_env(&daemonset);

        // App secret and the command receiver token are native projections.
        assert_secret_key_ref(&env, "APP_SECRET", "agent-env");
        assert_secret_key_ref(&env, ENV_ALIEN_COMMANDS_TOKEN, "agent-env");

        // The runtime vault-load pointer never reaches the manifest.
        assert!(
            !env.iter().any(|var| var.name == ENV_ALIEN_SECRETS),
            "ALIEN_SECRETS must not appear in a Kubernetes DaemonSet manifest"
        );

        // Plain vars still flow through.
        let app_env = env
            .iter()
            .find(|var| var.name == "APP_ENV")
            .expect("plain APP_ENV");
        assert_eq!(app_env.value.as_deref(), Some("prod"));

        // No worker-era runtime env leaks into Daemon manifests.
        for name in [
            ENV_ALIEN_TRANSPORT,
            ENV_ALIEN_WORKER_GRPC_ADDRESS,
            ENV_ALIEN_RUNTIME_SEND_OTLP,
            ENV_ALIEN_LAMBDA_MODE,
            ENV_ALIEN_COMMANDS_POLLING_ENABLED,
            ENV_ALIEN_COMMANDS_POLLING_URL,
        ] {
            assert!(
                !env.iter().any(|var| var.name == name),
                "worker-era env var '{name}' must not appear in a Daemon manifest"
            );
        }

        // The pod template carries the checksum that rolls pods on rotation.
        assert_eq!(
            pod_template_checksum_annotation(&daemonset.spec.expect("spec").template),
            Some(plan.checksum)
        );
    }

    /// The runtime-less daemon contract on Kubernetes: `command` overrides
    /// the image entrypoint, ResourceSpecs become requests/limits (never
    /// BestEffort), and the stop grace period reaches the pod spec.
    #[tokio::test]
    async fn daemonset_manifest_carries_command_resources_and_grace_period() {
        let mut config = manifest_test_daemon(&[]);
        config.command = Some(vec!["/agent".to_string(), "--poll".to_string()]);
        config.cpu = alien_core::ResourceSpec {
            min: "0.25".to_string(),
            desired: "1".to_string(),
        };
        config.memory = alien_core::ResourceSpec {
            min: "256Mi".to_string(),
            desired: "1Gi".to_string(),
        };
        config.stop_grace_period_seconds = Some(90);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), vec![]);
        let controller = manifest_test_controller();

        let daemonset = controller
            .build_daemonset(
                &config,
                "agent",
                "test-ns",
                "agent-sa",
                None,
                None,
                &harness.ctx(),
            )
            .await
            .expect("daemonset manifest");

        let pod_spec = daemonset
            .spec
            .expect("spec")
            .template
            .spec
            .expect("pod spec");
        assert_eq!(pod_spec.termination_grace_period_seconds, Some(90));

        let container = &pod_spec.containers[0];
        assert_eq!(
            container.command.as_deref(),
            Some(&["/agent".to_string(), "--poll".to_string()][..])
        );

        let resources = container.resources.as_ref().expect("resources");
        let requests = resources.requests.as_ref().expect("requests");
        assert_eq!(requests["cpu"].0, "0.25");
        assert_eq!(requests["memory"].0, "256Mi");
        let limits = resources.limits.as_ref().expect("limits");
        assert_eq!(limits["cpu"].0, "1");
        assert_eq!(limits["memory"].0, "1Gi");
    }

    #[tokio::test]
    async fn daemonset_secret_rotation_changes_the_rendered_pod_template() {
        let config = manifest_test_daemon(&[]);
        let controller = manifest_test_controller();

        let mut daemonsets = Vec::new();
        for value in ["v1", "v1", "v2"] {
            let variables = vec![secret_env_var("APP_SECRET", value, None)];
            let plan = environment_secret_plan("agent", "agent", &variables).expect("plan");
            let harness =
                KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
            let daemonset = controller
                .build_daemonset(
                    &config,
                    "agent",
                    "test-ns",
                    "agent-sa",
                    None,
                    Some(&plan),
                    &harness.ctx(),
                )
                .await
                .expect("daemonset manifest");
            daemonsets.push(daemonset);
        }

        assert_eq!(
            daemonsets[0].spec.as_ref().expect("spec").template,
            daemonsets[1].spec.as_ref().expect("spec").template,
            "identical secret values must render an identical pod template"
        );
        assert_ne!(
            pod_template_checksum_annotation(&daemonsets[0].spec.as_ref().expect("spec").template),
            pod_template_checksum_annotation(&daemonsets[2].spec.as_ref().expect("spec").template),
            "rotating a secret value must change the pod template (rollout)"
        );
    }

    #[test]
    fn needs_update_detects_env_secret_rotation() {
        let config = manifest_test_daemon(&[]);
        let original = vec![secret_env_var("APP_SECRET", "v1", None)];
        let original_plan = environment_secret_plan("agent", "agent", &original).expect("plan");

        let mut controller = manifest_test_controller();
        controller.env_secret.record(Some(&original_plan));

        let harness =
            KubernetesManifestTestHarness::new(Resource::new(config.clone()), original.clone());
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        let rotated = vec![secret_env_var("APP_SECRET", "v2", None)];
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), rotated);
        assert!(controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));

        controller.env_secret.record(None);
        let harness = KubernetesManifestTestHarness::new(Resource::new(config), vec![]);
        assert!(!controller
            .needs_update(&harness.ctx())
            .expect("needs_update"));
    }
}
