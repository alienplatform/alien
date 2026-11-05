//! The gate's Azure calls: Container Apps through Azure Resource Manager and
//! secrets in Key Vault, with the token of the managed identity the gate
//! runs as.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::container_apps::AppStore;
use crate::error::{ErrorData, Result};
use crate::state::GateState;

const ARM: &str = "https://management.azure.com";
const CONTAINER_APPS_API: &str = "2024-03-01";
const KEY_VAULT_API: &str = "7.4";

fn failed(operation: impl Into<String>) -> ErrorData {
    ErrorData::CloudCallFailed {
        operation: operation.into(),
    }
}

/// Azure REST calls for one resource group and Key Vault.
#[derive(Debug, Clone)]
pub struct Azure {
    http: reqwest::Client,
    /// `/subscriptions/<id>/resourceGroups/<name>`, where the apps live.
    pub resource_group: String,
    /// `https://<vault>.vault.azure.net`.
    pub vault: String,
    /// Client id of the gate's user-assigned identity, if it has several.
    pub client_id: Option<String>,
}

impl Azure {
    /// A client for the apps in `resource_group` and secrets in `vault`.
    pub fn new(resource_group: String, vault: String, client_id: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            resource_group,
            vault: vault.trim_end_matches('/').to_string(),
            client_id,
        }
    }

    /// A token for `resource` from the Container Apps identity endpoint.
    async fn token(&self, resource: &str) -> Result<String> {
        let endpoint = std::env::var("IDENTITY_ENDPOINT")
            .map_err(|_| AlienError::new(failed("IDENTITY_ENDPOINT is not set")))?;
        let header = std::env::var("IDENTITY_HEADER")
            .map_err(|_| AlienError::new(failed("IDENTITY_HEADER is not set")))?;
        let mut query = vec![("resource", resource), ("api-version", "2019-08-01")];
        if let Some(client_id) = &self.client_id {
            query.push(("client_id", client_id));
        }
        let response: Value = self
            .http
            .get(&endpoint)
            .query(&query)
            .header("X-IDENTITY-HEADER", header)
            .send()
            .await
            .into_alien_error()
            .context(failed("managed identity token"))?
            .json()
            .await
            .into_alien_error()
            .context(failed("managed identity token"))?;
        response["access_token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AlienError::new(failed("the identity endpoint returned no token")))
    }

    /// Send a request; `Ok(None)` for 404.
    async fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        resource: &str,
        body: Option<&Value>,
        operation: &str,
    ) -> Result<Option<Value>> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token(resource).await?);
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

    fn app_url(&self, name: &str) -> String {
        format!(
            "{ARM}{}/providers/Microsoft.App/containerApps/{name}?api-version={CONTAINER_APPS_API}",
            self.resource_group
        )
    }

    /// The value of a Key Vault secret, or `None` if it doesn't exist.
    pub async fn secret(&self, name: &str) -> Result<Option<String>> {
        let url = format!("{}/secrets/{name}?api-version={KEY_VAULT_API}", self.vault);
        Ok(self
            .call(
                reqwest::Method::GET,
                &url,
                "https://vault.azure.net",
                None,
                &format!("keyvault get {name}"),
            )
            .await?
            .and_then(|secret| secret["value"].as_str().map(str::to_string)))
    }

    /// Set a Key Vault secret.
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        let url = format!("{}/secrets/{name}?api-version={KEY_VAULT_API}", self.vault);
        self.call(
            reqwest::Method::PUT,
            &url,
            "https://vault.azure.net",
            Some(&json!({"value": value})),
            &format!("keyvault set {name}"),
        )
        .await?;
        Ok(())
    }

    /// The gate's state from `secret`; empty when it doesn't exist yet.
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

    /// Store the gate's state in `secret`.
    pub async fn store_state(&self, secret: &str, state: &GateState) -> Result<()> {
        let value = serde_json::to_string(state).into_alien_error().context(
            ErrorData::ConfigurationInvalid {
                message: "the gate's state does not serialize".to_string(),
            },
        )?;
        self.set_secret(secret, &value).await
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
impl AppStore for Azure {
    async fn get(&self, name: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &self.app_url(name),
            ARM,
            None,
            &format!("containerApps get {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("container app {name} does not exist; setup creates it"),
            })
        })
    }

    async fn patch(&self, name: &str, app: &Value) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.app_url(name),
            ARM,
            Some(app),
            &format!("containerApps patch {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("container app {name} does not exist; setup creates it"),
            })
        })?;
        Ok(())
    }
}
