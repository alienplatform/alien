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
    /// A setup link created together with its deployment group, on these
    /// platforms.
    Project {
        id_or_name: &'a str,
        platforms: &'a [String],
    },
}

/// The platform's answer when the deployment's group has more than one
/// manager that could encrypt its secret values.
const NEED_ONE_MANAGER: &str = "SECRET_VALUES_NEED_ONE_PRIVATE_ALIEN_INSTANCE";

/// One value to encrypt: its slot (`input:<id>`, `env:<name>`) and value.
#[derive(Serialize)]
pub(crate) struct SecretSlotValue {
    pub slot: String,
    pub value: String,
}

impl std::fmt::Debug for SecretSlotValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretSlotValue")
            .field("slot", &self.slot)
            .field("value", &"<redacted>")
            .finish()
    }
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

/// Checks that the address the platform gave for the manager is one the CLI
/// may send secret values to: an `https` URL with a host and no credentials.
/// Plain `http` is accepted only when `allow_http` is set, which only tests do
/// against a local mock server.
fn parse_manager_url(manager_url: &str, allow_http: bool) -> Result<url::Url> {
    let invalid = |message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: "managerUrl".to_string(),
            message,
        })
    };
    let parsed = url::Url::parse(manager_url)
        .map_err(|_| invalid("The manager address is not a valid URL".to_string()))?;
    let scheme_allowed = parsed.scheme() == "https" || (allow_http && parsed.scheme() == "http");
    if !scheme_allowed {
        return Err(invalid(format!(
            "The manager address must use https, not {}; secret values are not sent over an unencrypted connection",
            parsed.scheme()
        )));
    }
    if parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid(
            "The manager address must be a plain host with no credentials".to_string(),
        ));
    }
    Ok(parsed)
}

