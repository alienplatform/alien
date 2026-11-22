//! Client configuration for the live cloud tests, taken from the environment without any secret.
//!
//! `identity` is `MANAGEMENT` or `TARGET`. Subscription, tenant and client id come from
//! `.env.test` (`AZURE_<IDENTITY>_SUBSCRIPTION_ID`, `_TENANT_ID`, `_CLIENT_ID`).
//!
//! - In CI, `AZURE_<IDENTITY>_FEDERATED_TOKEN_FILE` holds a GitHub OIDC token, kept fresh by the
//!   job, that the app registration trusts. The client exchanges it for each token it needs.
//! - On a laptop, with no such file, `az login` provides the tokens (through the `az` CLI), for
//!   the Azure management, storage, Key Vault and Service Bus scopes.

use crate::{AzureClientConfig, AzureCredentials};
use std::collections::HashMap;

const SCOPES: [&str; 4] = [
    "https://management.azure.com/.default",
    "https://storage.azure.com/.default",
    "https://vault.azure.net/.default",
    "https://servicebus.azure.net/.default",
];

/// Builds the client configuration for a live test identity; panics with a clear message when
/// the environment is incomplete, as tests do.
pub fn live_test_config(identity: &str) -> AzureClientConfig {
    let id = identity.to_uppercase();
    let var = |suffix: &str| {
        std::env::var(format!("AZURE_{id}_{suffix}"))
            .unwrap_or_else(|_| panic!("AZURE_{id}_{suffix} must be set (see .env.test)"))
    };
    let subscription_id = var("SUBSCRIPTION_ID");
    let tenant_id = var("TENANT_ID");

    let credentials = match std::env::var(format!("AZURE_{id}_FEDERATED_TOKEN_FILE")) {
        Ok(federated_token_file) => AzureCredentials::WorkloadIdentity {
            client_id: var("CLIENT_ID"),
            tenant_id: tenant_id.clone(),
            federated_token_file,
            authority_host: "https://login.microsoftonline.com".to_string(),
        },
        Err(_) => AzureCredentials::ScopedAccessTokens {
            tokens: az_cli_tokens(&tenant_id),
        },
    };

    AzureClientConfig {
        subscription_id,
        tenant_id,
        region: Some(
            std::env::var(format!("AZURE_{id}_REGION")).unwrap_or_else(|_| "eastus".to_string()),
        ),
        credentials,
        service_overrides: None,
    }
}

fn az_cli_tokens(tenant_id: &str) -> HashMap<String, String> {
    SCOPES
        .iter()
        .map(|scope| {
            let output = std::process::Command::new("az")
                .args(["account", "get-access-token", "--tenant", tenant_id, "--scope", scope])
                .args(["--query", "accessToken", "--output", "tsv"])
                .output()
                .unwrap_or_else(|e| {
                    panic!(
                        "set AZURE_<IDENTITY>_FEDERATED_TOKEN_FILE (CI) or install the az CLI and run `az login`: {e}"
                    )
                });
            assert!(
                output.status.success(),
                "`az account get-access-token` failed for {scope}; run `az login`: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            (
                scope.to_string(),
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            )
        })
        .collect()
}
