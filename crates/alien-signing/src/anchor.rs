//! The anchor: what the customer's setup pins for one deployment.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::key::PublicKey;

/// Who must approve an exec or debug session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionApproval {
    /// A developer signer with the `operate` role.
    #[default]
    #[serde(rename = "developer")]
    Developer,
    /// The developer, and then the customer through their own inputs store.
    #[serde(rename = "developer+customer")]
    DeveloperAndCustomer,
}

/// The anchor, written once by setup and never by the control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Anchor {
    /// Project the deployment belongs to.
    pub project: String,
    /// This deployment.
    pub deployment: String,
    /// Whether enforcers refuse what isn't signed.
    pub protected: bool,
    /// `SHA256:<hex>` fingerprint of the project's signer list version 1.
    pub signing: String,
    /// `ecdsa-p256:<SPKI>` of the key that signs enforcer and component builds.
    pub components: String,
    /// Who approves exec sessions.
    #[serde(default)]
    pub session_approval: SessionApproval,
    /// Where deployer inputs live, in the platform's own terms (a parameter
    /// path, a secret name, a file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<String>,
    /// Prefix of the names, and value of the `deployment` tag, of what this
    /// deployment creates in the cloud. Enforcers attach only volumes that
    /// carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_prefix: Option<String>,
}

impl Anchor {
    /// Parse an anchor and check that each field is usable, so a broken
    /// anchor fails at read time rather than at the first statement.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let anchor: Anchor =
            serde_json::from_slice(bytes).map_err(|error| invalid(format!("{error}")))?;
        anchor.validate()?;
        Ok(anchor)
    }

    /// Check each field.
    pub fn validate(&self) -> Result<()> {
        if self.project.is_empty() || self.deployment.is_empty() {
            return Err(invalid("project and deployment must not be empty"));
        }
        let fingerprint_ok = self
            .signing
            .strip_prefix("SHA256:")
            .is_some_and(|hex_part| {
                hex_part.len() == 64
                    && hex_part
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            });
        if !fingerprint_ok {
            return Err(invalid(format!(
                "signing '{}' is not SHA256:<64 lowercase hex>",
                self.signing
            )));
        }
        PublicKey::parse(&self.components)
            .map_err(|error| invalid(format!("components: {}", error.message)))?;
        Ok(())
    }

    /// The components key.
    pub fn components_key(&self) -> Result<PublicKey> {
        PublicKey::parse(&self.components)
    }
}

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::AnchorInvalid {
        reason: reason.into(),
    })
}
