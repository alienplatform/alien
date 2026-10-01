//! `alien-deploy sync` — keep an air-gapped site up to date by carrying one
//! folder across the gap.
//!
//! Run it on both sides. It does whatever it can reach:
//!
//! - **Online** (the manager is reachable): sends back the reports the site
//!   wrote, then downloads the release the site should run as a signed
//!   bundle into the folder. Only image layers the site doesn't have yet are
//!   included.
//! - **Inside the site** (a registry is configured and the cluster is
//!   reachable): verifies the newest bundle against the key the install
//!   trusts, pushes its images to the site's registry, installs or upgrades
//!   the chart, waits for the rollout, and writes a report: the deployment's
//!   state and the telemetry the Operator buffered.
//!
//! The deployment token stays on the online machine, outside the folder.
//!
//! ```text
//! <site>-sync/
//!   site.json         which deployment, and settings each side remembers
//!   to-site/          bundles waiting to be installed
//!     installed/      the last installed bundles, for `alien-deploy rollback`
//!   from-site/        reports waiting to be sent
//! ```

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use alien_cli_common::airgap::{self, BundleManifest, RegistryAccess};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine as _;
use clap::Parser;
use oci_client::secrets::RegistryAuth;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncBufReadExt as _, process::Command};

use crate::error::{ErrorData, Result};

const SITE_FILE: &str = "site.json";
const TO_SITE: &str = "to-site";
const FROM_SITE: &str = "from-site";
const INSTALLED: &str = "installed";
const BUNDLE_EXTENSION: &str = "bundle";
const REPORT_EXTENSION: &str = "report";
/// Installed bundles kept for rollback.
const KEEP_INSTALLED: usize = 3;
const ROLLOUT_TIMEOUT: Duration = Duration::from_secs(600);
/// `alien-deploy` builds for the site, inside bundles and the folder.
const TOOLS_DIR: &str = "tools";

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Keep an air-gapped deployment up to date",
    long_about = "Keep an air-gapped deployment up to date by carrying one folder across the gap.\n\nRun `alien-deploy sync` on a machine that can reach the vendor's manager: it sends back what the site reported and downloads the next update. Carry the folder into the site and run `alien-deploy sync` again: it installs the update and writes a report to carry back. On a machine that reaches both, one run does both.",
    after_help = "EXAMPLES:
    # First run, on the online machine (token and URL from the vendor)
    alien-deploy sync --token ax_... --manager https://manager.example.com

    # Inside the site, the first time
    alien-deploy sync --registry registry.internal/vendor -f values.yaml --trusted-key ed25519:...

    # Every time after that, on either side
    alien-deploy sync
    alien-deploy sync --dry-run"
)]
pub struct SyncArgs {
    /// Transfer folder (defaults to the one in the current directory)
    #[arg(long)]
    pub folder: Option<PathBuf>,
    /// Deployment token from the vendor. Needed once on the online machine.
    #[arg(long, env = "ALIEN_SYNC_TOKEN")]
    pub token: Option<String>,
    /// Manager URL from the vendor. Needed once on the online machine.
    #[arg(long)]
    pub manager: Option<String>,
    /// Site registry (and optional path) for the images. Needed once inside the site.
    #[arg(long)]
    pub registry: Option<String>,
    #[arg(long, env = "AIRGAP_REGISTRY_USERNAME")]
    pub registry_username: Option<String>,
    #[arg(long, env = "AIRGAP_REGISTRY_PASSWORD")]
    pub registry_password: Option<String>,
    /// The site registry serves plain HTTP
    #[arg(long)]
    pub insecure_registry: bool,
    /// Namespace (defaults to the application name)
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Helm values for the site (infrastructure, inputs). Remembered.
    #[arg(long = "values", short = 'f')]
    pub values: Vec<PathBuf>,
    /// Kubernetes context (defaults to the current one). Remembered.
    #[arg(long)]
    pub kube_context: Option<String>,
    /// The vendor's bundle key (`ed25519:...`). Needed for the first install.
    #[arg(long, env = "ALIEN_TRUSTED_KEY")]
    pub trusted_key: Option<String>,
    /// Show what would happen without installing or downloading anything
    #[arg(long)]
    pub dry_run: bool,
    /// Include every image layer, for sites whose reports never come back
    #[arg(long)]
    pub full: bool,
}

#[derive(Parser, Debug, Clone)]
#[command(about = "Return an air-gapped deployment to the release it ran before the last sync")]
pub struct RollbackArgs {
    /// Transfer folder (defaults to the one in the current directory)
    #[arg(long)]
    pub folder: Option<PathBuf>,
    /// Kubernetes context (defaults to the one sync remembered)
    #[arg(long)]
    pub kube_context: Option<String>,
}

/// `site.json`: which deployment the folder is for, and what the online side
/// remembers between trips. Holds no secrets and no settings for either
/// machine, since the folder travels between them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SiteState {
    deployment_id: String,
    name: String,
    manager_url: String,
    /// The manager's bundle key, as the online side last saw it. The site
    /// trusts the key pinned in its install, not this one.
    #[serde(default)]
    bundle_key: Option<String>,

    // Online side.
    /// Image blobs the site reported having.
    #[serde(default)]
    site_has: BTreeSet<String>,
    /// Highest telemetry batch the manager has received.
    #[serde(default)]
    uploaded_through: i64,
    /// Release the site last reported running.
    #[serde(default)]
    site_release: Option<String>,

    /// Telemetry acknowledgement in the last bundle built for the site.
    #[serde(default)]
    ack_sent: i64,
}

/// A report file: what the site sends back.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SiteReport {
    deployment_id: String,
    written_at: String,
    installed_release: Option<String>,
    state: serde_json::Value,
    telemetry: Vec<ReportedBatch>,
    /// Batches the Operator dropped to stay within its buffer.
    dropped: u64,
    /// Image blobs in the site's registry for the installed release.
    inventory: BTreeSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportedBatch {
    id: i64,
    signal: String,
    data: String,
}

/// What `installed/<name>/install.json` records, enough to reinstall it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledBundle {
    release_id: String,
    stack_id: String,
    chart: String,
    /// Where each image the chart runs lives in the site registry, by values
    /// key (`runtime.image`, ...).
    chart_images: BTreeMap<String, SiteImage>,
    telemetry_ack: Option<i64>,
    inventory: BTreeSet<String>,
    /// The site registry the images were pushed to. Images are pinned by
    /// digest, so this only says where to find them.
    #[serde(default)]
    registry: Option<String>,
}

/// An image in the site registry, as Helm values name it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SiteImage {
    repository: String,
    /// `<tag>@<digest>`, so the pull is pinned.
    tag: String,
}

/// What one machine remembers for a deployment, outside the folder: the
/// token on the online machine, the install settings inside the site. Kept
/// per machine so an online machine never installs anything, even though
/// the folder has been inside the site.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineState {
    #[serde(default)]
    manager_url: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    insecure_registry: bool,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    values: Vec<PathBuf>,
    #[serde(default)]
    kube_context: Option<String>,
}

/// How the online side reaches the manager.
struct ManagerAccess {
    url: String,
    token: String,
}

