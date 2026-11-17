//! `alien approve` and `alien devices`: the devices and CI keys trusted to
//! sign a project's releases.
//!
//! Every change is the next version of the project's signer list. An admin's
//! device signs it after the CLI verifies the current list from the
//! fingerprint in `.alien/signing.json` and shows the change as a diff
//! against it; the control plane only stores and delivers it.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::signer_list::KeyBackend;
use alien_signing::{
    display_fingerprint, Keep, KeptKind, Kind, PublicKey, RecoveryCode, Role, SecretKey,
    SignedStatement, Signer, SignerChain, SignerKind, SignerListBody,
};
use clap::{Args, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json};
use crate::signing::api::{self, ListPublished, PendingChange, SigningApi};
use crate::signing::release::{self, added_signers, Enabled, RECOVERY_SIGNER};
use crate::signing::{
    kind_name, next_list, pairing_code, same_code, sign_envelope, signer_with_key, verified_chain,
    RepoSigning, SigningHome, StatementSigner, REPO_SIGNING_FILE,
};

/// GitHub environment the CI key is stored in.
const GITHUB_ENVIRONMENT: &str = "release";
/// Name of the CI key secret in that environment.
const GITHUB_SECRET: &str = "ALIEN_SIGNING_KEY";
/// Branch the `release` environment allows.
const RELEASE_BRANCH: &str = "main";
/// Tags the `release` environment allows.
const RELEASE_TAGS: &str = "v*";
/// The public repository Alien CLI builds are published and attested from.
const CLI_REPOSITORY: &str = "alienplatform/alien";
/// The release workflow the dashboard's Connect GitHub opens, which installs
/// the pinned CLI.
const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
/// The workflow variable holding the CLI pin, one line of JSON.
const CLI_PIN_VARIABLE: &str = "ALIEN_CLI_PIN";
/// The CLI release archives the release workflow installs, by
/// `$RUNNER_OS-$RUNNER_ARCH`, named `alien-v<version>-<suffix>`.
const CLI_ARCHIVES: &[(&str, &str)] = &[
    ("Linux-X64", "x86_64-unknown-linux-musl.tar.gz"),
    ("Linux-ARM64", "aarch64-unknown-linux-musl.tar.gz"),
    ("macOS-ARM64", "aarch64-apple-darwin.tar.gz"),
    ("Windows-X64", "x86_64-pc-windows-msvc.zip"),
];

#[derive(Args, Debug, Clone)]
#[command(about = "Trust a teammate's device, or sign a change prepared in the dashboard")]
pub struct ApproveArgs {
    /// The waiting change to approve. Without it, each waiting change in turn.
    pub id: Option<String>,
}

#[derive(Args, Debug, Clone)]
#[command(about = "The devices and CI keys trusted to sign releases")]
pub struct DevicesArgs {
    #[command(subcommand)]
    pub command: Option<DevicesCommand>,

    /// Print JSON (for `list`).
    #[arg(long, global = true)]
    pub json: bool,
}

impl DevicesArgs {
    /// Whether errors should be printed as JSON.
    pub fn wants_json_output(&self) -> bool {
        self.json
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum DevicesCommand {
    /// Turn signing on for the project: this device becomes its first trusted device
    Init {
        /// The project, when the directory isn't linked to one.
        #[arg(long)]
        project: Option<String>,
    },
    /// Show the trusted devices, the recovery key and the fingerprint
    List,
    /// Stop trusting a device, such as a lost laptop
    Remove {
        /// The device's name, as `alien devices list` shows it.
        name: String,
        /// The device was leaked or stolen: keep none of what it signed, so protected
        /// deployments refuse every release and config it signed.
        #[arg(long, conflicts_with = "keep_running")]
        revoke: bool,
        /// The device was only lost or retired: keep what is current on a channel or running
        /// in a protected deployment, and print each one.
        #[arg(long)]
        keep_running: bool,
    },
    /// Trust GitHub Actions in a repository to sign releases
    AddCi {
        /// The repository, as OWNER/REPO.
        repository: String,
    },
    /// Trust a new key for any CI system or remote box to sign releases, and print it once
    AddKey {
        /// The key's name on the list, such as `ci` or `build-box`.
        name: String,
        /// Write the key to this file (mode 0600) instead of printing it.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Replace a CI key, for example after it leaked
    Rotate {
        /// The key's name, as `alien devices list` shows it.
        name: String,
        /// The key leaked: keep none of what it signed, so protected deployments refuse
        /// every release and config it signed. Use this after a leak.
        #[arg(long, conflicts_with = "keep_running")]
        revoke: bool,
        /// The key was not leaked: keep what is current on a channel or running in a
        /// protected deployment, and print each one.
        #[arg(long)]
        keep_running: bool,
        /// For a key made by `add-key`: write the new key to this file (mode 0600) instead of printing it.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Restore signing with the recovery code when every admin device is lost
    Recover,
}

/// An admin's session: the verified list and the key that signs the next one.
struct Admin {
    api: SigningApi,
    chain: SignerChain,
    signer: StatementSigner,
}

/// The repository's pin, which every command here starts from.
fn repo(dir: &Path) -> Result<RepoSigning> {
    RepoSigning::read(dir)?.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "This repo has no {REPO_SIGNING_FILE}. Run `alien devices init` to turn signing on; if a teammate did, pull it."
            ),
        })
    })
}

fn current_dir() -> Result<PathBuf> {
    crate::get_current_dir()
}

/// Open this device as an admin of the project's verified list.
async fn admin(ctx: &ExecutionMode) -> Result<Admin> {
    let repo = repo(&current_dir()?)?;
    let api = SigningApi::from_context(ctx).await?;
    let chain = verified_chain(&api, &repo).await?;
    let (_, key) = SigningHome::current()?.device()?.ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: "This device has no signing key. Ask an admin, or use `alien devices recover` if every admin device is lost.".to_string(),
        })
    })?;
    let list = chain.current();
    let signer = signer_with_key(list, &key.public_key)
        .filter(|signer| signer.roles.contains(&Role::Admin))
        .ok_or_else(|| {
            AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "This device isn't an admin of {}; only admins change the trusted devices",
                    chain.project()
                ),
            })
        })?;
    let signer = StatementSigner::Device {
        id: signer.id.clone(),
        key,
    };
    Ok(Admin { api, chain, signer })
}

impl Admin {
    /// Sign `body` as the next list version and store it. The signed list is
    /// checked to extend the verified chain before it is sent.
    fn sign_next(&self, body: SignerListBody, prompt: &str) -> Result<SignedStatement> {
        let envelope = next_list(&self.chain, body);
        if let Some(gesture) = self.signer.gesture() {
            print!("  {gesture} to {prompt} ▸ ");
            std::io::stdout().flush().into_alien_error().context(
                ErrorData::CliInteractionFailed {
                    message: "Failed to show the prompt".to_string(),
                },
            )?;
        }
        let statement = sign_envelope(&envelope, &self.signer)?;
        if self.signer.gesture().is_some() {
            println!("✓");
        }
        let mut chain = self.chain.clone();
        chain
            .extend(std::slice::from_ref(&statement))
            .context(ErrorData::SigningFailed {
                message: "The next device list does not extend the current one".to_string(),
            })?;
        Ok(statement)
    }

    async fn publish(&self, statement: &SignedStatement) -> Result<u64> {
        let version = statement
            .open()
            .context(ErrorData::SigningFailed {
                message: "Could not read the signed device list".to_string(),
            })?
            .sequence;
        match self
            .api
            .publish_signer_list(self.chain.project(), statement)
            .await?
        {
            ListPublished::Stored => Ok(version),
            ListPublished::AlreadyOn { enabled_by } => {
                Err(AlienError::new(ErrorData::SigningFailed {
                    message: format!(
                        "The server refused device list version {version}: signing was turned on by {enabled_by}"
                    ),
                }))
            }
        }
    }
}

pub async fn approve_task(args: ApproveArgs, ctx: ExecutionMode) -> Result<()> {
    if !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "`alien approve` asks you to type the pairing code; run it in a terminal"
                .to_string(),
        }));
    }
    let mut admin = admin(&ctx).await?;
    let project = admin.chain.project().to_string();
    let changes = match &args.id {
        Some(id) => vec![admin.api.pending_change(id).await?],
        None => admin.api.pending(&project).await?,
    };
    if changes.is_empty() {
        println!("No waiting requests for {}.", admin.chain.project());
        return Ok(());
    }
    for change in changes {
        approve_one(&mut admin, &change).await?;
    }
    Ok(())
}

async fn approve_one(admin: &mut Admin, change: &PendingChange) -> Result<()> {
    let list = admin.chain.current();
    if change.status != api::PendingStatus::Pending {
        println!("{} is already resolved.", change.id);
        return Ok(());
    }
    if change.based_on_version != list.version {
        println!(
            "{} was prepared from device list v{}, but the list is now v{}; ask for it again.",
            change.id, change.based_on_version, list.version
        );
        return Ok(());
    }
    let body = change.body.clone();
    if body.recovered {
        return Err(refused(change, "it claims to be a recovery"));
    }
    let added: Vec<Signer> = added_signers(&list.body, &body)
        .into_iter()
        .cloned()
        .collect();
    for signer in &added {
        if signer.kind != SignerKind::Device {
            return Err(refused(
                change,
                &format!(
                    "it adds a {} key ({}); CI keys are added with `alien devices add-ci` or `alien devices add-key`",
                    kind_name(signer.kind),
                    signer.id
                ),
            ));
        }
    }
    match added.as_slice() {
        [signer] if diff(&list.body, &body).len() == 1 => {
            println!(
                "{} asks to: {}",
                who_with_backend(signer),
                roles_text(&signer.roles)
            )
        }
        _ => {
            println!(
                "{} prepared a change to the devices of {}:",
                change.requested_by.as_deref().unwrap_or("Someone"),
                admin.chain.project()
            );
            for line in diff(&list.body, &body) {
                println!("  {line}");
            }
        }
    }
    for signer in &added {
        let typed = prompt(&format!("  Code from {} ▸ ", first_name(signer)))?;
        if !same_code(&typed, &pairing_code(signer, &change.id)) {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "That isn't the pairing code {} shows; nothing was signed. Ask them to read it again.",
                    signer.id
                ),
            }));
        }
    }
    let statement = admin.sign_next(body, "approve")?;
    let version = admin.publish(&statement).await?;
    admin.api.approve(&change.id, version).await?;
    admin
        .chain
        .extend(std::slice::from_ref(&statement))
        .context(ErrorData::SigningFailed {
            message: "The approved device list does not extend the current one".to_string(),
        })?;
    match added.as_slice() {
        [signer] => println!(
            "✓ {} can {} · delivered to protected deployments as the next device list",
            signer.id,
            roles_text(&signer.roles)
        ),
        _ => println!(
            "✓ Device list v{version} · delivered to protected deployments as the next device list"
        ),
    }
    Ok(())
}

fn refused(change: &PendingChange, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SigningFailed {
        message: format!("Refusing {}: {reason}. Nothing was signed.", change.id),
    })
}

