//! `verify --terraform`: check a protected Terraform module, as downloaded,
//! against the vendor's signed release before applying it, and the install's
//! signing state in its cloud.
//!
//! The module directory is hashed as the file map setup signs (see
//! [`file_map_bytes`]); the settings and ledger are read with the customer's
//! own credentials from where setup writes them: SSM on AWS, Parameter
//! Manager on GCP, the resource group tag and the inputs Key Vault on Azure.

use std::collections::BTreeMap;
use std::path::Path;

use alien_azure_clients::azure::token_cache::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_cli_common::setup_templates::file_map_bytes;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use alien_signing::{settings::names, InstallId};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde_json::Value;

use super::verify::{sha256_digest, Choices, Installed, Signed};
use crate::error::{ErrorData, Result};

/// What `verify` says about Terraform variables: they are set in the
/// customer's own configuration, which the module can't see.
const VARIABLES: &str =
    "set in your own configuration; the module pins the fingerprint and refuses any other";

/// Cloud a Terraform module installs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModuleCloud {
    Aws,
    Gcp,
    Azure,
}

impl ModuleCloud {
    /// The platform its releases are signed for.
    pub fn platform(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
        }
    }

    /// The setup template its module is.
    pub fn template(self) -> &'static str {
        match self {
            Self::Aws => "terraform-aws",
            Self::Gcp => "terraform-gcp",
            Self::Azure => "terraform-azure",
        }
    }
}

/// Where the install lives, from the command line.
#[derive(Debug, Clone)]
pub struct ModuleInstall {
    pub cloud: ModuleCloud,
    /// The module's `resource_prefix`.
    pub prefix: String,
    /// GCP project.
    pub project: Option<String>,
    /// Azure subscription id.
    pub subscription: Option<String>,
    /// Azure resource group.
    pub resource_group: Option<String>,
    /// Azure inputs Key Vault name.
    pub vault: Option<String>,
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::VerifyFailed {
        message: message.into(),
    })
}

/// The files of a module or chart directory as the file map setup signs:
/// every regular file under `dir`, keyed by its path relative to `dir` with
/// `/` separators. Symlinks and files that aren't UTF-8 are refused (the
/// signed map holds neither), and nothing is skipped, so a file added to the
/// directory changes the digest.
pub fn directory_files(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.into_alien_error().context(ErrorData::VerifyFailed {
            message: format!("could not read the directory {}", dir.display()),
        })?;
        let kind = entry.file_type();
        if kind.is_dir() {
            continue;
        }
        let path = entry.path();
        if !kind.is_file() {
            return Err(failed(format!(
                "{} is not a regular file; a signed module or chart holds only files",
                path.display()
            )));
        }
        let relative =
            path.strip_prefix(dir)
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: format!("{} is outside {}", path.display(), dir.display()),
                })?;
        let key = relative
            .components()
            .map(|part| {
                part.as_os_str()
                    .to_str()
                    .ok_or_else(|| failed(format!("the path {} is not UTF-8", relative.display())))
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        let contents =
            std::fs::read_to_string(path)
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: format!("could not read {} as UTF-8 text", path.display()),
                })?;
        files.insert(key, contents);
    }
    if files.is_empty() {
        return Err(failed(format!("{} holds no files", dir.display())));
    }
    Ok(files)
}

/// The module directory as an installed setup template.
pub fn installed_module(dir: &Path, cloud: ModuleCloud) -> Result<Installed> {
    Ok(Installed {
        template: cloud.template(),
        digest: sha256_digest(&file_map_bytes(&directory_files(dir)?)),
        choices: Choices::NotRead(VARIABLES),
    })
}

