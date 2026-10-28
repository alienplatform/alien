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
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use alien_cli_common::airgap::{self, BundleManifest, RegistryAccess};
use alien_core::{embedded_config::DeployCliConfig, DeletionPolicy, ResourceHeartbeat};
use alien_error::{AlienError, Context, IntoAlienError};
use base64::Engine as _;
use clap::Parser;
use oci_client::secrets::RegistryAuth;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncBufReadExt as _, process::Command};
use uuid::Uuid;

use crate::error::{ErrorData, Result};

const SITE_FILE: &str = "site.json";
const SYNC_SECRET_MANAGER: &str = "alien-deploy-sync";
const TO_SITE: &str = "to-site";
const FROM_SITE: &str = "from-site";
const INSTALLED: &str = "installed";
const BUNDLE_EXTENSION: &str = "bundle";
const REPORT_EXTENSION: &str = "report";
/// Installed bundles kept for rollback.
const KEEP_INSTALLED: usize = 3;
const ROLLOUT_TIMEOUT: Duration = Duration::from_secs(600);
const SECRET_STATUS_TIMEOUT: Duration = Duration::from_secs(120);
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
    /// Manager URL from the vendor. Branded CLIs discover it automatically.
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
    /// Helm values for image selection online and installation at the site. Remembered at the site.
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
    #[serde(default)]
    deleted: bool,

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
    #[serde(default)]
    heartbeats: Vec<ResourceHeartbeat>,
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
    /// Only a completed Running observation makes an install a rollback target.
    /// Older records are unknown until the site observes their running release.
    #[serde(default)]
    rollout_succeeded: bool,
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
    #[serde(default)]
    deletion_inventory: Option<DeletionInventory>,
}

/// How the online side reaches the manager.
struct ManagerAccess {
    url: String,
    token: String,
}

pub async fn sync_command(
    mut args: SyncArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    if let (Some(token), None, Some(base_url)) = (
        args.token.as_deref(),
        args.manager.as_ref(),
        embedded_config.and_then(|config| config.api_base_url.as_deref()),
    ) {
        args.manager =
            Some(super::up::resolve_manager_url_option(None, base_url, token, "kubernetes").await?);
    }
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

    // Reports that couldn't be sent. They stay in the folder, and sync fails
    // once the rest of its work is done.
    let mut unsent_reports = None;
    if online {
        let token = token.as_ref().expect("online implies a stored token");
        unsent_reports = exchange_online(
            &folder,
            &mut site,
            token,
            args.full,
            args.dry_run,
            &args.values,
        )
        .await?;
    }
    if let Some(settings) = &site_side {
        install_pending(&folder, &mut site, settings, &args).await?;
        if !args.dry_run {
            write_report(&folder, &site, settings).await?;
        }
        save_site(&folder, &site)?;
        if online {
            let token = token.as_ref().expect("online implies a stored token");
            // Sends every waiting report, including any the first try left.
            unsent_reports = send_reports(&folder, &mut site, token, false).await.err();
            save_site(&folder, &site)?;
        } else if !args.dry_run {
            println!("Copy {} back out.", folder.display());
        }
    } else if !args.dry_run {
        println!("Copy {} to the site.", folder.display());
    }
    match unsent_reports {
        Some(e) => Err(e).context(ErrorData::ManagerRequestFailed {
            message: format!(
                "the site's reports weren't sent; they stay in {} for the next sync",
                folder.join(FROM_SITE).display()
            ),
        }),
        None => Ok(()),
    }
}

/// The online side: send back the site's reports, then download the next
/// update.
///
/// A report that can't be sent doesn't hold the update back: the site would
/// otherwise get no more updates for as long as one report fails, including
/// an update that fixes what makes it fail. The report stays in the folder
/// and its error comes back once the update is in the folder, for the
/// caller to fail with.
async fn exchange_online(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    full: bool,
    dry_run: bool,
    values: &[PathBuf],
) -> Result<Option<AlienError<ErrorData>>> {
    let unsent_reports = send_reports(folder, site, token, dry_run).await.err();
    if let Some(e) = &unsent_reports {
        eprintln!("Couldn't send the site's reports ({e}); downloading the update anyway.");
    }
    download_update(folder, site, token, full, dry_run, values).await?;
    save_site(folder, site)?;
    Ok(unsent_reports)
}

fn rollback_candidate(installed: &[(u64, PathBuf)]) -> Result<&Path> {
    for (_, path) in installed.iter().skip(1) {
        if read_installed(path)?.rollout_succeeded {
            return Ok(path);
        }
    }
    Err(AlienError::new(ErrorData::ValidationError {
        field: "rollback".to_string(),
        message: "There's no earlier successful release to return to".to_string(),
    }))
}

fn remember_successful_rollout(folder: &Path, state: &serde_json::Value) -> Result<()> {
    if state["status"] != "running" {
        return Ok(());
    }
    let Some(release) = state["currentRelease"]["releaseId"].as_str() else {
        return Ok(());
    };
    mark_successful_release(folder, release)
}

fn mark_successful_release(folder: &Path, release: &str) -> Result<()> {
    for (_, path) in installed_bundles(folder)? {
        let mut record = read_installed(&path)?;
        if record.release_id == release && !record.rollout_succeeded {
            record.rollout_succeeded = true;
            std::fs::write(
                path.join("install.json"),
                serde_json::to_vec_pretty(&record)
                    .into_alien_error()
                    .context(config_error("encoding successful rollout"))?,
            )
            .into_alien_error()
            .context(config_error("recording successful rollout"))?;
        }
    }
    Ok(())
}

fn prune_installed(folder: &Path) -> Result<()> {
    let mut successful = 0;
    for (index, (_, path)) in installed_bundles(folder)?.into_iter().enumerate() {
        let record = read_installed(&path)?;
        if record.rollout_succeeded {
            successful += 1;
            if successful <= KEEP_INSTALLED {
                continue;
            }
        }
        // Keep the most recent attempt for status reporting and signed deletion.
        if index != 0 {
            std::fs::remove_dir_all(path)
                .into_alien_error()
                .context(config_error("pruning old installs"))?;
        }
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
    if let Some((_, latest)) = installed.first() {
        let record = read_installed(latest)?;
        let namespace = settings.namespace(&record.stack_id);
        if let Some((status, _)) = read_status(&settings, &record.stack_id, &namespace).await? {
            remember_successful_rollout(&folder, &status["state"])?;
        }
    }
    let previous = rollback_candidate(&installed)?;
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
    let mut machine = load_machine(&site.deployment_id)?;
    machine.deletion_inventory = None;
    save_machine(&site.deployment_id, &machine)?;
    let attempt = write_target(
        &settings,
        &record.stack_id,
        &namespace,
        &target_path,
        sequence,
        ack,
    )
    .await?;
    if let Err(error) = wait_for_rollout(
        &settings,
        &record.stack_id,
        &namespace,
        &record.release_id,
        sequence,
        &attempt,
    )
    .await
    {
        write_report_for_install(&folder, &site, &settings, &record).await?;
        return Err(error);
    }
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
        let fresh_batches = report
            .telemetry
            .iter()
            .filter(|batch| batch.id > site.uploaded_through)
            .count();
        if dry_run {
            println!(
                "Would send report from {}: {} running {}, {fresh_batches} telemetry batches",
                report.written_at,
                site.name,
                report.installed_release.as_deref().unwrap_or("nothing yet"),
            );
            continue;
        }
        send_report(folder, site, token, &report).await?;
        site.site_has = report.inventory;
        site.site_release = report.installed_release.clone();
        site.deleted = report.state["status"].as_str() == Some("deleted");
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
            "Sent report from {}: {} {}, release {}, {fresh_batches} telemetry batches{dropped}.",
            report.written_at,
            site.name,
            report.state["status"].as_str().unwrap_or("unknown"),
            report.installed_release.as_deref().unwrap_or("none"),
        );
        std::fs::remove_file(&path)
            .into_alien_error()
            .context(config_error(format!("removing sent {}", path.display())))?;
    }
    Ok(())
}

/// What the manager answers a status report with.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusReportResponse {
    /// Highest telemetry batch the manager has received.
    telemetry_through: Option<i64>,
}

/// Send one report. The manager may take only part of a large telemetry
/// backlog per request (it answers before a gateway would time the request
/// out), so the rest goes in further requests for as long as each one makes
/// progress. `site.uploaded_through` follows what the manager confirmed.
async fn send_report(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    report: &SiteReport,
) -> Result<()> {
    let last = report.telemetry.iter().map(|batch| batch.id).max();
    let deleting = report.state["status"].as_str() == Some("deleted");
    let terminal = deleting || report.state["status"].as_str() == Some("retained");
    let initial_mark = site.uploaded_through;
    let total = report
        .telemetry
        .iter()
        .filter(|batch| batch.id > initial_mark)
        .count();
    let started = Instant::now();
    let client = http()?;
    if total > 0 {
        println!("Uploading {total} telemetry batches for {}...", site.name);
    }
    loop {
        let mut body = serde_json::json!({
            "state": report.state, "telemetry": [], "writtenAt": report.written_at,
            "heartbeats": report.heartbeats
        });
        let mut bytes = serde_json::to_vec(&body)
            .into_alien_error()
            .context(config_error("encoding the report state"))?
            .len();
        let mut fresh = Vec::new();
        for batch in report
            .telemetry
            .iter()
            .filter(|batch| batch.id > site.uploaded_through)
        {
            let value = serde_json::json!({
                "id": batch.id, "signal": batch.signal, "data": batch.data
            });
            let batch_bytes = serde_json::to_vec(&value)
                .into_alien_error()
                .context(config_error("encoding a telemetry batch"))?
                .len()
                + 1;
            // Keep the state and batches within a 1 MiB request. An individual
            // larger batch is sent alone so its contents are never discarded.
            if !fresh.is_empty() && bytes + batch_bytes > 1024 * 1024 {
                break;
            }
            bytes += batch_bytes;
            fresh.push(value);
        }
        let sending = !fresh.is_empty();
        // A Deleted state removes the deployment metadata. Send it only after
        // every batch is durably acknowledged, in a separate empty request.
        if terminal && sending {
            body["state"]["status"] = "deleting".into();
        }
        body["telemetry"] = serde_json::Value::Array(fresh);
        let path = format!("/v1/deployments/{}/status-report", site.deployment_id);
        let response = client
            .post(format!("{}{path}", token.url))
            .bearer_auth(&token.token)
            .json(&body)
            .send()
            .await
            .into_alien_error()
            .context(api_failed(format!("POST {path}")))?;
        // Deletion can remove the credential before its acknowledgement arrives.
        // Accept removal only for the empty marker after all telemetry is acknowledged.
        if deleting
            && !sending
            && matches!(
                response.status(),
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::NOT_FOUND
            )
        {
            println!(
                "{} was already removed; all telemetry was acknowledged.",
                site.name
            );
            return Ok(());
        }
        let response: StatusReportResponse = read_response(response, &path).await?;
        if !sending {
            return Ok(());
        }
        let confirmed = response.telemetry_through.ok_or_else(|| {
            AlienError::new(ErrorData::ManagerRequestFailed {
                message: "the manager didn't say which telemetry it received".to_string(),
            })
        })?;
        if confirmed <= site.uploaded_through {
            return Err(AlienError::new(ErrorData::ManagerRequestFailed {
                message: format!(
                    "the manager took none of the telemetry after batch {}",
                    site.uploaded_through
                ),
            }));
        }
        site.uploaded_through = confirmed;
        // Saved after every request, so progress is never lost.
        save_site(folder, site)?;
        let acknowledged = report
            .telemetry
            .iter()
            .filter(|batch| batch.id > initial_mark && batch.id <= confirmed)
            .count();
        println!(
            "Uploaded {acknowledged}/{total} telemetry batches in {}s (acknowledged through {confirmed}).",
            started.elapsed().as_secs()
        );
        if last.is_some_and(|last| confirmed >= last) {
            if !terminal {
                return Ok(());
            }
        }
    }
}

