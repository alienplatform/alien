//! Runtime credential Secrets are updated independently of workload revisions.

use std::collections::BTreeMap;

use alien_client_core::{runtime_credentials::RuntimeCapability, ErrorData as ClientError};
use alien_core::{commands_types::CommandTargetType, ENV_ALIEN_DEPLOYMENT_ID};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use k8s_openapi::{
    api::core::v1::{PodSpec, Secret, SecretVolumeSource, Volume, VolumeMount},
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
    ByteString,
};
use serde_json::json;

use crate::{
    core::{kubernetes_cleanup_resource_labels, ResourceControllerContext},
    error::{ErrorData, Result},
};

pub(crate) const DIRECTORY: &str = "/var/run/alien-runtime";
const VOLUME: &str = "runtime-credentials";

fn invalid(resource_id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(resource_id.to_owned()),
        message: message.to_owned(),
    })
}

pub(crate) fn deployment_id<'a>(
    ctx: &'a ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<&'a str> {
    let mut ids = ctx
        .deployment_config
        .environment_variables
        .variables
        .iter()
        .filter(|variable| variable.name == ENV_ALIEN_DEPLOYMENT_ID);
    let id = ids.next().ok_or_else(|| {
        invalid(
            resource_id,
            "Runtime authority requires a deployment identity",
        )
    })?;
    if ids.next().is_some()
        || id.target_resources.is_some()
        || id.value.is_empty()
        || id.var_type != alien_core::EnvironmentVariableType::Plain
    {
        return Err(invalid(
            resource_id,
            "Runtime deployment identity must be unique, plain and untargeted",
        ));
    }
    Ok(&id.value)
}

/// Acquire new leaves, write them, then publish the exact written command leaf
/// to the dispatcher. Failed writes never replace its last successful generation.
pub(crate) async fn reconcile(
    ctx: &ResourceControllerContext<'_>,
    namespace: &str,
    workload_name: &str,
    resource_id: &str,
    target_type: CommandTargetType,
) -> Result<()> {
    if ctx.deployment_config.runtime_identity_audience.is_none() {
        return Ok(());
    }
    let deployment_id = deployment_id(ctx, resource_id)?;
    let purpose = json!({"type":"receiveCommands", "target":{"resourceId":resource_id,"resourceType":target_type}});
    let commands_enabled = ctx
        .desired_stack
        .command_targets()
        .iter()
        .any(|target| target.resource_id == resource_id && target.resource_type == target_type);
    let command = if commands_enabled {
        Some(
            ctx.service_provider
                .runtime_credential(deployment_id, purpose.clone())
                .await?,
        )
    } else {
        None
    };
    let telemetry = ctx
        .service_provider
        .runtime_credential(deployment_id, json!({"type":"telemetry"}))
        .await?;
    for credential in command.iter().chain(std::iter::once(&telemetry)) {
        validate_credential(credential, resource_id)?;
    }
    let mut data = BTreeMap::new();
    for (name, credential) in command
        .as_ref()
        .map(|value| ("commands.json", value))
        .into_iter()
        .chain(std::iter::once(("telemetry.json", &telemetry)))
    {
        data.insert(
            name.to_owned(),
            ByteString(serde_json::to_vec(credential).into_alien_error().context(
                ErrorData::ResourceConfigInvalid {
                    message: "Cannot encode workload runtime authority".into(),
                    resource_id: Some(resource_id.into()),
                },
            )?),
        );
    }
    let secret_name = format!("{workload_name}-runtime");
    let labels = kubernetes_cleanup_resource_labels(ctx, resource_id);
    let mut desired = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.clone()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        type_: Some("Opaque".to_owned()),
        data: Some(data),
        ..Default::default()
    };
    let client = ctx
        .service_provider
        .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
        .await?;
    match client.create_secret(namespace, &desired).await {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.error,
                Some(ClientError::RemoteResourceConflict { .. })
            ) =>
        {
            let existing = client.get_secret(namespace, &secret_name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Cannot inspect runtime credential Secret before refresh".into(),
                    resource_id: Some(resource_id.into()),
                },
            )?;
            if existing.type_.as_deref() != Some("Opaque")
                || existing.metadata.labels.as_ref().is_none_or(|existing| {
                    labels
                        .iter()
                        .any(|(key, value)| existing.get(key) != Some(value))
                })
            {
                return Err(invalid(
                    resource_id,
                    "Runtime credential Secret belongs to another workload",
                ));
            }
            // Refuse stale concurrent writers. A later renewal will converge on
            // the live generation without regressing a file to older authority.
            let data = existing
                .data
                .as_ref()
                .ok_or_else(|| invalid(resource_id, "Runtime credential Secret has no data"))?;
            for (name, credential) in command
                .as_ref()
                .map(|value| ("commands.json", value))
                .into_iter()
                .chain(std::iter::once(("telemetry.json", &telemetry)))
            {
                let Some(bytes) = data.get(name) else {
                    // A workload may enable commands after its telemetry was installed.
                    continue;
                };
                let previous: RuntimeCapability =
                    serde_json::from_slice(&bytes.0).map_err(|_| {
                        invalid(resource_id, "Runtime credential Secret has invalid data")
                    })?;
                if previous.expires_at > credential.expires_at {
                    return Err(invalid(
                        resource_id,
                        "Refusing a stale runtime credential Secret refresh",
                    ));
                }
            }
            desired.metadata.resource_version =
                Some(existing.metadata.resource_version.ok_or_else(|| {
                    invalid(
                        resource_id,
                        "Runtime credential Secret has no resource version",
                    )
                })?);
            client
                .update_secret(namespace, &secret_name, &desired)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Cannot refresh runtime credential Secret".into(),
                    resource_id: Some(resource_id.into()),
                })?;
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: "Cannot create runtime credential Secret".into(),
                resource_id: Some(resource_id.into()),
            }))
        }
    }
    if let Some(command) = command {
        ctx.service_provider
            .runtime_credential_published(deployment_id, purpose, command)
            .await?;
    }
    Ok(())
}

