//! The control plane's signing routes, the same on a manager and on the
//! platform API. The server stores and forwards what the CLI sends; nothing
//! it returns is trusted until it verifies here.

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{SignedStatement, SignerListBody};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Where a pending change stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingStatus {
    /// Waiting for an admin.
    Pending,
    /// Carried by a signer list version.
    Approved,
    /// Turned down.
    Rejected,
}

/// The body of the next signer list, prepared for an admin to review and
/// sign: a device request, a removal or a refresh.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChange {
    /// Change id, for `alien approve <id>`; the pairing code covers it.
    pub id: String,
    /// Where it stands.
    pub status: PendingStatus,
    /// One line for lists. Display only.
    pub summary: String,
    /// Signer list version the body was prepared from.
    pub based_on_version: u64,
    /// The next list's body. The admin's CLI shows it as a diff against the
    /// list it verified, and writes the header itself.
    pub body: SignerListBody,
    /// Who filed it. Display only.
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewPendingChange<'a> {
    summary: &'a str,
    based_on_version: u64,
    body: &'a SignerListBody,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Resolution {
    status: PendingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    signer_list_version: Option<u64>,
}

#[derive(Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

#[derive(Serialize)]
struct StatementBody<'a> {
    statement: &'a SignedStatement,
}

/// What storing a signer list version answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPublished {
    /// Stored, or the same bytes were already stored.
    Stored,
    /// Someone else turned signing on first: another version 1 is stored.
    AlreadyOn {
        /// Who turned it on.
        enabled_by: String,
    },
}

/// Client for the signing routes of a manager or the platform API.
pub struct SigningApi {
    client: Client,
    base_url: String,
    /// `?workspace=` for every request: the platform API answers a user's
    /// own credentials only within a named workspace.
    workspace: Option<String>,
}