/// Encrypts `values` with the deployment's manager. Returns the ciphertext per
/// slot, or `None` when the platform encrypts this deployment's secret values
/// itself.
///
/// `require_instance` is set when an earlier response in the same command
/// already said a Private Alien Instance serves this deployment. The platform
/// then has no business answering that it doesn't, so that answer is an error
/// instead of a reason to send the values to the platform as they are.
pub(crate) async fn encrypt_with_manager(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    target: SecretValuesTarget<'_>,
    values: &[SecretSlotValue],
    require_instance: bool,
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
        SecretValuesTarget::Project {
            id_or_name,
            platforms,
        } => {
            let request = client
                .create_project_secret_access()
                .id_or_name(*id_or_name)
                .body(json_body::<types::CreateProjectSecretAccessRequest>(
                    serde_json::json!({ "platforms": platforms, "slots": slots }),
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
    };
    let access = match access {
        Ok(access) => access,
        Err(error) if error.code == NOT_SERVED_BY_MANAGER && !require_instance => return Ok(None),
        Err(error) if error.code == NOT_SERVED_BY_MANAGER => {
            return Err(error).context(ErrorData::ValidationError {
                field: "secret values".to_string(),
                message: "The platform reported that a Private Alien Instance serves this deployment, then said it does not. The secret values were not sent".to_string(),
            })
        }
        Err(error) if error.code == NEED_ONE_MANAGER => {
            return Err(error).context(ErrorData::ValidationError {
                field: "secret values".to_string(),
                message: "The deployment group has more than one Private Alien Instance. Run the command against a single deployment, or a group with one instance".to_string(),
            })
        }
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

    let manager_url = parse_manager_url(&access.manager_url, cfg!(test))?;
    let url = format!(
        "{}/v1/secret-values/encrypt",
        manager_url.as_str().trim_end_matches('/')
    );
    // A redirect would carry the bearer token and the values to another host.
    let manager_client = http_client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Failed to create HTTP client".to_string(),
        })?;
    let response = manager_client
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
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("The deployment's manager refused the secret values: HTTP {status}"),
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
    let mut values: Vec<SecretSlotValue> = Vec::new();
    let mut repeated_slot = None;
    for variable in json.as_array().into_iter().flatten().filter(|variable| {
        variable.get("type").and_then(serde_json::Value::as_str) == Some("secret")
    }) {
        let slot = slot(variable).ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "environment variables".to_string(),
                message: "A secret variable has no name".to_string(),
            })
        })?;
        let value = variable
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: slot.clone(),
                    message: "A secret value must be a string".to_string(),
                })
            })?;
        if values.iter().any(|existing| existing.slot == slot) {
            repeated_slot.get_or_insert(slot);
            continue;
        }
        values.push(SecretSlotValue {
            slot,
            value: value.to_string(),
        });
    }
    let Some(sealed) = encrypt_with_manager(client, workspace, target, &values, false).await?
    else {
        return Ok(variables);
    };
    if let Some(slot) = repeated_slot {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: slot,
            message: "Two secret variables have the same name. A manager that encrypts secret values takes one value per name, so give each secret its own name".to_string(),
        }));
    }
    for variable in json
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter(|variable| {
            variable.get("type").and_then(serde_json::Value::as_str) == Some("secret")
        })
    {
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
    require_instance: bool,
) -> Result<HashMap<String, serde_json::Value>> {
    let mut secret = Vec::new();
    for (id, value) in values
        .iter()
        .filter(|(id, _)| secret_input_ids.contains(id))
    {
        let value = value.as_str().ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: format!("input:{id}"),
                message: "A secret value must be a string".to_string(),
            })
        })?;
        secret.push(SecretSlotValue {
            slot: format!("input:{id}"),
            value: value.to_string(),
        });
    }
    let Some(sealed) =
        encrypt_with_manager(client, workspace, target, &secret, require_instance).await?
    else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method::POST, MockServer};

    fn platform(server: &MockServer) -> alien_platform_api::Client {
        alien_platform_api::Client::new_with_client(
            &server.base_url(),
            http_client::client().expect("client"),
        )
    }

    fn variables() -> Vec<types::EnvironmentVariableConfig> {
        serde_json::from_value(serde_json::json!([
            { "name": "API_TOKEN", "value": "canary-cli-secret", "type": "secret" },
            { "name": "LOG_LEVEL", "value": "debug", "type": "plain" }
        ]))
        .expect("variables")
    }

    /// The platform gets slot names only; the value goes to the manager, and
    /// what the caller submits afterwards is the manager's ciphertext.
    #[tokio::test]
    async fn secret_values_go_to_the_manager_and_only_ciphertext_comes_back() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        let access = platform_server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_123456789012345678901234567a/secret-access")
                    .json_body(serde_json::json!({ "slots": ["env:API_TOKEN"] }));
                then.status(200).json_body(serde_json::json!({
                    "managerUrl": manager.base_url(),
                    "accessToken": "short-lived",
                    "expiresIn": 300,
                    "slots": ["env:API_TOKEN"]
                }));
            })
            .await;
        let encrypt = manager
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/secret-values/encrypt")
                    .header("authorization", "Bearer short-lived")
                    .json_body(serde_json::json!({
                        "values": [{ "slot": "env:API_TOKEN", "value": "canary-cli-secret" }]
                    }));
                then.status(200).json_body(serde_json::json!({
                    "values": [{ "slot": "env:API_TOKEN", "ciphertext": "aiv1:key:sealed" }]
                }));
            })
            .await;

        let sealed = seal_environment_variables(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables(),
        )
        .await
        .expect("sealed");

        access.assert_async().await;
        encrypt.assert_async().await;
        let sealed = serde_json::to_value(sealed).unwrap();
        assert_eq!(sealed[0]["value"], "aiv1:key:sealed");
        assert_eq!(sealed[1]["value"], "debug");
    }

    #[tokio::test]
    async fn values_the_platform_encrypts_are_left_as_they_are() {
        let platform_server = MockServer::start_async().await;
        platform_server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_123456789012345678901234567a/secret-access");
                then.status(409).json_body(serde_json::json!({
                    "code": "SECRET_VALUES_NOT_SERVED_BY_INSTANCE",
                    "message": "Send its secret values to the Platform API.",
                    "retryable": false,
                    "internal": false
                }));
            })
            .await;

        let unchanged = seal_environment_variables(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables(),
        )
        .await
        .expect("unchanged");
        assert_eq!(
            serde_json::to_value(unchanged).unwrap()[0]["value"],
            "canary-cli-secret"
        );
    }

    #[tokio::test]
    async fn a_manager_that_refuses_stops_the_command() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        platform_server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_123456789012345678901234567a/secret-access");
                then.status(200).json_body(serde_json::json!({
                    "managerUrl": manager.base_url(),
                    "accessToken": "short-lived",
                    "expiresIn": 300,
                    "slots": ["env:API_TOKEN"]
                }));
            })
            .await;
        manager
            .mock_async(|when, then| {
                when.method(POST).path("/v1/secret-values/encrypt");
                then.status(503).json_body(serde_json::json!({
                    "code": "INSTANCE_KEY_UNAVAILABLE",
                    "message": "The key is disabled"
                }));
            })
            .await;

        let error = seal_environment_variables(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables(),
        )
        .await
        .expect_err("a refused value must not fall back to plaintext");
        assert!(error.message.contains("503"), "{}", error.message);
    }

    fn access_mock(platform_server: &MockServer, manager_url: &str) {
        let manager_url = manager_url.to_string();
        platform_server.mock(|when, then| {
            when.method(POST)
                .path("/v1/deployments/dep_123456789012345678901234567a/secret-access");
            then.status(200).json_body(serde_json::json!({
                "managerUrl": manager_url,
                "accessToken": "short-lived",
                "expiresIn": 300,
                "slots": ["env:API_TOKEN"]
            }));
        });
    }

    async fn seal(platform_server: &MockServer) -> Result<Vec<types::EnvironmentVariableConfig>> {
        seal_environment_variables(
            &platform(platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables(),
        )
        .await
    }

    #[tokio::test]
    async fn a_group_with_several_instances_stops_the_command() {
        let platform_server = MockServer::start_async().await;
        platform_server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_123456789012345678901234567a/secret-access");
                then.status(409).json_body(serde_json::json!({
                    "code": "SECRET_VALUES_NEED_ONE_PRIVATE_ALIEN_INSTANCE",
                    "message": "More than one instance",
                    "retryable": false,
                    "internal": false
                }));
            })
            .await;

        let error = seal(&platform_server)
            .await
            .expect_err("several instances must not fall back to plaintext");
        assert!(
            error
                .message
                .contains("more than one Private Alien Instance"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_redirecting_manager_never_receives_the_values() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        let elsewhere = MockServer::start_async().await;
        access_mock(&platform_server, &manager.base_url());
        manager
            .mock_async(|when, then| {
                when.method(POST).path("/v1/secret-values/encrypt");
                then.status(307)
                    .header("location", format!("{}/steal", elsewhere.base_url()));
            })
            .await;
        let stolen = elsewhere
            .mock_async(|when, then| {
                when.any_request();
                then.status(200);
            })
            .await;

        let error = seal(&platform_server)
            .await
            .expect_err("a redirect must stop the command");
        assert!(error.message.contains("307"), "{}", error.message);
        stolen.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn a_manager_error_body_is_not_echoed() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        access_mock(&platform_server, &manager.base_url());
        manager
            .mock_async(|when, then| {
                when.method(POST).path("/v1/secret-values/encrypt");
                then.status(400).body("echo: canary-cli-secret");
            })
            .await;

        let error = seal(&platform_server).await.expect_err("refused");
        assert!(error.message.contains("400"), "{}", error.message);
        assert!(
            !format!("{error:?} {error}").contains("canary-cli-secret"),
            "the manager's body must not reach the CLI's output"
        );
    }

    #[test]
    fn the_manager_address_must_be_a_plain_https_url() {
        assert!(parse_manager_url("https://manager.example.com", false).is_ok());
        for rejected in [
            "http://manager.example.com",
            "ftp://manager.example.com",
            "https://user:pass@manager.example.com",
            "https://",
            "not a url",
        ] {
            let error = parse_manager_url(rejected, false)
                .expect_err(&format!("{rejected} must be rejected"));
            assert!(error.message.contains("manager"), "{}", error.message);
        }
        assert!(parse_manager_url("http://127.0.0.1:1234", true).is_ok());
    }

    #[tokio::test]
    async fn a_manager_address_without_https_is_refused_before_any_request() {
        let platform_server = MockServer::start_async().await;
        access_mock(&platform_server, "ftp://manager.example.com");
        let error = seal(&platform_server).await.expect_err("scheme refused");
        assert!(error.message.contains("https"), "{}", error.message);
    }

    #[tokio::test]
    async fn a_plain_variable_with_a_secret_name_keeps_its_own_value() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        access_mock(&platform_server, &manager.base_url());
        manager.mock(|when, then| {
            when.method(POST).path("/v1/secret-values/encrypt");
            then.status(200).json_body(serde_json::json!({
                "values": [{ "slot": "env:API_TOKEN", "ciphertext": "aiv1:key:sealed" }]
            }));
        });
        let variables = serde_json::from_value(serde_json::json!([
            { "name": "API_TOKEN", "value": "plain-value", "type": "plain" },
            { "name": "API_TOKEN", "value": "canary-cli-secret", "type": "secret" }
        ]))
        .expect("variables");

        let sealed = seal_environment_variables(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables,
        )
        .await
        .expect("sealed");

        let sealed = serde_json::to_value(sealed).unwrap();
        assert_eq!(sealed[0]["value"], "plain-value");
        assert_eq!(sealed[1]["value"], "aiv1:key:sealed");
    }

    #[tokio::test]
    async fn two_secrets_with_one_name_are_refused_instead_of_sharing_a_value() {
        let platform_server = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        access_mock(&platform_server, &manager.base_url());
        manager.mock(|when, then| {
            when.method(POST).path("/v1/secret-values/encrypt");
            then.status(200).json_body(serde_json::json!({
                "values": [{ "slot": "env:API_TOKEN", "ciphertext": "aiv1:key:sealed" }]
            }));
        });
        let variables = serde_json::from_value(serde_json::json!([
            { "name": "API_TOKEN", "value": "first-secret", "type": "secret" },
            { "name": "API_TOKEN", "value": "second-secret", "type": "secret" }
        ]))
        .expect("variables");

        let error = seal_environment_variables(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            variables,
        )
        .await
        .expect_err("one ciphertext must not stand in for two different secrets");
        assert!(error.message.contains("same name"), "{}", error.message);
    }

    #[tokio::test]
    async fn a_secret_that_is_not_a_string_is_an_error() {
        let platform_server = MockServer::start_async().await;
        let values = HashMap::from([("token".to_string(), serde_json::json!(12345))]);
        let error = seal_input_values(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            &values,
            &["token".to_string()],
            false,
        )
        .await
        .expect_err("a non-string secret must not be sent or dropped");
        assert!(
            error.message.contains("must be a string"),
            "{}",
            error.message
        );
    }

    #[test]
    fn debug_output_hides_the_value() {
        let value = SecretSlotValue {
            slot: "env:API_TOKEN".to_string(),
            value: "canary-cli-secret".to_string(),
        };
        let debug = format!("{value:?}");
        assert!(debug.contains("env:API_TOKEN"));
        assert!(!debug.contains("canary-cli-secret"), "{debug}");
    }

    #[tokio::test]
    async fn a_platform_that_contradicts_itself_stops_the_command() {
        let platform_server = MockServer::start_async().await;
        platform_server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_123456789012345678901234567a/secret-access");
                then.status(409).json_body(serde_json::json!({
                    "code": "SECRET_VALUES_NOT_SERVED_BY_INSTANCE",
                    "message": "Send its secret values to the Platform API.",
                    "retryable": false,
                    "internal": false
                }));
            })
            .await;
        let values = HashMap::from([("token".to_string(), serde_json::json!("canary-cli-secret"))]);

        let error = seal_input_values(
            &platform(&platform_server),
            Some("acme"),
            SecretValuesTarget::Deployment("dep_123456789012345678901234567a"),
            &values,
            &["token".to_string()],
            true,
        )
        .await
        .expect_err("a known instance deployment must not fall back to the platform");
        assert!(error.message.contains("were not sent"), "{}", error.message);
    }
}