/// How `next` differs from `list`, one line per signer and setting.
fn diff(current: &SignerListBody, next: &SignerListBody) -> Vec<String> {
    let mut lines = Vec::new();
    for signer in added_signers(current, next) {
        lines.push(format!(
            "+ {} · {} · {} · key {}",
            who_with_backend(signer),
            kind_name(signer.kind),
            roles_text(&signer.roles),
            short_key(&signer.key)
        ));
    }
    for signer in &current.signers {
        let Some(other) = next.signers.iter().find(|other| other.key == signer.key) else {
            lines.push(format!("- {}", who(signer)));
            continue;
        };
        // Each field on its own line, so no change hides behind another.
        if other.id != signer.id {
            lines.push(format!(
                "~ {}: id {} → {}",
                who(signer),
                signer.id,
                other.id
            ));
        }
        if other.label != signer.label {
            lines.push(format!(
                "~ {}: label {} → {}",
                who(signer),
                signer.label.as_deref().unwrap_or("none"),
                other.label.as_deref().unwrap_or("none")
            ));
        }
        if other.kind != signer.kind {
            lines.push(format!(
                "~ {}: kind {} → {}",
                who(signer),
                kind_name(signer.kind),
                kind_name(other.kind)
            ));
        }
        if other.backend != signer.backend {
            lines.push(format!(
                "~ {}: key kept in {} → {}",
                who(signer),
                signer.backend.map_or("unknown", |backend| backend.as_str()),
                other.backend.map_or("unknown", |backend| backend.as_str())
            ));
        }
        if other.roles != signer.roles {
            lines.push(format!(
                "~ {}: roles {} → {}",
                who(signer),
                roles_text(&signer.roles),
                roles_text(&other.roles)
            ));
        }
    }
    for entry in next
        .deny
        .iter()
        .filter(|entry| !current.deny.contains(entry))
    {
        lines.push(format!("+ refuse {}", deny_text(entry)));
    }
    for entry in current
        .deny
        .iter()
        .filter(|entry| !next.deny.contains(entry))
    {
        lines.push(format!("- stop refusing {}", deny_text(entry)));
    }
    for entry in next
        .keep
        .iter()
        .filter(|entry| !current.keep.iter().any(|held| held.digest == entry.digest))
    {
        lines.push(format!("+ keep {}", keep_text(entry)));
    }
    for entry in current
        .keep
        .iter()
        .filter(|entry| !next.keep.iter().any(|held| held.digest == entry.digest))
    {
        lines.push(format!("- stop keeping {}", keep_text(entry)));
    }
    if next.admin_threshold != current.admin_threshold {
        lines.push(format!(
            "~ admin signatures needed: {} → {}",
            current.admin_threshold, next.admin_threshold
        ));
    }
    if lines.is_empty() {
        lines.push("no device changes".to_string());
    }
    lines
}

/// A kept statement in words.
fn keep_text(entry: &Keep) -> String {
    format!(
        "{} signed as {} ({}) · called {} by the control plane · {}",
        entry.kind.as_str(),
        entry.sequence,
        signed_date(entry.sequence),
        entry.release_id.as_deref().unwrap_or("no release id"),
        entry.digest
    )
}

/// A deny-list entry in words: a whole release or one statement.
fn deny_text(entry: &str) -> String {
    match entry.strip_prefix("release:") {
        Some(sequence) => format!("release signed as {sequence}"),
        None => format!("statement {entry}"),
    }
}

/// The first characters of a public key, enough to tell keys apart.
fn short_key(key: &str) -> String {
    let key = key
        .strip_prefix(alien_signing::PUBLIC_KEY_PREFIX)
        .unwrap_or(key);
    key.chars().take(12).collect()
}

/// `who`, and where the key lives when the signer who added it said.
fn who_with_backend(signer: &Signer) -> String {
    match signer.backend {
        Some(backend) => format!("{} · {}", who(signer), backend.as_str()),
        None => who(signer),
    }
}

fn who(signer: &Signer) -> String {
    match &signer.label {
        Some(label) => format!("{} ({label})", signer.id),
        None => signer.id.clone(),
    }
}

/// `Bob` for `bob@acme.com`, or the device id.
fn first_name(signer: &Signer) -> String {
    let Some(label) = &signer.label else {
        return signer.id.clone();
    };
    let name = label
        .split('@')
        .next()
        .unwrap_or(label)
        .split(['.', '_', '-', '+'])
        .next()
        .unwrap_or(label);
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => signer.id.clone(),
    }
}

fn roles_text(roles: &[Role]) -> String {
    roles
        .iter()
        .map(|role| match role {
            Role::Release => "release",
            Role::Admin => "manage devices",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn prompt(text: &str) -> Result<String> {
    print!("{text}");
    std::io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to show the prompt".to_string(),
        })?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to read the answer".to_string(),
        })?;
    Ok(line.trim().to_string())
}

/// `alien devices init`: the only way signing starts for a project. Releases
/// without `.alien/signing.json` stay unsigned and never reach the signing
/// routes.
async fn init(ctx: &ExecutionMode, project: Option<&str>) -> Result<()> {
    if !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "`alien devices init` shows a recovery code and asks for part of it back; run it in a terminal".to_string(),
        }));
    }
    let dir = current_dir()?;
    if let Some(pin) = RepoSigning::read(&dir)? {
        println!(
            "Signing is already on for {}: {REPO_SIGNING_FILE} pins {}.",
            pin.project,
            display_fingerprint(&pin.fingerprint)
        );
        return Ok(());
    }
    let (project_id, _link) = ctx.resolve_project(project, false).await?;
    let api = SigningApi::from_context(ctx).await?;
    match release::enable(&api, &dir, &project_id).await? {
        None => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "This control plane doesn't support signed releases yet, so signing was not turned on".to_string(),
        })),
        Some(Enabled::AlreadyOn { by }) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Signing was already turned on for {project_id} by {by}; pull {REPO_SIGNING_FILE}"
            ),
        })),
        Some(Enabled::On { .. }) => {
            show_protected_groups(ctx, &project_id).await;
            println!("Next:");
            println!("  alien devices add-ci OWNER/REPO   let GitHub Actions sign releases");
            println!("  alien approve                     trust a teammate's device after they run `alien release`");
            println!("  alien release                     sign and publish a release");
            Ok(())
        }
    }
}

/// Which deployment groups protect their new deployments, and what that
/// leaves alone. Only informational: signing is already on when it runs.
async fn show_protected_groups(ctx: &ExecutionMode, project_id: &str) {
    println!("Existing deployments stay unprotected.");
    if !ctx.is_platform() {
        println!();
        return;
    }
    match protecting_groups(ctx, project_id).await {
        Ok(groups) if groups.is_empty() => {
            println!("No deployment group protects its new deployments.");
        }
        Ok(groups) => {
            println!(
                "New deployments follow signed releases in: {}.",
                groups.join(", ")
            );
            println!("Turn it off per group in the dashboard.");
        }
        Err(error) => {
            println!(
                "Could not list the deployment groups that protect new deployments: {}",
                error.message
            );
        }
    }
    println!();
}

/// Names of the project's deployment groups whose new deployments start
/// protected.
async fn protecting_groups(ctx: &ExecutionMode, project_id: &str) -> Result<Vec<String>> {
    let mut query = api::project_query(ctx, project_id).await?;
    query.push(("limit".to_string(), "100".to_string()));
    let mut groups = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut page_query = query.clone();
        if let Some(cursor) = &cursor {
            page_query.push(("cursor".to_string(), cursor.clone()));
        }
        let page = api::get_json(ctx, "/v1/deployment-groups", &page_query).await?;
        for item in page["items"].as_array().into_iter().flatten() {
            let protects = item["protectNewDeployments"].as_bool() == Some(true);
            if let (true, Some(name)) = (protects, item["name"].as_str()) {
                groups.push(name.to_string());
            }
        }
        cursor = page["nextCursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            return Ok(groups);
        }
    }
}

pub async fn devices_task(args: DevicesArgs, ctx: ExecutionMode) -> Result<()> {
    match args.command.unwrap_or(DevicesCommand::List) {
        DevicesCommand::Init { project } => init(&ctx, project.as_deref()).await,
        DevicesCommand::List => list(&ctx, args.json).await,
        DevicesCommand::Remove {
            name,
            revoke,
            keep_running,
        } => remove(&ctx, &name, intent(revoke, keep_running, "device")?).await,
        DevicesCommand::AddCi { repository } => add_ci(&ctx, &repository).await,
        DevicesCommand::AddKey { name, out } => add_key(&ctx, &name, out.as_deref()).await,
        DevicesCommand::Rotate {
            name,
            revoke,
            keep_running,
            out,
        } => {
            rotate(
                &ctx,
                &name,
                intent(revoke, keep_running, "key")?,
                out.as_deref(),
            )
            .await
        }
        DevicesCommand::Recover => recover(&ctx).await,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicesJson<'a> {
    project: &'a str,
    fingerprint: String,
    list_version: u64,
    recovered: bool,
    devices: Vec<DeviceJson<'a>>,
    waiting: Vec<WaitingJson<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceJson<'a> {
    id: &'a str,
    label: Option<&'a str>,
    kind: &'static str,
    backend: Option<&'static str>,
    roles: &'a [Role],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WaitingJson<'a> {
    id: &'a str,
    summary: &'a str,
    requested_by: Option<&'a str>,
}

async fn list(ctx: &ExecutionMode, json: bool) -> Result<()> {
    let repo = repo(&current_dir()?)?;
    let api = SigningApi::from_context(ctx).await?;
    let chain = verified_chain(&api, &repo).await?;
    let waiting = api.pending(chain.project()).await?;
    let list = chain.current();
    if json {
        return print_json(&DevicesJson {
            project: chain.project(),
            fingerprint: repo.fingerprint.clone(),
            list_version: list.version,
            recovered: list.body.recovered,
            devices: list
                .body
                .signers
                .iter()
                .map(|signer| DeviceJson {
                    id: &signer.id,
                    label: signer.label.as_deref(),
                    kind: kind_name(signer.kind),
                    backend: signer.backend.map(|backend| backend.as_str()),
                    roles: &signer.roles,
                })
                .collect(),
            waiting: waiting
                .iter()
                .map(|change| WaitingJson {
                    id: &change.id,
                    summary: &change.summary,
                    requested_by: change.requested_by.as_deref(),
                })
                .collect(),
        });
    }
    println!("Trusted devices for {}", chain.project());
    let mut table = crate::ui::make_table(&["Device", "Kind", "Key", "Can", "Label"]);
    for signer in &list.body.signers {
        table.add_row(vec![
            signer.id.clone(),
            kind_name(signer.kind).to_string(),
            signer
                .backend
                .map(|backend| backend.as_str().to_string())
                .unwrap_or_default(),
            roles_text(&signer.roles),
            signer.label.clone().unwrap_or_default(),
        ]);
    }
    crate::ui::print_table(table);
    if let Some((record, key)) = SigningHome::current()?.device()? {
        let on_list = list
            .body
            .signers
            .iter()
            .any(|signer| signer.key == key.public_key.as_str());
        println!(
            "This device  {} · {}{}",
            record.signer_id,
            record.backend.hardware(),
            if on_list { "" } else { " · not trusted yet" }
        );
    }
    println!("Fingerprint  {}", display_fingerprint(&repo.fingerprint));
    println!(
        "Device list  v{}{}",
        list.version,
        if list.body.recovered {
            " · restored with the recovery code"
        } else {
            ""
        }
    );
    if !waiting.is_empty() {
        println!(
            "Waiting      {} request(s); run `alien approve`",
            waiting.len()
        );
    }
    Ok(())
}

/// The device list without `name`. Refused when no admin but the recovery key would be left:
/// every later device change would then need the recovery code, which is for a lost device,
/// not for a removal.
fn without_signer(current: &SignerListBody, name: &str) -> Result<SignerListBody> {
    let mut body = current.clone();
    let before = body.signers.len();
    body.signers.retain(|signer| signer.id != name);
    if body.signers.len() == before {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("No trusted device is named '{name}'; see `alien devices list`"),
        }));
    }
    let admins = |signers: &[Signer], recovery: bool| {
        signers
            .iter()
            .filter(|signer| {
                signer.roles.contains(&Role::Admin)
                    && (recovery || signer.kind != SignerKind::Recovery)
            })
            .count()
    };
    if admins(&body.signers, false) == 0 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!(
                "'{name}' is the last admin device; only the recovery code could change the devices after. To move admin to another device, run `alien devices recover` there with the recovery code"
            ),
        }));
    }
    body.admin_threshold = body.admin_threshold.min(admins(&body.signers, true) as u32);
    body.recovered = false;
    Ok(body)
}

