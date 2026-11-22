//! Before the person running destroy deletes a Kubernetes install's workloads themselves (the
//! Operator is gone), prove the shell points at the install's cluster. A delete on another
//! cluster finds nothing, which reads as "already deleted" while the workloads keep running.

use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    ClientConfig, KubernetesClientConfig, ResourceLifecycle, ResourceStatus, StackState,
};
use alien_error::{AlienError, Context, ContextError};
use alien_infra::PlatformServiceProvider;
use serde_json::Value;

/// A workload the deployment record names and has not yet recorded as deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecordedWorkload {
    pub resource_id: String,
    pub namespace: String,
    pub kind: WorkloadKind,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
    Job,
}

fn text(state: &Value, snake: &str, camel: &str) -> Option<String> {
    state
        .get(snake)
        .or_else(|| state.get(camel))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// The workloads the record says still exist: every Live resource not yet deleted whose
/// controller state recorded a namespace and the name of what it created.
pub(super) fn recorded_workloads(stack_state: &StackState) -> Vec<RecordedWorkload> {
    let mut found = Vec::new();
    for (resource_id, resource) in &stack_state.resources {
        if resource.lifecycle != Some(ResourceLifecycle::Live)
            || resource.status == ResourceStatus::Deleted
        {
            continue;
        }
        let Some(state) = resource.internal_state.as_ref() else {
            continue;
        };
        let Some(namespace) = text(state, "namespace", "namespace") else {
            continue;
        };
        let stateful = state
            .get("is_stateful")
            .or_else(|| state.get("isStateful"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let named = [
            (
                text(state, "workload_name", "workloadName"),
                if stateful {
                    WorkloadKind::StatefulSet
                } else {
                    WorkloadKind::Deployment
                },
            ),
            (
                text(state, "deployment_name", "deploymentName"),
                WorkloadKind::Deployment,
            ),
            (
                text(state, "daemon_set_name", "daemonSetName"),
                WorkloadKind::DaemonSet,
            ),
            (text(state, "job_name", "jobName"), WorkloadKind::Job),
        ];
        for (name, kind) in named {
            if let Some(name) = name {
                found.push(RecordedWorkload {
                    resource_id: resource_id.clone(),
                    namespace: namespace.clone(),
                    kind,
                    name,
                });
            }
        }
    }
    found
}

/// The label selector for this deployment's runtime objects: `managed-by=runtime` plus the
/// deployment scope label the runtime controllers put on every object they create. Its key is
/// `<label domain>/deployment` and its value is the stack's resource prefix (the same helper the
/// controllers use, `kubernetes_deployment_scope_label_for`; a missing label domain means the
/// default one, as for the controllers). The chart's cleanup hook selects the same pair through
/// the release's runtime label value. Without a recorded resource prefix the label cannot be
/// built: only `managed-by=runtime` can be asked for, and the result is not this deployment's
/// alone.
pub(super) fn runtime_selector(
    label_domain: Option<&str>,
    resource_prefix: Option<&str>,
) -> String {
    match resource_prefix.filter(|prefix| !prefix.is_empty()) {
        Some(prefix) => {
            let (key, value) = alien_infra::kubernetes_deployment_scope_label_for(
                label_domain.filter(|domain| !domain.is_empty()),
                prefix,
            );
            format!("managed-by=runtime,{key}={value}")
        }
        None => "managed-by=runtime".to_string(),
    }
}

/// The objects the runtime controllers label `managed-by=runtime` that are still in `namespace`:
/// "Kind name" each. After the runtime cleanup the deployment's own workloads are gone; what this
/// lists is what no controller deletes (for example a registry Secret of a Container). Reading
/// them is only for a warning: a failure to read says so and changes nothing. PersistentVolumeClaims
/// are not read (this client has no list call for them).
pub(super) async fn leftover_runtime_objects(
    config: &ClientConfig,
    namespace: &str,
    label_selector: &str,
    provider: &dyn PlatformServiceProvider,
) -> std::result::Result<Vec<String>, String> {
    let ClientConfig::Kubernetes(kubernetes) = config else {
        return Ok(Vec::new());
    };
    let kubernetes: &KubernetesClientConfig = kubernetes.as_ref();
    let selector = || Some(label_selector.to_string());
    let failed = |what: &str, error: &dyn std::fmt::Display| format!("{what}: {error}");
    let deployments = provider
        .get_kubernetes_deployment_client(kubernetes)
        .await
        .map_err(|e| failed("workloads", &e))?;
    let jobs = provider
        .get_kubernetes_job_client(kubernetes)
        .await
        .map_err(|e| failed("jobs", &e))?;
    let services = provider
        .get_kubernetes_service_client(kubernetes)
        .await
        .map_err(|e| failed("services", &e))?;
    let secrets = provider
        .get_kubernetes_secrets_client(kubernetes)
        .await
        .map_err(|e| failed("secrets", &e))?;
    let mut left = Vec::new();
    for item in deployments
        .list_deployments(namespace, selector(), None)
        .await
        .map_err(|e| failed("Deployments", &e))?
        .items
    {
        left.push(format!(
            "Deployment {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    for item in deployments
        .list_statefulsets(namespace, selector(), None)
        .await
        .map_err(|e| failed("StatefulSets", &e))?
        .items
    {
        left.push(format!(
            "StatefulSet {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    for item in deployments
        .list_daemonsets(namespace, selector(), None)
        .await
        .map_err(|e| failed("DaemonSets", &e))?
        .items
    {
        left.push(format!(
            "DaemonSet {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    for item in jobs
        .list_jobs(namespace, selector(), None)
        .await
        .map_err(|e| failed("Jobs", &e))?
        .items
    {
        left.push(format!(
            "Job {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    for item in services
        .list_services(namespace, selector(), None)
        .await
        .map_err(|e| failed("Services", &e))?
        .items
    {
        left.push(format!(
            "Service {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    for item in secrets
        .list_secrets(namespace, selector(), None)
        .await
        .map_err(|e| failed("Secrets", &e))?
        .items
    {
        left.push(format!(
            "Secret {}",
            item.metadata.name.clone().unwrap_or_default()
        ));
    }
    Ok(left)
}

/// The one-line warning for [`leftover_runtime_objects`], or none when nothing is left. `scoped`:
/// the list was limited to this deployment's label. Unscoped, it is every runtime-managed object in
/// the namespace, which may include another install's, so no delete command is offered.
pub(super) fn leftover_warning(namespace: &str, left: &[String], scoped: bool) -> Option<String> {
    if left.is_empty() {
        return None;
    }
    let objects = left.join(", ");
    if scoped {
        Some(format!(
            "Objects this deployment's runtime created are still in namespace '{namespace}': {objects}. The delete goes on; remove them with kubectl if you do not want them. This list is not exhaustive: objects that carry only the legacy label key, unlabelled objects and PersistentVolumeClaims (kept by default) are not listed."
        ))
    } else {
        Some(format!(
            "The runtime-managed objects still in namespace '{namespace}' are: {objects}. This command does not know the deployment's label, so the list is every runtime-managed object in the namespace and may include another install's. Check before removing anything. This list is not exhaustive: objects that carry only the legacy label key, unlabelled objects and PersistentVolumeClaims are not listed."
        ))
    }
}

/// What showed the cluster to be the install's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClusterProof {
    /// A workload the record names exists on it.
    RecordedWorkload,
    /// No recorded workload was found; only the Operator's Secret in the release's namespace
    /// was. Another cluster with a same-named namespace and Operator Secret looks the same, so
    /// the person must confirm the cluster (see `confirm_secret_only_cluster`).
    OperatorSecretOnly { server_url: String },
}

/// Refuse unless the cluster `config` reaches is the install's: at least one workload the record
/// names exists on it, or (when none does, or the record names none) the Operator's own Secret
/// exists in the release's namespace. A record that names no workload proves nothing about the
/// cluster, so it still needs the Secret: an empty list must not pass on any cluster.
pub(super) async fn verify_install_cluster(
    config: &ClientConfig,
    stack_state: Option<&StackState>,
    helm_release: Option<&alien_core::InstalledHelmRelease>,
    provider: &dyn PlatformServiceProvider,
) -> Result<ClusterProof> {
    let workloads = stack_state.map(recorded_workloads).unwrap_or_default();
    // Any other kind of client config is one this check cannot verify, so it does not pass.
    let ClientConfig::Kubernetes(kubernetes) = config else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "kube_context".to_string(),
            message: "Cannot confirm the cluster this command would delete on: its credentials are not a Kubernetes client configuration. Nothing was deleted.".to_string(),
        }));
    };
    let kubernetes: &KubernetesClientConfig = kubernetes.as_ref();
    let deployments = provider
        .get_kubernetes_deployment_client(kubernetes)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to create the Kubernetes client for the cluster check".to_string(),
        })?;
    let jobs = provider
        .get_kubernetes_job_client(kubernetes)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to create the Kubernetes client for the cluster check".to_string(),
        })?;
    let mut missing = Vec::new();
    for workload in &workloads {
        let result = match workload.kind {
            WorkloadKind::Deployment => deployments
                .get_deployment(&workload.namespace, &workload.name)
                .await
                .map(|_| ()),
            WorkloadKind::StatefulSet => deployments
                .get_statefulset(&workload.namespace, &workload.name)
                .await
                .map(|_| ()),
            WorkloadKind::DaemonSet => deployments
                .get_daemonset(&workload.namespace, &workload.name)
                .await
                .map(|_| ()),
            WorkloadKind::Job => jobs
                .get_job(&workload.namespace, &workload.name)
                .await
                .map(|_| ()),
        };
        match result {
            Ok(()) => return Ok(ClusterProof::RecordedWorkload),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                missing.push(format!(
                    "{} in namespace {}",
                    workload.name, workload.namespace
                ));
            }
            // An unreachable cluster or a refused credential is not proof of the wrong cluster,
            // and not a reason to go on either.
            Err(error) => {
                return Err(ContextError::context(error, ErrorData::ConfigurationError {
                    message: format!(
                        "Could not read '{}' in namespace '{}' to confirm this is the install's cluster",
                        workload.name, workload.namespace
                    ),
                }))
            }
        }
    }
    // The chart's uninstall hook removes the workloads the Operator created, so none being left
    // is the usual state after `helm uninstall`. A record that names none (only a vault or
    // service account, or state this check cannot read) is the same case. What stays is the Secret the Operator created
    // for itself (`<release>-operator`, which Helm does not own): finding it in the release's
    // namespace is what separates that state from another cluster. A namespace alone would
    // pass on any cluster that has one of that name.
    if let Some(release) = helm_release {
        let resolved = alien_infra::resolve_kubeconfig(kubernetes).await.context(
            ErrorData::ConfigurationError {
                message: "Failed to resolve the kubeconfig for the cluster check".to_string(),
            },
        )?;
        let server_url = match &resolved {
            KubernetesClientConfig::Manual { server_url, .. } => server_url.clone(),
            _ => "the in-cluster API server".to_string(),
        };
        let client = alien_k8s_clients::KubernetesClient::new(resolved)
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to create the Kubernetes client for the cluster check".to_string(),
            })?;
        // The chart names the Secret after its full name (`fullnameOverride` changes it), which
        // the record does not hold. `<release>-operator` is the usual name; the label the
        // Operator puts on the Secret it creates finds the others. Limit: this label match
        // accepts ANY Operator Secret in a namespace of this name; that is why a proof by the
        // Secret alone is confirmed by the person (`ClusterProof::OperatorSecretOnly`).
        let secret = format!("{}-operator", release.name);
        let by_name = match client.get_secret(&release.namespace, &secret).await {
            Ok(_) => return Ok(ClusterProof::OperatorSecretOnly { server_url }),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        };
        let found = match by_name {
            Ok(()) => client
                .list_secrets(
                    &release.namespace,
                    Some("app.kubernetes.io/managed-by=alien-operator".to_string()),
                    None,
                )
                .await
                .map(|list| !list.items.is_empty()),
            Err(error) => Err(error),
        };
        match found {
            Ok(true) => return Ok(ClusterProof::OperatorSecretOnly { server_url }),
            Ok(false) => {}
            // The namespace itself is gone.
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(ContextError::context(
                    error,
                    ErrorData::ConfigurationError {
                        message: format!(
                            "Could not read the Operator's Secret in namespace '{}' to confirm this is the install's cluster",
                            release.namespace
                        ),
                    },
                ))
            }
        }
    }
    let named = if missing.is_empty() {
        "the record names no workload to look for".to_string()
    } else {
        format!(
            "no workload the record names ({}) exists",
            missing.join(", ")
        )
    };
    Err(AlienError::new(ErrorData::ValidationError {
        field: "kube_context".to_string(),
        message: format!(
            "Cannot confirm the cluster your kubeconfig points at is the install's: {named}, and the Operator's own Secret was not found in the release's namespace{}. Nothing was deleted. If this is the cluster the chart was installed on, the Operator's Secret and workloads are gone from it (removed by hand, or the namespace was deleted): there is nothing left here to clean, and `--force-delete-record` removes only the deployment's record without any cleanup. Otherwise point --kubeconfig and --kube-context at the cluster the chart was installed on.",
            if helm_release.is_none() {
                " (the record names no Helm release to look in)"
            } else {
                ""
            }
        ),
    }))
}

/// Fix the kubeconfig file and the context once, before the cluster check. Every Kubernetes
/// client re-reads the kubeconfig when it is built, so with the path and context left implicit
/// a `kubectl config use-context` made while this command waits (the runtime acquire can wait
/// minutes) would send the cleanup to a cluster that was never checked. The returned
/// configuration names the file, the context, and the context's cluster and user explicitly:
/// the check, the runtime cleanup, the setup teardown and the listing then all read the same
/// entries. Names are not content: see `api_server_url` and `ensure_same_server`.
///
/// A kubeconfig that cannot be read or has no usable context cannot be pinned, and the command
/// refuses: leaving it unpinned would let any later change redirect the cleanup.
pub(super) fn pin_kubeconfig(config: ClientConfig) -> Result<ClientConfig> {
    let ClientConfig::Kubernetes(mut kubernetes) = config else {
        return Ok(config);
    };
    if let KubernetesClientConfig::Kubeconfig {
        kubeconfig_path,
        context,
        cluster,
        user,
        ..
    } = kubernetes.as_mut()
    {
        let unpinnable = |why: String| {
            AlienError::new(ErrorData::ValidationError {
                field: "kube_context".to_string(),
                message: format!(
                    "Cannot fix the kubeconfig this command deletes on ({why}). Nothing was deleted. Point --kubeconfig and --kube-context at a readable kubeconfig that names a context."
                ),
            })
        };
        let path = match kubeconfig_path.as_deref() {
            Some(path) => std::path::PathBuf::from(path),
            None => alien_infra::Kubeconfig::kubeconfig_path()
                .map_err(|error| unpinnable(format!("no kubeconfig path: {error}")))?,
        };
        let path = std::path::absolute(&path).unwrap_or(path);
        let file = alien_infra::Kubeconfig::read_from(path.clone())
            .map_err(|error| unpinnable(format!("{}: {error}", path.display())))?;
        let loader = alien_infra::ConfigLoader::load(
            file.clone(),
            context.as_ref(),
            cluster.as_ref(),
            user.as_ref(),
        )
        .map_err(|error| unpinnable(format!("{}: {error}", path.display())))?;
        *kubeconfig_path = Some(path.display().to_string());
        if context.is_none() {
            *context = file.current_context;
        }
        if cluster.is_none() {
            *cluster = Some(loader.current_context.cluster);
        }
        if user.is_none() {
            *user = loader.current_context.user;
        }
    }
    Ok(ClientConfig::Kubernetes(kubernetes))
}

/// The API server URL a kubeconfig-based configuration resolves to right now (`None` for other
/// kinds of configuration, which name their server themselves and are not re-read).
pub(super) async fn api_server_url(config: &ClientConfig, at_pin: bool) -> Result<Option<String>> {
    let ClientConfig::Kubernetes(kubernetes) = config else {
        return Ok(None);
    };
    if !matches!(kubernetes.as_ref(), KubernetesClientConfig::Kubeconfig { .. }) {
        return Ok(None);
    }
    let resolved = alien_infra::resolve_kubeconfig(kubernetes.as_ref())
        .await
        .context(ErrorData::ConfigurationError {
            message: if at_pin {
                "Failed to resolve the kubeconfig. Nothing was deleted".to_string()
            } else {
                "Failed to resolve the kubeconfig; nothing further was deleted".to_string()
            },
        })?;
    Ok(match resolved {
        KubernetesClientConfig::Manual { server_url, .. } => Some(server_url),
        _ => None,
    })
}

/// Refuse when the kubeconfig now resolves to another API server than the one recorded when it
/// was pinned: the file is re-read on every client build, so an entry of the same name rewritten
/// while the command waited would otherwise redirect what comes next.
pub(super) async fn ensure_same_server(
    config: &ClientConfig,
    pinned_server: Option<&str>,
) -> Result<()> {
    let Some(pinned) = pinned_server else {
        return Ok(());
    };
    let now = api_server_url(config, false).await?;
    if now.as_deref() == Some(pinned) {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::ValidationError {
        field: "kube_context".to_string(),
        message: format!(
            "The kubeconfig changed while the command ran (it pointed at {pinned}, now {}); nothing further was deleted.",
            now.as_deref().unwrap_or("no API server")
        ),
    }))
}

/// The context name a pinned configuration carries, for the confirmation text.
pub(super) fn context_name(config: &ClientConfig) -> Option<String> {
    match config {
        ClientConfig::Kubernetes(kubernetes) => match kubernetes.as_ref() {
            KubernetesClientConfig::Kubeconfig { context, .. } => context.clone(),
            _ => None,
        },
        _ => None,
    }
}

/// When the Operator's Secret was the only proof of the cluster, the person confirms it: another
/// cluster with a namespace and an Operator Secret of the same name looks identical to the check.
/// Shows the API server and context, then needs `--yes` or a typed `yes`. Without either (no
/// terminal, a different answer) nothing is deleted.
pub(super) fn confirm_secret_only_cluster(
    server_url: &str,
    context: Option<&str>,
    assume_yes: bool,
    interactive: bool,
    read_answer: impl FnOnce() -> std::io::Result<String>,
) -> Result<()> {
    if assume_yes {
        return Ok(());
    }
    let cluster = format!(
        "API server {server_url}, context {}",
        context.unwrap_or("(none: not a kubeconfig context)")
    );
    let refuse = |why: &str| {
        Err(AlienError::new(ErrorData::ValidationError {
            field: "kube_context".to_string(),
            message: format!(
                "None of the deployment's workloads were found on the cluster this command points at ({cluster}); only an Operator Secret in the release's namespace, which a same-named namespace on another cluster would also have. {why} Nothing was deleted."
            ),
        }))
    };
    if !interactive {
        return refuse(
            "Confirm it is the install's cluster by running the command again with --yes.",
        );
    }
    crate::output::warn(&format!(
        "None of the deployment's workloads were found on {cluster}; only an Operator Secret in the release's namespace. Continue only if this is the cluster the chart was installed on."
    ));
    eprint!("Type yes to delete on this cluster: ");
    let answer = read_answer().unwrap_or_default();
    if answer.trim() == "yes" {
        Ok(())
    } else {
        refuse("It was not confirmed.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Platform, Resource, StackResourceState};
    use httpmock::prelude::*;

    fn stack_with_workload(status: ResourceStatus, state: Value) -> StackState {
        let mut stack = StackState::new(Platform::Kubernetes);
        let mut resource = StackResourceState::new_pending(
            "container".to_string(),
            Resource::new(
                alien_core::Container::new("web".to_string())
                    .code(alien_core::ContainerCode::Image {
                        image: "app:latest".to_string(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "0.5".to_string(),
                        desired: "1".to_string(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "512Mi".to_string(),
                        desired: "1Gi".to_string(),
                    })
                    .port(8080)
                    .permissions("execution".to_string())
                    .build(),
            ),
            Some(ResourceLifecycle::Live),
            vec![],
        );
        resource.status = status;
        resource.internal_state = Some(state);
        stack.resources.insert("web".to_string(), resource);
        stack
    }

    /// Saved controller state naming `web` in `apps`, written by hand in the field names the
    /// Kubernetes controllers use (not produced by the controllers themselves).
    fn controller_state(kind: &str) -> Value {
        match kind {
            "container" => serde_json::json!({"workload_name": "web", "namespace": "apps"}),
            "stateful container" => serde_json::json!({
                "workload_name": "web", "is_stateful": true, "namespace": "apps"
            }),
            "worker" => serde_json::json!({"deployment_name": "web", "namespace": "apps"}),
            "daemon" => serde_json::json!({"daemon_set_name": "web", "namespace": "apps"}),
            "build" => serde_json::json!({"job_name": "web", "namespace": "apps"}),
            other => panic!("unknown kind {other}"),
        }
    }

    fn recorded(status: ResourceStatus) -> StackState {
        stack_with_workload(status, controller_state("container"))
    }

    fn release() -> alien_core::InstalledHelmRelease {
        alien_core::InstalledHelmRelease {
            name: "app".to_string(),
            namespace: "apps".to_string(),
        }
    }

    fn secret_status(server: &MockServer, code: u16) -> httpmock::Mock<'_> {
        server.mock(|when, then| {
            when.method(GET).path("/api/v1/namespaces/apps/secrets/app-operator");
            then.status(code).json_body(if code == 200 {
                serde_json::json!({"apiVersion": "v1", "kind": "Secret", "metadata": {"name": "app-operator", "namespace": "apps"}})
            } else {
                serde_json::json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "reason": "NotFound", "code": 404, "message": "secrets \"app-operator\" not found"})
            });
        })
    }

    fn cluster_at(server: &MockServer) -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Manual {
            server_url: server.base_url(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: None,
            client_certificate_data: None,
            client_key_data: None,
            token: Some("token".to_string()),
            username: None,
            password: None,
            namespace: None,
            additional_headers: Default::default(),
        }))
    }

    #[test]
    fn only_live_workloads_not_yet_deleted_are_named() {
        let named = recorded_workloads(&recorded(ResourceStatus::Running));
        assert_eq!(
            named,
            vec![RecordedWorkload {
                resource_id: "web".into(),
                namespace: "apps".into(),
                kind: WorkloadKind::Deployment,
                name: "web".into()
            }]
        );
        assert!(recorded_workloads(&recorded(ResourceStatus::Deleted)).is_empty());
        // A workload that never got as far as a namespace names nothing.
        let never = stack_with_workload(ResourceStatus::ProvisionFailed, serde_json::json!({}));
        assert!(recorded_workloads(&never).is_empty());
    }

    /// The saved-state field names this check reads, for each kind of Kubernetes workload.
    #[test]
    fn the_state_every_kubernetes_controller_saves_is_read() {
        for (kind, expected) in [
            ("container", WorkloadKind::Deployment),
            ("stateful container", WorkloadKind::StatefulSet),
            ("worker", WorkloadKind::Deployment),
            ("daemon", WorkloadKind::DaemonSet),
            ("build", WorkloadKind::Job),
        ] {
            let stack = stack_with_workload(ResourceStatus::Running, controller_state(kind));
            let named = recorded_workloads(&stack);
            assert_eq!(
                named.len(),
                1,
                "{kind}: {:?}",
                stack.resources["web"].internal_state
            );
            assert_eq!(
                (
                    named[0].kind,
                    named[0].namespace.as_str(),
                    named[0].name.as_str()
                ),
                (expected, "apps", "web"),
                "{kind}"
            );
        }
    }

    /// The wrong cluster has none of the recorded workloads: refused, with nothing deleted.
    #[tokio::test]
    async fn the_wrong_cluster_is_refused() {
        let server = MockServer::start_async().await;
        let lookup = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/apps/v1/namespaces/apps/deployments/web");
                then.status(404).json_body(serde_json::json!({
                    "kind": "Status", "apiVersion": "v1", "status": "Failure",
                    "reason": "NotFound", "code": 404, "message": "deployments.apps \"web\" not found"
                }));
            })
            .await;
        let namespace = secret_status(&server, 404);
        let error = verify_install_cluster(
            &cluster_at(&server),
            Some(&recorded(ResourceStatus::Running)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("another cluster must be refused");
        let message = serde_json::to_string(&error).unwrap();
        assert!(message.contains("Nothing was deleted"), "{message}");
        assert!(message.contains("--kube-context"), "{message}");
        lookup.assert_hits_async(1).await;
        namespace.assert_hits_async(1).await;
    }

    /// The accepted limit: the check proves the cluster by the Operator's Secret under the
    /// recorded release name and namespace. Another cluster that holds a Secret of the same name
    /// in the same namespace (a second install of the same release name) passes it, and the
    /// check cannot tell the two apart. This test records that result; it is not a refusal.
    #[tokio::test]
    async fn another_cluster_with_its_own_operator_secret_of_the_same_name_passes() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/apps/v1/namespaces/apps/deployments/web");
                then.status(404).json_body(serde_json::json!({
                    "kind": "Status", "apiVersion": "v1", "status": "Failure",
                    "reason": "NotFound", "code": 404, "message": "deployments.apps \"web\" not found"
                }));
            })
            .await;
        secret_status(&server, 200);
        let proof = verify_install_cluster(
            &cluster_at(&server),
            Some(&recorded(ResourceStatus::Running)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("accepted limit: a same-named Operator Secret passes the check");
        // ... but only as a proof the person confirms.
        assert_eq!(
            proof,
            ClusterProof::OperatorSecretOnly {
                server_url: server.base_url()
            }
        );
    }

    /// After `helm uninstall` the chart's hook has removed the workloads; the Operator's own
    /// Secret, which Helm leaves, is what shows this is the right cluster.
    #[tokio::test]
    async fn workloads_already_removed_in_the_installs_namespace_are_accepted() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/apps/v1/namespaces/apps/deployments/web");
                then.status(404).json_body(serde_json::json!({
                    "kind": "Status", "apiVersion": "v1", "status": "Failure",
                    "reason": "NotFound", "code": 404, "message": "not found"
                }));
            })
            .await;
        secret_status(&server, 200);
        verify_install_cluster(
            &cluster_at(&server),
            Some(&recorded(ResourceStatus::Running)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("the Operator-owned Secret proves the cluster");
    }

    #[tokio::test]
    async fn the_installs_cluster_is_accepted() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/apps/v1/namespaces/apps/deployments/web");
                then.status(200).json_body(serde_json::json!({
                    "apiVersion": "apps/v1", "kind": "Deployment",
                    "metadata": {"name": "web", "namespace": "apps"}
                }));
            })
            .await;
        let proof = verify_install_cluster(
            &cluster_at(&server),
            Some(&recorded(ResourceStatus::Running)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("the workload exists here");
        assert_eq!(proof, ClusterProof::RecordedWorkload);
    }

    /// A record that names no workload (only a vault or service account, or a state key that
    /// stops matching) still needs the Operator's own Secret: an empty list must not pass on
    /// any cluster.
    #[tokio::test]
    async fn a_record_naming_no_workload_still_needs_the_operators_secret() {
        let server = MockServer::start_async().await;
        let missing = secret_status(&server, 404);
        let error = verify_install_cluster(
            &cluster_at(&server),
            Some(&StackState::new(Platform::Kubernetes)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("no workload and no Secret is not the install's cluster");
        let message = serde_json::to_string(&error).unwrap();
        assert!(message.contains("Nothing was deleted"), "{message}");
        missing.assert_hits_async(1).await;

        let server = MockServer::start_async().await;
        secret_status(&server, 200);
        verify_install_cluster(
            &cluster_at(&server),
            Some(&StackState::new(Platform::Kubernetes)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("the Operator's Secret proves the cluster");
    }

    /// Nothing to look for and nothing to look in: refused, not waved through.
    #[tokio::test]
    async fn no_workload_and_no_release_is_refused() {
        let server = MockServer::start_async().await;
        verify_install_cluster(
            &cluster_at(&server),
            None,
            None,
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("nothing proves the cluster");
    }

    /// A chart installed with `fullnameOverride` names the Operator's Secret after that name,
    /// not the release: the label the Operator puts on its Secret still finds it. The Secret is
    /// the only proof there, so the person confirms it.
    #[tokio::test]
    async fn an_operator_secret_found_by_label_is_a_proof_to_confirm() {
        let server = MockServer::start_async().await;
        secret_status(&server, 404);
        let listed = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/namespaces/apps/secrets")
                .query_param(
                    "labelSelector",
                    "app.kubernetes.io/managed-by=alien-operator",
                );
            then.status(200).json_body(serde_json::json!({
                "apiVersion": "v1", "kind": "SecretList", "metadata": {},
                "items": [{"metadata": {"name": "shop-operator", "namespace": "apps"}}]
            }));
        });
        let proof = verify_install_cluster(
            &cluster_at(&server),
            Some(&StackState::new(Platform::Kubernetes)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("the labelled Secret proves the cluster");
        assert!(matches!(proof, ClusterProof::OperatorSecretOnly { .. }));
        listed.assert_hits_async(1).await;
    }

    /// The wrong cluster answers for real: no workload (404) and an empty, successful list of
    /// Operator Secrets (200 with no items), not a mock's default 404 everywhere. Refused.
    #[tokio::test]
    async fn a_200_with_no_operator_secrets_is_refused() {
        let server = MockServer::start_async().await;
        secret_status(&server, 404);
        let listed = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/namespaces/apps/secrets")
                .query_param(
                    "labelSelector",
                    "app.kubernetes.io/managed-by=alien-operator",
                );
            then.status(200).json_body(serde_json::json!({
                "apiVersion": "v1", "kind": "SecretList", "metadata": {}, "items": []
            }));
        });
        let error = verify_install_cluster(
            &cluster_at(&server),
            Some(&StackState::new(Platform::Kubernetes)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("an empty list is not the install's cluster");
        assert!(serde_json::to_string(&error)
            .unwrap()
            .contains("Nothing was deleted"));
        listed.assert_hits_async(1).await;
    }

    fn answer(text: &'static str) -> impl FnOnce() -> std::io::Result<String> {
        move || Ok(format!("{text}\n"))
    }

    /// A proof by the Operator's Secret alone needs the person: a typed `yes`...
    #[test]
    fn the_secret_only_proof_needs_a_typed_yes() {
        confirm_secret_only_cluster("https://k.test", Some("prod"), false, true, answer("yes"))
            .expect("typed yes");
        for wrong in ["", "no", "y", "Yes please"] {
            let error = confirm_secret_only_cluster(
                "https://k.test",
                Some("prod"),
                false,
                true,
                answer(wrong),
            )
            .expect_err(wrong);
            let message = serde_json::to_string(&error).unwrap();
            assert!(message.contains("Nothing was deleted"), "{message}");
            assert!(
                message.contains("https://k.test") && message.contains("prod"),
                "{message}"
            );
        }
    }

    /// ... or `--yes`, which never reads an answer.
    #[test]
    fn yes_passes_without_asking() {
        for interactive in [true, false] {
            confirm_secret_only_cluster("https://k.test", None, true, interactive, || {
                panic!("must not ask")
            })
            .expect("--yes");
        }
    }

    /// Without a terminal and without `--yes` it refuses, names the flag, and never reads.
    #[test]
    fn without_a_terminal_and_without_yes_it_refuses() {
        let error =
            confirm_secret_only_cluster("https://k.test", Some("prod"), false, false, || {
                panic!("must not read")
            })
            .expect_err("refused");
        let message = serde_json::to_string(&error).unwrap();
        assert!(
            message.contains("--yes") && message.contains("Nothing was deleted"),
            "{message}"
        );
    }

    const KUBECONFIG: &str = r#"
apiVersion: v1
kind: Config
current-context: {current}
clusters:
- cluster: {server: "{a}"}
  name: ca
- cluster: {server: "{b}"}
  name: cb
contexts:
- context: {cluster: ca, user: u}
  name: ctx-a
- context: {cluster: cb, user: u}
  name: ctx-b
users:
- name: u
  user: {token: t}
"#;

    fn write_kubeconfig(path: &std::path::Path, current: &str, a: &str, b: &str) {
        std::fs::write(
            path,
            KUBECONFIG
                .replace("{current}", current)
                .replace("{a}", a)
                .replace("{b}", b),
        )
        .unwrap();
    }

    fn kubeconfig_config(path: &std::path::Path, context: Option<&str>) -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Kubeconfig {
            kubeconfig_path: Some(path.display().to_string()),
            context: context.map(ToOwned::to_owned),
            cluster: None,
            user: None,
            namespace: None,
            additional_headers: None,
        }))
    }

    /// `kubectl config use-context` while the command waits must not move later requests to a
    /// cluster that was never checked: after the pin, switching the file's current context
    /// changes nothing, and the check, the cleanup and the listing still reach the first cluster.
    #[tokio::test]
    async fn a_context_switched_after_the_pin_does_not_move_later_requests() {
        let first = MockServer::start_async().await;
        let second = MockServer::start_async().await;
        let on_first = first
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/apis/apps/v1/namespaces/apps/deployments/web");
                then.status(200).json_body(serde_json::json!({
                    "apiVersion": "apps/v1", "kind": "Deployment",
                    "metadata": {"name": "web", "namespace": "apps"}
                }));
            })
            .await;
        let on_second = second
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).json_body(serde_json::json!({}));
            })
            .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        write_kubeconfig(&path, "ctx-a", &first.base_url(), &second.base_url());

        // No flag, no context: the ambient current-context is the one pinned.
        let pinned = pin_kubeconfig(kubeconfig_config(&path, None)).unwrap();
        assert_eq!(context_name(&pinned).as_deref(), Some("ctx-a"));

        // The person (or another terminal) switches context while the command waits.
        write_kubeconfig(&path, "ctx-b", &first.base_url(), &second.base_url());

        let provider = alien_infra::DefaultPlatformServiceProvider::default();
        let proof = verify_install_cluster(
            &pinned,
            Some(&recorded(ResourceStatus::Running)),
            Some(&release()),
            &provider,
        )
        .await
        .expect("the first cluster");
        assert_eq!(proof, ClusterProof::RecordedWorkload);
        // The listing the command does after the cleanup goes through the same client config.
        let _ = leftover_runtime_objects(&pinned, "apps", "managed-by=runtime", &provider).await;
        assert!(on_first.hits_async().await >= 1);
        assert_eq!(
            on_second.hits_async().await,
            0,
            "the switched-to cluster was never called"
        );

        // Without the pin the switch does move the request (what the pin prevents).
        let unpinned = kubeconfig_config(&path, None);
        let resolved = alien_infra::resolve_kubeconfig(match &unpinned {
            ClientConfig::Kubernetes(kubernetes) => kubernetes.as_ref(),
            _ => unreachable!(),
        })
        .await
        .unwrap();
        assert!(
            matches!(&resolved, KubernetesClientConfig::Manual { server_url, .. } if *server_url == second.base_url())
        );
    }

    /// A context named by the flag is kept as given; the path is made explicit.
    #[test]
    fn a_named_context_is_kept_and_the_path_is_made_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        write_kubeconfig(&path, "ctx-a", "https://a.test", "https://b.test");
        let pinned = pin_kubeconfig(kubeconfig_config(&path, Some("ctx-b"))).unwrap();
        assert_eq!(context_name(&pinned).as_deref(), Some("ctx-b"));
        let ClientConfig::Kubernetes(kubernetes) = pinned else {
            unreachable!()
        };
        let KubernetesClientConfig::Kubeconfig {
            kubeconfig_path,
            cluster,
            user,
            ..
        } = *kubernetes
        else {
            unreachable!()
        };
        assert_eq!(kubeconfig_path.as_deref(), Some(path.to_str().unwrap()));
        assert_eq!(
            (cluster.as_deref(), user.as_deref()),
            (Some("cb"), Some("u"))
        );
    }

    /// Credentials this check cannot read are not waved through.
    #[tokio::test]
    async fn a_client_config_that_is_not_kubernetes_is_refused() {
        let error = verify_install_cluster(
            &ClientConfig::Local {
                state_directory: "/tmp/none".to_string(),
            },
            None,
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("refused");
        assert!(serde_json::to_string(&error)
            .unwrap()
            .contains("Nothing was deleted"));
    }

    /// The refusal names the next step truthfully: the record-only removal when the install's
    /// own objects are gone from the right cluster.
    #[tokio::test]
    async fn the_refusal_names_the_record_only_removal() {
        let server = MockServer::start_async().await;
        secret_status(&server, 404);
        let error = verify_install_cluster(
            &cluster_at(&server),
            Some(&StackState::new(Platform::Kubernetes)),
            Some(&release()),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect_err("refused");
        let message = serde_json::to_string(&error).unwrap();
        assert!(message.contains("--force-delete-record"), "{message}");
    }

    fn list_at(server: &MockServer, path: &str, kind: &str, names: &[&str], selector: &str) {
        let items: Vec<Value> = names
            .iter()
            .map(|name| serde_json::json!({"metadata": {"name": name, "namespace": "apps"}}))
            .collect();
        let selector = selector.to_string();
        server.mock(|when, then| {
            when.method(GET)
                .path(path)
                .query_param("labelSelector", selector);
            then.status(200).json_body(serde_json::json!({
                "apiVersion": if path.contains("/apps/v1/") { "apps/v1" } else if path.contains("/batch/") { "batch/v1" } else { "v1" },
                "kind": kind, "metadata": {}, "items": items
            }));
        });
    }

    fn list_all(server: &MockServer, selector: &str) {
        list_at(
            server,
            "/apis/apps/v1/namespaces/apps/deployments",
            "DeploymentList",
            &[],
            selector,
        );
        list_at(
            server,
            "/apis/apps/v1/namespaces/apps/statefulsets",
            "StatefulSetList",
            &[],
            selector,
        );
        list_at(
            server,
            "/apis/apps/v1/namespaces/apps/daemonsets",
            "DaemonSetList",
            &["agent"],
            selector,
        );
        list_at(
            server,
            "/apis/batch/v1/namespaces/apps/jobs",
            "JobList",
            &[],
            selector,
        );
        list_at(
            server,
            "/api/v1/namespaces/apps/services",
            "ServiceList",
            &[],
            selector,
        );
        list_at(
            server,
            "/api/v1/namespaces/apps/secrets",
            "SecretList",
            &["web-registry"],
            selector,
        );
    }

    /// With a recorded resource prefix the listing asks for this deployment's objects only, by the
    /// scope label the controllers write; the warning offers the kubectl removal. Mocks answer only
    /// for that exact selector, so a `managed-by=runtime`-only request would fail the test. The
    /// literal below is the pair as it sits on a labelled object (key `<domain>/deployment` where a
    /// branded domain is slugged: `example.com` gives `example`; value the resource prefix), so a drift in either the helper or this selector fails here.
    #[tokio::test]
    async fn what_the_runtime_left_is_listed_for_this_deployment_only() {
        // Labels as the controllers write them on a workload (fixture).
        let object_labels: std::collections::BTreeMap<&str, &str> =
            [("managed-by", "runtime"), ("example/deployment", "stk123")]
                .into_iter()
                .collect();
        let literal = object_labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>();
        let selector = runtime_selector(Some("example.com"), Some("stk123"));
        for pair in &literal {
            assert!(
                selector.split(',').any(|part| part == pair),
                "{selector} lacks {pair}"
            );
        }
        // The same helper the controllers use.
        let (key, value) =
            alien_infra::kubernetes_deployment_scope_label_for(Some("example.com"), "stk123");
        assert_eq!(selector, format!("managed-by=runtime,{key}={value}"));
        let server = MockServer::start_async().await;
        list_all(&server, "managed-by=runtime,example/deployment=stk123");
        let left = leftover_runtime_objects(
            &cluster_at(&server),
            "apps",
            &selector,
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("listed");
        assert_eq!(left, ["DaemonSet agent", "Secret web-registry"]);
        let warning = leftover_warning("apps", &left, true).expect("a warning");
        assert!(
            warning.contains("DaemonSet agent, Secret web-registry")
                && warning.contains("remove them with kubectl")
                && warning.contains("This list is not exhaustive")
                && warning.contains("legacy label key")
                && warning.contains("unlabelled objects"),
            "{warning}"
        );
        assert_eq!(leftover_warning("apps", &[], true), None);
        // No label domain: the default one, as for the controllers.
        assert_eq!(
            runtime_selector(None, Some("stk123")),
            "managed-by=runtime,alien.dev/deployment=stk123"
        );
    }

    /// Without a recorded resource prefix the list is every runtime-managed object in the namespace: the warning
    /// says so and offers no delete command.
    #[tokio::test]
    async fn without_a_resource_prefix_the_list_says_it_may_include_another_install() {
        assert_eq!(
            runtime_selector(Some("example.com"), None),
            "managed-by=runtime"
        );
        assert_eq!(
            runtime_selector(Some("example.com"), Some("")),
            "managed-by=runtime"
        );
        let server = MockServer::start_async().await;
        list_all(&server, "managed-by=runtime");
        let left = leftover_runtime_objects(
            &cluster_at(&server),
            "apps",
            &runtime_selector(None, None),
            &alien_infra::DefaultPlatformServiceProvider::default(),
        )
        .await
        .expect("listed");
        let warning = leftover_warning("apps", &left, false).expect("a warning");
        assert!(
            warning.contains("may include another install's")
                && warning.contains("DaemonSet agent, Secret web-registry")
                && !warning.contains("kubectl"),
            "{warning}"
        );
    }
}
