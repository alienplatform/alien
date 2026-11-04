//! The control plane's signing routes. The server stores and forwards what
//! the CLI sends; nothing it returns is trusted until it verifies here.

use alien_error::{AlienError, Context, IntoAlienError};
use alien_release_trust::{Signature, SignedStatement, Signer};
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// What a pending change asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingKind {
    /// A new device asks to be added to the signer list.
    Device,
    /// A statement prepared elsewhere waits for signatures.
    Statement,
}

/// Where a pending change stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingStatus {
    /// Waiting for a signer.
    Pending,
    /// Signed and applied.
    Approved,
    /// Turned down.
    Rejected,
}

/// A change the dashboard or a teammate prepared, waiting for a signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChange {
    /// Change id, for `alien approve <id>`.
    pub id: String,
    /// Project.
    pub project_id: String,
    /// What the change is.
    pub kind: PendingKind,
    /// Where it stands.
    pub status: PendingStatus,
    /// One line for lists. Display only: the CLI prints the statement itself
    /// before signing.
    pub summary: String,
    /// Deployment the change is for, if one.
    #[serde(default)]
    pub deployment_id: Option<String>,
    /// For device changes: the signer asking to be added.
    #[serde(default)]
    pub device: Option<Signer>,
    /// For statement changes: base64 of the payload bytes to sign.
    #[serde(default)]
    pub payload: Option<String>,
    /// Signatures gathered so far, for statements that need several.
    #[serde(default)]
    pub signatures: Vec<Signature>,
}

/// A new pending change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewPendingChange {
    /// What the change is.
    pub kind: PendingKind,
    /// One line for lists.
    pub summary: String,
    /// Deployment the change is for, if one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    /// For device changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Signer>,
    /// For statement changes: base64 payload bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Signatures already made.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<Signature>,
}

/// How a pending change is resolved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    /// Signatures to add to the change.
    pub signatures: Vec<Signature>,
    /// The signed statement, once it has every signature it needs. For a
    /// device change, the next signer list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<SignedStatement>,
}

#[derive(Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

#[derive(Serialize)]
struct StatementBody<'a> {
    statement: &'a SignedStatement,
}

/// Client for the signing routes of a manager or the platform API.
pub struct SigningApi {
    client: Client,
    base_url: String,
}

impl SigningApi {
    /// A client sending requests with `client`'s default headers to `base_url`.
    pub fn new(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// From the execution context's authenticated client.
    pub async fn from_context(ctx: &crate::execution_context::ExecutionMode) -> Result<Self> {
        let http = ctx.auth_http().await?;
        Ok(Self::new(http.client.clone(), http.base_url.clone()))
    }

    /// Every signer list version the server holds, oldest first.
    pub async fn signer_lists(&self, project: &str) -> Result<Vec<SignedStatement>> {
        let items: Items<SignedStatement> = self
            .send(
                Method::GET,
                &format!("/v1/projects/{project}/signer-lists"),
                None::<&()>,
            )
            .await?;
        Ok(items.items)
    }

    /// Store the next signer list version.
    pub async fn publish_signer_list(
        &self,
        project: &str,
        statement: &SignedStatement,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::POST,
                &format!("/v1/projects/{project}/signer-lists"),
                Some(&StatementBody { statement }),
            )
            .await?;
        Ok(())
    }

    /// Create a pending change.
    pub async fn create_pending(
        &self,
        project: &str,
        change: &NewPendingChange,
    ) -> Result<PendingChange> {
        self.send(
            Method::POST,
            &format!("/v1/projects/{project}/pending-changes"),
            Some(change),
        )
        .await
    }

    /// Pending changes of a project.
    pub async fn pending(&self, project: &str) -> Result<Vec<PendingChange>> {
        let items: Items<PendingChange> = self
            .send(
                Method::GET,
                &format!("/v1/projects/{project}/pending-changes"),
                None::<&()>,
            )
            .await?;
        Ok(items.items)
    }

    /// One pending change.
    pub async fn pending_change(&self, id: &str) -> Result<PendingChange> {
        self.send(
            Method::GET,
            &format!("/v1/pending-changes/{id}"),
            None::<&()>,
        )
        .await
    }

    /// Add signatures to a pending change, or resolve it with its statement.
    pub async fn resolve(&self, id: &str, resolution: &Resolution) -> Result<PendingChange> {
        self.send(
            Method::POST,
            &format!("/v1/pending-changes/{id}/signatures"),
            Some(resolution),
        )
        .await
    }

    /// Store a signed config statement for a deployment.
    pub async fn publish_config(
        &self,
        deployment: &str,
        statement: &SignedStatement,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::POST,
                &format!("/v1/deployments/{deployment}/config-statements"),
                Some(&StatementBody { statement }),
            )
            .await?;
        Ok(())
    }

    async fn send<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let mut request = self.client.request(method.clone(), &url);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response =
            request
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("{method} {path} could not be sent"),
                    url: Some(url.clone()),
                })?;
        let status = response.status();
        let text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("{method} {path}: the response could not be read"),
                    url: Some(url.clone()),
                })?;
        if !status.is_success() {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("{method} {path} returned {status}: {}", text.trim()),
                url: Some(url),
            }));
        }
        serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("{method} {path}: the response is not the expected JSON"),
                url: Some(url),
            })
    }
}
