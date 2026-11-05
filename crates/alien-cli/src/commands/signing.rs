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
use alien_signing::{
    display_fingerprint, Kind, PublicKey, RecoveryCode, Role, SecretKey, SignedStatement, Signer,
    SignerChain, SignerKind, SignerListBody,
};
use chrono::Utc;
use clap::{Args, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json};
use crate::signing::api::{self, ListPublished, PendingChange, SigningApi};
use crate::signing::release::{added_signers, RECOVERY_SIGNER};
use crate::signing::{
    kind_name, next_list, pairing_code, same_code, sign_envelope, signer_with_key, verified_chain,
    RepoSigning, SigningHome, StatementSigner, LIST_WARNING_DAYS, REPO_SIGNING_FILE,
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
    /// Show the trusted devices, the recovery key and the fingerprint
    List,
    /// Stop trusting a device, such as a lost laptop
    Remove {
        /// The device's name, as `alien devices list` shows it.
        name: String,
    },
    /// Trust GitHub Actions in a repository to sign releases
    AddCi {
        /// The repository, as OWNER/REPO.
        repository: String,
    },
    /// Replace the CI key, for example after it leaked
    RotateCi {
        /// Also refuse the recent releases the old key signed.
        #[arg(long)]
        revoke: bool,
        /// The repository, when more than one CI key is trusted.
        repository: Option<String>,
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
                "This repo has no {REPO_SIGNING_FILE}. The first `alien release` from a laptop turns signing on and writes it; if a teammate did, pull it."
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
            message: "This device has no device key. Ask an admin, or use `alien devices recover` if every admin device is lost.".to_string(),
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
            .extend(std::slice::from_ref(&statement), Utc::now())
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
        return refresh_if_due(&admin).await;
    }
    for change in changes {
        approve_one(&mut admin, &change).await?;
    }
    Ok(())
}

/// With nothing waiting, refresh a list that expires within
/// [`LIST_WARNING_DAYS`]: the same devices under a new expiry.
async fn refresh_if_due(admin: &Admin) -> Result<()> {
    let list = admin.chain.current();
    let left = (list.not_after - Utc::now()).num_days();
    if left >= LIST_WARNING_DAYS {
        println!("No waiting requests for {}.", admin.chain.project());
        return Ok(());
    }
    println!(
        "The device list for {} expires on {}; refreshing it with the same devices.",
        admin.chain.project(),
        list.not_after.format("%Y-%m-%d")
    );
    let mut body = list.body.clone();
    body.recovered = false;
    let statement = admin.sign_next(body, "refresh")?;
    admin.publish(&statement).await?;
    println!(
        "✓ Device list refreshed · delivered to protected deployments as the next device list"
    );
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
                    "it adds a {} key ({}); CI keys are added with `alien devices add-ci`",
                    kind_name(signer.kind),
                    signer.id
                ),
            ));
        }
    }
    match added.as_slice() {
        [signer] if diff(&list.body, &body).len() == 1 => {
            println!("{} asks to: {}", who(signer), roles_text(&signer.roles))
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
        .extend(std::slice::from_ref(&statement), Utc::now())
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
            who(signer),
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
        if other.roles != signer.roles {
            lines.push(format!(
                "~ {}: roles {} → {}",
                who(signer),
                roles_text(&signer.roles),
                roles_text(&other.roles)
            ));
        }
    }
    for digest in next
        .deny
        .iter()
        .filter(|digest| !current.deny.contains(digest))
    {
        lines.push(format!("+ refuse statement {digest}"));
    }
    for digest in current
        .deny
        .iter()
        .filter(|digest| !next.deny.contains(digest))
    {
        lines.push(format!("- stop refusing statement {digest}"));
    }
    if next.admin_threshold != current.admin_threshold {
        lines.push(format!(
            "~ admin signatures needed: {} → {}",
            current.admin_threshold, next.admin_threshold
        ));
    }
    if lines.is_empty() {
        lines.push(format!(
            "no device changes; the list stays valid for {} more days",
            crate::signing::LIST_LIFETIME_DAYS
        ));
    }
    lines
}

