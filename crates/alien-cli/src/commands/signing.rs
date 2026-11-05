//! `alien signing`, `alien signers` and `alien approve`: the developer's
//! signer list and the changes that wait for a signature.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{
    fingerprint, Envelope, Kind, Role, SignedStatement, Signer, SignerChain, SignerKind,
    SignerListBody, SoftwareKey,
};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json};
use crate::signing::api::{NewPendingChange, PendingChange, PendingKind, Resolution, SigningApi};
use crate::signing::device::{approval_code, write_private, Backend};
use crate::signing::{
    check_signer, default_device_id, kms::KmsKey, sign_envelope, verified_chain, SignerArgs,
    SigningHome, StatementSigner,
};

/// GitHub environment the CI key is stored in.
const GITHUB_ENVIRONMENT: &str = "release";
/// Name of the CI key secret in that environment.
const GITHUB_SECRET: &str = "ALIEN_SIGNING_KEY";

#[derive(Args, Debug, Clone)]
#[command(about = "Set up release signing for the project")]
pub struct SigningArgs {
    #[command(subcommand)]
    pub command: SigningCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SigningCommand {
    /// Create the project's signer list: this device, a CI key and a recovery key
    Init(InitArgs),
    /// Create this machine's device key and ask an admin to approve it
    Device(DeviceArgs),
    /// Show the pinned fingerprint and the verified signer list
    Status(StatusArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Store the CI key as the `ALIEN_SIGNING_KEY` secret of the `release`
    /// environment of this GitHub repository (owner/repo), using your own
    /// `gh` login. The environment is limited to the `main` branch.
    #[arg(long, value_name = "OWNER/REPO")]
    pub github: Option<String>,

    /// Use this existing AWS KMS key (ECC_NIST_P256, SIGN_VERIFY) as the CI key.
    #[arg(long, conflicts_with_all = ["github", "create_kms_key", "ci_key_out"])]
    pub kms_key: Option<String>,

    /// Create a new AWS KMS key for CI with the AWS credentials in the environment.
    #[arg(long, conflicts_with_all = ["github", "ci_key_out"])]
    pub create_kms_key: bool,

    /// Write the CI key to this new file (PKCS#8 PEM) instead of GitHub or KMS.
    #[arg(long)]
    pub ci_key_out: Option<PathBuf>,

    /// Write the recovery key to this new file. Keep it offline, then delete it here.
    #[arg(long, required_unless_present = "recovery_public_key")]
    pub recovery_key_out: Option<PathBuf>,

    /// Use a recovery key made elsewhere, as `ecdsa-p256:<base64 SPKI>`.
    #[arg(long, conflicts_with = "recovery_key_out")]
    pub recovery_public_key: Option<String>,

    /// Admin signatures each later signer list version needs.
    #[arg(long, default_value_t = 1)]
    pub admin_threshold: u32,

    /// Device key backend, if this machine has no device key yet.
    #[arg(long, value_enum)]
    pub backend: Option<BackendArg>,

    /// Project to set up (defaults to the linked project).
    #[arg(long)]
    pub project: Option<String>,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DeviceArgs {
    /// Signer id to ask for (defaults to `<user>-<host>`).
    #[arg(long)]
    pub id: Option<String>,

    /// Label admins see, such as your email.
    #[arg(long)]
    pub label: Option<String>,

    /// Device key backend (defaults to the first available).
    #[arg(long, value_enum)]
    pub backend: Option<BackendArg>,

    /// Project to ask for approval on (defaults to the linked project).
    #[arg(long)]
    pub project: Option<String>,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Project (defaults to the linked project).
    #[arg(long)]
    pub project: Option<String>,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum BackendArg {
    SecureEnclave,
    Tpm,
    Yubikey,
    Software,
}

impl From<BackendArg> for Backend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::SecureEnclave => Backend::SecureEnclave,
            BackendArg::Tpm => Backend::Tpm,
            BackendArg::Yubikey => Backend::Yubikey,
            BackendArg::Software => Backend::Software,
        }
    }
}

#[derive(Args, Debug, Clone)]
#[command(about = "Approve, remove and rotate the keys that may sign")]
pub struct SignersArgs {
    #[command(subcommand)]
    pub command: SignersCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SignersCommand {
    /// List the verified signer list and pending devices
    List(ListArgs),
    /// Add a pending device to the signer list
    Approve(ApproveDeviceArgs),
    /// Remove a signer
    Remove(RemoveArgs),
    /// Replace the CI key
    RotateCi(RotateCiArgs),
    /// Add another CI key, for example for a second CI system
    AddCi(RotateCiArgs),
    /// Pin the project's signing fingerprint on this machine
    Trust(TrustArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ApproveDeviceArgs {
    /// Signer id of the pending device, or the pending change id.
    pub id: String,

    /// The code the new device printed. Without it, you are asked to compare.
    #[arg(long)]
    pub code: Option<String>,

    /// Roles to give, comma-separated: release, operate, runtime, admin.
    #[arg(long, value_delimiter = ',', default_value = "operate")]
    pub roles: Vec<String>,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub project: Option<String>,

    /// Don't ask for confirmation.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RemoveArgs {
    /// Signer id to remove.
    pub id: String,

    /// Statement digests to add to the deny-list, such as the last releases
    /// a leaked key signed.
    #[arg(long = "deny")]
    pub deny: Vec<String>,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RotateCiArgs {
    /// Signer id of the CI key.
    #[arg(long, default_value = "ci")]
    pub id: String,

    /// Store the new key in this GitHub repository's `release` environment.
    #[arg(long, value_name = "OWNER/REPO")]
    pub github: Option<String>,

    /// Use this AWS KMS key.
    #[arg(long = "ci-kms-key", conflicts_with_all = ["github", "ci_key_out"])]
    pub ci_kms_key: Option<String>,

    /// Write the new key to this new file.
    #[arg(long)]
    pub ci_key_out: Option<PathBuf>,

    /// Statement digests to deny, such as releases a leaked key signed.
    #[arg(long = "deny")]
    pub deny: Vec<String>,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TrustArgs {
    /// `SHA256:<hex>` fingerprint from `alien signing init`.
    pub fingerprint: String,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Args, Debug, Clone)]
#[command(about = "Review and sign a change prepared in the dashboard")]
pub struct ApproveArgs {
    /// Pending change id.
    pub id: String,

    #[command(flatten)]
    pub signer: SignerArgs,

    /// Don't ask for confirmation.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitOutput {
    project: String,
    fingerprint: String,
    device: String,
    ci_key: String,
}

pub async fn signing_task(args: SigningArgs, ctx: ExecutionMode) -> Result<()> {
    match args.command {
        SigningCommand::Init(args) => init(args, ctx).await,
        SigningCommand::Device(args) => device(args, ctx).await,
        SigningCommand::Status(args) => status(args.project, args.json, ctx).await,
    }
}

pub async fn signers_task(args: SignersArgs, ctx: ExecutionMode) -> Result<()> {
    match args.command {
        SignersCommand::List(args) => status(args.project, args.json, ctx).await,
        SignersCommand::Approve(args) => approve_device(args, ctx).await,
        SignersCommand::Remove(args) => remove(args, ctx).await,
        SignersCommand::RotateCi(args) => rotate_ci(args, ctx, true).await,
        SignersCommand::AddCi(args) => rotate_ci(args, ctx, false).await,
        SignersCommand::Trust(args) => {
            let project = project(&ctx, args.project.as_deref()).await?;
            SigningHome::current()?.pin(&project, args.fingerprint.trim())?;
            println!("Pinned {} for project {project}.", args.fingerprint.trim());
            Ok(())
        }
    }
}

async fn project(ctx: &ExecutionMode, project: Option<&str>) -> Result<String> {
    let (project, _) = ctx.resolve_project(project, can_prompt()).await?;
    Ok(project)
}

async fn init(args: InitArgs, ctx: ExecutionMode) -> Result<()> {
    let project = project(&ctx, args.project.as_deref()).await?;
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let existing = api.signer_lists(&project).await?;
    if let Some(first) = existing.first() {
        let digest = first
            .open()
            .context(signing_failed("read the stored signer list"))?
            .digest;
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Project '{project}' already has a signer list with fingerprint {}; use `alien signers trust` on this machine",
                fingerprint(&digest)
            ),
        }));
    }
    if args.ci_key_out.is_none()
        && args.github.is_none()
        && args.kms_key.is_none()
        && !args.create_kms_key
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "ci key".to_string(),
            message: "Choose where the CI key lives: --github OWNER/REPO, --kms-key, --create-kms-key or --ci-key-out".to_string(),
        }));
    }

    let (device_record, device_key) = match home.device()? {
        Some(device) => device,
        None => home.create_device(args.backend.map(Into::into), &default_device_id(), None)?,
    };

    // The recovery key first: if it can't be written, nothing else is made.
    let recovery_public = match (&args.recovery_key_out, &args.recovery_public_key) {
        (Some(path), _) => {
            let key =
                SoftwareKey::generate().context(signing_failed("generate the recovery key"))?;
            write_private(
                path,
                &key.to_pkcs8_pem()
                    .context(signing_failed("encode the recovery key"))?,
            )?;
            key.public_key().as_str().to_string()
        }
        (None, Some(public)) => alien_signing::PublicKey::parse(public)
            .context(signing_failed("read --recovery-public-key"))?
            .as_str()
            .to_string(),
        (None, None) => unreachable!("clap requires one of them"),
    };

    let (ci_public, ci_description) = ci_key(
        &args.github,
        args.kms_key.as_deref(),
        args.create_kms_key,
        args.ci_key_out.as_deref(),
    )
    .await?;

    let body = SignerListBody {
        previous: None,
        admin_threshold: args.admin_threshold,
        signers: vec![
            Signer {
                id: "ci".to_string(),
                key: ci_public,
                kind: SignerKind::Ci,
                roles: vec![Role::Release],
                label: None,
            },
            Signer {
                id: device_record.signer_id.clone(),
                key: device_key.public_key.as_str().to_string(),
                kind: device_record.backend.signer_kind(),
                roles: vec![Role::Release, Role::Operate, Role::Admin],
                label: device_record.label.clone(),
            },
            Signer {
                id: "recovery".to_string(),
                key: recovery_public,
                kind: SignerKind::Recovery,
                roles: vec![Role::Admin],
                label: None,
            },
        ],
        deny: Vec::new(),
    };
    let envelope = Envelope {
        kind: Kind::SignerList.as_str().to_string(),
        project: Some(project.clone()),
        deployment: None,
        sequence: 1,
        not_after: None,
        signer_list_version: 1,
        body,
    };
    let signer = StatementSigner::Device {
        id: device_record.signer_id.clone(),
        key: device_key,
    };
    let statement = sign_envelope(&envelope, &signer).await?;
    let digest = statement
        .open()
        .context(signing_failed("read the new signer list"))?
        .digest;
    let fingerprint = fingerprint(&digest);
    // Check the list exactly as an enforcer will before anyone pins it.
    SignerChain::genesis(&project, &fingerprint, &statement)
        .context(signing_failed("verify the new signer list"))?;
    api.publish_signer_list(&project, &statement).await?;
    home.pin(&project, &fingerprint)?;

    if args.json {
        return print_json(&InitOutput {
            project,
            fingerprint,
            device: device_record.signer_id,
            ci_key: ci_description,
        });
    }
    println!("Signing is set up for project {project}.");
    println!();
    println!("  Fingerprint: {fingerprint}");
    println!(
        "  Device:      {} ({})",
        device_record.signer_id,
        device_record.backend.as_str()
    );
    println!("  CI key:      {ci_description}");
    if let Some(path) = &args.recovery_key_out {
        println!(
            "  Recovery:    {}  (move it offline now, then delete it here)",
            path.display()
        );
    }
    println!();
    println!("Publish the fingerprint in your install guide; each customer's setup pins it.");
    Ok(())
}