pub async fn sync_command(args: SyncArgs) -> Result<()> {
    let (folder, mut site) = open_folder(&args).await?;
    let mut machine = load_machine(&site.deployment_id)?;
    let site_side = site_side_settings(&args, &mut machine)?;
    save_machine(&site.deployment_id, &machine)?;
    let token = match (&machine.manager_url, &machine.token) {
        (Some(url), Some(token)) => Some(ManagerAccess {
            url: url.clone(),
            token: token.clone(),
        }),
        _ => None,
    };

    let online = match &token {
        Some(token) => manager_reachable(&token.url).await,
        None => false,
    };
    if !online && site_side.is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "sync".to_string(),
            message: match token {
                Some(token) => format!(
                    "Can't reach {} and no site registry is set, so there's nothing to do here. Online, check the connection; inside the site, pass --registry the first time.",
                    token.url
                ),
                None => "No manager token on this machine and no site registry set. Online, run `alien-deploy sync --token ... --manager ...` once; inside the site, pass --registry the first time.".to_string(),
            },
        }));
    }

    if online {
        let token = token.as_ref().expect("online implies a stored token");
        send_reports(&folder, &mut site, token, args.dry_run).await?;
        download_update(&folder, &mut site, token, args.full, args.dry_run).await?;
        save_site(&folder, &site)?;
    }
    if let Some(settings) = &site_side {
        install_pending(&folder, &mut site, settings, &args).await?;
        if !args.dry_run {
            write_report(&folder, &site, settings).await?;
        }
        save_site(&folder, &site)?;
        if online {
            let token = token.as_ref().expect("online implies a stored token");
            send_reports(&folder, &mut site, token, false).await?;
            save_site(&folder, &site)?;
        } else if !args.dry_run {
            println!("Copy {} back out.", folder.display());
        }
    } else if !args.dry_run {
        println!("Copy {} to the site.", folder.display());
    }
    Ok(())
}

pub async fn rollback_command(args: RollbackArgs) -> Result<()> {
    let folder = find_folder(args.folder.as_deref())?;
    let site = load_site(&folder)?;
    let machine = load_machine(&site.deployment_id)?;
    let settings = SiteSide::from_state(&machine, args.kube_context.clone())?.ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "registry".to_string(),
            message: "Nothing has been installed from this folder yet".to_string(),
        })
    })?;
    let installed = installed_bundles(&folder)?;
    let (_, previous) = installed.get(1).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "rollback".to_string(),
            message: "There's no earlier installed release to return to".to_string(),
        })
    })?;
    let record = read_installed(previous)?;
    let namespace = settings.namespace(&record.stack_id);
    let trusted_key = pinned_key(&settings, &record.stack_id, &namespace)
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "rollback".to_string(),
                message: "This install trusts no bundle key; install with sync first".to_string(),
            })
        })?;
    // The folder travels: install only what the key this install trusts
    // signed, the same rule a sync follows.
    let manifest = airgap::verify_installed(previous, &trusted_key)
        .await
        .context(config_error(format!(
            "verifying {} before reinstalling it",
            previous.display()
        )))?;
    if manifest.deployment_id != site.deployment_id || manifest.stack_id != record.stack_id {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "rollback".to_string(),
            message: format!("{} is for another deployment", previous.display()),
        }));
    }
    // Images stay where this release was installed from, even if the
    // site's registry setting changed since.
    let registry = record
        .registry
        .clone()
        .unwrap_or_else(|| settings.registry.clone());
    let record = installed_record(&manifest, &registry, record.inventory)?;
    let mut target: serde_json::Value = serde_json::from_slice(
        &std::fs::read(previous.join(airgap::TARGET_FILE))
            .into_alien_error()
            .context(config_error("reading the saved target"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the saved target"))?;
    airgap::rewrite_references(
        &mut target,
        &airgap::site_references(&manifest.images, &registry),
    );
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let target_path = work.path().join("installed-target.json");
    std::fs::write(
        &target_path,
        serde_json::to_vec(&target)
            .into_alien_error()
            .context(config_error("encoding the target"))?,
    )
    .into_alien_error()
    .context(config_error("writing the target"))?;

    println!("Returning {} to {}", site.name, record.release_id);
    let sequence = chrono::Utc::now().timestamp_millis() as u64;
    let ack = current_ack(&settings, &record.stack_id, &namespace).await?;
    helm_upgrade(
        &settings,
        &record,
        &previous.join(&manifest.chart),
        &namespace,
        None,
        &trusted_key,
        &site.deployment_id,
        &site.name,
    )
    .await?;
    write_target(
        &settings,
        &record.stack_id,
        &namespace,
        &target_path,
        sequence,
        ack,
    )
    .await?;
    wait_for_rollout(&settings, &record.stack_id, &namespace, &record.release_id).await?;
    // The rolled-back release is now the newest install.
    std::fs::rename(
        previous,
        folder
            .join(TO_SITE)
            .join(INSTALLED)
            .join(format!("{sequence}-{}", record.release_id)),
    )
    .into_alien_error()
    .context(config_error("recording the rollback"))?;
    write_report(&folder, &site, &settings).await?;
    println!(
        "{} runs {} again. Copy {} out to report it.",
        site.name,
        record.release_id,
        folder.display()
    );
    Ok(())
}

// --- Folder ------------------------------------------------------------------

async fn open_folder(args: &SyncArgs) -> Result<(PathBuf, SiteState)> {
    if let (Some(token), Some(manager)) = (&args.token, &args.manager) {
        let manager = manager.trim_end_matches('/').to_string();
        let whoami: serde_json::Value = manager_get(&manager, token, "/v1/whoami").await?;
        let deployment_id = whoami["scope"]["deploymentId"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: "This token isn't a deployment token. Use the token `alien onboard --airgapped` printed.".to_string(),
                })
            })?
            .to_string();
        let deployment: serde_json::Value =
            manager_get(&manager, token, &format!("/v1/deployments/{deployment_id}")).await?;
        let name = deployment["name"]
            .as_str()
            .unwrap_or(&deployment_id)
            .to_string();
        let mut machine = load_machine(&deployment_id)?;
        machine.manager_url = Some(manager.clone());
        machine.token = Some(token.clone());
        save_machine(&deployment_id, &machine)?;
        let folder = args
            .folder
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{name}-sync")));
        let site = if folder.join(SITE_FILE).exists() {
            let mut site = load_site(&folder)?;
            if site.deployment_id != deployment_id {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "folder".to_string(),
                    message: format!(
                        "{} belongs to another deployment ({})",
                        folder.display(),
                        site.name
                    ),
                }));
            }
            site.manager_url = manager.clone();
            site
        } else {
            for dir in [TO_SITE, FROM_SITE] {
                std::fs::create_dir_all(folder.join(dir))
                    .into_alien_error()
                    .context(config_error("creating the transfer folder"))?;
            }
            println!("Created transfer folder {}", folder.display());
            SiteState {
                deployment_id,
                name,
                manager_url: manager,
                ..Default::default()
            }
        };
        save_site(&folder, &site)?;
        return Ok((folder, site));
    }
    if args.token.is_some() || args.manager.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: "Pass --token and --manager together".to_string(),
        }));
    }
    let folder = find_folder(args.folder.as_deref())?;
    let site = load_site(&folder)?;
    Ok((folder, site))
}

fn find_folder(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(folder) = explicit {
        return Ok(folder.to_path_buf());
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(".")
        .into_alien_error()
        .context(config_error("listing the current directory"))?
    {
        let path = entry
            .into_alien_error()
            .context(config_error("listing the current directory"))?
            .path();
        if path.join(SITE_FILE).is_file() {
            found.push(path);
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(AlienError::new(ErrorData::ValidationError {
            field: "folder".to_string(),
            message: "No transfer folder here. Online, start with `alien-deploy sync --token ... --manager ...`; inside the site, run from the directory you copied the folder into, or pass --folder.".to_string(),
        })),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "folder".to_string(),
            message: format!(
                "Several transfer folders here ({}); pick one with --folder",
                found
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })),
    }
}

fn load_site(folder: &Path) -> Result<SiteState> {
    let bytes = std::fs::read(folder.join(SITE_FILE))
        .into_alien_error()
        .context(config_error(format!(
            "reading {}",
            folder.join(SITE_FILE).display()
        )))?;
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(config_error("parsing site.json"))
}

