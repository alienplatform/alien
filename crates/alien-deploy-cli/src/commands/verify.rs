//! `verify`: check, with the customer's own read-only credentials, that an
//! installed stack is what the vendor's release signed.
//!
//! Everything is read from the install itself (the stack, its parameters and
//! template, the signing settings and ledger) except the signed statements,
//! which come from the control plane the install already uses. Those are not
//! trusted: every one must verify from the fingerprint the stack pins.

use std::collections::BTreeMap;

use alien_aws_clients::ssm::{GetParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{
    display_fingerprint, settings::names, InstallId, Kind, Ledger, ReleaseBody, Role,
    SignedStatement, SignerChain, SignerKind, SigningSettings, Trust,
};
use aws_credential_types::provider::ProvideCredentials;
use chrono::{DateTime, Utc};
use clap::Args;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::up::resolve_optional_token;
use super::verify_terraform::{directory_files, ModuleCloud, ModuleInstall};
use crate::error::{ErrorData, Result};

/// The template parameter that pins the fingerprint.
const FINGERPRINT_PARAMETER: &str = "SigningFingerprint";
/// Logical ids of the signing parameters in the setup template.
const SIGNING_RESOURCES: [&str; 2] = ["SigningFingerprintParameter", "SigningStateParameter"];
/// The setup template a CloudFormation stack is checked against.
const CLOUDFORMATION: &str = "cloudformation";

#[derive(Args, Debug, Clone)]
#[command(about = "Check that this install runs only what the vendor signed")]
#[command(group(clap::ArgGroup::new("install").required(true).args(["stack", "terraform", "helm"])))]
pub struct VerifyArgs {
    /// The CloudFormation stack of the install.
    #[arg(long)]
    pub stack: Option<String>,

    /// Check a pending setup update (a change set of the stack) before you
    /// execute it.
    #[arg(long, requires = "stack")]
    pub change_set: Option<String>,

    /// After the checks pass, add a stack policy that refuses updates to the
    /// signing settings and the verifier.
    #[arg(long, requires = "stack")]
    pub lock: bool,

    /// A Terraform module directory, exactly as downloaded, to check before
    /// you apply it. Keep it apart from your own configuration: every file
    /// in it counts.
    #[arg(long, value_name = "DIR", requires_all = ["platform", "prefix"])]
    pub terraform: Option<std::path::PathBuf>,

    /// Cloud the Terraform module installs into.
    #[arg(long, value_enum, requires = "terraform")]
    pub platform: Option<ModuleCloud>,

    /// The module's resource_prefix (see `terraform output`).
    #[arg(long, requires = "terraform")]
    pub prefix: Option<String>,

    /// GCP project of the install.
    #[arg(long, requires = "terraform")]
    pub project: Option<String>,

    /// Azure subscription id of the install.
    #[arg(long, requires = "terraform")]
    pub subscription: Option<String>,

    /// Azure resource group of the install.
    #[arg(long, requires = "terraform")]
    pub resource_group: Option<String>,

    /// The install's inputs Key Vault (Azure), which holds its ledger.
    #[arg(long, requires = "terraform")]
    pub vault: Option<String>,

    /// A Helm release to check, with the kubeconfig's cluster and credentials.
    #[arg(long, value_name = "RELEASE", requires = "chart")]
    pub helm: Option<String>,

    /// Namespace of the Helm release.
    #[arg(long, default_value = "default", requires = "helm")]
    pub namespace: String,

    /// The chart directory, exactly as downloaded (extract the archive first).
    #[arg(long, value_name = "DIR", requires = "helm")]
    pub chart: Option<std::path::PathBuf>,

    /// Where to fetch the vendor's signed releases. Defaults to the one this
    /// CLI was built for. What it returns is checked, not trusted.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Token for fetching the signed releases.
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read the token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<std::path::PathBuf>,
}

/// Everything `verify` reads, before any check.
#[derive(Debug, Clone)]
pub struct Observed {
    /// `arn:aws:cloudformation:<region>:<account>:stack/<name>/<id>`.
    pub stack_id: String,
    /// The stack name, which is the install's prefix.
    pub stack_name: String,
    /// Parameters of the stack, or of the change set.
    pub parameters: BTreeMap<String, String>,
    /// Template body as CloudFormation returns it.
    pub template: String,
    /// The signing settings parameter, with when it was last written.
    pub settings: Option<(Vec<u8>, Option<DateTime<Utc>>)>,
    /// The ledger parameter.
    pub ledger: Option<Vec<u8>>,
    /// Signer lists, as the control plane serves them.
    pub signer_lists: Vec<SignedStatement>,
    /// Release ids and their AWS statements, as the control plane serves them.
    pub releases: Vec<(String, SignedStatement)>,
}

/// How one line of the screen came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Checked and matches.
    Pass,
    /// Checked and doesn't match.
    Fail,
    /// Shown, not checked.
    Info,
}