/// Make or find the CI key; returns its public key and a description.
async fn ci_key(
    github: &Option<String>,
    kms_key: Option<&str>,
    create_kms_key: bool,
    out: Option<&std::path::Path>,
) -> Result<(String, String)> {
    if let Some(key_id) = kms_key {
        let key = KmsKey::open(key_id).await?;
        return Ok((
            key.public_key().as_str().to_string(),
            format!("AWS KMS {}", key.key_id()),
        ));
    }
    if create_kms_key {
        let key = KmsKey::create("Release signing CI key").await?;
        return Ok((
            key.public_key().as_str().to_string(),
            format!("AWS KMS {}", key.key_id()),
        ));
    }
    let key = SoftwareKey::generate().context(signing_failed("generate the CI key"))?;
    let pem = key
        .to_pkcs8_pem()
        .context(signing_failed("encode the CI key"))?;
    let mut places = Vec::new();
    if let Some(repository) = github {
        store_github_secret(repository, &pem)?;
        places.push(format!(
            "GitHub secret {GITHUB_SECRET} in the '{GITHUB_ENVIRONMENT}' environment of {repository}"
        ));
    }
    if let Some(path) = out {
        write_private(path, &pem)?;
        places.push(path.display().to_string());
    }
    Ok((key.public_key().as_str().to_string(), places.join(" and ")))
}

