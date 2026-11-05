//! `verify`: check, with the customer's own read-only credentials, that an
//! installed stack is what the vendor's release signed.
//!
//! Everything is read from the install itself (the stack, its parameters and
//! template, the signing settings and ledger) except the signed statements,
//! which come from the control plane the install already uses. Those are not
//! trusted: every one must verify from the fingerprint the stack pins.

use std::collections::BTreeMap;

use alien_aws_clients::ssm::{GetParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{
    display_fingerprint, settings::names, InstallId, Kind, Ledger, ReleaseBody, SignedStatement,
    SignerKind, SigningSettings, Trust,
};
use chrono::{DateTime, Utc};
use clap::Args;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::up::resolve_optional_token;
use crate::error::{ErrorData, Result};

/// The template parameter that pins the fingerprint.
const FINGERPRINT_PARAMETER: &str = "SigningFingerprint";
/// Logical ids of the signing parameters in the setup template.
const SIGNING_RESOURCES: [&str; 2] = ["SigningFingerprintParameter", "SigningStateParameter"];
/// The setup template a CloudFormation stack is checked against.
const CLOUDFORMATION: &str = "cloudformation";

#[derive(Args, Debug, Clone)]
#[command(about = "Check that this install runs only what the vendor signed")]
pub struct VerifyArgs {
    /// The CloudFormation stack of the install.
    #[arg(long)]
    pub stack: String,

    /// Check a pending setup update (a change set of the stack) before you
    /// execute it.
    #[arg(long)]
    pub change_set: Option<String>,

    /// After the checks pass, add a stack policy that refuses updates to the
    /// signing settings and the verifier.
    #[arg(long)]
    pub lock: bool,

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

fn line(label: &'static str, text: impl Into<String>, mark: Mark) -> Line {
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

/// Check what was read. Errors are reasons nothing could be checked; a
/// mismatch is a failed line.
pub fn check(observed: &Observed, now: DateTime<Utc>) -> Result<Vec<Line>> {
    let prefix = observed.stack_name.as_str();
    let install = install_of(&observed.stack_id, prefix)?;
    let mut lines = vec![line("Install", install.to_string(), Mark::Info)];

    let (settings_bytes, set_at) = observed.settings.as_ref().ok_or_else(|| {
        failed(format!(
            "{} does not exist, so this stack is not protected: it runs whatever the vendor's control plane sends",
            names::aws_settings(prefix)
        ))
    })?;
    let settings =
        SigningSettings::parse(settings_bytes, prefix).context(ErrorData::VerifyFailed {
            message: format!(
                "{} is not valid signing settings",
                names::aws_settings(prefix)
            ),
        })?;
    lines.push(line(
        "Signing fingerprint",
        format!(
            "{}   (set by this stack{})",
            display_fingerprint(&settings.fingerprint),
            set_at
                .map(|at| format!(", {}", at.format("%Y-%m-%d")))
                .unwrap_or_default()
        ),
        Mark::Info,
    ));

    let ledger_bytes = observed.ledger.as_ref().ok_or_else(|| {
        failed(format!(
            "{} is missing; setup creates it, so the stack was changed outside setup",
            names::aws_ledger(prefix)
        ))
    })?;
    let ledger = Ledger::parse(ledger_bytes, prefix).context(ErrorData::VerifyFailed {
        message: format!("{} is not a valid ledger", names::aws_ledger(prefix)),
    })?;
    let trust = Trust::new(
        settings.clone(),
        install.clone(),
        &observed.signer_lists,
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
    let verified: Vec<(&str, alien_signing::Verified<ReleaseBody>)> = observed
        .releases
        .iter()
        .filter_map(|(id, statement)| {
            trust
                .verify_release(statement, now)
                .ok()
                .filter(|verified| verified.body.platform == "aws")
                .map(|verified| (id.as_str(), verified))
        })
        .collect();

    let digest = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(observed.template.as_bytes()))
    );
    let matching = verified
        .iter()
        .filter(|(_, verified)| {
            verified
                .body
                .setup
                .get(CLOUDFORMATION)
                .is_some_and(|template| template.sha256 == digest)
        })
        .max_by_key(|(_, verified)| verified.sequence);
    let allowed: Vec<String> = match matching {
        Some((id, verified)) => {
            lines.push(line(
                "Template",
                format!("matches release {id} · {}", verified.body.channel),
                Mark::Pass,
            ));
            verified.body.setup[CLOUDFORMATION].allowed.clone()
        }
        None => {
            lines.push(line(
                "Template",
                format!(
                    "{digest} is not the template of any release signed for {}",
                    settings.channel
                ),
                Mark::Fail,
            ));
            Vec::new()
        }
    };

    let mut refused = Vec::new();
    let mut choices = 0;
    for (name, value) in &observed.parameters {
        if name == FINGERPRINT_PARAMETER {
            if *value != settings.fingerprint {
                refused.push(format!("{name} is {value}, not the pinned fingerprint"));
            }
        } else if allowed.contains(name) {
            choices += 1;
        } else {
            refused.push(format!("{name} is not a choice the signed release allows"));
        }
    }
    if !observed.parameters.contains_key(FINGERPRINT_PARAMETER) {
        refused.push(format!("{FINGERPRINT_PARAMETER} is missing"));
    }
    lines.push(if refused.is_empty() {
        line(
            "Parameters",
            format!(
                "fingerprint pinned · {choices} customer choice{} allowed",
                if choices == 1 { "" } else { "s" }
            ),
            Mark::Pass,
        )
    } else {
        line("Parameters", refused.join("; "), Mark::Fail)
    });

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
        Some(entry) => match verified
            .iter()
            .find(|(_, verified)| verified.digest == entry.digest)
        {
            Some((id, verified)) => line(
                "Running",
                format!(
                    "release {id} · signed by {}",
                    verified
                        .signers
                        .iter()
                        .map(|id| label(id))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Mark::Pass,
            ),
            None => {
                let known = observed.releases.iter().find(|(_, statement)| {
                    statement
                        .open()
                        .is_ok_and(|opened| opened.digest == entry.digest)
                });
                let text = match known {
                    // It verified when the install accepted it; its signer
                    // has since left the list, which doesn't stop it.
                    Some((id, _)) => format!(
                        "release {id} · its signer is no longer on the list; it keeps running until the next release"
                    ),
                    None => format!(
                        "the release with digest {} (the control plane didn't return it)",
                        entry.digest
                    ),
                };
                line("Running", text, Mark::Info)
            }
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
    let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let cloudformation = aws_sdk_cloudformation::Client::new(&aws);
    let ssm = ssm_client().await?;
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

    let observed = observe(&args, &cloudformation, &ssm, &url, token.as_deref()).await?;
    let lines = check(&observed, Utc::now())?;
    print!("{}", render(&lines));
    if lines.iter().any(|line| line.mark == Mark::Fail) {
        return Err(failed(
            "this install does not match what the vendor signed; don't create or update it",
        ));
    }
    if args.lock {
        lock(&cloudformation, &args.stack).await?;
    }
    Ok(())
}

async fn ssm_client() -> Result<SsmClient> {
    let config = AwsClientConfig::from_std_env()
        .await
        .context(ErrorData::ConfigurationError {
            message: "Could not load AWS credentials; set AWS_PROFILE or AWS_REGION".to_string(),
        })?;
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
    args: &VerifyArgs,
    cloudformation: &aws_sdk_cloudformation::Client,
    ssm: &SsmClient,
    url: &str,
    token: Option<&str>,
) -> Result<Observed> {
    let stack = args.stack.as_str();
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

    let (parameters, template_request) = match &args.change_set {
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

    let settings = parameter(ssm, &names::aws_settings(&stack_name)).await?;
    let ledger = parameter(ssm, &names::aws_ledger(&stack_name))
        .await?
        .map(|(bytes, _)| bytes);
    let (signer_lists, releases) = signed_releases(url, token).await?;
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

async fn parameter(
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

/// The project's signer lists and its signed AWS releases, as the control
/// plane serves them.
async fn signed_releases(
    url: &str,
    token: Option<&str>,
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
            if let Some(statement) = item.signed_stacks.get("aws") {
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
        return Err(failed(
            "the control plane returned no signed AWS release to check this stack against",
        ));
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
mod tests;