/// Renew the credential Secrets of the installed Kubernetes workloads and
/// republish their command leaf, without changing any workload. The Operator
/// runs this when it cannot refresh the installed release: an update waits
/// for approval, runs, is stuck, or failed. Workloads being deleted are skipped. `installed` is the stack the workloads run;
/// each resource uses the config recorded in `state`.
pub async fn refresh_installed_credentials(
    state: &alien_core::StackState,
    installed: &alien_core::Stack,
    deployment_config: &alien_core::DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: &std::sync::Arc<dyn crate::core::PlatformServiceProvider>,
) -> Result<()> {
    if deployment_config.runtime_identity_audience.is_none() {
        return Ok(());
    }
    let registry = std::sync::Arc::new(crate::core::ResourceRegistry::with_built_ins());
    let mut first_error = None;
    for (resource_id, resource) in &state.resources {
        // An update may be removing a workload. Writing its Secret would
        // recreate what the delete flow removes.
        if matches!(
            resource.status,
            alien_core::ResourceStatus::Deleting
                | alien_core::ResourceStatus::Deleted
                | alien_core::ResourceStatus::DeleteFailed
        ) {
            continue;
        }
        let Some(internal) = resource.internal_state.as_ref() else {
            continue;
        };
        let installed_workload = match internal.get("type").and_then(|value| value.as_str()) {
            Some("KubernetesWorkerController") => {
                serde_json::from_value::<crate::KubernetesWorkerController>(internal.clone())
                    .ok()
                    .and_then(|controller| controller.deployment_name.zip(controller.namespace))
                    .map(|names| (names, CommandTargetType::Worker))
            }
            Some("KubernetesContainerController") => {
                serde_json::from_value::<crate::KubernetesContainerController>(internal.clone())
                    .ok()
                    .and_then(|controller| controller.workload_name.zip(controller.namespace))
                    .map(|names| (names, CommandTargetType::Container))
            }
            Some("KubernetesDaemonController") => {
                serde_json::from_value::<crate::KubernetesDaemonController>(internal.clone())
                    .ok()
                    .and_then(|controller| controller.daemon_set_name.zip(controller.namespace))
                    .map(|names| (names, CommandTargetType::Daemon))
            }
            _ => None,
        };
        let Some(((workload_name, namespace), target_type)) = installed_workload else {
            continue;
        };
        let ctx = ResourceControllerContext {
            desired_config: &resource.config,
            platform: alien_core::Platform::Kubernetes,
            client_config: client_config.clone(),
            state,
            resource_prefix: &state.resource_prefix,
            registry: &registry,
            desired_stack: installed,
            service_provider,
            deployment_config,
            initial_setup_authority: Default::default(),
            heartbeat_collector: Default::default(),
        };
        let image = resource
            .config
            .downcast_ref::<alien_core::Worker>()
            .and_then(|worker| match &worker.code {
                alien_core::WorkerCode::Image { image } => Some(image.clone()),
                _ => None,
            })
            .or_else(|| {
                resource
                    .config
                    .downcast_ref::<alien_core::Container>()
                    .and_then(|container| match &container.code {
                        alien_core::ContainerCode::Image { image } => Some(image.clone()),
                        _ => None,
                    })
            })
            .or_else(|| {
                resource
                    .config
                    .downcast_ref::<alien_core::Daemon>()
                    .and_then(|daemon| match &daemon.code {
                        alien_core::DaemonCode::Image { image } => Some(image.clone()),
                        _ => None,
                    })
            });
        let result = async {
            reconcile(&ctx, &namespace, &workload_name, resource_id, target_type).await?;
            if let Some(image) = &image {
                crate::kubernetes_registry::refresh_pull_secret(
                    &ctx,
                    &namespace,
                    &workload_name,
                    resource_id,
                    image,
                )
                .await?;
            }
            Ok::<_, AlienError<ErrorData>>(())
        }
        .await;
        // One workload's failure does not stop the others from renewing.
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn validate_credential(credential: &RuntimeCapability, resource_id: &str) -> Result<()> {
    let now = chrono::Utc::now();
    if credential.token.is_empty()
        || credential.expires_at <= now
        || credential.expires_at > now + chrono::Duration::hours(1)
    {
        return Err(invalid(resource_id, "Runtime leaf has an invalid deadline"));
    }
    Ok(())
}

/// Mount the directory: individual subPath mounts would retain an old inode.
pub(crate) fn mount(pod: &mut PodSpec, container_name: &str, workload_name: &str) -> Result<()> {
    let container = pod
        .containers
        .iter_mut()
        .find(|container| container.name == container_name)
        .ok_or_else(|| invalid(workload_name, "Runtime credential container is missing"))?;
    container
        .volume_mounts
        .get_or_insert_with(Vec::new)
        .push(VolumeMount {
            name: VOLUME.into(),
            mount_path: DIRECTORY.into(),
            read_only: Some(true),
            ..Default::default()
        });
    pod.volumes.get_or_insert_with(Vec::new).push(Volume {
        name: VOLUME.into(),
        secret: Some(SecretVolumeSource {
            secret_name: Some(format!("{workload_name}-runtime")),
            default_mode: Some(0o440),
            ..Default::default()
        }),
        ..Default::default()
    });
    pod.security_context
        .get_or_insert_with(Default::default)
        .fs_group
        .get_or_insert(65532);
    Ok(())
}

/// Remove only a Secret carrying this workload's complete ownership scope.
pub(crate) async fn delete_environment_secret(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
    legacy_owner_proven: bool,
) -> Result<()> {
    if ctx.deployment_config.runtime_identity_audience.is_some() {
        let name = format!("{workload_name}-runtime");
        let client = ctx
            .service_provider
            .get_kubernetes_secrets_client(ctx.get_kubernetes_config()?)
            .await?;
        let existing = match client.get_secret(namespace, &name).await {
            Ok(secret) => Some(secret),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                None
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Cannot inspect runtime credential Secret before deletion".into(),
                    resource_id: Some(resource_id.into()),
                }))
            }
        };
        if let Some(existing) = existing {
            let labels = kubernetes_cleanup_resource_labels(ctx, resource_id);
            if existing.type_.as_deref() != Some("Opaque")
                || existing.metadata.labels.as_ref().is_none_or(|existing| {
                    labels
                        .iter()
                        .any(|(key, value)| existing.get(key) != Some(value))
                })
            {
                return Err(invalid(
                    resource_id,
                    "Runtime credential Secret belongs to another workload",
                ));
            }
            match client.delete_secret(namespace, &name).await {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(ClientError::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Cannot delete runtime credential Secret".into(),
                        resource_id: Some(resource_id.into()),
                    }))
                }
            }
        }
    }
    crate::core::delete_environment_secret(
        resource_kind,
        resource_id,
        workload_name,
        namespace,
        ctx,
        legacy_owner_proven,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_core::{EnvironmentVariable, EnvironmentVariableType, Resource, Vault};
    use alien_k8s_clients::secrets::{MockSecretsApi, SecretsApi};
    use chrono::{Duration, Utc};

    use super::*;
    use crate::core::{
        kubernetes_manifest_test_support::KubernetesManifestTestHarness,
        MockPlatformServiceProvider,
    };

    fn capability(token: &str, minutes: i64) -> RuntimeCapability {
        RuntimeCapability {
            token: token.into(),
            expires_at: Utc::now() + Duration::minutes(minutes),
        }
    }

    fn telemetry(secret: &Secret) -> RuntimeCapability {
        serde_json::from_slice(&secret.data.as_ref().unwrap()["telemetry.json"].0).unwrap()
    }

    /// Every ready pass writes the provider's current leaf over the workload's
    /// Secret, so the file is renewed while the previous leaf is still valid.
    /// A writer holding an older leaf than the live Secret never regresses it.
    #[tokio::test]
    async fn each_pass_replaces_the_workload_leaf_and_never_regresses_it() {
        let issued = Arc::new(Mutex::new(vec![
            capability("ax_cap_v1_older", 20),
            capability("ax_cap_v1_renewed", 60),
            capability("ax_cap_v1_first", 30),
        ]));
        let mut provider = MockPlatformServiceProvider::new();
        let queue = issued.clone();
        provider
            .expect_runtime_credential()
            .withf(|deployment, purpose| {
                deployment == "dep-1" && purpose == &json!({"type":"telemetry"})
            })
            .times(3)
            .returning(move |_, _| Ok(queue.lock().unwrap().pop().unwrap()));

        let live: Arc<Mutex<Option<Secret>>> = Arc::new(Mutex::new(None));
        let mut secrets = MockSecretsApi::new();
        let created = live.clone();
        secrets.expect_create_secret().returning(move |_, secret| {
            let mut live = created.lock().unwrap();
            if live.is_some() {
                return Err(AlienError::new(ClientError::RemoteResourceConflict {
                    resource_type: "Secret".into(),
                    resource_name: "agent-runtime".into(),
                    message: "exists".into(),
                }));
            }
            let mut stored = secret.clone();
            stored.metadata.resource_version = Some("1".into());
            *live = Some(stored.clone());
            Ok(stored)
        });
        let read = live.clone();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(read.lock().unwrap().clone().unwrap()));
        let updated = live.clone();
        secrets
            .expect_update_secret()
            .returning(move |_, _, secret| {
                assert_eq!(secret.metadata.resource_version.as_deref(), Some("1"));
                *updated.lock().unwrap() = Some(secret.clone());
                Ok(secret.clone())
            });
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        provider
            .expect_get_kubernetes_secrets_client()
            .returning(move |_| Ok(secrets.clone()));

        let mut harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("agent".to_string()).build()),
            vec![EnvironmentVariable {
                name: ENV_ALIEN_DEPLOYMENT_ID.into(),
                value: "dep-1".into(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            }],
        )
        .with_service_provider(Arc::new(provider));
        harness.deployment_config_mut().runtime_identity_audience =
            Some("https://manager.example".into());
        let ctx = harness.ctx();
        let pass = || {
            reconcile(
                &ctx,
                "test-ns",
                "agent",
                "agent",
                CommandTargetType::Container,
            )
        };

        pass().await.unwrap();
        let first = telemetry(live.lock().unwrap().as_ref().unwrap());
        assert_eq!(first.token, "ax_cap_v1_first");

        pass().await.unwrap();
        let renewed = telemetry(live.lock().unwrap().as_ref().unwrap());
        assert_eq!(renewed.token, "ax_cap_v1_renewed");
        assert!(renewed.expires_at > first.expires_at);

        assert!(pass().await.is_err());
        assert_eq!(
            telemetry(live.lock().unwrap().as_ref().unwrap()).token,
            "ax_cap_v1_renewed"
        );
    }

    /// While an update waits or failed, the Operator cannot refresh the
    /// installed release. The credentials-only pass still renews the Secret of
    /// every installed workload, and leaves other resources alone.
    #[cfg(feature = "test-utils")]
    #[tokio::test]
    async fn installed_workloads_are_renewed_without_a_refresh() {
        let issued = Arc::new(Mutex::new(vec![
            capability("ax_cap_v1_renewed", 60),
            capability("ax_cap_v1_first", 30),
        ]));
        let mut provider = MockPlatformServiceProvider::new();
        let queue = issued.clone();
        provider
            .expect_runtime_credential()
            .withf(|deployment, purpose| {
                deployment == "dep-1" && purpose == &json!({"type":"telemetry"})
            })
            .times(2)
            .returning(move |_, _| Ok(queue.lock().unwrap().pop().unwrap()));
        let live: Arc<Mutex<Option<Secret>>> = Arc::new(Mutex::new(None));
        let mut secrets = MockSecretsApi::new();
        let created = live.clone();
        secrets.expect_create_secret().returning(move |namespace, secret| {
            assert_eq!(namespace, "agents");
            assert_eq!(secret.metadata.name.as_deref(), Some("agent-deploy-runtime"));
            let mut live = created.lock().unwrap();
            if live.is_some() {
                return Err(AlienError::new(ClientError::RemoteResourceConflict {
                    resource_type: "Secret".into(),
                    resource_name: "agent-deploy-runtime".into(),
                    message: "exists".into(),
                }));
            }
            let mut stored = secret.clone();
            stored.metadata.resource_version = Some("1".into());
            *live = Some(stored.clone());
            Ok(stored)
        });
        let read = live.clone();
        secrets
            .expect_get_secret()
            .returning(move |_, _| Ok(read.lock().unwrap().clone().unwrap()));
        let updated = live.clone();
        secrets
            .expect_update_secret()
            .returning(move |_, _, secret| {
                *updated.lock().unwrap() = Some(secret.clone());
                Ok(secret.clone())
            });
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        provider
            .expect_get_kubernetes_secrets_client()
            .returning(move |_| Ok(secrets.clone()));
        let provider: Arc<dyn crate::core::PlatformServiceProvider> = Arc::new(provider);

        let mut harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("unused".to_string()).build()),
            vec![EnvironmentVariable {
                name: ENV_ALIEN_DEPLOYMENT_ID.into(),
                value: "dep-1".into(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            }],
        );
        harness.deployment_config_mut().runtime_identity_audience =
            Some("https://manager.example".into());
        let (deployment_config, client_config) = {
            let ctx = harness.ctx();
            (ctx.deployment_config.clone(), ctx.client_config.clone())
        };

        let worker = alien_core::Worker::new("agent".into())
            .code(alien_core::WorkerCode::Image {
                image: "example.test/agent:1".into(),
            })
            .permissions("default".into())
            .build();
        let installed = alien_core::Stack::new("app".into())
            .add(worker.clone(), alien_core::ResourceLifecycle::Live)
            .add(Vault::new("files".into()).build(), alien_core::ResourceLifecycle::Frozen)
            .build();
        let mut state = alien_core::StackState::new(alien_core::Platform::Kubernetes);
        state.resources.insert(
            "agent".into(),
            alien_core::StackResourceState::builder()
                .resource_type("worker".to_string())
                .status(alien_core::ResourceStatus::Running)
                .config(Resource::new(worker))
                .internal_state({
                    // Stored controller state carries its type, as the executor writes it.
                    let mut internal = serde_json::to_value(
                        crate::KubernetesWorkerController::mock_ready("agent-deploy", "agents"),
                    )
                    .unwrap();
                    internal["type"] = json!("KubernetesWorkerController");
                    internal
                })
                .build(),
        );
        // A workload the update is removing keeps no credential Secret.
        state.resources.insert(
            "old".into(),
            alien_core::StackResourceState::builder()
                .resource_type("worker".to_string())
                .status(alien_core::ResourceStatus::Deleting)
                .config(Resource::new(
                    alien_core::Worker::new("old".into())
                        .code(alien_core::WorkerCode::Image {
                            image: "example.test/old:1".into(),
                        })
                        .permissions("default".into())
                        .build(),
                ))
                .internal_state({
                    let mut internal = serde_json::to_value(
                        crate::KubernetesWorkerController::mock_ready("old-deploy", "agents"),
                    )
                    .unwrap();
                    internal["type"] = json!("KubernetesWorkerController");
                    internal
                })
                .build(),
        );
        state.resources.insert(
            "files".into(),
            alien_core::StackResourceState::builder()
                .resource_type("vault".to_string())
                .status(alien_core::ResourceStatus::Running)
                .config(Resource::new(Vault::new("files".into()).build()))
                .internal_state(json!({"type": "KubernetesVaultController"}))
                .build(),
        );

        let pass = || {
            refresh_installed_credentials(
                &state,
                &installed,
                &deployment_config,
                client_config.clone(),
                &provider,
            )
        };
        pass().await.unwrap();
        assert_eq!(
            telemetry(live.lock().unwrap().as_ref().unwrap()).token,
            "ax_cap_v1_first"
        );
        pass().await.unwrap();
        assert_eq!(
            telemetry(live.lock().unwrap().as_ref().unwrap()).token,
            "ax_cap_v1_renewed"
        );
    }

}