async fn remove(ctx: &ExecutionMode, name: &str, revoke: bool) -> Result<()> {
    let admin = admin(ctx).await?;
    let mut body = without_signer(&admin.chain.current().body, name)?;
    let removed = admin
        .chain
        .current()
        .body
        .signers
        .iter()
        .find(|signer| signer.id == name)
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: format!("No trusted device is named '{name}'; see `alien devices list`"),
            })
        })?;
    println!(
        "Removing {name} from the devices of {}.",
        admin.chain.project()
    );
    let keeps = keep_current(ctx, &admin, &removed, revoke).await?;
    add_keeps(&mut body, keeps)?;
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    println!(
        "✓ {name} can no longer sign · delivered to protected deployments as the next device list"
    );
    Ok(())
}

/// One statement a key signed that is current on a channel or run by a deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    keep: Keep,
    /// The release's platform, for a release statement.
    platform: Option<String>,
    /// Channels the control plane says the release is current on.
    channels: Vec<String>,
    /// Deployments the control plane says run it (or the config).
    deployments: Vec<String>,
    /// The release's channel as the statement signs it.
    signed_channel: Option<String>,
}

/// When a statement was signed: its sequence is the time in milliseconds.
fn signed_date(sequence: u64) -> String {
    i64::try_from(sequence)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|date| date.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "unknown date".to_string())
}

/// Print what is kept, one line per statement, so the admin sees exactly what the
/// next list signs.
fn found_text(found: &Found) -> String {
    let what = match (&found.keep.kind, &found.platform) {
        (KeptKind::Release, Some(platform)) => format!("release for {platform}"),
        (KeptKind::Release, None) => "release".to_string(),
        (KeptKind::Config, _) => "config".to_string(),
    };
    let mut places = Vec::new();
    if !found.channels.is_empty() {
        places.push(format!("current on {}", found.channels.join(", ")));
    }
    if !found.deployments.is_empty() {
        places.push(format!("run by {}", found.deployments.join(", ")));
    }
    format!(
        "{what} signed as {} ({}){}{} · called {} by the control plane · {}",
        found.keep.sequence,
        signed_date(found.keep.sequence),
        found
            .signed_channel
            .as_ref()
            .map(|channel| format!(" · channel {channel}"))
            .unwrap_or_default(),
        if places.is_empty() {
            String::new()
        } else {
            format!(" · {}", places.join(" · "))
        },
        found.keep.release_id.as_deref().unwrap_or("no release id"),
        found.keep.digest,
    )
}

/// The statements `key` signed (as `signer`) that are current on a channel or run by
/// a deployment, from what the control plane lists. Only signatures that verify
/// count, so a control plane can't get other statements kept this way. A statement the
/// control plane leaves out is not kept, which is the safe direction: it is refused.
fn current_signed_by(
    releases: &[serde_json::Value],
    channels: &[serde_json::Value],
    deployments: &[(serde_json::Value, Vec<SignedStatement>)],
    project: &str,
    signer: &str,
    key: &PublicKey,
) -> Vec<Found> {
    let mut channels_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for channel in channels {
        if let (Some(release), Some(name)) = (
            channel["currentReleaseId"].as_str(),
            channel["name"].as_str(),
        ) {
            channels_of
                .entry(release.to_string())
                .or_default()
                .push(name.to_string());
        }
    }
    let mut running: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (deployment, _) in deployments {
        let Some(id) = deployment["id"].as_str() else {
            continue;
        };
        for field in ["currentReleaseId", "desiredReleaseId"] {
            if let Some(release) = deployment[field].as_str() {
                let held = running.entry(release.to_string()).or_default();
                if !held.iter().any(|name| name == id) {
                    held.push(id.to_string());
                }
            }
        }
    }
    let mut found: Vec<Found> = Vec::new();
    let mut push = |item: Found| {
        if !found
            .iter()
            .any(|held| held.keep.digest == item.keep.digest)
        {
            found.push(item);
        }
    };
    for release in releases {
        let Some(id) = release["id"].as_str() else {
            continue;
        };
        let mut release_channels = channels_of.get(id).cloned().unwrap_or_default();
        for name in release["currentChannels"].as_array().into_iter().flatten() {
            if let Some(name) = name.as_str() {
                if !release_channels.iter().any(|held| held == name) {
                    release_channels.push(name.to_string());
                }
            }
        }
        let deployments_running = running.get(id).cloned().unwrap_or_default();
        if release_channels.is_empty() && deployments_running.is_empty() {
            continue;
        }
        for (platform, statement) in release["signedStacks"].as_object().into_iter().flatten() {
            let Ok(statement) = serde_json::from_value::<SignedStatement>(statement.clone()) else {
                continue;
            };
            let Some(opened) = signed_by_key(&statement, Kind::Release, project, signer, key)
            else {
                continue;
            };
            let signed_channel = opened
                .body::<alien_signing::ReleaseBody>()
                .ok()
                .map(|body| body.channel);
            push(Found {
                keep: Keep {
                    digest: opened.digest,
                    kind: KeptKind::Release,
                    release_id: Some(id.to_string()),
                    sequence: opened.sequence,
                },
                platform: Some(platform.clone()),
                channels: release_channels.clone(),
                deployments: deployments_running.clone(),
                signed_channel,
            });
        }
    }
    for (deployment, statements) in deployments {
        let Some(id) = deployment["id"].as_str() else {
            continue;
        };
        // The config a deployment runs is its newest.
        let newest = statements
            .iter()
            .filter_map(|statement| statement.open().ok())
            .filter(|opened| opened.kind == Kind::Config && opened.project == project)
            .max_by_key(|opened| opened.sequence);
        let Some(newest) = newest else {
            continue;
        };
        let signed = newest.signatures.iter().any(|signature| {
            signature.signer == signer && key.verify(&newest.bytes, &signature.sig).is_ok()
        });
        if signed {
            push(Found {
                keep: Keep {
                    digest: newest.digest,
                    kind: KeptKind::Config,
                    release_id: deployment["currentReleaseId"].as_str().map(str::to_string),
                    sequence: newest.sequence,
                },
                platform: None,
                channels: Vec::new(),
                deployments: vec![id.to_string()],
                signed_channel: None,
            });
        }
    }
    found
}

/// `statement` opened, when it is a `kind` statement of `project` that `key` signed as `signer`.
fn signed_by_key(
    statement: &SignedStatement,
    kind: Kind,
    project: &str,
    signer: &str,
    key: &PublicKey,
) -> Option<alien_signing::envelope::Opened> {
    let opened = statement.open().ok()?;
    let ours = opened.kind == kind
        && opened.project == project
        && opened.signatures.iter().any(|signature| {
            signature.signer == signer && key.verify(&opened.bytes, &signature.sig).is_ok()
        });
    ours.then_some(opened)
}

/// Add `keeps` to `body`'s `keep`, once each.
fn add_keeps(body: &mut SignerListBody, keeps: Vec<Keep>) -> Result<()> {
    for keep in keeps {
        if !body.keep.iter().any(|held| held.digest == keep.digest) {
            body.keep.push(keep);
        }
    }
    if body.keep.len() > alien_signing::MAX_KEEP {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "The device list would keep {} statements; a list keeps at most {}",
                body.keep.len(),
                alien_signing::MAX_KEEP
            ),
        }));
    }
    Ok(())
}

/// Whether the admin says the key leaked: `true` keeps nothing it signed.
///
/// The control plane can only say what is current, and a thief with the key can make
/// its own release current. So the admin states the intent: `--revoke` (leaked) or
/// `--keep-running` (not leaked). With neither, a terminal is asked; without a terminal
/// the command refuses.
fn intent(revoke: bool, keep_running: bool, what: &str) -> Result<bool> {
    let interactive = crate::output::can_prompt();
    resolve_intent(revoke, keep_running, what, interactive, || {
        use std::io::{BufRead, Write};
        print!("Was this {what} leaked or lost? (leaked/no) ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok();
        line
    })
}

fn resolve_intent(
    revoke: bool,
    keep_running: bool,
    what: &str,
    interactive: bool,
    ask: impl FnOnce() -> String,
) -> Result<bool> {
    let invalid = |message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: "revoke".to_string(),
            message,
        })
    };
    match (revoke, keep_running) {
        (true, true) => Err(invalid(
            "Pass only one of --revoke and --keep-running".to_string(),
        )),
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        (false, false) if !interactive => Err(invalid(format!(
            "Say what happened to the {what}: pass --revoke if it leaked (keeps nothing it signed), or --keep-running if it did not (keeps what is current and prints it)"
        ))),
        (false, false) => match ask().trim().to_lowercase().as_str() {
            "leaked" | "l" => Ok(true),
            "no" | "n" => Ok(false),
            other => Err(invalid(format!(
                "Answer 'leaked' or 'no', not '{other}'. Nothing was changed."
            ))),
        },
    }
}

/// Ask the control plane for what `signer`'s key signed that is current and print it.
/// Returns what to keep: all of it, or none with `revoke`. Prints exactly what is kept.
async fn keep_current(
    ctx: &ExecutionMode,
    admin: &Admin,
    signer: &Signer,
    revoke: bool,
) -> Result<Vec<Keep>> {
    let key = PublicKey::parse(&signer.key).context(ErrorData::SigningFailed {
        message: format!("The key of '{}' on the list doesn't parse", signer.id),
    })?;
    let project = admin.chain.project().to_string();
    let query = api::project_query(ctx, &project).await?;
    let releases = all_items(ctx, "/v1/releases", &query).await?;
    let channels = all_items(ctx, "/v1/release-channels", &query).await?;
    let mut deployments = Vec::new();
    for deployment in all_items(ctx, "/v1/deployments", &query).await? {
        let id = deployment["id"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::SigningFailed {
                message: "The server listed a deployment without an id".to_string(),
            })
        })?;
        let statements = admin.api.config_statements(id).await?;
        deployments.push((deployment.clone(), statements));
    }
    let found = current_signed_by(
        &releases,
        &channels,
        &deployments,
        &project,
        &signer.id,
        &key,
    );
    if found.is_empty() {
        println!(
            "  {} signed no release current on a channel and no config a deployment runs, so nothing needs keeping.",
            signer.id
        );
        return Ok(Vec::new());
    }
    if revoke {
        println!(
            "  --revoke: keeping none. Protected deployments will refuse these {} statement(s) {} signed:",
            found.len(),
            signer.id
        );
    } else {
        println!(
            "  Keeping these {} statement(s) {} signed, so they keep running (pass --revoke to keep none):",
            found.len(),
            signer.id
        );
    }
    for item in &found {
        println!("    - {}", found_text(item));
    }
    Ok(if revoke {
        Vec::new()
    } else {
        found.into_iter().map(|item| item.keep).collect()
    })
}

