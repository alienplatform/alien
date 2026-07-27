//! Runtime state for resolving Postgres bindings.
//!
//! Inline-password bindings are immutable and their resolved handles are cached by
//! binding name. Cloud bindings are resolved on every load so password rotation is
//! observable, while their typed secret-store clients are initialized once and reused.

use crate::{
    error::{ErrorData, Result},
    traits::Postgres,
};
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use alien_core::Platform;
use alien_core::{bindings::PostgresBinding, ClientConfig};
use alien_error::AlienError;
#[cfg(feature = "aws")]
use alien_error::Context;
use std::{collections::HashMap, fmt, sync::Arc};
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use tokio::sync::OnceCell;
use tokio::sync::RwLock;

use super::ResolvedPostgres;

#[cfg(feature = "aws")]
use super::aurora;
#[cfg(feature = "gcp")]
use super::cloud_sql;
#[cfg(feature = "azure")]
use super::flexible_server;
#[cfg(feature = "aws")]
use alien_aws_clients::secrets_manager::{SecretsManagerApi, SecretsManagerClient};
#[cfg(feature = "azure")]
use alien_azure_clients::{
    keyvault::{AzureKeyVaultSecretsClient, KeyVaultSecretsApi},
    AzureTokenCache,
};
#[cfg(feature = "gcp")]
use alien_gcp_clients::secret_manager::{SecretManagerApi, SecretManagerClient};

pub(crate) struct PostgresRuntime {
    #[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
    client_config: ClientConfig,
    inline: RwLock<HashMap<String, Arc<dyn Postgres>>>,
    #[cfg(feature = "aws")]
    aws_secrets: OnceCell<Arc<dyn SecretsManagerApi>>,
    #[cfg(feature = "gcp")]
    gcp_secrets: OnceCell<Arc<dyn SecretManagerApi>>,
    #[cfg(feature = "azure")]
    azure_secrets: OnceCell<Arc<dyn KeyVaultSecretsApi>>,
}

impl fmt::Debug for PostgresRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("PostgresRuntime");
        debug.field("client_config", &"<redacted>");
        debug.field("inline", &"<cached bindings>");
        #[cfg(feature = "aws")]
        debug.field("aws_secrets_initialized", &self.aws_secrets.initialized());
        #[cfg(feature = "gcp")]
        debug.field("gcp_secrets_initialized", &self.gcp_secrets.initialized());
        #[cfg(feature = "azure")]
        debug.field(
            "azure_secrets_initialized",
            &self.azure_secrets.initialized(),
        );
        debug.finish()
    }
}

impl PostgresRuntime {
    pub(crate) fn new(client_config: ClientConfig) -> Self {
        #[cfg(not(any(feature = "aws", feature = "gcp", feature = "azure")))]
        let _ = client_config;

        Self {
            #[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
            client_config,
            inline: RwLock::new(HashMap::new()),
            #[cfg(feature = "aws")]
            aws_secrets: OnceCell::new(),
            #[cfg(feature = "gcp")]
            gcp_secrets: OnceCell::new(),
            #[cfg(feature = "azure")]
            azure_secrets: OnceCell::new(),
        }
    }