fn save_site(folder: &Path, site: &SiteState) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(site)
        .into_alien_error()
        .context(config_error("encoding site.json"))?;
    std::fs::write(folder.join(SITE_FILE), bytes)
        .into_alien_error()
        .context(config_error("writing site.json"))
}

fn machine_path(deployment_id: &str) -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "No configuration directory to remember sync settings in".to_string(),
        })
    })?;
    Ok(base
        .join("alien-deploy")
        .join("sync")
        .join(format!("{deployment_id}.json")))
}

fn load_machine(deployment_id: &str) -> Result<MachineState> {
    let path = machine_path(deployment_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(config_error(format!("parsing {}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MachineState::default()),
        Err(e) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to read {}: {e}", path.display()),
        })),
    }
}

/// Saved owner-only: it may hold the deployment token.
fn save_machine(deployment_id: &str, machine: &MachineState) -> Result<()> {
    let path = machine_path(deployment_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .into_alien_error()
            .context(config_error("creating the settings directory"))?;
    }
    let bytes = serde_json::to_vec_pretty(machine)
        .into_alien_error()
        .context(config_error("encoding sync settings"))?;
    alien_core::file_utils::write_secret_file(&path, &bytes)
        .into_alien_error()
        .context(config_error("saving sync settings"))
}