/// The first characters of a public key, enough to tell keys apart.
fn short_key(key: &str) -> String {
    let key = key
        .strip_prefix(alien_signing::PUBLIC_KEY_PREFIX)
        .unwrap_or(key);
    key.chars().take(12).collect()
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
            Role::Operate => "operate",
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

pub async fn devices_task(args: DevicesArgs, ctx: ExecutionMode) -> Result<()> {
    match args.command.unwrap_or(DevicesCommand::List) {
        DevicesCommand::List => list(&ctx, args.json).await,
        DevicesCommand::Remove { name } => remove(&ctx, &name).await,
        DevicesCommand::AddCi { repository } => add_ci(&ctx, &repository).await,
        DevicesCommand::RotateCi { revoke, repository } => {
            rotate_ci(&ctx, revoke, repository.as_deref()).await
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
    expires_at: String,
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
            expires_at: list.not_after.to_rfc3339(),
            recovered: list.body.recovered,
            devices: list
                .body
                .signers
                .iter()
                .map(|signer| DeviceJson {
                    id: &signer.id,
                    label: signer.label.as_deref(),
                    kind: kind_name(signer.kind),
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
    let mut table = crate::ui::make_table(&["Device", "Kind", "Can", "Label"]);
    for signer in &list.body.signers {
        table.add_row(vec![
            signer.id.clone(),
            kind_name(signer.kind).to_string(),
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
        "Device list  v{} · expires {}{}",
        list.version,
        list.not_after.format("%Y-%m-%d"),
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

async fn remove(ctx: &ExecutionMode, name: &str) -> Result<()> {
    let admin = admin(ctx).await?;
    let list = admin.chain.current();
    let mut body = list.body.clone();
    let before = body.signers.len();
    body.signers.retain(|signer| signer.id != name);
    if body.signers.len() == before {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("No trusted device is named '{name}'; see `alien devices list`"),
        }));
    }
    let admins = body
        .signers
        .iter()
        .filter(|signer| signer.roles.contains(&Role::Admin))
        .count();
    if admins == 0 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("'{name}' is the last admin; nobody could change the devices after"),
        }));
    }
    body.admin_threshold = body.admin_threshold.min(admins as u32);
    body.recovered = false;
    println!(
        "Removing {name} from the devices of {}.",
        admin.chain.project()
    );
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    println!(
        "✓ {name} can no longer sign · delivered to protected deployments as the next device list"
    );
    Ok(())
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
    let list = admin.chain.current();
    if list.signer(&id).is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "repository".to_string(),
            message: format!(
                "{repository} already has a CI key; replace it with `alien devices rotate-ci`"
            ),
        }));
    }
    // The CLI pin is checked before anything changes, so a release that
    // doesn't verify leaves no key stored and no list published.
    pin_cli(&current_dir()?)?;
    let key = SecretKey::generate().context(ErrorData::SigningFailed {
        message: "Could not create the CI key".to_string(),
    })?;
    let who_can = store_ci_key(repository, &key)?;
    let mut body = list.body.clone();
    body.signers.push(Signer {
        id: id.clone(),
        key: key.public_key().as_str().to_string(),
        kind: SignerKind::Ci,
        roles: vec![Role::Release],
        label: Some(ci_label(repository)),
    });
    body.recovered = false;
    println!(
        "Trusting GitHub Actions in {repository} to sign releases of {}.",
        admin.chain.project()
    );
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    println!(
        "✓ {} can release · its key is in the `{GITHUB_ENVIRONMENT}` environment, which {who_can}",
        ci_label(repository)
    );
    Ok(())
}

async fn rotate_ci(ctx: &ExecutionMode, revoke: bool, repository: Option<&str>) -> Result<()> {
    let admin = admin(ctx).await?;
    let list = admin.chain.current();
    let ci: Vec<&Signer> = list
        .body
        .signers
        .iter()
        .filter(|signer| signer.kind == SignerKind::Ci)
        .filter(|signer| repository.is_none_or(|repository| signer.id == ci_id(repository)))
        .collect();
    let old = match ci.as_slice() {
        [signer] => (*signer).clone(),
        [] => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "repository".to_string(),
                message: "No CI key to replace; add one with `alien devices add-ci OWNER/REPO`"
                    .to_string(),
            }))
        }
        _ => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "repository".to_string(),
                message: "More than one CI key is trusted; name the repository".to_string(),
            }))
        }
    };
    let repository = old
        .label
        .as_deref()
        .and_then(|label| label.strip_prefix("GitHub Actions ("))
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::SigningFailed {
                message: format!("CI key '{}' doesn't name its GitHub repository", old.id),
            })
        })?;
    let old_key = PublicKey::parse(&old.key).context(ErrorData::SigningFailed {
        message: format!("CI key '{}' on the list doesn't parse", old.id),
    })?;
    let deny = if revoke {
        signed_by(ctx, &admin.api, admin.chain.project(), &old.id, &old_key).await?
    } else {
        Vec::new()
    };
    pin_cli(&current_dir()?)?;
    let key = SecretKey::generate().context(ErrorData::SigningFailed {
        message: "Could not create the CI key".to_string(),
    })?;
    let who_can = store_ci_key(&repository, &key)?;
    let mut body = list.body.clone();
    for signer in body.signers.iter_mut().filter(|signer| signer.id == old.id) {
        signer.key = key.public_key().as_str().to_string();
    }
    for digest in &deny {
        if !body.deny.contains(digest) {
            body.deny.push(digest.clone());
        }
    }
    body.recovered = false;
    println!("Replacing the CI key of {repository}.");
    if revoke {
        println!(
            "  Protected deployments will refuse the {} release and config statement(s) the old key signed.",
            deny.len()
        );
    }
    let statement = admin.sign_next(body, "sign")?;
    admin.publish(&statement).await?;
    println!("✓ New CI key in the `{GITHUB_ENVIRONMENT}` environment of {repository}, which {who_can}; the old key can't sign");
    Ok(())
}

