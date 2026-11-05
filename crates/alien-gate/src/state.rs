//! What the gate keeps between calls: the signer chain it accepted and the
//! sequence floors. Setup creates the SSM parameter, Secret Manager secret or
//! Key Vault secret that holds it, and only the gate's identity can write it,
//! so the control plane can't roll it back.

use alien_aws_clients::ssm::{GetParameterRequest, PutParameterRequest, SsmApi};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{Context, IntoAlienError};
use alien_release_trust::{Floors, SignedStatement};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// The gate's accepted state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateState {
    /// Signer list versions, from version 1.
    #[serde(default)]
    pub signer_lists: Vec<SignedStatement>,
    /// Sequence floors.
    #[serde(default)]
    pub floors: Floors,
}

impl GateState {
    /// Load the state from `parameter`. A parameter that doesn't exist yet is
    /// an empty state: the first call trusts the list the anchor pins.
    pub async fn load(ssm: &dyn SsmApi, parameter: &str) -> Result<Self> {
        let response = ssm
            .get_parameter(GetParameterRequest {
                name: parameter.to_string(),
                with_decryption: Some(true),
            })
            .await;
        let value = match response {
            Ok(response) => response.parameter.and_then(|parameter| parameter.value),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                None
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudCallFailed {
                    operation: format!("ssm:GetParameter {parameter}"),
                })
            }
        };
        match value {
            None => Ok(Self::default()),
            Some(text) => serde_json::from_str(&text).into_alien_error().context(
                ErrorData::ConfigurationInvalid {
                    message: format!("parameter {parameter} does not hold the gate's state"),
                },
            ),
        }
    }

    /// Store the state in `parameter`.
    pub async fn store(&self, ssm: &dyn SsmApi, parameter: &str) -> Result<()> {
        let value = serde_json::to_string(self).into_alien_error().context(
            ErrorData::ConfigurationInvalid {
                message: "the gate's state does not serialize".to_string(),
            },
        )?;
        ssm.put_parameter(
            PutParameterRequest::builder()
                .name(parameter.to_string())
                .value(value)
                .parameter_type("String".to_string())
                .overwrite(true)
                .build(),
        )
        .await
        .context(ErrorData::CloudCallFailed {
            operation: format!("ssm:PutParameter {parameter}"),
        })?;
        Ok(())
    }
}