/// Newest first: `(sequence, path)` of `<dir>/<sequence>-<release>[.ext]`.
fn by_sequence(dir: &Path, extension: Option<&str>) -> Result<Vec<(u64, PathBuf)>> {
    let mut entries = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }
    for entry in std::fs::read_dir(dir)
        .into_alien_error()
        .context(config_error(format!("listing {}", dir.display())))?
    {
        let path = entry
            .into_alien_error()
            .context(config_error(format!("listing {}", dir.display())))?
            .path();
        let matches_kind = match extension {
            Some(extension) => path.extension().is_some_and(|e| e == extension),
            None => path.is_dir(),
        };
        if !matches_kind {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if let Some(sequence) = stem.split('-').next().and_then(|s| s.parse().ok()) {
            entries.push((sequence, path));
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(entries)
}

fn installed_bundles(folder: &Path) -> Result<Vec<(u64, PathBuf)>> {
    by_sequence(&folder.join(TO_SITE).join(INSTALLED), None)
}

fn read_installed(dir: &Path) -> Result<InstalledBundle> {
    let bytes = std::fs::read(dir.join("install.json"))
        .into_alien_error()
        .context(config_error(format!("reading {}", dir.display())))?;
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(config_error("parsing install.json"))
}

// --- Online side -------------------------------------------------------------

async fn manager_reachable(manager_url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(format!("{manager_url}/health"))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .into_alien_error()
        .context(config_error("creating an HTTP client"))
}

async fn manager_get<T: serde::de::DeserializeOwned>(
    manager_url: &str,
    token: &str,
    path: &str,
) -> Result<T> {
    let response = http()?
        .get(format!("{manager_url}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .into_alien_error()
        .context(api_failed(format!("GET {path}")))?;
    read_response(response, path).await
}

async fn manager_post<T: serde::de::DeserializeOwned>(
    manager_url: &str,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<T> {
    let response = http()?
        .post(format!("{manager_url}{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .into_alien_error()
        .context(api_failed(format!("POST {path}")))?;
    read_response(response, path).await
}

async fn read_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    path: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value["message"].as_str().map(str::to_string))
            .unwrap_or(body);
        return Err(AlienError::new(ErrorData::ManagerRequestFailed {
            message: format!("{path} returned {status}: {message}"),
        }));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(api_failed(format!("reading the response of {path}")))
}

/// Send the reports waiting in `from-site/`, oldest first. Telemetry the
/// manager already has is left out, so reports that overlap don't duplicate.
async fn send_reports(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    dry_run: bool,
) -> Result<()> {
    let mut reports = by_sequence(&folder.join(FROM_SITE), Some(REPORT_EXTENSION))?;
    reports.reverse();
    for (_, path) in reports {
        let report: SiteReport = serde_json::from_slice(
            &std::fs::read(&path)
                .into_alien_error()
                .context(config_error(format!("reading {}", path.display())))?,
        )
        .into_alien_error()
        .context(config_error(format!("parsing {}", path.display())))?;
        if report.deployment_id != site.deployment_id {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "report".to_string(),
                message: format!("{} is for another deployment", path.display()),
            }));
        }
        let fresh: Vec<&ReportedBatch> = report
            .telemetry
            .iter()
            .filter(|batch| batch.id > site.uploaded_through)
            .collect();
        if dry_run {
            println!(
                "Would send report from {}: {} running {}, {} telemetry batches",
                report.written_at,
                site.name,
                report.installed_release.as_deref().unwrap_or("nothing yet"),
                fresh.len()
            );
            continue;
        }
        let body = serde_json::json!({
            "state": report.state,
            "telemetry": fresh
                .iter()
                .map(|batch| {
                    serde_json::json!({ "id": batch.id, "signal": batch.signal, "data": batch.data })
                })
                .collect::<Vec<_>>(),
        });
        let _: serde_json::Value = manager_post(
            &token.url,
            &token.token,
            &format!("/v1/deployments/{}/status-report", site.deployment_id),
            &body,
        )
        .await?;
        if let Some(through) = report.telemetry.iter().map(|batch| batch.id).max() {
            site.uploaded_through = site.uploaded_through.max(through);
        }
        site.site_has = report.inventory;
        site.site_release = report.installed_release.clone();
        // Saved before the report is removed, so progress is never lost.
        save_site(folder, site)?;
        let dropped = if report.dropped > 0 {
            format!(
                " ({} batches were dropped at the site: its buffer was full)",
                report.dropped
            )
        } else {
            String::new()
        };
        println!(
            "Sent report from {}: {} running {}, {} telemetry batches{dropped}.",
            report.written_at,
            site.name,
            report.installed_release.as_deref().unwrap_or("nothing yet"),
            fresh.len()
        );
        std::fs::remove_file(&path)
            .into_alien_error()
            .context(config_error(format!("removing sent {}", path.display())))?;
    }
    Ok(())
}

/// Download the release the site should run, unless the site already runs it
/// or the folder already carries it.
async fn download_update(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    full: bool,
    dry_run: bool,
) -> Result<()> {
    let manager = &token.url;
    let target: serde_json::Value = manager_get(
        manager,
        &token.token,
        &format!("/v1/deployments/{}/target", site.deployment_id),
    )
    .await?;
    let release_id = target["releaseInfo"]["releaseId"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ManagerRequestFailed {
                message: "The target has no release".to_string(),
            })
        })?
        .to_string();
    let stack_id = target["releaseInfo"]["stack"]["id"]
        .as_str()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ManagerRequestFailed {
                message: "The target has no stack".to_string(),
            })
        })?
        .to_string();

    let pending = by_sequence(&folder.join(TO_SITE), Some(BUNDLE_EXTENSION))?;
    let already_carried = pending.iter().any(|(_, path)| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.ends_with(&format!("-{release_id}")))
    });
    let new_ack = site.uploaded_through > site.ack_sent;
    if already_carried {
        println!("Update {release_id} is already in the folder.");
        return Ok(());
    }
    if site.site_release.as_deref() == Some(release_id.as_str()) && !new_ack {
        println!("{} is up to date ({release_id}).", site.name);
        return Ok(());
    }
    // The site already runs this release: the update only carries the
    // acknowledgement that frees the telemetry the manager received.
    let what = if site.site_release.as_deref() == Some(release_id.as_str()) {
        format!("an acknowledgement of the site's reports ({release_id}, already running)")
    } else {
        format!("update {release_id}")
    };
    if dry_run {
        println!("Would download {what}.");
        return Ok(());
    }

    let sources: serde_json::Value = manager_get(
        manager,
        &token.token,
        &format!(
            "/v1/deployments/{}/bundle-sources?releaseId={release_id}",
            site.deployment_id
        ),
    )
    .await?;
    let manager_info: serde_json::Value = manager_get(manager, &token.token, "/v1/manager").await?;
    let public_host = manager_info["registryHost"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let reachable_host = alien_core::image_rewrite::strip_url_scheme(manager).to_string();
    let manager_access = RegistryAccess {
        auth: RegistryAuth::Basic("token".to_string(), token.token.clone()),
        insecure: manager.starts_with("http://"),
    };
    // Artifacts named by the manager's public host are pulled from the
    // address this machine reaches it at, with the token. Others are public
    // unless the manager says to use the token.
    let source_access = |reference: &str, caller: bool| -> (String, RegistryAccess) {
        let reference = reference.trim_start_matches("oci://");
        let pull = match reference.strip_prefix(&format!("{public_host}/")) {
            Some(rest) if !public_host.is_empty() => format!("{reachable_host}/{rest}"),
            _ => reference.to_string(),
        };
        let access = if caller || pull.starts_with(&format!("{reachable_host}/")) {
            manager_access.clone()
        } else {
            RegistryAccess {
                auth: RegistryAuth::Anonymous,
                insecure: false,
            }
        };
        (pull, access)
    };

    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let dir = work.path();
    println!("Downloading {what} for {}", site.name);
    let chart_reference = sources["chart"]["reference"].as_str().ok_or_else(|| {
        AlienError::new(ErrorData::ManagerRequestFailed {
            message: "bundle-sources has no chart".to_string(),
        })
    })?;
    let (chart_pull, chart_access) = source_access(
        chart_reference,
        sources["chart"]["credentials"].as_str() == Some("caller"),
    );
    let chart = airgap::pull_chart(dir, &chart_pull, &chart_access)
        .await
        .context(config_error("downloading the chart"))?;
    // The chart names the images it runs itself: the Operator and its hooks.
    let chart_images = airgap::chart_images(&dir.join(&chart))
        .context(config_error("reading the chart's images"))?;

    let mut images = airgap::image_references(&target);
    images.extend(chart_images.values().cloned());
    images.sort();
    images.dedup();
    let skip = if full {
        BTreeSet::new()
    } else {
        site.site_has.clone()
    };
    let pulls: Vec<(String, RegistryAccess)> = images
        .iter()
        .map(|image| source_access(image, false))
        .collect();
    let (mut exported, omitted) = airgap::export_images(&dir.join(airgap::OCI_DIR), &pulls, &skip)
        .await
        .context(config_error("copying images"))?;
    for (image, source) in exported.iter_mut().zip(images.iter()) {
        image.source = source.clone();
    }

    std::fs::write(
        dir.join(airgap::TARGET_FILE),
        serde_json::to_vec_pretty(&target)
            .into_alien_error()
            .context(config_error("encoding the target"))?,
    )
    .into_alien_error()
    .context(config_error("writing the target"))?;
    let tools = download_tools(dir, folder, &sources).await?;

    let sequence = chrono::Utc::now().timestamp_millis() as u64;
    let manifest = BundleManifest {
        format_version: airgap::FORMAT_VERSION,
        deployment_id: site.deployment_id.clone(),
        deployment_name: site.name.clone(),
        release_id: release_id.clone(),
        sequence,
        created_at: chrono::Utc::now().to_rfc3339(),
        stack_id,
        images: exported,
        chart_images,
        chart,
        files: airgap::checksums(dir)
            .await
            .context(config_error("checksumming"))?,
        omitted_blobs: omitted.clone(),
        telemetry_ack: (site.uploaded_through > 0).then_some(site.uploaded_through),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .into_alien_error()
        .context(config_error("encoding the manifest"))?;
    std::fs::write(dir.join(airgap::MANIFEST_FILE), &manifest_bytes)
        .into_alien_error()
        .context(config_error("writing the manifest"))?;
    let signature: serde_json::Value = manager_post(
        manager,
        &token.token,
        &format!("/v1/deployments/{}/bundle-signature", site.deployment_id),
        &serde_json::json!({
            "manifest": base64::engine::general_purpose::STANDARD.encode(&manifest_bytes),
        }),
    )
    .await?;
    let signature_text = signature["signature"].as_str().ok_or_else(|| {
        AlienError::new(ErrorData::ManagerRequestFailed {
            message: "The manager returned no signature".to_string(),
        })
    })?;
    std::fs::write(dir.join(airgap::SIGNATURE_FILE), signature_text)
        .into_alien_error()
        .context(config_error("writing the signature"))?;
    site.bundle_key = signature["publicKey"].as_str().map(str::to_string);
    site.ack_sent = site.uploaded_through;

    // A newer update replaces bundles that haven't been carried in yet.
    for (_, path) in &pending {
        std::fs::remove_file(path)
            .into_alien_error()
            .context(config_error("removing a superseded bundle"))?;
    }
    let file = folder
        .join(TO_SITE)
        .join(format!("{sequence}-{release_id}.{BUNDLE_EXTENSION}"));
    airgap::pack(dir, &file).context(config_error("writing the bundle"))?;
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let mut delta = if omitted.is_empty() {
        String::new()
    } else {
        format!(", {} layers the site already has left out", omitted.len())
    };
    if tools == Tools::Unavailable {
        delta.push_str(", without alien-deploy builds");
    }
    println!(
        "Downloaded {what} ({:.1} MB, signed{delta}).",
        size as f64 / 1_000_000.0
    );
    Ok(())
}

/// Lists the builds in a `tools/` directory, so a folder that already carries
/// them isn't sent them again.
const TOOLS_SOURCES_FILE: &str = "sources.json";

#[derive(Debug, PartialEq, Eq)]
enum Tools {
    Included,
    /// The folder already carries these builds.
    AlreadyThere,
    /// The releases server didn't have them; the update goes without.
    Unavailable,
}

/// Put the `alien-deploy` builds bundle-sources lists into `<dir>/tools/`,
/// unless the folder already carries them. A missing build doesn't hold up
/// the update.
async fn download_tools(dir: &Path, folder: &Path, sources: &serde_json::Value) -> Result<Tools> {
    let downloads = sources["deployCli"].as_array().cloned().unwrap_or_default();
    if downloads.is_empty() {
        return Ok(Tools::Unavailable);
    }
    let listed = serde_json::to_vec_pretty(&downloads)
        .into_alien_error()
        .context(config_error("encoding the tool list"))?;
    if std::fs::read(folder.join(TOOLS_DIR).join(TOOLS_SOURCES_FILE)).is_ok_and(|had| had == listed)
    {
        return Ok(Tools::AlreadyThere);
    }
    let client = http()?;
    let mut builds = Vec::new();
    for download in &downloads {
        let (Some(platform), Some(url)) = (download["platform"].as_str(), download["url"].as_str())
        else {
            continue;
        };
        let response = match client.get(url).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                println!(
                    "Couldn't include alien-deploy for {platform}: {url} returned {}",
                    response.status()
                );
                return Ok(Tools::Unavailable);
            }
            Err(e) => {
                println!("Couldn't include alien-deploy for {platform}: {e}");
                return Ok(Tools::Unavailable);
            }
        };
        let bytes = response
            .bytes()
            .await
            .into_alien_error()
            .context(config_error(format!(
                "downloading alien-deploy for {platform}"
            )))?;
        builds.push((format!("alien-deploy-{platform}"), bytes));
    }
    std::fs::create_dir_all(dir.join(TOOLS_DIR))
        .into_alien_error()
        .context(config_error("creating the tools directory"))?;
    for (name, bytes) in builds {
        std::fs::write(dir.join(TOOLS_DIR).join(name), bytes)
            .into_alien_error()
            .context(config_error("writing alien-deploy"))?;
    }
    std::fs::write(dir.join(TOOLS_DIR).join(TOOLS_SOURCES_FILE), listed)
        .into_alien_error()
        .context(config_error("writing the tool list"))?;
    Ok(Tools::Included)
}

