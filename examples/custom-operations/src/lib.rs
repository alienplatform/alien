//! Custom operations backed by an application's HTTP admin API.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::time::Duration;

use alien_operations_sdk::{
    Arch, CanonicalPluginManifest, OperationDefinition, OperationFailure, Result, RiskTier,
    TypedOperations,
};
use reqwest::Client;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DoctorParams {}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DoctorOutput {
    status: String,
    max_exports_per_minute: NonZeroU32,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportThrottle {
    max_exports_per_minute: NonZeroU32,
}

/// Build the registry for a fixed application admin endpoint.
/// The URL belongs to plugin configuration, not caller params.
pub fn operations(base_url: &str) -> Result<TypedOperations> {
    let mut operations = TypedOperations::new();
    let doctor_url = format!("{}/health", base_url.trim_end_matches('/'));
    operations.register(
        OperationDefinition::new(
            "doctor",
            RiskTier::ReadOnly,
            "Read application health and export throttle.",
        )
        .with_no_permissions()
        .with_timeout_seconds(15),
        move |_params: DoctorParams| {
            let url = doctor_url.clone();
            async move {
                http_client()?
                    .get(url)
                    .send()
                    .await
                    .map_err(|_| request_failed())?
                    .error_for_status()
                    .map_err(|_| request_failed())?
                    .json::<DoctorOutput>()
                    .await
                    .map_err(|_| response_invalid())
            }
        },
    )?;
    let throttle_url = format!("{}/admin/export-throttle", base_url.trim_end_matches('/'));
    operations.register(
        OperationDefinition::new(
            "throttle-export-automation",
            RiskTier::Mutating,
            "Set the application's export limit per minute.",
        )
        .with_no_permissions()
        .with_timeout_seconds(15),
        move |params: ExportThrottle| {
            let url = throttle_url.clone();
            async move {
                http_client()?
                    .put(url)
                    .json(&params)
                    .send()
                    .await
                    .map_err(|_| request_failed())?
                    .error_for_status()
                    .map_err(|_| request_failed())?
                    .json::<ExportThrottle>()
                    .await
                    .map_err(|_| response_invalid())
            }
        },
    )?;
    Ok(operations)
}

fn http_client() -> std::result::Result<Client, OperationFailure> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| OperationFailure::new("CLIENT_UNAVAILABLE", "could not create HTTP client"))
}

fn request_failed() -> OperationFailure {
    OperationFailure::new(
        "APPLICATION_UNAVAILABLE",
        "application admin request failed",
    )
}

fn response_invalid() -> OperationFailure {
    OperationFailure::new(
        "APPLICATION_RESPONSE_INVALID",
        "application returned an invalid admin response",
    )
}

/// Generate bundle metadata without contacting the application.
pub fn plugin_manifest() -> Result<CanonicalPluginManifest> {
    let registry = operations("http://localhost")?;
    let manifest = CanonicalPluginManifest {
        name: "custom-ops".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        tier: RiskTier::Mutating,
        binaries: BTreeMap::from([
            (Arch::Amd64, "custom-ops-linux-amd64".to_string()),
            (Arch::Arm64, "custom-ops-linux-arm64".to_string()),
        ]),
        operations: registry.manifests(),
    };
    manifest.validate()?;
    Ok(manifest)
}
