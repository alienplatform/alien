//! Select one explicit runtime identity source, without credential fallback.

use std::{collections::HashMap, sync::Arc};

use alien_client_core::{
    runtime_credentials::{RuntimeCredentialConfig, RuntimeCredentials, RuntimeProofProvider},
    runtime_proofs::{GcpMetadataProof, SessionFileProof},
};
use alien_error::{AlienError, Context};
use serde_json::json;

use crate::error::{ErrorData, Result};

pub async fn telemetry_credentials(
    env: &HashMap<String, String>,
) -> Result<Option<Arc<RuntimeCredentials>>> {
    let Some(kind) = env.get("ALIEN_IDENTITY_PROVIDER") else {
        return Ok(None);
    };
    if kind == "capabilityFile" {
        let credentials = RuntimeCredentials::from_capability_file(
            RuntimeCredentialConfig {
                manager_url: required(env, "ALIEN_MANAGER_URL")?.into(),
                audience: required(env, "ALIEN_MANAGER_AUDIENCE")?.into(),
                deployment_id: required(env, "ALIEN_DEPLOYMENT_ID")?.into(),
                purpose: json!({"type":"telemetry"}),
            },
            required(env, "ALIEN_TELEMETRY_CAPABILITY_FILE")?.into(),
        )
        .context(configuration("invalid telemetry capability file"))?;
        credentials
            .token()
            .await
            .context(configuration("telemetry capability is unavailable"))?;
        return Ok(Some(Arc::new(credentials)));
    }
    let proof: Arc<dyn RuntimeProofProvider> = match kind.as_str() {
        "gcp" => Arc::new(GcpMetadataProof),
        "session" => Arc::new(SessionFileProof(
            required(env, "ALIEN_SESSION_FILE")?.into(),
        )),
        #[cfg(feature = "aws")]
        "aws" => {
            use alien_aws_clients::{
                runtime_proof_signer::AwsRuntimeProof, AwsClientConfig, AwsClientConfigExt,
                AwsCredentialProvider,
            };
            let config = AwsClientConfig::from_env(env)
                .await
                .context(configuration("cannot resolve cloud identity"))?;
            let region = config.region.clone();
            let provider = AwsCredentialProvider::from_config(config)
                .await
                .context(configuration("cannot initialize cloud identity"))?;
            Arc::new(
                AwsRuntimeProof::new(provider, region)
                    .context(configuration("invalid cloud proof configuration"))?,
            )
        }
        _ => {
            return Err(AlienError::new(configuration(
                "unsupported runtime identity provider",
            )))
        }
    };
    let credentials = RuntimeCredentials::new(
        RuntimeCredentialConfig {
            manager_url: required(env, "ALIEN_MANAGER_URL")?.into(),
            audience: required(env, "ALIEN_MANAGER_AUDIENCE")?.into(),
            deployment_id: required(env, "ALIEN_DEPLOYMENT_ID")?.into(),
            purpose: json!({"type":"telemetry"}),
        },
        proof,
    )
    .context(configuration("invalid runtime exchange configuration"))?;
    // Fail startup if the explicitly selected identity cannot establish authority.
    credentials
        .token()
        .await
        .context(configuration("runtime identity exchange failed"))?;
    Ok(Some(Arc::new(credentials)))
}

fn required<'a>(env: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    env.get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: "Runtime identity configuration is missing".into(),
                field: Some(key.into()),
            })
        })
}
fn configuration(message: &str) -> ErrorData {
    ErrorData::ConfigurationInvalid {
        message: message.into(),
        field: Some("ALIEN_IDENTITY_PROVIDER".into()),
    }
}
