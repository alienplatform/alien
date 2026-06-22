use alien_client_core::{ErrorData, Result};
use alien_core::KubernetesClientConfig;
use alien_error::AlienError;
use alien_error::{Context, IntoAlienError};
use kube::{
    api::{Api, ListParams, PostParams},
    config::{AuthInfo, Cluster, Context as KubeContext, KubeConfigOptions, Kubeconfig},
    Client, Config,
};
use secrecy::SecretString;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalKubernetesReadSource {
    MetricsApi,
    Events,
    Nodes,
}

impl OptionalKubernetesReadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MetricsApi => "metricsApi",
            Self::Events => "events",
            Self::Nodes => "nodes",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionalKubernetesReadStatus {
    pub available: bool,
    pub reason: Option<alien_core::HeartbeatCollectionIssueReason>,
    pub message: Option<String>,
}

impl OptionalKubernetesReadStatus {
    pub fn available() -> Self {
        Self {
            available: true,
            reason: None,
            message: None,
        }
    }

    pub fn unavailable(
        reason: alien_core::HeartbeatCollectionIssueReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OptionalKubernetesReadContext<'a> {
    pub source: OptionalKubernetesReadSource,
    pub resource_id: &'a str,
    pub namespace: Option<&'a str>,
    pub kubernetes_resource: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionalKubernetesRead<T> {
    pub value: Option<T>,
    pub status: OptionalKubernetesReadStatus,
}

impl<T> OptionalKubernetesRead<T> {
    fn available(value: T) -> Self {
        Self {
            value: Some(value),
            status: OptionalKubernetesReadStatus::available(),
        }
    }

    fn unavailable(reason: alien_core::HeartbeatCollectionIssueReason, message: String) -> Self {
        Self {
            value: None,
            status: OptionalKubernetesReadStatus::unavailable(reason, message),
        }
    }
}

pub async fn optional_kubernetes_read<T, F>(
    context: OptionalKubernetesReadContext<'_>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    match read.await {
        Ok(value) => Ok(OptionalKubernetesRead::available(value)),
        Err(error) => {
            if let Some((reason, message, log_at_info)) =
                classify_optional_kubernetes_error(context.source, &error)
            {
                if log_at_info {
                    tracing::info!(
                        source = context.source.as_str(),
                        resource_id = context.resource_id,
                        namespace = context.namespace,
                        kubernetes_resource = context.kubernetes_resource,
                        reason = ?reason,
                        error = %error,
                        "Optional Kubernetes heartbeat collection unavailable"
                    );
                } else {
                    tracing::debug!(
                        source = context.source.as_str(),
                        resource_id = context.resource_id,
                        namespace = context.namespace,
                        kubernetes_resource = context.kubernetes_resource,
                        reason = ?reason,
                        error = %error,
                        "Optional Kubernetes heartbeat collection unavailable"
                    );
                }

                Ok(OptionalKubernetesRead::unavailable(reason, message))
            } else {
                Err(error)
            }
        }
    }
}

pub async fn optional_metrics_read<T, F>(
    resource_id: &str,
    namespace: Option<&str>,
    kubernetes_resource: Option<&str>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::MetricsApi,
            resource_id,
            namespace,
            kubernetes_resource,
        },
        read,
    )
    .await
}

pub async fn optional_events_read<T, F>(
    resource_id: &str,
    namespace: &str,
    kubernetes_resource: Option<&str>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::Events,
            resource_id,
            namespace: Some(namespace),
            kubernetes_resource,
        },
        read,
    )
    .await
}

pub async fn optional_nodes_read<T, F>(
    resource_id: &str,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::Nodes,
            resource_id,
            namespace: None,
            kubernetes_resource: None,
        },
        read,
    )
    .await
}

fn classify_optional_kubernetes_error(
    source: OptionalKubernetesReadSource,
    error: &AlienError<ErrorData>,
) -> Option<(alien_core::HeartbeatCollectionIssueReason, String, bool)> {
    let error_data = error.error.as_ref()?;
    match error_data {
        ErrorData::RemoteAccessDenied { .. } => Some((
            alien_core::HeartbeatCollectionIssueReason::Forbidden,
            format!(
                "Kubernetes {} collection is forbidden by RBAC",
                source.as_str()
            ),
            true,
        )),
        ErrorData::RemoteResourceNotFound { .. }
            if matches!(source, OptionalKubernetesReadSource::MetricsApi) =>
        {
            Some((
                alien_core::HeartbeatCollectionIssueReason::NotInstalled,
                "Kubernetes metrics API is not installed".to_string(),
                false,
            ))
        }
        ErrorData::RemoteServiceUnavailable { .. } => Some((
            alien_core::HeartbeatCollectionIssueReason::ApiUnavailable,
            format!(
                "Kubernetes {} API is temporarily unavailable",
                source.as_str()
            ),
            false,
        )),
        ErrorData::Timeout { .. } => Some((
            alien_core::HeartbeatCollectionIssueReason::TimedOut,
            format!("Kubernetes {} collection timed out", source.as_str()),
            false,
        )),
        _ => None,
    }
}

pub(crate) async fn kube_client_from_alien_config(
    config: KubernetesClientConfig,
) -> Result<Client> {
    let client_config = kube_config_from_alien_config(config).await?;
    Client::try_from(client_config)
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to create Kubernetes client".to_string(),
        })
}