/// Create the `release` environment, limit it to `main`, and store the key
/// in it, all with the developer's own `gh` login.
fn store_github_secret(repository: &str, pem: &str) -> Result<()> {
    if repository.split('/').count() != 2 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "github".to_string(),
            message: format!("'{repository}' is not OWNER/REPO"),
        }));
    }
    let environment = format!("repos/{repository}/environments/{GITHUB_ENVIRONMENT}");
    gh(
        &["api", "-X", "PUT", &environment, "--input", "-"],
        Some(br#"{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}"#),
    )?;
    let policies = gh(
        &["api", &format!("{environment}/deployment-branch-policies")],
        None,
    )?;
    if !policies.contains("\"name\":\"main\"") {
        gh(
            &[
                "api",
                "-X",
                "POST",
                &format!("{environment}/deployment-branch-policies"),
                "-f",
                "name=main",
                "-f",
                "type=branch",
            ],
            None,
        )?;
    }
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
    Ok(())
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
        .context(signing_failed(
            "run `gh`; install the GitHub CLI and run `gh auth login`",
        ))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(input)
            .into_alien_error()
            .context(signing_failed("write to `gh`"))?;
    }
    let output = child
        .wait_with_output()
        .into_alien_error()
        .context(signing_failed("wait for `gh`"))?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "`gh {}` failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn device(args: DeviceArgs, ctx: ExecutionMode) -> Result<()> {
    let project = project(&ctx, args.project.as_deref()).await?;
    let home = SigningHome::current()?;
    let (record, key) = match home.device()? {
        Some(device) => device,
        None => home.create_device(
            args.backend.map(Into::into),
            &args.id.unwrap_or_else(default_device_id),
            args.label,
        )?,
    };
    let api = SigningApi::from_context(&ctx).await?;
    let signer = Signer {
        id: record.signer_id.clone(),
        key: key.public_key.as_str().to_string(),
        kind: record.backend.signer_kind(),
        roles: vec![Role::Operate],
        label: record.label.clone(),
    };
    let change = api
        .create_pending(
            &project,
            &NewPendingChange {
                kind: PendingKind::Device,
                summary: format!("Add device {}", record.signer_id),
                deployment_id: None,
                device: Some(signer),
                payload: None,
                signatures: Vec::new(),
            },
        )
        .await?;
    let code = approval_code(&key.public_key);
    if args.json {
        return print_json(&serde_json::json!({
            "pendingChange": change.id, "signerId": record.signer_id, "code": code,
            "backend": record.backend.as_str(),
        }));
    }
    println!(
        "Device key {} ({}) is waiting for approval.",
        record.signer_id,
        record.backend.as_str()
    );
    println!();
    println!("  Code: {code}");
    println!();
    println!(
        "Ask an admin to run `alien signers approve {}` and compare this code.",
        record.signer_id
    );
    Ok(())
}