/// Read the install's signing settings and ledger from its cloud.
pub async fn read_install(install: &ModuleInstall) -> Result<Signed> {
    let prefix = install.prefix.clone();
    match install.cloud {
        ModuleCloud::Aws => {
            let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .load()
                .await;
            let region = aws
                .region()
                .map(|region| region.to_string())
                .ok_or_else(|| {
                    failed("no AWS region is set; set AWS_REGION or a profile region")
                })?;
            let identity = aws_sdk_sts::Client::new(&aws)
                .get_caller_identity()
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: "GetCallerIdentity failed; check the AWS credentials".to_string(),
                })?;
            let account = identity
                .account()
                .ok_or_else(|| failed("GetCallerIdentity returned no account"))?
                .to_string();
            let id = parse_install(&format!("aws:{account}:{region}:{prefix}"))?;
            let ssm = super::verify::ssm_client_for(&aws, account, region).await?;
            let settings = super::verify::parameter(&ssm, &names::aws_settings(&prefix)).await?;
            let ledger =
                super::verify::ledger_parameter(&ssm, &names::aws_ledger(&prefix), &prefix).await?;
            Ok(signed(
                id,
                install,
                names::aws_settings(&prefix),
                settings,
                names::aws_ledger(&prefix),
                ledger,
            ))
        }
        ModuleCloud::Gcp => {
            let project = install
                .project
                .clone()
                .ok_or_else(|| failed("pass --project: the GCP project of the install"))?;
            let id = parse_install(&format!("gcp:{project}:{prefix}"))?;
            let config =
                GcpClientConfig::from_std_env()
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: "Could not load GCP credentials".to_string(),
                    })?;
            let token = config
                .get_bearer_token("https://parametermanager.googleapis.com/")
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Could not get a GCP access token".to_string(),
                })?;
            let http = reqwest::Client::new();
            let settings_name = names::gcp_settings(&prefix);
            let ledger_name = names::gcp_ledger(&prefix);
            let settings = gcp_parameter(&http, &token, &project, &settings_name).await?;
            let ledger = gcp_secret(&http, &token, &project, &ledger_name).await?;
            Ok(signed(
                id,
                install,
                settings_name,
                settings,
                ledger_name,
                ledger,
            ))
        }
        ModuleCloud::Azure => {
            let subscription = install.subscription.clone().ok_or_else(|| {
                failed("pass --subscription: the Azure subscription of the install")
            })?;
            let resource_group = install.resource_group.clone().ok_or_else(|| {
                failed("pass --resource-group: the Azure resource group of the install")
            })?;
            let vault = install
                .vault
                .clone()
                .ok_or_else(|| failed("pass --vault: the install's inputs Key Vault"))?;
            let id = parse_install(&format!("azure:{subscription}:{resource_group}"))?;
            let config =
                AzureClientConfig::from_std_env()
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: "Could not load Azure credentials".to_string(),
                    })?;
            let tokens = AzureTokenCache::new(config);
            let http = reqwest::Client::new();
            let group_url = format!(
                "https://management.azure.com/subscriptions/{subscription}/resourceGroups/{resource_group}?api-version=2021-04-01"
            );
            let arm = tokens
                .get_bearer_token_with_scope("https://management.azure.com/.default")
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Could not get an Azure Resource Manager token".to_string(),
                })?;
            let settings = get_json(&http, &arm, &group_url, "resourceGroups get")
                .await?
                .map(|group| resource_group_settings(&group))
                .ok_or_else(|| failed(format!("resource group {resource_group} does not exist")))?;
            let vault_token = tokens
                .get_bearer_token_with_scope("https://vault.azure.net/.default")
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Could not get a Key Vault token".to_string(),
                })?;
            let secret_url = format!(
                "https://{vault}.vault.azure.net/secrets/{}?api-version=7.4",
                names::AZURE_LEDGER_SECRET
            );
            let ledger = get_json(&http, &vault_token, &secret_url, "keyvault get")
                .await?
                .map(|secret| key_vault_value(&secret))
                .transpose()?;
            Ok(signed(
                id,
                install,
                format!(
                    "tag {} on resource group {resource_group}",
                    names::AZURE_SETTINGS_TAG
                ),
                settings.map(|bytes| (bytes, None)),
                format!("Key Vault secret {vault}/{}", names::AZURE_LEDGER_SECRET),
                ledger,
            ))
        }
    }
}

fn parse_install(text: &str) -> Result<InstallId> {
    InstallId::parse(text).context(ErrorData::VerifyFailed {
        message: format!("'{text}' is not a valid install"),
    })
}

fn signed(
    install: InstallId,
    module: &ModuleInstall,
    settings_name: String,
    settings: Option<(Vec<u8>, Option<DateTime<Utc>>)>,
    ledger_name: String,
    ledger: Option<Vec<u8>>,
) -> Signed {
    Signed {
        install,
        prefix: module.prefix.clone(),
        settings_name,
        settings,
        ledger_name,
        ledger,
        signer_lists: Vec::new(),
        releases: Vec::new(),
        platform: module.cloud.platform(),
    }
}

/// GET `url`: the status and body.
async fn fetch(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    operation: &str,
) -> Result<(reqwest::StatusCode, String)> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("{operation} failed"),
        })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("{operation}: the response could not be read"),
        })?;
    Ok((status, text))
}

/// The JSON of a response, `None` for 404; any other failure status is an error.
fn json_body(status: reqwest::StatusCode, text: &str, operation: &str) -> Result<Option<Value>> {
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(failed(format!(
            "{operation}: HTTP {status}: {}",
            text.chars().take(500).collect::<String>()
        )));
    }
    serde_json::from_str(text)
        .map(Some)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("{operation}: the response is not JSON"),
        })
}

/// GET `url` as JSON; `None` for 404.
async fn get_json(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    operation: &str,
) -> Result<Option<Value>> {
    let (status, text) = fetch(http, token, url, operation).await?;
    json_body(status, &text, operation)
}

