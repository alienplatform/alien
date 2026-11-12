//! Keeps the Operator on the image its manager targets.
//!
//! The manager advertises the Operator image its charts install. An Operator
//! installed with `OPERATOR_SELF_UPDATE_DEPLOYMENT` updates its own
//! Deployment to that image. The Deployment uses the Recreate strategy (the
//! Operator holds an exclusive lock on its state directory), so the old pod
//! stops before the new one starts, and an image that can't start leaves no
//! Operator running until it is fixed. The chart is installed once and the
//! Operator still moves forward with the manager.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context};
use alien_k8s_clients::{
    ErrorData as KubernetesErrorData, KubernetesClient, KubernetesClientConfig,
    KubernetesClientConfigExt,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use k8s_openapi::{api::core::v1::Secret, ByteString};
use tracing::{info, warn};

use crate::{
    error::{ErrorData, Result},
    OperatorState,
};

/// Container in the Operator Deployment that runs the Operator.
const OPERATOR_CONTAINER: &str = "operator";

/// Move the Operator's own Deployment to `target_image` if it runs another.
pub async fn apply(state: &OperatorState, target_image: &str) -> Result<()> {
    let Some(deployment_name) = state.config.self_update_deployment.as_deref() else {
        return Ok(());
    };
    let namespace = state.config.namespace.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "self-update needs the Operator's namespace".to_string(),
        })
    })?;
    let failed = |message: &str| ErrorData::ConfigurationError {
        message: format!("self-update of Deployment '{deployment_name}': {message}"),
    };

    let client = KubernetesClient::new(
        KubernetesClientConfig::try_incluster()
            .await
            .context(failed("loading in-cluster credentials"))?,
    )
    .await
    .context(failed("creating Kubernetes client"))?;
    let mut deployment = client
        .get_deployment(namespace, deployment_name)
        .await
        .context(failed("reading the Deployment"))?;

    let container = deployment
        .spec
        .as_mut()
        .and_then(|spec| spec.template.spec.as_mut())
        .and_then(|pod| {
            pod.containers
                .iter_mut()
                .find(|container| container.name == OPERATOR_CONTAINER)
        })
        .ok_or_else(|| AlienError::new(failed("no 'operator' container")))?;
    if container.image.as_deref() == Some(target_image) {
        return Ok(());
    }
    // Protected, the Operator moves only to the image digest its newest
    // accepted release signed: the manager chooses when and from where, not what.
    if state.config.is_protected() {
        let signed = state
            .db
            .get_signing_trust()
            .await?
            .and_then(|stored| stored.operator_digest);
        let target_digest = alien_core::release_statement::image_digest(target_image);
        if signed.is_none() || signed.as_deref() != target_digest {
            warn!(
                image = target_image,
                signed = signed.as_deref().unwrap_or("none"),
                "Protected mode: refused to update the Operator to an image no accepted release signed"
            );
            return Ok(());
        }
    }
    let current = container.image.clone().unwrap_or_default();
    container.image = Some(target_image.to_string());

    info!(from = %current, to = %target_image, "Updating Operator image");
    client
        .update_deployment(namespace, deployment_name, &deployment)
        .await
        .context(failed("updating the Deployment"))?;
    Ok(())
}

/// Keep the pull Secret of the Operator's own Deployment filled with the
/// Operator's registry pull credential. The chart creates the Secret with the
/// one-hour enrollment code, or with the install token of an older chart. The
/// credential written here lasts as long as the daily key, so the Operator pod
/// still pulls its image on a new node after the code or token is gone, or
/// while the Operator is down. It runs on every sync, so a `helm upgrade` that
/// puts the old value back is overwritten again.
///
/// The Secret is named by `OPERATOR_REGISTRY_SECRET`. Older charts do not set
/// it; then the Secrets the Operator Deployment references are searched for an
/// entry for the manager's host, and only those are rewritten.
///
/// The manager keeps the install token until the Operator reports that this
/// worked, so the outcome is kept for the next sync request.
pub async fn refresh_pull_secret(state: &OperatorState) -> Result<()> {
    let result = refresh_pull_secrets(state).await;
    state
        .db
        .set_pull_secret_refreshed(matches!(result, Ok(true)));
    result.map(|_| ())
}