/// Copy the `alien-deploy` builds a verified bundle carries into the folder.
fn copy_tools(bundle_dir: &Path, folder: &Path) -> Result<()> {
    let tools = bundle_dir.join(TOOLS_DIR);
    if !tools.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(folder.join(TOOLS_DIR))
        .into_alien_error()
        .context(config_error("creating the tools directory"))?;
    for entry in std::fs::read_dir(&tools)
        .into_alien_error()
        .context(config_error("reading the bundle's tools"))?
    {
        let path = entry
            .into_alien_error()
            .context(config_error("reading the bundle's tools"))?
            .path();
        let Some(name) = path.file_name() else {
            continue;
        };
        let destination = folder.join(TOOLS_DIR).join(name);
        std::fs::copy(&path, &destination)
            .into_alien_error()
            .context(config_error("copying alien-deploy"))?;
        #[cfg(unix)]
        if name.to_string_lossy().starts_with("alien-deploy-") {
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
                .into_alien_error()
                .context(config_error("making alien-deploy executable"))?;
        }
    }
    Ok(())
}

// --- Site side ---------------------------------------------------------------

/// Settings for installing inside the site.
#[derive(Debug, Clone)]
struct SiteSide {
    registry: String,
    registry_credentials: Option<(String, String)>,
    insecure_registry: bool,
    namespace: Option<String>,
    values: Vec<PathBuf>,
    kube_context: Option<String>,
}

impl SiteSide {
    fn from_state(machine: &MachineState, kube_context: Option<String>) -> Result<Option<Self>> {
        Ok(machine.registry.clone().map(|registry| SiteSide {
            registry,
            registry_credentials: None,
            insecure_registry: machine.insecure_registry,
            namespace: machine.namespace.clone(),
            values: machine.values.clone(),
            kube_context: kube_context.or_else(|| machine.kube_context.clone()),
        }))
    }

    fn namespace(&self, stack_id: &str) -> String {
        self.namespace
            .clone()
            .unwrap_or_else(|| stack_id.to_string())
    }

    fn registry_access(&self) -> RegistryAccess {
        RegistryAccess {
            auth: match &self.registry_credentials {
                Some((user, password)) => RegistryAuth::Basic(user.clone(), password.clone()),
                None => RegistryAuth::Anonymous,
            },
            insecure: self.insecure_registry,
        }
    }
}

/// Site settings from flags, remembered on this machine (except registry
/// credentials). `None` on a machine that installs nothing.
fn site_side_settings(args: &SyncArgs, site: &mut MachineState) -> Result<Option<SiteSide>> {
    if let Some(registry) = &args.registry {
        site.registry = Some(registry.clone());
        site.insecure_registry = args.insecure_registry;
    }
    if args.namespace.is_some() {
        site.namespace = args.namespace.clone();
    }
    if !args.values.is_empty() {
        let mut values = Vec::new();
        for path in &args.values {
            values.push(
                std::fs::canonicalize(path)
                    .into_alien_error()
                    .context(config_error(format!("finding {}", path.display())))?,
            );
        }
        site.values = values;
    }
    if args.kube_context.is_some() {
        site.kube_context = args.kube_context.clone();
    }
    let Some(mut settings) = SiteSide::from_state(site, None)? else {
        return Ok(None);
    };
    settings.registry_credentials = match (&args.registry_username, &args.registry_password) {
        (Some(user), Some(password)) => Some((user.clone(), password.clone())),
        (None, None) => None,
        _ => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "registry-username".to_string(),
                message: "Pass --registry-username and --registry-password together".to_string(),
            }))
        }
    };
    Ok(Some(settings))
}

async fn install_pending(
    folder: &Path,
    site: &mut SiteState,
    settings: &SiteSide,
    args: &SyncArgs,
) -> Result<()> {
    let pending = by_sequence(&folder.join(TO_SITE), Some(BUNDLE_EXTENSION))?;
    let Some((_, bundle)) = pending.first() else {
        println!("No update to install.");
        return Ok(());
    };
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    airgap::unpack(bundle, work.path()).context(config_error("reading the bundle"))?;
    // The release name only locates the key this install trusts; nothing
    // else from the manifest is used before its signature is checked.
    let unverified: BundleManifest = serde_json::from_slice(
        &std::fs::read(work.path().join(airgap::MANIFEST_FILE))
            .into_alien_error()
            .context(config_error("reading the bundle manifest"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the bundle manifest"))?;
    if unverified.deployment_id != site.deployment_id {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle".to_string(),
            message: format!("{} is for another deployment", bundle.display()),
        }));
    }
    let namespace = settings.namespace(&unverified.stack_id);
    let trusted_key = match (
        pinned_key(settings, &unverified.stack_id, &namespace).await?,
        args.trusted_key.as_deref().map(str::trim),
    ) {
        (Some(pinned), Some(given)) if pinned != given => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "trusted-key".to_string(),
                message: format!(
                    "This install already trusts {pinned}; updates must be signed with that key"
                ),
            }))
        }
        (Some(pinned), _) => pinned,
        (None, Some(given)) => given.to_string(),
        (None, None) => {
            let seen = site
                .bundle_key
                .as_deref()
                .map(|key| {
                    format!(
                        " The folder says the vendor's key is {key}; confirm it with them first."
                    )
                })
                .unwrap_or_default();
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "trusted-key".to_string(),
                message: format!(
                    "The first install needs --trusted-key, the bundle key the vendor gave you.{seen}"
                ),
            }));
        }
    };
    let manifest = airgap::verify(work.path(), &trusted_key)
        .await
        .context(config_error("verifying the update"))?;
    let installed = installed_release(settings, &manifest.stack_id, &namespace).await?;

    if args.dry_run {
        println!(
            "Update {} verified (signed by {}).",
            manifest.release_id,
            short_key(&trusted_key)
        );
        println!(
            "  Release   {} → {}",
            installed.as_deref().unwrap_or("not installed"),
            manifest.release_id
        );
        println!(
            "  Images    {} to push to {} ({} layers already there)",
            manifest.images.len(),
            settings.registry,
            manifest.omitted_blobs.len()
        );
        println!("  Chart     {}", manifest.chart);
        print_chart_diff(
            settings,
            &manifest,
            &work.path().join(&manifest.chart),
            &namespace,
        )
        .await?;
        return Ok(());
    }

    // Credentials passed now replace the ones the install stored; otherwise
    // the stored ones push the images.
    let passed_credentials = settings.registry_credentials.is_some();
    let mut settings = settings.clone();
    if !passed_credentials {
        settings.registry_credentials =
            stored_pull_credentials(&settings, &manifest.stack_id, &namespace).await?;
    }
    let settings = &settings;
    println!(
        "Installing update {} ({} images → {})",
        manifest.release_id,
        manifest.images.len(),
        settings.registry
    );
    let mapping = airgap::import_images(
        &work.path().join(airgap::OCI_DIR),
        &manifest.images,
        &settings.registry,
        &settings.registry_access(),
    )
    .await
    .context(config_error("pushing images"))?;
    let mut target: serde_json::Value = serde_json::from_slice(
        &std::fs::read(work.path().join(airgap::TARGET_FILE))
            .into_alien_error()
            .context(config_error("reading the target"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the target"))?;
    airgap::rewrite_references(&mut target, &mapping);
    let record = installed_record(
        &manifest,
        &settings.registry,
        airgap::blob_inventory(&work.path().join(airgap::OCI_DIR), &manifest.images)
            .await
            .context(config_error("listing the update's layers"))?,
    )?;

    let pull_secret = match settings
        .registry_credentials
        .as_ref()
        .filter(|_| passed_credentials)
    {
        Some((username, password)) => Some(
            write_pull_secret(settings, &manifest.stack_id, &namespace, username, password).await?,
        ),
        None => None,
    };
    helm_upgrade(
        settings,
        &record,
        &work.path().join(&manifest.chart),
        &namespace,
        pull_secret.as_deref(),
        &trusted_key,
        &site.deployment_id,
        &site.name,
    )
    .await?;
    let target_path = work.path().join("installed-target.json");
    std::fs::write(
        &target_path,
        serde_json::to_vec(&target)
            .into_alien_error()
            .context(config_error("encoding the target"))?,
    )
    .into_alien_error()
    .context(config_error("writing the target"))?;
    let ack = match manifest.telemetry_ack {
        Some(ack) => ack,
        None => current_ack(settings, &manifest.stack_id, &namespace).await?,
    };
    write_target(
        settings,
        &manifest.stack_id,
        &namespace,
        &target_path,
        manifest.sequence,
        ack,
    )
    .await?;
    wait_for_rollout(
        settings,
        &manifest.stack_id,
        &namespace,
        &manifest.release_id,
    )
    .await?;

    // Keep what a rollback needs, as signed: the manifest and its
    // signature, the target and the chart. A rollback verifies them again
    // before installing anything, since the folder leaves the site.
    let keep = folder
        .join(TO_SITE)
        .join(INSTALLED)
        .join(format!("{}-{}", manifest.sequence, manifest.release_id));
    std::fs::create_dir_all(keep.join(airgap::CHART_DIR))
        .into_alien_error()
        .context(config_error("saving the installed update"))?;
    for file in [
        airgap::MANIFEST_FILE,
        airgap::SIGNATURE_FILE,
        airgap::TARGET_FILE,
        manifest.chart.as_str(),
    ] {
        std::fs::copy(work.path().join(file), keep.join(file))
            .into_alien_error()
            .context(config_error(format!("saving the installed {file}")))?;
    }
    std::fs::write(
        keep.join("install.json"),
        serde_json::to_vec_pretty(&record)
            .into_alien_error()
            .context(config_error("encoding install.json"))?,
    )
    .into_alien_error()
    .context(config_error("writing install.json"))?;
    for (_, old) in installed_bundles(folder)?.into_iter().skip(KEEP_INSTALLED) {
        std::fs::remove_dir_all(old)
            .into_alien_error()
            .context(config_error("pruning old installs"))?;
    }
    for (_, path) in &pending {
        std::fs::remove_file(path)
            .into_alien_error()
            .context(config_error("removing the installed bundle"))?;
    }
    copy_tools(work.path(), folder)?;
    println!(
        "Installed update {}. All services running.",
        manifest.release_id
    );
    Ok(())
}

/// How an update is installed at this site, from its verified manifest:
/// where each image the chart runs lives in the site registry.
fn installed_record(
    manifest: &BundleManifest,
    registry: &str,
    inventory: BTreeSet<String>,
) -> Result<InstalledBundle> {
    let mut chart_images = BTreeMap::new();
    for (key, source) in &manifest.chart_images {
        let image = manifest
            .images
            .iter()
            .find(|image| &image.source == source)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "bundle".to_string(),
                    message: format!("The update doesn't carry {source} ({key})"),
                })
            })?;
        chart_images.insert(
            key.clone(),
            SiteImage {
                repository: format!("{}/{}", registry.trim_end_matches('/'), image.repository),
                tag: format!(
                    "{}@{}",
                    image.tag.as_deref().unwrap_or("latest"),
                    image.digest
                ),
            },
        );
    }
    if !chart_images.contains_key(airgap::CHART_IMAGE_VALUES[0]) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "bundle".to_string(),
            message: "The update has no Operator image".to_string(),
        }));
    }
    Ok(InstalledBundle {
        release_id: manifest.release_id.clone(),
        stack_id: manifest.stack_id.clone(),
        chart: manifest.chart.clone(),
        chart_images,
        telemetry_ack: manifest.telemetry_ack,
        inventory,
        registry: Some(registry.to_string()),
    })
}