/// Digests of the release and config statements the server holds that
/// `key` signed as `signer`: every release of the project and every config
/// statement of its deployments. Only signatures that verify count, so a
/// server can't get other statements refused this way.
async fn signed_by(
    ctx: &ExecutionMode,
    api: &SigningApi,
    project: &str,
    signer: &str,
    key: &PublicKey,
) -> Result<Vec<String>> {
    let query = api::project_query(ctx, project).await?;
    let releases = whole_page(
        api::get_json(ctx, "/v1/releases", &query).await?,
        "releases",
    )?;
    let mut statements = Vec::new();
    for release in releases["items"].as_array().into_iter().flatten() {
        for (_, statement) in release["signedStacks"].as_object().into_iter().flatten() {
            if let Ok(statement) = serde_json::from_value::<SignedStatement>(statement.clone()) {
                statements.push(statement);
            }
        }
    }
    let deployments = whole_page(
        api::get_json(ctx, "/v1/deployments", &query).await?,
        "deployments",
    )?;
    for deployment in deployments["items"].as_array().into_iter().flatten() {
        let id = deployment["id"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::SigningFailed {
                message: "The server listed a deployment without an id".to_string(),
            })
        })?;
        statements.extend(api.config_statements(id).await?);
    }
    Ok(digests_signed_by(&statements, project, signer, key))
}

/// `page` if it holds every item: revoking from part of a list would leave
/// the rest accepted.
fn whole_page(page: serde_json::Value, what: &str) -> Result<serde_json::Value> {
    if page["nextCursor"].is_null() {
        Ok(page)
    } else {
        Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "The server returned the project's {what} in pages; revoking needs all of them, so nothing was changed"
            ),
        }))
    }
}

/// Digests of the release and config statements in `statements` that `key`
/// signed as `signer`, each once.
fn digests_signed_by(
    statements: &[SignedStatement],
    project: &str,
    signer: &str,
    key: &PublicKey,
) -> Vec<String> {
    let mut digests = Vec::new();
    for statement in statements {
        let Ok(opened) = statement.open() else {
            continue;
        };
        let ours = matches!(opened.kind, Kind::Release | Kind::Config)
            && opened.project == project
            && opened.signatures.iter().any(|signature| {
                signature.signer == signer && key.verify(&opened.bytes, &signature.sig).is_ok()
            });
        if ours && !digests.contains(&opened.digest) {
            digests.push(opened.digest);
        }
    }
    digests
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
    let (record, device) = home.device_or_create()?.ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: "This device has no hardware key to become the new admin".to_string(),
        })
    })?;
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
                roles: vec![Role::Release, Role::Operate, Role::Admin],
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
        .extend(std::slice::from_ref(&statement), Utc::now())
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
                "  No {RELEASE_WORKFLOW} here yet. Merge the Connect GitHub pull request, then run `alien devices rotate-ci` from the repo to pin this CLI."
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
            roles,
            label: Some(format!("{id}@example.com")),
        }
    }

    fn body(signers: Vec<Signer>) -> SignerListBody {
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers,
            deny: Vec::new(),
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

    #[test]
    fn revoking_a_ci_key_refuses_its_releases_and_configs_only() {
        let old = SecretKey::from_secret_bytes(&[7; 32]).unwrap().public_key();
        let digest = |statement: &SignedStatement| statement.open().unwrap().digest;
        let release = statement(Kind::Release, "prj", 1, &[("ci", 7)]);
        let config = statement(Kind::Config, "prj", 2, &[("ci", 7), ("alice", 1)]);
        let statements = vec![
            release.clone(),
            config.clone(),
            release.clone(),
            // Under the CI id, but another key: a server can't get it refused.
            statement(Kind::Release, "prj", 3, &[("ci", 8)]),
            // Signed by the old key for another project.
            statement(Kind::Release, "other", 4, &[("ci", 7)]),
            // Not a release or config.
            statement(Kind::SignerList, "prj", 5, &[("ci", 7)]),
            statement(Kind::Config, "prj", 6, &[("alice", 1)]),
        ];
        assert_eq!(
            digests_signed_by(&statements, "prj", "ci", &old),
            [digest(&release), digest(&config)]
        );
        assert!(whole_page(
            serde_json::json!({"items": [], "nextCursor": null}),
            "deployments"
        )
        .is_ok());
        whole_page(
            serde_json::json!({"items": [], "nextCursor": "c2"}),
            "deployments",
        )
        .expect_err("a partial list is refused");
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