/// Download the release the site should run, unless the site already runs it
/// or the folder already carries it.
async fn download_update(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    full: bool,
    dry_run: bool,
    values: &[PathBuf],
) -> Result<()> {
    if site.deleted {
        println!("{} has been deleted.", site.name);
        return Ok(());
    }
    let manager = &token.url;
    let target: serde_json::Value = manager_get(
        manager,
        &token.token,
        &format!("/v1/deployments/{}/target", site.deployment_id),
    )
    .await?;
    if target["action"].as_str() == Some("retained") {
        println!(
            "{} is uninstalled; its data and recovery metadata are retained.",
            site.name
        );
        return Ok(());
    }
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

    if target["action"].as_str() == Some("delete") {
        return download_deletion(folder, site, token, &target, &stack_id, dry_run).await;
    }

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
    let values = values
        .iter()
        .map(|path| {
            let bytes = std::fs::read(path)
                .into_alien_error()
                .context(config_error(format!(
                    "reading Helm values {}",
                    path.display()
                )))?;
            serde_yaml::from_slice(&bytes)
                .into_alien_error()
                .context(config_error(format!(
                    "parsing Helm values {}",
                    path.display()
                )))
        })
        .collect::<Result<Vec<serde_yaml::Value>>>()?;
    let chart_images = airgap::chart_images(&dir.join(&chart), &target, &values)
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
        deletion_policy: None,
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
    let trusted_key = site.bundle_key.as_deref().ok_or_else(|| {
        AlienError::new(config_error("the manager returned no bundle signing key"))
    })?;
    write_update_bundle(dir, &file, folder, trusted_key).await?;
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

/// Publish verified tools alongside the bundle so a first-time site can run sync.
async fn write_update_bundle(
    dir: &Path,
    file: &Path,
    folder: &Path,
    trusted_key: &str,
) -> Result<()> {
    airgap::verify(dir, trusted_key)
        .await
        .context(config_error("verifying the downloaded update"))?;
    airgap::pack(dir, file).context(config_error("writing the bundle"))?;
    copy_tools(dir, folder)
}

/// A deletion carries no images or chart: the installed chart owns cleanup.
async fn download_deletion(
    folder: &Path,
    site: &mut SiteState,
    token: &ManagerAccess,
    target: &serde_json::Value,
    stack_id: &str,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!("Would download the deletion of {}.", site.name);
        return Ok(());
    }
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("preparing deletion"))?;
    std::fs::write(
        work.path().join(airgap::TARGET_FILE),
        serde_json::to_vec(target)
            .into_alien_error()
            .context(config_error("encoding deletion"))?,
    )
    .into_alien_error()
    .context(config_error("writing deletion"))?;
    let sequence = chrono::Utc::now().timestamp_millis() as u64;
    let manifest = BundleManifest {
        format_version: airgap::FORMAT_VERSION,
        deployment_id: site.deployment_id.clone(),
        deployment_name: site.name.clone(),
        release_id: "delete".to_string(),
        sequence,
        created_at: chrono::Utc::now().to_rfc3339(),
        stack_id: stack_id.to_string(),
        images: Vec::new(),
        chart_images: BTreeMap::new(),
        chart: String::new(),
        files: airgap::checksums(work.path())
            .await
            .context(config_error("checksumming deletion"))?,
        omitted_blobs: BTreeSet::new(),
        telemetry_ack: None,
        deletion_policy: Some(
            serde_json::from_value(
                target
                    .get("deletionPolicy")
                    .cloned()
                    .unwrap_or_else(|| "retain".into()),
            )
            .into_alien_error()
            .context(config_error("parsing deletion policy"))?,
        ),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .into_alien_error()
        .context(config_error("encoding deletion manifest"))?;
    let signature: serde_json::Value = manager_post(&token.url, &token.token,
        &format!("/v1/deployments/{}/bundle-signature", site.deployment_id),
        &serde_json::json!({ "manifest": base64::engine::general_purpose::STANDARD.encode(&bytes) })).await?;
    let signed = signature["signature"]
        .as_str()
        .ok_or_else(|| AlienError::new(config_error("missing deletion signature")))?;
    std::fs::write(work.path().join(airgap::MANIFEST_FILE), bytes)
        .into_alien_error()
        .context(config_error("writing deletion manifest"))?;
    std::fs::write(work.path().join(airgap::SIGNATURE_FILE), signed)
        .into_alien_error()
        .context(config_error("writing deletion signature"))?;
    let destination = folder
        .join(TO_SITE)
        .join(format!("{sequence}-delete.{BUNDLE_EXTENSION}"));
    airgap::pack(work.path(), &destination).context(config_error("packing deletion"))?;
    for (_, old) in by_sequence(&folder.join(TO_SITE), Some(BUNDLE_EXTENSION))? {
        if old != destination {
            std::fs::remove_file(old)
                .into_alien_error()
                .context(config_error("removing superseded update"))?;
        }
    }
    println!("Downloaded signed deletion for {}.", site.name);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionInventory {
    namespace: String,
    stack_id: String,
    kube_context: Option<String>,
    objects: Vec<DeletionObject>,
    /// Claim templates remain authoritative after their StatefulSets are stopped.
    #[serde(default)]
    claim_prefixes: Vec<String>,
    #[serde(default)]
    runtime_selector: Option<String>,
    #[serde(default)]
    report: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionObject {
    resource: String,
    name: String,
    uid: String,
    #[serde(default)]
    volume: Option<DeletionVolume>,
}

#[cfg(test)]
async fn deletion_inventory(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<DeletionInventory> {
    deletion_inventory_with_previous(settings, stack_id, namespace, None).await
}

async fn deletion_inventory_with_previous(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
    previous: Option<&DeletionInventory>,
) -> Result<DeletionInventory> {
    let mut command = Command::new("helm");
    command.args(["get", "manifest", stack_id, "--namespace", namespace]);
    if let Some(context) = &settings.kube_context {
        command.args(["--kube-context", context]);
    }
    let manifest = run(command, "reading the installed chart").await?;
    let mut retained = BTreeSet::new();
    let mut scope = None;
    for document in serde_yaml::Deserializer::from_slice(&manifest) {
        let value = serde_json::Value::deserialize(document)
            .into_alien_error()
            .context(config_error("parsing the installed chart"))?;
        if matches!(
            value["kind"].as_str(),
            Some("PersistentVolumeClaim" | "Secret")
        ) && value["metadata"]["annotations"]["helm.sh/resource-policy"] == "keep"
        {
            if let (Some(kind), Some(name)) =
                (value["kind"].as_str(), value["metadata"]["name"].as_str())
            {
                retained.insert((kind.to_string(), name.to_string()));
            }
        }
        if value["kind"] == "Deployment" {
            let env = value
                .pointer("/spec/template/spec/containers/0/env")
                .and_then(|v| v.as_array());
            if let Some(env) = env {
                let get = |name| {
                    env.iter()
                        .find(|v| v["name"] == name)
                        .and_then(|v| v["value"].as_str())
                        .map(str::to_owned)
                };
                if let (Some(key), Some(prefix)) = (
                    get("ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY"),
                    get("OPERATOR_RESOURCE_PREFIX"),
                ) {
                    scope = Some((key, prefix));
                }
            }
        }
    }
    let mut get = kubectl(settings.kube_context.as_deref());
    get.args([
        "get",
        "pvc,secret,statefulset",
        "--namespace",
        namespace,
        "-o",
        "json",
    ]);
    let objects: serde_json::Value =
        serde_json::from_slice(&run(get, "reading owned data inventory").await?)
            .into_alien_error()
            .context(config_error("parsing owned data inventory"))?;
    let items = objects["items"]
        .as_array()
        .ok_or_else(|| AlienError::new(config_error("missing data inventory")))?;
    let runtime_owned = |object: &serde_json::Value| {
        scope.as_ref().is_some_and(|(key, prefix)| {
            object["metadata"]["labels"]["managed-by"] == "runtime"
                && object["metadata"]["labels"][key] == *prefix
        })
    };
    let mut claim_prefixes = previous
        .map(|inventory| inventory.claim_prefixes.clone())
        .unwrap_or_default();
    for object in items
        .iter()
        .filter(|v| v["kind"] == "StatefulSet" && runtime_owned(v))
    {
        if let (Some(name), Some(claims)) = (
            object["metadata"]["name"].as_str(),
            object["spec"]["volumeClaimTemplates"].as_array(),
        ) {
            for claim in claims {
                if let Some(claim) = claim["metadata"]["name"].as_str() {
                    claim_prefixes.push(format!("{claim}-{name}-"));
                }
            }
        }
    }
    claim_prefixes.sort();
    claim_prefixes.dedup();
    let mut data = previous
        .map(|inventory| inventory.objects.clone())
        .unwrap_or_default();
    for object in items {
        let resource = match object["kind"].as_str() {
            Some("PersistentVolumeClaim") => "persistentvolumeclaims",
            Some("Secret") => "secrets",
            _ => continue,
        };
        let name = object["metadata"]["name"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("data object has no name")))?;
        let annotations = &object["metadata"]["annotations"];
        let helm_owned = retained.contains(&(
            object["kind"].as_str().unwrap().to_string(),
            name.to_string(),
        )) && annotations["meta.helm.sh/release-name"] == stack_id
            && annotations["meta.helm.sh/release-namespace"] == namespace
            && object["metadata"]["labels"]["app.kubernetes.io/managed-by"] == "Helm";
        // Container claim templates have no labels; ownership comes from the StatefulSet.
        let runtime_claim = resource == "persistentvolumeclaims"
            && claim_prefixes.iter().any(|prefix| {
                name.strip_prefix(prefix).is_some_and(|ordinal| {
                    !ordinal.is_empty() && ordinal.bytes().all(|b| b.is_ascii_digit())
                })
            });
        if helm_owned || runtime_owned(object) || runtime_claim {
            let uid = object["metadata"]["uid"]
                .as_str()
                .ok_or_else(|| AlienError::new(config_error("data object has no UID")))?;
            if let Some(saved) = data
                .iter()
                .find(|saved| saved.resource == resource && saved.name == name)
            {
                if saved.uid != uid {
                    return Err(AlienError::new(config_error(format!(
                        "{resource}/{name} was replaced; refusing to erase a different object"
                    ))));
                }
            } else {
                data.push(DeletionObject {
                    resource: resource.into(),
                    name: name.into(),
                    uid: uid.into(),
                    volume: None,
                });
            }
        }
    }
    Ok(DeletionInventory {
        namespace: namespace.into(),
        stack_id: stack_id.into(),
        kube_context: settings.kube_context.clone(),
        objects: data,
        claim_prefixes,
        runtime_selector: scope.map(|(key, prefix)| format!("managed-by=runtime,{key}={prefix}")),
        report: None,
    })
}

/// Stop claim creation before the final inventory. Orphan deletion preserves data
/// regardless of the StatefulSet's PVC retention policy; the Helm hook stops pods.
async fn stop_claim_controllers(settings: &SiteSide, inventory: &DeletionInventory) -> Result<()> {
    let Some(selector) = &inventory.runtime_selector else {
        return Ok(());
    };
    let mut get = kubectl(settings.kube_context.as_deref());
    get.args([
        "get",
        "statefulsets",
        "-n",
        &inventory.namespace,
        "-l",
        selector,
        "-o",
        "json",
    ]);
    let sets: serde_json::Value =
        serde_json::from_slice(&run(get, "reading claim controllers").await?)
            .into_alien_error()
            .context(config_error("parsing claim controllers"))?;
    let sets = sets["items"]
        .as_array()
        .ok_or_else(|| AlienError::new(config_error("missing StatefulSet list")))?;
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("preparing controller cleanup"))?;
    for set in sets {
        let name = set["metadata"]["name"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("StatefulSet has no name")))?;
        let uid = set["metadata"]["uid"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("StatefulSet has no UID")))?;
        // Retain claims while scaling down, then remove the controller so an HPA
        // cannot start creating claims again between the inventory and Helm cleanup.
        let patch = serde_json::json!([
            {"op":"test", "path":"/metadata/uid", "value":uid},
            {"op":"add", "path":"/spec/persistentVolumeClaimRetentionPolicy", "value":{"whenDeleted":"Retain","whenScaled":"Retain"}},
            {"op":"add", "path":"/spec/replicas", "value":0}
        ]);
        let mut stop = kubectl(settings.kube_context.as_deref());
        stop.args([
            "patch",
            "statefulset",
            name,
            "-n",
            &inventory.namespace,
            "--type=json",
            "-p",
            &patch.to_string(),
        ]);
        run(stop, "stopping claim creation").await?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let mut get = kubectl(settings.kube_context.as_deref());
            get.args([
                "get",
                "statefulset",
                name,
                "-n",
                &inventory.namespace,
                "-o",
                "json",
            ]);
            let current: serde_json::Value =
                serde_json::from_slice(&run(get, "waiting for claim creation to stop").await?)
                    .into_alien_error()
                    .context(config_error("parsing stopped controller"))?;
            if current["metadata"]["uid"] != uid || current["spec"]["replicas"] != 0 {
                return Err(AlienError::new(config_error(
                    "StatefulSet changed while stopping claim creation; retry deletion",
                )));
            }
            if current["status"]["observedGeneration"]
                .as_u64()
                .unwrap_or(0)
                >= current["metadata"]["generation"].as_u64().unwrap_or(1)
                && current["status"]["replicas"].as_u64().unwrap_or(0) == 0
            {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(AlienError::new(config_error(
                    "StatefulSet did not stop before the data inventory deadline",
                )));
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let options = work.path().join("delete.json");
        std::fs::write(&options, serde_json::json!({"apiVersion":"v1", "kind":"DeleteOptions", "propagationPolicy":"Orphan", "preconditions":{"uid":uid}}).to_string())
            .into_alien_error().context(config_error("writing controller deletion precondition"))?;
        let mut delete = kubectl(settings.kube_context.as_deref());
        delete
            .args([
                "delete",
                "--raw",
                &format!(
                    "/apis/apps/v1/namespaces/{}/statefulsets/{name}",
                    inventory.namespace
                ),
                "-f",
            ])
            .arg(options);
        run(delete, "removing stopped claim controller").await?;
        let mut wait = kubectl(settings.kube_context.as_deref());
        wait.args([
            "wait",
            "--for=delete",
            &format!("statefulset/{name}"),
            "-n",
            &inventory.namespace,
            "--timeout=120s",
        ]);
        run(wait, "waiting for claim controller deletion").await?;
    }
    Ok(())
}

async fn erase_inventory(settings: &SiteSide, inventory: &DeletionInventory) -> Result<()> {
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("preparing data deletion"))?;
    for object in &inventory.objects {
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args([
            "get",
            &object.resource,
            &object.name,
            "--namespace",
            &inventory.namespace,
            "--ignore-not-found",
            "-o",
            "json",
        ]);
        let bytes = run(get, "checking data identity").await?;
        if bytes.is_empty() {
            continue;
        }
        let current: serde_json::Value = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(config_error("parsing data identity"))?;
        if current["metadata"]["uid"] != object.uid {
            return Err(AlienError::new(config_error(format!(
                "{}/{} was replaced; refusing to erase a different object",
                object.resource, object.name
            ))));
        }
        if object.resource == "persistentvolumeclaims"
            && current["spec"]["volumeName"]
                .as_str()
                .filter(|name| !name.is_empty())
                != object.volume.as_ref().map(|volume| volume.name.as_str())
        {
            return Err(AlienError::new(config_error(format!(
                "volume binding for {}/{} changed; retry deletion to record and prepare the bound volume",
                object.resource, object.name
            ))));
        }
        // Bindings can change after this read; let the caller re-inventory on conflict.
        let resource_version = current["metadata"]["resourceVersion"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("data object has no resource version")))?;
        let options = work.path().join("delete.json");
        std::fs::write(&options, serde_json::json!({"apiVersion":"v1","kind":"DeleteOptions","preconditions":{"uid":object.uid,"resourceVersion":resource_version}}).to_string())
            .into_alien_error().context(config_error("writing deletion precondition"))?;
        let mut delete = kubectl(settings.kube_context.as_deref());
        delete
            .args([
                "delete",
                "--raw",
                &format!(
                    "/api/v1/namespaces/{}/{}/{}",
                    inventory.namespace, object.resource, object.name
                ),
                "-f",
            ])
            .arg(options);
        run(delete, "erasing owned data").await?;
        let mut wait = kubectl(settings.kube_context.as_deref());
        wait.args([
            "wait",
            "--for=delete",
            &format!("{}/{}", object.resource, object.name),
            "--namespace",
            &inventory.namespace,
            "--timeout=120s",
        ]);
        run(wait, "waiting for data deletion").await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionVolume {
    name: String,
    uid: String,
}

async fn prepare_volume_erasure(
    settings: &SiteSide,
    inventory: &mut DeletionInventory,
) -> Result<()> {
    for object in &mut inventory.objects {
        if object.resource != "persistentvolumeclaims" || object.volume.is_some() {
            continue;
        }
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args([
            "get",
            "pvc",
            &object.name,
            "-n",
            &inventory.namespace,
            "--ignore-not-found",
            "-o",
            "json",
        ]);
        let bytes = run(get, "reading the volume claim").await?;
        if bytes.is_empty() {
            continue;
        }
        let claim: serde_json::Value = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(config_error("parsing the volume claim"))?;
        if claim["metadata"]["uid"] != object.uid {
            return Err(AlienError::new(config_error(
                "volume claim was replaced; refusing to erase it",
            )));
        }
        let Some(name) = claim["spec"]["volumeName"].as_str() else {
            continue;
        };
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args(["get", "pv", name, "-o", "json"]);
        let volume: serde_json::Value =
            serde_json::from_slice(&run(get, "reading the bound volume").await?)
                .into_alien_error()
                .context(config_error("parsing the bound volume"))?;
        if volume["spec"]["claimRef"]["uid"] != object.uid
            || volume["spec"]["claimRef"]["namespace"] != inventory.namespace
            || volume["metadata"]["annotations"]["pv.kubernetes.io/provisioned-by"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            return Err(AlienError::new(config_error(
                "erasure requires a dynamically provisioned volume bound to the recorded claim",
            )));
        }
        let uid = volume["metadata"]["uid"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("bound volume has no UID")))?;
        let patch = serde_json::json!([
            {"op":"test","path":"/metadata/uid","value":uid},
            {"op":"test","path":"/spec/claimRef/uid","value":object.uid},
            {"op":"replace","path":"/spec/persistentVolumeReclaimPolicy","value":"Delete"}
        ])
        .to_string();
        let mut command = kubectl(settings.kube_context.as_deref());
        command.args(["patch", "pv", name, "--type=json", "-p", &patch]);
        run(command, "setting owned volume erasure").await?;
        object.volume = Some(DeletionVolume {
            name: name.into(),
            uid: uid.into(),
        });
    }
    Ok(())
}

async fn wait_for_erased_volumes(settings: &SiteSide, inventory: &DeletionInventory) -> Result<()> {
    for volume in inventory
        .objects
        .iter()
        .filter_map(|object| object.volume.as_ref())
    {
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args([
            "get",
            "pv",
            &volume.name,
            "--ignore-not-found",
            "-o",
            "json",
        ]);
        let bytes = run(get, "checking erased volume").await?;
        if bytes.is_empty() {
            continue;
        }
        let current: serde_json::Value = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(config_error("parsing erased volume"))?;
        if current["metadata"]["uid"] != volume.uid {
            return Err(AlienError::new(config_error(
                "bound volume was replaced; refusing to report erasure",
            )));
        }
        let mut wait = kubectl(settings.kube_context.as_deref());
        wait.args([
            "wait",
            "--for=delete",
            &format!("pv/{}", volume.name),
            "--timeout=180s",
        ]);
        run(
            wait,
            "waiting for the storage provisioner to erase the volume",
        )
        .await?;
    }
    Ok(())
}