fn short_key(key: &str) -> String {
    key.chars().take(20).collect::<String>() + "…"
}

/// The key the install pinned, from its Helm values. `None` before the first
/// install.
async fn pinned_key(settings: &SiteSide, release: &str, namespace: &str) -> Result<Option<String>> {
    let mut cmd = Command::new("helm");
    cmd.args(["get", "values", release, "-n", namespace, "-o", "json"]);
    if let Some(context) = &settings.kube_context {
        cmd.arg("--kube-context").arg(context);
    }
    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(config_error("running helm get values; is helm installed?"))?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("not found") {
            return Ok(None);
        }
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "helm get values failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    let values: serde_json::Value = serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(config_error("parsing the release's values"))?;
    Ok(values["airgapped"]["bundleSigningKey"]
        .as_str()
        .filter(|key| !key.is_empty())
        .map(str::to_string))
}

/// The telemetry acknowledgement the Operator last received, from the target
/// Secret. Everything up to it has been freed.
async fn current_ack(settings: &SiteSide, stack_id: &str, namespace: &str) -> Result<i64> {
    let mut cmd = kubectl(settings.kube_context.as_deref());
    cmd.args([
        "get",
        "secret",
        &format!("{stack_id}-airgap-target"),
        "-n",
        namespace,
        "-o",
        "jsonpath={.data.telemetry-ack}",
        "--ignore-not-found",
    ]);
    let encoded = String::from_utf8_lossy(&run(cmd, "kubectl get secret").await?)
        .trim()
        .to_string();
    if encoded.is_empty() {
        return Ok(0);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .into_alien_error()
        .context(config_error("decoding the telemetry acknowledgement"))?;
    String::from_utf8_lossy(&decoded)
        .trim()
        .parse()
        .into_alien_error()
        .context(config_error("parsing the telemetry acknowledgement"))
}

/// Release the Operator last reported running, from its status Secret.
async fn installed_release(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<Option<String>> {
    Ok(read_status(settings, stack_id, namespace)
        .await?
        .and_then(|(status, _)| current_release(&status)))
}

fn current_release(status: &serde_json::Value) -> Option<String> {
    status["state"]["currentRelease"]["releaseId"]
        .as_str()
        .map(str::to_string)
}

/// The status Secret: the state the Operator last wrote, and the token that
/// authorizes telemetry export.
async fn read_status(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<Option<(serde_json::Value, String)>> {
    let mut cmd = kubectl(settings.kube_context.as_deref());
    cmd.args([
        "get",
        "secret",
        &format!("{stack_id}-airgap-status"),
        "-n",
        namespace,
        "-o",
        "json",
        "--ignore-not-found",
    ]);
    let output = run(cmd, "kubectl get secret").await?;
    if output.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let secret: serde_json::Value = serde_json::from_slice(&output)
        .into_alien_error()
        .context(config_error("parsing the status Secret"))?;
    let field = |key: &str| -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(secret["data"][key].as_str().unwrap_or_default())
            .into_alien_error()
            .context(config_error(format!("decoding {key} in the status Secret")))
    };
    let status: serde_json::Value = serde_json::from_slice(&field("status.json")?)
        .into_alien_error()
        .context(config_error("parsing status.json"))?;
    let token = String::from_utf8(field("export-token")?)
        .into_alien_error()
        .context(config_error("reading the export token"))?;
    Ok(Some((status, token)))
}

async fn wait_for_rollout(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
    release_id: &str,
) -> Result<()> {
    println!("Waiting for {release_id} to roll out");
    let started = std::time::Instant::now();
    loop {
        if let Some((status, _)) = read_status(settings, stack_id, namespace).await? {
            let state = &status["state"];
            let running = state["status"].as_str() == Some("running");
            if running && current_release(&status).as_deref() == Some(release_id) {
                return Ok(());
            }
            if state["status"]
                .as_str()
                .is_some_and(|s| s.ends_with("failed"))
            {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "The rollout of {release_id} failed: {}. `alien-deploy rollback` returns to the previous release.",
                        state["error"]["message"].as_str().unwrap_or("see the Operator's logs")
                    ),
                }));
            }
        }
        if started.elapsed() > ROLLOUT_TIMEOUT {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "{release_id} hasn't finished rolling out after {} minutes. Check the pods in {namespace}; run sync again to report.",
                    ROLLOUT_TIMEOUT.as_secs() / 60
                ),
            }));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Write a report into `from-site/`: the deployment's state, the telemetry
