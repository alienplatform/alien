//! Secret values that a deployment's manager encrypts with its own key.
//!
//! A manager may hold a key in the customer's cloud account and encrypt the
//! secret values of the deployments it runs with it. For those deployments the
//! platform accepts secret values only as that manager's ciphertext. The CLI
//! asks the platform for a short-lived token covering exactly the values it is
//! about to set, sends the values straight to the manager, and submits the
//! returned ciphertext in their place. The platform never receives the values.

use std::collections::HashMap;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{types, SdkResultExt};
use serde::{Deserialize, Serialize};

use crate::{
    error::{ErrorData, Result},
    http_client,
};

/// The platform's answer when the deployment's secret values are not
/// encrypted by its manager: send them to the platform as they are.
const NOT_SERVED_BY_MANAGER: &str = "SECRET_VALUES_NOT_SERVED_BY_INSTANCE";

/// What the values are for.
pub(crate) enum SecretValuesTarget<'a> {
    /// An existing deployment.
    Deployment(&'a str),
    /// A deployment the group is about to create, or a setup link of the
    /// group, on these platforms.
    DeploymentGroup {
        id: &'a str,
        platforms: &'a [String],
    },
}

/// One value to encrypt: its slot (`input:<id>`, `env:<name>`) and value.
#[derive(Debug, Serialize)]
pub(crate) struct SecretSlotValue {
    pub slot: String,
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagerAccess {
    manager_url: String,
    access_token: String,
}

#[derive(Serialize)]
struct EncryptRequest<'a> {
    values: &'a [SecretSlotValue],
}

#[derive(Deserialize)]
struct EncryptResponse {
    values: Vec<EncryptedValue>,
}

#[derive(Deserialize)]
struct EncryptedValue {
    slot: String,
    ciphertext: String,
}

/// Encrypts `values` with the deployment's manager. Returns the ciphertext per
/// slot, or `None` when the platform encrypts this deployment's secret values
/// itself.
pub(crate) async fn encrypt_with_manager(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    target: SecretValuesTarget<'_>,
    values: &[SecretSlotValue],
) -> Result<Option<HashMap<String, String>>> {
    if values.is_empty() {
        return Ok(None);
    }
    let slots: Vec<&str> = values.iter().map(|value| value.slot.as_str()).collect();
    let access = match &target {
        SecretValuesTarget::Deployment(id) => {
            let request = client
                .create_deployment_secret_access()
                .id(*id)
                .body(json_body::<types::CreateSecretAccessRequest>(
                    serde_json::json!({ "slots": slots }),
                )?);
            match workspace {
                Some(workspace) => request.workspace(workspace),
                None => request,
            }
            .send()
            .await
            .into_sdk_error()
            .map(|response| serde_json::to_value(response.into_inner()))
        }
        SecretValuesTarget::DeploymentGroup { id, platforms } => {
            let request = client
                .create_deployment_group_secret_access()
                .id(*id)
                .body(
                    json_body::<types::CreateDeploymentGroupSecretAccessRequest>(
                        serde_json::json!({ "platforms": platforms, "slots": slots }),
                    )?,
                );
            match workspace {
                Some(workspace) => request.workspace(workspace),
                None => request,
            }
            .send()
            .await
            .into_sdk_error()
            .map(|response| serde_json::to_value(response.into_inner()))
        }
    };
    let access = match access {
        Ok(access) => access,
        Err(error) if error.code == NOT_SERVED_BY_MANAGER => return Ok(None),
        Err(error) => {
            return Err(error).context(ErrorData::ApiRequestFailed {
                message: "getting access to the deployment's manager for secret values".to_string(),
                url: None,
            })
        }
    };
    let access: ManagerAccess = access
        .and_then(serde_json::from_value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "secret access response".to_string(),
        })?;

    let url = format!(
        "{}/v1/secret-values/encrypt",
        access.manager_url.trim_end_matches('/')
    );
    let response = http_client::client()?
        .post(&url)
        .bearer_auth(&access.access_token)
        .json(&EncryptRequest { values })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: format!("The deployment's manager at {url} could not be reached"),
        })?;
    let status = response.status();
    if !status.is_success() {
        // The body carries the manager's error, never a value.
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "The deployment's manager refused the secret values: HTTP {status} {body}"
            ),
            url: Some(url),
        }));
    }
    let encrypted: EncryptResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "manager encrypt response".to_string(),
            })?;
    let encrypted: HashMap<String, String> = encrypted
        .values
        .into_iter()
        .map(|value| (value.slot, value.ciphertext))
        .collect();
    if let Some(missing) = slots.iter().find(|slot| !encrypted.contains_key(**slot)) {
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("The deployment's manager returned no ciphertext for '{missing}'"),
            url: Some(url),
        }));
    }
    Ok(Some(encrypted))
}

fn json_body<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "secret values".to_string(),
            message: "The request doesn't match the Platform API schema".to_string(),
        })
}

/// Encrypts the `secret` variables among `variables` with the deployment's
/// manager, when it encrypts them. Other variables are returned unchanged.
pub(crate) async fn seal_environment_variables(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    target: SecretValuesTarget<'_>,
    variables: Vec<types::EnvironmentVariableConfig>,
) -> Result<Vec<types::EnvironmentVariableConfig>> {
    let mut json = serde_json::to_value(&variables)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "environment variables".to_string(),
        })?;
    let slot = |variable: &serde_json::Value| {
        variable
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(|name| format!("env:{name}"))
    };
    let values: Vec<SecretSlotValue> = json
        .as_array()
        .into_iter()
        .flatten()
        .filter(|variable| {
            variable.get("type").and_then(serde_json::Value::as_str) == Some("secret")
        })
        .filter_map(|variable| {
            Some(SecretSlotValue {
                slot: slot(variable)?,
                value: variable.get("value")?.as_str()?.to_string(),
            })
        })
        .collect();
    let Some(sealed) = encrypt_with_manager(client, workspace, target, &values).await? else {
        return Ok(variables);
    };
    for variable in json.as_array_mut().into_iter().flatten() {
        if let Some(ciphertext) = slot(variable).and_then(|slot| sealed.get(&slot)) {
            variable["value"] = serde_json::Value::String(ciphertext.clone());
        }
    }
    json_body(json)
}

/// Encrypts the values of `secret_input_ids` among `values` with the
/// deployment's manager, when it encrypts them. Other values are returned
/// unchanged.
pub(crate) async fn seal_input_values(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    target: SecretValuesTarget<'_>,
    values: &HashMap<String, serde_json::Value>,
    secret_input_ids: &[String],
) -> Result<HashMap<String, serde_json::Value>> {
    let secret: Vec<SecretSlotValue> = values
        .iter()
        .filter(|(id, _)| secret_input_ids.contains(id))
        .filter_map(|(id, value)| {
            Some(SecretSlotValue {
                slot: format!("input:{id}"),
                value: value.as_str()?.to_string(),
            })
        })
        .collect();
    let Some(sealed) = encrypt_with_manager(client, workspace, target, &secret).await? else {
        return Ok(values.clone());
    };
    Ok(values
        .iter()
        .map(|(id, value)| {
            let value = sealed
                .get(&format!("input:{id}"))
                .map(|ciphertext| serde_json::Value::String(ciphertext.clone()))
                .unwrap_or_else(|| value.clone());
            (id.clone(), value)
        })
        .collect())
}