/// The signer id a CI key in `repository` gets.
fn ci_id(repository: &str) -> String {
    let id: String = format!("github-{repository}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    id.chars().take(64).collect()
}

fn ci_label(repository: &str) -> String {
    format!("GitHub Actions ({repository})")
}

fn check_repository(repository: &str) -> Result<()> {
    let parts: Vec<&str> = repository.split('/').collect();
    let valid = parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        });
    if valid {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::ValidationError {
            field: "repository".to_string(),
            message: format!("'{repository}' is not OWNER/REPO"),
        }))
    }
}

async fn add_ci(ctx: &ExecutionMode, repository: &str) -> Result<()> {
    check_repository(repository)?;
    let admin = admin(ctx).await?;
    let id = ci_id(repository);
    if admin.chain.current().signer(&id).is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "repository".to_string(),
            message: format!(
                "{repository} already has a CI key; replace it with `alien devices rotate {id}`"
            ),
        }));
    }
    // The CLI pin is checked before anything changes, so a release that
    // doesn't verify leaves no key stored and no list published.
    pin_cli(&current_dir()?)?;
    let key = new_ci_key()?;
    let who_can = store_ci_key(repository, &key)?;
    println!(
        "Trusting GitHub Actions in {repository} to sign releases of {}.",
        admin.chain.project()
    );
    publish_ci_signer(&admin, &id, Some(ci_label(repository)), &key).await?;
    println!(
        "✓ {} can release · its key is in the `{GITHUB_ENVIRONMENT}` environment, which {who_can}",
        ci_label(repository)
    );
    Ok(())
}

/// Whether `name` is a signer id: 1-64 of `[a-z0-9-]`.
fn check_key_name(name: &str) -> Result<()> {
    let valid = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("'{name}' is not a key name: use 1-64 of a-z, 0-9 and -"),
        }))
    }
}

fn new_ci_key() -> Result<SecretKey> {
    SecretKey::generate().context(ErrorData::SigningFailed {
        message: "Could not create the CI key".to_string(),
    })
}

/// Add `key` as CI signer `id`, with the release role only, as the next
/// version of the list.
async fn publish_ci_signer(
    admin: &Admin,
    id: &str,
    label: Option<String>,
    key: &SecretKey,
) -> Result<()> {
    let mut body = admin.chain.current().body.clone();
    body.signers.push(Signer {
        id: id.to_string(),
        key: key.public_key().as_str().to_string(),
        kind: SignerKind::Ci,
        backend: Some(KeyBackend::Software),
        roles: vec![Role::Release],
        label,
    });
    body.recovered = false;
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    Ok(())
}

/// `alien devices add-key`: a software key with the release role for any CI
/// system or remote box. The key is shown once, after the list that trusts it
/// is published.
async fn add_key(ctx: &ExecutionMode, name: &str, out: Option<&Path>) -> Result<()> {
    check_key_name(name)?;
    check_out_free(out)?;
    let admin = admin(ctx).await?;
    if admin.chain.current().signer(name).is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!(
                "'{name}' is already on the list; replace its key with `alien devices rotate {name}`"
            ),
        }));
    }
    let key = new_ci_key()?;
    eprintln!(
        "Trusting a new key, {name}, to sign releases of {}.",
        admin.chain.project()
    );
    // The key is written before the list trusts it: a key that could not be
    // written is never left trusted with nowhere to be found.
    show_ci_key(&key, out)?;
    publish_ci_signer(&admin, name, None, &key).await?;
    eprintln!("✓ {name} can release (and sign debug sessions); it can't change the devices");
    Ok(())
}

/// Refuse an `--out` file that already exists before the list changes, so a key
/// the list trusts is never left with nowhere to be written.
fn check_out_free(out: Option<&Path>) -> Result<()> {
    match out {
        Some(path) if path.exists() => Err(AlienError::new(ErrorData::ValidationError {
            field: "out".to_string(),
            message: format!("{} already exists; choose a new file", path.display()),
        })),
        _ => Ok(()),
    }
}

/// Print `key` as PKCS#8 PEM to stdout, or write it to `out` (mode 0600),
/// then say how to store it. Shown once: the control plane never holds it.
fn show_ci_key(key: &SecretKey, out: Option<&Path>) -> Result<()> {
    let pem = key.to_pkcs8_pem().context(ErrorData::SigningFailed {
        message: "Could not encode the CI key".to_string(),
    })?;
    match out {
        Some(path) => {
            crate::signing::device::write_private(path, &pem)?;
            eprintln!("Key written to {} (mode 0600).", path.display());
        }
        None => print!("{pem}"),
    }
    eprintln!(
        "Store it as {GITHUB_SECRET} in your CI system's secrets: the CLI reads the PEM from that variable. It is shown once."
    );
    Ok(())
}

async fn rotate(ctx: &ExecutionMode, name: &str, revoke: bool, out: Option<&Path>) -> Result<()> {
    check_out_free(out)?;
    let admin = admin(ctx).await?;
    let list = admin.chain.current();
    let old = list
        .body
        .signers
        .iter()
        .find(|signer| signer.id == name && signer.kind == SignerKind::Ci)
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: format!(
                    "No CI key is named '{name}'; add one with `alien devices add-key {name}` or `add-ci OWNER/REPO`"
                ),
            })
        })?;
    // A key `add-ci` made lives in a GitHub environment; any other is shown.
    let github = old
        .label
        .as_deref()
        .and_then(|label| label.strip_prefix("GitHub Actions ("))
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::to_string);
    println!("Replacing the key {name}.");
    let keeps = keep_current(ctx, &admin, &old, revoke).await?;
    let key = new_ci_key()?;
    let who_can = match &github {
        Some(repository) => {
            pin_cli(&current_dir()?)?;
            Some(store_ci_key(repository, &key)?)
        }
        None => None,
    };
    if github.is_none() {
        // Written before the list trusts it, so a failed write publishes nothing.
        show_ci_key(&key, out)?;
    }
    let mut body = list.body.clone();
    for signer in body.signers.iter_mut().filter(|signer| signer.id == old.id) {
        signer.key = key.public_key().as_str().to_string();
        signer.backend = Some(KeyBackend::Software);
    }
    add_keeps(&mut body, keeps)?;
    body.recovered = false;
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    match who_can {
        Some(who_can) => println!(
            "✓ New key in the `{GITHUB_ENVIRONMENT}` environment of {}, which {who_can}; the old key can't sign",
            github.unwrap_or_default()
        ),
        None => eprintln!("✓ The old {name} key can't sign"),
    }
    Ok(())
}

/// One platform's statement of a release, as the admin's own list verifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SignedFacts {
    platform: String,
    sequence: u64,
    digest: String,
    channel: String,
    /// What the control plane calls the release.
    called: String,
    /// Channels the control plane says the release is current on.
    current_on: Vec<String>,
}

/// The release statements the control plane lists, each checked against the admin's
/// verified signer list, by the list version each names. A statement that doesn't check
/// out is an error when `strict` (the admin named that release) and skipped otherwise.
fn signed_facts(
    chain: &SignerChain,
    release: &serde_json::Value,
    channels: &[serde_json::Value],
    strict: bool,
) -> Result<Vec<SignedFacts>> {
    let id = release["id"].as_str().unwrap_or("unknown").to_string();
    let mut current_on: Vec<String> = release["currentChannels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|channel| channel.as_str().map(str::to_string))
        .collect();
    for channel in channels {
        if channel["currentReleaseId"].as_str() == Some(id.as_str()) {
            if let Some(name) = channel["name"].as_str() {
                if !current_on.iter().any(|held| held == name) {
                    current_on.push(name.to_string());
                }
            }
        }
    }
    let mut facts = Vec::new();
    for (platform, statement) in release["signedStacks"].as_object().into_iter().flatten() {
        let parsed: std::result::Result<SignedStatement, _> =
            serde_json::from_value(statement.clone());
        let statement = match parsed {
            Ok(statement) => statement,
            Err(error) if strict => {
                return Err(AlienError::new(ErrorData::SigningFailed {
                    message: format!(
                        "The server returned a release statement that doesn't parse: {error}"
                    ),
                }))
            }
            Err(_) => continue,
        };
        let opened = match statement.open() {
            Ok(opened) => opened,
            Err(_) if !strict => continue,
            Err(_) => {
                return Err(AlienError::new(ErrorData::SigningFailed {
                    message: "The server returned a release statement that doesn't open"
                        .to_string(),
                }))
            }
        };
        if let Err(reason) = crate::signing::check_statement(chain, &opened, Kind::Release, None) {
            if strict {
                return Err(AlienError::new(ErrorData::SigningFailed {
                    message: format!(
                        "A statement of release {id} doesn't check out against the device list: {reason}. Nothing was signed."
                    ),
                }));
            }
            continue;
        }
        let channel = opened
            .body::<alien_signing::ReleaseBody>()
            .map(|body| body.channel)
            .unwrap_or_default();
        facts.push(SignedFacts {
            platform: platform.clone(),
            sequence: opened.sequence,
            digest: opened.digest,
            channel,
            called: id.clone(),
            current_on: current_on.clone(),
        });
    }
    Ok(facts)
}

/// What `alien releases revoke` will refuse: every statement of one signed release,
/// found by the release id the admin named (as the control plane resolves it) or by
/// the signed sequence. Refuses when the statements don't all belong to one signed
/// release, or when that release is current on a channel.
fn revoke_target(
    chain: &SignerChain,
    releases: &[serde_json::Value],
    channels: &[serde_json::Value],
    id: Option<&str>,
    sequence: Option<u64>,
    force: bool,
) -> Result<Vec<SignedFacts>> {
    let refuse = |message: String| {
        AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message,
        })
    };
    let mut all = Vec::new();
    for release in releases {
        all.extend(signed_facts(chain, release, channels, false)?);
    }
    let named: Vec<SignedFacts> = match (id, sequence) {
        (Some(id), None) => {
            let release = releases
                .iter()
                .find(|release| release["id"].as_str() == Some(id))
                .ok_or_else(|| refuse(format!("The control plane has no release {id}")))?;
            let facts = signed_facts(chain, release, channels, true)?;
            if facts.is_empty() {
                return Err(refuse(format!(
                    "Release {id} is not signed, so there is nothing to refuse"
                )));
            }
            // The control plane's id is only a name: the statements it returns must be one
            // signed release, one sequence and one channel, or they are not what was asked for.
            let mut sequences: Vec<u64> = facts.iter().map(|fact| fact.sequence).collect();
            sequences.dedup();
            let mut signed_channels: Vec<&str> =
                facts.iter().map(|fact| fact.channel.as_str()).collect();
            signed_channels.dedup();
            if sequences.len() != 1 || signed_channels.len() != 1 {
                return Err(refuse(format!(
                    "The statements returned for release {id} are not one signed release (sequences {sequences:?}, channels {signed_channels:?}). Nothing was signed."
                )));
            }
            facts
        }
        (None, Some(sequence)) => all
            .iter()
            .filter(|fact| fact.sequence == sequence)
            .cloned()
            .collect(),
        _ => {
            return Err(refuse(
                "Name a release id or --sequence, not both".to_string(),
            ))
        }
    };
    // The signed sequence is the release. Any release the control plane lists under it that
    // is current on a channel stops the revoke, whichever id was named.
    let mut sequences: Vec<u64> = named.iter().map(|fact| fact.sequence).collect();
    sequences.extend(sequence);
    if let Some((fact, channel)) = all
        .iter()
        .filter(|fact| sequences.contains(&fact.sequence))
        .find_map(|fact| fact.current_on.first().map(|channel| (fact, channel)))
    {
        if force {
            println!(
                "Warning: the control plane says the release signed as {} ({}) is current on {channel} (it calls it {}). Revoking it anyway because of --force.",
                fact.sequence,
                signed_date(fact.sequence),
                fact.called
            );
        } else {
            return Err(refuse(format!(
                "The release signed as {} ({}) is current on {channel} (the control plane calls it {}). Promote another release first, or pass --force to revoke it anyway (for example when the control plane itself is not trusted).",
                fact.sequence,
                signed_date(fact.sequence),
                fact.called
            )));
        }
    }
    let mut all_named = named;
    for fact in all
        .into_iter()
        .filter(|fact| sequences.contains(&fact.sequence))
    {
        if !all_named.iter().any(|held| held.digest == fact.digest) {
            all_named.push(fact);
        }
    }
    Ok(all_named)
}

