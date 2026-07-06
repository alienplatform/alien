use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_runtime_pod_labels,
    reconcile_environment_secret, EnvironmentVariableBuilder, KubernetesEnvSecretPlan,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use crate::kubernetes_workload_heartbeat::{
    emit_kubernetes_workload_heartbeat, label_selector, KubernetesWorkload,
    KubernetesWorkloadDataKind, KubernetesWorkloadHeartbeatInput,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_resource_name, kubernetes_service_account_name, Daemon, DaemonCode, DaemonOutputs,
    ResourceOutputs, ResourceStatus, ENV_ALIEN_SECRETS,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use k8s_openapi::api::apps::v1::{DaemonSet, DaemonSetSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, LocalObjectReference, PodSpec, PodTemplateSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

#[controller]
pub struct KubernetesDaemonController {
    /// The name of the created Kubernetes DaemonSet.
    pub(crate) daemon_set_name: Option<String>,
    /// The namespace where resources are deployed.
    pub(crate) namespace: Option<String>,
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

        workload_client
            .create_daemonset(&namespace, &daemonset)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create daemonset '{}'.", daemon_set_name),
                resource_id: Some(config.id.clone()),
            })?;

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
        let image = match &config.code {
            DaemonCode::Image { image } => image.clone(),
            DaemonCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: config.id.clone(),
                    message: "Source-based daemons are not yet supported on Kubernetes".to_string(),
                }));
            }
        };

        let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
            .add_standard_alien_env_vars(ctx)?
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;

        // Command-enabled Daemons no longer get `ALIEN_TRANSPORT=passthrough`.
        // Their receiver config (`ALIEN_COMMANDS_*`) is injected per-resource into
        // `config.environment` by the manager/operator snapshot (ALIEN-222); the
        // `ALIEN_COMMANDS_TOKEN` Secret flows through the same ALIEN_SECRETS →
        // secretKeyRef path as any other resource secret (handled below).

        let (env_map, bindings) = env_builder.build_with_bindings();

        let mut env_vars = Vec::new();

        // Secret-kind env vars scoped to this Daemon (e.g. ALIEN_COMMANDS_TOKEN)
        // are rendered as secretKeyRefs into the `{daemon}-env` Secret, mirroring
        // the container controller.
        if let Some(plan) = env_secret_plan {
            for key in &plan.keys {
                env_vars.push(EnvVar {
                    name: key.clone(),
                    value: None,
                    value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                        secret_key_ref: Some(k8s_openapi::api::core::v1::SecretKeySelector {
                            name: plan.secret_name.clone(),
                            key: key.clone(),
                            optional: Some(false),
                        }),
                        ..Default::default()
                    }),
                });
            }
        }

        for (binding_name, binding_json) in bindings {
            if let Ok(extraction) = crate::core::k8s_secret_bindings::extract_binding_secrets(
                &binding_name,
                &binding_json,
            ) {
                for (env_name, secret_name, secret_key) in extraction.secret_env_vars {
                    env_vars.push(EnvVar {
                        name: env_name,
                        value: None,
                        value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                            secret_key_ref: Some(k8s_openapi::api::core::v1::SecretKeySelector {
                                name: secret_name,
                                key: secret_key,
                                optional: Some(false),
                            }),
                            ..Default::default()
                        }),
                    });
                }

                let env_key = format!(
                    "ALIEN_{}_BINDING",
                    binding_name.to_uppercase().replace('-', "_")
                );
                env_vars.push(EnvVar {
                    name: env_key,
                    value: Some(extraction.resolved_binding_json),
                    value_from: None,
                });
            }
        }

        for (key, value) in env_map {
            // When an env Secret plan exists, the runtime reads its secrets from
            // the mounted secretKeyRefs, so the ALIEN_SECRETS vault-load pointer is
            // redundant and dropped (matches the container controller).
            if key == ENV_ALIEN_SECRETS && env_secret_plan.is_some() {
                continue;
            }
            if !env_vars.iter().any(|ev| ev.name == key) {
                env_vars.push(EnvVar {
                    name: key,
                    value: Some(value),
                    value_from: None,
                });
            }
        }

        let container = Container {
            name: "daemon".to_string(),
            image: Some(image),
            env: Some(env_vars),
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