impl SigningApi {
    /// A client sending requests with `client`'s default headers to `base_url`.
    pub fn new(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            workspace: None,
        }
    }

    /// From the execution context's authenticated client, scoped to its
    /// workspace on the platform API (a manager serves one project and
    /// takes none).
    pub async fn from_context(ctx: &crate::execution_context::ExecutionMode) -> Result<Self> {
        let http = ctx.auth_http().await?;
        Ok(Self {
            workspace: workspace_scope(ctx).await?,
            ..Self::new(http.client.clone(), http.base_url.clone())
        })
    }

    /// What the control plane renders setup templates with
    /// (`setupHosting` on `GET /v1/projects/{project}/signing`). A release
    /// signs the digests of templates rendered with it; without it there is
    /// nothing to sign against, so its absence is an error.
    pub async fn setup_hosting(
        &self,
        project: &str,
    ) -> Result<alien_cli_common::setup_templates::SetupHostingOptions> {
        let mut summary: serde_json::Value = self
            .send(
                Method::GET,
                &format!("/v1/projects/{project}/signing"),
                None::<&()>,
            )
            .await?;
        let hosting = summary["setupHosting"].take();
        if hosting.is_null() {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "the control plane doesn't say how it hosts setup templates for {project} (setupHosting)"
                ),
            }));
        }
        serde_json::from_value(hosting)
            .into_alien_error()
            .context(ErrorData::SigningFailed {
                message: format!("setupHosting for {project} doesn't parse"),
            })
    }

    /// Every signer list version the server holds.
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

    /// Every signer list version the server holds, or `None` when the server
    /// has no signer list route at all: a control plane or manager from before
    /// signed releases, which can't have protected installs either.
    pub async fn signer_lists_if_supported(
        &self,
        project: &str,
    ) -> Result<Option<Vec<SignedStatement>>> {
        let path = format!("/v1/projects/{project}/signer-lists");
        let (status, text) = self.request(Method::GET, &path, None::<&()>).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(self.failed(&Method::GET, &path, status, &text));
        }
        let items: Items<SignedStatement> = serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("GET {path} answered with a body that doesn't parse"),
                url: Some(format!("{}{path}", self.base_url)),
            })?;
        Ok(Some(items.items))
    }

    /// Store a signer list version. A second version 1 for the project is
    /// answered with who turned signing on.
    pub async fn publish_signer_list(
        &self,
        project: &str,
        statement: &SignedStatement,
    ) -> Result<ListPublished> {
        let path = format!("/v1/projects/{project}/signer-lists");
        let (status, text) = self
            .request(Method::POST, &path, Some(&StatementBody { statement }))
            .await?;
        if status.is_success() {
            return Ok(ListPublished::Stored);
        }
        if status == StatusCode::CONFLICT {
            if let Ok(error) = serde_json::from_str::<ErrorBody>(&text) {
                if error.code == "SIGNING_ALREADY_ON" {
                    let enabled_by = error
                        .context
                        .and_then(|context| context.enabled_by)
                        .unwrap_or_else(|| "another admin".to_string());
                    return Ok(ListPublished::AlreadyOn { enabled_by });
                }
            }
        }
        Err(self.failed(&Method::POST, &path, status, &text))
    }

    /// File a pending change.
    pub async fn create_pending(
        &self,
        project: &str,
        summary: &str,
        based_on_version: u64,
        body: &SignerListBody,
    ) -> Result<PendingChange> {
        self.send(
            Method::POST,
            &format!("/v1/projects/{project}/pending-changes"),
            Some(&NewPendingChange {
                summary,
                based_on_version,
                body,
            }),
        )
        .await
    }

    /// The changes of a project still waiting for an admin.
    pub async fn pending(&self, project: &str) -> Result<Vec<PendingChange>> {
        let items: Items<PendingChange> = self
            .send(
                Method::GET,
                &format!("/v1/projects/{project}/pending-changes?status=pending"),
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

    /// Mark a change approved by signer list `version`.
    pub async fn approve(&self, id: &str, version: u64) -> Result<PendingChange> {
        self.resolve(
            id,
            &Resolution {
                status: PendingStatus::Approved,
                signer_list_version: Some(version),
            },
        )
        .await
    }

    /// Turn a change down.
    pub async fn reject(&self, id: &str) -> Result<PendingChange> {
        self.resolve(
            id,
            &Resolution {
                status: PendingStatus::Rejected,
                signer_list_version: None,
            },
        )
        .await
    }

    async fn resolve(&self, id: &str, resolution: &Resolution) -> Result<PendingChange> {
        self.send(
            Method::POST,
            &format!("/v1/pending-changes/{id}/resolution"),
            Some(resolution),
        )
        .await
    }

    /// Every config statement stored for a deployment, lowest sequence
    /// first. Untrusted: the caller verifies what it uses.
    pub async fn config_statements(&self, deployment: &str) -> Result<Vec<SignedStatement>> {
        let items: Items<SignedStatement> = self
            .send(
                Method::GET,
                &format!("/v1/deployments/{deployment}/config-statements"),
                None::<&()>,
            )
            .await?;
        Ok(items.items)
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

    async fn request<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(StatusCode, String)> {
        let url = format!("{}{path}", self.base_url);
        let mut request = self.client.request(method.clone(), &url);
        if let Some(workspace) = &self.workspace {
            request = request.query(&[("workspace", workspace)]);
        }
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
                    url: Some(url),
                })?;
        Ok((status, text))
    }

    fn failed(
        &self,
        method: &Method,
        path: &str,
        status: StatusCode,
        text: &str,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("{method} {path} returned {status}: {}", text.trim()),
            url: Some(format!("{}{path}", self.base_url)),
        })
    }

    async fn send<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T> {
        let (status, text) = self.request(method.clone(), path, body).await?;
        if !status.is_success() {
            return Err(self.failed(&method, path, status, &text));
        }
        // Writes that store something may answer with an empty body.
        let text = if text.trim().is_empty() {
            "null"
        } else {
            text.as_str()
        };
        serde_json::from_str(text)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("{method} {path}: the response is not the expected JSON"),
                url: Some(format!("{}{path}", self.base_url)),
            })
    }
}

