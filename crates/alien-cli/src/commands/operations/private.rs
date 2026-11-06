//! Direct operation inputs and results, with metadata-only authorization.
use std::time::Duration;

use alien_commands_client::{CommandsClient, CommandsClientConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    manager_url: String,
    token: String,
    command: String,
    target_resource_id: String,
    deadline: String,
    idempotency_key: String,
    operation_result_contract: Value,
}

fn failure(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ApiRequestFailed {
        message: message.into(),
        url: None,
    })
}

async fn json_response(response: reqwest::Response) -> Result<Value> {
    if !response.status().is_success() {
        return Err(failure(&format!(
            "Direct operation request refused with HTTP {}",
            response.status()
        )));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) =
        response
            .chunk()
            .await
            .into_alien_error()
            .context(ErrorData::NetworkError {
                message: "Reading operation metadata".into(),
            })?
    {
        if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
            return Err(failure("Operation metadata exceeded its size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Invalid operation metadata response".into(),
            url: None,
        })
}

fn endpoint(base: &str, path: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(base)
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Invalid operation endpoint".into(),
        })?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failure(
            "Operation endpoints require HTTPS without credentials, query, or fragment",
        ));
    }
    url.set_path(&format!("{}{}", url.path().trim_end_matches('/'), path));
    Ok(url)
}

pub(super) struct Invocation<'a> {
    pub deployment: &'a str,
    pub plugin: &'a str,
    pub operation: &'a str,
    pub params: Value,
    pub timeout_secs: u64,
    pub print_as_json: bool,
}

pub(super) async fn invoke(
    ctx: &ExecutionMode,
    workspace: &str,
    project: &str,
    invocation: Invocation<'_>,
) -> Result<()> {
    let Invocation {
        deployment,
        plugin,
        operation,
        params,
        timeout_secs,
        print_as_json,
    } = invocation;
    let auth = ctx.auth_http().await?;
    let token = auth
        .bearer_token
        .as_ref()
        .ok_or_else(|| failure("An authenticated operation connection is required"))?;
    let http = crate::http_client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Initializing direct operation transport".into(),
        })?;
    let mut prepare = endpoint(&auth.base_url, "/v1/operations/invoke")?;
    prepare
        .query_pairs_mut()
        .append_pair("workspace", workspace)
        .append_pair("project", project);
    let prepared = json_response(http.post(prepare).bearer_auth(token).json(&json!({"deploymentId": deployment, "plugin":plugin, "operation":operation, "delivery":"direct"})).send().await.into_alien_error().context(ErrorData::NetworkError { message: "Authorizing direct operation".into() })?).await?;
    if prepared["status"] != "prepared" {
        return Err(failure("This operation requires approval. No inputs were sent; private parameter-bound approval is not available yet."));
    }
    let connection: Connection = serde_json::from_value(prepared["connection"].clone())
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Invalid direct operation connection".into(),
            url: None,
        })?;
    let selected = endpoint(&connection.manager_url, "/v1/commands")?;
    let created = json_response(http.post(selected).bearer_auth(&connection.token).json(&json!({"deploymentId": deployment, "command":connection.command, "targetResourceId":connection.target_resource_id, "deadline":connection.deadline, "idempotencyKey":connection.idempotency_key, "operationResultContract":connection.operation_result_contract, "params":{"mode":"inline", "inlineBase64":STANDARD.encode(serde_json::to_vec(&params).into_alien_error().context(ErrorData::NetworkError { message:"Encoding operation input".into() })?)}})).send().await.into_alien_error().context(ErrorData::NetworkError { message:"Dispatching directly to selected instance".into() })?).await?;
    let command_id = created["commandId"]
        .as_str()
        .filter(|id| {
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .ok_or_else(|| failure("Invalid command acknowledgement"))?;
    let mut status_url = endpoint(
        &auth.base_url,
        &format!("/v1/commands/{command_id}/connection"),
    )?;
    status_url
        .query_pairs_mut()
        .append_pair("workspace", workspace);
    let status = json_response(
        http.post(status_url)
            .bearer_auth(token)
            .json(&json!({"confirmSensitiveOutput":false}))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::NetworkError {
                message: "Authorizing exact operation result".into(),
            })?,
    )
    .await?;
    if status["managerUrl"].as_str() != Some(connection.manager_url.as_str())
        || status["commandId"].as_str() != Some(command_id)
    {
        return Err(failure(
            "Operation assignment changed; refusing another result destination",
        ));
    }
    let status_token = status["token"]
        .as_str()
        .ok_or_else(|| failure("Missing exact result capability"))?;
    let client = CommandsClient::with_config(
        &format!("{}/v1", connection.manager_url.trim_end_matches('/')),
        deployment,
        status_token,
        CommandsClientConfig {
            timeout: Duration::from_secs(timeout_secs.min(240)),
            ..Default::default()
        },
    );
    let result: Value = client.wait_for_completion(command_id).await.into_alien_error().context(ErrorData::ApiRequestFailed { message: format!("Waiting for private operation {command_id}; do not retry a write without checking its state"), url: None })?;
    let output = json!({"commandId":command_id,"result":result,"verification":{"status":"unverified","reason":"Private operation verification has not run"}});
    if print_as_json {
        crate::output::print_json(&output)?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&output)
                .into_alien_error()
                .context(ErrorData::NetworkError {
                    message: "Formatting operation result".into()
                })?
        );
    }
    Ok(())
}
