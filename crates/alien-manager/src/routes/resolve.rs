//! `GET /v1/resolve?platform=<platform>` — manager + install-context discovery.
//!
//! Called by `alien-deploy` (CLI) before it talks to a manager, to learn (a)
//! which manager handles this platform and (b) the cross-account management
//! config the controllers need to assume roles. In the production SaaS this
//! is served by the platform API; standalone managers answer themselves.
//!
//! For standalone single-account setups there is no separate management
//! account — the deployment account *is* the managing account. So we
//! synthesize a `ManagementConfig` whose role ARN points at the local
//! account's `root`, matching the standalone fallback the cloud controllers
//! already implement when `aws_management` is absent.
//!
//! Account ID is read from the same env vars the manager uses for its own
//! AWS calls (`AWS_ACCOUNT_ID`); when unset the field is omitted and the
//! controller-side standalone fallback fills in `aws_cfg.account_id` at run
//! time.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Debug, Deserialize)]
struct ResolveQuery {
    platform: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveResponse {
    manager_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_context: Option<InstallContext>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallContext {
    management_config: ManagementConfigEnvelope,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "platform")]
enum ManagementConfigEnvelope {
    Aws(AwsManagement),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AwsManagement {
    managing_role_arn: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/resolve", axum::routing::get(resolve))
}

async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ResolveQuery>,
) -> Response {
    // Prefer the Host the caller actually used to reach us — that's the URL
    // we know is routable from the caller's perspective. Falls back to the
    // configured base URL. Critical for local dev on macOS where
    // `localhost` resolves to `::1` while the manager binds to 127.0.0.1.
    let manager_url = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .filter(|h| !h.is_empty())
        .map(|host| format!("http://{host}"))
        .unwrap_or_else(|| state.config.base_url());
    let platform = params.platform.as_deref().unwrap_or("");

    // Standalone single-account managers don't need a separate management
    // identity — the deployment account *is* the managing account, and the
    // cloud controllers already have a standalone fallback (see the
    // `aws_management = None` branch in compute_cluster/aws.rs). Returning
    // `install_context: None` here also keeps the `RemoteStackManagement`
    // preflight mutation from firing (it only runs when
    // `config.management_config` is present), avoiding a no-op cross-account
    // IAM policy that fails with duplicate SIDs in this single-account setup.
    let _ = platform;
    let install_context: Option<InstallContext> = None;

    Json(ResolveResponse {
        manager_url,
        install_context,
    })
    .into_response()
}