/// the Operator buffered since the last acknowledgement, and the layers the
/// site holds.
async fn write_report(folder: &Path, site: &SiteState, settings: &SiteSide) -> Result<()> {
    let Some(latest) = installed_bundles(folder)?.into_iter().next() else {
        return Ok(());
    };
    let record = read_installed(&latest.1)?;
    let namespace = settings.namespace(&record.stack_id);
    let Some((status, export_token)) = read_status(settings, &record.stack_id, &namespace).await?
    else {
        return Ok(());
    };
    let after = current_ack(settings, &record.stack_id, &namespace).await?;
    let (telemetry, dropped) =
        export_telemetry(settings, &record.stack_id, &namespace, &export_token, after).await?;
    let report = SiteReport {
        deployment_id: site.deployment_id.clone(),
        written_at: chrono::Utc::now().to_rfc3339(),
        installed_release: current_release(&status),
        state: status["state"].clone(),
        telemetry,
        dropped,
        inventory: record.inventory,
    };
    let sequence = chrono::Utc::now().timestamp_millis();
    let path = folder
        .join(FROM_SITE)
        .join(format!("{sequence}-report.{REPORT_EXTENSION}"));
    std::fs::create_dir_all(folder.join(FROM_SITE))
        .into_alien_error()
        .context(config_error("creating from-site"))?;
    std::fs::write(
        &path,
        serde_json::to_vec(&report)
            .into_alien_error()
            .context(config_error("encoding the report"))?,
    )
    .into_alien_error()
    .context(config_error("writing the report"))?;
    println!(
        "Wrote a report for the vendor ({} telemetry batches).",
        report.telemetry.len()
    );
    Ok(())
}

/// Read the Operator's buffered telemetry after `after` through a
/// port-forward to its OTLP port.
async fn export_telemetry(
    settings: &SiteSide,
    release: &str,
    namespace: &str,
    token: &str,
    after: i64,
) -> Result<(Vec<ReportedBatch>, u64)> {
    let mut port_cmd = kubectl(settings.kube_context.as_deref());
    port_cmd.args([
        "get",
        "deployment",
        release,
        "-n",
        namespace,
        "-o",
        "jsonpath={.spec.template.spec.containers[0].ports[?(@.name==\"otlp\")].containerPort}",
    ]);
    let port = String::from_utf8_lossy(&run(port_cmd, "kubectl get deployment").await?)
        .trim()
        .to_string();
    if port.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("The Operator deployment {namespace}/{release} has no otlp port"),
        }));
    }
    let mut forward = kubectl(settings.kube_context.as_deref());
    forward
        .args([
            "port-forward",
            &format!("deployment/{release}"),
            &format!(":{port}"),
            "-n",
            namespace,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = forward
        .spawn()
        .into_alien_error()
        .context(config_error("starting kubectl port-forward"))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "kubectl port-forward has no output".to_string(),
        })
    })?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let local_port = tokio::time::timeout(Duration::from_secs(20), async {
        while let Ok(Some(line)) = lines.next_line().await {
            // "Forwarding from 127.0.0.1:54321 -> 8080"
            if let Some(port) = line
                .strip_prefix("Forwarding from 127.0.0.1:")
                .and_then(|rest| rest.split_whitespace().next())
            {
                return Some(port.to_string());
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
    .ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("kubectl port-forward to {namespace}/{release} didn't start"),
        })
    })?;

    let client = http()?;
    let mut batches = Vec::new();
    let mut read_through = after;
    let dropped = loop {
        let response = client
            .get(format!(
                "http://127.0.0.1:{local_port}/airgap/telemetry?after={read_through}&limit=200"
            ))
            .bearer_auth(token)
            .send()
            .await
            .into_alien_error()
            .context(config_error("reading buffered telemetry"))?;
        let page: serde_json::Value = read_response(response, "/airgap/telemetry").await?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            break page["dropped"].as_u64().unwrap_or(0);
        }
        for item in items {
            let batch: ReportedBatch = serde_json::from_value(item)
                .into_alien_error()
                .context(config_error("parsing a telemetry batch"))?;
            read_through = read_through.max(batch.id);
            batches.push(batch);
        }
    };
    drop(child);
    Ok((batches, dropped))
}

// --- Dry run -----------------------------------------------------------------

/// Resources the new chart adds, removes or changes against the install.
async fn print_chart_diff(
    settings: &SiteSide,
    manifest: &BundleManifest,
    chart: &Path,
    namespace: &str,
) -> Result<()> {
    let mut current = Command::new("helm");
    current.args(["get", "manifest", &manifest.stack_id, "-n", namespace]);
    let mut values = Command::new("helm");
    values.args([
        "get",
        "values",
        &manifest.stack_id,
        "-n",
        namespace,
        "-o",
        "yaml",
    ]);
    if let Some(context) = &settings.kube_context {
        current.arg("--kube-context").arg(context);
        values.arg("--kube-context").arg(context);
    }
    let current = current
        .output()
        .await
        .into_alien_error()
        .context(config_error("running helm get manifest"))?;
    if !current.status.success() {
        println!("  Resources everything is new (not installed yet)");
        return Ok(());
    }
    let values = run(values, "helm get values").await?;
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let values_path = work.path().join("values.yaml");
    std::fs::write(&values_path, values)
        .into_alien_error()
        .context(config_error("writing the current values"))?;
    let mut template = Command::new("helm");
    template
        .args(["template", &manifest.stack_id])
        .arg(chart)
        .args(["-n", namespace, "-f"])
        .arg(&values_path);
    let rendered = run(template, "helm template").await?;
    let before = resources(&String::from_utf8_lossy(&current.stdout));
    let after = resources(&String::from_utf8_lossy(&rendered));
    let mut changes = Vec::new();
    for (name, body) in &after {
        match before.get(name) {
            None => changes.push(format!("+ {name}")),
            Some(old) if old != body => changes.push(format!("~ {name}")),
            _ => {}
        }
    }
    for name in before.keys() {
        if !after.contains_key(name) {
            changes.push(format!("- {name}"));
        }
    }
    if changes.is_empty() {
        println!("  Resources no changes to the chart's resources");
    } else {
        println!("  Resources");
        for change in changes {
            println!("    {change}");
        }
    }
    Ok(())
}

/// `Kind/name` → document, for a multi-document YAML stream.
fn resources(yaml: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for document in yaml.split("\n---") {
        let mut kind = None;
        let mut name = None;
        let mut in_metadata = false;
        for line in document.lines() {
            if let Some(value) = line.strip_prefix("kind: ") {
                kind = Some(value.trim().to_string());
            }
            if line.starts_with("metadata:") {
                in_metadata = true;
                continue;
            }
            if in_metadata {
                if let Some(value) = line.strip_prefix("  name: ") {
                    name = Some(value.trim().trim_matches('"').to_string());
                    in_metadata = false;
                } else if !line.starts_with(' ') {
                    in_metadata = false;
                }
            }
        }
        if let (Some(kind), Some(name)) = (kind, name) {
            // Comments carry the template path, not content.
            let body: String = document
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n");
            out.insert(format!("{kind}/{name}"), body);
        }
    }
    out
}