/// `alien releases revoke`: refuse a release's statements everywhere, as the
/// next signer list version. The release is named by id or by its signed
/// sequence. It prints every deny-list entry it adds, led by what the statements
/// sign: the sequence, when it was signed, the channel, the platforms and the
/// digests. The control plane's release id appears only as what it calls the release.
pub async fn revoke_release(
    ctx: &ExecutionMode,
    id: Option<&str>,
    sequence: Option<u64>,
    force: bool,
) -> Result<()> {
    let admin = admin(ctx).await?;
    let project = admin.chain.project().to_string();
    let query = api::project_query(ctx, &project).await?;
    let releases = all_items(ctx, "/v1/releases", &query).await?;
    let channels = all_items(ctx, "/v1/release-channels", &query).await?;
    let target = revoke_target(&admin.chain, &releases, &channels, id, sequence, force)?;
    let mut body = admin.chain.current().body.clone();
    let lines = revoke_lines(&target);
    let mut entries: Vec<String> = Vec::new();
    // A sequence the admin names is refused even when the control plane returns no
    // statement for it: the sequence is what protected deployments check.
    if let Some(sequence) = sequence {
        let entry = alien_signing::release_deny_entry(sequence);
        if !body.deny.contains(&entry) {
            entries.push(entry);
        }
        if target.is_empty() {
            println!(
                "The control plane returned no statement signed as {sequence} ({}); refusing the sequence alone.",
                signed_date(sequence)
            );
        }
    }
    for fact in &target {
        for entry in [
            alien_signing::release_deny_entry(fact.sequence),
            fact.digest.clone(),
        ] {
            if !entries.contains(&entry) && !body.deny.contains(&entry) {
                entries.push(entry);
            }
        }
    }
    for line in &lines {
        println!("{line}");
    }
    if entries.is_empty() {
        println!("Already refused: every entry is on the device list.");
        return Ok(());
    }
    println!("Adding {} deny-list entries for {project}:", entries.len());
    for entry in &entries {
        println!("  + refuse {}", deny_text(entry));
    }
    println!("Protected deployments will not apply it, also as a rollback.");
    body.deny.extend(entries.iter().cloned());
    // The deny-list wins over keep; the list drops what it no longer needs.
    body.keep
        .retain(|kept| !target.iter().any(|fact| fact.digest == kept.digest));
    body.recovered = false;
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    println!(
        "✓ The release is refused · delivered to protected deployments as the next device list"
    );
    Ok(())
}

/// The signed facts of a revoke, before anything is added.
fn revoke_lines(target: &[SignedFacts]) -> Vec<String> {
    let Some(first) = target.first() else {
        return Vec::new();
    };
    let mut lines = vec![format!(
        "Release signed as {} ({}), channel {}:",
        first.sequence,
        signed_date(first.sequence),
        first.channel
    )];
    for fact in target {
        lines.push(format!(
            "  {} · {} · signed as {} ({})",
            fact.platform,
            fact.digest,
            fact.sequence,
            signed_date(fact.sequence)
        ));
    }
    let mut called: Vec<&str> = target.iter().map(|fact| fact.called.as_str()).collect();
    called.sort_unstable();
    called.dedup();
    lines.push(format!(
        "  called {} by the control plane",
        called.join(", ")
    ));
    lines
}

/// Every item of the list at `path`, following the server's pages to the end:
/// revoking from part of a list would leave the rest accepted.
async fn all_items(
    ctx: &ExecutionMode,
    path: &str,
    query: &[(String, String)],
) -> Result<Vec<serde_json::Value>> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut page_query = query.to_vec();
        page_query.push(("limit".to_string(), "100".to_string()));
        if let Some(cursor) = &cursor {
            page_query.push(("cursor".to_string(), cursor.clone()));
        }
        let page = api::get_json(ctx, path, &page_query).await?;
        items.extend(page["items"].as_array().into_iter().flatten().cloned());
        cursor = page["nextCursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            return Ok(items);
        }
    }
}

async fn recover(ctx: &ExecutionMode) -> Result<()> {
    if !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "`alien devices recover` reads the recovery code from a hidden prompt; run it in a terminal".to_string(),
        }));
    }
    let repo = repo(&current_dir()?)?;
    let api = SigningApi::from_context(ctx).await?;
    let chain = verified_chain(&api, &repo).await?;
    let project = chain.project().to_string();
    print!("Recovery code for {project}: ");
    std::io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to show the prompt".to_string(),
        })?;
    let typed = console::Term::stdout()
        .read_secure_line()
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to read the recovery code".to_string(),
        })?;
    let code = RecoveryCode::parse(&typed).context(ErrorData::SigningFailed {
        message: "That isn't a recovery code".to_string(),
    })?;
    let recovery = code.key(&project).context(ErrorData::SigningFailed {
        message: "Could not derive the recovery key".to_string(),
    })?;
    let list = chain.current();
    let recovery_signer = signer_with_key(list, &recovery.public_key())
        .filter(|signer| signer.kind == SignerKind::Recovery)
        .ok_or_else(|| {
            AlienError::new(ErrorData::SigningFailed {
                message: format!("That code isn't the recovery code of {project}"),
            })
        })?
        .clone();
    let home = SigningHome::current()?;
    let (record, device) = home.device_or_create()?;
    let mut body = list.body.clone();
    let lost: Vec<String> = body
        .signers
        .iter()
        .filter(|signer| {
            signer.kind == SignerKind::Device
                && signer.roles.contains(&Role::Admin)
                && signer.key != device.public_key.as_str()
        })
        .map(|signer| signer.id.clone())
        .collect();
    body.signers.retain(|signer| !lost.contains(&signer.id));
    match body
        .signers
        .iter_mut()
        .find(|signer| signer.key == device.public_key.as_str())
    {
        Some(signer) => {
            if !signer.roles.contains(&Role::Admin) {
                signer.roles.push(Role::Admin);
            }
        }
        None => {
            let mut id = record.signer_id.clone();
            let mut suffix = 2;
            while body.signers.iter().any(|signer| signer.id == id) {
                id = format!("{}-{suffix}", record.signer_id);
                suffix += 1;
            }
            body.signers.push(Signer {
                id,
                key: device.public_key.as_str().to_string(),
                kind: SignerKind::Device,
                backend: Some(record.backend.key_backend()),
                roles: vec![Role::Release, Role::Admin],
                label: record.label.clone(),
            });
        }
    }
    body.admin_threshold = 1;
    body.recovered = true;
    let signer = StatementSigner::Key {
        id: recovery_signer.id.clone(),
        key: recovery,
    };
    let envelope = next_list(&chain, body);
    let statement = sign_envelope(&envelope, &signer)?;
    let mut extended = chain.clone();
    extended
        .extend(std::slice::from_ref(&statement))
        .context(ErrorData::SigningFailed {
            message: "The recovered device list does not extend the current one".to_string(),
        })?;
    match api.publish_signer_list(&project, &statement).await? {
        ListPublished::Stored => {}
        ListPublished::AlreadyOn { enabled_by } => {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "The server refused the recovered list: signing was turned on by {enabled_by}"
                ),
            }))
        }
    }
    println!("✓ Signing restored for {project}. This device is its admin now.");
    if !lost.is_empty() {
        println!("  No longer trusted: {}", lost.join(", "));
    }
    println!(
        "  The list is marked as restored with the recovery code; `verify` shows it. The code ({RECOVERY_SIGNER}) still works."
    );
    Ok(())
}

/// Store `key` as the CI secret of `repository`'s `release` environment with
/// the admin's own `gh`, after making sure only `main`, and release tags if
/// a ruleset protects them, can use that environment. Returns who can.
fn store_ci_key(repository: &str, key: &SecretKey) -> Result<String> {
    let environment = format!("repos/{repository}/environments/{GITHUB_ENVIRONMENT}");
    gh(
        &["api", "-X", "PUT", &environment, "--input", "-"],
        Some(br#"{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}"#),
    )?;
    let policies_path = format!("{environment}/deployment-branch-policies");
    let tags = tag_protection_gap(&tag_rulesets(repository));
    let mut expected = vec![(RELEASE_BRANCH.to_string(), "branch".to_string())];
    if tags.is_none() {
        expected.push((RELEASE_TAGS.to_string(), "tag".to_string()));
    }
    let policies = branch_policies(&policies_path)?;
    for (name, kind) in &expected {
        if !policies
            .iter()
            .any(|policy| &policy.0 == name && &policy.1 == kind)
        {
            gh(
                &[
                    "api",
                    "-X",
                    "POST",
                    &policies_path,
                    "-f",
                    &format!("name={name}"),
                    "-f",
                    &format!("type={kind}"),
                ],
                None,
            )?;
        }
    }
    check_environment(
        repository,
        &environment,
        &policies_path,
        &expected,
        tags.as_deref(),
    )?;
    let pem = key.to_pkcs8_pem().context(ErrorData::SigningFailed {
        message: "Could not encode the CI key".to_string(),
    })?;
    gh(
        &[
            "secret",
            "set",
            GITHUB_SECRET,
            "--env",
            GITHUB_ENVIRONMENT,
            "--repo",
            repository,
        ],
        Some(pem.as_bytes()),
    )?;
    Ok(match tags {
        None => format!("only {RELEASE_BRANCH} and {RELEASE_TAGS} tags can use"),
        Some(_) => format!("only {RELEASE_BRANCH} can use"),
    })
}

/// The repository's tag rulesets, in full, or why they couldn't be read.
fn tag_rulesets(repository: &str) -> std::result::Result<Vec<serde_json::Value>, String> {
    let text = gh(&["api", &format!("repos/{repository}/rulesets")], None)
        .map_err(|error| format!("its rulesets can't be read ({})", error.message))?;
    let list: Vec<serde_json::Value> = serde_json::from_str(&text)
        .map_err(|error| format!("its rulesets didn't parse ({error})"))?;
    let mut rulesets = Vec::new();
    for ruleset in list.iter().filter(|ruleset| ruleset["target"] == "tag") {
        let id = &ruleset["id"];
        let text = gh(&["api", &format!("repos/{repository}/rulesets/{id}")], None)
            .map_err(|error| format!("ruleset {id} can't be read ({})", error.message))?;
        rulesets.push(
            serde_json::from_str(&text)
                .map_err(|error| format!("ruleset {id} didn't parse ({error})"))?,
        );
    }
    Ok(rulesets)
}

/// Why release tags may not use the `release` environment, or `None` when an
/// active tag ruleset covering `refs/tags/v*` blocks creating, moving and
/// deleting them and no app can bypass it. Without that, anyone who can push
/// a tag could run a workflow that reads the key.
fn tag_protection_gap(
    rulesets: &std::result::Result<Vec<serde_json::Value>, String>,
) -> Option<String> {
    let rulesets = match rulesets {
        Ok(rulesets) => rulesets,
        Err(reason) => return Some(reason.clone()),
    };
    let pattern = format!("refs/tags/{RELEASE_TAGS}");
    let mut gaps = Vec::new();
    for ruleset in rulesets {
        let name = ruleset["name"].as_str().unwrap_or("unnamed");
        let covers = ruleset["target"] == "tag"
            && ruleset["enforcement"] == "active"
            && ruleset["conditions"]["ref_name"]["include"]
                .as_array()
                .is_some_and(|include| {
                    include
                        .iter()
                        .any(|item| item == pattern.as_str() || item == "~ALL")
                })
            && ruleset["conditions"]["ref_name"]["exclude"]
                .as_array()
                .is_some_and(Vec::is_empty);
        if !covers {
            continue;
        }
        let types: Vec<&str> = ruleset["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|rule| rule["type"].as_str())
            .collect();
        let missing: Vec<&str> = ["creation", "update", "deletion"]
            .into_iter()
            .filter(|rule| !types.contains(rule))
            .collect();
        let app_bypass = match ruleset["bypass_actors"].as_array() {
            Some(actors) => actors
                .iter()
                .any(|actor| actor["actor_type"] == "Integration"),
            // Only admins see bypass actors; unknown counts as a bypass.
            None => true,
        };
        if missing.is_empty() && !app_bypass {
            return None;
        }
        let mut gap = Vec::new();
        if !missing.is_empty() {
            gap.push(format!("doesn't restrict {}", missing.join(", ")));
        }
        if app_bypass {
            gap.push("lets an app bypass it".to_string());
        }
        gaps.push(format!("ruleset '{name}' {}", gap.join(" and ")));
    }
    Some(if gaps.is_empty() {
        format!("no active tag ruleset covers {pattern}")
    } else {
        gaps.join("; ")
    })
}

/// `(name, type)` of each deployment policy of an environment.
fn branch_policies(path: &str) -> Result<Vec<(String, String)>> {
    let text = gh(&["api", path], None)?;
    let value: serde_json::Value =
        serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::SigningFailed {
                message: format!("`gh api {path}` did not answer JSON"),
            })?;
    Ok(value["branch_policies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|policy| {
            (
                policy["name"].as_str().unwrap_or_default().to_string(),
                policy["type"].as_str().unwrap_or("branch").to_string(),
            )
        })
        .collect())
}