    pub(crate) async fn load(
        &self,
        binding_name: &str,
        binding: &PostgresBinding,
    ) -> Result<Arc<dyn Postgres>> {
        match binding {
            PostgresBinding::Local(_) | PostgresBinding::External(_) => {
                self.load_inline(binding_name, binding).await
            }
            #[cfg(feature = "aws")]
            PostgresBinding::Aurora(config) => {
                let params =
                    aurora::resolve(binding_name, config, self.aws_secrets().await?).await?;
                Ok(Arc::new(ResolvedPostgres::new(params)))
            }
            #[cfg(not(feature = "aws"))]
            PostgresBinding::Aurora(_) => Err(feature_not_enabled("aws")),
            #[cfg(feature = "gcp")]
            PostgresBinding::CloudSql(config) => {
                let params =
                    cloud_sql::resolve(binding_name, config, self.gcp_secrets().await?).await?;
                Ok(Arc::new(ResolvedPostgres::new(params)))
            }
            #[cfg(not(feature = "gcp"))]
            PostgresBinding::CloudSql(_) => Err(feature_not_enabled("gcp")),
            #[cfg(feature = "azure")]
            PostgresBinding::FlexibleServer(config) => {
                let params =
                    flexible_server::resolve(binding_name, config, self.azure_secrets().await?)
                        .await?;
                Ok(Arc::new(ResolvedPostgres::new(params)))
            }
            #[cfg(not(feature = "azure"))]
            PostgresBinding::FlexibleServer(_) => Err(feature_not_enabled("azure")),
        }
    }

    async fn load_inline(
        &self,
        binding_name: &str,
        binding: &PostgresBinding,
    ) -> Result<Arc<dyn Postgres>> {
        if let Some(postgres) = self.inline.read().await.get(binding_name).cloned() {
            return Ok(postgres);
        }

        let resolved: Arc<dyn Postgres> =
            Arc::new(ResolvedPostgres::from_binding(binding_name, binding)?);
        let mut cache = self.inline.write().await;
        Ok(cache
            .entry(binding_name.to_string())
            .or_insert(resolved)
            .clone())
    }

    #[cfg(feature = "aws")]
    async fn aws_secrets(&self) -> Result<Arc<dyn SecretsManagerApi>> {
        let client = self
            .aws_secrets
            .get_or_try_init(|| async {
                let config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;
                let credentials =
                    alien_aws_clients::AwsCredentialProvider::from_config(config.clone())
                        .await
                        .context(ErrorData::ClientConfigInvalid {
                            platform: Platform::Aws,
                            message: "Failed to create AWS credential provider".to_string(),
                        })?;
                Ok::<Arc<dyn SecretsManagerApi>, AlienError<ErrorData>>(Arc::new(
                    SecretsManagerClient::new(
                        crate::http_client::create_http_client(),
                        credentials,
                    ),
                ))
            })
            .await?;
        Ok(client.clone())
    }

    #[cfg(feature = "gcp")]
    async fn gcp_secrets(&self) -> Result<Arc<dyn SecretManagerApi>> {
        let client = self
            .gcp_secrets
            .get_or_try_init(|| async {
                let config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;
                Ok::<Arc<dyn SecretManagerApi>, AlienError<ErrorData>>(Arc::new(
                    SecretManagerClient::new(
                        crate::http_client::create_http_client(),
                        config.clone(),
                    ),
                ))
            })
            .await?;
        Ok(client.clone())
    }

    #[cfg(feature = "azure")]
    async fn azure_secrets(&self) -> Result<Arc<dyn KeyVaultSecretsApi>> {
        let client = self
            .azure_secrets
            .get_or_try_init(|| async {
                let config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;
                Ok::<Arc<dyn KeyVaultSecretsApi>, AlienError<ErrorData>>(Arc::new(
                    AzureKeyVaultSecretsClient::new(
                        crate::http_client::create_http_client(),
                        AzureTokenCache::new(config.clone()),
                    ),
                ))
            })
            .await?;
        Ok(client.clone())
    }
}

