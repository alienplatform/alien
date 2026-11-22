//! Credentials for the live cloud tests, taken from the environment without any stored key.
//!
//! `identity` is `MANAGEMENT` or `TARGET`. The project, region and service account come from
//! `.env.test` (`GOOGLE_<IDENTITY>_PROJECT_ID`, `_REGION`, `_SERVICE_ACCOUNT_EMAIL`).
//!
//! - In CI, `GOOGLE_<IDENTITY>_CREDENTIALS_FILE` points at a workload identity federation
//!   credential configuration that impersonates the service account.
//! - On a laptop, with no such file, the user's Application Default Credentials
//!   (`gcloud auth application-default login`) impersonate the service account.

use crate::{GcpClientConfig, GcpClientConfigExt, GcpImpersonationConfig};
use std::collections::HashMap;

/// Builds the client configuration for a live test identity; panics with a clear message when
/// the environment is incomplete, as tests do.
pub async fn live_test_config(identity: &str) -> GcpClientConfig {
    let id = identity.to_uppercase();
    let var = |suffix: &str| std::env::var(format!("GOOGLE_{id}_{suffix}")).ok();
    let project_id = var("PROJECT_ID")
        .unwrap_or_else(|| panic!("GOOGLE_{id}_PROJECT_ID must be set (see .env.test)"));
    let region = var("REGION").unwrap_or_else(|| "us-central1".to_string());

    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.insert("GCP_PROJECT_ID".to_string(), project_id);
    env.insert("GCP_REGION".to_string(), region);

    if let Some(file) = var("CREDENTIALS_FILE") {
        env.insert("GOOGLE_APPLICATION_CREDENTIALS".to_string(), file);
        return GcpClientConfig::from_env(&env)
            .await
            .unwrap_or_else(|e| panic!("GOOGLE_{id}_CREDENTIALS_FILE is not usable: {e}"));
    }

    // Another identity's file must not leak in through the default variable.
    env.remove("GOOGLE_APPLICATION_CREDENTIALS");
    let email = var("SERVICE_ACCOUNT_EMAIL").unwrap_or_else(|| {
        panic!(
            "set GOOGLE_{id}_CREDENTIALS_FILE (CI) or run `gcloud auth application-default login` \
             and set GOOGLE_{id}_SERVICE_ACCOUNT_EMAIL (laptop)"
        )
    });
    let source = GcpClientConfig::from_env(&env).await.unwrap_or_else(|e| {
        panic!("no Application Default Credentials; run `gcloud auth application-default login`: {e}")
    });
    source
        .impersonate(GcpImpersonationConfig {
            service_account_email: email,
            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
            ..Default::default()
        })
        .await
        .unwrap_or_else(|e| panic!("cannot impersonate the {id} service account: {e}"))
}