/// `true` when every pull Secret of the Operator holds its credential, or none
/// is known to need it.
async fn refresh_pull_secrets(state: &OperatorState) -> Result<bool> {
    if state.db.read_enrolled_daily_identity().await?.is_none() {
        return Ok(false);
    }
    let configured = state.config.registry_secret.as_deref();
    let deployment = state.config.self_update_deployment.as_deref();
    if configured.is_none() && deployment.is_none() {
        return Ok(true);
    }
    let failed = |message: &str| ErrorData::ConfigurationError {
        message: format!("pull Secret: {message}"),
    };
    let namespace = state
        .config
        .namespace
        .as_deref()
        .ok_or_else(|| AlienError::new(failed("no namespace")))?;
    let host = state
        .config
        .sync
        .as_ref()
        .and_then(|sync| {
            let host = sync.url.host_str()?;
            Some(match sync.url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            })
        })
        .ok_or_else(|| AlienError::new(failed("no manager host")))?;
    let token =
        crate::daily_identity::request_token(state, serde_json::json!({"type":"registryPull"}))
            .await?;
    let client = KubernetesClient::new(
        KubernetesClientConfig::try_incluster()
            .await
            .context(failed("loading in-cluster credentials"))?,
    )
    .await
    .context(failed("creating Kubernetes client"))?;
    refresh_secrets(&client, namespace, configured, deployment, &host, &token).await
}

/// Rewrite the pull Secrets of the Operator for `host` to hold `token`.
async fn refresh_secrets(
    client: &KubernetesClient,
    namespace: &str,
    configured: Option<&str>,
    deployment: Option<&str>,
    host: &str,
    token: &str,
) -> Result<bool> {
    // A configured Secret is the Operator's own: it is rewritten whatever it
    // holds. Referenced Secrets may belong to the user; only one with an entry
    // for the manager's host is touched.
    let (names, own) = match (configured, deployment) {
        (Some(name), _) => (vec![name.to_string()], true),
        (None, Some(deployment)) => {
            let failed = |message: &str| ErrorData::ConfigurationError {
                message: format!("pull Secret of Deployment '{deployment}': {message}"),
            };
            let current = client
                .get_deployment(namespace, deployment)
                .await
                .context(failed("reading the Deployment"))?;
            let names = current
                .spec
                .and_then(|spec| spec.template.spec)
                .and_then(|pod| pod.image_pull_secrets)
                .unwrap_or_default()
                .into_iter()
                .map(|reference| reference.name)
                .collect();
            (names, false)
        }
        (None, None) => return Ok(true),
    };
    for name in names {
        let failed = |message: &str| ErrorData::ConfigurationError {
            message: format!("pull Secret '{name}': {message}"),
        };
        let secret = match client.get_secret(namespace, &name).await {
            Ok(secret) => secret,
            Err(error)
                if !own
                    && matches!(
                        error.error.as_ref(),
                        Some(KubernetesErrorData::RemoteResourceNotFound { .. })
                    ) =>
            {
                continue
            }
            Err(error) => return Err(error).context(failed("reading the Secret")),
        };
        if !own && !has_registry_entry(&secret, host) {
            continue;
        }
        // The chart owns the Secret. Only its data changes here.
        let Some(secret) = with_pull_credential(secret, host, token)? else {
            continue;
        };
        client
            .update_secret(namespace, &name, &secret)
            .await
            .context(failed("updating the Secret"))?;
        info!(secret = %name, "Refreshed the Operator pull Secret");
    }
    Ok(true)
}

/// Whether the Secret's Docker config has an entry for `host`.
fn has_registry_entry(secret: &Secret, host: &str) -> bool {
    secret
        .data
        .as_ref()
        .and_then(|data| data.get(".dockerconfigjson"))
        .and_then(|stored| serde_json::from_slice::<serde_json::Value>(&stored.0).ok())
        .is_some_and(|config| config["auths"].get(host).is_some())
}

