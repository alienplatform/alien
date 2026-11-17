//! `verify --offline`: check local setup files against signed files, with no cloud or network
//! reads. For reviewing a template before you apply it, and for checking what a hosting service
//! produced against the public renderer.
//!
//! What it checks is what `verify` checks of a template: the renderer version and hosting inputs
//! the template records, the signed minimum renderer version, and the bytes against a render of
//! the signed stack. What an install holds (its signing settings, ledger and the parameters or
//! values the customer chose) doesn't exist yet, so the settings come from the flags and the
//! customer's choices aren't checked.

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{Ledger, SignedStatement, SigningSettings};
use chrono::Utc;

use super::verify::{check_signed, failed, line, render, Choices, Content, Installed, Line, Mark, Signed};
use super::verify_terraform::directory_files;
use crate::error::{ErrorData, Result};

/// The prefix and install an offline check gives its stand-in signing settings and ledger.
const PREFIX: &str = "offline";
const INSTALL: &str = "aws:000000000000:offline:offline";

/// The setup a local template is.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineSetup {
    /// A CloudFormation template file.
    Cloudformation,
    /// A Terraform module directory for AWS.
    TerraformAws,
    /// A Terraform module directory for GCP.
    TerraformGcp,
    /// A Terraform module directory for Azure.
    TerraformAzure,
    /// A Helm chart directory.
    Helm,
}

impl OfflineSetup {
    fn template(self) -> &'static str {
        match self {
            Self::Cloudformation => "cloudformation",
            Self::TerraformAws => "terraform-aws",
            Self::TerraformGcp => "terraform-gcp",
            Self::TerraformAzure => "terraform-azure",
            Self::Helm => "helm",
        }
    }

    fn platform(self) -> &'static str {
        match self {
            Self::Cloudformation | Self::TerraformAws => "aws",
            Self::TerraformGcp => "gcp",
            Self::TerraformAzure => "azure",
            Self::Helm => "kubernetes",
        }
    }
}

/// What `verify --offline` reads.
pub struct OfflineInputs {
    pub setup: OfflineSetup,
    pub template: PathBuf,
    pub fingerprint: String,
    pub channel: String,
    pub signer_lists: Vec<PathBuf>,
    pub releases: Vec<PathBuf>,
}

fn statement(path: &Path) -> Result<SignedStatement> {
    let bytes = std::fs::read(path)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("could not read {}", path.display()),
        })?;
    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: format!("{} is not a signed statement", path.display()),
        })
}

/// Check the local template and print the screen; a failed line fails the command.
pub async fn verify_offline(inputs: &OfflineInputs) -> Result<()> {
    let lines = check_offline(inputs).await?;
    print!("{}", render(&lines));
    if lines.iter().any(|line| line.mark == Mark::Fail) {
        return Err(failed(
            "this template does not match what the vendor signed; don't apply it",
        ));
    }
    Ok(())
}

pub(crate) async fn check_offline(inputs: &OfflineInputs) -> Result<Vec<Line>> {
    let content = match inputs.setup {
        OfflineSetup::Cloudformation => {
            let text = std::fs::read_to_string(&inputs.template)
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: format!("could not read {}", inputs.template.display()),
                })?;
            Content::Yaml(text)
        }
        _ => Content::Files(directory_files(&inputs.template)?),
    };
    let installed = Installed {
        template: inputs.setup.template(),
        content,
        choices: Choices::NotRead("not checked: nothing is installed yet"),
    };
    let signer_lists = inputs
        .signer_lists
        .iter()
        .map(|path| statement(path))
        .collect::<Result<Vec<_>>>()?;
    let releases = inputs
        .releases
        .iter()
        .map(|path| {
            let id = path
                .file_stem()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            Ok((id, statement(path)?))
        })
        .collect::<Result<Vec<_>>>()?;
    let settings = SigningSettings {
        fingerprint: inputs.fingerprint.clone(),
        prefix: PREFIX.to_string(),
        channel: inputs.channel.clone(),
    };
    settings.validate().context(ErrorData::VerifyFailed {
        message: "--fingerprint or --channel is not valid".to_string(),
    })?;
    let signed = Signed {
        install: alien_signing::InstallId::parse(INSTALL)
            .context(ErrorData::VerifyFailed {
                message: "the offline install id is not valid".to_string(),
            })?,
        prefix: PREFIX.to_string(),
        settings_name: "--fingerprint".to_string(),
        settings: Some((
            serde_json::to_vec(&settings)
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: "the settings could not be written".to_string(),
                })?,
            None,
        )),
        ledger_name: "the offline ledger".to_string(),
        ledger: Some(
            serde_json::to_vec(&Ledger::new(PREFIX))
                .into_alien_error()
                .context(ErrorData::VerifyFailed {
                    message: "the ledger could not be written".to_string(),
                })?,
        ),
        signer_lists,
        releases,
        platform: inputs.setup.platform(),
    };
    let mut lines = vec![line(
        "Mode",
        "offline: nothing was read from a cloud or from the vendor's control plane",
        Mark::Info,
    )];
    lines.extend(check_signed(&signed, &installed, Vec::new(), Utc::now()).await?);
    Ok(lines)
}


#[cfg(test)]
#[path = "verify_offline_tests.rs"]
mod tests;