async fn status(project_arg: Option<String>, json: bool, ctx: ExecutionMode) -> Result<()> {
    let project = project(&ctx, project_arg.as_deref()).await?;
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let chain = verified_chain(&api, &home, &project, None).await?;
    let pending: Vec<PendingChange> = api
        .pending(&project)
        .await?
        .into_iter()
        .filter(|change| change.kind == PendingKind::Device)
        .collect();
    let list = chain.current();
    if json {
        return print_json(&serde_json::json!({
            "project": project,
            "fingerprint": home.fingerprint(&project)?,
            "version": list.version,
            "adminThreshold": list.body.admin_threshold,
            "signers": list.body.signers,
            "pendingDevices": pending.iter().filter_map(|change| change.device.clone()).collect::<Vec<_>>(),
        }));
    }
    println!(
        "Signer list version {} (admin threshold {}), verified from {}",
        list.version,
        list.body.admin_threshold,
        home.fingerprint(&project)?.unwrap_or_default()
    );
    for signer in &list.body.signers {
        let roles: Vec<&str> = signer.roles.iter().map(|role| role.as_str()).collect();
        println!(
            "  {:<24} {:<9} {:<28} {}",
            signer.id,
            format!("{:?}", signer.kind).to_lowercase(),
            roles.join(","),
            signer.label.as_deref().unwrap_or("")
        );
    }
    for change in pending {
        if let Some(device) = change.device {
            println!(
                "  pending: {} ({:?}) — `alien signers approve {}`",
                device.id, device.kind, device.id
            );
        }
    }
    Ok(())
}