/// One line of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Left column.
    pub label: &'static str,
    /// What was found.
    pub text: String,
    /// The outcome.
    pub mark: Mark,
}

pub(crate) fn line(label: &'static str, text: impl Into<String>, mark: Mark) -> Line {
    Line {
        label,
        text: text.into(),
        mark,
    }
}

/// The install id from the stack's ARN.
fn install_of(stack_id: &str, prefix: &str) -> Result<InstallId> {
    let parts: Vec<&str> = stack_id.split(':').collect();
    let (region, account) = match parts.as_slice() {
        ["arn", _, "cloudformation", region, account, _] => (*region, *account),
        _ => {
            return Err(failed(format!(
                "'{stack_id}' is not a CloudFormation stack ARN"
            )))
        }
    };
    InstallId::parse(&format!("aws:{account}:{region}:{prefix}")).context(ErrorData::VerifyFailed {
        message: format!("stack '{prefix}' doesn't name a valid install"),
    })
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::VerifyFailed {
        message: message.into(),
    })
}

/// What a protected install holds of its signing state, and the signed
/// statements the control plane serves for its platform.
#[derive(Debug, Clone)]
pub struct Signed {
    /// The install id, derived from where the install was read.
    pub install: InstallId,
    /// The install's prefix (stack name, Helm release, ...).
    pub prefix: String,
    /// Where the signing settings live, for messages.
    pub settings_name: String,
    /// The signing settings, with when they were last written if known.
    pub settings: Option<(Vec<u8>, Option<DateTime<Utc>>)>,
    /// Where the ledger lives, for messages.
    pub ledger_name: String,
    /// The ledger.
    pub ledger: Option<Vec<u8>>,
    /// Signer lists, as the control plane serves them.
    pub signer_lists: Vec<SignedStatement>,
    /// Release ids and their statements for this platform, as the control
    /// plane serves them.
    pub releases: Vec<(String, SignedStatement)>,
    /// The platform the statements must be for (`aws`, `gcp`, `azure`, `kubernetes`).
    pub platform: &'static str,
}

/// The setup an install was created from, as read from it.
#[derive(Debug, Clone)]
pub struct Installed {
    /// The setup template it must be (`cloudformation`, `terraform-gcp`, `helm`, ...).
    pub template: &'static str,
    /// `sha256:<hex>` of the installed template bytes.
    pub digest: String,
    /// What the customer chose.
    pub choices: Choices,
}

