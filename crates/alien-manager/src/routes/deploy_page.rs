//! Deploy page — serves an embedded HTML page for deployment onboarding.
//!
//! `GET /deploy` returns a self-contained HTML page that:
//! - Reads the deployment group token from the URL fragment
//! - Shows platform selection (AWS, GCP, Azure, Kubernetes, Local)
//! - Displays the appropriate install + deploy commands

use axum::{
    response::{Html, IntoResponse},
    Router,
};

use super::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/deploy", axum::routing::get(deploy_page))
}

async fn deploy_page() -> impl IntoResponse {
    Html(include_str!("deploy_page.html"))
}