async fn approve_device(args: ApproveDeviceArgs, ctx: ExecutionMode) -> Result<()> {
    let project = project(&ctx, args.project.as_deref()).await?;
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let chain = verified_chain(&api, &home, &project, args.signer.fingerprint.as_deref()).await?;
    let (change, mut device) = api
        .pending(&project)
        .await?
        .into_iter()
        .filter(|change| change.kind == PendingKind::Device)
        .find_map(|change| {
            let device = change.device.clone()?;
            (change.id == args.id || device.id == args.id).then_some((change, device))
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "id".to_string(),
                message: format!("No pending device '{}' in project {project}", args.id),
            })
        })?;
    let key = alien_signing::PublicKey::parse(&device.key)
        .context(signing_failed("read the pending device key"))?;
    let code = approval_code(&key);
    match &args.code {
        Some(given) if given.eq_ignore_ascii_case(&code) => {}
        Some(given) => {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "The device's code is {code}, not {given}; it is not the device you expect"
                ),
            }))
        }
        None => confirm(
            &format!(
                "Device {} ({:?}{}) shows code {code}. Does that match its screen?",
                device.id,
                device.kind,
                device
                    .label
                    .as_deref()
                    .map(|label| format!(", {label}"))
                    .unwrap_or_default()
            ),
            args.yes,
        )?,
    }
    device.roles = parse_roles(&args.roles)?;
    if chain.current().signer(&device.id).is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message: format!("'{}' is already on the signer list", device.id),
        }));
    }
    let next = next_list(&chain, &project, |body| body.signers.push(device.clone()));
    publish_list(
        &api,
        &home,
        &chain,
        &project,
        next,
        &args.signer,
        Some(&change.id),
    )
    .await
}

async fn remove(args: RemoveArgs, ctx: ExecutionMode) -> Result<()> {
    let project = project(&ctx, args.project.as_deref()).await?;
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let chain = verified_chain(&api, &home, &project, args.signer.fingerprint.as_deref()).await?;
    if chain.current().signer(&args.id).is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message: format!("'{}' is not on the signer list", args.id),
        }));
    }
    confirm(
        &format!("Remove '{}' from the signer list?", args.id),
        args.yes,
    )?;
    let next = next_list(&chain, &project, |body| {
        body.signers.retain(|signer| signer.id != args.id);
        body.deny.extend(args.deny.iter().cloned());
    });
    publish_list(&api, &home, &chain, &project, next, &args.signer, None).await
}