pub(crate) async fn kube_config_from_alien_config(
    config: KubernetesClientConfig,
) -> Result<Config> {
    let mut kube_config = match config {
        KubernetesClientConfig::InCluster {
            namespace,
            additional_headers,
        } => {
            let mut config =
                Config::incluster()
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Failed to load in-cluster Kubernetes configuration".to_string(),
                    })?;
            if let Some(namespace) = namespace {
                config.default_namespace = namespace;
            }
            apply_headers(&mut config, additional_headers)?;
            config
        }
        KubernetesClientConfig::Kubeconfig {
            kubeconfig_path,
            context,
            cluster,
            user,
            namespace,
            additional_headers,
        } => {
            let options = KubeConfigOptions {
                context,
                cluster,
                user,
            };
            let previous_kubeconfig = set_temporary_kubeconfig(kubeconfig_path.as_deref());
            let config_result = Config::from_kubeconfig(&options).await;
            restore_kubeconfig(previous_kubeconfig);

            let mut config =
                config_result
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Failed to load kubeconfig-based Kubernetes configuration"
                            .to_string(),
                    })?;
            if let Some(namespace) = namespace {
                config.default_namespace = namespace;
            }
            apply_headers(&mut config, additional_headers)?;
            config
        }
        KubernetesClientConfig::Manual {
            server_url,
            certificate_authority_data,
            insecure_skip_tls_verify,
            client_certificate_data,
            client_key_data,
            token,
            username,
            password,
            namespace,
            additional_headers,
        } => {
            let kubeconfig = Kubeconfig {
                clusters: vec![kube::config::NamedCluster {
                    name: "manual".to_string(),
                    cluster: Some(Cluster {
                        server: Some(server_url),
                        insecure_skip_tls_verify,
                        certificate_authority: None,
                        certificate_authority_data,
                        proxy_url: None,
                        disable_compression: None,
                        tls_server_name: None,
                        extensions: None,
                    }),
                }],
                auth_infos: vec![kube::config::NamedAuthInfo {
                    name: "manual".to_string(),
                    auth_info: Some(AuthInfo {
                        token: token.map(|token| SecretString::new(token.into())),
                        username,
                        password: password.map(|password| SecretString::new(password.into())),
                        client_certificate_data,
                        client_key_data: client_key_data.map(|key| SecretString::new(key.into())),
                        ..Default::default()
                    }),
                }],
                contexts: vec![kube::config::NamedContext {
                    name: "manual".to_string(),
                    context: Some(KubeContext {
                        cluster: "manual".to_string(),
                        user: Some("manual".to_string()),
                        namespace,
                        extensions: None,
                    }),
                }],
                current_context: Some("manual".to_string()),
                ..Default::default()
            };

            let mut config = Config::from_custom_kubeconfig(
                kubeconfig,
                &KubeConfigOptions {
                    context: Some("manual".to_string()),
                    cluster: None,
                    user: None,
                },
            )
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to load manual Kubernetes configuration".to_string(),
            })?;
            apply_headers(&mut config, Some(additional_headers))?;
            config
        }
    };

    kube_config.apply_debug_overrides();
    Ok(kube_config)
}

fn set_temporary_kubeconfig(kubeconfig_path: Option<&str>) -> Option<String> {
    let previous = std::env::var("KUBECONFIG").ok();
    if let Some(kubeconfig_path) = kubeconfig_path {
        std::env::set_var("KUBECONFIG", kubeconfig_path);
    }
    previous
}

fn restore_kubeconfig(previous_kubeconfig: Option<String>) {
    if let Some(previous_kubeconfig) = previous_kubeconfig {
        std::env::set_var("KUBECONFIG", previous_kubeconfig);
    } else {
        std::env::remove_var("KUBECONFIG");
    }
}

fn apply_headers(
    config: &mut Config,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Result<()> {
    let Some(headers) = headers else {
        return Ok(());
    };

    for (key, value) in headers {
        let header_name = key
            .parse()
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Invalid Kubernetes header name '{key}'"),
            })?;
        let header_value =
            value
                .parse()
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Invalid Kubernetes header value for '{key}'"),
                })?;
        config.headers.push((header_name, header_value));
    }

    Ok(())
}

pub(crate) fn list_params(
    label_selector: Option<String>,
    field_selector: Option<String>,
) -> ListParams {
    let mut params = ListParams::default();
    if let Some(label_selector) = label_selector {
        params = params.labels(&label_selector);
    }
    if let Some(field_selector) = field_selector {
        params = params.fields(&field_selector);
    }
    params
}

pub(crate) async fn create<K>(api: Api<K>, value: &K) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned + Serialize,
{
    api.create(&PostParams::default(), value)
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Kubernetes create operation failed".to_string(),
        })
}

pub(crate) async fn get<K>(api: Api<K>, name: &str) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned,
{
    api.get(name)
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Kubernetes get operation failed for '{name}'"),
        })
}

pub(crate) async fn replace<K>(api: Api<K>, name: &str, value: &K) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned + Serialize,
{
    api.replace(name, &PostParams::default(), value)
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Kubernetes replace operation failed for '{name}'"),
        })
}
