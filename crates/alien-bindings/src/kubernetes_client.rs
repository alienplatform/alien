use alien_client_config::ClientConfigExt;
use alien_client_core::{ErrorData, Result};
use alien_core::{ClientConfig, KubernetesClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use k8s_openapi::{
    api::{batch::v1::Job, core::v1::Secret},
    List,
};
use kube::{
    api::{Api, DeleteParams, ListParams, ObjectList, PostParams},
    config::{AuthInfo, Cluster, Context as KubeContext, KubeConfigOptions, Kubeconfig},
    Client, Config,
};
use secrecy::SecretString;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fmt::{self, Debug};

#[async_trait]
pub trait KubernetesClientConfigExt {
    async fn from_env(
        environment_variables: &HashMap<String, String>,
    ) -> Result<KubernetesClientConfig>;
    async fn from_std_env() -> Result<KubernetesClientConfig>;
}

#[async_trait]
impl KubernetesClientConfigExt for KubernetesClientConfig {
    async fn from_env(
        environment_variables: &HashMap<String, String>,
    ) -> Result<KubernetesClientConfig> {
        let config = ClientConfig::from_env(Platform::Kubernetes, environment_variables).await?;
        match config {
            ClientConfig::Kubernetes(config) => Ok(*config),
            _ => Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Expected Kubernetes client configuration".to_string(),
                errors: None,
            })),
        }
    }

    async fn from_std_env() -> Result<KubernetesClientConfig> {
        Self::from_env(&std::env::vars().collect()).await
    }
}

#[async_trait]
pub trait SecretsApi: Send + Sync + std::fmt::Debug {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret>;
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret>;
    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret>;
    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()>;
}

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
    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()>;
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
}

async fn kube_config_from_alien_config(config: KubernetesClientConfig) -> Result<Config> {
    let mut kube_config = match config {
        KubernetesClientConfig::InCluster {
            namespace,
            additional_headers,
        } => {
            let mut config =
                Config::incluster()
                    .into_alien_error()
                    .context(ErrorData::InvalidClientConfig {
                        message: "Failed to load in-cluster Kubernetes configuration".to_string(),
                        errors: None,
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
                    .context(ErrorData::InvalidClientConfig {
                        message: "Failed to load kubeconfig-based Kubernetes configuration"
                            .to_string(),
                        errors: None,
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
            .context(ErrorData::InvalidClientConfig {
                message: "Failed to load manual Kubernetes configuration".to_string(),
                errors: None,
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

fn apply_headers(config: &mut Config, headers: Option<HashMap<String, String>>) -> Result<()> {
    let Some(headers) = headers else {
        return Ok(());
    };

    for (key, value) in headers {
        let header_name =
            key.parse()
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!("Invalid Kubernetes header name '{key}'"),
                    errors: None,
                })?;
        let header_value =
            value
                .parse()
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: format!("Invalid Kubernetes header value for '{key}'"),
                    errors: None,
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

fn map_kube_error(
    error: kube::Error,
    resource_type: &str,
    resource_name: &str,
) -> AlienError<ErrorData> {
    match error {
        kube::Error::Api(response) => match response.code {
            404 => AlienError::new(ErrorData::RemoteResourceNotFound {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            }),
            409 => AlienError::new(ErrorData::RemoteResourceConflict {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
                message: response.message,
            }),
            401 | 403 => AlienError::new(ErrorData::RemoteAccessDenied {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            }),
            429 => AlienError::new(ErrorData::RateLimitExceeded {
                message: response.message,
            }),
            500 | 502 | 503 | 504 => AlienError::new(ErrorData::RemoteServiceUnavailable {
                message: response.message,
            }),
            _ => AlienError::new(ErrorData::HttpResponseError {
                message: response.message,
                url: resource_type.to_string(),
                http_status: response.code,
                http_request_text: None,
                http_response_text: None,
            }),
        },
        other => AlienError::new(ErrorData::HttpRequestFailed {
            message: other.to_string(),
        }),
    }
}

async fn create<K>(api: Api<K>, value: &K, resource_type: &str) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned + Serialize + kube::ResourceExt,
{
    let resource_name = value.name_any();
    api.create(&PostParams::default(), value)
        .await
        .map_err(|error| map_kube_error(error, resource_type, &resource_name))
}

async fn get<K>(api: Api<K>, name: &str, resource_type: &str) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned,
{
    api.get(name)
        .await
        .map_err(|error| map_kube_error(error, resource_type, name))
}

async fn replace<K>(api: Api<K>, name: &str, value: &K, resource_type: &str) -> Result<K>
where
    K: Clone + Debug + DeserializeOwned + Serialize,
{
    api.replace(name, &PostParams::default(), value)
        .await
        .map_err(|error| map_kube_error(error, resource_type, name))
}

async fn delete<K>(api: Api<K>, name: &str, resource_type: &str) -> Result<()>
where
    K: Clone + Debug + DeserializeOwned,
{
    api.delete(name, &DeleteParams::default())
        .await
        .map(|_| ())
        .map_err(|error| map_kube_error(error, resource_type, name))
}

#[async_trait]
impl SecretsApi for KubernetesClient {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        create(self.namespaced(namespace), secret, "Secret").await
    }

    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        get(self.namespaced(namespace), name, "Secret").await
    }

    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret> {
        replace(self.namespaced(namespace), name, secret, "Secret").await
    }

    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Secret>(self.namespaced(namespace), name, "Secret").await
    }
}

#[async_trait]
impl JobApi for KubernetesClient {
    async fn create_job(&self, namespace: &str, job: &Job) -> Result<Job> {
        create(self.namespaced(namespace), job, "Job").await
    }

    async fn get_job(&self, namespace: &str, name: &str) -> Result<Job> {
        get(self.namespaced(namespace), name, "Job").await
    }

    async fn list_jobs(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Job>> {
        let jobs = self
            .namespaced(namespace)
            .list(&list_params(label_selector, field_selector))
            .await
            .map_err(|error| map_kube_error(error, "Job", namespace))?;
        convert_list(jobs)
    }

    async fn delete_job(&self, namespace: &str, name: &str) -> Result<()> {
        delete::<Job>(self.namespaced(namespace), name, "Job").await
    }
}