/// Refuse to store the key unless the environment allows exactly
/// `expected`: any other branch or tag could run a workflow that reads it.
/// `tags` is why release tags aren't among them, if they aren't.
fn check_environment(
    repository: &str,
    environment: &str,
    policies_path: &str,
    expected: &[(String, String)],
    tags: Option<&str>,
) -> Result<()> {
    let text = gh(&["api", environment], None)?;
    let value: serde_json::Value =
        serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::SigningFailed {
                message: format!("`gh api {environment}` did not answer JSON"),
            })?;
    let policy = &value["deployment_branch_policy"];
    let custom = policy["custom_branch_policies"].as_bool() == Some(true)
        && policy["protected_branches"].as_bool() == Some(false);
    let mut policies = branch_policies(policies_path)?;
    policies.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    if custom && policies == expected {
        return Ok(());
    }
    let allowed: Vec<String> = policies
        .iter()
        .map(|(name, kind)| format!("{kind} {name}"))
        .collect();
    let wanted: Vec<String> = expected
        .iter()
        .map(|(name, kind)| format!("{kind} {name}"))
        .collect();
    let why_no_tags = match tags {
        Some(gap) => format!(
            " Tags {RELEASE_TAGS} may not use it: {gap}. Add an active tag ruleset on refs/tags/{RELEASE_TAGS} that restricts creation, update and deletion with no app bypass to allow them."
        ),
        None => String::new(),
    };
    Err(AlienError::new(ErrorData::SigningFailed {
        message: format!(
            "The `{GITHUB_ENVIRONMENT}` environment of {repository} must allow only {}, but allows: {}.{why_no_tags} Remove the others in the repository's Settings → Environments, then run this again. The CI key was not stored.",
            wanted.join(" and "),
            if custom { allowed.join(", ") } else { "every branch".to_string() }
        ),
    }))
}

/// Pin this CLI's own version in the release workflow, by the checksums of
/// its release archives that the GitHub release attestation vouches for. CLI
/// upgrades then reach CI only as a change to this file that the vendor
/// reviews.
fn pin_cli(dir: &Path) -> Result<()> {
    let workflow = dir.join(RELEASE_WORKFLOW);
    let text = match std::fs::read_to_string(&workflow) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "  No {RELEASE_WORKFLOW} here yet. Merge the Connect GitHub pull request, then run `alien devices rotate` from the repo to pin this CLI."
            );
            return Ok(());
        }
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: workflow.display().to_string(),
                    reason: "Failed to read the release workflow".to_string(),
                })
        }
    };
    let version = env!("CARGO_PKG_VERSION");
    let checksums = attested_checksums(version)?;
    let pinned = pin_workflow(&text, &cli_pin(version, &checksums))?;
    std::fs::write(&workflow, pinned)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: workflow.display().to_string(),
            reason: "Failed to pin the CLI in the release workflow".to_string(),
        })?;
    println!("✓ Pinned Alien CLI {version} in {RELEASE_WORKFLOW} by its attested release archives — commit it.");
    Ok(())
}

/// The SHA-256 of each CLI release archive of `version`, by runner, each
/// checked with `gh release verify-asset` against the attestation of the
/// public repository's immutable release `v<version>`.
fn attested_checksums(version: &str) -> Result<BTreeMap<&'static str, String>> {
    let dir = tempfile::tempdir()
        .into_alien_error()
        .context(ErrorData::SigningFailed {
            message: "Could not create a directory for the CLI archives".to_string(),
        })?;
    let tag = format!("v{version}");
    let target = dir.path().to_string_lossy().to_string();
    let mut checksums = BTreeMap::new();
    for (runner, suffix) in CLI_ARCHIVES {
        let asset = format!("alien-v{version}-{suffix}");
        gh(
            &[
                "release",
                "download",
                &tag,
                "--repo",
                CLI_REPOSITORY,
                "--pattern",
                &asset,
                "--dir",
                &target,
            ],
            None,
        )?;
        let file = dir.path().join(&asset);
        gh(
            &[
                "release",
                "verify-asset",
                &tag,
                &file.to_string_lossy(),
                "--repo",
                CLI_REPOSITORY,
            ],
            None,
        )?;
        let bytes =
            std::fs::read(&file)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: file.display().to_string(),
                    reason: "Failed to read the downloaded CLI archive".to_string(),
                })?;
        checksums.insert(*runner, hex::encode(Sha256::digest(&bytes)));
    }
    Ok(checksums)
}

/// The `ALIEN_CLI_PIN` value: `{"version":…,"sha256":{"<runner>":…}}`.
fn cli_pin(version: &str, checksums: &BTreeMap<&str, String>) -> String {
    serde_json::json!({ "version": version, "sha256": checksums }).to_string()
}

/// `workflow` with the value of its one `ALIEN_CLI_PIN:` line set to `pin`.
fn pin_workflow(workflow: &str, pin: &str) -> Result<String> {
    let key = format!("{CLI_PIN_VARIABLE}:");
    let mut out = String::new();
    let mut pinned = 0;
    for line in workflow.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let indent = body.len() - body.trim_start().len();
        if body.trim_start().starts_with(&key) {
            pinned += 1;
            out.push_str(&format!(
                "{}{key} '{pin}'{}",
                &body[..indent],
                &line[body.len()..]
            ));
        } else {
            out.push_str(line);
        }
    }
    if pinned != 1 {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "{RELEASE_WORKFLOW} has {pinned} `{CLI_PIN_VARIABLE}` lines, not one; it isn't the workflow Connect GitHub opened"
            ),
        }));
    }
    Ok(out)
}