async fn rotate_ci(args: RotateCiArgs, ctx: ExecutionMode, replace: bool) -> Result<()> {
    let project = project(&ctx, args.project.as_deref()).await?;
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let chain = verified_chain(&api, &home, &project, args.signer.fingerprint.as_deref()).await?;
    let exists = chain.current().signer(&args.id).is_some();
    if replace && !exists {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message: format!(
                "'{}' is not on the signer list; use `alien signers add-ci`",
                args.id
            ),
        }));
    }
    if !replace && exists {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "id".to_string(),
            message: format!(
                "'{}' is already on the signer list; use `alien signers rotate-ci`",
                args.id
            ),
        }));
    }
    if args.ci_key_out.is_none() && args.github.is_none() && args.ci_kms_key.is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "ci key".to_string(),
            message: "Choose where the new CI key lives: --github, --ci-kms-key or --ci-key-out"
                .to_string(),
        }));
    }
    let (public, description) = ci_key(
        &args.github,
        args.ci_kms_key.as_deref(),
        false,
        args.ci_key_out.as_deref(),
    )
    .await?;
    let id = args.id.clone();
    let next = next_list(&chain, &project, |body| {
        body.signers.retain(|signer| signer.id != id);
        body.signers.push(Signer {
            id: id.clone(),
            key: public.clone(),
            kind: SignerKind::Ci,
            roles: vec![Role::Release],
            label: None,
        });
        body.deny.extend(args.deny.iter().cloned());
    });
    publish_list(&api, &home, &chain, &project, next, &args.signer, None).await?;
    println!("CI key '{}' is now {description}.", args.id);
    Ok(())
}

/// The next version of the list, changed by `change`.
fn next_list(
    chain: &SignerChain,
    project: &str,
    change: impl FnOnce(&mut SignerListBody),
) -> Envelope<SignerListBody> {
    let current = chain.current();
    let mut body = current.body.clone();
    body.previous = Some(current.digest.clone());
    change(&mut body);
    Envelope {
        kind: Kind::SignerList.as_str().to_string(),
        project: Some(project.to_string()),
        deployment: None,
        sequence: current.version + 1,
        not_after: None,
        signer_list_version: current.version,
        body,
    }
}

/// Sign the next list as an admin. With enough admin signatures it is
/// published; otherwise it waits as a pending change for the other admins.
async fn publish_list(
    api: &SigningApi,
    home: &SigningHome,
    chain: &SignerChain,
    project: &str,
    next: Envelope<SignerListBody>,
    signer_args: &SignerArgs,
    resolves: Option<&str>,
) -> Result<()> {
    let signer = signer_args.open(home).await?;
    check_signer(chain, &signer, Role::Admin)?;
    let statement = sign_envelope(&next, &signer).await?;
    // Check the link exactly as an enforcer will.
    let mut extended = chain.clone();
    let threshold = chain.current().body.admin_threshold as usize;
    if threshold <= statement.signatures.len() {
        extended
            .extend(std::slice::from_ref(&statement))
            .context(signing_failed("verify the next signer list"))?;
        api.publish_signer_list(project, &statement).await?;
        if let Some(id) = resolves {
            api.resolve(
                id,
                &Resolution {
                    signatures: statement.signatures.clone(),
                    statement: Some(statement.clone()),
                },
            )
            .await?;
        }
        println!("Signer list version {} is published.", next.sequence);
        return Ok(());
    }
    let change = api
        .create_pending(
            project,
            &NewPendingChange {
                kind: PendingKind::Statement,
                summary: format!("Signer list version {}", next.sequence),
                deployment_id: None,
                device: None,
                payload: Some(statement.payload.clone()),
                signatures: statement.signatures.clone(),
            },
        )
        .await?;
    println!(
        "Signer list version {} needs {threshold} admin signatures. Other admins run `alien approve {}`.",
        next.sequence, change.id
    );
    Ok(())
}