/// `secret` with a Docker config for `host` and `token`, or `None` when it
/// already holds exactly that. Entries for other hosts stay.
fn with_pull_credential(mut secret: Secret, host: &str, token: &str) -> Result<Option<Secret>> {
    let data = secret.data.get_or_insert_with(BTreeMap::new);
    let mut config = data
        .get(".dockerconfigjson")
        .and_then(|stored| serde_json::from_slice::<serde_json::Value>(&stored.0).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let before = config.clone();
    if !config["auths"].is_object() {
        config["auths"] = serde_json::json!({});
    }
    config["auths"][host] = serde_json::json!({
        "username": "deployment",
        "password": token,
        "auth": STANDARD.encode(format!("deployment:{token}")),
    });
    if config == before && data.contains_key(".dockerconfigjson") {
        return Ok(None);
    }
    let config = serde_json::to_vec(&config).map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "cannot encode the pull credential".to_string(),
        })
    })?;
    data.insert(".dockerconfigjson".to_string(), ByteString(config));
    Ok(Some(secret))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use axum::{
        extract::{Path, State},
        http::StatusCode,
        routing::get,
        Json, Router,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use serde_json::{json, Value};

    use super::*;

    const HOST: &str = "manager.example.test:8443";

    fn docker_secret(name: &str, config: Value) -> Secret {
        Secret {
            metadata: ObjectMeta {
                name: Some(name.into()),
                resource_version: Some("7".into()),
                ..Default::default()
            },
            type_: Some("kubernetes.io/dockerconfigjson".into()),
            data: Some(BTreeMap::from([(
                ".dockerconfigjson".to_string(),
                ByteString(serde_json::to_vec(&config).unwrap()),
            )])),
            ..Default::default()
        }
    }

    /// What an older chart writes: the install token as the host's `auth`.
    fn chart_config(host: &str, user: &str, password: &str) -> Value {
        json!({"auths": {host: {"auth": STANDARD.encode(format!("{user}:{password}"))}}})
    }

    fn password_for(secret: &Secret, host: &str) -> String {
        let config: Value =
            serde_json::from_slice(&secret.data.as_ref().unwrap()[".dockerconfigjson"].0).unwrap();
        let auth = STANDARD
            .decode(config["auths"][host]["auth"].as_str().unwrap())
            .unwrap();
        String::from_utf8(auth).unwrap()
    }

    /// A Kubernetes API with one Deployment and a set of Secrets. It counts
    /// the Secret writes.
    #[derive(Clone, Default)]
    struct FakeCluster {
        secrets: Arc<Mutex<HashMap<String, Value>>>,
        writes: Arc<Mutex<Vec<String>>>,
        pull_secrets: Arc<Mutex<Vec<String>>>,
    }

    impl FakeCluster {
        fn put(&self, secret: Secret) {
            let name = secret.metadata.name.clone().unwrap();
            self.secrets
                .lock()
                .unwrap()
                .insert(name, serde_json::to_value(secret).unwrap());
        }

        fn secret(&self, name: &str) -> Secret {
            serde_json::from_value(self.secrets.lock().unwrap()[name].clone()).unwrap()
        }

        fn writes(&self) -> Vec<String> {
            self.writes.lock().unwrap().clone()
        }

        async fn client(&self, pull_secrets: &[&str]) -> KubernetesClient {
            *self.pull_secrets.lock().unwrap() =
                pull_secrets.iter().map(|n| n.to_string()).collect();
            async fn deployment(State(cluster): State<FakeCluster>) -> Json<Value> {
                let references: Vec<Value> = cluster
                    .pull_secrets
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|name| json!({"name": name}))
                    .collect();
                Json(json!({
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "metadata": {"name": "prod"},
                    "spec": {
                        "selector": {},
                        "template": {"spec": {"containers": [], "imagePullSecrets": references}},
                    },
                }))
            }
            async fn read(
                State(cluster): State<FakeCluster>,
                Path((_, name)): Path<(String, String)>,
            ) -> (StatusCode, Json<Value>) {
                match cluster.secrets.lock().unwrap().get(&name) {
                    Some(secret) => (StatusCode::OK, Json(secret.clone())),
                    None => (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "kind": "Status", "apiVersion": "v1", "status": "Failure",
                            "reason": "NotFound", "code": 404,
                            "message": format!("secrets \"{name}\" not found"),
                        })),
                    ),
                }
            }
            async fn write(
                State(cluster): State<FakeCluster>,
                Path((_, name)): Path<(String, String)>,
                Json(secret): Json<Value>,
            ) -> Json<Value> {
                cluster.writes.lock().unwrap().push(name.clone());
                cluster.secrets.lock().unwrap().insert(name, secret.clone());
                Json(secret)
            }
            let app = Router::new()
                .route(
                    "/apis/apps/v1/namespaces/{ns}/deployments/{name}",
                    get(deployment),
                )
                .route(
                    "/api/v1/namespaces/{ns}/secrets/{name}",
                    get(read).put(write),
                )
                .with_state(self.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let server_url = format!("http://{}", listener.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(listener, app).await });
            KubernetesClient::new(KubernetesClientConfig::Manual {
                server_url,
                certificate_authority_data: None,
                insecure_skip_tls_verify: None,
                client_certificate_data: None,
                client_key_data: None,
                token: Some("test".into()),
                username: None,
                password: None,
                namespace: None,
                additional_headers: HashMap::new(),
            })
            .await
            .unwrap()
        }
    }

    #[test]
    fn pull_credential_replaces_the_code_and_keeps_the_chart_metadata() {
        let annotations = BTreeMap::from([(
            "alien.dev/enrollment-code-sha256".to_string(),
            "hash".to_string(),
        )]);
        let mut chart_secret = docker_secret("prod-registry", json!({"auths":{}}));
        chart_secret.metadata.annotations = Some(annotations.clone());
        let updated = with_pull_credential(chart_secret, "manager.example.test", "ax_cap_v1_pull")
            .unwrap()
            .expect("the code is replaced");
        assert_eq!(updated.metadata.annotations, Some(annotations));
        assert_eq!(updated.metadata.resource_version.as_deref(), Some("7"));
        assert_eq!(
            password_for(&updated, "manager.example.test"),
            "deployment:ax_cap_v1_pull"
        );
        // The same credential again writes nothing.
        assert!(
            with_pull_credential(updated, "manager.example.test", "ax_cap_v1_pull")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn pull_credential_keeps_entries_for_other_hosts() {
        let secret = docker_secret(
            "prod-registry",
            json!({"auths": {
                HOST: {"auth": STANDARD.encode("deployment:ax_deploy_dead")},
                "other.example.test": {"auth": STANDARD.encode("user:secret")},
            }}),
        );
        let updated = with_pull_credential(secret, HOST, "ax_cap_v1_pull")
            .unwrap()
            .expect("the dead token is replaced");
        assert_eq!(password_for(&updated, HOST), "deployment:ax_cap_v1_pull");
        assert_eq!(password_for(&updated, "other.example.test"), "user:secret");
    }

    /// An older chart's Secret holds the install token, and the Operator was
    /// not told the Secret's name. The Secret the Operator Deployment
    /// references for the manager's host gets the Operator's credential. A
    /// user's Secret for another registry, and one that does not exist, stay
    /// as they are.
    #[tokio::test]
    async fn the_secret_with_the_manager_entry_is_found_through_the_deployment() {
        let cluster = FakeCluster::default();
        cluster.put(docker_secret(
            "prod-registry",
            chart_config(HOST, "deployment", "ax_deploy_dead"),
        ));
        let users = docker_secret(
            "users-registry",
            chart_config("registry.example.test", "me", "mine"),
        );
        cluster.put(users.clone());
        let client = cluster
            .client(&["users-registry", "missing", "prod-registry"])
            .await;

        let refreshed = refresh_secrets(&client, "ns", None, Some("prod"), HOST, "ax_cap_v1_pull")
            .await
            .unwrap();

        assert!(refreshed);
        assert_eq!(cluster.writes(), vec!["prod-registry"]);
        assert_eq!(
            password_for(&cluster.secret("prod-registry"), HOST),
            "deployment:ax_cap_v1_pull"
        );
        assert_eq!(cluster.secret("users-registry"), users);
    }

    /// `helm upgrade` with the old values writes the dead token back. The
    /// refresh that runs on every sync replaces it again, and writes nothing
    /// when the Secret is already right.
    #[tokio::test]
    async fn a_dead_token_put_back_by_helm_is_overwritten_on_the_next_refresh() {
        let cluster = FakeCluster::default();
        let chart = docker_secret(
            "prod-registry",
            chart_config(HOST, "deployment", "ax_deploy_dead"),
        );
        cluster.put(chart.clone());
        let client = cluster.client(&["prod-registry"]).await;
        let refresh = || refresh_secrets(&client, "ns", None, Some("prod"), HOST, "ax_cap_v1_pull");

        assert!(refresh().await.unwrap());
        assert!(refresh().await.unwrap());
        assert_eq!(
            cluster.writes().len(),
            1,
            "a Secret that is right is not written"
        );

        cluster.put(chart);
        assert_eq!(
            password_for(&cluster.secret("prod-registry"), HOST),
            "deployment:ax_deploy_dead"
        );
        assert!(refresh().await.unwrap());
        assert_eq!(cluster.writes().len(), 2);
        assert_eq!(
            password_for(&cluster.secret("prod-registry"), HOST),
            "deployment:ax_cap_v1_pull"
        );
    }

    /// A newer chart names the Secret. It is rewritten whatever it holds, and
    /// a Secret that cannot be read fails the refresh instead of reporting it
    /// done.
    #[tokio::test]
    async fn a_named_secret_is_rewritten_and_a_missing_one_fails() {
        let cluster = FakeCluster::default();
        cluster.put(docker_secret("prod-registry", json!({"auths": {}})));
        let client = cluster.client(&[]).await;

        assert!(refresh_secrets(
            &client,
            "ns",
            Some("prod-registry"),
            Some("prod"),
            HOST,
            "ax_cap_v1_pull"
        )
        .await
        .unwrap());
        assert_eq!(
            password_for(&cluster.secret("prod-registry"), HOST),
            "deployment:ax_cap_v1_pull"
        );
        assert!(refresh_secrets(
            &client,
            "ns",
            Some("gone-registry"),
            None,
            HOST,
            "ax_cap_v1_pull"
        )
        .await
        .is_err());
    }
}