fn operator_deployment_name(values: &serde_json::Value, release: &str) -> String {
    values["fullnameOverride"]
        .as_str()
        .filter(|name| !name.is_empty())
        .unwrap_or(release)
        .chars()
        .take(63)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

async fn wait_for_deleted_pods(settings: &SiteSide, namespace: &str, selector: &str) -> Result<()> {
    let mut wait = kubectl(settings.kube_context.as_deref());
    wait.args([
        "wait",
        "--for=delete",
        "pods",
        "-n",
        namespace,
        "-l",
        selector,
        "--timeout=180s",
    ]);
    run(wait, "waiting for workload pods to stop").await?;
    Ok(())
}

async fn uninstall_site(
    folder: &Path,
    site: &SiteState,
    settings: &SiteSide,
    manifest: &BundleManifest,
    dry_run: bool,
) -> Result<()> {
    let namespace = settings.namespace(&manifest.stack_id);
    let policy = manifest.deletion_policy.unwrap_or_default();
    let mut machine = load_machine(&site.deployment_id)?;
    if let Some(inventory) = &machine.deletion_inventory {
        if inventory.namespace != namespace
            || inventory.stack_id != manifest.stack_id
            || inventory.kube_context != settings.kube_context
        {
            return Err(AlienError::new(config_error(
                "saved deletion inventory belongs to a different site location",
            )));
        }
    }
    let values = release_values(settings, &manifest.stack_id, &namespace).await?;
    if let Some(values) = &values {
        if values["management"]["deploymentId"].as_str() != Some(site.deployment_id.as_str()) {
            return Err(AlienError::new(config_error(
                "the installed Helm release belongs to another deployment",
            )));
        }
        if values["runtime"]["cleanup"]["onUninstall"]["enabled"].as_bool() != Some(true) {
            return Err(AlienError::new(config_error(
                "enable runtime.cleanup.onUninstall.enabled before deleting the site",
            )));
        }
    }
    if policy == DeletionPolicy::Retain
        && values.as_ref().is_some_and(|v| {
            v["runtime"]["cleanup"]["onUninstall"]["deletePersistentVolumeClaims"] == true
        })
    {
        return Err(AlienError::new(config_error(
            "disable runtime.cleanup.onUninstall.deletePersistentVolumeClaims before applying a retain request",
        )));
    }
    if dry_run {
        println!(
            "Would uninstall {}/{} and carry its final report back.",
            namespace, manifest.stack_id
        );
        return Ok(());
    }
    // Keep the snapshot outside from-site until Helm confirms cleanup. It also
    // lets an interrupted uninstall resume after the Operator is gone.
    let snapshot = folder.join("deleting-report.json");
    let sealed_snapshot = folder.join("deleting-telemetry-sequence.json");
    let telemetry_sealed = if snapshot.exists() && sealed_snapshot.exists() {
        let sequence: u64 = serde_json::from_slice(
            &std::fs::read(&sealed_snapshot)
                .into_alien_error()
                .context(config_error("reading final telemetry receipt"))?,
        )
        .into_alien_error()
        .context(config_error("parsing final telemetry receipt"))?;
        // A fresh deletion does not reopen telemetry. Only an install or rollback
        // with a target newer than the completed drain can do that.
        sequence <= manifest.sequence
            && target_sequence(settings, &manifest.stack_id, &namespace)
                .await?
                .is_none_or(|target| target <= sequence)
    } else {
        false
    };
    let mut report: SiteReport = if snapshot.exists() {
        serde_json::from_slice(
            &std::fs::read(&snapshot)
                .into_alien_error()
                .context(config_error("reading deletion snapshot"))?,
        )
        .into_alien_error()
        .context(config_error("parsing deletion snapshot"))?
    } else if let Some(report) = machine
        .deletion_inventory
        .as_ref()
        .and_then(|v| v.report.as_ref())
    {
        serde_json::from_value(report.clone())
            .into_alien_error()
            .context(config_error("reading the site-local deletion report"))?
    } else {
        let (_, installed) = installed_bundles(folder)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AlienError::new(config_error(
                    "no installed bundle is recorded for this deletion",
                ))
            })?;
        let record = read_installed(&installed)?;
        if record.stack_id != manifest.stack_id {
            return Err(AlienError::new(config_error(
                "the deletion does not match the recorded install",
            )));
        }
        let report = collect_report(site, settings, &record)
            .await?
            .ok_or_else(|| {
                AlienError::new(config_error(
                    "the Operator has no status to carry back before deletion",
                ))
            })?;
        std::fs::write(
            &snapshot,
            serde_json::to_vec(&report)
                .into_alien_error()
                .context(config_error("encoding deletion snapshot"))?,
        )
        .into_alien_error()
        .context(config_error("saving deletion snapshot"))?;
        report
    };
    if report.deployment_id != site.deployment_id {
        return Err(AlienError::new(config_error(
            "deletion snapshot belongs to another deployment",
        )));
    }
    if let Some(values) = &values {
        let operator_name = operator_deployment_name(values, &manifest.stack_id);
        // A previous cleanup hook may have removed the Operator before failing.
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args([
            "get",
            "deployment",
            &operator_name,
            "--namespace",
            &namespace,
            "--ignore-not-found",
            "-o",
            "json",
        ]);
        let deployment = run(get, "checking for the Operator").await?;
        if !deployment.is_empty() {
            if !telemetry_sealed {
                let (status, export_token) = read_status(settings, &manifest.stack_id, &namespace)
                    .await?
                    .ok_or_else(|| {
                        AlienError::new(config_error(
                            "the Operator export credential is unavailable before deletion",
                        ))
                    })?;
                // An intervening install or rollback makes the saved recovery state stale.
                // Keep its buffered logs, but report the state of the current installation.
                report.state = status["state"].clone();
                let after = report
                    .telemetry
                    .iter()
                    .map(|batch| batch.id)
                    .max()
                    .unwrap_or(site.uploaded_through);
                // The first snapshot may predate batches accepted while it was written.
                // Seal both ingestion paths, then drain those batches before stopping.
                let (remaining, dropped) = export_telemetry(
                    settings,
                    &manifest.stack_id,
                    &namespace,
                    &export_token,
                    after,
                    Some(manifest.sequence),
                )
                .await?;
                report.telemetry.extend(remaining);
                report.dropped = dropped;
                std::fs::write(
                    &snapshot,
                    serde_json::to_vec(&report)
                        .into_alien_error()
                        .context(config_error("encoding final telemetry snapshot"))?,
                )
                .into_alien_error()
                .context(config_error("saving final telemetry snapshot"))?;
                // Persist the completed drain before stopping. A retry must not
                // port-forward to an Operator already scaled to zero.
                std::fs::write(&sealed_snapshot, manifest.sequence.to_string())
                    .into_alien_error()
                    .context(config_error("saving final telemetry receipt"))?;
            }
            let deployment: serde_json::Value = serde_json::from_slice(&deployment)
                .into_alien_error()
                .context(config_error("reading the Operator selector"))?;
            let labels = deployment
                .pointer("/spec/selector/matchLabels")
                .and_then(serde_json::Value::as_object)
                .filter(|labels| !labels.is_empty())
                .ok_or_else(|| AlienError::new(config_error("the Operator has no pod selector")))?;
            let selector = labels
                .iter()
                .map(|(key, value)| {
                    value
                        .as_str()
                        .map(|value| format!("{key}={value}"))
                        .ok_or_else(|| {
                            AlienError::new(config_error("invalid Operator pod selector"))
                        })
                })
                .collect::<Result<Vec<_>>>()?
                .join(",");
            // Stop reconciliation before the chart removes its runtime resources.
            let mut stop = kubectl(settings.kube_context.as_deref());
            stop.args([
                "scale",
                "deployment",
                &operator_name,
                "--namespace",
                &namespace,
                "--replicas=0",
            ]);
            run(stop, "stopping the Operator").await?;
            let mut wait = kubectl(settings.kube_context.as_deref());
            wait.args([
                "rollout",
                "status",
                &format!("deployment/{operator_name}"),
                "--namespace",
                &namespace,
                "--timeout=120s",
            ]);
            run(wait, "waiting for the Operator to stop").await?;
            wait_for_deleted_pods(settings, &namespace, &selector).await?;
        }
    }
    // This receipt stays on the site machine, outside the carried folder.
    // It survives Helm removal and never authorizes deletion of a replacement UID.
    if values.is_some() {
        // Save template ownership before removing controllers, including on retries
        // that already have an inventory from an interrupted scale-up.
        machine.deletion_inventory = Some(
            deletion_inventory_with_previous(
                settings,
                &manifest.stack_id,
                &namespace,
                machine.deletion_inventory.as_ref(),
            )
            .await?,
        );
        save_machine(&site.deployment_id, &machine)?;
        stop_claim_controllers(
            settings,
            machine
                .deletion_inventory
                .as_ref()
                .expect("saved inventory"),
        )
        .await?;
        machine.deletion_inventory = Some(
            deletion_inventory_with_previous(
                settings,
                &manifest.stack_id,
                &namespace,
                machine.deletion_inventory.as_ref(),
            )
            .await?,
        );
        save_machine(&site.deployment_id, &machine)?;
    }
    if policy == DeletionPolicy::Erase && machine.deletion_inventory.is_none() {
        return Err(AlienError::new(config_error(
            "no site-local data inventory remains; restore the site machine's sync settings before erasing",
        )));
    }
    if policy == DeletionPolicy::Erase {
        prepare_volume_erasure(
            settings,
            machine
                .deletion_inventory
                .as_mut()
                .expect("checked erase inventory"),
        )
        .await?;
        save_machine(&site.deployment_id, &machine)?;
    }
    if let Some(inventory) = &mut machine.deletion_inventory {
        inventory.report = Some(
            serde_json::to_value(&report)
                .into_alien_error()
                .context(config_error("encoding deletion recovery state"))?,
        );
        save_machine(&site.deployment_id, &machine)?;
    }
    let mut command = Command::new("helm");
    command.args([
        "uninstall",
        &manifest.stack_id,
        "--namespace",
        &namespace,
        "--wait",
        "--timeout",
        "300s",
        "--ignore-not-found",
    ]);
    if let Some(context) = &settings.kube_context {
        command.args(["--kube-context", context]);
    }
    run(command, "helm uninstall").await?;
    wait_for_deleted_pods(
        settings,
        &namespace,
        &format!(
            "app.kubernetes.io/managed-by=Helm,app.kubernetes.io/instance={}",
            manifest.stack_id
        ),
    )
    .await?;
    if let Some(selector) = machine
        .deletion_inventory
        .as_ref()
        .and_then(|inventory| inventory.runtime_selector.as_deref())
    {
        wait_for_deleted_pods(settings, &namespace, selector).await?;
    }
    if policy == DeletionPolicy::Erase {
        prepare_volume_erasure(
            settings,
            machine
                .deletion_inventory
                .as_mut()
                .expect("checked erase inventory"),
        )
        .await?;
        save_machine(&site.deployment_id, &machine)?;
        erase_inventory(
            settings,
            machine
                .deletion_inventory
                .as_ref()
                .expect("checked erase inventory"),
        )
        .await?;
        wait_for_erased_volumes(
            settings,
            machine
                .deletion_inventory
                .as_ref()
                .expect("checked erase inventory"),
        )
        .await?;
        report.state["currentRelease"] = serde_json::Value::Null;
    }
    report.state["status"] = serde_json::json!(if policy == DeletionPolicy::Erase {
        "deleted"
    } else {
        "retained"
    });
    if !report.state["runtimeMetadata"].is_object() {
        report.state["runtimeMetadata"] = serde_json::json!({});
    }
    report.state["runtimeMetadata"]["deletionPolicy"] = serde_json::to_value(policy)
        .into_alien_error()
        .context(config_error("encoding deletion policy"))?;
    report.state["targetRelease"] = serde_json::Value::Null;
    report.state["error"] = serde_json::Value::Null;
    if let Some(resources) = report
        .state
        .pointer_mut("/stackState/resources")
        .and_then(serde_json::Value::as_object_mut)
    {
        for resource in resources.values_mut() {
            if policy == DeletionPolicy::Erase || resource["lifecycle"].as_str() == Some("live") {
                resource["status"] = serde_json::json!("deleted");
            }
        }
    }
    report.installed_release = None;
    report.written_at = chrono::Utc::now().to_rfc3339();
    report.heartbeats.clear();
    save_report(folder, &report)?;
    if let Some(inventory) = &mut machine.deletion_inventory {
        inventory.report = Some(
            serde_json::to_value(&report)
                .into_alien_error()
                .context(config_error("encoding completed deletion"))?,
        );
        save_machine(&site.deployment_id, &machine)?;
    }
    remove_transport_secrets(settings, &manifest.stack_id, &namespace).await?;
    for (_, pending) in by_sequence(&folder.join(TO_SITE), Some(BUNDLE_EXTENSION))? {
        std::fs::remove_file(pending)
            .into_alien_error()
            .context(config_error("removing applied deletion"))?;
    }
    for (_, installed) in installed_bundles(folder)? {
        std::fs::remove_dir_all(installed)
            .into_alien_error()
            .context(config_error("removing deleted install history"))?;
    }
    if sealed_snapshot.exists() {
        std::fs::remove_file(sealed_snapshot)
            .into_alien_error()
            .context(config_error("removing final telemetry receipt"))?;
    }
    if snapshot.exists() {
        std::fs::remove_file(snapshot)
            .into_alien_error()
            .context(config_error("removing deletion snapshot"))?;
    }
    println!(
        "Uninstalled {}. Copy the folder out to complete deletion.",
        site.name
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
        let parsed_url = url::Url::parse(url)
            .into_alien_error()
            .context(config_error("parsing the deploy CLI download URL"))?;
        let name = parsed_url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| {
                !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
            .ok_or_else(|| {
                AlienError::new(config_error("deploy CLI URL needs a binary filename"))
            })?;
        builds.push((format!("{name}-{platform}"), bytes));
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
        let unchanged = match std::fs::read(&destination) {
            Ok(existing) => {
                existing
                    == std::fs::read(&path)
                        .into_alien_error()
                        .context(config_error("reading the bundled tool"))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(config_error("reading the carried tool"));
            }
        };
        if unchanged {
            #[cfg(unix)]
            if name != TOOLS_SOURCES_FILE {
                std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
                    .into_alien_error()
                    .context(config_error("making alien-deploy executable"))?;
            }
            continue;
        }
        let temporary = tempfile::NamedTempFile::new_in(folder.join(TOOLS_DIR))
            .into_alien_error()
            .context(config_error("creating the replacement tool"))?;
        std::fs::copy(&path, temporary.path())
            .into_alien_error()
            .context(config_error("copying alien-deploy"))?;
        #[cfg(unix)]
        if name != TOOLS_SOURCES_FILE {
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o755))
                .into_alien_error()
                .context(config_error("making alien-deploy executable"))?;
        }
        // The old inode can still be executing the site-side sync command.
        temporary
            .persist(&destination)
            .map_err(|error| error.error)
            .into_alien_error()
            .context(config_error("replacing the carried tool"))?;
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
            }));
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
            }));
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
    let target_bytes = std::fs::read(work.path().join(airgap::TARGET_FILE))
        .into_alien_error()
        .context(config_error("reading the target"))?;
    let verified_target: serde_json::Value = serde_json::from_slice(&target_bytes)
        .into_alien_error()
        .context(config_error("parsing the target"))?;
    let deleting = verified_target["action"].as_str() == Some("delete");
    if deleting != (manifest.release_id == "delete") {
        return Err(AlienError::new(config_error(
            "the signed deletion action and manifest do not agree",
        )));
    }
    if deleting {
        if target_sequence(settings, &manifest.stack_id, &namespace)
            .await?
            .is_some_and(|sequence| sequence > manifest.sequence)
        {
            return Err(AlienError::new(config_error(
                "the deletion bundle is older than the installed target; download the current deletion request",
            )));
        }
        return uninstall_site(folder, site, settings, &manifest, args.dry_run).await;
    }
    let observation = read_status(settings, &manifest.stack_id, &namespace).await?;
    if !args.dry_run {
        if let Some((status, _)) = &observation {
            remember_successful_rollout(folder, &status["state"])?;
        }
    }
    let installed = observation.and_then(|(status, _)| current_release(&status));

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
    let mut machine = load_machine(&site.deployment_id)?;
    machine.deletion_inventory = None;
    save_machine(&site.deployment_id, &machine)?;
    // Helm has installed the Operator even if rollout later fails. Keep the
    // record now so reports, signed deletion, and rollback remain available.
    // Save signed originals: rollback verifies them again because the folder
    // leaves the site.
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
    let attempt = write_target(
        settings,
        &manifest.stack_id,
        &namespace,
        &target_path,
        manifest.sequence,
        ack,
    )
    .await?;
    if let Err(error) = wait_for_rollout(
        settings,
        &manifest.stack_id,
        &namespace,
        &manifest.release_id,
        manifest.sequence,
        &attempt,
    )
    .await
    {
        // Carry the failed rollout and its logs back; the install record remains
        // available for a later retry or signed deletion.
        write_report_for_install(folder, site, settings, &record).await?;
        return Err(error);
    }

    mark_successful_release(folder, &manifest.release_id)?;
    prune_installed(folder)?;
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
        rollout_succeeded: false,
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
    Ok(release_values(settings, release, namespace)
        .await?
        .and_then(|values| {
            values["airgapped"]["bundleSigningKey"]
                .as_str()
                .filter(|key| !key.is_empty())
                .map(str::to_string)
        }))
}

async fn release_values(
    settings: &SiteSide,
    release: &str,
    namespace: &str,
) -> Result<Option<serde_json::Value>> {
    read_release_values(settings, release, namespace, true).await
}

async fn read_release_values(
    settings: &SiteSide,
    release: &str,
    namespace: &str,
    include_defaults: bool,
) -> Result<Option<serde_json::Value>> {
    let mut cmd = Command::new("helm");
    cmd.args(["get", "values", release, "-n", namespace, "-o", "json"]);
    if include_defaults {
        cmd.arg("--all");
    }
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
    Ok(Some(values))
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

/// The target Secret also covers rollback and installs from another transfer folder.
async fn target_sequence(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<Option<u64>> {
    let mut cmd = kubectl(settings.kube_context.as_deref());
    cmd.args([
        "get",
        "secret",
        &format!("{stack_id}-airgap-target"),
        "-n",
        namespace,
        "-o",
        "jsonpath={.data.sequence}",
        "--ignore-not-found",
    ]);
    let output = run(cmd, "reading the installed target sequence").await?;
    let encoded = String::from_utf8_lossy(&output);
    if encoded.trim().is_empty() {
        return Ok(None);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .into_alien_error()
        .context(config_error("decoding the installed target sequence"))?;
    String::from_utf8_lossy(&decoded)
        .trim()
        .parse()
        .map(Some)
        .into_alien_error()
        .context(config_error("parsing the installed target sequence"))
}

/// Release the Operator last reported running, from its status Secret.
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

fn rollout_finished(
    status: &serde_json::Value,
    release_id: &str,
    sequence: u64,
    secrets_checked_after: Option<chrono::DateTime<chrono::Utc>>,
    attempt: Option<&str>,
) -> Result<bool> {
    // A chart restart can leave the preceding target's status in the Secret.
    // Sequence also distinguishes a retry or rollback to the same release.
    if status["sequence"].as_u64() != Some(sequence) {
        return Ok(false);
    }
    // Older Operators do not export attempt IDs. Preserve their sequence-based
    // behavior; current Operators acknowledge each explicit site invocation.
    if let Some(attempt) = attempt {
        if status.get("attempt").is_some() && status["attempt"].as_str() != Some(attempt) {
            return Ok(false);
        }
    }
    let state = &status["state"];
    // Upgrading the Operator can leave a legacy failure snapshot without an
    // attempt marker. Give the new target loop time to acknowledge this retry.
    if attempt.is_some()
        && status.get("attempt").is_none()
        && secrets_checked_after.is_some()
        && state["status"]
            .as_str()
            .is_some_and(|status| status.ends_with("-failed"))
    {
        return Ok(false);
    }
    if state["retryRequested"].as_bool() == Some(true) {
        return Ok(false);
    }
    if state["status"].as_str() == Some("waiting-for-secrets") {
        // A same-bundle retry can read the snapshot from before Secrets were created.
        if let Some(not_before) = secrets_checked_after {
            let written_at = status["writtenAt"]
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
            if written_at.is_none_or(|written_at| written_at <= not_before) {
                return Ok(false);
            }
        }
        let mut message = String::from(
            "The site needs required Secrets before it can continue. Create them in the site cluster, then run sync again.",
        );
        if let Some(secrets) = state["runtimeMetadata"]["deployerSecrets"].as_array() {
            for secret in secrets {
                if secret["required"].as_bool() != Some(true)
                    || secret["status"].as_str() != Some("missing")
                {
                    continue;
                }
                let location = &secret["location"];
                if let Some(name) = location["name"].as_str() {
                    message.push_str(&format!("\n\nSecret: {name}"));
                }
                if let Some(command) = location["cliCommand"].as_str() {
                    message.push_str(&format!("\n{command}"));
                }
            }
        }
        return Err(AlienError::new(ErrorData::ConfigurationError { message }));
    }
    let running = state["status"].as_str() == Some("running");
    if running && current_release(&status).as_deref() == Some(release_id) {
        return Ok(true);
    }
    if state["status"]
        .as_str()
        .is_some_and(|s| s.ends_with("failed"))
    {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "The rollout of {release_id} failed: {}. Run sync again to retry the same bundle; `alien-deploy rollback` returns to the previous release.",
                state["error"]["message"]
                    .as_str()
                    .unwrap_or("see the Operator's logs")
            ),
        }));
    }
    Ok(false)
}

