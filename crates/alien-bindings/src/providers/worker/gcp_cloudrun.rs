use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use alien_core::bindings::CloudRunWorkerBinding;
use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Client, Method, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Worker, WorkerInvokeRequest, WorkerInvokeResponse};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const CLOUD_RUN_REST_BASE_URL: &str = "https://run.googleapis.com/v2";

/// GCP Cloud Run worker binding implementation
#[derive(Debug)]
pub struct CloudRunWorker {
    client: Client,
    project_id: String,
    endpoint: String,
    credentials: Credentials,
    binding: CloudRunWorkerBinding,
}

impl CloudRunWorker {
    pub fn new(
        client: Client,
        config: GcpClientConfig,
        binding: CloudRunWorkerBinding,
    ) -> Result<Self> {
        let endpoint = config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("cloudrun"))
            .cloned()
            .unwrap_or_else(|| CLOUD_RUN_REST_BASE_URL.to_string());

        Ok(Self {
            client,
            project_id: config.project_id.clone(),
            endpoint,
            credentials: credentials_from_gcp_config(&config)?,
            binding,
        })
    }

    /// Get the private URL from the binding, resolving template expressions if needed
    fn get_private_url(&self) -> Result<String> {
        self.binding
            .private_url
            .clone()
            .into_value("worker", "private_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve private_url from binding".to_string(),
            })
    }

    /// Resolve the target URL for invocation
    async fn resolve_target_url(&self, target_worker: &str) -> Result<String> {
        if !target_worker.is_empty() {
            // Check if target_worker looks like a URL (starts with http)
            if target_worker.starts_with("http://") || target_worker.starts_with("https://") {
                // Use the provided target worker as URL
                Ok(target_worker.to_string())
            } else {
                // target_worker is likely a path/identifier, use binding URL
                self.get_private_url()
            }
        } else {
            // Use the private URL from binding
            self.get_private_url()
        }
    }

    fn build_url(&self, location: &str, service_name: &str) -> Result<Url> {
        Url::parse(&format!(
            "{}/projects/{}/locations/{}/services/{}",
            self.endpoint.trim_end_matches('/'),
            self.project_id,
            location,
            service_name
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.cloudRun".to_string(),
            reason: "Invalid Cloud Run service URL".to_string(),
        })
    }

    async fn authed_request(&self, method: Method, url: Url) -> Result<reqwest::RequestBuilder> {
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.cloudRun".to_string(),
                reason: "Failed to get Google auth headers".to_string(),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "worker.cloudRun".to_string(),
                    reason: "Google auth returned NotModified without cached headers".to_string(),
                }));
            }
        };

        Ok(self.client.request(method, url).headers(headers))
    }
}

impl Binding for CloudRunWorker {}

#[async_trait]
impl Worker for CloudRunWorker {
    async fn invoke(&self, request: WorkerInvokeRequest) -> Result<WorkerInvokeResponse> {
        let target_url = self.resolve_target_url(&request.target_worker).await?;

        // Construct the full URL with path
        let url = if request.path.starts_with('/') {
            format!("{}{}", target_url.trim_end_matches('/'), request.path)
        } else {
            format!("{}/{}", target_url.trim_end_matches('/'), request.path)
        };

        // Build the HTTP request
        let method = match request.method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            _ => {
                return Err(AlienError::new(ErrorData::InvalidInput {
                    operation_context: "Worker invocation".to_string(),
                    details: format!("Unsupported HTTP method: {}", request.method),
                    field_name: Some("method".to_string()),
                }));
            }
        };

        let mut req_builder = self.client.request(method, &url);

        // Add headers
        for (key, value) in &request.headers {
            req_builder = req_builder.header(key, value);
        }

        // Add body if present
        if !request.body.is_empty() {
            req_builder = req_builder.body(request.body.clone());
        }

        // Set timeout if specified
        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Send the request
        let response =
            req_builder
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.clone(),
                    method: request.method.clone(),
                })?;

        // Extract response components
        let status = response.status().as_u16();

        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect::<BTreeMap<String, String>>();

        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.clone(),
                method: "READ_BODY".to_string(),
            })?
            .to_vec();

        Ok(WorkerInvokeResponse {
            status,
            headers,
            body,
        })
    }

    async fn get_worker_url(&self) -> Result<Option<String>> {
        // First check if we have it in the binding
        if let Some(url_binding) = &self.binding.public_url {
            let url = url_binding
                .clone()
                .into_value("worker", "public_url")
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "worker".to_string(),
                    reason: "Failed to resolve public_url from binding".to_string(),
                })?;
            return Ok(Some(url));
        }

        // If not in binding, try to fetch it from GCP
        let service_name = self
            .binding
            .service_name
            .clone()
            .into_value("worker", "service_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve service_name from binding".to_string(),
            })?;

        let location = self
            .binding
            .location
            .clone()
            .into_value("worker", "location")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "worker".to_string(),
                reason: "Failed to resolve location from binding".to_string(),
            })?;

        let url = self.build_url(&location, &service_name)?;
        let response = self
            .authed_request(Method::GET, url.clone())
            .await?
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: "GET".to_string(),
            })?;

        if response.status().as_u16() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Ok(None);
        }

        let service = response
            .json::<CloudRunService>()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.cloudRun".to_string(),
                reason: "Failed to parse Cloud Run service response".to_string(),
            })?;

        Ok(service.urls.into_iter().next())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudRunService {
    #[serde(default)]
    urls: Vec<String>,
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "worker.cloudRun".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "worker.cloudRun".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "worker.cloudRun".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "worker.cloudRun".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "worker.cloudRun".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "worker.cloudRun".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.cloudRun".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "worker.cloudRun".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "worker.cloudRun".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}