/// What the customer chose when installing, checked against the signed allow-list.
#[derive(Debug, Clone)]
pub enum Choices {
    /// CloudFormation parameters: the fingerprint parameter must hold the
    /// pinned fingerprint, every other parameter must be allowed.
    Parameters(BTreeMap<String, String>),
    /// Helm values the installer set: each must be allowed, or equal the
    /// chart's own default.
    Values {
        /// Values the installer set (`helm get values`).
        set: serde_json::Map<String, serde_json::Value>,
        /// The chart's `values.yaml`.
        defaults: serde_json::Map<String, serde_json::Value>,
    },
    /// Not readable from the install (Terraform variables live in the
    /// customer's own configuration); the reason is shown.
    NotRead(&'static str),
}

/// Check what was read from a CloudFormation stack.
pub fn check(observed: &Observed, now: DateTime<Utc>) -> Result<Vec<Line>> {
    let prefix = observed.stack_name.as_str();
    let signed = Signed {
        install: install_of(&observed.stack_id, prefix)?,
        prefix: prefix.to_string(),
        settings_name: names::aws_settings(prefix),
        settings: observed.settings.clone(),
        ledger_name: names::aws_ledger(prefix),
        ledger: observed.ledger.clone(),
        signer_lists: observed.signer_lists.clone(),
        releases: observed.releases.clone(),
        platform: "aws",
    };
    let installed = Installed {
        template: CLOUDFORMATION,
        digest: sha256_digest(observed.template.as_bytes()),
        choices: Choices::Parameters(observed.parameters.clone()),
    };
    // Who manages the install is the customer's choice; whoever it is, the
    // signing limits hold. Shown so they know whom they let in.
    let managing = observed
        .parameters
        .get("ManagingRoleArn")
        .filter(|arn| !arn.is_empty());
    let external_id = observed
        .parameters
        .get("ManagementExternalId")
        .is_some_and(|id| !id.is_empty());
    let managed_by = line(
        "Managed by",
        match managing {
            Some(arn) if external_id => format!("{arn} · with an external id"),
            Some(arn) => arn.clone(),
            None => "no management role".to_string(),
        },
        Mark::Info,
    );
    check_signed(&signed, &installed, vec![managed_by], now)
}

/// `sha256:<hex>` of `bytes`.
pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Check an install against the releases signed for it. Errors are reasons
/// nothing could be checked; a mismatch is a failed line. `extra` lines go
/// after the customer's choices.
pub fn check_signed(
    signed: &Signed,
    installed: &Installed,
    extra: Vec<Line>,
    now: DateTime<Utc>,
) -> Result<Vec<Line>> {
    let prefix = signed.prefix.as_str();
    let mut lines = vec![line("Install", signed.install.to_string(), Mark::Info)];

    let (settings_bytes, set_at) = signed.settings.as_ref().ok_or_else(|| {
        failed(format!(
            "{} does not exist, so this install is not protected: it runs whatever the vendor's control plane sends",
            signed.settings_name
        ))
    })?;
    let settings =
        SigningSettings::parse(settings_bytes, prefix).context(ErrorData::VerifyFailed {
            message: format!("{} is not valid signing settings", signed.settings_name),
        })?;
    lines.push(line(
        "Signing fingerprint",
        format!(
            "{}   (set by setup{})",
            display_fingerprint(&settings.fingerprint),
            set_at
                .map(|at| format!(", {}", at.format("%Y-%m-%d")))
                .unwrap_or_default()
        ),
        Mark::Info,
    ));

    let ledger_bytes = signed.ledger.as_ref().ok_or_else(|| {
        failed(format!(
            "{} is missing; setup creates it, so the install was changed outside setup",
            signed.ledger_name
        ))
    })?;
    let ledger = Ledger::parse(ledger_bytes, prefix).context(ErrorData::VerifyFailed {
        message: format!("{} is not a valid ledger", signed.ledger_name),
    })?;
    let trust = Trust::new(
        settings.clone(),
        signed.install.clone(),
        &signed.signer_lists,
        &ledger,
        now,
    )
    .context(ErrorData::VerifyFailed {
        message: format!(
            "the signer lists the control plane returned don't verify from fingerprint {} and this install's ledger",
            display_fingerprint(&settings.fingerprint)
        ),
    })?;

    // Only statements that verify here count; the rest are the control
    // plane's word.
    let verified: Vec<(&str, alien_signing::Verified<ReleaseBody>)> = signed
        .releases
        .iter()
        .filter_map(|(id, statement)| {
            trust
                .verify_release(statement, now)
                .ok()
                .filter(|verified| verified.body.platform == signed.platform)
                .map(|verified| (id.as_str(), verified))
        })
        .collect();

    let digest = &installed.digest;
    let matching = verified
        .iter()
        .filter(|(_, verified)| {
            verified
                .body
                .setup
                .get(installed.template)
                .is_some_and(|template| template.sha256 == *digest)
        })
        .max_by_key(|(_, verified)| verified.sequence);
    let allowed: Vec<String> = match matching {
        Some((id, verified)) => {
            lines.push(line(
                "Template",
                format!(
                    "matches release {id} · {} · {digest}",
                    verified.body.channel
                ),
                Mark::Pass,
            ));
            verified.body.setup[installed.template].allowed.clone()
        }
        None => {
            lines.push(line(
                "Template",
                format!(
                    "{digest} is not the {} template of any release signed for {}",
                    installed.template, settings.channel
                ),
                Mark::Fail,
            ));
            Vec::new()
        }
    };
    lines.push(choices_line(&installed.choices, &allowed, &settings));
    lines.extend(extra);

    let list = trust.chain().current();
    let label = |id: &str| {
        list.body
            .signers
            .iter()
            .find(|signer| signer.id == id)
            .map(|signer| match signer.kind {
                SignerKind::Recovery => "recovery".to_string(),
                _ => signer.label.clone().unwrap_or_else(|| signer.id.clone()),
            })
            .unwrap_or_else(|| id.to_string())
    };
    let running = ledger
        .floor(Kind::Release)
        .context(ErrorData::VerifyFailed {
            message: "the ledger can't be read".to_string(),
        })?;
    lines.push(match running {
        None => line("Running", "nothing yet: no release has been accepted", Mark::Info),
        Some(entry) => match running_release(&trust, &settings, &signed.releases, &entry.digest, now) {
            Ok((id, signers, still_trusted)) => {
                let signed_by = signers
                    .iter()
                    .map(|id| label(id))
                    .collect::<Vec<_>>()
                    .join(", ");
                if still_trusted {
                    line(
                        "Running",
                        format!("release {id} · signed by {signed_by} · {}", entry.digest),
                        Mark::Pass,
                    )
                } else {
                    // It verified against the list it names; its signer has
                    // since left the list, which doesn't stop it.
                    line(
                        "Running",
                        format!(
                            "release {id} · signed by {signed_by}, no longer on the list; it keeps running until the next release · {}",
                            entry.digest
                        ),
                        Mark::Info,
                    )
                }
            }
            Err(reason) => line(
                "Running",
                format!("the release with digest {}: {reason}", entry.digest),
                Mark::Fail,
            ),
        },
    });

    lines.push(line(
        "Trusted keys",
        format!(
            "{}  (list v{}, expires {}{})",
            list.body
                .signers
                .iter()
                .map(|signer| label(&signer.id))
                .collect::<Vec<_>>()
                .join(", "),
            list.version,
            list.not_after.format("%Y-%m-%d"),
            if list.body.recovered {
                ", restored with the recovery code"
            } else {
                ""
            }
        ),
        Mark::Info,
    ));
    Ok(lines)
}

/// The line for what the customer chose, against the signed `allowed` list.
fn choices_line(choices: &Choices, allowed: &[String], settings: &SigningSettings) -> Line {
    let mut refused = Vec::new();
    let mut chosen = 0;
    let (label, what) = match choices {
        Choices::Parameters(parameters) => {
            for (name, value) in parameters {
                if name == FINGERPRINT_PARAMETER {
                    if *value != settings.fingerprint {
                        refused.push(format!("{name} is {value}, not the pinned fingerprint"));
                    }
                } else if allowed.contains(name) {
                    chosen += 1;
                } else {
                    refused.push(format!("{name} is not a choice the signed release allows"));
                }
            }
            if !parameters.contains_key(FINGERPRINT_PARAMETER) {
                refused.push(format!("{FINGERPRINT_PARAMETER} is missing"));
            }
            ("Parameters", "fingerprint pinned · ")
        }
        Choices::Values { set, defaults } => {
            for (name, value) in set {
                if allowed.contains(name) {
                    chosen += 1;
                } else if defaults.get(name) != Some(value) {
                    refused.push(format!(
                        "{name} is set, and it is not a value the signed release allows"
                    ));
                }
            }
            ("Values", "")
        }
        Choices::NotRead(reason) => return line("Variables", *reason, Mark::Info),
    };
    if refused.is_empty() {
        line(
            label,
            format!(
                "{what}{chosen} customer choice{} allowed",
                if chosen == 1 { "" } else { "s" }
            ),
            Mark::Pass,
        )
    } else {
        line(label, refused.join("; "), Mark::Fail)
    }
}

/// The release the ledger says runs, checked from the statement the control
/// plane returned for its digest: not deny-listed, a release of this project
/// for this install's channel, signed with the release role by the list
/// version it names. Returns its id (the control plane's name for it), its
/// signers and whether they all still hold the release role now.
fn running_release<'a>(
    trust: &Trust,
    settings: &SigningSettings,
    releases: &'a [(String, SignedStatement)],
    digest: &str,
    now: DateTime<Utc>,
) -> std::result::Result<(&'a str, Vec<String>, bool), String> {
    trust
        .keeps(digest)
        .map_err(|_| "it is deny-listed on the current signer list".to_string())?;
    let (id, opened) = releases
        .iter()
        .find_map(|(id, statement)| {
            statement
                .open()
                .ok()
                .filter(|opened| opened.digest == digest)
                .map(|opened| (id.as_str(), opened))
        })
        .ok_or_else(|| "the control plane didn't return it, so it can't be checked".to_string())?;
    if opened.kind != Kind::Release || opened.project != trust.project() {
        return Err(format!(
            "it is a {} statement of project {}, not a release of {}",
            opened.kind.as_str(),
            opened.project,
            trust.project()
        ));
    }
    let body: ReleaseBody = opened
        .body()
        .map_err(|error| format!("its body doesn't parse: {error}"))?;
    if body.channel != settings.channel {
        return Err(format!(
            "it is signed for channel {}, but this install follows {}",
            body.channel, settings.channel
        ));
    }
    let chain = trust.chain();
    let version = opened.signer_list_version;
    if version == 0 || version > chain.current().version {
        return Err(format!(
            "it names signer list v{version}, which the chain doesn't hold"
        ));
    }
    let named = SignerChain::from_statements(
        &settings.fingerprint,
        &chain.statements()[..version as usize],
        now,
    )
    .map_err(|error| format!("signer list v{version} doesn't verify: {error}"))?;
    let signers = named
        .current()
        .check_signatures(&opened, Role::Release)
        .map_err(|error| {
            format!("it isn't signed by release keys of signer list v{version}: {error}")
        })?;
    let still_trusted = signers.iter().all(|id| {
        chain
            .current()
            .signer(id)
            .is_some_and(|(_, roles)| roles.contains(&Role::Release))
    });
    Ok((id, signers.into_iter().collect(), still_trusted))
}

