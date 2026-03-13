use crate::gcp::gcp_request_utils::{auth_send_json, auth_send_no_response, GcpAuthConfig};
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};
use alien_error::AlienError;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

pub trait GcpServiceConfig: Send + Sync + std::fmt::Debug {
    fn base_url(&self) -> &'static str;
    fn default_audience(&self) -> &'static str;
    fn service_name(&self) -> &'static str;
    fn service_key(&self) -> &'static str;
}

/// Lightweight client that relies on the cloud-agnostic helpers defined in
/// `gcp_request_utils.rs` and `request_utils.rs`.
#[derive(Debug)]
pub struct GcpClientBase {
    http: Client,
    cfg: GcpClientConfig,
    svc_cfg: Box<dyn GcpServiceConfig>,
}

impl GcpClientBase {
    pub fn new(http: Client, cfg: GcpClientConfig, svc_cfg: Box<dyn GcpServiceConfig>) -> Self {
        Self { http, cfg, svc_cfg }
    }

    pub fn http_client(&self) -> &Client {
        &self.http
    }

    pub fn config(&self) -> &GcpClientConfig {
        &self.cfg
    }

    /// Helper to obtain an auth config (Bearer token) for the current service.
    pub(crate) async fn auth(&self) -> Result<GcpAuthConfig> {
        let token = self
            .cfg
            .get_bearer_token(self.svc_cfg.default_audience())
            .await?;
        Ok(GcpAuthConfig {
            bearer_token: token,
        })
    }

    /// Get the base URL for the service, checking for overrides first
    fn get_base_url(&self) -> &str {
        if let Some(override_url) = self
            .cfg
            .get_service_endpoint_option(self.svc_cfg.service_key())
        {
            override_url
        } else {
            self.svc_cfg.base_url()
        }
    }

    /// Build the full URL combining the service base URL, path and optional query parameters.
    fn build_url(&self, path: &str, query_params: Option<&Vec<(&str, String)>>) -> Result<Url> {
        // Start from the base URL (with override support).
        let base_url = self.get_base_url();
        let mut url = Url::parse(base_url).map_err(|e| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: format!(
                    "Invalid base URL '{}' for service {}: {}",
                    base_url,
                    self.svc_cfg.service_name(),
                    e
                ),
                errors: None,
            })
        })?;

        // Append path segments (avoid duplicating slashes).
        let mut joined_path = url.path().trim_end_matches('/').to_string();
        if !joined_path.is_empty() {
            joined_path.push('/');
        }
        joined_path.push_str(path.trim_start_matches('/'));
        url.set_path(&joined_path);

        // Append query parameters if provided.
        if let Some(params) = query_params {
            if !params.is_empty() {
                let mut qp = url.query_pairs_mut();
                for (k, v) in params {
                    qp.append_pair(k, v);
                }
            }
        }
        Ok(url)
    }

    /// Generic helper mirroring the original signature that deserialises a JSON body.
    pub async fn execute_request<T, B>(
        &self,
        method: Method,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
        body: Option<B>,
        resource_name: &str,
    ) -> Result<T>
    where
        T: DeserializeOwned + Send + 'static,
        B: Serialize + Send + Sync + Clone + 'static,
    {
        let url = self.build_url(path, query_params.as_ref())?;
        let mut builder = self.http.request(method.clone(), url);

        if let Some(b) = body.as_ref() {
            builder = builder.json(b);
        } else if method == Method::POST {
            // For POST requests with no body, explicitly set Content-Length to 0
            // This is required by HTTP/1.1 specification to avoid 411 errors
            builder = builder.header(reqwest::header::CONTENT_LENGTH, "0");
        }

        let operation = format!("{} {}", method, path);
        auth_send_json(
            builder,
            &self.auth().await?,
            &operation,
            resource_name,
            self.svc_cfg.service_name(),
        )
        .await
    }

    /// Variant for requests that do not return a body (HTTP 2xx with empty body).
    pub async fn execute_request_no_response<B>(
        &self,
        method: Method,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
        body: Option<B>,
        resource_name: &str,
    ) -> Result<()>
    where
        B: Serialize + Send + Sync + Clone + 'static,
    {
        let url = self.build_url(path, query_params.as_ref())?;
        let mut builder = self.http.request(method.clone(), url);

        if let Some(b) = body.as_ref() {
            builder = builder.json(b);
        } else if method == Method::POST {
            // For POST requests with no body, explicitly set Content-Length to 0
            // This is required by HTTP/1.1 specification to avoid 411 errors
            builder = builder.header(reqwest::header::CONTENT_LENGTH, "0");
        }

        let operation = format!("{} {}", method, path);
        auth_send_no_response(
            builder,
            &self.auth().await?,
            &operation,
            resource_name,
            self.svc_cfg.service_name(),
        )
        .await
    }
}