pub async fn approve_task(args: ApproveArgs, ctx: ExecutionMode) -> Result<()> {
    let api = SigningApi::from_context(&ctx).await?;
    let home = SigningHome::current()?;
    let change = api.pending_change(&args.id).await?;
    if change.kind == PendingKind::Device {
        return approve_device(
            ApproveDeviceArgs {
                id: change.id.clone(),
                code: None,
                roles: vec!["operate".to_string()],
                signer: args.signer,
                project: Some(change.project_id.clone()),
                yes: args.yes,
            },
            ctx,
        )
        .await;
    }
    let payload = change.payload.clone().ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!("Pending change {} carries no statement", change.id),
        })
    })?;
    let unsigned = SignedStatement {
        payload,
        signatures: Vec::new(),
    };
    let opened = unsigned
        .open()
        .context(signing_failed("read the pending statement"))?;
    if opened.project.as_deref() != Some(change.project_id.as_str()) {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "The statement names project '{}', but the change is for '{}'",
                opened.project.as_deref().unwrap_or("none"),
                change.project_id
            ),
        }));
    }
    let chain = verified_chain(
        &api,
        &home,
        &change.project_id,
        args.signer.fingerprint.as_deref(),
    )
    .await?;
    let role = opened.kind.role().ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!("'{}' statements are not signed by developers", opened.kind),
        })
    })?;
    let signer = args.signer.open(&home).await?;
    check_signer(&chain, &signer, role)?;

    // Show the statement itself, decoded from the bytes that will be signed,
    // never the server's summary of it.
    let pretty: serde_json::Value = serde_json::from_slice(&opened.bytes)
        .into_alien_error()
        .context(signing_failed("decode the pending statement"))?;
    let pretty = serde_json::to_string_pretty(&pretty)
        .into_alien_error()
        .context(signing_failed("print the pending statement"))?;
    println!("{pretty}");
    println!();
    confirm(
        &format!("Sign this {} statement as '{}'?", opened.kind, signer.id()),
        args.yes,
    )?;

    let signature = signer.sign(&opened.bytes).await?;
    let mut signatures = change.signatures.clone();
    signatures.retain(|existing| existing.signer != signature.signer);
    signatures.push(signature.clone());
    let statement = SignedStatement {
        payload: unsigned.payload.clone(),
        signatures: signatures.clone(),
    };
    let complete = if opened.kind == Kind::SignerList {
        let threshold = chain.current().body.admin_threshold as usize;
        if signatures.len() >= threshold {
            let mut extended = chain.clone();
            extended
                .extend(std::slice::from_ref(&statement))
                .context(signing_failed("verify the next signer list"))?;
            api.publish_signer_list(&change.project_id, &statement)
                .await?;
            true
        } else {
            false
        }
    } else {
        true
    };
    api.resolve(
        &change.id,
        &Resolution {
            signatures: vec![signature],
            statement: complete.then_some(statement),
        },
    )
    .await?;
    if complete {
        println!("Signed and applied.");
    } else {
        println!("Signed; it waits for more admin signatures.");
    }
    Ok(())
}

fn parse_roles(roles: &[String]) -> Result<Vec<Role>> {
    roles
        .iter()
        .map(|role| match role.trim() {
            "release" => Ok(Role::Release),
            "operate" => Ok(Role::Operate),
            "runtime" => Ok(Role::Runtime),
            "admin" => Ok(Role::Admin),
            other => Err(AlienError::new(ErrorData::ValidationError {
                field: "roles".to_string(),
                message: format!("'{other}' is not release, operate, runtime or admin"),
            })),
        })
        .collect()
}

fn confirm(question: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("{question} Pass --yes to confirm without a terminal."),
        }));
    }
    eprint!("{question} [y/N] ");
    let answer = console::Term::stderr()
        .read_line()
        .into_alien_error()
        .context(ErrorData::CliInteractionFailed {
            message: "Failed to read the answer".to_string(),
        })?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: "Not confirmed; nothing was signed".to_string(),
        }));
    }
    Ok(())
}

fn signing_failed(action: &str) -> ErrorData {
    ErrorData::SigningFailed {
        message: format!("Could not {action}"),
    }
}