/// The workspace the platform API scopes requests to; none on a manager.
#[cfg(feature = "platform")]
async fn workspace_scope(ctx: &crate::execution_context::ExecutionMode) -> Result<Option<String>> {
    if !ctx.is_platform() {
        return Ok(None);
    }
    ctx.resolve_workspace_query_with_bootstrap(false).await
}

#[cfg(not(feature = "platform"))]
async fn workspace_scope(_ctx: &crate::execution_context::ExecutionMode) -> Result<Option<String>> {
    Ok(None)
}

/// Query parameters that scope a listing to the project on the platform API;
/// a manager serves one project and needs none.
pub async fn project_query(
    ctx: &crate::execution_context::ExecutionMode,
    project_id: &str,
) -> Result<Vec<(String, String)>> {
    if !ctx.is_platform() {
        return Ok(Vec::new());
    }
    let mut query = vec![("project".to_string(), project_id.to_string())];
    #[cfg(feature = "platform")]
    if let Some(workspace) = ctx.resolve_workspace_query_with_bootstrap(false).await? {
        query.push(("workspace".to_string(), workspace));
    }
    Ok(query)
}

/// GET `path` on the control plane as JSON.
pub async fn get_json(
    ctx: &crate::execution_context::ExecutionMode,
    path: &str,
    query: &[(String, String)],
) -> Result<serde_json::Value> {
    let http = ctx.auth_http().await?;
    let url = format!("{}{path}", http.base_url.trim_end_matches('/'));
    let response = http
        .client
        .get(&url)
        .query(query)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("GET {path} could not be sent"),
            url: Some(url.clone()),
        })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("GET {path}: the response could not be read"),
            url: Some(url.clone()),
        })?;
    if !status.is_success() {
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("GET {path} returned {status}: {}", text.trim()),
            url: Some(url),
        }));
    }
    serde_json::from_str(&text)
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("GET {path}: the response is not JSON"),
            url: Some(url),
        })
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    context: Option<AlreadyOnContext>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlreadyOnContext {
    #[serde(default)]
    enabled_by: Option<String>,
}

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;
    use crate::execution_context::ExecutionMode;

    /// The platform API answers signing routes for a user's credentials only
    /// within a workspace, so every request carries the one the CLI uses.
    #[tokio::test]
    async fn signing_requests_on_the_platform_name_the_workspace() {
        let server = httpmock::MockServer::start_async().await;
        let lists = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/projects/shop/signer-lists")
                    .query_param("workspace", "acme");
                then.status(200).body(r#"{"items":[]}"#);
            })
            .await;
        let pending = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/projects/shop/pending-changes")
                    .query_param("status", "pending")
                    .query_param("workspace", "acme");
                then.status(200).body(r#"{"items":[]}"#);
            })
            .await;
        let ctx = ExecutionMode::Platform {
            base_url: server.base_url(),
            api_key: Some("ax_test".to_string()),
            no_browser: true,
            workspace: Some("acme".to_string()),
            project: None,
        };

        let api = SigningApi::from_context(&ctx).await.unwrap();
        assert!(api.signer_lists("shop").await.unwrap().is_empty());
        assert!(api.pending("shop").await.unwrap().is_empty());

        lists.assert_async().await;
        pending.assert_async().await;
    }

    /// A manager serves one project and takes no workspace.
    #[tokio::test]
    async fn signing_requests_on_a_manager_name_no_workspace() {
        let server = httpmock::MockServer::start_async().await;
        let lists = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/projects/shop/signer-lists")
                    .matches(|request| {
                        !request.query_params.as_ref().is_some_and(|params| {
                            params.iter().any(|(name, _)| name == "workspace")
                        })
                    });
                then.status(200).body(r#"{"items":[]}"#);
            })
            .await;
        let ctx = ExecutionMode::Standalone {
            server_url: server.base_url(),
            api_key: "ax_test".to_string(),
        };

        let api = SigningApi::from_context(&ctx).await.unwrap();
        assert!(api.signer_lists("shop").await.unwrap().is_empty());
        lists.assert_async().await;
    }
}