async fn wait_for_rollout(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
    release_id: &str,
    sequence: u64,
    attempt: &str,
) -> Result<()> {
    println!("Waiting for {release_id} to roll out");
    let started = std::time::Instant::now();
    let started_at = chrono::Utc::now();
    loop {
        if let Some((status, _)) = read_status(settings, stack_id, namespace).await? {
            let secrets_checked_after =
                (started.elapsed() < SECRET_STATUS_TIMEOUT).then_some(started_at);
            if rollout_finished(
                &status,
                release_id,
                sequence,
                secrets_checked_after,
                Some(attempt),
            )? {
                return Ok(());
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
    write_report_for_install(folder, site, settings, &record).await
}

async fn write_report_for_install(
    folder: &Path,
    site: &SiteState,
    settings: &SiteSide,
    record: &InstalledBundle,
) -> Result<()> {
    if let Some(report) = collect_report(site, settings, record).await? {
        remember_successful_rollout(folder, &report.state)?;
        save_report(folder, &report)?;
    }
    Ok(())
}

async fn collect_report(
    site: &SiteState,
    settings: &SiteSide,
    record: &InstalledBundle,
) -> Result<Option<SiteReport>> {
    let namespace = settings.namespace(&record.stack_id);
    let Some((status, export_token)) = read_status(settings, &record.stack_id, &namespace).await?
    else {
        return Ok(None);
    };
    let after = current_ack(settings, &record.stack_id, &namespace).await?;
    let (telemetry, dropped) = export_telemetry(
        settings,
        &record.stack_id,
        &namespace,
        &export_token,
        after,
        None,
    )
    .await?;
    report_from_status(site, record, status, telemetry, dropped).map(Some)
}

fn report_from_status(
    site: &SiteState,
    record: &InstalledBundle,
    status: serde_json::Value,
    telemetry: Vec<ReportedBatch>,
    dropped: u64,
) -> Result<SiteReport> {
    let report = SiteReport {
        deployment_id: site.deployment_id.clone(),
        written_at: status["writtenAt"]
            .as_str()
            .ok_or_else(|| AlienError::new(config_error("the Operator status has no writtenAt")))?
            .to_string(),
        installed_release: current_release(&status),
        state: status["state"].clone(),
        heartbeats: serde_json::from_value(
            status
                .get("heartbeats")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .into_alien_error()
        .context(config_error("reading resource heartbeats"))?,
        telemetry,
        dropped,
        inventory: record.inventory.clone(),
    };
    Ok(report)
}

fn save_report(folder: &Path, report: &SiteReport) -> Result<()> {
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
    seal: Option<u64>,
) -> Result<(Vec<ReportedBatch>, u64)> {
    let values = release_values(settings, release, namespace)
        .await?
        .unwrap_or_default();
    let deployment = operator_deployment_name(&values, release);
    let mut port_cmd = kubectl(settings.kube_context.as_deref());
    port_cmd.args([
        "get",
        "deployment",
        &deployment,
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
            &format!("deployment/{deployment}"),
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
    if let Some(sequence) = seal {
        let response = client
            .post(format!(
                "http://127.0.0.1:{local_port}/airgap/telemetry/seal"
            ))
            .bearer_auth(token)
            .json(&serde_json::json!({ "sequence": sequence }))
            .send()
            .await
            .into_alien_error()
            .context(config_error("stopping telemetry intake before deletion"))?;
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(AlienError::new(config_error(format!(
                "The Operator could not stop telemetry intake ({}). Upgrade the Operator before retrying deletion.", response.status()
            ))));
        }
    }
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
    // Match --reset-then-reuse-values: new chart defaults, saved user values,
    // then the files supplied for this installation.
    let mut values = Vec::new();
    if let Some(saved) = read_release_values(settings, &record.stack_id, namespace, false)
        .await?
        .filter(|saved| !saved.is_null())
    {
        values.push(
            serde_yaml::to_value(saved)
                .into_alien_error()
                .context(config_error("reading the installed Helm values"))?,
        );
    }
    for path in &settings.values {
        let bytes = std::fs::read(path)
            .into_alien_error()
            .context(config_error(format!(
                "reading Helm values {}",
                path.display()
            )))?;
        values.push(
            serde_yaml::from_slice(&bytes)
                .into_alien_error()
                .context(config_error(format!(
                    "parsing Helm values {}",
                    path.display()
                )))?,
        );
    }
    // Helm pins this image from the bundle even when chart defaults omit it.
    if let Some(image) = record.chart_images.get(airgap::CHART_IMAGE_VALUES[0]) {
        values.push(
            serde_yaml::to_value(serde_json::json!({"runtime": {"image": {
                "repository": image.repository, "tag": image.tag
            }}}))
            .into_alien_error()
            .context(config_error("reading the bundled Operator image"))?,
        );
    }
    let images = airgap::chart_images(chart, &serde_json::Value::Null, &values)
        .context(config_error("checking the site's chart images"))?;
    for key in images.keys().filter(|key| key.starts_with("postgres.")) {
        if !record.chart_images.contains_key(key) {
            let resource = key
                .trim_start_matches("postgres.")
                .trim_end_matches(".image");
            return Err(AlienError::new(ErrorData::ValidationError {
                field: format!("postgres.{resource}.enabled"),
                message: format!(
                    "Database '{resource}' is enabled at this site, but its image is not in the bundle. Remove this pending bundle from to-site/, then pass the same values file online and at the site and re-download the update."
                ),
            }));
        }
    }
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
    // Credentials for other registries stay: a rollback may return to a
    // release whose images are in a registry the site used before.
    let mut config = stored_registry_config(settings, stack_id, namespace).await?;
    config["auths"][registry_host] =
        serde_json::json!({ "username": username, "password": password, "auth": auth });
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
        .args(["--dry-run=client", "-o", "json"]);
    let secret_yaml = run(secret_yaml, "kubectl create secret").await?;
    let secret_yaml =
        mark_created_sync_secret(settings, &secret_name, namespace, secret_yaml).await?;

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
    let config = stored_registry_config(settings, stack_id, namespace).await?;
    let auth = &config["auths"][registry_host(&settings.registry)];
    Ok(
        match (auth["username"].as_str(), auth["password"].as_str()) {
            (Some(username), Some(password)) => Some((username.to_string(), password.to_string())),
            _ => None,
        },
    )
}

/// The Docker config in the install's pull Secret, with credentials for each
/// registry the site has used. Empty before the first install with
/// credentials.
async fn stored_registry_config(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<serde_json::Value> {
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
        return Ok(serde_json::json!({ "auths": {} }));
    }
    serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .into_alien_error()
            .context(config_error("decoding the stored registry credentials"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the stored registry credentials"))
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
) -> Result<String> {
    let attempt = Uuid::new_v4().to_string();
    let secret_name = format!("{stack_id}-airgap-target");
    let mut create = kubectl(settings.kube_context.as_deref());
    create
        .args(["create", "secret", "generic", &secret_name, "-n", namespace])
        .arg(format!("--from-file=target.json={}", target_path.display()))
        .arg(format!("--from-literal=sequence={sequence}"))
        .arg(format!("--from-literal=attempt={attempt}"))
        .arg(format!("--from-literal=telemetry-ack={telemetry_ack}"))
        .args(["--dry-run=client", "-o", "json"]);
    let yaml = run(create, "kubectl create secret").await?;
    let yaml = mark_created_sync_secret(settings, &secret_name, namespace, yaml).await?;
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
    run(apply, "kubectl apply").await?;
    Ok(attempt)
}

/// Preserve customer ownership even when sync updates a pre-existing Secret.
async fn mark_created_sync_secret(
    settings: &SiteSide,
    name: &str,
    namespace: &str,
    manifest: Vec<u8>,
) -> Result<Vec<u8>> {
    let mut existing = kubectl(settings.kube_context.as_deref());
    existing.args([
        "get",
        "secret",
        name,
        "-n",
        namespace,
        "--ignore-not-found",
        "-o",
        "json",
    ]);
    let existing = run(existing, "checking sync Secret ownership").await?;
    if !existing.is_empty() {
        let secret: serde_json::Value = serde_json::from_slice(&existing)
            .into_alien_error()
            .context(config_error("reading sync Secret ownership"))?;
        if secret["metadata"]["labels"]["app.kubernetes.io/managed-by"].as_str()
            != Some(SYNC_SECRET_MANAGER)
        {
            return Ok(manifest);
        }
    }
    let mut secret: serde_json::Value = serde_json::from_slice(&manifest)
        .into_alien_error()
        .context(config_error("reading the Secret manifest"))?;
    secret["metadata"]["labels"]["app.kubernetes.io/managed-by"] = SYNC_SECRET_MANAGER.into();
    serde_json::to_vec(&secret)
        .into_alien_error()
        .context(config_error("encoding the Secret manifest"))
}

async fn remove_transport_secrets(
    settings: &SiteSide,
    stack_id: &str,
    namespace: &str,
) -> Result<()> {
    for name in [
        format!("{stack_id}-airgap-target"),
        format!("{stack_id}-airgap-status"),
        pull_secret_name(stack_id),
    ] {
        let mut get = kubectl(settings.kube_context.as_deref());
        get.args([
            "get",
            "secret",
            &name,
            "-n",
            namespace,
            "--ignore-not-found",
            "-o",
            "json",
        ]);
        let bytes = run(get, "checking sync Secret ownership").await?;
        if bytes.is_empty() {
            continue;
        }
        let secret: serde_json::Value = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(config_error("reading sync Secret ownership"))?;
        if secret["metadata"]["labels"]["app.kubernetes.io/managed-by"].as_str()
            != Some(SYNC_SECRET_MANAGER)
        {
            continue;
        }
        let mut delete = kubectl(settings.kube_context.as_deref());
        delete.args([
            "delete",
            "secret",
            &name,
            "-n",
            namespace,
            "--ignore-not-found",
        ]);
        run(delete, "removing sync Secret").await?;
    }
    Ok(())
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
    #[cfg(unix)]
    use alien_core::bundle_signature::BundleSigningKey;
    use httpmock::prelude::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::MetadataExt as _;

    use super::*;

    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn site_database_requires_carried_image() {
        let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
        let namespace = format!("sync-database-image-{}", std::process::id());
        let chart = tempfile::tempdir().unwrap();
        std::fs::write(
            chart.path().join("Chart.yaml"),
            "apiVersion: v2\nname: app\nversion: 0.1.0\n",
        )
        .unwrap();
        std::fs::write(chart.path().join("values.yaml"), "runtime:\n  image:\n    repository: registry.example.com/operator\n    tag: fixture\npostgres:\n  database:\n    enabled: false\n    image:\n      repository: pgvector/pgvector\n      tag: pg16\n").unwrap();
        let values = chart.path().join("site.yaml");
        std::fs::write(&values, "postgres:\n  database:\n    enabled: true\n").unwrap();
        let mut package = Command::new("helm");
        package
            .arg("package")
            .arg(chart.path())
            .arg("--destination")
            .arg(chart.path());
        run(package, "packaging database fixture").await.unwrap();
        let mut settings = SiteSide {
            registry: "registry.example.com/site".into(),
            registry_credentials: None,
            insecure_registry: false,
            namespace: Some(namespace.clone()),
            values: vec![values],
            kube_context: Some(context),
        };
        let mut record = InstalledBundle {
            rollout_succeeded: false,
            release_id: "release".into(),
            stack_id: "app".into(),
            chart: "app-0.1.0.tgz".into(),
            chart_images: BTreeMap::from([(
                "runtime.image".into(),
                SiteImage {
                    repository: "registry.example.com/site/operator".into(),
                    tag: "fixture".into(),
                },
            )]),
            telemetry_ack: None,
            inventory: BTreeSet::new(),
            registry: Some(settings.registry.clone()),
        };
        let result = helm_upgrade(
            &settings,
            &record,
            &chart.path().join(&record.chart),
            &namespace,
            None,
            "fixture",
            "dep_fixture",
            "fixture",
        )
        .await;
        let installed = release_values(&settings, "app", &namespace).await.unwrap();
        let mut cleanup = kubectl(settings.kube_context.as_deref());
        cleanup.args([
            "delete",
            "namespace",
            &namespace,
            "--ignore-not-found",
            "--wait=true",
        ]);
        run(cleanup, "removing database fixture").await.unwrap();
        let error = result.expect_err("site enabled database without a carried image");
        assert_eq!(error.code, "VALIDATION_ERROR");
        let message = error.to_string();
        assert!(message.contains("database"), "{message}");
        assert!(message.contains("same values"), "{message}");
        assert!(message.contains("online"), "{message}");
        assert!(message.contains("download"), "{message}");
        assert!(installed.is_none(), "Helm must not install the release");

        record.chart_images.insert(
            "postgres.database.image".into(),
            SiteImage {
                repository: "registry.example.com/site/database".into(),
                tag: "pg16".into(),
            },
        );
        let carried = helm_upgrade(
            &settings,
            &record,
            &chart.path().join(&record.chart),
            &namespace,
            None,
            "fixture",
            "dep_fixture",
            "fixture",
        )
        .await;
        let carried_values = release_values(&settings, "app", &namespace).await.unwrap();
        record.chart_images.remove("postgres.database.image");
        settings.values.clear();
        let remembered = helm_upgrade(
            &settings,
            &record,
            &chart.path().join(&record.chart),
            &namespace,
            None,
            "fixture",
            "dep_fixture",
            "fixture",
        )
        .await;
        let values = chart.path().join("site.yaml");
        settings.values = vec![values.clone()];
        std::fs::write(&values, "postgres:\n  database:\n    enabled: false\n").unwrap();
        let disabled = helm_upgrade(
            &settings,
            &record,
            &chart.path().join(&record.chart),
            &namespace,
            None,
            "fixture",
            "dep_fixture",
            "fixture",
        )
        .await;
        std::fs::write(&values, "postgres:\n  database:\n    enabled: true\nstackSettings:\n  externalBindings:\n    database:\n      type: postgres\n      service: external\n").unwrap();
        let external = helm_upgrade(
            &settings,
            &record,
            &chart.path().join(&record.chart),
            &namespace,
            None,
            "fixture",
            "dep_fixture",
            "fixture",
        )
        .await;
        let mut cleanup = kubectl(settings.kube_context.as_deref());
        cleanup.args([
            "delete",
            "namespace",
            &namespace,
            "--ignore-not-found",
            "--wait=true",
        ]);
        run(cleanup, "removing database fixture").await.unwrap();
        carried.unwrap();
        assert_eq!(
            carried_values.unwrap()["postgres"]["database"]["image"]["repository"],
            "registry.example.com/site/database"
        );
        assert_eq!(remembered.unwrap_err().code, "VALIDATION_ERROR");
        disabled.unwrap();
        external.unwrap();
    }
    /// Rollback changes chart-owned data even if the subsequent rollout is interrupted.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn rollback_discards_interrupted_deletion_inventory() {
        let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
        let namespace = format!("sync-rollback-inventory-{}", std::process::id());
        let (folder, mut site) = folder_with_report();
        site.deployment_id = format!("dep_rollback_inventory_{}", std::process::id());
        save_site(folder.path(), &site).unwrap();
        let key = BundleSigningKey::from_seed([7; 32]);
        let chart = tempfile::tempdir().unwrap();
        std::fs::create_dir(chart.path().join("templates")).unwrap();
        std::fs::write(
            chart.path().join("Chart.yaml"),
            "apiVersion: v2\nname: app\nversion: 0.1.0\n",
        )
        .unwrap();
        std::fs::write(
            chart.path().join("templates/data.yaml"),
            r#"
apiVersion: v1
kind: Secret
metadata:
  name: restored-database
  labels:
    app.kubernetes.io/instance: app
    app.kubernetes.io/managed-by: Helm
  annotations:
    helm.sh/resource-policy: keep
stringData:
  password: fixture
"#,
        )
        .unwrap();
        for (sequence, release) in [(1, "previous"), (2, "latest")] {
            let installed = folder
                .path()
                .join(TO_SITE)
                .join(INSTALLED)
                .join(format!("{sequence}-{release}"));
            std::fs::create_dir_all(&installed).unwrap();
            let mut package = Command::new("helm");
            package
                .arg("package")
                .arg(chart.path())
                .arg("--destination")
                .arg(&installed);
            run(package, "packaging rollback fixture").await.unwrap();
            std::fs::write(installed.join(airgap::TARGET_FILE), b"{}").unwrap();
            let manifest = BundleManifest {
                format_version: airgap::FORMAT_VERSION,
                deployment_id: site.deployment_id.clone(),
                deployment_name: site.name.clone(),
                release_id: release.into(),
                sequence,
                created_at: chrono::Utc::now().to_rfc3339(),
                stack_id: "app".into(),
                images: vec![airgap::BundleImage {
                    source: "registry.example.com/operator:fixture".into(),
                    repository: "operator".into(),
                    tag: Some("fixture".into()),
                    digest: format!("sha256:{}", "a".repeat(64)),
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                }],
                chart_images: BTreeMap::from([(
                    "runtime.image".into(),
                    "registry.example.com/operator:fixture".into(),
                )]),
                chart: "app-0.1.0.tgz".into(),
                files: airgap::checksums(&installed).await.unwrap(),
                omitted_blobs: BTreeSet::new(),
                telemetry_ack: None,
                deletion_policy: None,
            };
            let bytes = serde_json::to_vec(&manifest).unwrap();
            std::fs::write(installed.join(airgap::MANIFEST_FILE), &bytes).unwrap();
            std::fs::write(installed.join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
            let mut record =
                installed_record(&manifest, "registry.example.com/site", BTreeSet::new()).unwrap();
            record.rollout_succeeded = true;
            std::fs::write(
                installed.join("install.json"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        let mut install = Command::new("helm");
        install.args(["install", "app"]).arg(chart.path()).args([
            "--namespace",
            &namespace,
            "--create-namespace",
            "--kube-context",
            &context,
            "--set-string",
            &format!("airgapped.bundleSigningKey={}", key.public_key()),
            "--set-string",
            &format!("management.deploymentId={}", site.deployment_id),
        ]);
        run(install, "installing rollback fixture").await.unwrap();
        let machine = MachineState {
            registry: Some("registry.example.com/site".into()),
            namespace: Some(namespace.clone()),
            kube_context: Some(context.clone()),
            deletion_inventory: Some(DeletionInventory {
                namespace: namespace.clone(),
                stack_id: "app".into(),
                kube_context: Some(context.clone()),
                objects: vec![DeletionObject {
                    resource: "secret".into(),
                    name: "restored-database".into(),
                    uid: "old-uid".into(),
                    volume: None,
                }],
                claim_prefixes: Vec::new(),
                runtime_selector: None,
                report: None,
            }),
            ..Default::default()
        };
        save_machine(&site.deployment_id, &machine).unwrap();
        let settings = SiteSide::from_state(&machine, Some(context.clone()))
            .unwrap()
            .unwrap();
        // Observe the real rollback's written target, then interrupt its readiness wait.
        // Inventory must already be invalidated, even when readiness never completes.
        let result = {
            let rollback = rollback_command(RollbackArgs {
                folder: Some(folder.path().into()),
                kube_context: Some(context.clone()),
            });
            tokio::pin!(rollback);
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    tokio::select! {
                        result = &mut rollback => panic!("rollback ended before target observation: {result:?}"),
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            if target_sequence(&settings, "app", &namespace).await.unwrap().is_some() { break; }
                        }
                    }
                }
            }).await
        };
        let after = load_machine(&site.deployment_id).unwrap();
        let fresh = deletion_inventory(&settings, "app", &namespace)
            .await
            .unwrap();
        let mut delete = kubectl(Some(&context));
        delete.args(["delete", "namespace", &namespace]);
        run(delete, "removing rollback fixture").await.unwrap();
        result.expect("rollback must write a target");
        assert!(fresh
            .objects
            .iter()
            .any(|object| object.name == "restored-database" && object.uid != "old-uid"));
        assert!(
            after.deletion_inventory.is_none(),
            "rollback kept the previous deletion's object UIDs"
        );
    }

    #[test]
    fn rollback_skips_consecutive_failed_rollouts() {
        let folder = tempfile::tempdir().unwrap();
        for (sequence, release, succeeded) in [(1, "a", true), (2, "b", false), (3, "c", false)] {
            let dir = folder
                .path()
                .join(TO_SITE)
                .join(INSTALLED)
                .join(format!("{sequence}-{release}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("install.json"),
                serde_json::json!({
                    "releaseId": release, "stackId":"app", "chart":"chart.tgz", "chartImages":{},
                    "telemetryAck":null,"inventory":[],"registry":null,"rolloutSucceeded":succeeded
                })
                .to_string(),
            )
            .unwrap();
        }
        let history = installed_bundles(folder.path()).unwrap();
        let previous = rollback_candidate(&history).unwrap();
        assert_eq!(read_installed(previous).unwrap().release_id, "a");
        assert_eq!(
            read_installed(&history[0].1).unwrap().release_id,
            "c",
            "latest failed attempt remains available for reports and deletion"
        );
    }

    #[test]
    fn rollback_history_uses_observed_success_and_keeps_it_when_pruning() {
        let folder = tempfile::tempdir().unwrap();
        for (sequence, release) in [(1, "a"), (2, "b"), (3, "c"), (4, "d")] {
            let dir = folder
                .path()
                .join(TO_SITE)
                .join(INSTALLED)
                .join(format!("{sequence}-{release}"));
            std::fs::create_dir_all(&dir).unwrap();
            // An older record has no rollout receipt.
            std::fs::write(
                dir.join("install.json"),
                serde_json::json!({
                    "releaseId":release,"stackId":"app","chart":"chart.tgz","chartImages":{},
                    "telemetryAck":null,"inventory":[],"registry":null
                })
                .to_string(),
            )
            .unwrap();
        }
        assert!(rollback_candidate(&installed_bundles(folder.path()).unwrap()).is_err());
        remember_successful_rollout(
            folder.path(),
            &serde_json::json!({
                "status":"update-failed", "currentRelease":{"releaseId":"b"}
            }),
        )
        .unwrap();
        assert!(rollback_candidate(&installed_bundles(folder.path()).unwrap()).is_err());
        for release in ["a", "d"] {
            remember_successful_rollout(
                folder.path(),
                &serde_json::json!({
                    "status":"running", "currentRelease":{"releaseId":release}
                }),
            )
            .unwrap();
        }
        prune_installed(folder.path()).unwrap();
        let history = installed_bundles(folder.path()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(read_installed(&history[0].1).unwrap().release_id, "d");
        assert_eq!(
            read_installed(rollback_candidate(&history).unwrap())
                .unwrap()
                .release_id,
            "a"
        );
    }

    /// Supply a signed first-install bundle and site arguments for a disposable
    /// cluster whose workload cannot finish rollout. No successful install is needed.
    #[tokio::test]
    #[ignore = "requires ALIEN_SYNC_TEST_ARGS with a signed failing install and explicit context"]
    async fn failed_first_rollout_keeps_verified_install_for_deletion() {
        let arguments: Vec<String> = serde_json::from_str(
            &std::env::var("ALIEN_SYNC_TEST_ARGS").expect("site arguments as JSON"),
        )
        .unwrap();
        let args =
            SyncArgs::try_parse_from(std::iter::once("sync".to_string()).chain(arguments)).unwrap();
        assert!(
            args.kube_context.is_some(),
            "an explicit disposable context is required"
        );
        let (folder, mut site) = open_folder(&args).await.unwrap();
        assert!(
            installed_bundles(&folder).unwrap().is_empty(),
            "first install only"
        );
        let mut machine = MachineState::default();
        let settings = site_side_settings(&args, &mut machine).unwrap().unwrap();
        let error = install_pending(&folder, &mut site, &settings, &args)
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("rolling out")
                || message.contains("rollout")
                || message.contains("required Secrets"),
            "must reach rollout: {message}"
        );
        let installs = installed_bundles(&folder).unwrap();
        assert_eq!(
            installs.len(),
            1,
            "failed rollout must remain uninstallable"
        );
        let record = read_installed(&installs[0].1).unwrap();
        let verified =
            airgap::verify_installed(&installs[0].1, args.trusted_key.as_deref().unwrap())
                .await
                .unwrap();
        assert_eq!(record.stack_id, verified.stack_id);
        assert_eq!(record.release_id, verified.release_id);
        assert_eq!(verified.deployment_id, site.deployment_id);
        let reports = by_sequence(&folder.join(FROM_SITE), Some(REPORT_EXTENSION)).unwrap();
        assert!(
            !reports.is_empty(),
            "failed rollout report must be carried back"
        );
        let report: SiteReport =
            serde_json::from_slice(&std::fs::read(&reports[0].1).unwrap()).unwrap();
        assert_ne!(report.state["status"], "running");
    }

    /// Supply a signed retain request after an interrupted deletion and a newer install.
    #[tokio::test]
    #[ignore = "requires ALIEN_SYNC_RETAIN_TEST_ARGS for a disposable site with a stale deletion snapshot"]
    async fn signed_retain_refreshes_recovery_state_after_another_install() {
        let arguments: Vec<String> = serde_json::from_str(
            &std::env::var("ALIEN_SYNC_RETAIN_TEST_ARGS").expect("site arguments as JSON"),
        )
        .unwrap();
        let args =
            SyncArgs::try_parse_from(std::iter::once("sync".to_string()).chain(arguments)).unwrap();
        assert!(
            args.kube_context.is_some(),
            "explicit disposable context required"
        );
        let (folder, mut site) = open_folder(&args).await.unwrap();
        let mut machine = load_machine(&site.deployment_id).unwrap();
        let settings = site_side_settings(&args, &mut machine).unwrap().unwrap();
        let snapshot: SiteReport =
            serde_json::from_slice(&std::fs::read(folder.join("deleting-report.json")).unwrap())
                .unwrap();
        let (_, installed) = installed_bundles(&folder)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let record = read_installed(&installed).unwrap();
        let (status, _) = read_status(
            &settings,
            &record.stack_id,
            &settings.namespace(&record.stack_id),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(
            snapshot.state["currentRelease"],
            status["state"]["currentRelease"]
        );
        let old_batches: Vec<_> = snapshot.telemetry.iter().map(|batch| batch.id).collect();
        install_pending(&folder, &mut site, &settings, &args)
            .await
            .unwrap();
        let reports = by_sequence(&folder.join(FROM_SITE), Some(REPORT_EXTENSION)).unwrap();
        assert_eq!(reports.len(), 1);
        let report: SiteReport =
            serde_json::from_slice(&std::fs::read(&reports[0].1).unwrap()).unwrap();
        assert_eq!(report.state["status"], "retained");
        assert_eq!(
            report.state["currentRelease"],
            status["state"]["currentRelease"]
        );
        assert_eq!(
            report.state["stackState"]["resources"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            status["state"]["stackState"]["resources"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
        );
        for id in old_batches {
            assert!(
                report.telemetry.iter().any(|batch| batch.id == id),
                "lost buffered batch {id}"
            );
        }
        let machine = load_machine(&site.deployment_id).unwrap();
        let saved = machine.deletion_inventory.unwrap().report.unwrap();
        assert_eq!(saved["state"], report.state);
    }

    /// Resume signed deletion after the Operator stops, unless another install reopens telemetry.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn signed_uninstall_resumes_without_operator_deployment() {
        for (stopped, deletion_sequence, target_sequence) in [
            (false, 2, 1),
            (true, 2, 1),
            (true, 3, 1),
            (true, 4, 3),
            (true, 5, 0),
        ] {
            let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
            let namespace = format!(
                "sync-delete-retry-{}-{stopped}-{deletion_sequence}",
                std::process::id()
            );
            let (folder, mut site) = folder_with_report();
            site.deployment_id = format!(
                "dep_retry_{}_{stopped}_{deletion_sequence}",
                std::process::id()
            );
            let snapshot = folder.path().join("deleting-report.json");
            std::fs::rename(report_path(folder.path()), &snapshot).unwrap();
            let mut original: SiteReport =
                serde_json::from_slice(&std::fs::read(&snapshot).unwrap()).unwrap();
            original.deployment_id = site.deployment_id.clone();
            std::fs::write(&snapshot, serde_json::to_vec(&original).unwrap()).unwrap();
            let settings = SiteSide {
                registry: "registry.example.com/site".into(),
                registry_credentials: None,
                insecure_registry: false,
                namespace: Some(namespace.clone()),
                values: Vec::new(),
                kube_context: Some(context.clone()),
            };
            let chart = tempfile::tempdir().unwrap();
            std::fs::create_dir(chart.path().join("templates")).unwrap();
            std::fs::write(
                chart.path().join("Chart.yaml"),
                "apiVersion: v2\nname: app\nversion: 0.1.0\n",
            )
            .unwrap();
            std::fs::write(
                chart.path().join("templates/runtime.yaml"),
                r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
spec:
  replicas: 0
  selector:
    matchLabels: {app: app}
  template:
    metadata:
      labels: {app: app}
    spec:
      containers:
        - name: operator
          image: registry.k8s.io/pause:3.10
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: runtime
"#,
            )
            .unwrap();
            let mut install = Command::new("helm");
            install.args(["install", "app"]).arg(chart.path()).args([
                "--namespace",
                &namespace,
                "--create-namespace",
                "--kube-context",
                &context,
                "--set",
                &format!("management.deploymentId={}", site.deployment_id),
                "--set",
                "runtime.cleanup.onUninstall.enabled=true",
            ]);
            run(install, "installing test release").await.unwrap();
            if stopped {
                // The final drain was saved, then the CLI stopped before uninstall.
                std::fs::write(folder.path().join("deleting-telemetry-sequence.json"), "2")
                    .unwrap();
            } else {
                let mut delete = kubectl(Some(&context));
                delete.args(["delete", "deployment", "app", "--namespace", &namespace]);
                run(delete, "removing the Operator before retry")
                    .await
                    .unwrap();
            }
            if target_sequence != 0 {
                let target = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(target.path(), b"{}").unwrap();
                write_target(
                    &settings,
                    "app",
                    &namespace,
                    target.path(),
                    target_sequence,
                    0,
                )
                .await
                .unwrap();
            }
            println!(
                "Retry: stopped={stopped}, deletion={deletion_sequence}, target={target_sequence}"
            );
            assert!(release_values(&settings, "app", &namespace)
                .await
                .unwrap()
                .is_some());

            let bundle = tempfile::tempdir().unwrap();
            std::fs::write(
                bundle.path().join(airgap::TARGET_FILE),
                br#"{"action":"delete"}"#,
            )
            .unwrap();
            let manifest = BundleManifest {
                format_version: airgap::FORMAT_VERSION,
                deployment_id: site.deployment_id.clone(),
                deployment_name: site.name.clone(),
                release_id: "delete".into(),
                sequence: deletion_sequence,
                created_at: chrono::Utc::now().to_rfc3339(),
                stack_id: "app".into(),
                images: Vec::new(),
                chart_images: BTreeMap::new(),
                chart: String::new(),
                files: airgap::checksums(bundle.path()).await.unwrap(),
                omitted_blobs: BTreeSet::new(),
                telemetry_ack: None,
                deletion_policy: None,
            };
            let bytes = serde_json::to_vec(&manifest).unwrap();
            let key = BundleSigningKey::from_seed([6; 32]);
            std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &bytes).unwrap();
            std::fs::write(bundle.path().join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
            std::fs::create_dir_all(folder.path().join(TO_SITE)).unwrap();
            let pending = folder
                .path()
                .join(TO_SITE)
                .join(format!("{deletion_sequence}-delete.bundle"));
            airgap::pack(bundle.path(), &pending).unwrap();
            let args =
                SyncArgs::try_parse_from(["sync", "--trusted-key", &key.public_key()]).unwrap();
            if target_sequence > 2 {
                // An intervening install can accept more logs, even from another folder.
                // Without an export credential, deletion must stop rather than skip its drain.
                let error = install_pending(folder.path(), &mut site, &settings, &args)
                    .await
                    .unwrap_err();
                assert!(error
                    .to_string()
                    .contains("export credential is unavailable"));
                assert!(snapshot.exists());
                assert!(pending.exists());
                assert!(release_values(&settings, "app", &namespace)
                    .await
                    .unwrap()
                    .is_some());
                let mut uninstall = Command::new("helm");
                uninstall.args([
                    "uninstall",
                    "app",
                    "--namespace",
                    &namespace,
                    "--kube-context",
                    &context,
                ]);
                run(uninstall, "removing test release").await.unwrap();
                let mut delete = kubectl(Some(&context));
                delete.args(["delete", "namespace", &namespace]);
                run(delete, "removing test namespace").await.unwrap();
                continue;
            }
            install_pending(folder.path(), &mut site, &settings, &args)
                .await
                .unwrap();

            assert!(release_values(&settings, "app", &namespace)
                .await
                .unwrap()
                .is_none());
            let mut get = kubectl(Some(&context));
            get.args([
                "get",
                "configmap",
                "runtime",
                "--namespace",
                &namespace,
                "--ignore-not-found",
                "-o",
                "name",
            ]);
            assert!(run(get, "checking Helm cleanup").await.unwrap().is_empty());
            let reports =
                by_sequence(&folder.path().join(FROM_SITE), Some(REPORT_EXTENSION)).unwrap();
            assert_eq!(reports.len(), 1);
            let report: SiteReport =
                serde_json::from_slice(&std::fs::read(&reports[0].1).unwrap()).unwrap();
            assert_eq!(report.state["status"], "retained");
            assert_eq!(
                serde_json::to_value(&report.telemetry).unwrap(),
                serde_json::to_value(&original.telemetry).unwrap()
            );
            assert!(!snapshot.exists());
            assert!(!folder
                .path()
                .join("deleting-telemetry-sequence.json")
                .exists());
            assert!(!pending.exists());
            let mut delete = kubectl(Some(&context));
            delete.args(["delete", "namespace", &namespace]);
            run(delete, "removing test namespace").await.unwrap();
        }
    }

    /// Retry a signed deletion after a cleanup hook has removed the Operator.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn signed_erase_removes_only_owned_data() {
        signed_cleanup_preserves_unowned_claims("erase", false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn signed_retain_preserves_statefulset_claims() {
        signed_cleanup_preserves_unowned_claims("retain", false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires Helm, kubectl and ALIEN_SYNC_TEST_CONTEXT"]
    async fn signed_erase_includes_claims_created_after_interrupted_inventory() {
        signed_cleanup_preserves_unowned_claims("erase", true).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires kubectl and ALIEN_SYNC_TEST_CONTEXT for a disposable Kind cluster"]
    async fn erase_rechecks_a_claim_bound_after_preparation() {
        let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
        let namespace = format!("sync-late-volume-{}", std::process::id());
        let settings = SiteSide {
            registry: "registry.example.com/site".into(),
            registry_credentials: None,
            insecure_registry: false,
            namespace: Some(namespace.clone()),
            values: Vec::new(),
            kube_context: Some(context.clone()),
        };
        let kubectl_output = |args: &[&str]| {
            std::process::Command::new("kubectl")
                .args(["--context", &context, "-n", &namespace])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(kubectl_output(&["create", "namespace", &namespace])
            .status
            .success());
        let work = tempfile::tempdir().unwrap();
        let storage_class = format!("sync-late-volume-{}", std::process::id());
        let class_path = work.path().join("class.json");
        std::fs::write(&class_path, serde_json::json!({
            "apiVersion":"storage.k8s.io/v1", "kind":"StorageClass", "metadata":{"name":storage_class},
            "provisioner":"rancher.io/local-path", "reclaimPolicy":"Retain", "volumeBindingMode":"WaitForFirstConsumer"
        }).to_string()).unwrap();
        assert!(
            kubectl_output(&["apply", "-f", class_path.to_str().unwrap()])
                .status
                .success()
        );
        let claim_path = work.path().join("claim.json");
        std::fs::write(&claim_path, serde_json::json!({
            "apiVersion":"v1", "kind":"PersistentVolumeClaim", "metadata":{"name":"data"},
            "spec":{"storageClassName":storage_class, "accessModes":["ReadWriteOnce"], "resources":{"requests":{"storage":"1Mi"}}}
        }).to_string()).unwrap();
        assert!(
            kubectl_output(&["apply", "-f", claim_path.to_str().unwrap()])
                .status
                .success()
        );
        let claim: serde_json::Value =
            serde_json::from_slice(&kubectl_output(&["get", "pvc", "data", "-o", "json"]).stdout)
                .unwrap();
        assert!(claim["spec"]["volumeName"].is_null());
        let mut inventory = DeletionInventory {
            namespace: namespace.clone(),
            stack_id: "app".into(),
            kube_context: Some(context.clone()),
            objects: vec![DeletionObject {
                resource: "persistentvolumeclaims".into(),
                name: "data".into(),
                uid: claim["metadata"]["uid"].as_str().unwrap().into(),
                volume: None,
            }],
            claim_prefixes: Vec::new(),
            runtime_selector: None,
            report: None,
        };
        prepare_volume_erasure(&settings, &mut inventory)
            .await
            .unwrap();
        assert!(inventory.objects[0].volume.is_none());
        eprintln!("prepared unbound claim uid={}", inventory.objects[0].uid);
        let pod_path = work.path().join("pod.json");
        std::fs::write(&pod_path, serde_json::json!({
            "apiVersion":"v1", "kind":"Pod", "metadata":{"name":"consumer"},
            "spec":{"containers":[{"name":"consumer","image":"registry.k8s.io/pause:3.10","volumeMounts":[{"name":"data","mountPath":"/data"}]}],
                "volumes":[{"name":"data","persistentVolumeClaim":{"claimName":"data"}}]}
        }).to_string()).unwrap();
        assert!(kubectl_output(&["apply", "-f", pod_path.to_str().unwrap()])
            .status
            .success());
        assert!(kubectl_output(&[
            "wait",
            "pvc/data",
            "--for=jsonpath={.status.phase}=Bound",
            "--timeout=120s"
        ])
        .status
        .success());
        let bound: serde_json::Value =
            serde_json::from_slice(&kubectl_output(&["get", "pvc", "data", "-o", "json"]).stdout)
                .unwrap();
        let volume = bound["spec"]["volumeName"].as_str().unwrap();
        let pv: serde_json::Value =
            serde_json::from_slice(&kubectl_output(&["get", "pv", volume, "-o", "json"]).stdout)
                .unwrap();
        assert_eq!(pv["spec"]["persistentVolumeReclaimPolicy"], "Retain");
        eprintln!("claim bound after preparation: volume={volume}, reclaimPolicy=Retain");
        assert!(
            kubectl_output(&["delete", "pod", "consumer", "--wait=true"])
                .status
                .success()
        );
        let result = erase_inventory(&settings, &inventory).await;
        eprintln!("first erasure result: {result:?}");
        let remaining = kubectl_output(&["get", "pvc", "data", "-o", "json"]);
        // Clean even on a regression: a failed assertion must not leave a Retain volume.
        if result.is_ok() {
            assert!(kubectl_output(&[
                "patch",
                "pv",
                volume,
                "--type=merge",
                "-p",
                r#"{"spec":{"persistentVolumeReclaimPolicy":"Delete"}}"#
            ])
            .status
            .success());
        } else {
            let retained: serde_json::Value = serde_json::from_slice(
                &kubectl_output(&["get", "pv", volume, "-o", "json"]).stdout,
            )
            .unwrap();
            assert_eq!(retained["metadata"]["uid"], pv["metadata"]["uid"]);
            assert_eq!(retained["spec"]["persistentVolumeReclaimPolicy"], "Retain");
            prepare_volume_erasure(&settings, &mut inventory)
                .await
                .unwrap();
            assert_eq!(inventory.objects[0].volume.as_ref().unwrap().name, volume);
            let prepared: serde_json::Value = serde_json::from_slice(
                &kubectl_output(&["get", "pv", volume, "-o", "json"]).stdout,
            )
            .unwrap();
            assert_eq!(prepared["spec"]["persistentVolumeReclaimPolicy"], "Delete");
            erase_inventory(&settings, &inventory).await.unwrap();
            wait_for_erased_volumes(&settings, &inventory)
                .await
                .unwrap();
            eprintln!("retry recorded the bound volume, changed policy to Delete, and observed volume removal");
        }
        assert!(
            kubectl_output(&["delete", "namespace", &namespace, "--wait=false"])
                .status
                .success()
        );
        assert!(kubectl_output(&["delete", "storageclass", &storage_class])
            .status
            .success());
        let error = result
            .expect_err("erase must retain a claim whose newly bound volume was not recorded");
        assert!(error.to_string().contains("retry deletion"));
        assert!(
            remaining.status.success(),
            "claim must remain available for volume preparation"
        );
        let remaining: serde_json::Value = serde_json::from_slice(&remaining.stdout).unwrap();
        assert_eq!(remaining["metadata"]["uid"], claim["metadata"]["uid"]);
    }

    #[cfg(unix)]
    async fn signed_cleanup_preserves_unowned_claims(policy: &str, late_claim: bool) {
        let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
        let namespace = format!("sync-{policy}-{}", std::process::id());
        let (folder, mut site) = folder_with_report();
        site.deployment_id = format!("dep_{policy}_{}", std::process::id());
        let snapshot = folder.path().join("deleting-report.json");
        std::fs::rename(report_path(folder.path()), &snapshot).unwrap();
        let mut original: SiteReport =
            serde_json::from_slice(&std::fs::read(&snapshot).unwrap()).unwrap();
        original.deployment_id = site.deployment_id.clone();
        std::fs::write(&snapshot, serde_json::to_vec(&original).unwrap()).unwrap();
        let settings = SiteSide {
            registry: "registry.example.com/site".into(),
            registry_credentials: None,
            insecure_registry: false,
            namespace: Some(namespace.clone()),
            values: Vec::new(),
            kube_context: Some(context.clone()),
        };
        let chart = tempfile::tempdir().unwrap();
        std::fs::create_dir(chart.path().join("templates")).unwrap();
        std::fs::write(
            chart.path().join("Chart.yaml"),
            "apiVersion: v2\nname: app\nversion: 0.1.0\n",
        )
        .unwrap();
        std::fs::write(
            chart.path().join("templates/runtime.yaml"),
            r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
spec:
  replicas: 0
  selector:
    matchLabels: {app: app}
  template:
    metadata:
      labels: {app: app}
    spec:
      containers:
        - name: operator
          image: registry.k8s.io/pause:3.10
          env:
            - {name: ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY, value: alien.dev/deployment}
            - {name: OPERATOR_RESOURCE_PREFIX, value: app}
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: runtime
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: database
  annotations:
    helm.sh/resource-policy: keep
spec:
  storageClassName: ""
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 1Mi
---
apiVersion: v1
kind: Secret
metadata:
  name: database-password
  annotations:
    helm.sh/resource-policy: keep
stringData:
  value: fixture
"#,
        )
        .unwrap();
        let mut install = Command::new("helm");
        install.args(["install", "app"]).arg(chart.path()).args([
            "--namespace",
            &namespace,
            "--create-namespace",
            "--kube-context",
            &context,
            "--set",
            &format!("management.deploymentId={}", site.deployment_id),
            "--set",
            "runtime.cleanup.onUninstall.enabled=true",
        ]);
        run(install, "installing test release").await.unwrap();
        let mut delete = kubectl(Some(&context));
        delete.args(["delete", "deployment", "app", "--namespace", &namespace]);
        run(delete, "removing the Operator before retry")
            .await
            .unwrap();
        assert!(release_values(&settings, "app", &namespace)
            .await
            .unwrap()
            .is_some());

        // Container claim templates have names but no ownership labels.
        let workloads = tempfile::NamedTempFile::new().unwrap();
        let mut objects = Vec::new();
        for (name, labels) in [
            (
                "app-data",
                serde_json::json!({"managed-by":"runtime", "alien.dev/deployment":"app"}),
            ),
            (
                "other-data",
                serde_json::json!({"managed-by":"runtime", "alien.dev/deployment":"other"}),
            ),
            ("customer-data", serde_json::json!({})),
        ] {
            objects.push(serde_json::json!({
                "apiVersion":"apps/v1", "kind":"StatefulSet",
                "metadata":{"name":name, "labels":labels},
                "spec":{
                    "serviceName":name, "replicas":1,
                    "selector":{"matchLabels":{"app":name}},
                    "template":{"metadata":{"labels":{"app":name}}, "spec":{
                        "containers":[{"name":"app", "image":"registry.k8s.io/pause:3.10",
                            "volumeMounts":[{"name":"data", "mountPath":"/data"}]}]
                    }},
                    "volumeClaimTemplates":[{"metadata":{"name":"data"}, "spec":{
                        "accessModes":["ReadWriteOnce"], "resources":{"requests":{"storage":"1Mi"}}
                    }}]
                }
            }));
        }
        for name in ["data-app-data-backup", "data-app-data-0-extra", "unrelated"] {
            objects.push(serde_json::json!({
                "apiVersion":"v1", "kind":"PersistentVolumeClaim", "metadata":{"name":name},
                "spec":{"storageClassName":"", "accessModes":["ReadWriteOnce"],
                    "resources":{"requests":{"storage":"1Mi"}}}
            }));
        }
        std::fs::write(
            workloads.path(),
            serde_json::to_vec(&serde_json::json!({
                "apiVersion":"v1", "kind":"List", "items":objects
            }))
            .unwrap(),
        )
        .unwrap();
        let mut apply = kubectl(Some(&context));
        apply
            .args(["apply", "--namespace", &namespace, "-f"])
            .arg(workloads.path());
        run(apply, "creating persistent workloads").await.unwrap();
        for name in ["app-data", "other-data", "customer-data"] {
            let mut wait = kubectl(Some(&context));
            wait.args([
                "rollout",
                "status",
                &format!("statefulset/{name}"),
                "--namespace",
                &namespace,
                "--timeout=120s",
            ]);
            run(wait, "waiting for persistent workload").await.unwrap();
        }
        let mut get = kubectl(Some(&context));
        get.args(["get", "pvc", "--namespace", &namespace, "-o", "json"]);
        let claims_before: serde_json::Value =
            serde_json::from_slice(&run(get, "reading original claims").await.unwrap()).unwrap();
        let claim = claims_before["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["metadata"]["name"] == "data-app-data-0")
            .unwrap();
        assert!(claim["metadata"]["labels"]["managed-by"].is_null());
        assert_eq!(claim["status"]["phase"], "Bound");
        // The signed deletion follows workload teardown; retain the StatefulSet metadata.
        let mut scale = kubectl(Some(&context));
        scale.args([
            "scale",
            "statefulset",
            "--all",
            "--replicas=0",
            "--namespace",
            &namespace,
        ]);
        run(scale, "stopping test workloads").await.unwrap();
        let mut wait = kubectl(Some(&context));
        wait.args([
            "wait",
            "pod",
            "--all",
            "--for=delete",
            "--namespace",
            &namespace,
            "--timeout=120s",
        ]);
        run(wait, "waiting for test workloads to stop")
            .await
            .unwrap();

        let recorded_data = deletion_inventory(&settings, "app", &namespace)
            .await
            .unwrap();
        let claims_before = if late_claim {
            let mut machine = load_machine(&site.deployment_id).unwrap();
            machine.deletion_inventory = Some(recorded_data.clone());
            save_machine(&site.deployment_id, &machine).unwrap();
            let mut scale = kubectl(Some(&context));
            scale.args([
                "scale",
                "statefulset/app-data",
                "--replicas=2",
                "-n",
                &namespace,
            ]);
            run(scale, "resuming interrupted scale-up").await.unwrap();
            let mut wait = kubectl(Some(&context));
            wait.args([
                "rollout",
                "status",
                "statefulset/app-data",
                "-n",
                &namespace,
                "--timeout=120s",
            ]);
            run(wait, "creating the late claim").await.unwrap();
            let mut get = kubectl(Some(&context));
            get.args(["get", "pvc", "-n", &namespace, "-o", "json"]);
            let claims =
                serde_json::from_slice(&run(get, "recording late claim").await.unwrap()).unwrap();
            // Keep the controller metadata; the production deletion must stop it.
            let mut scale = kubectl(Some(&context));
            scale.args([
                "scale",
                "statefulset/app-data",
                "--replicas=0",
                "-n",
                &namespace,
            ]);
            run(scale, "releasing fixture volume mounts").await.unwrap();
            let mut wait = kubectl(Some(&context));
            wait.args([
                "wait",
                "pods",
                "--all",
                "--for=delete",
                "-n",
                &namespace,
                "--timeout=120s",
            ]);
            run(wait, "waiting for fixture mounts").await.unwrap();
            claims
        } else {
            claims_before.clone()
        };

        let bundle = tempfile::tempdir().unwrap();
        std::fs::write(
            bundle.path().join(airgap::TARGET_FILE),
            br#"{"action":"delete"}"#,
        )
        .unwrap();
        let manifest = BundleManifest {
            format_version: airgap::FORMAT_VERSION,
            deployment_id: site.deployment_id.clone(),
            deployment_name: site.name.clone(),
            release_id: "delete".into(),
            sequence: 2,
            created_at: chrono::Utc::now().to_rfc3339(),
            stack_id: "app".into(),
            images: Vec::new(),
            chart_images: BTreeMap::new(),
            chart: String::new(),
            files: airgap::checksums(bundle.path()).await.unwrap(),
            omitted_blobs: BTreeSet::new(),
            telemetry_ack: None,
            deletion_policy: None,
        };
        let mut signed_manifest = serde_json::to_value(&manifest).unwrap();
        signed_manifest["deletionPolicy"] = policy.into();
        let bytes = serde_json::to_vec(&signed_manifest).unwrap();
        let key = BundleSigningKey::from_seed([6; 32]);
        std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &bytes).unwrap();
        std::fs::write(bundle.path().join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
        std::fs::create_dir_all(folder.path().join(TO_SITE)).unwrap();
        let pending = folder.path().join(TO_SITE).join("2-delete.bundle");
        airgap::pack(bundle.path(), &pending).unwrap();
        let args = SyncArgs::try_parse_from(["sync", "--trusted-key", &key.public_key()]).unwrap();
        let mut mismatched = signed_manifest.clone();
        mismatched["releaseId"] = "rel_other".into();
        let mismatched = serde_json::to_vec(&mismatched).unwrap();
        std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &mismatched).unwrap();
        std::fs::write(
            bundle.path().join(airgap::SIGNATURE_FILE),
            key.sign(&mismatched),
        )
        .unwrap();
        airgap::pack(bundle.path(), &pending).unwrap();
        let error = install_pending(folder.path(), &mut site, &settings, &args)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("signed deletion action and manifest"));
        assert!(release_values(&settings, "app", &namespace)
            .await
            .unwrap()
            .is_some());
        std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &bytes).unwrap();
        std::fs::write(bundle.path().join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
        airgap::pack(bundle.path(), &pending).unwrap();
        let expected_current_data = deletion_inventory(&settings, "app", &namespace)
            .await
            .unwrap();
        let target = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(target.path(), br#"{"releaseInfo":{"releaseId":"rel_new"}}"#).unwrap();
        write_target(&settings, "app", &namespace, target.path(), 3, 0)
            .await
            .unwrap();
        let error = install_pending(folder.path(), &mut site, &settings, &args)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("older than the installed target"));
        assert!(pending.exists());
        assert!(snapshot.exists());
        assert!(release_values(&settings, "app", &namespace)
            .await
            .unwrap()
            .is_some());
        let unchanged = deletion_inventory(&settings, "app", &namespace)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&unchanged).unwrap(),
            serde_json::to_value(&expected_current_data).unwrap()
        );
        // A current signed request can still complete the same cleanup.
        signed_manifest["sequence"] = 4.into();
        let bytes = serde_json::to_vec(&signed_manifest).unwrap();
        std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &bytes).unwrap();
        std::fs::write(bundle.path().join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
        airgap::pack(bundle.path(), &pending).unwrap();
        install_pending(folder.path(), &mut site, &settings, &args)
            .await
            .unwrap();

        assert!(release_values(&settings, "app", &namespace)
            .await
            .unwrap()
            .is_none());
        let mut get = kubectl(Some(&context));
        get.args([
            "get",
            "configmap",
            "runtime",
            "--namespace",
            &namespace,
            "--ignore-not-found",
            "-o",
            "name",
        ]);
        assert!(run(get, "checking Helm cleanup").await.unwrap().is_empty());
        let reports = by_sequence(&folder.path().join(FROM_SITE), Some(REPORT_EXTENSION)).unwrap();
        assert_eq!(reports.len(), 1);
        let report: SiteReport =
            serde_json::from_slice(&std::fs::read(&reports[0].1).unwrap()).unwrap();
        assert_eq!(
            report.state["status"],
            if policy == "erase" {
                "deleted"
            } else {
                "retained"
            }
        );
        assert_eq!(report.state["runtimeMetadata"]["deletionPolicy"], policy);
        let mut get = kubectl(Some(&context));
        get.args(["get", "pvc,secret", "--namespace", &namespace, "-o", "json"]);
        let remaining: serde_json::Value =
            serde_json::from_slice(&run(get, "checking data erasure").await.unwrap()).unwrap();
        let remaining = remaining["items"].as_array().unwrap();
        for original_claim in claims_before["items"].as_array().unwrap() {
            let name = original_claim["metadata"]["name"].as_str().unwrap();
            let found = remaining.iter().find(|v| v["metadata"]["name"] == name);
            if policy == "erase"
                && matches!(name, "database" | "data-app-data-0" | "data-app-data-1")
            {
                assert!(found.is_none(), "erase left claim {name}");
            } else {
                let found = found.unwrap_or_else(|| panic!("lost claim {name}"));
                assert_eq!(found["metadata"]["uid"], original_claim["metadata"]["uid"]);
                assert_eq!(
                    found["spec"]["volumeName"],
                    original_claim["spec"]["volumeName"]
                );
            }
        }
        let saved = load_machine(&site.deployment_id)
            .unwrap()
            .deletion_inventory
            .unwrap();
        assert!(saved
            .objects
            .iter()
            .any(|v| v.name == "data-app-data-0"
                && v.uid == claim["metadata"]["uid"].as_str().unwrap()));
        assert_eq!(
            remaining
                .iter()
                .any(|v| v["metadata"]["name"] == "database-password"),
            policy == "retain"
        );
        assert_eq!(
            serde_json::to_value(&report.telemetry).unwrap(),
            serde_json::to_value(&original.telemetry).unwrap()
        );
        assert!(!snapshot.exists());
        assert!(!pending.exists());
        if policy == "erase" {
            // A completed erase receipt must not delete data recreated at the same name.
            let mut create = kubectl(Some(&context));
            create.args([
                "create",
                "secret",
                "generic",
                "database-password",
                "--namespace",
                &namespace,
                "--from-literal=value=replacement",
            ]);
            run(create, "creating replacement data").await.unwrap();
            let error = erase_inventory(&settings, &recorded_data)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("was replaced"));
            let mut get = kubectl(Some(&context));
            get.args([
                "get",
                "secret",
                "database-password",
                "--namespace",
                &namespace,
                "-o",
                "json",
            ]);
            let replacement: serde_json::Value =
                serde_json::from_slice(&run(get, "checking replacement data").await.unwrap())
                    .unwrap();
            assert_eq!(replacement["data"]["value"], "cmVwbGFjZW1lbnQ=");
        }
        let mut delete = kubectl(Some(&context));
        delete.args(["delete", "namespace", &namespace]);
        run(delete, "removing test namespace").await.unwrap();
    }

    /// Run with ALIEN_SYNC_TEST_CONTEXT pointing at a disposable Kind cluster.
    #[tokio::test]
    #[ignore = "requires kubectl and a disposable Kind cluster"]
    async fn uninstall_transport_secrets_preserves_customer_credentials() {
        let context = std::env::var("ALIEN_SYNC_TEST_CONTEXT").expect("explicit test context");
        let namespace = format!("sync-cleanup-{}", std::process::id());
        let settings = SiteSide {
            registry: "registry.example.com/site".into(),
            registry_credentials: None,
            insecure_registry: false,
            namespace: Some(namespace.clone()),
            values: Vec::new(),
            kube_context: Some(context),
        };
        let mut create = kubectl(settings.kube_context.as_deref());
        create.args(["create", "namespace", &namespace]);
        run(create, "creating test namespace").await.unwrap();
        for name in [
            "customer-airgap-target",
            "customer-airgap-status",
            "deployer-password",
            "site-tls",
        ] {
            let mut create = kubectl(settings.kube_context.as_deref());
            create.args([
                "create",
                "secret",
                "generic",
                name,
                "-n",
                &namespace,
                "--from-literal=value=test",
            ]);
            run(create, "creating customer Secret").await.unwrap();
        }
        let mut create = kubectl(settings.kube_context.as_deref());
        create.args([
            "create",
            "secret",
            "docker-registry",
            "customer-registry-credentials",
            "-n",
            &namespace,
            "--docker-server=registry.example.com",
            "--docker-username=user",
            "--docker-password=password",
        ]);
        run(create, "creating customer pull Secret").await.unwrap();
        write_pull_secret(&settings, "customer", &namespace, "user", "password")
            .await
            .unwrap();
        write_pull_secret(&settings, "owned", &namespace, "user", "password")
            .await
            .unwrap();
        write_pull_secret(&settings, "owned", &namespace, "user", "updated-password")
            .await
            .unwrap();
        let target = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(target.path(), b"{}").unwrap();
        write_target(&settings, "customer", &namespace, target.path(), 1, 0)
            .await
            .unwrap();
        write_target(&settings, "owned", &namespace, target.path(), 1, 0)
            .await
            .unwrap();
        let mut create = kubectl(settings.kube_context.as_deref());
        create.args([
            "create",
            "secret",
            "generic",
            "owned-airgap-status",
            "-n",
            &namespace,
        ]);
        run(create, "creating status Secret").await.unwrap();
        let mut label = kubectl(settings.kube_context.as_deref());
        label.args([
            "label",
            "secret",
            "owned-airgap-status",
            "-n",
            &namespace,
            "app.kubernetes.io/managed-by=alien-deploy-sync",
        ]);
        run(label, "marking status Secret").await.unwrap();
        remove_transport_secrets(&settings, "owned", &namespace)
            .await
            .unwrap();
        remove_transport_secrets(&settings, "customer", &namespace)
            .await
            .unwrap();
        remove_transport_secrets(&settings, "owned", &namespace)
            .await
            .unwrap();
        let mut list = kubectl(settings.kube_context.as_deref());
        list.args(["get", "secrets", "-n", &namespace, "-o", "json"]);
        let remaining: serde_json::Value =
            serde_json::from_slice(&run(list, "reading retained Secrets").await.unwrap()).unwrap();
        let mut names: Vec<_> = remaining["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["metadata"]["name"].as_str().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "customer-airgap-status",
                "customer-airgap-target",
                "customer-registry-credentials",
                "deployer-password",
                "site-tls"
            ]
        );
        let mut delete = kubectl(settings.kube_context.as_deref());
        delete.args(["delete", "namespace", &namespace]);
        run(delete, "removing test namespace").await.unwrap();
    }

    #[test]
    fn airgap_retry_waits_for_its_attempt_and_retry_transition() {
        let mut status = serde_json::json!({"sequence": 7, "attempt": "previous", "state": {
            "status": "update-failed", "targetRelease": {"releaseId": "release"},
            "error": {"message": "previous attempt failed"}
        }});
        assert!(!rollout_finished(&status, "release", 7, None, Some("retry")).unwrap());
        status["attempt"] = "retry".into();
        status["state"]["retryRequested"] = true.into();
        assert!(!rollout_finished(&status, "release", 7, None, Some("retry")).unwrap());
        status["state"]["retryRequested"] = false.into();
        assert!(rollout_finished(&status, "release", 7, None, Some("retry")).is_err());
        status["state"]["status"] = "running".into();
        status["state"]["currentRelease"] = serde_json::json!({"releaseId": "release"});
        assert!(rollout_finished(&status, "release", 7, None, Some("retry")).unwrap());
        assert!(!rollout_finished(&status, "release", 8, None, Some("retry")).unwrap());
        status.as_object_mut().unwrap().remove("attempt");
        assert!(rollout_finished(&status, "release", 7, None, Some("retry")).unwrap());
        status["state"]["status"] = "update-failed".into();
        assert!(!rollout_finished(
            &status,
            "release",
            7,
            Some(chrono::Utc::now()),
            Some("retry")
        )
        .unwrap());
        assert!(rollout_finished(&status, "release", 7, None, Some("retry")).is_err());
    }

    #[test]
    fn rollout_waits_for_requested_sequence_before_judging_status() {
        let mut status = serde_json::json!({
            "sequence": 10,
            "state": {
                "status": "update-failed",
                "currentRelease": {"releaseId": "rel_old"},
                "targetRelease": {"releaseId": "rel_new"},
                "error": {"message": "previous attempt failed"}
            }
        });
        assert!(!rollout_finished(&status, "rel_new", 11, None, None).unwrap());
        status["sequence"] = 11.into();
        assert!(rollout_finished(&status, "rel_new", 11, None, None).is_err());
        status["state"]["status"] = "updating".into();
        assert!(!rollout_finished(&status, "rel_new", 11, None, None).unwrap());
        status["state"]["status"] = "running".into();
        assert!(!rollout_finished(&status, "rel_new", 11, None, None).unwrap());
        status["state"]["currentRelease"]["releaseId"] = "rel_new".into();
        assert!(rollout_finished(&status, "rel_new", 11, None, None).unwrap());
        assert!(!rollout_finished(&status, "rel_new", 12, None, None).unwrap());
    }

    #[test]
    fn missing_secrets_stop_rollout_with_commands_and_allow_retry() {
        let mut status = serde_json::json!({
            "sequence": 10,
            "state": {
                "status": "waiting-for-secrets",
                "runtimeMetadata": {"deployerSecrets": [
                    {"required": true, "status": "missing", "value": "never-print-this",
                     "location": {"name": "app-password", "cliCommand": "kubectl create secret generic app-password --from-literal=value='<VALUE>'"}},
                    {"required": false, "status": "missing", "location": {"name": "optional-secret"}},
                    {"required": true, "status": "available", "location": {"name": "existing-secret"}}
                ]}
            }
        });
        assert!(!rollout_finished(&status, "rel_new", 11, None, None).unwrap());
        let error = rollout_finished(&status, "rel_new", 10, None, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Secret: app-password"));
        assert!(error.contains("kubectl create secret generic app-password"));
        assert!(error.contains("run sync again"));
        assert!(!error.contains("never-print-this"));
        assert!(!error.contains("optional-secret"));
        assert!(!error.contains("existing-secret"));
        status["state"]["status"] = "provisioning".into();
        assert!(!rollout_finished(&status, "rel_new", 10, None, None).unwrap());
        status["state"]["status"] = "running".into();
        status["state"]["currentRelease"] = serde_json::json!({"releaseId": "rel_new"});
        assert!(rollout_finished(&status, "rel_new", 10, None, None).unwrap());
    }

    #[test]
    fn secret_retry_waits_for_fresh_status_but_still_reports_missing_inputs() {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:01:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut status = serde_json::json!({
            "sequence": 10,
            "writtenAt": "2026-01-01T00:00:59Z",
            "state": {
                "status": "waiting-for-secrets",
                "runtimeMetadata": {"deployerSecrets": [
                    {"required": true, "status": "missing",
                     "location": {"name": "app-password"}}
                ]}
            }
        });
        assert!(!rollout_finished(&status, "rel_new", 10, Some(started_at), None).unwrap());
        // Once the bounded freshness wait expires, an unchanged snapshot still
        // produces the actionable error instead of waiting for the rollout timeout.
        assert!(rollout_finished(&status, "rel_new", 10, None, None).is_err());
        status["writtenAt"] = serde_json::Value::Null;
        assert!(!rollout_finished(&status, "rel_new", 10, Some(started_at), None).unwrap());
        status["writtenAt"] = "2026-01-01T00:01:01Z".into();
        assert!(rollout_finished(&status, "rel_new", 10, Some(started_at), None).is_err());
        status["state"]["status"] = "provisioning".into();
        assert!(!rollout_finished(&status, "rel_new", 10, Some(started_at), None).unwrap());
        status["state"]["status"] = "running".into();
        status["state"]["currentRelease"] = serde_json::json!({"releaseId": "rel_new"});
        assert!(rollout_finished(&status, "rel_new", 10, Some(started_at), None).unwrap());
    }

    #[tokio::test]
    async fn branded_sync_discovers_manager_only_when_registering_online() {
        let api = MockServer::start_async().await;
        let manager = MockServer::start_async().await;
        let discovery = api
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/resolve")
                    .query_param("platform", "kubernetes")
                    .header("authorization", "Bearer ax_site");
                then.status(200).json_body(serde_json::json!({
                    "managerUrl": manager.base_url()
                }));
            })
            .await;
        let whoami = manager
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/whoami")
                    .header("authorization", "Bearer ax_site");
                // Stop before writing machine credentials.
                then.status(403);
            })
            .await;
        let config: DeployCliConfig = serde_json::from_value(serde_json::json!({
            "apiBaseUrl": api.base_url(), "name": "acmectl"
        }))
        .unwrap();
        let args = SyncArgs::try_parse_from(["sync", "--token", "ax_site"]).unwrap();
        assert!(sync_command(args, Some(&config)).await.is_err());
        discovery.assert_hits_async(1).await;
        whoami.assert_hits_async(1).await;

        let args = SyncArgs::try_parse_from([
            "sync",
            "--token",
            "ax_site",
            "--manager",
            &manager.base_url(),
        ])
        .unwrap();
        assert!(sync_command(args, Some(&config)).await.is_err());
        discovery.assert_hits_async(1).await;
        whoami.assert_hits_async(2).await;

        let dir = tempfile::tempdir().unwrap();
        let args = SyncArgs::try_parse_from([
            "sync",
            "--folder",
            dir.path().to_str().unwrap(),
            "--registry",
            "localhost:5001",
        ])
        .unwrap();
        assert!(sync_command(args, Some(&config)).await.is_err());
        discovery.assert_hits_async(1).await;
        whoami.assert_hits_async(2).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn carried_tools_can_refresh_the_running_executable() {
        let bundle = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(bundle.path().join(TOOLS_DIR)).unwrap();
        let source = bundle.path().join("tools/acmectl-linux-x86_64");
        std::fs::copy("/bin/sleep", &source).unwrap();
        copy_tools(bundle.path(), folder.path()).unwrap();
        let executable = folder.path().join("tools/acmectl-linux-x86_64");
        let mut running = Command::new(&executable)
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        assert!(running.try_wait().unwrap().is_none());
        let original_inode = std::fs::metadata(&executable).unwrap().ino();
        copy_tools(bundle.path(), folder.path()).unwrap();
        assert_eq!(
            std::fs::metadata(&executable).unwrap().ino(),
            original_inode
        );
        std::fs::copy("/bin/true", &source).unwrap();
        copy_tools(bundle.path(), folder.path()).unwrap();
        assert_ne!(
            std::fs::metadata(&executable).unwrap().ino(),
            original_inode
        );
        assert_eq!(
            std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(Command::new(&executable).status().await.unwrap().success());
        assert!(running.try_wait().unwrap().is_none());
        running.kill().await.unwrap();
        running.wait().await.unwrap();
    }

    #[tokio::test]
    async fn carried_tools_keep_the_downloaded_brand_and_skip_unchanged_builds() {
        let server = MockServer::start_async().await;
        let binary = server
            .mock_async(|when, then| {
                when.method(GET).path("/linux-x64/acmectl");
                then.status(200).body("branded executable bytes");
            })
            .await;
        let sources = serde_json::json!({ "deployCli": [{
            "platform": "linux-x86_64",
            "url": format!("{}/linux-x64/acmectl", server.base_url()),
        }] });
        let bundle = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            download_tools(bundle.path(), folder.path(), &sources)
                .await
                .unwrap(),
            Tools::Included
        );
        copy_tools(bundle.path(), folder.path()).unwrap();
        let executable = folder.path().join("tools/acmectl-linux-x86_64");
        assert_eq!(
            std::fs::read(&executable).unwrap(),
            b"branded executable bytes"
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            download_tools(bundle.path(), folder.path(), &sources)
                .await
                .unwrap(),
            Tools::AlreadyThere
        );
        binary.assert_hits_async(1).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn first_download_carries_runnable_verified_tools() {
        let server = MockServer::start_async().await;
        let binary = server
            .mock_async(|when, then| {
                when.method(GET).path("/acmectl");
                then.status(200)
                    .body("#!/bin/sh\nprintf 'site CLI ready'\n");
            })
            .await;
        let bundle = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let sources = serde_json::json!({"deployCli": [{
            "platform": "linux-x86_64", "url": format!("{}/acmectl", server.base_url())
        }]});
        assert_eq!(
            download_tools(bundle.path(), folder.path(), &sources)
                .await
                .unwrap(),
            Tools::Included
        );
        let manifest = BundleManifest {
            format_version: airgap::FORMAT_VERSION,
            deployment_id: "dep_1".into(),
            deployment_name: "site-1".into(),
            release_id: "rel_1".into(),
            sequence: 1,
            created_at: "now".into(),
            stack_id: "app".into(),
            images: vec![],
            chart_images: BTreeMap::new(),
            chart: "chart/app.tgz".into(),
            files: airgap::checksums(bundle.path()).await.unwrap(),
            omitted_blobs: BTreeSet::new(),
            telemetry_ack: None,
            deletion_policy: None,
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let key = BundleSigningKey::from_seed([5; 32]);
        std::fs::write(bundle.path().join(airgap::MANIFEST_FILE), &bytes).unwrap();
        std::fs::write(bundle.path().join(airgap::SIGNATURE_FILE), key.sign(&bytes)).unwrap();
        let output = folder.path().join("update.alienbundle");
        write_update_bundle(bundle.path(), &output, folder.path(), &key.public_key())
            .await
            .unwrap();
        assert!(output.is_file());
        let executable = folder.path().join("tools/acmectl-linux-x86_64");
        let result = std::process::Command::new(&executable).output().unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, b"site CLI ready");
        binary.assert_hits_async(1).await;

        // Changed downloads must not replace the carried executable.
        std::fs::write(bundle.path().join("tools/acmectl-linux-x86_64"), b"changed").unwrap();
        assert!(
            write_update_bundle(bundle.path(), &output, folder.path(), &key.public_key())
                .await
                .is_err()
        );
        assert_eq!(
            std::process::Command::new(executable)
                .output()
                .unwrap()
                .stdout,
            b"site CLI ready"
        );
    }

    /// A transfer folder for `deployment` with one report from the site
    /// carrying telemetry batches 1 to 4.
    fn folder_with_report() -> (tempfile::TempDir, SiteState) {
        let dir = tempfile::tempdir().unwrap();
        let site = SiteState {
            deployment_id: "dep_1".to_string(),
            name: "site".to_string(),
            site_release: Some("rel_old".to_string()),
            ..SiteState::default()
        };
        save_site(dir.path(), &site).unwrap();
        std::fs::create_dir_all(dir.path().join(FROM_SITE)).unwrap();
        let report = SiteReport {
            deployment_id: "dep_1".to_string(),
            written_at: "2026-01-01T00:00:00Z".to_string(),
            installed_release: Some("rel_old".to_string()),
            state: serde_json::json!({ "status": "running" }),
            heartbeats: Vec::new(),
            telemetry: (1..=4)
                .map(|id| ReportedBatch {
                    id,
                    signal: "logs".to_string(),
                    data: "b3RscA==".to_string(),
                })
                .collect(),
            dropped: 0,
            inventory: BTreeSet::new(),
        };
        std::fs::write(
            report_path(dir.path()),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        (dir, site)
    }

    fn report_path(folder: &Path) -> PathBuf {
        folder.join(FROM_SITE).join("1-report.report")
    }

    fn access(server: &MockServer) -> ManagerAccess {
        ManagerAccess {
            url: server.base_url(),
            token: "ax_deploy_test".to_string(),
        }
    }

    /// The first telemetry batch id in a status report.
    fn first_batch(request: &HttpMockRequest) -> Option<i64> {
        let body: serde_json::Value = serde_json::from_slice(request.body.as_deref()?).ok()?;
        body["telemetry"][0]["id"].as_i64()
    }

    #[tokio::test]
    async fn a_report_that_fails_to_send_does_not_hold_back_the_update() {
        let server = MockServer::start_async().await;
        let reports = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_1/status-report");
                then.status(504).body("gateway timeout");
            })
            .await;
        let target = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/dep_1/target");
                then.status(200).json_body(serde_json::json!({
                    "releaseInfo": { "releaseId": "rel_new", "stack": { "id": "app" } }
                }));
            })
            .await;
        let (dir, mut site) = folder_with_report();
        // The update is already in the folder, so the download step stops
        // there instead of pulling images.
        std::fs::create_dir_all(dir.path().join(TO_SITE)).unwrap();
        std::fs::write(dir.path().join(TO_SITE).join("2-rel_new.bundle"), b"x").unwrap();

        let unsent = exchange_online(dir.path(), &mut site, &access(&server), false, false, &[])
            .await
            .expect("the update step runs");

        let error = unsent.expect("the report error comes back");
        assert_eq!(error.code, "MANAGER_REQUEST_FAILED");
        assert!(error.message.contains("504"), "{}", error.message);
        reports.assert_hits_async(1).await;
        target.assert_hits_async(1).await;
        assert!(
            report_path(dir.path()).exists(),
            "the report stays for the next sync"
        );
        assert_eq!(load_site(dir.path()).unwrap().uploaded_through, 0);
    }

    #[tokio::test]
    async fn only_a_fully_acknowledged_deleted_report_accepts_removed_credentials() {
        for (status, deleted, acknowledged, accepted) in [
            (401, true, true, true),
            (401, true, false, false),
            (401, false, true, false),
            (401, false, false, false),
            (404, true, true, true),
            (404, true, false, false),
            (404, false, true, false),
            (403, true, true, false),
            (500, true, true, false),
        ] {
            let server = MockServer::start_async().await;
            let request = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v1/deployments/dep_1/status-report");
                    then.status(status);
                })
                .await;
            let (dir, mut site) = folder_with_report();
            if deleted {
                let path = report_path(dir.path());
                let mut report: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                report["state"]["status"] = serde_json::json!("deleted");
                std::fs::write(path, serde_json::to_vec(&report).unwrap()).unwrap();
            }
            if acknowledged {
                site.uploaded_through = 4;
                save_site(dir.path(), &site).unwrap();
            }
            let original_report = std::fs::read(report_path(dir.path())).unwrap();
            let result = send_reports(dir.path(), &mut site, &access(&server), false).await;
            assert_eq!(
                result.is_ok(),
                accepted,
                "status {status}, deleted {deleted}"
            );
            assert_eq!(report_path(dir.path()).exists(), !accepted);
            assert_eq!(site.deleted, accepted);
            assert_eq!(
                load_site(dir.path()).unwrap().uploaded_through,
                if acknowledged { 4 } else { 0 }
            );
            if !accepted {
                assert_eq!(
                    std::fs::read(report_path(dir.path())).unwrap(),
                    original_report
                );
            }
            request.assert_hits_async(1).await;
        }
    }

    #[test]
    fn collected_report_keeps_operator_time_and_observations() {
        let heartbeat = serde_json::json!({
         "deploymentId": "dep_1", "resourceId": "web", "resourceType": "container",
         "controllerPlatform": "kubernetes", "backend": "kubernetes", "observedAt": "2026-01-01T00:00:00Z",
         "data": {"resourceType": "container", "data": {"backend": "kubernetes", "namespace": "site", "name": "web",
         "workloadKind": "deployment", "status": {"health": "healthy", "lifecycle": "running", "stale": false, "partial": false, "collectionIssues": []},
         "replicas": {"desired": 7, "current": 7, "ready": 7, "available": 7, "updated": 7}, "pods": [], "events": []}}, "raw": []
        });
        let record = InstalledBundle {
            rollout_succeeded: false,
            release_id: "rel_1".into(),
            stack_id: "site".into(),
            chart: "chart.tgz".into(),
            chart_images: BTreeMap::new(),
            telemetry_ack: None,
            inventory: BTreeSet::new(),
            registry: None,
        };
        let site = SiteState {
            deployment_id: "dep_1".into(),
            ..Default::default()
        };
        let mut status = serde_json::json!({"writtenAt": "2026-01-01T00:00:00Z", "state": {"status": "running"}, "heartbeats": [heartbeat.clone()]});
        let report = report_from_status(&site, &record, status.clone(), Vec::new(), 0).unwrap();
        assert_eq!(report.written_at, "2026-01-01T00:00:00Z");
        assert_eq!(
            report.heartbeats,
            vec![serde_json::from_value(heartbeat).unwrap()]
        );
        status.as_object_mut().unwrap().remove("heartbeats");
        assert!(
            report_from_status(&site, &record, status.clone(), Vec::new(), 0)
                .unwrap()
                .heartbeats
                .is_empty()
        );
        status["heartbeats"] = serde_json::json!([{"invalid": true}]);
        assert!(report_from_status(&site, &record, status, Vec::new(), 0).is_err());
    }

    #[tokio::test]
    async fn carried_report_upload_preserves_heartbeats() {
        let server = MockServer::start_async().await;
        let heartbeat = serde_json::json!({
         "deploymentId": "dep_1", "resourceId": "web", "resourceType": "container",
         "controllerPlatform": "kubernetes", "backend": "kubernetes", "observedAt": "2026-01-01T00:00:00Z",
         "data": {"resourceType": "container", "data": {"backend": "kubernetes", "namespace": "site", "name": "web",
         "workloadKind": "deployment", "status": {"health": "healthy", "lifecycle": "running", "stale": false, "partial": false, "collectionIssues": []},
         "replicas": {"desired": 7, "current": 7, "ready": 7, "available": 7, "updated": 7}, "pods": [], "events": []}}, "raw": []
        });
        let request = server.mock_async(|when, then| {
            when.method(POST).path("/v1/deployments/dep_1/status-report")
                .json_body_partial(serde_json::json!({"heartbeats": [heartbeat.clone()], "writtenAt": "2026-01-01T00:00:00Z"}).to_string());
            then.status(200).json_body(serde_json::json!({"telemetryThrough": 4}));
        }).await;
        let (dir, mut site) = folder_with_report();
        let mut report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(report_path(dir.path())).unwrap()).unwrap();
        report["heartbeats"] = serde_json::json!([heartbeat]);
        std::fs::write(
            report_path(dir.path()),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        send_reports(dir.path(), &mut site, &access(&server), false)
            .await
            .unwrap();
        request.assert_hits_async(1).await;
        assert!(!report_path(dir.path()).exists());
    }

    #[tokio::test]
    async fn a_report_the_manager_takes_in_parts_is_sent_in_full() {
        let server = MockServer::start_async().await;
        // The manager takes batches 1 and 2 of the first request, then the
        // rest.
        let first = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_1/status-report")
                    .matches(|request| first_batch(request) == Some(1));
                then.status(200).json_body(serde_json::json!({
                    "status": "running", "telemetryAccepted": 2, "telemetryThrough": 2
                }));
            })
            .await;
        let rest = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_1/status-report")
                    .matches(|request| first_batch(request) == Some(3));
                then.status(200).json_body(serde_json::json!({
                    "status": "running", "telemetryAccepted": 2, "telemetryThrough": 4
                }));
            })
            .await;
        let (dir, mut site) = folder_with_report();

        send_reports(dir.path(), &mut site, &access(&server), false)
            .await
            .expect("the report is sent");

        first.assert_hits_async(1).await;
        rest.assert_hits_async(1).await;
        assert_eq!(site.uploaded_through, 4);
        assert_eq!(load_site(dir.path()).unwrap().uploaded_through, 4);
        assert!(
            !report_path(dir.path()).exists(),
            "a sent report is removed"
        );
    }

    #[tokio::test]
    async fn chunked_deletion_waits_for_all_logs_and_retries_a_lost_final_ack() {
        for lose_final_ack in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stopped = stop.clone();
            let server = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                let mut received = Vec::new();
                let mut requests = Vec::new();
                let mut deleted = false;
                while !stopped.load(std::sync::atomic::Ordering::SeqCst)
                    && std::time::Instant::now() < deadline
                {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => panic!("accept: {error}"),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = std::io::BufReader::new(&mut stream);
                    let mut length = None;
                    loop {
                        let mut line = String::new();
                        std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = Some(value.trim().parse::<usize>().unwrap());
                        }
                    }
                    let mut bytes = vec![0; length.unwrap()];
                    std::io::Read::read_exact(&mut reader, &mut bytes).unwrap();
                    assert!(bytes.len() <= 1024 * 1024);
                    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let batches = body["telemetry"].as_array().unwrap();
                    let status = if deleted {
                        404
                    } else {
                        for batch in batches {
                            assert_eq!(batch["data"], "x".repeat(300_000));
                            received.push(batch["id"].as_i64().unwrap());
                        }
                        if body["state"]["status"] == "deleted" {
                            deleted = true;
                            if lose_final_ack {
                                500
                            } else {
                                200
                            }
                        } else {
                            200
                        }
                    };
                    requests.push((
                        body["state"]["status"].as_str().unwrap().to_string(),
                        batches.len(),
                    ));
                    let response =
                        serde_json::json!({"telemetryThrough": received.last()}).to_string();
                    std::io::Write::write_all(&mut stream, format!(
                        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len(),
                    ).as_bytes()).unwrap();
                }
                (received, requests, deleted)
            });
            let (dir, mut site) = folder_with_report();
            let mut report: SiteReport =
                serde_json::from_slice(&std::fs::read(report_path(dir.path())).unwrap()).unwrap();
            report.state["status"] = "deleted".into();
            report.state["metadata"] = "y".repeat(220_000).into();
            report.telemetry = (1..=8)
                .map(|id| ReportedBatch {
                    id,
                    signal: "logs".into(),
                    data: "x".repeat(300_000),
                })
                .collect();
            std::fs::write(
                report_path(dir.path()),
                serde_json::to_vec(&report).unwrap(),
            )
            .unwrap();
            let access = ManagerAccess {
                url: format!("http://{address}"),
                token: "ax_deploy_test".into(),
            };
            let first = send_reports(dir.path(), &mut site, &access, false).await;
            let retained_after_first = report_path(dir.path()).exists();
            let first_mark = load_site(dir.path()).unwrap().uploaded_through;
            let result = if lose_final_ack {
                site = load_site(dir.path()).unwrap();
                send_reports(dir.path(), &mut site, &access, false).await
            } else {
                Ok(())
            };
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            let (received, requests, deleted) = server.join().unwrap();
            assert_eq!(
                received,
                (1..=8).collect::<Vec<_>>(),
                "all logs precede metadata deletion"
            );
            assert_eq!(first_mark, 8);
            assert_eq!(first.is_err(), lose_final_ack);
            assert_eq!(retained_after_first, lose_final_ack);
            result.unwrap();
            assert!(deleted);
            assert_eq!(site.uploaded_through, 8);
            assert!(site.deleted);
            assert!(!report_path(dir.path()).exists());
            assert_eq!(requests[..4], vec![("deleting".to_string(), 2); 4]);
            assert!(requests[4..]
                .iter()
                .all(|request| *request == ("deleted".to_string(), 0)));
            assert_eq!(requests.len(), if lose_final_ack { 6 } else { 5 });
        }
    }

    #[tokio::test]
    async fn large_reports_keep_every_batch_within_bounded_requests() {
        let server = MockServer::start_async().await;
        let rejected = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_1/status-report")
                    .matches(|request| {
                        request
                            .body
                            .as_ref()
                            .is_some_and(|body| body.len() > 1024 * 1024)
                    });
                then.status(413);
            })
            .await;
        let mut requests = Vec::new();
        for first in [1, 3, 5, 7] {
            requests.push(
                server
                    .mock_async(|when, then| {
                        when.method(POST)
                            .path("/v1/deployments/dep_1/status-report")
                            .header("authorization", "Bearer ax_deploy_test")
                            .body_contains(format!("\"id\":{first}"))
                            .body_contains(format!("\"id\":{}", first + 1))
                            .matches(|request| {
                                let Some(bytes) = request.body.as_deref() else {
                                    return false;
                                };
                                if bytes.len() > 1024 * 1024 {
                                    return false;
                                }
                                let body: serde_json::Value =
                                    serde_json::from_slice(bytes).unwrap();
                                body["state"]["metadata"] == "y".repeat(220_000)
                                    && body["writtenAt"] == "2026-01-01T00:00:00Z"
                                    && body["telemetry"].as_array().is_some_and(|batches| {
                                        batches.len() == 2
                                            && batches.iter().all(|batch| {
                                                batch["data"] == "x".repeat(300_000)
                                                    && batch["signal"] == "logs"
                                            })
                                    })
                            });
                        then.status(200)
                            .json_body(serde_json::json!({"telemetryThrough": first + 1}));
                    })
                    .await,
            );
        }
        let (dir, mut site) = folder_with_report();
        let mut report: SiteReport =
            serde_json::from_slice(&std::fs::read(report_path(dir.path())).unwrap()).unwrap();
        report.state["metadata"] = "y".repeat(220_000).into();
        report.telemetry = (1..=8)
            .map(|id| ReportedBatch {
                id,
                signal: "logs".into(),
                data: "x".repeat(300_000),
            })
            .collect();
        std::fs::write(
            report_path(dir.path()),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        send_reports(dir.path(), &mut site, &access(&server), false)
            .await
            .unwrap();
        rejected.assert_hits_async(0).await;
        for request in requests {
            request.assert_hits_async(1).await;
        }
        assert_eq!(site.uploaded_through, 8);
        assert_eq!(load_site(dir.path()).unwrap().uploaded_through, 8);
        assert!(!report_path(dir.path()).exists());
    }

    #[tokio::test]
    async fn a_report_the_manager_makes_no_progress_on_is_kept() {
        let server = MockServer::start_async().await;
        let reports = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployments/dep_1/status-report");
                then.status(200).json_body(serde_json::json!({
                    "status": "running", "telemetryAccepted": 0
                }));
            })
            .await;
        let (dir, mut site) = folder_with_report();

        let error = send_reports(dir.path(), &mut site, &access(&server), false)
            .await
            .expect_err("telemetry the manager doesn't confirm isn't treated as sent");

        assert_eq!(error.code, "MANAGER_REQUEST_FAILED");
        reports.assert_hits_async(1).await;
        assert_eq!(site.uploaded_through, 0);
        assert!(report_path(dir.path()).exists());
    }

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