/// The screen for `lines`.
pub fn render(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        let mark = match line.mark {
            Mark::Pass => "  ✓",
            Mark::Fail => "  ✗",
            Mark::Info => "",
        };
        out.push_str(&format!("{:<21}{}{mark}\n", line.label, line.text));
    }
    out
}

pub async fn verify_command(
    args: VerifyArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    let token = resolve_optional_token(
        args.token.clone(),
        args.token_file.as_ref(),
        embedded_config,
    )?;
    let url = args
        .manager_url
        .clone()
        .or_else(|| embedded_config.and_then(|config| config.api_base_url.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Pass --manager-url: where the vendor's signed releases are served"
                    .to_string(),
            })
        })?;

    let token = token.as_deref();
    if let Some(stack) = &args.stack {
        let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let cloudformation = aws_sdk_cloudformation::Client::new(&aws);
        let observed = observe(
            stack,
            args.change_set.as_deref(),
            &aws,
            &cloudformation,
            &url,
            token,
        )
        .await?;
        show(&check(&observed, Utc::now())?)?;
        if args.lock {
            lock(&cloudformation, stack).await?;
        }
        return Ok(());
    }
    if let (Some(dir), Some(cloud), Some(prefix)) = (&args.terraform, args.platform, &args.prefix) {
        let installed = super::verify_terraform::installed_module(dir, cloud)?;
        let mut signed = super::verify_terraform::read_install(&ModuleInstall {
            cloud,
            prefix: prefix.clone(),
            project: args.project.clone(),
            subscription: args.subscription.clone(),
            resource_group: args.resource_group.clone(),
            vault: args.vault.clone(),
        })
        .await?;
        (signed.signer_lists, signed.releases) =
            signed_releases(&url, token, cloud.platform()).await?;
        return show(&check_signed(&signed, &installed, Vec::new(), Utc::now())?);
    }
    if let (Some(release), Some(chart)) = (&args.helm, &args.chart) {
        let files = directory_files(chart)?;
        let observed = super::verify_helm::observe(release, &args.namespace).await?;
        let (mut signed, installed, chart_line) =
            super::verify_helm::helm_install(&observed, &files)?;
        (signed.signer_lists, signed.releases) = signed_releases(&url, token, "kubernetes").await?;
        return show(&check_signed(
            &signed,
            &installed,
            vec![chart_line],
            Utc::now(),
        )?);
    }
    Err(failed(
        "pass --stack, --terraform with --platform and --prefix, or --helm with --chart",
    ))
}

