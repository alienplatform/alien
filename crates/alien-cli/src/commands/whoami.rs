use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use alien_error::{Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show the current authenticated principal",
    long_about = "Display information about the current authenticated principal for the selected target.",
    after_help = "EXAMPLES:
    alien whoami
    alien whoami --json
    alien dev whoami"
)]
pub struct WhoamiArgs {
    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn whoami_task(args: WhoamiArgs, ctx: ExecutionMode) -> Result<()> {
    ctx.ensure_ready().await?;

    if let ExecutionMode::Dev { port } = ctx {
        return whoami_task_dev(args, port).await;
    }

    let response = ctx
        .sdk_client()
        .await?
        .whoami()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to get user information".to_string(),
            url: None,
        })?
        .into_inner();

    if args.json {
        print_json(&response)?;
    } else {
        println!("{:#?}", response);
    }

    Ok(())
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalWhoamiScope {
    #[serde(rename = "type")]
    scope_type: String,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalWhoamiResponse {
    kind: String,
    id: String,
    scope: LocalWhoamiScope,
}

async fn whoami_task_dev(args: WhoamiArgs, port: u16) -> Result<()> {
    let url = format!("http://localhost:{port}/v1/whoami");
    let response =
        reqwest::get(&url)
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to get local manager identity".to_string(),
                url: Some(url.clone()),
            })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to read local manager identity response".to_string(),
            url: Some(url.clone()),
        })?;

    if !status.is_success() {
        return Err(alien_error::AlienError::new(ErrorData::ApiRequestFailed {
            message: if body.trim().is_empty() {
                format!("local manager returned HTTP {}", status.as_u16())
            } else {
                body
            },
            url: Some(url),
        }));
    }

    let response: LocalWhoamiResponse =
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: "Failed to parse local manager identity response".to_string(),
            })?;

    if args.json {
        print_json(&response)?;
    } else {
        println!("Kind: {}", response.kind);
        println!("ID: {}", response.id);
        println!("Scope: {}", response.scope.scope_type);
    }

    Ok(())
}
