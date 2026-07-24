use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MachinesJoinTokenResponse {
    pub(super) join_token: String,
    pub(super) control_plane_url: Option<String>,
    pub(super) cluster_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WrappedMachinesJoinToken<'a> {
    join_token: &'a str,
    control_plane_url: &'a str,
    cluster_id: &'a str,
}

pub(super) async fn create_machines_join_token(
    base_url: &str,
    token: &str,
    deployment_id: &str,
) -> Result<String> {
    let http_client = create_manager_http_client(token)?;
    let url = format!(
        "{}/v1/machines/deployments/{}/join-tokens/rotate",
        base_url.trim_end_matches('/'),
        urlencoding::encode(deployment_id),
    );

    let response = http_client
        .post(&url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Machines join token from platform API".to_string(),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to create Machines join token (HTTP {status}): {body}"),
        }));
    }

    let response: MachinesJoinTokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse Machines join token response".to_string(),
            })?;

    normalize_machines_join_token_response(response)
}

pub(super) fn normalize_machines_join_token_response(
    response: MachinesJoinTokenResponse,
) -> Result<String> {
    let join_token = response.join_token.trim();
    if join_token.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an empty Machines join token".to_string(),
        }));
    }
    if join_token.starts_with("aj1_") {
        return Ok(join_token.to_string());
    }

    let control_plane_url = response
        .control_plane_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cluster_id = response
        .cluster_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (control_plane_url, cluster_id) {
        (Some(control_plane_url), Some(cluster_id)) => {
            validate_machines_control_plane_url(control_plane_url)?;
            let payload = WrappedMachinesJoinToken {
                join_token,
                control_plane_url,
                cluster_id,
            };
            let json = serde_json::to_vec(&payload).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to encode Machines join token context".to_string(),
                },
            )?;
            Ok(format!("aj1_{}", URL_SAFE_NO_PAD.encode(json)))
        }
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Platform API returned a raw Machines join token without control plane context"
                    .to_string(),
        })),
    }
}

fn validate_machines_control_plane_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Platform API returned an invalid Machines control plane URL: {e}"),
        })
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an invalid Machines control plane URL".to_string(),
        }));
    }
    Ok(())
}

pub(super) fn machines_join_command(
    cli_name: &str,
    install_script_url: Option<&str>,
    join_token: &str,
) -> String {
    if let Some(install_script_url) = install_script_url {
        return format!(
            "curl -fsSL {} | sudo bash -s -- join --token {}",
            shell_single_quote(install_script_url),
            shell_single_quote(join_token)
        );
    }

    format!(
        "sudo {cli_name} join --token {}",
        shell_single_quote(join_token)
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