#[cfg(any(not(feature = "aws"), not(feature = "gcp"), not(feature = "azure")))]
fn feature_not_enabled(feature: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::FeatureNotEnabled {
        feature: feature.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inline_password_binding_is_served_from_the_cache() {
        let runtime = PostgresRuntime::new(ClientConfig::Test);
        let binding = PostgresBinding::Local(alien_core::bindings::LocalPostgresBinding {
            host: "127.0.0.1".into(),
            port: alien_core::bindings::BindingValue::value(6543),
            database: "app".into(),
            username: "alien".into(),
            password: "inline-pw".into(),
        });

        let first = runtime.load("db", &binding).await.expect("first load");
        let second = runtime.load("db", &binding).await.expect("second load");

        assert!(
            Arc::ptr_eq(&first, &second),
            "an inline-password binding should return the cached handle"
        );
        assert_eq!(first.connection_params().password, "inline-pw");
    }

    #[cfg(feature = "aws")]
    mod aws_tests {
        use super::*;
        use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
        use axum::{
            extract::{ConnectInfo, State},
            routing::post,
            Json, Router,
        };
        use std::{net::SocketAddr, sync::Mutex};

        const SECRET_ARN: &str = "arn:aws:secretsmanager:us-east-1:000000000000:secret:pg-AbCdEf";
        type ReadLog = Arc<Mutex<Vec<SocketAddr>>>;

        async fn get_secret_value(
            State(reads): State<ReadLog>,
            ConnectInfo(peer): ConnectInfo<SocketAddr>,
        ) -> Json<serde_json::Value> {
            let nth = {
                let mut reads = reads.lock().expect("read log");
                reads.push(peer);
                reads.len()
            };
            Json(serde_json::json!({
                "ARN": SECRET_ARN,
                "Name": "pg",
                "SecretString": format!("password-v{nth}"),
            }))
        }

        async fn spawn_secrets_manager() -> (SocketAddr, ReadLog) {
            let reads: ReadLog = Arc::new(Mutex::new(Vec::new()));
            let app = Router::new()
                .route("/", post(get_secret_value))
                .with_state(reads.clone());
            let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            tokio::spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .expect("serve");
            });
            (addr, reads)
        }

        fn runtime(addr: SocketAddr) -> PostgresRuntime {
            PostgresRuntime::new(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: "000000000000".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                    secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
                    session_token: None,
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: HashMap::from([(
                        "secretsmanager".to_string(),
                        format!("http://{addr}"),
                    )]),
                }),
            })))
        }

        fn binding() -> PostgresBinding {
            serde_json::from_value(serde_json::json!({
                "service": "aurora",
                "clusterEndpoint": "cluster.cluster-abc.us-east-1.rds.amazonaws.com",
                "port": 5432,
                "database": "app",
                "username": "alien",
                "passwordSecretArn": SECRET_ARN,
            }))
            .expect("binding parses")
        }

        #[tokio::test]
        async fn cloud_binding_rereads_its_secret_on_every_load() {
            let (addr, reads) = spawn_secrets_manager().await;
            let runtime = runtime(addr);
            let binding = binding();

            let first = runtime.load("db", &binding).await.expect("first load");
            let second = runtime.load("db", &binding).await.expect("second load");

            assert_eq!(reads.lock().expect("read log").len(), 2);
            assert_eq!(first.connection_params().password, "password-v1");
            assert_eq!(second.connection_params().password, "password-v2");
        }

        #[tokio::test]
        async fn concurrent_first_loads_initialize_one_secret_store_client() {
            let (addr, _reads) = spawn_secrets_manager().await;
            let runtime = Arc::new(runtime(addr));
            let binding = Arc::new(binding());

            let (first, second) = tokio::join!(
                runtime.load("db", binding.as_ref()),
                runtime.load("db", binding.as_ref()),
            );

            first.expect("first load");
            second.expect("second load");
            assert!(
                runtime.aws_secrets.get().is_some(),
                "the typed client should be initialized once and shared"
            );
        }

        #[tokio::test]
        async fn cloud_binding_reuses_its_secret_store_client_across_loads() {
            let (addr, reads) = spawn_secrets_manager().await;
            let runtime = runtime(addr);
            let binding = binding();

            runtime.load("db", &binding).await.expect("first load");
            runtime.load("db", &binding).await.expect("second load");

            let peers = reads.lock().expect("read log").clone();
            assert_eq!(peers.len(), 2);
            assert_eq!(
                peers[0], peers[1],
                "both reads should reuse one client connection pool"
            );
        }
    }
}
