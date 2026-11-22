//! The Kubernetes client configuration a command-line delete uses.

use alien_core::{ClientConfig, DeploymentState, KubernetesClientConfig};

/// The client configuration for deleting a Kubernetes deployment from a command line: the
/// namespace is the one the deployment's record holds for its Helm release, never an ambient
/// value (the shell's `KUBERNETES_NAMESPACE`, the kubeconfig context's). Objects the Operator
/// wrote live in the release's namespace; a delete that looks in another one finds nothing and
/// reads as done while they run on. With no release recorded (an install older than the
/// record) the configuration is returned unchanged: the namespace the command already used
/// before this record existed. Other platforms' configurations are returned unchanged.
pub fn client_config_for_cli_delete(config: ClientConfig, state: &DeploymentState) -> ClientConfig {
    let ClientConfig::Kubernetes(mut kubernetes) = config else {
        return config;
    };
    let namespace = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.installed_helm_release.as_ref())
        .map(|release| release.namespace.clone())
        .filter(|namespace| !namespace.is_empty());
    let Some(namespace) = namespace else {
        return ClientConfig::Kubernetes(kubernetes);
    };
    let namespace = Some(namespace);
    match kubernetes.as_mut() {
        KubernetesClientConfig::InCluster { namespace: n, .. }
        | KubernetesClientConfig::Kubeconfig { namespace: n, .. }
        | KubernetesClientConfig::Manual { namespace: n, .. } => *n = namespace,
    }
    ClientConfig::Kubernetes(kubernetes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{InstalledHelmRelease, Platform, RuntimeMetadata};

    fn config(namespace: Option<&str>) -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Manual {
            server_url: "https://kubernetes.test".to_string(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: Some(true),
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: namespace.map(ToOwned::to_owned),
            additional_headers: Default::default(),
        }))
    }

    fn namespace_of(config: &ClientConfig) -> Option<String> {
        match config {
            ClientConfig::Kubernetes(kubernetes) => match kubernetes.as_ref() {
                KubernetesClientConfig::Manual { namespace, .. }
                | KubernetesClientConfig::InCluster { namespace, .. }
                | KubernetesClientConfig::Kubeconfig { namespace, .. } => namespace.clone(),
            },
            _ => None,
        }
    }

    fn state(release: Option<&str>) -> DeploymentState {
        let mut state = DeploymentState {
            status: alien_core::DeploymentStatus::DeletePending,
            platform: Platform::Kubernetes,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: 1,
        };
        let mut metadata = RuntimeMetadata::default();
        metadata.installed_helm_release = release.map(|namespace| InstalledHelmRelease {
            name: "app".to_string(),
            namespace: namespace.to_string(),
        });
        state.runtime_metadata = Some(metadata);
        state
    }

    /// Whatever the shell says, the delete acts in the recorded release's namespace.
    #[test]
    fn the_recorded_release_namespace_replaces_the_ambient_one() {
        for ambient in [None, Some("default"), Some("apps")] {
            let configured = client_config_for_cli_delete(config(ambient), &state(Some("apps")));
            assert_eq!(
                namespace_of(&configured).as_deref(),
                Some("apps"),
                "{ambient:?}"
            );
        }
    }

    /// No release in the record (an older install): the namespace the command had before the
    /// record existed is kept, ambient or not, so the Container network-policy delete still
    /// has one.
    #[test]
    fn without_a_recorded_release_the_previous_namespace_is_kept() {
        for ambient in [None, Some("default"), Some("apps")] {
            let configured = client_config_for_cli_delete(config(ambient), &state(None));
            assert_eq!(namespace_of(&configured).as_deref(), ambient);
        }
    }
}
