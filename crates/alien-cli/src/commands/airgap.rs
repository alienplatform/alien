//! `alien airgap` — ship releases to deployments that can't reach your manager.
//!
//! `bundle` packages what sync would deliver (the release's target, its
//! images, the Operator and the chart) into one file; the environment applies
//! it with `alien-deploy airgap apply`. `import` takes the status file the
//! environment exports back and records it on the manager.

use std::path::PathBuf;

use alien_cli_common::airgap::{self, BundleManifest, RegistryAccess, CHART_DIR, OCI_DIR};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::SdkResultExt;
use clap::{Parser, Subcommand};
use oci_client::{client::ClientProtocol, secrets::RegistryAuth, Reference};

use crate::error::{ErrorData, Result};
use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::ui::{command, dim_label, success_line};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Ship releases to deployments that can't reach your manager",
    long_about = "Ship releases to deployments that can't reach your manager.\n\n`bundle` packages a release for one deployment: its images, the Operator and the Helm chart, in one file. Someone with access to the environment applies it with `alien-deploy airgap apply`, and sends back what `alien-deploy airgap status` exports, which `import` records here.",
    after_help = "EXAMPLES:
    alien onboard customer-3 --platforms kubernetes --airgapped
    alien airgap bundle customer-3/customer-3 -o customer-3.tar
    alien airgap import status.tar --deployment customer-3/customer-3"
)]
pub struct AirgapArgs {
    #[command(subcommand)]
    pub command: AirgapCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AirgapCommand {
    /// Package a release for an air-gapped deployment
    Bundle {
        /// Deployment (<group>/<name> or ID)
        deployment: String,
        /// Release to package (defaults to the latest)
        #[arg(long)]
        release: Option<String>,
        /// Output file
        #[arg(long, short = 'o')]
        output: PathBuf,
    },
    /// Record a status file exported by `alien-deploy airgap status`
    Import {
        /// Status file
        status: PathBuf,
        /// Deployment (<group>/<name> or ID)
        #[arg(long)]
        deployment: String,
    },
}

pub async fn airgap_task(args: AirgapArgs, ctx: ExecutionMode) -> Result<()> {
    let (project_id, _) = ctx.resolve_project(None, false).await?;
    let mgr = ctx.resolve_manager(&project_id, "local").await?;
    match args.command {
        AirgapCommand::Bundle {
            deployment,
            release,
            output,
        } => bundle(&mgr, &deployment, release, output, ctx.is_dev()).await,
        AirgapCommand::Import { status, deployment } => {
            import(&mgr, &status, &deployment, ctx.is_dev()).await
        }
    }
}

async fn bundle(
    mgr: &ManagerContext,
    reference: &str,
    release: Option<String>,
    output: PathBuf,
    is_dev: bool,
) -> Result<()> {
    let deployment = crate::deployment_resolver::resolve(&mgr.client, reference, is_dev).await?;
    let manager = mgr
        .client
        .manager_info()
        .send()
        .await
        .into_sdk_error()
        .context(api_failed("reading manager information"))?
        .into_inner();
    let operator_image = manager.operator_image.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "This manager serves no Operator image, so it can't build bundles".to_string(),
        })
    })?;

    println!("Resolving the target for {reference}");
    let mut target_request = mgr.client.get_deployment_target().id(&deployment.id);
    if let Some(release) = &release {
        target_request = target_request.release_id(release);
    }
    let target: serde_json::Value = target_request
        .send()
        .await
        .into_sdk_error()
        .context(api_failed("building the deployment target"))?
        .into_inner();
    let release_id = target["releaseInfo"]["releaseId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let stack_id = target["releaseInfo"]["stack"]["id"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "target has no stack id".to_string(),
            })
        })?
        .to_string();

    let work = tempfile::tempdir()
        .into_alien_error()
        .context(file_error("creating a working directory"))?;
    let dir = work.path();

    // Images are named by the manager's public host; this CLI may reach the
    // manager at another address, so pulls go there.
    let access = RegistryAccess {
        auth: match &mgr.auth_token {
            Some(token) => RegistryAuth::Basic("token".to_string(), token.clone()),
            None => RegistryAuth::Anonymous,
        },
        insecure: mgr.manager_url.starts_with("http://"),
    };
    let reachable_host = alien_core::image_rewrite::strip_url_scheme(&mgr.manager_url).to_string();
    let mut images = airgap::image_references(&target);
    images.push(operator_image.clone());
    images.sort();
    images.dedup();
    println!("Copying {} images", images.len());
    let pull_refs: Vec<String> = images
        .iter()
        .map(|image| retarget_host(image, &manager.registry_host, &reachable_host))
        .collect();
    let mut exported = airgap::export_images(&dir.join(OCI_DIR), &pull_refs, &access)
        .await
        .context(file_error("copying images"))?;
    for (image, source) in exported.iter_mut().zip(images.iter()) {
        image.source = source.clone();
    }

    println!("Adding the Helm chart");
    let chart_file = pull_chart(mgr, &access, &reachable_host, &stack_id, &release_id, dir).await?;

    std::fs::write(
        dir.join(airgap::TARGET_FILE),
        serde_json::to_vec_pretty(&target)
            .into_alien_error()
            .context(file_error("encoding the target"))?,
    )
    .into_alien_error()
    .context(file_error("writing the target"))?;

    let manifest = BundleManifest {
        format_version: airgap::FORMAT_VERSION,
        deployment_id: deployment.id.clone(),
        deployment_name: deployment.name.clone(),
        release_id: release_id.clone(),
        sequence: chrono::Utc::now().timestamp_millis() as u64,
        created_at: chrono::Utc::now().to_rfc3339(),
        stack_id,
        images: exported,
        operator_image,
        chart: chart_file,
        files: airgap::checksums(dir)
            .await
            .context(file_error("checksumming"))?,
    };
    std::fs::write(
        dir.join(airgap::MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)
            .into_alien_error()
            .context(file_error("encoding the manifest"))?,
    )
    .into_alien_error()
    .context(file_error("writing the manifest"))?;
    airgap::pack(dir, &output).context(file_error("writing the bundle"))?;

    let size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    println!(
        "{}",
        success_line(&format!(
            "Bundle ready: {} ({:.1} MiB, release {release_id}).",
            output.display(),
            size as f64 / 1_048_576.0
        ))
    );
    println!(
        "{}",
        dim_label("In the environment, with access to its registry and cluster:")
    );
    println!(
        "  {}",
        command(&format!(
            "alien-deploy airgap apply {} --registry <registry>/<path> -f values.yaml",
            output.display()
        ))
    );
    Ok(())
}

