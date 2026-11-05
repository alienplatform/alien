//! The gate's GCP calls: Cloud Run services and Secret Manager, with the
//! token of the service account the gate runs as.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};

use crate::cloud_run::ServiceStore;
use crate::error::{ErrorData, Result};
use crate::state::GateState;

const METADATA_TOKEN: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

/// Where the gate's token comes from.
#[derive(Debug, Clone)]
pub enum TokenSource {
    /// The metadata server of the Cloud Run instance the gate runs on.
    Metadata,
    /// A file holding an access token, for running the gate elsewhere.
    File(String),
}

/// GCP REST calls for one project and region.
#[derive(Debug, Clone)]
pub struct Gcp {
    http: reqwest::Client,
    tokens: TokenSource,
    /// Project id.
    pub project: String,
    /// Cloud Run region.
    pub region: String,
}

fn failed(operation: impl Into<String>) -> ErrorData {
    ErrorData::CloudCallFailed {
        operation: operation.into(),
    }
}

impl Gcp {
    /// A client for `project` and `region`.
    pub fn new(tokens: TokenSource, project: String, region: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            tokens,
            project,
            region,
        }
    }

    async fn token(&self) -> Result<String> {
        match &self.tokens {
            TokenSource::File(path) => std::fs::read_to_string(path)
                .map(|token| token.trim().to_string())
                .into_alien_error()
                .context(failed(format!("reading the token file {path}"))),
            TokenSource::Metadata => {
                let response: Value = self
                    .http
                    .get(METADATA_TOKEN)
                    .header("Metadata-Flavor", "Google")
                    .send()
                    .await
                    .into_alien_error()
                    .context(failed("metadata token"))?
                    .json()
                    .await
                    .into_alien_error()
                    .context(failed("metadata token"))?;
                response["access_token"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| AlienError::new(failed("metadata token has no access_token")))
            }
        }
    }

    /// Send a request; `Ok(None)` for 404.
    async fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
        operation: &str,
    ) -> Result<Option<Value>> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token().await?);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(AlienError::new(failed(format!(
                "{operation}: HTTP {status}: {}",
                text.chars().take(2000).collect::<String>()
            ))));
        }
        if text.trim().is_empty() {
            return Ok(Some(Value::Null));
        }
        serde_json::from_str(&text)
            .map(Some)
            .into_alien_error()
            .context(failed(format!("{operation}: the response is not JSON")))
    }

    fn service_url(&self, name: &str) -> String {
        format!(
            "https://run.googleapis.com/v2/projects/{}/locations/{}/services/{name}",
            self.project, self.region
        )
    }

    /// The latest version of a secret, or `None` if it has none.
    pub async fn secret(&self, secret: &str) -> Result<Option<String>> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{secret}/versions/latest:access",
            self.project
        );
        let Some(response) = self
            .call(reqwest::Method::GET, &url, None, &format!("secretmanager access {secret}"))
            .await?
        else {
            return Ok(None);
        };
        let data = response["payload"]["data"].as_str().unwrap_or_default();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .into_alien_error()
            .context(failed(format!("secret {secret} is not base64")))?;
        String::from_utf8(bytes)
            .map(Some)
            .into_alien_error()
            .context(failed(format!("secret {secret} is not UTF-8")))
    }

    /// Add a version to a secret.
    pub async fn add_secret_version(&self, secret: &str, value: &str) -> Result<()> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{secret}:addVersion",
            self.project
        );
        let body = json!({"payload": {
            "data": base64::engine::general_purpose::STANDARD.encode(value),
        }});
        self.call(
            reqwest::Method::POST,
            &url,
            Some(&body),
            &format!("secretmanager addVersion {secret}"),
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("secret {secret} does not exist"))))?;
        Ok(())
    }

    /// The gate's state from `secret`; empty when it has no version yet.
    pub async fn load_state(&self, secret: &str) -> Result<GateState> {
        match self.secret(secret).await? {
            None => Ok(GateState::default()),
            Some(text) => serde_json::from_str(&text).into_alien_error().context(
                ErrorData::ConfigurationInvalid {
                    message: format!("secret {secret} does not hold the gate's state"),
                },
            ),
        }
    }

    /// Store the gate's state as a new version of `secret`.
    pub async fn store_state(&self, secret: &str, state: &GateState) -> Result<()> {
        let value = serde_json::to_string(state).into_alien_error().context(
            ErrorData::ConfigurationInvalid {
                message: "the gate's state does not serialize".to_string(),
            },
        )?;
        self.add_secret_version(secret, &value).await
    }

    /// Deployer inputs from `secret`, a JSON object of strings.
    pub async fn deployer_inputs(&self, secret: &str) -> Result<BTreeMap<String, String>> {
        match self.secret(secret).await? {
            None => Ok(BTreeMap::new()),
            Some(text) => serde_json::from_str(&text).into_alien_error().context(
                ErrorData::ConfigurationInvalid {
                    message: format!("secret {secret} is not a JSON object of strings"),
                },
            ),
        }
    }
}

#[async_trait]
impl ServiceStore for Gcp {
    async fn get(&self, name: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &self.service_url(name),
            None,
            &format!("run.services.get {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("service {name} does not exist; setup creates it"),
            })
        })
    }

    async fn replace(&self, name: &str, service: &Value) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.service_url(name),
            Some(service),
            &format!("run.services.patch {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("service {name} does not exist; setup creates it"),
            })
        })?;
        Ok(())
    }
}