fn gh(args: &[&str], input: Option<&[u8]>) -> Result<String> {
    let mut child = Command::new("gh")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(ErrorData::SigningFailed {
            message: "Could not run `gh`; install the GitHub CLI and run `gh auth login`"
                .to_string(),
        })?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(input)
            .into_alien_error()
            .context(ErrorData::SigningFailed {
                message: "Could not write to `gh`".to_string(),
            })?;
    }
    let output = child
        .wait_with_output()
        .into_alien_error()
        .context(ErrorData::SigningFailed {
            message: "`gh` did not finish".to_string(),
        })?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "`gh {}` failed: {}",
                args.iter().take(2).copied().collect::<Vec<_>>().join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW: &str = "jobs:\n  release:\n    env:\n      ALIEN_CLI_PIN: \"\"\n    steps:\n      - run: alien release\n";

    #[test]
    fn the_pin_line_is_filled_and_nothing_else_changes() {
        let pin = cli_pin(
            "3.4.0",
            &BTreeMap::from([
                ("Linux-X64", "ab".repeat(32)),
                ("macOS-ARM64", "cd".repeat(32)),
            ]),
        );
        let pinned = pin_workflow(WORKFLOW, &pin).unwrap();
        assert_eq!(
            pinned,
            WORKFLOW.replace("ALIEN_CLI_PIN: \"\"", &format!("ALIEN_CLI_PIN: '{pin}'"))
        );
        // The workflow reads it back as YAML, then as JSON.
        let yaml: serde_yaml::Value = serde_yaml::from_str(&pinned).unwrap();
        let value = yaml["jobs"]["release"]["env"]["ALIEN_CLI_PIN"]
            .as_str()
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(value).unwrap();
        assert_eq!(json["version"], "3.4.0");
        assert_eq!(json["sha256"]["Linux-X64"], "ab".repeat(32));
        // Pinning again replaces the old pin.
        assert_eq!(pin_workflow(&pinned, &pin).unwrap(), pinned);

        for workflow in [
            WORKFLOW.replace("      ALIEN_CLI_PIN: \"\"\n", ""),
            WORKFLOW.replace(
                "ALIEN_CLI_PIN: \"\"",
                "ALIEN_CLI_PIN: \"\"\n      ALIEN_CLI_PIN: \"\"",
            ),
        ] {
            pin_workflow(&workflow, &pin)
                .expect_err("a workflow without exactly one pin line is not rewritten");
        }
    }

    /// Against the real public release: every archive passes
    /// `gh release verify-asset` and the checksums match the release's own
    /// checksums file. Needs network and gh 2.67 or later on PATH.
    #[test]
    #[ignore = "needs network and a current gh"]
    fn a_published_release_is_pinned_by_its_attested_archives() {
        let checksums = attested_checksums("3.3.53").unwrap();
        assert_eq!(
            checksums,
            BTreeMap::from([
                (
                    "Linux-X64",
                    "4d2dfc764e311ea653fa38ef95b43dca17cbd151dac03b5ec822841726e204e4".to_string()
                ),
                (
                    "Linux-ARM64",
                    "c096e64e82bf20138e7275d8a2fb1d83c8f2ed19a1a398d4469a3f8d1a253ac4".to_string()
                ),
                (
                    "macOS-ARM64",
                    "0af0f934ff5357b87df34d64d1486371b151530034d193d591a41193b6f844ca".to_string()
                ),
                (
                    "Windows-X64",
                    "6a623917185dc775d2c9df971cea99785f2866c89403ca97758f192894615f36".to_string()
                ),
            ])
        );
        // A version that was never released is refused, not pinned empty.
        attested_checksums("0.0.1").expect_err("no such release");
    }

    /// `GET repos/alienplatform/alien/rulesets/19766651`, recorded
    /// 2026-11-05 (links and timestamps dropped).
    const RECORDED_TAG_RULESET: &str = r#"{
        "id": 19766651,
        "name": "Immutable release tags",
        "target": "tag",
        "enforcement": "active",
        "conditions": {"ref_name": {"exclude": [], "include": ["refs/tags/v*"]}},
        "rules": [{"type": "deletion"}, {"type": "update"}],
        "bypass_actors": []
    }"#;

    #[test]
    fn release_tags_count_only_behind_a_ruleset_that_freezes_them() {
        let recorded: serde_json::Value = serde_json::from_str(RECORDED_TAG_RULESET).unwrap();
        // The recorded ruleset stops moving and deleting tags, not creating
        // them: anyone who can push could create v9.9.9 and deploy.
        assert_eq!(
            tag_protection_gap(&Ok(vec![recorded.clone()])).as_deref(),
            Some("ruleset 'Immutable release tags' doesn't restrict creation")
        );
        let mut frozen = recorded.clone();
        frozen["rules"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "creation"}));
        assert_eq!(tag_protection_gap(&Ok(vec![frozen.clone()])), None);

        let mut app = frozen.clone();
        app["bypass_actors"] = serde_json::json!([
            {"actor_id": 1, "actor_type": "Integration", "bypass_mode": "always"}
        ]);
        assert_eq!(
            tag_protection_gap(&Ok(vec![app])).as_deref(),
            Some("ruleset 'Immutable release tags' lets an app bypass it")
        );
        let mut unseen = frozen.clone();
        unseen.as_object_mut().unwrap().remove("bypass_actors");
        assert!(
            tag_protection_gap(&Ok(vec![unseen])).is_some(),
            "unknown bypass counts"
        );

        for (field, value) in [
            ("enforcement", serde_json::json!("evaluate")),
            ("target", serde_json::json!("branch")),
            (
                "conditions",
                serde_json::json!({"ref_name": {"exclude": [], "include": ["refs/tags/release-*"]}}),
            ),
            (
                "conditions",
                serde_json::json!({"ref_name": {"exclude": ["refs/tags/v0*"], "include": ["refs/tags/v*"]}}),
            ),
        ] {
            let mut other = frozen.clone();
            other[field] = value;
            assert_eq!(
                tag_protection_gap(&Ok(vec![other])).as_deref(),
                Some("no active tag ruleset covers refs/tags/v*"),
                "{field}"
            );
        }
        assert_eq!(
            tag_protection_gap(&Err("its rulesets can't be read (HTTP 403)".to_string()))
                .as_deref(),
            Some("its rulesets can't be read (HTTP 403)")
        );
    }

    /// When GitHub last updated the CI key secret.
    fn secret_updated_at(repository: &str) -> String {
        let text = gh(
            &[
                "api",
                &format!(
                    "repos/{repository}/environments/{GITHUB_ENVIRONMENT}/secrets/{GITHUB_SECRET}"
                ),
            ],
            None,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["updated_at"].as_str().unwrap().to_string()
    }

    /// Against a real GitHub repository (`TEST_GITHUB_REPOSITORY`, with `gh`
    /// logged in as an admin of it, on a plan without rulesets for it): with a
    /// policy letting every branch use the `release` environment, or one for
    /// release tags that no ruleset protects, the CI key is refused and the
    /// secret is not touched; with only `main`, it is stored.
    #[test]
    #[ignore = "needs a GitHub repository and gh"]
    fn the_ci_key_is_stored_only_where_other_refs_cant_read_it() {
        let repository =
            std::env::var("TEST_GITHUB_REPOSITORY").expect("set TEST_GITHUB_REPOSITORY");
        let policies = format!(
            "repos/{repository}/environments/{GITHUB_ENVIRONMENT}/deployment-branch-policies"
        );
        let add = |name: &str, kind: &str| {
            let text = gh(
                &[
                    "api",
                    "-X",
                    "POST",
                    &policies,
                    "-f",
                    &format!("name={name}"),
                    "-f",
                    &format!("type={kind}"),
                ],
                None,
            )
            .unwrap();
            let policy: serde_json::Value = serde_json::from_str(&text).unwrap();
            format!("{policies}/{}", policy["id"])
        };
        // Start from `main` only.
        let text = gh(&["api", &policies], None).unwrap();
        let existing: serde_json::Value = serde_json::from_str(&text).unwrap();
        for policy in existing["branch_policies"].as_array().unwrap() {
            if !(policy["name"] == "main" && policy["type"] == "branch") {
                gh(
                    &[
                        "api",
                        "-X",
                        "DELETE",
                        &format!("{policies}/{}", policy["id"]),
                    ],
                    None,
                )
                .unwrap();
            }
        }
        let before = secret_updated_at(&repository);

        let every_branch = add("*", "branch");
        let refused = store_ci_key(&repository, &SecretKey::generate().unwrap());
        gh(&["api", "-X", "DELETE", &every_branch], None).unwrap();
        let error = refused.expect_err("a key any branch can read must not be stored");
        assert!(
            error.to_string().contains("allows: branch *, branch main"),
            "the refusal names the extra policy: {error}"
        );
        assert_eq!(secret_updated_at(&repository), before);

        let tags = add(RELEASE_TAGS, "tag");
        let refused = store_ci_key(&repository, &SecretKey::generate().unwrap());
        gh(&["api", "-X", "DELETE", &tags], None).unwrap();
        let error = refused.expect_err("a key unprotected tags can read must not be stored");
        assert!(
            error
                .to_string()
                .contains("must allow only branch main, but allows: branch main, tag v*. Tags v* may not use it: its rulesets can't be read"),
            "the refusal says why tags don't count: {error}"
        );
        assert_eq!(secret_updated_at(&repository), before);

        // GitHub reports secret times to the second.
        std::thread::sleep(std::time::Duration::from_secs(2));
        let who_can = store_ci_key(&repository, &SecretKey::generate().unwrap()).unwrap();
        assert_eq!(who_can, "only main can use");
        assert_ne!(secret_updated_at(&repository), before);
        assert_eq!(
            branch_policies(&policies).unwrap(),
            [("main".to_string(), "branch".to_string())]
        );
    }

    fn signer(id: &str, key_seed: u8, roles: Vec<Role>) -> Signer {
        Signer {
            id: id.to_string(),
            key: SecretKey::from_secret_bytes(&[key_seed; 32])
                .unwrap()
                .public_key()
                .as_str()
                .to_string(),
            kind: SignerKind::Device,
            backend: None,
            roles,
            label: Some(format!("{id}@example.com")),
        }
    }

    #[test]
    fn the_last_admin_device_is_not_removed_while_only_the_recovery_key_would_be_left() {
        let mut recovery = signer(RECOVERY_SIGNER, 9, vec![Role::Admin]);
        recovery.kind = SignerKind::Recovery;
        let alice = signer("alice", 1, vec![Role::Admin, Role::Release]);
        let ci = signer("github-acme-app", 2, vec![Role::Release]);
        let list = body(vec![alice.clone(), ci.clone(), recovery.clone()]);
        let error = without_signer(&list, "alice").expect_err("alice is the last admin device");
        assert!(
            error.message.contains("'alice' is the last admin device"),
            "{}",
            error.message
        );
        assert_commands_exist(&error.message);

        // With another admin device, alice may go; the recovery key stays.
        let bob = signer("bob", 3, vec![Role::Admin]);
        let mut list = body(vec![alice, bob.clone(), ci.clone(), recovery.clone()]);
        list.admin_threshold = 2;
        let next = without_signer(&list, "alice").unwrap();
        assert_eq!(next.signers, vec![bob, ci, recovery]);
        assert_eq!(
            next.admin_threshold, 2,
            "bob and the recovery key still make two"
        );
    }

    /// Every `` `alien ...` `` command `message` names is a command of the CLI.
    fn assert_commands_exist(message: &str) {
        let commands: Vec<&str> = message
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|quoted| quoted.starts_with("alien "))
            .collect();
        assert!(!commands.is_empty(), "the hint names a command: {message}");
        for quoted in commands {
            let mut command = <crate::Cli as clap::CommandFactory>::command();
            for word in quoted.split_whitespace().skip(1) {
                command = command
                    .find_subcommand(word)
                    .unwrap_or_else(|| panic!("`{quoted}` is not a command of the CLI"))
                    .clone();
            }
        }
    }

    fn body(signers: Vec<Signer>) -> SignerListBody {
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers,
            deny: Vec::new(),
            keep: Vec::new(),
            recovered: false,
        }
    }

    #[test]
    fn every_changed_field_of_a_signer_is_shown() {
        let mut recovery = signer("recovery", 2, vec![Role::Admin]);
        recovery.kind = SignerKind::Recovery;
        recovery.label = None;
        let current = body(vec![
            signer("alice", 1, vec![Role::Admin, Role::Release]),
            recovery.clone(),
        ]);

        // Unchanged: only the refresh line.
        assert_eq!(diff(&current, &current).len(), 1);

        // A prepared refresh that quietly makes the recovery key a device.
        let mut next = current.clone();
        next.signers[1].kind = SignerKind::Device;
        assert_eq!(
            diff(&current, &next),
            ["~ recovery: kind recovery → device"]
        );

        // Roles and label changed together: two lines, neither hidden.
        let mut next = current.clone();
        next.signers[0].roles = vec![Role::Release];
        next.signers[0].label = Some("mallory@example.com".to_string());
        next.signers[0].id = "mallory".to_string();
        assert_eq!(
            diff(&current, &next),
            [
                "~ alice (alice@example.com): id alice → mallory",
                "~ alice (alice@example.com): label alice@example.com → mallory@example.com",
                "~ alice (alice@example.com): roles manage devices, release → release",
            ]
        );
    }

    fn statement(kind: Kind, project: &str, sequence: u64, by: &[(&str, u8)]) -> SignedStatement {
        let mut statement = SignedStatement::new(
            &alien_signing::Envelope {
                kind: kind.as_str().to_string(),
                project: project.to_string(),
                install: (kind == Kind::Config)
                    .then(|| "aws:111122223333:us-east-1:acme".to_string()),
                sequence,
                not_after: None,
                signer_list_version: 1,
                body: serde_json::json!({}),
            }
            .to_payload()
            .unwrap(),
        );
        for (id, seed) in by {
            statement = statement
                .sign_with(id, &SecretKey::from_secret_bytes(&[*seed; 32]).unwrap())
                .unwrap();
        }
        statement
    }

    const PROJECT: &str = "prj";

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_secret_bytes(&[seed; 32]).unwrap()
    }

    /// A chain whose version 1 trusts `alice` (admin, release) and `ci` (release).
    fn chain() -> SignerChain {
        let signer = |id: &str, seed: u8, kind, roles: Vec<Role>| Signer {
            id: id.to_string(),
            key: key(seed).public_key().as_str().to_string(),
            kind,
            backend: None,
            roles,
            label: None,
        };
        let body = SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![
                signer(
                    "alice",
                    1,
                    SignerKind::Device,
                    vec![Role::Admin, Role::Release],
                ),
                signer("ci", 7, SignerKind::Ci, vec![Role::Release]),
            ],
            deny: Vec::new(),
            keep: Vec::new(),
            recovered: false,
        };
        let statement = SignedStatement::new(
            &alien_signing::Envelope {
                kind: Kind::SignerList.as_str().to_string(),
                project: PROJECT.to_string(),
                install: None,
                sequence: 1,
                not_after: None,
                signer_list_version: 1,
                body,
            }
            .to_payload()
            .unwrap(),
        )
        .sign_with("alice", &key(1))
        .unwrap();
        let digest = statement.open().unwrap().digest;
        SignerChain::genesis(&alien_signing::fingerprint(&digest), &statement).unwrap()
    }

    /// A release statement of `platform` signed as `sequence` by `signer`.
    fn release_statement(
        platform: &str,
        channel: &str,
        sequence: u64,
        signer: (&str, u8),
    ) -> SignedStatement {
        SignedStatement::new(
            &alien_signing::Envelope {
                kind: Kind::Release.as_str().to_string(),
                project: PROJECT.to_string(),
                install: None,
                sequence,
                not_after: None,
                signer_list_version: 1,
                body: serde_json::json!({
                    "platform": platform, "channel": channel, "workloads": []
                }),
            }
            .to_payload()
            .unwrap(),
        )
        .sign_with(signer.0, &key(signer.1))
        .unwrap()
    }

    fn release_json(
        id: &str,
        current_on: &[&str],
        statements: &[(&str, SignedStatement)],
    ) -> serde_json::Value {
        let stacks: serde_json::Map<String, serde_json::Value> = statements
            .iter()
            .map(|(platform, statement)| {
                (
                    platform.to_string(),
                    serde_json::to_value(statement).unwrap(),
                )
            })
            .collect();
        serde_json::json!({"id": id, "currentChannels": current_on, "signedStacks": stacks})
    }

    #[test]
    fn what_a_key_signed_that_is_current_is_found_and_nothing_else() {
        let old = key(7).public_key();
        let live = release_statement("aws", "production", 1_700_000_000_000, ("ci", 7));
        let live_gcp = release_statement("gcp", "production", 1_700_000_000_000, ("ci", 7));
        let stale = release_statement("aws", "production", 1_600_000_000_000, ("ci", 7));
        // Under the CI id, but another key: a server can't get it kept.
        let forged = release_statement("aws", "production", 1_700_000_000_001, ("ci", 8));
        let releases = vec![
            release_json("rel_live", &[], &[("aws", live.clone()), ("gcp", live_gcp)]),
            release_json("rel_stale", &[], &[("aws", stale)]),
            release_json("rel_forged", &["production"], &[("aws", forged)]),
        ];
        let channels =
            vec![serde_json::json!({"name": "production", "currentReleaseId": "rel_live"})];
        let config = |sequence: u64, by: (&str, u8)| {
            SignedStatement::new(
                &alien_signing::Envelope {
                    kind: Kind::Config.as_str().to_string(),
                    project: PROJECT.to_string(),
                    install: Some("aws:111122223333:us-east-1:acme".to_string()),
                    sequence,
                    not_after: None,
                    signer_list_version: 1,
                    body: serde_json::json!({}),
                }
                .to_payload()
                .unwrap(),
            )
            .sign_with(by.0, &key(by.1))
            .unwrap()
        };
        let newest = config(20, ("ci", 7));
        let deployments = vec![
            (
                serde_json::json!({"id": "dep_a", "currentReleaseId": "rel_live"}),
                // Only the newest config is current.
                vec![config(10, ("ci", 7)), newest.clone()],
            ),
            // Its newest config is by a current signer: nothing to keep.
            (
                serde_json::json!({"id": "dep_b", "currentReleaseId": "rel_live"}),
                vec![config(10, ("ci", 7)), config(30, ("alice", 1))],
            ),
        ];
        let found = current_signed_by(&releases, &channels, &deployments, PROJECT, "ci", &old);
        let digests: Vec<_> = found.iter().map(|item| item.keep.digest.clone()).collect();
        assert_eq!(found.len(), 3, "{found:#?}");
        assert!(digests.contains(&live.open().unwrap().digest));
        assert!(digests.contains(&newest.open().unwrap().digest));
        let release = found
            .iter()
            .find(|item| item.platform.as_deref() == Some("aws"))
            .unwrap();
        assert_eq!(release.keep.release_id.as_deref(), Some("rel_live"));
        assert_eq!(release.keep.sequence, 1_700_000_000_000);
        assert_eq!(release.channels, ["production"]);
        assert_eq!(release.deployments, ["dep_a", "dep_b"]);
        // Printed text leads with the signed facts and names the id as the control plane's.
        let text = found_text(release);
        assert!(
            text.starts_with("release for aws signed as 1700000000000 (2023-11-14 22:13 UTC)"),
            "{text}"
        );
        assert!(
            text.contains("called rel_live by the control plane"),
            "{text}"
        );
    }

    #[test]
    fn revoke_names_one_signed_release_and_prints_what_it_signed() {
        let chain = chain();
        let aws = release_statement("aws", "production", 1_700_000_000_000, ("alice", 1));
        let gcp = release_statement("gcp", "production", 1_700_000_000_000, ("alice", 1));
        let releases = vec![release_json(
            "rel_old",
            &[],
            &[("aws", aws.clone()), ("gcp", gcp.clone())],
        )];
        for (id, sequence) in [(Some("rel_old"), None), (None, Some(1_700_000_000_000))] {
            let target = revoke_target(&chain, &releases, &[], id, sequence, false).unwrap();
            assert_eq!(target.len(), 2);
            let lines = revoke_lines(&target).join("\n");
            assert!(
                lines.starts_with(
                    "Release signed as 1700000000000 (2023-11-14 22:13 UTC), channel production:"
                ),
                "{lines}"
            );
            assert!(lines.contains(&aws.open().unwrap().digest), "{lines}");
            assert!(
                lines.ends_with("called rel_old by the control plane"),
                "{lines}"
            );
        }
    }

    #[test]
    fn revoke_refuses_statements_that_are_not_the_release_asked_for() {
        let chain = chain();
        // The control plane answers for rel_old with the statements of two releases.
        let old = release_statement("aws", "production", 1_700_000_000_000, ("alice", 1));
        let live = release_statement("gcp", "production", 1_700_000_000_999, ("alice", 1));
        let mixed = vec![release_json("rel_old", &[], &[("aws", old), ("gcp", live)])];
        let error = revoke_target(&chain, &mixed, &[], Some("rel_old"), None, false).unwrap_err();
        assert!(
            error.message.contains("not one signed release"),
            "{}",
            error.message
        );

        // Different channels under one sequence are refused too.
        let a = release_statement("aws", "production", 5, ("alice", 1));
        let b = release_statement("gcp", "dev", 5, ("alice", 1));
        let mixed = vec![release_json("rel_x", &[], &[("aws", a), ("gcp", b)])];
        assert!(revoke_target(&chain, &mixed, &[], Some("rel_x"), None, false).is_err());

        // A statement that is not signed by the list is an error, not a deny entry.
        let forged = release_statement("aws", "production", 6, ("alice", 2));
        let releases = vec![release_json("rel_f", &[], &[("aws", forged)])];
        let error = revoke_target(&chain, &releases, &[], Some("rel_f"), None, false).unwrap_err();
        assert!(
            error.message.contains("doesn't check out"),
            "{}",
            error.message
        );
    }

    #[test]
    fn revoke_by_sequence_needs_no_statement_from_the_control_plane() {
        let chain = chain();
        let target = revoke_target(&chain, &[], &[], None, Some(1_700_000_000_000), false).unwrap();
        assert!(target.is_empty());
    }

    #[test]
    fn revoke_refuses_a_release_that_is_current_under_any_name() {
        let chain = chain();
        let one = release_statement("aws", "production", 1_700_000_000_000, ("alice", 1));
        // Another release the control plane lists carries the same signed sequence and is
        // current on a channel: revoking the first by id or by sequence refuses.
        let releases = vec![
            release_json("rel_old", &[], &[("aws", one.clone())]),
            release_json("rel_live", &[], &[("aws", one)]),
        ];
        let channels =
            vec![serde_json::json!({"name": "production", "currentReleaseId": "rel_live"})];
        for (id, sequence) in [(Some("rel_old"), None), (None, Some(1_700_000_000_000))] {
            let error =
                revoke_target(&chain, &releases, &channels, id, sequence, false).unwrap_err();
            assert!(
                error.message.contains("current on production"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn force_revokes_a_release_the_control_plane_says_is_current() {
        let chain = chain();
        let one = release_statement("aws", "production", 1_700_000_000_000, ("alice", 1));
        let releases = vec![release_json("rel_live", &[], &[("aws", one)])];
        let channels =
            vec![serde_json::json!({"name": "production", "currentReleaseId": "rel_live"})];
        let refused =
            revoke_target(&chain, &releases, &channels, Some("rel_live"), None, false).unwrap_err();
        assert!(refused.message.contains("--force"), "{}", refused.message);
        let target =
            revoke_target(&chain, &releases, &channels, Some("rel_live"), None, true).unwrap();
        assert_eq!(target.len(), 1);
    }

    #[test]
    fn a_leaked_key_is_revoked_a_kept_one_is_not_and_nothing_given_is_asked_or_refused() {
        let never = || -> String { panic!("must not ask") };
        // Flags decide, with or without a terminal.
        assert!(resolve_intent(true, false, "key", false, never).unwrap());
        assert!(!resolve_intent(false, true, "key", false, never).unwrap());
        assert!(resolve_intent(true, true, "key", true, never).is_err());
        // No terminal and no flag: refuse, and say what to pass.
        let error = resolve_intent(false, false, "key", false, never).unwrap_err();
        assert!(error.message.contains("--revoke") && error.message.contains("--keep-running"));
        // A terminal is asked.
        assert!(resolve_intent(false, false, "key", true, || "leaked\n".into()).unwrap());
        assert!(!resolve_intent(false, false, "key", true, || "no\n".into()).unwrap());
        assert!(resolve_intent(false, false, "key", true, || "maybe\n".into()).is_err());
    }

    /// Revoking reads every release and deployment, so it follows the server's
    /// pages to the end instead of stopping at the first.
    #[tokio::test]
    async fn every_page_of_a_list_is_read() {
        let server = httpmock::MockServer::start_async().await;
        // The page after `c2` is registered first, so a request with that cursor never
        // matches the first page.
        let second = server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/v1/releases")
                    .query_param("limit", "100")
                    .query_param("cursor", "c2");
                then.status(200)
                    .json_body(serde_json::json!({"items": [{"id": "rel_3"}], "nextCursor": null}));
            })
            .await;
        let first = server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/v1/releases")
                    .query_param("limit", "100");
                then.status(200)
                    .json_body(serde_json::json!({"items": [{"id": "rel_1"}, {"id": "rel_2"}], "nextCursor": "c2"}));
            })
            .await;
        let ctx = ExecutionMode::Standalone {
            server_url: server.base_url(),
            api_key: "test".to_string(),
        };
        let items = all_items(&ctx, "/v1/releases", &[]).await.unwrap();
        let ids: Vec<&str> = items
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        assert_eq!(ids, ["rel_1", "rel_2", "rel_3"]);
        first.assert_hits_async(1).await;
        second.assert_hits_async(1).await;
    }

    #[test]
    fn names_for_people_and_ci() {
        assert_eq!(first_name(&signer("bob", 1, vec![Role::Release])), "Bob");
        assert_eq!(ci_id("Acme/App.Server"), "github-acme-app-server");
        check_repository("acme/app").unwrap();
        check_repository("acme").expect_err("no repo");
        check_repository("acme/app/x").expect_err("too many parts");
        check_repository("acme/a b").expect_err("space");
    }
}
