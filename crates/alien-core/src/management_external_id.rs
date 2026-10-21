//! The `sts:ExternalId` a control plane sends when it assumes a deployment's
//! AWS management roles.
//!
//! A control plane that manages many customers through one IAM identity is a
//! confused deputy unless each customer's role demands a value only that
//! customer's installation knows. The control plane generates the value, the
//! customer's setup template puts it in the role trust policy, and every
//! AssumeRole of that role sends it.

use crate::{ErrorData, Result};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};

/// Shortest accepted value. Long enough that a random value cannot be guessed.
pub const MANAGEMENT_EXTERNAL_ID_MIN_LENGTH: usize = 32;
/// Longest accepted value.
pub const MANAGEMENT_EXTERNAL_ID_MAX_LENGTH: usize = 128;
/// CloudFormation `AllowedPattern` for the setup parameter. Empty keeps the
/// role trust unchanged, so stacks created before the parameter existed can be
/// updated without one.
pub const MANAGEMENT_EXTERNAL_ID_PARAMETER_PATTERN: &str = "^$|^[A-Za-z0-9]{32,128}$";
/// CloudFormation `AllowedPattern` for the setup parameter when a template
/// requires it: empty is refused, so every install and update sets the trust
/// condition.
pub const MANAGEMENT_EXTERNAL_ID_REQUIRED_PARAMETER_PATTERN: &str = "^[A-Za-z0-9]{32,128}$";

/// A validated management ExternalId.
///
/// `Debug` never prints the value and there is no `Display`, so it cannot end
/// up in logs or error messages by accident. Read it with [`Self::expose`] at
/// the AssumeRole call.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ManagementExternalId(String);

impl ManagementExternalId {
    /// Validate a value: 32 to 128 ASCII letters and digits.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let length_ok = (MANAGEMENT_EXTERNAL_ID_MIN_LENGTH..=MANAGEMENT_EXTERNAL_ID_MAX_LENGTH)
            .contains(&value.len());
        if !length_ok || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(AlienError::new(ErrorData::ManagementExternalIdInvalid {
                length: value.len(),
            }));
        }
        Ok(Self(value))
    }

    /// The raw value, for the AssumeRole request only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ManagementExternalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagementExternalId([REDACTED])")
    }
}

impl TryFrom<String> for ManagementExternalId {
    type Error = AlienError<ErrorData>;

    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<ManagementExternalId> for String {
    fn from(value: ManagementExternalId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUE: &str = "0123456789abcdef0123456789ABCDEF";

    #[test]
    fn accepts_the_values_the_template_parameter_accepts() {
        let pattern = regex::Regex::new(MANAGEMENT_EXTERNAL_ID_PARAMETER_PATTERN).unwrap();
        for candidate in [
            VALUE.to_string(),
            "a".repeat(128),
            "a".repeat(31),
            "a".repeat(129),
            format!("{}-", &VALUE[..31]),
            format!("{VALUE} "),
        ] {
            assert_eq!(
                ManagementExternalId::new(candidate.clone()).is_ok(),
                pattern.is_match(&candidate),
                "validation and the CloudFormation pattern disagree on {} characters",
                candidate.len()
            );
        }
    }

    #[test]
    fn the_required_pattern_accepts_exactly_the_valid_values() {
        let pattern = regex::Regex::new(MANAGEMENT_EXTERNAL_ID_REQUIRED_PARAMETER_PATTERN).unwrap();
        for candidate in [
            String::new(),
            VALUE.to_string(),
            "a".repeat(128),
            "a".repeat(31),
            "a".repeat(129),
            format!("{}-", &VALUE[..31]),
            format!("{VALUE} "),
        ] {
            assert_eq!(
                ManagementExternalId::new(candidate.clone()).is_ok(),
                pattern.is_match(&candidate),
                "validation and the required CloudFormation pattern disagree on {} characters",
                candidate.len()
            );
        }
    }

    #[test]
    fn debug_and_errors_never_contain_the_value() {
        let id = ManagementExternalId::new(VALUE).unwrap();
        assert!(!format!("{id:?}").contains(VALUE));
        assert!(!format!("{:?}", Some(&id)).contains(VALUE));

        let rejected = format!("{VALUE}!");
        let error = ManagementExternalId::new(rejected.clone()).unwrap_err();
        assert!(!error.to_string().contains(&rejected));
        assert!(!format!("{error:?}").contains(VALUE));
    }

    #[test]
    fn round_trips_as_a_plain_string_and_validates_on_read() {
        let id = ManagementExternalId::new(VALUE).unwrap();
        assert_eq!(serde_json::to_value(&id).unwrap(), serde_json::json!(VALUE));
        assert_eq!(
            serde_json::from_value::<ManagementExternalId>(serde_json::json!(VALUE)).unwrap(),
            id
        );
        assert!(
            serde_json::from_value::<ManagementExternalId>(serde_json::json!("short")).is_err()
        );
    }
}
