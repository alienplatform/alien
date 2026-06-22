use alien_client_core::{ErrorData, Result};
use alien_core::KubernetesClientConfig;
use alien_error::AlienError;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use k8s_openapi::{
    api::{
        apps::v1::{DaemonSet, Deployment, StatefulSet},
        batch::v1::Job,
        core::v1::{Event, Node, Pod, Secret, Service},
        networking::v1::Ingress,
    },
    apimachinery::pkg::version::Info as KubernetesVersionInfo,
    List,
};
use kube::{
    api::{Api, ApiResource, DeleteParams, DynamicObject, ListParams, ObjectList, PostParams},
    config::{AuthInfo, Cluster, Context as KubeContext, KubeConfigOptions, Kubeconfig},
    Client, Config,
};
#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;
use secrecy::SecretString;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::fmt::{self, Debug};

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait DeploymentApi: Send + Sync + std::fmt::Debug {
    async fn create_deployment(
        &self,
        namespace: &str,
        deployment: &Deployment,
    ) -> Result<Deployment>;
    async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment>;
    async fn list_deployments(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Deployment>>;
    async fn update_deployment(
        &self,
        namespace: &str,
        name: &str,
        deployment: &Deployment,
    ) -> Result<Deployment>;
    async fn delete_deployment(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_statefulset(
        &self,
        namespace: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet>;
    async fn get_statefulset(&self, namespace: &str, name: &str) -> Result<StatefulSet>;
    async fn update_statefulset(
        &self,
        namespace: &str,
        name: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet>;
    async fn delete_statefulset(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_daemonset(&self, namespace: &str, daemonset: &DaemonSet) -> Result<DaemonSet>;
    async fn get_daemonset(&self, namespace: &str, name: &str) -> Result<DaemonSet>;
    async fn update_daemonset(
        &self,
        namespace: &str,
        name: &str,
        daemonset: &DaemonSet,
    ) -> Result<DaemonSet>;
    async fn delete_daemonset(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait JobApi: Send + Sync + std::fmt::Debug {
    async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job>;
    async fn get_job(&self, namespace: &str, name: &str) -> Result<Job>;
    async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>>;
    async fn update_job(&self, namespace: &str, name: &str, job: &Job) -> Result<Job>;
    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait PodApi: Send + Sync + std::fmt::Debug {
    async fn create_pod(&self, namespace: &str, pod: &Pod) -> Result<Pod>;
    async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod>;
    async fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Pod>>;
    async fn update_pod(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod>;
    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait EventApi: Send + Sync + std::fmt::Debug {
    async fn list_events(
        &self,
        namespace: &str,
        field_selector: Option<String>,
    ) -> Result<List<Event>>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait NodeApi: Send + Sync + std::fmt::Debug {
    async fn list_nodes(
        &self,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Node>>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait MetricsApi: Send + Sync + std::fmt::Debug {
    async fn list_pod_metrics(
        &self,
        namespace: &str,
        label_selector: Option<String>,
    ) -> Result<ObjectList<DynamicObject>>;

    async fn list_node_metrics(
        &self,
        label_selector: Option<String>,
    ) -> Result<ObjectList<DynamicObject>>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait SecretsApi: Send + Sync + std::fmt::Debug {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret>;
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret>;
    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>>;
    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret>;
    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait ServiceApi: Send + Sync + std::fmt::Debug {
    async fn create_service(&self, namespace: &str, service: &Service) -> Result<Service>;
    async fn get_service(&self, namespace: &str, name: &str) -> Result<Service>;
    async fn list_services(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Service>>;
    async fn update_service(
        &self,
        namespace: &str,
        name: &str,
        service: &Service,
    ) -> Result<Service>;
    async fn delete_service(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait RouteApi: Send + Sync + std::fmt::Debug {
    async fn create_ingress(&self, namespace: &str, ingress: &Ingress) -> Result<Ingress>;
    async fn get_ingress(&self, namespace: &str, name: &str) -> Result<Ingress>;
    async fn update_ingress(
        &self,
        namespace: &str,
        name: &str,
        ingress: &Ingress,
    ) -> Result<Ingress>;
    async fn delete_ingress(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_gateway(&self, namespace: &str, gateway: &Value) -> Result<Value>;
    async fn get_gateway(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_gateway(&self, namespace: &str, name: &str, gateway: &Value) -> Result<Value>;
    async fn delete_gateway(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_http_route(&self, namespace: &str, route: &Value) -> Result<Value>;
    async fn get_http_route(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_http_route(&self, namespace: &str, name: &str, route: &Value) -> Result<Value>;
    async fn delete_http_route(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_gke_health_check_policy(
        &self,
        namespace: &str,
        policy: &Value,
    ) -> Result<Value>;
    async fn get_gke_health_check_policy(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_gke_health_check_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &Value,
    ) -> Result<Value>;
    async fn delete_gke_health_check_policy(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_azure_health_check_policy(
        &self,
        namespace: &str,
        policy: &Value,
    ) -> Result<Value>;
    async fn get_azure_health_check_policy(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_azure_health_check_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &Value,
    ) -> Result<Value>;
    async fn delete_azure_health_check_policy(&self, namespace: &str, name: &str) -> Result<()>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait]
pub trait VersionApi: Send + Sync + std::fmt::Debug {
    async fn get_version(&self) -> Result<KubernetesVersionInfo>;
}

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

#[derive(Clone)]
pub struct KubernetesClient {
    client: Client,
}

impl Debug for KubernetesClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("KubernetesClient").finish()
    }
}

impl KubernetesClient {
    pub async fn new(config: KubernetesClientConfig) -> Result<Self> {
        let client_config = kube_config_from_alien_config(config).await?;
        let client = Client::try_from(client_config).into_alien_error().context(
            ErrorData::HttpRequestFailed {
                message: "Failed to create Kubernetes client".to_string(),
            },
        )?;

        Ok(Self { client })
    }

    fn namespaced<K>(&self, namespace: &str) -> Api<K>
    where
        K: kube::Resource<Scope = kube::core::NamespaceResourceScope>,
        <K as kube::Resource>::DynamicType: Default,
    {
        Api::namespaced(self.client.clone(), namespace)
    }

    fn cluster<K>(&self) -> Api<K>
    where
        K: kube::Resource<Scope = kube::core::ClusterResourceScope>,
        <K as kube::Resource>::DynamicType: Default,
    {
        Api::all(self.client.clone())
    }

    fn dynamic_namespaced(
        &self,
        namespace: &str,
        group: &str,
        version: &str,
        kind: &str,
        plural: &str,
    ) -> Api<DynamicObject> {
        let resource = ApiResource {
            group: group.to_string(),
            version: version.to_string(),
            api_version: format!("{group}/{version}"),
            kind: kind.to_string(),
            plural: plural.to_string(),
        };

        Api::namespaced_with(self.client.clone(), namespace, &resource)
    }

    fn dynamic_cluster(
        &self,
        group: &str,
        version: &str,
        kind: &str,
        plural: &str,
    ) -> Api<DynamicObject> {
        let resource = ApiResource {
            group: group.to_string(),
            version: version.to_string(),
            api_version: format!("{group}/{version}"),
            kind: kind.to_string(),
            plural: plural.to_string(),
        };

        Api::all_with(self.client.clone(), &resource)
    }
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

fn list_params(label_selector: Option<String>, field_selector: Option<String>) -> ListParams {
    let mut params = ListParams::default();
    if let Some(label_selector) = label_selector {
        params = params.labels(&label_selector);
    }
    if let Some(field_selector) = field_selector {
        params = params.fields(&field_selector);
    }
    params
}

fn convert_list<K>(list: ObjectList<K>) -> Result<List<K>>
where
    K: Clone + DeserializeOwned + Serialize + k8s_openapi::ListableResource,
{
    serde_json::from_value(serde_json::to_value(list).into_alien_error().context(
        ErrorData::HttpRequestFailed {
            message: "Failed to serialize Kubernetes list response".to_string(),
        },
    )?)
    .into_alien_error()
    .context(ErrorData::HttpRequestFailed {
        message: "Failed to deserialize Kubernetes list response".to_string(),
    })
}

fn dynamic_value(value: Value) -> Result<DynamicObject> {
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to deserialize Kubernetes dynamic object".to_string(),
        })
}

fn value_from_dynamic(object: DynamicObject) -> Result<Value> {
    serde_json::to_value(object)
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to serialize Kubernetes dynamic object".to_string(),
        })
}

async fn create<K>(api: Api<K>, value: &K) -> Result<K>
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

async fn get<K>(api: Api<K>, name: &str) -> Result<K>
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

async fn list<K>(
    api: Api<K>,
    label_selector: Option<String>,
    field_selector: Option<String>,
) -> Result<List<K>>
where
    K: Clone + Debug + DeserializeOwned + Serialize + k8s_openapi::ListableResource,
{
    let list = api
        .list(&list_params(label_selector, field_selector))
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Kubernetes list operation failed".to_string(),
        })?;
    convert_list(list)
}

async fn replace<K>(api: Api<K>, name: &str, value: &K) -> Result<K>
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

async fn delete<K>(api: Api<K>, name: &str) -> Result<()>
where
    K: Clone + Debug + DeserializeOwned,
{
    api.delete(name, &DeleteParams::default())
        .await
        .map(|_| ())
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Kubernetes delete operation failed for '{name}'"),
        })
}

async fn create_dynamic(api: Api<DynamicObject>, value: &Value) -> Result<Value> {
    let object = dynamic_value(value.clone())?;
    let created = create(api, &object).await?;
    value_from_dynamic(created)
}

async fn get_dynamic(api: Api<DynamicObject>, name: &str) -> Result<Value> {
    value_from_dynamic(get(api, name).await?)
}

async fn replace_dynamic(api: Api<DynamicObject>, name: &str, value: &Value) -> Result<Value> {
    let object = dynamic_value(value.clone())?;
    let replaced = replace(api, name, &object).await?;
    value_from_dynamic(replaced)
}

#[async_trait]
impl DeploymentApi for KubernetesClient {
    async fn create_deployment(
        &self,
        namespace: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        create(self.namespaced(namespace), deployment).await
    }

    async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment> {
        get(self.namespaced(namespace), name).await
    }

    async fn list_deployments(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Deployment>> {
        list(self.namespaced(namespace), label_selector, field_selector).await
    }

    async fn update_deployment(
        &self,
        namespace: &str,
        name: &str,
        deployment: &Deployment,
    ) -> Result<Deployment> {
        replace(self.namespaced(namespace), name, deployment).await
    }

    async fn delete_deployment(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Deployment>(self.namespaced(namespace), name).await
    }

    async fn create_statefulset(
        &self,
        namespace: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        create(self.namespaced(namespace), statefulset).await
    }

    async fn get_statefulset(&self, namespace: &str, name: &str) -> Result<StatefulSet> {
        get(self.namespaced(namespace), name).await
    }

    async fn update_statefulset(
        &self,
        namespace: &str,
        name: &str,
        statefulset: &StatefulSet,
    ) -> Result<StatefulSet> {
        replace(self.namespaced(namespace), name, statefulset).await
    }

    async fn delete_statefulset(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<StatefulSet>(self.namespaced(namespace), name).await
    }

    async fn create_daemonset(&self, namespace: &str, daemonset: &DaemonSet) -> Result<DaemonSet> {
        create(self.namespaced(namespace), daemonset).await
    }

    async fn get_daemonset(&self, namespace: &str, name: &str) -> Result<DaemonSet> {
        get(self.namespaced(namespace), name).await
    }

    async fn update_daemonset(
        &self,
        namespace: &str,
        name: &str,
        daemonset: &DaemonSet,
    ) -> Result<DaemonSet> {
        replace(self.namespaced(namespace), name, daemonset).await
    }

    async fn delete_daemonset(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<DaemonSet>(self.namespaced(namespace), name).await
    }
}

#[async_trait]
impl JobApi for KubernetesClient {
    async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job> {
        create(self.namespaced(namespace), job).await
    }

    async fn get_job(&self, namespace: &str, name: &str) -> Result<Job> {
        get(self.namespaced(namespace), name).await
    }

    async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>> {
        list(self.namespaced(namespace), label_selector, field_selector).await
    }

    async fn update_job(&self, namespace: &str, name: &str, job: &Job) -> Result<Job> {
        replace(self.namespaced(namespace), name, job).await
    }

    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Job>(self.namespaced(namespace), name).await
    }
}

#[async_trait]
impl PodApi for KubernetesClient {
    async fn create_pod(&self, namespace: &str, pod: &Pod) -> Result<Pod> {
        create(self.namespaced(namespace), pod).await
    }

    async fn get_pod(&self, namespace: &str, name: &str) -> Result<Pod> {
        get(self.namespaced(namespace), name).await
    }

    async fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Pod>> {
        list(self.namespaced(namespace), label_selector, field_selector).await
    }

    async fn update_pod(&self, namespace: &str, name: &str, pod: &Pod) -> Result<Pod> {
        replace(self.namespaced(namespace), name, pod).await
    }

    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Pod>(self.namespaced(namespace), name).await
    }
}

#[async_trait]
impl EventApi for KubernetesClient {
    async fn list_events(
        &self,
        namespace: &str,
        field_selector: Option<String>,
    ) -> Result<List<Event>> {
        list(self.namespaced(namespace), None, field_selector).await
    }
}

#[async_trait]
impl NodeApi for KubernetesClient {
    async fn list_nodes(
        &self,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Node>> {
        list(self.cluster(), label_selector, field_selector).await
    }
}

#[async_trait]
impl MetricsApi for KubernetesClient {
    async fn list_pod_metrics(
        &self,
        namespace: &str,
        label_selector: Option<String>,
    ) -> Result<ObjectList<DynamicObject>> {
        let api =
            self.dynamic_namespaced(namespace, "metrics.k8s.io", "v1beta1", "PodMetrics", "pods");
        api.list(&list_params(label_selector, None))
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Kubernetes pod metrics list operation failed".to_string(),
            })
    }

    async fn list_node_metrics(
        &self,
        label_selector: Option<String>,
    ) -> Result<ObjectList<DynamicObject>> {
        let api = self.dynamic_cluster("metrics.k8s.io", "v1beta1", "NodeMetrics", "nodes");
        api.list(&list_params(label_selector, None))
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Kubernetes node metrics list operation failed".to_string(),
            })
    }
}

#[async_trait]
impl SecretsApi for KubernetesClient {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        create(self.namespaced(namespace), secret).await
    }

    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        get(self.namespaced(namespace), name).await
    }

    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>> {
        list(self.namespaced(namespace), label_selector, field_selector).await
    }

    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret> {
        replace(self.namespaced(namespace), name, secret).await
    }

    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Secret>(self.namespaced(namespace), name).await
    }
}

#[async_trait]
impl ServiceApi for KubernetesClient {
    async fn create_service(&self, namespace: &str, service: &Service) -> Result<Service> {
        create(self.namespaced(namespace), service).await
    }

    async fn get_service(&self, namespace: &str, name: &str) -> Result<Service> {
        get(self.namespaced(namespace), name).await
    }

    async fn list_services(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Service>> {
        list(self.namespaced(namespace), label_selector, field_selector).await
    }

    async fn update_service(
        &self,
        namespace: &str,
        name: &str,
        service: &Service,
    ) -> Result<Service> {
        replace(self.namespaced(namespace), name, service).await
    }

    async fn delete_service(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Service>(self.namespaced(namespace), name).await
    }
}

#[async_trait]
impl RouteApi for KubernetesClient {
    async fn create_ingress(&self, namespace: &str, ingress: &Ingress) -> Result<Ingress> {
        create(self.namespaced(namespace), ingress).await
    }

    async fn get_ingress(&self, namespace: &str, name: &str) -> Result<Ingress> {
        get(self.namespaced(namespace), name).await
    }

    async fn update_ingress(
        &self,
        namespace: &str,
        name: &str,
        ingress: &Ingress,
    ) -> Result<Ingress> {
        replace(self.namespaced(namespace), name, ingress).await
    }

    async fn delete_ingress(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Ingress>(self.namespaced(namespace), name).await
    }

    async fn create_gateway(&self, namespace: &str, gateway: &Value) -> Result<Value> {
        create_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "Gateway",
                "gateways",
            ),
            gateway,
        )
        .await
    }

    async fn get_gateway(&self, namespace: &str, name: &str) -> Result<Value> {
        get_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "Gateway",
                "gateways",
            ),
            name,
        )
        .await
    }

    async fn update_gateway(&self, namespace: &str, name: &str, gateway: &Value) -> Result<Value> {
        replace_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "Gateway",
                "gateways",
            ),
            name,
            gateway,
        )
        .await
    }

    async fn delete_gateway(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<DynamicObject>(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "Gateway",
                "gateways",
            ),
            name,
        )
        .await
    }

    async fn create_http_route(&self, namespace: &str, route: &Value) -> Result<Value> {
        create_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "HTTPRoute",
                "httproutes",
            ),
            route,
        )
        .await
    }

    async fn get_http_route(&self, namespace: &str, name: &str) -> Result<Value> {
        get_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "HTTPRoute",
                "httproutes",
            ),
            name,
        )
        .await
    }

    async fn update_http_route(&self, namespace: &str, name: &str, route: &Value) -> Result<Value> {
        replace_dynamic(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "HTTPRoute",
                "httproutes",
            ),
            name,
            route,
        )
        .await
    }

    async fn delete_http_route(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<DynamicObject>(
            self.dynamic_namespaced(
                namespace,
                "gateway.networking.k8s.io",
                "v1",
                "HTTPRoute",
                "httproutes",
            ),
            name,
        )
        .await
    }

    async fn create_gke_health_check_policy(
        &self,
        namespace: &str,
        policy: &Value,
    ) -> Result<Value> {
        create_dynamic(
            self.dynamic_namespaced(
                namespace,
                "networking.gke.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicies",
            ),
            policy,
        )
        .await
    }

    async fn get_gke_health_check_policy(&self, namespace: &str, name: &str) -> Result<Value> {
        get_dynamic(
            self.dynamic_namespaced(
                namespace,
                "networking.gke.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicies",
            ),
            name,
        )
        .await
    }

    async fn update_gke_health_check_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &Value,
    ) -> Result<Value> {
        replace_dynamic(
            self.dynamic_namespaced(
                namespace,
                "networking.gke.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicies",
            ),
            name,
            policy,
        )
        .await
    }

    async fn delete_gke_health_check_policy(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<DynamicObject>(
            self.dynamic_namespaced(
                namespace,
                "networking.gke.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicies",
            ),
            name,
        )
        .await
    }

    async fn create_azure_health_check_policy(
        &self,
        namespace: &str,
        policy: &Value,
    ) -> Result<Value> {
        create_dynamic(
            self.dynamic_namespaced(
                namespace,
                "alb.networking.azure.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicy",
            ),
            policy,
        )
        .await
    }

    async fn get_azure_health_check_policy(&self, namespace: &str, name: &str) -> Result<Value> {
        get_dynamic(
            self.dynamic_namespaced(
                namespace,
                "alb.networking.azure.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicy",
            ),
            name,
        )
        .await
    }

    async fn update_azure_health_check_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &Value,
    ) -> Result<Value> {
        replace_dynamic(
            self.dynamic_namespaced(
                namespace,
                "alb.networking.azure.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicy",
            ),
            name,
            policy,
        )
        .await
    }

    async fn delete_azure_health_check_policy(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<DynamicObject>(
            self.dynamic_namespaced(
                namespace,
                "alb.networking.azure.io",
                "v1",
                "HealthCheckPolicy",
                "healthcheckpolicy",
            ),
            name,
        )
        .await
    }
}

#[async_trait]
impl VersionApi for KubernetesClient {
    async fn get_version(&self) -> Result<KubernetesVersionInfo> {
        self.client
            .apiserver_version()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Kubernetes version request failed".to_string(),
            })
    }
}
