//! Release channels and pins on a manager you run (`/v1/release-channels`,
//! `/v1/releases/{id}/promote`, `/v1/deployments/{id}/channel|pin`).

use alien_error::Context;
use alien_manager_api::types::{
    CreateReleaseChannelRequest, DeploymentRoutingResponse, PromoteReleaseRequest,
    ReleaseChannelResponse, SetDeploymentChannelRequest, SetDeploymentPinRequest,
};
use alien_manager_api::SdkResultExt as _;

use crate::error::{ErrorData, Result};
use crate::output::print_json;
use crate::ui::{make_table, print_table, success_line};

fn api_failed(message: impl Into<String>) -> ErrorData {
    ErrorData::ApiRequestFailed {
        message: message.into(),
        url: None,
    }
}

fn print_channel(channel: &ReleaseChannelResponse, json: bool, verb: &str) -> Result<()> {
    if json {
        return print_json(channel);
    }
    println!(
        "{}",
        success_line(&format!(
            "Channel {} {verb} {}.",
            channel.name,
            channel
                .current_release_id
                .as_deref()
                .unwrap_or("no release yet")
        ))
    );
    Ok(())
}

fn print_routing(routing: &DeploymentRoutingResponse, json: bool) -> Result<()> {
    if json {
        return print_json(routing);
    }
    let release = routing.release_id.as_deref().unwrap_or("no release yet");
    match &routing.pinned_release_id {
        Some(pinned) => println!(
            "{}",
            success_line(&format!(
                "{} is pinned to {pinned} (channel {}).",
                routing.deployment_id, routing.channel
            ))
        ),
        None => println!(
            "{}",
            success_line(&format!(
                "{} follows {}, now at {release}.",
                routing.deployment_id, routing.channel
            ))
        ),
    }
    Ok(())
}

pub async fn list_channels(client: &alien_manager_api::Client, json: bool) -> Result<()> {
    let response = client
        .list_manager_release_channels()
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed("listing release channels"))?
        .into_inner();
    if json {
        return print_json(&response.items);
    }
    if response.items.is_empty() {
        println!("(no channels yet; the first `alien release` creates production)");
        return Ok(());
    }
    let mut table = make_table(&["Channel", "Release", "Deployments", "Updated"]);
    for channel in &response.items {
        table.add_row(vec![
            comfy_table::Cell::new(&channel.name),
            comfy_table::Cell::new(channel.current_release_id.as_deref().unwrap_or("—")),
            comfy_table::Cell::new(channel.deployments),
            comfy_table::Cell::new(channel.updated_at.to_rfc3339()),
        ]);
    }
    print_table(table);
    Ok(())
}

pub async fn create_channel(
    client: &alien_manager_api::Client,
    name: &str,
    release: Option<&str>,
    json: bool,
) -> Result<()> {
    let channel = client
        .create_manager_release_channel()
        .body(CreateReleaseChannelRequest {
            name: name.to_string(),
            release_id: release.map(str::to_string),
        })
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed(format!("creating channel '{name}'")))?
        .into_inner();
    print_channel(&channel, json, "created at")
}

pub async fn delete_channel(client: &alien_manager_api::Client, name: &str) -> Result<()> {
    client
        .delete_manager_release_channel()
        .name(name)
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed(format!("deleting channel '{name}'")))?;
    println!("{}", success_line(&format!("Deleted channel {name}.")));
    Ok(())
}

pub async fn promote(
    client: &alien_manager_api::Client,
    release_id: &str,
    channel: &str,
    json: bool,
) -> Result<()> {
    let channel = client
        .promote_manager_release()
        .id(release_id)
        .body(PromoteReleaseRequest {
            channel: channel.to_string(),
        })
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed(format!(
            "promoting {release_id} to channel '{channel}'"
        )))?
        .into_inner();
    print_channel(&channel, json, "now points at")
}

pub async fn set_channel(
    client: &alien_manager_api::Client,
    deployment_id: &str,
    channel: &str,
    json: bool,
) -> Result<()> {
    let routing = client
        .set_deployment_channel()
        .id(deployment_id)
        .body(SetDeploymentChannelRequest {
            channel: channel.to_string(),
        })
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed(format!(
            "moving deployment '{deployment_id}' to channel '{channel}'"
        )))?
        .into_inner();
    print_routing(&routing, json)
}

pub async fn pin(
    client: &alien_manager_api::Client,
    deployment_id: &str,
    release_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let routing = client
        .set_deployment_pin()
        .id(deployment_id)
        .body(SetDeploymentPinRequest {
            release_id: release_id.map(str::to_string),
        })
        .send()
        .await
        .into_sdk_error()
        .await
        .context(api_failed(format!("pinning deployment '{deployment_id}'")))?
        .into_inner();
    print_routing(&routing, json)
}
