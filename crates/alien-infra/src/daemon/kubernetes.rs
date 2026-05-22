use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::{EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    kubernetes_resource_name, kubernetes_service_account_name, Daemon, DaemonCode, DaemonOutputs,
    ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, LocalObjectReference, PodSpec, PodTemplateSpec, Secret,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

#[controller]
pub struct KubernetesDaemonController {
    /// The name of the created Kubernetes Deployment.
    pub(crate) deployment_name: Option<String>,
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

        let deployment_name = kubernetes_resource_name(&ctx.resource_prefix, &config.id);
        let namespace = self.get_kubernetes_namespace(ctx)?;
        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());

        let image_pull_secret_name = if matches!(config.code, DaemonCode::Image { .. }) {
            let token = ctx.deployment_config.deployment_token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let proxy_host = ctx
                .deployment_config
                .manager_url
                .as_deref()
                .map(alien_core::image_rewrite::strip_url_scheme)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "manager_url is required for Kubernetes registry pull credentials"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            let secret_name = format!("{}-registry", deployment_name);
            let secrets_client = ctx
                .service_provider
                .get_kubernetes_secrets_client(kubernetes_config)
                .await?;
            create_registry_pull_secret(
                &secrets_client,
                &namespace,
                &secret_name,
                proxy_host,
                token,
            )
            .await?;
            Some(secret_name)
        } else {
            None
        };

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let deployment = self
            .build_deployment(
                config,
                &deployment_name,
                &namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                ctx,
            )
            .await?;

        deployment_client
            .create_deployment(&namespace, &deployment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create daemon deployment '{}'.", deployment_name),
                resource_id: Some(config.id.clone()),
            })?;

        self.deployment_name = Some(deployment_name.clone());
        self.namespace = Some(namespace.clone());

        info!(deployment_name=%deployment_name, namespace=%namespace, "Daemon deployment creation initiated");

        Ok(HandlerAction::Continue {
            state: WaitingForDeployment,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForDeployment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_deployment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.deployment_ready(ctx).await? {
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
        if !self.deployment_ready(ctx).await? {
            let config = ctx.desired_resource_config::<Daemon>()?;
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Daemon deployment is not fully ready".to_string(),
            }));
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
        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        let existing = deployment_client
            .get_deployment(namespace, deployment_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get deployment '{}' before update",
                    deployment_name
                ),
                resource_id: Some(config.id.clone()),
            })?;
        let resource_version = existing.metadata.resource_version.clone();

        let service_account_name =
            kubernetes_service_account_name(&ctx.resource_prefix, config.get_permissions());
        let image_pull_secret_name = if matches!(config.code, DaemonCode::Image { .. }) {
            if ctx.deployment_config.deployment_token.is_none() {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "deployment_token is required for Kubernetes to pull images from the registry proxy".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
            Some(format!("{}-registry", deployment_name))
        } else {
            None
        };

        let mut new_deployment = self
            .build_deployment(
                config,
                deployment_name,
                namespace,
                &service_account_name,
                image_pull_secret_name.as_deref(),
                ctx,
            )
            .await?;
        new_deployment.metadata.resource_version = resource_version;

        deployment_client
            .update_deployment(namespace, deployment_name, &new_deployment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update daemon deployment '{}'.", deployment_name),
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
        if self.deployment_ready(ctx).await? {
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

        if let Some(deployment_name) = &self.deployment_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match deployment_client
                .delete_deployment(namespace, deployment_name)
                .await
            {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.deployment_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete deployment '{}'.", deployment_name),
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

        if let Some(deployment_name) = &self.deployment_name {
            let deployment_client = ctx
                .service_provider
                .get_kubernetes_deployment_client(kubernetes_config)
                .await?;
            match deployment_client
                .get_deployment(namespace, deployment_name)
                .await
            {
                Ok(_) => {
                    debug!(deployment_name=%deployment_name, "Daemon deployment still exists");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.deployment_name = None;
                    self.namespace = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get deployment '{}'.", deployment_name),
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
        self.deployment_name.as_ref().map(|deployment_name| {
            ResourceOutputs::new(DaemonOutputs {
                daemon_name: deployment_name.clone(),
                running: true,
            })
        })
    }
}

impl KubernetesDaemonController {
    async fn deployment_ready(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let config = ctx.desired_resource_config::<Daemon>()?;
        let deployment_name = self.deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Deployment name not set in state".to_string(),
            })
        })?;
        let namespace = self.namespace.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Namespace not set in state".to_string(),
            })
        })?;

        let deployment_client = ctx
            .service_provider
            .get_kubernetes_deployment_client(kubernetes_config)
            .await?;
        match deployment_client
            .get_deployment(namespace, deployment_name)
            .await
        {
            Ok(deployment) => {
                if let Some(status) = &deployment.status {
                    if let (Some(ready_replicas), Some(replicas)) =
                        (status.ready_replicas, status.replicas)
                    {
                        return Ok(ready_replicas >= replicas && replicas > 0);
                    }
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
                message: format!("Failed to get deployment '{}'.", deployment_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    async fn build_deployment(
        &self,
        config: &Daemon,
        deployment_name: &str,
        namespace: &str,
        service_account_name: &str,
        image_pull_secret_name: Option<&str>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Deployment> {
        let labels = self.build_labels(deployment_name);
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
            .add_passthrough_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?;
        let (env_map, bindings) = env_builder.build_with_bindings();

        let mut env_vars = Vec::new();
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

        Ok(Deployment {
            metadata: ObjectMeta {
                name: Some(deployment_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(1),
                selector: LabelSelector {
                    match_labels: Some(labels.clone()),
                    ..Default::default()
                },
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some(labels),
                        ..Default::default()
                    }),
                    spec: Some(pod_spec),
                },
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn build_labels(&self, deployment_name: &str) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), deployment_name.to_string());
        labels.insert("managed-by".to_string(), "alien".to_string());
        labels.insert("component".to_string(), "daemon".to_string());
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
    use base64::engine::{general_purpose::STANDARD as BASE64, Engine as _};

    let auth = BASE64.encode(format!("deployment:{}", deployment_token));
    let mut auths = serde_json::Map::new();
    auths.insert(
        proxy_host.to_string(),
        serde_json::json!({
            "username": "deployment",
            "password": deployment_token,
            "auth": auth,
        }),
    );
    let docker_config = serde_json::json!({ "auths": auths });

    let docker_config_bytes = serde_json::to_vec(&docker_config)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to serialize Docker config".to_string(),
            resource_id: None,
        })?;

    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some({
            let mut data = BTreeMap::new();
            data.insert(
                ".dockerconfigjson".to_string(),
                k8s_openapi::ByteString(docker_config_bytes),
            );
            data
        }),
        ..Default::default()
    };

    match secrets_client.create_secret(namespace, &secret).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("AlreadyExists") || err_str.contains("409") {
                Ok(())
            } else {
                Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create registry pull secret '{}'", secret_name),
                    resource_id: None,
                }))
            }
        }
    }
}