/// Print the screen; any failed line fails the command.
fn show(lines: &[Line]) -> Result<()> {
    print!("{}", render(lines));
    if lines.iter().any(|line| line.mark == Mark::Fail) {
        return Err(failed(
            "this install does not match what the vendor signed; don't create or update it",
        ));
    }
    Ok(())
}

/// An SSM client on the same credentials and region as CloudFormation, so
/// both read the one install: the credentials the SDK chain resolves, in
/// the account and region of the stack.
async fn ssm_client(aws: &aws_config::SdkConfig, stack_id: &str) -> Result<SsmClient> {
    let region = aws
        .region()
        .map(|region| region.to_string())
        .ok_or_else(|| failed("no AWS region is set; set AWS_REGION or a profile region"))?;
    // arn:aws:cloudformation:<region>:<account>:stack/<name>/<id>
    let (stack_region, account) = match stack_id.split(':').collect::<Vec<_>>().as_slice() {
        ["arn", _, "cloudformation", region, account, ..] => {
            (region.to_string(), account.to_string())
        }
        _ => {
            return Err(failed(format!(
                "'{stack_id}' is not a CloudFormation stack ARN"
            )))
        }
    };
    if stack_region != region {
        return Err(failed(format!(
            "the stack is in {stack_region}, but the credentials are for {region}"
        )));
    }
    ssm_client_for(aws, account, region).await
}