// --- Kubernetes helpers ------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn helm_upgrade(
    settings: &SiteSide,
    record: &InstalledBundle,
    chart: &Path,
    namespace: &str,
    pull_secret: Option<&str>,
    trusted_key: &str,
    deployment_id: &str,
    deployment_name: &str,
) -> Result<()> {
    let mut cmd = Command::new("helm");
    cmd.arg("upgrade")
        .arg("--install")
        .arg(&record.stack_id)
        .arg(chart)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("--reset-then-reuse-values")
        .args(["--set", "airgapped.enabled=true"])
        .args([
            "--set-string",
            &format!("management.deploymentId={deployment_id}"),
        ])
        .args([
            "--set-string",
            &format!("management.name={deployment_name}"),
        ])
        .args(["--set", "runtime.image.pullWithManagementToken=false"])
        .args(["--set", "runtime.image.selfUpdate=false"])
        // The Operator buffers container logs for the next report.
        .args(["--set", "logCollector.enabled=true"])
        .args(["--set-string", "logCollector.mode=podApi"])
        .args(["--set", "tunnel.enabled=false"])
        .args([
            "--set-string",
            &format!("airgapped.bundleSigningKey={trusted_key}"),
        ])
        .args(["--wait", "--timeout", "300s"]);
    for (key, image) in &record.chart_images {
        cmd.args([
            "--set-string",
            &format!("{key}.repository={}", image.repository),
        ])
        .args(["--set-string", &format!("{key}.tag={}", image.tag)]);
    }
    if let Some(secret) = pull_secret {
        cmd.args([
            "--set-string",
            &format!("runtime.imagePullSecrets[0].name={secret}"),
        ]);
    }
    for values in &settings.values {
        cmd.arg("-f").arg(values);
    }
    if let Some(context) = &settings.kube_context {
        cmd.arg("--kube-context").arg(context);
    }
    run(cmd, "helm upgrade --install").await.map(|_| ())
}

/// Store the registry credentials as an image pull Secret in `namespace` and
/// return its name. The chart attaches it to the Operator and to the
/// application's ServiceAccounts.
async fn write_pull_secret(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let secret_name = pull_secret_name(stack_id);
    let registry_host = registry_host(&settings.registry);
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let config = serde_json::json!({
        "auths": { registry_host: { "username": username, "password": password, "auth": auth } }
    });
    let dir = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let config_path = dir.path().join("config.json");
    alien_core::file_utils::write_secret_file(&config_path, config.to_string().as_bytes())
        .into_alien_error()
        .context(config_error("writing registry credentials"))?;

    let mut namespace_yaml = kubectl(settings.kube_context.as_deref());
    namespace_yaml.args([
        "create",
        "namespace",
        namespace,
        "--dry-run=client",
        "-o",
        "yaml",
    ]);
    let namespace_yaml = run(namespace_yaml, "kubectl create namespace").await?;
    let mut secret_yaml = kubectl(settings.kube_context.as_deref());
    secret_yaml
        .args(["create", "secret", "generic", &secret_name, "-n", namespace])
        .args(["--type", "kubernetes.io/dockerconfigjson"])
        .arg(format!(
            "--from-file=.dockerconfigjson={}",
            config_path.display()
        ))
        .args(["--dry-run=client", "-o", "yaml"]);
    let secret_yaml = run(secret_yaml, "kubectl create secret").await?;

    let apply_path = dir.path().join("pull-secret.yaml");
    let mut documents = namespace_yaml;
    documents.extend_from_slice(b"\n---\n");
    documents.extend_from_slice(&secret_yaml);
    alien_core::file_utils::write_secret_file(&apply_path, &documents)
        .into_alien_error()
        .context(config_error("writing the pull Secret manifest"))?;
    let mut apply = kubectl(settings.kube_context.as_deref());
    apply.args(["apply", "-f"]).arg(&apply_path);
    run(apply, "kubectl apply").await?;
    Ok(secret_name)
}

/// The registry credentials an earlier install stored in its pull Secret,
/// for the site registry's host.
async fn stored_pull_credentials(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<Option<(String, String)>> {
    let mut cmd = kubectl(settings.kube_context.as_deref());
    cmd.args([
        "get",
        "secret",
        &pull_secret_name(stack_id),
        "-n",
        namespace,
        "-o",
        "jsonpath={.data.\\.dockerconfigjson}",
        "--ignore-not-found",
    ]);
    let encoded = String::from_utf8_lossy(&run(cmd, "kubectl get secret").await?)
        .trim()
        .to_string();
    if encoded.is_empty() {
        return Ok(None);
    }
    let config: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .into_alien_error()
            .context(config_error("decoding the stored registry credentials"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the stored registry credentials"))?;
    let auth = &config["auths"][registry_host(&settings.registry)];
    Ok(
        match (auth["username"].as_str(), auth["password"].as_str()) {
            (Some(username), Some(password)) => Some((username.to_string(), password.to_string())),
            _ => None,
        },
    )
}

fn pull_secret_name(stack_id: &str) -> String {
    format!("{stack_id}-registry-credentials")
}

fn registry_host(registry: &str) -> &str {
    registry.split('/').next().unwrap_or(registry)
}

/// Hand the Operator its target through the target Secret, with the bundle's
/// sequence (higher wins) and the telemetry acknowledgement.
async fn write_target(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
    target_path: &Path,
    sequence: u64,
    telemetry_ack: i64,
) -> Result<()> {
    let secret_name = format!("{stack_id}-airgap-target");
    let mut create = kubectl(settings.kube_context.as_deref());
    create
        .args(["create", "secret", "generic", &secret_name, "-n", namespace])
        .arg(format!("--from-file=target.json={}", target_path.display()))
        .arg(format!("--from-literal=sequence={sequence}"))
        .arg(format!("--from-literal=telemetry-ack={telemetry_ack}"))
        .args(["--dry-run=client", "-o", "yaml"]);
    let yaml = run(create, "kubectl create secret").await?;
    let dir = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let apply_path = dir.path().join("secret.yaml");
    std::fs::write(&apply_path, yaml)
        .into_alien_error()
        .context(config_error("writing the Secret manifest"))?;
    let mut apply = kubectl(settings.kube_context.as_deref());
    apply
        .args(["apply", "-n", namespace, "-f"])
        .arg(&apply_path);
    run(apply, "kubectl apply").await.map(|_| ())
}

fn kubectl(context: Option<&str>) -> Command {
    let mut cmd = Command::new("kubectl");
    if let Some(context) = context {
        cmd.arg("--context").arg(context);
    }
    cmd
}

async fn run(mut cmd: Command, what: &str) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(config_error(format!("running {what}; is it installed?")))?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{what} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    Ok(output.stdout)
}

fn config_error(message: impl Into<String>) -> ErrorData {
    ErrorData::ConfigurationError {
        message: message.into(),
    }
}

fn api_failed(message: impl Into<String>) -> ErrorData {
    ErrorData::ManagerRequestFailed {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_are_keyed_by_kind_and_name() {
        let yaml = "---\n# Source: x/templates/a.yaml\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a\n  labels:\n    x: y\ndata:\n  k: v\n---\nkind: Deployment\nmetadata:\n  namespace: n\n  name: \"api\"\nspec: {}\n";
        let found = resources(yaml);
        assert_eq!(
            found.keys().cloned().collect::<Vec<_>>(),
            ["ConfigMap/a", "Deployment/api"]
        );
        assert!(!found["ConfigMap/a"].contains("# Source"));
    }

    #[test]
    fn folders_list_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "100-rel_a.bundle",
            "300-rel_c.bundle",
            "200-rel_b.bundle",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let found: Vec<u64> = by_sequence(dir.path(), Some(BUNDLE_EXTENSION))
            .unwrap()
            .into_iter()
            .map(|(sequence, _)| sequence)
            .collect();
        assert_eq!(found, [300, 200, 100]);
    }
}