/// The chart for `release_id`, found by the release annotation on the charts
/// the manager serves for `stack_id`.
async fn pull_chart(
    mgr: &ManagerContext,
    access: &RegistryAccess,
    host: &str,
    stack_id: &str,
    release_id: &str,
    dir: &std::path::Path,
) -> Result<String> {
    let client = oci_client::Client::new(oci_client::client::ClientConfig {
        protocol: if access.insecure {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
        ..Default::default()
    });
    let repository: Reference = format!("{host}/charts/{stack_id}")
        .parse()
        .into_alien_error()
        .context(file_error("building the chart reference"))?;
    let tags = client
        .list_tags(&repository, &access.auth, None, None)
        .await
        .into_alien_error()
        .context(api_failed("listing charts"))?
        .tags;
    for tag in tags.iter().rev() {
        let reference =
            Reference::with_tag(host.to_string(), format!("charts/{stack_id}"), tag.clone());
        let (manifest, _) = client
            .pull_manifest_raw(
                &reference,
                &access.auth,
                &["application/vnd.oci.image.manifest.v1+json"],
            )
            .await
            .into_alien_error()
            .context(api_failed("reading a chart manifest"))?;
        let manifest: serde_json::Value = serde_json::from_slice(&manifest)
            .into_alien_error()
            .context(file_error("parsing a chart manifest"))?;
        if manifest["annotations"]["org.opencontainers.image.revision"] != release_id {
            continue;
        }
        let layer = manifest["layers"][0]["digest"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "chart manifest has no layer".to_string(),
            })
        })?;
        std::fs::create_dir_all(dir.join(CHART_DIR))
            .into_alien_error()
            .context(file_error("creating the chart directory"))?;
        let file = format!("{CHART_DIR}/{stack_id}-{tag}.tgz");
        let mut out = tokio::fs::File::create(dir.join(&file))
            .await
            .into_alien_error()
            .context(file_error("creating the chart file"))?;
        client
            .pull_blob(&reference, layer, &mut out)
            .await
            .into_alien_error()
            .context(api_failed("downloading the chart"))?;
        return Ok(file);
    }
    let _ = mgr;
    Err(AlienError::new(ErrorData::ConfigurationError {
        message: format!("the manager serves no chart for release {release_id}"),
    }))
}

async fn import(
    mgr: &ManagerContext,
    status: &std::path::Path,
    reference: &str,
    is_dev: bool,
) -> Result<()> {
    let deployment = crate::deployment_resolver::resolve(&mgr.client, reference, is_dev).await?;
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(file_error("creating a working directory"))?;
    airgap::unpack(status, work.path()).context(file_error("reading the status file"))?;
    let status_json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(work.path().join("status.json"))
            .into_alien_error()
            .context(file_error("status file has no status.json"))?,
    )
    .into_alien_error()
    .context(file_error("parsing status.json"))?;
    if status_json["deploymentId"].as_str() != Some(deployment.id.as_str()) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment".to_string(),
            message: format!(
                "status file is for {}, not {}",
                status_json["deploymentId"], deployment.id
            ),
        }));
    }
    let logs: Vec<serde_json::Value> = std::fs::read_to_string(work.path().join("logs.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()
        .into_alien_error()
        .context(file_error("parsing logs.jsonl"))?;

    let body = serde_json::json!({ "state": status_json["state"], "logs": logs });
    let response = mgr
        .http_client
        .post(format!(
            "{}/v1/deployments/{}/status-report",
            mgr.manager_url.trim_end_matches('/'),
            deployment.id
        ))
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(api_failed("sending the status report"))?;
    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "status report rejected ({}): {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ),
            url: None,
        }));
    }
    let result: serde_json::Value = response
        .json()
        .await
        .into_alien_error()
        .context(api_failed("reading the response"))?;
    println!(
        "{}",
        success_line(&format!(
            "{reference}: {} ({} log lines imported, status from {}).",
            result["status"].as_str().unwrap_or("unknown"),
            result["logsAccepted"],
            status_json["writtenAt"].as_str().unwrap_or("unknown time")
        ))
    );
    Ok(())
}

fn retarget_host(image: &str, public_host: &str, reachable_host: &str) -> String {
    match image.strip_prefix(&format!("{public_host}/")) {
        Some(rest) if public_host != reachable_host => format!("{reachable_host}/{rest}"),
        _ => image.to_string(),
    }
}

fn api_failed(message: &str) -> ErrorData {
    ErrorData::ApiRequestFailed {
        message: message.to_string(),
        url: None,
    }
}

fn file_error(message: &str) -> ErrorData {
    ErrorData::ConfigurationError {
        message: message.to_string(),
    }
}