/// An SSM client on the credentials the SDK chain resolves, for `account`
/// and `region`.
pub(crate) async fn ssm_client_for(
    aws: &aws_config::SdkConfig,
    account: String,
    region: String,
) -> Result<SsmClient> {
    let provider = aws
        .credentials_provider()
        .ok_or_else(|| failed("no AWS credentials were found"))?;
    let credentials = provider
        .provide_credentials()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Could not load AWS credentials".to_string(),
        })?;
    let config = AwsClientConfig {
        account_id: account,
        region,
        credentials: AwsCredentials::AccessKeys {
            access_key_id: credentials.access_key_id().to_string(),
            secret_access_key: credentials.secret_access_key().to_string(),
            session_token: credentials.session_token().map(str::to_string),
        },
        service_overrides: None,
    };
    let credentials = AwsCredentialProvider::from_config(config).await.context(
        ErrorData::ConfigurationError {
            message: "Could not load AWS credentials".to_string(),
        },
    )?;
    Ok(SsmClient::new(reqwest::Client::new(), credentials))
}

fn aws_failed(operation: &str, stack: &str) -> ErrorData {
    ErrorData::HttpError {
        operation: operation.to_string(),
        url: format!("aws://cloudformation/{stack}"),
        reason: format!("{operation} failed; check the stack name, region and credentials"),
    }
}