/// The newest version of Parameter Manager parameter `name`, with when it
/// was written; `None` when the parameter doesn't exist or has no version.
async fn gcp_parameter(
    http: &reqwest::Client,
    token: &str,
    project: &str,
    name: &str,
) -> Result<Option<(Vec<u8>, Option<DateTime<Utc>>)>> {
    let base = format!(
        "https://parametermanager.googleapis.com/v1/projects/{project}/locations/global/parameters/{name}"
    );
    let mut versions = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!("{base}/versions?pageSize=100");
        if let Some(token) = &page {
            url.push_str(&format!("&pageToken={token}"));
        }
        let Some(listed) =
            get_json(http, token, &url, &format!("parametermanager list {name}")).await?
        else {
            return Ok(None);
        };
        versions.extend(
            listed["parameterVersions"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        match listed["nextPageToken"].as_str() {
            Some(next) if !next.is_empty() => page = Some(next.to_string()),
            _ => break,
        }
    }
    let Some((version, created)) = newest_version(name, &versions)? else {
        return Ok(None);
    };
    let url = format!("https://parametermanager.googleapis.com/v1/{version}?view=FULL");
    let full = get_json(
        http,
        token,
        &url,
        &format!("parametermanager get {version}"),
    )
    .await?
    .ok_or_else(|| failed(format!("version {version} disappeared")))?;
    Ok(Some((parameter_payload(name, &full)?, Some(created))))
}

const SECRET_MANAGER: &str = "https://secretmanager.googleapis.com";

/// The newest version of Secret Manager secret `name`; `None` when the secret
/// doesn't exist or has no version. A disabled or destroyed newest version
/// fails, as it does in the verifier, rather than reading an older one.
async fn gcp_secret(
    http: &reqwest::Client,
    token: &str,
    project: &str,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    gcp_secret_at(http, SECRET_MANAGER, token, project, name).await
}

async fn gcp_secret_at(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    project: &str,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let url = format!("{base}/v1/projects/{project}/secrets/{name}/versions/latest:access");
    let operation = format!("secretmanager access {name}");
    let (status, text) = fetch(http, token, &url, &operation).await?;
    if status == reqwest::StatusCode::FORBIDDEN {
        return Err(AlienError::new(ErrorData::GcpLedgerAccessDenied {
            project: project.to_string(),
            secret: name.to_string(),
        }));
    }
    let Some(version) = json_body(status, &text, &operation)? else {
        return Ok(None);
    };
    parameter_payload(name, &version).map(Some)
}

/// The newest of `versions` by create time, refusing a disabled newest one:
/// the verifier refuses it too, rather than falling back to an older ledger.
pub(crate) fn newest_version(
    parameter: &str,
    versions: &[Value],
) -> Result<Option<(String, DateTime<Utc>)>> {
    let mut newest: Option<(DateTime<Utc>, &str, bool)> = None;
    for version in versions {
        let created = version["createTime"]
            .as_str()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|created| created.with_timezone(&Utc));
        let (Some(created), Some(name)) = (created, version["name"].as_str()) else {
            return Err(failed(format!(
                "parameter {parameter} lists a version without a name or create time"
            )));
        };
        let disabled = version["disabled"].as_bool().unwrap_or(false);
        if newest.is_none_or(|(newest_created, ..)| created > newest_created) {
            newest = Some((created, name, disabled));
        }
    }
    match newest {
        None => Ok(None),
        Some((_, name, true)) => Err(failed(format!(
            "the newest version of parameter {parameter}, {name}, is disabled, so the install's enforcers refuse to read it"
        ))),
        Some((created, name, false)) => Ok(Some((name.to_string(), created))),
    }
}

/// The bytes a Parameter Manager version or an accessed Secret Manager version holds.
pub(crate) fn parameter_payload(parameter: &str, version: &Value) -> Result<Vec<u8>> {
    let data = version["payload"]["data"]
        .as_str()
        .ok_or_else(|| failed(format!("parameter {parameter} carries no payload")))?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("parameter {parameter} is not base64"),
        })
}

/// The signing settings tag of a resource group, if it has one.
pub(crate) fn resource_group_settings(group: &Value) -> Option<Vec<u8>> {
    group["tags"][names::AZURE_SETTINGS_TAG]
        .as_str()
        .map(|value| value.as_bytes().to_vec())
}

/// The value of a Key Vault secret.
pub(crate) fn key_vault_value(secret: &Value) -> Result<Vec<u8>> {
    secret["value"]
        .as_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| failed("the Key Vault secret has no value"))
}

#[cfg(test)]
#[path = "verify_terraform_tests.rs"]
pub(crate) mod tests;