async fn observe(
    stack: &str,
    change_set: Option<&str>,
    aws: &aws_config::SdkConfig,
    cloudformation: &aws_sdk_cloudformation::Client,
    url: &str,
    token: Option<&str>,
) -> Result<Observed> {
    let described = cloudformation
        .describe_stacks()
        .stack_name(stack)
        .send()
        .await
        .into_alien_error()
        .context(aws_failed("DescribeStacks", stack))?;
    let found = described
        .stacks()
        .first()
        .ok_or_else(|| failed(format!("no stack '{stack}'")))?;
    let stack_id = found
        .stack_id()
        .ok_or_else(|| failed(format!("stack '{stack}' has no id")))?
        .to_string();
    let stack_name = found.stack_name().unwrap_or(stack).to_string();

    let (parameters, template_request) = match change_set {
        Some(change_set) => {
            let described = cloudformation
                .describe_change_set()
                .stack_name(stack)
                .change_set_name(change_set)
                .send()
                .await
                .into_alien_error()
                .context(aws_failed("DescribeChangeSet", stack))?;
            (
                parameters_of(described.parameters()),
                cloudformation
                    .get_template()
                    .stack_name(stack)
                    .change_set_name(change_set),
            )
        }
        None => (
            parameters_of(found.parameters()),
            cloudformation.get_template().stack_name(stack),
        ),
    };
    let template = template_request
        .template_stage(aws_sdk_cloudformation::types::TemplateStage::Original)
        .send()
        .await
        .into_alien_error()
        .context(aws_failed("GetTemplate", stack))?
        .template_body()
        .ok_or_else(|| failed(format!("stack '{stack}' returned no template")))?
        .to_string();

    let ssm = &ssm_client(aws, &stack_id).await?;
    let settings = parameter(ssm, &names::aws_settings(&stack_name)).await?;
    let ledger = parameter(ssm, &names::aws_ledger(&stack_name))
        .await?
        .map(|(bytes, _)| bytes);
    let (signer_lists, releases) = signed_releases(url, token, "aws").await?;
    Ok(Observed {
        stack_id,
        stack_name,
        parameters,
        template,
        settings,
        ledger,
        signer_lists,
        releases,
    })
}

fn parameters_of(
    parameters: &[aws_sdk_cloudformation::types::Parameter],
) -> BTreeMap<String, String> {
    parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                parameter.parameter_key()?.to_string(),
                parameter
                    .resolved_value()
                    .or(parameter.parameter_value())
                    .unwrap_or_default()
                    .to_string(),
            ))
        })
        .collect()
}

pub(crate) async fn parameter(
    ssm: &SsmClient,
    name: &str,
) -> Result<Option<(Vec<u8>, Option<DateTime<Utc>>)>> {
    match ssm
        .get_parameter(GetParameterRequest {
            name: name.to_string(),
            with_decryption: Some(true),
        })
        .await
    {
        Ok(response) => {
            let parameter = response
                .parameter
                .ok_or_else(|| failed(format!("{name} returned no value")))?;
            let set_at = parameter
                .last_modified_date
                .and_then(|seconds| DateTime::from_timestamp(seconds as i64, 0));
            let value = parameter
                .value
                .ok_or_else(|| failed(format!("{name} returned no value")))?;
            Ok(Some((value.into_bytes(), set_at)))
        }
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error).context(ErrorData::HttpError {
            operation: "GetParameter".to_string(),
            url: format!("aws://ssm/{name}"),
            reason: "GetParameter failed; verify needs ssm:GetParameter on /signed/*".to_string(),
        }),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePage {
    items: Vec<ReleaseItem>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseItem {
    id: String,
    #[serde(default)]
    signed_stacks: BTreeMap<String, SignedStatement>,
}

#[derive(Deserialize)]
struct Lists {
    items: Vec<SignedStatement>,
}

/// The project's signer lists and its releases signed for `platform`, as
/// the control plane serves them.
pub(crate) async fn signed_releases(
    url: &str,
    token: Option<&str>,
    platform: &str,
) -> Result<(Vec<SignedStatement>, Vec<(String, SignedStatement)>)> {
    let client = reqwest::Client::new();
    let base = url.trim_end_matches('/');
    let get = |path: String, query: Vec<(&'static str, String)>| {
        let mut request = client.get(format!("{base}{path}")).query(&query);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        async move {
            let url = format!("{base}{path}");
            let response = request.send().await.into_alien_error().context(
                ErrorData::ManagerRequestFailed {
                    message: format!("GET {url} failed"),
                },
            )?;
            let status = response.status();
            if !status.is_success() {
                return Err(AlienError::new(ErrorData::ManagerRequestFailed {
                    message: format!("GET {url} returned {status}"),
                }));
            }
            response
                .bytes()
                .await
                .into_alien_error()
                .context(ErrorData::ManagerRequestFailed {
                    message: format!("GET {url}: the response could not be read"),
                })
        }
    };
    let mut releases = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut query = vec![("limit", "100".to_string())];
        if let Some(cursor) = &cursor {
            query.push(("cursor", cursor.clone()));
        }
        let page: ReleasePage =
            serde_json::from_slice(&get("/v1/releases".to_string(), query).await?)
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "parse".to_string(),
                    reason: "the release list".to_string(),
                })?;
        for item in page.items {
            if let Some(statement) = item.signed_stacks.get(platform) {
                releases.push((item.id, statement.clone()));
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    // The project is the one the statements name; the lists of any other
    // project won't verify from this stack's fingerprint.
    let Some(project) = releases
        .iter()
        .find_map(|(_, statement)| statement.open().ok().map(|opened| opened.project))
    else {
        return Err(failed(format!(
            "the control plane returned no release signed for {platform} to check this install against"
        )));
    };
    let lists: Lists = serde_json::from_slice(
        &get(format!("/v1/projects/{project}/signer-lists"), Vec::new()).await?,
    )
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "parse".to_string(),
        reason: "the signer lists".to_string(),
    })?;
    Ok((lists.items, releases))
}

/// Add a stack policy that refuses updates to the signing settings and the
/// verifier, keeping whatever the existing policy says.
async fn lock(cloudformation: &aws_sdk_cloudformation::Client, stack: &str) -> Result<()> {
    let resources = cloudformation
        .describe_stack_resources()
        .stack_name(stack)
        .send()
        .await
        .into_alien_error()
        .context(aws_failed("DescribeStackResources", stack))?;
    let verifier = format!("{stack}-verifier");
    let mut locked: Vec<String> = SIGNING_RESOURCES.iter().map(|id| id.to_string()).collect();
    for resource in resources.stack_resources() {
        if resource.physical_resource_id() == Some(verifier.as_str()) {
            if let Some(id) = resource.logical_resource_id() {
                locked.push(id.to_string());
            }
        }
    }
    let existing = cloudformation
        .get_stack_policy()
        .stack_name(stack)
        .send()
        .await
        .into_alien_error()
        .context(aws_failed("GetStackPolicy", stack))?;
    let policy = locked_policy(existing.stack_policy_body(), &locked)?;
    cloudformation
        .set_stack_policy()
        .stack_name(stack)
        .stack_policy_body(policy)
        .send()
        .await
        .into_alien_error()
        .context(aws_failed("SetStackPolicy", stack))?;
    println!(
        "Locked: stack updates can no longer change {}.",
        locked.join(", ")
    );
    Ok(())
}

/// `existing` (or a policy allowing every update) plus a statement denying
/// updates to `resources`.
fn locked_policy(existing: Option<&str>, resources: &[String]) -> Result<String> {
    let mut policy: serde_json::Value = match existing {
        Some(body) if !body.trim().is_empty() => serde_json::from_str(body)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "the existing stack policy".to_string(),
            })?,
        _ => serde_json::json!({
            "Statement": [
                { "Effect": "Allow", "Action": "Update:*", "Principal": "*", "Resource": "*" }
            ]
        }),
    };
    let statements = policy["Statement"]
        .as_array_mut()
        .ok_or_else(|| failed("the existing stack policy has no Statement list"))?;
    let deny = serde_json::json!({
        "Effect": "Deny",
        "Action": "Update:*",
        "Principal": "*",
        "Resource": resources
            .iter()
            .map(|id| format!("LogicalResourceId/{id}"))
            .collect::<Vec<_>>(),
    });
    if !statements.contains(&deny) {
        statements.push(deny);
    }
    serde_json::to_string(&policy)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "the stack policy".to_string(),
        })
}

#[cfg(test)]
#[path = "verify_tests.rs"]
pub(crate) mod tests;
