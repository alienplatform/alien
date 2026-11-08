//! Scopes, slots and records for [`super::ValueStore`].

use std::fmt;
use std::str::FromStr;

use alien_error::AlienError;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Largest value the manager stores, in bytes.
///
/// The key-value bindings accept 24,576 bytes per record; the rest is left
/// for the record header.
pub const MAX_VALUE_BYTES: usize = 24_000;

const MAX_ID_LEN: usize = 128;
const MAX_CONTAINER_NAME_LEN: usize = 63;

/// Who a group of values belongs to: `dep:<deploymentId>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueScope(String);

impl ValueScope {
    /// The scope of a deployment's values.
    pub fn deployment(deployment_id: &str) -> Result<Self> {
        format!("dep:{deployment_id}").parse()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The deployment this scope belongs to.
    pub fn deployment_id(&self) -> &str {
        &self.0["dep:".len()..]
    }
}

impl FromStr for ValueScope {
    type Err = AlienError<ErrorData>;

    fn from_str(s: &str) -> Result<Self> {
        if !s.strip_prefix("dep:").is_some_and(valid_id) {
            return Err(ErrorData::bad_request(format!(
                "'{s}' is not a value scope; use dep:<deploymentId>"
            )));
        }
        Ok(Self(s.to_string()))
    }
}

impl fmt::Display for ValueScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One value's name inside a scope: `env:<NAME>`, `input:<id>` or
/// `cenv.<containerName>.<NAME>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueSlot(String);

impl ValueSlot {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The variable name, for `env:` slots.
    pub fn env_name(&self) -> Option<&str> {
        self.0.strip_prefix("env:")
    }

    /// The input id, for `input:` slots.
    pub fn input_id(&self) -> Option<&str> {
        self.0.strip_prefix("input:")
    }

    /// The slot of one environment variable of a dynamic container.
    pub fn container_env(container: &str, name: &str) -> Result<Self> {
        format!("cenv.{container}.{name}").parse()
    }

    /// The container name and variable name, for `cenv.` slots.
    ///
    /// Variable names have no '.', and container names may, so the split is
    /// at the last '.'.
    pub fn container_env_parts(&self) -> Option<(&str, &str)> {
        split_container_env(&self.0)
    }
}

fn split_container_env(slot: &str) -> Option<(&str, &str)> {
    let rest = slot.strip_prefix("cenv.")?;
    let (container, name) = rest.rsplit_once('.')?;
    (valid_container_name(container) && valid_env_name(name)).then_some((container, name))
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_ID_LEN
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A dynamic container's name is kept as a Kubernetes label value: at most
/// 63 characters of letters, digits, '-', '_' and '.', starting and ending
/// with a letter or digit.
fn valid_container_name(name: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    name.len() <= MAX_CONTAINER_NAME_LEN
        && edge(name.chars().next())
        && edge(name.chars().last())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl FromStr for ValueSlot {
    type Err = AlienError<ErrorData>;

    fn from_str(s: &str) -> Result<Self> {
        let valid = if let Some(name) = s.strip_prefix("env:") {
            valid_env_name(name)
        } else if let Some(id) = s.strip_prefix("input:") {
            valid_id(id)
        } else {
            split_container_env(s).is_some()
        };
        if !valid {
            return Err(ErrorData::bad_request(format!(
                "'{s}' is not a value slot; use env:<NAME>, input:<id> or cenv.<containerName>.<NAME>"
            )));
        }
        Ok(Self(s.to_string()))
    }
}

impl fmt::Display for ValueSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

macro_rules! string_serde {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> std::result::Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse()
                    .map_err(|e: AlienError<ErrorData>| serde::de::Error::custom(e.message))
            }
        }
    };
}

string_serde!(ValueScope);
string_serde!(ValueSlot);

/// Which slots a request touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotSelection {
    /// Exactly these slots.
    Slots(Vec<ValueSlot>),
    /// Every slot in the scope.
    All,
}

/// Whether a value is shown back to people. Secrets never are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    Plain,
    Secret,
}

/// A stored value.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredValue {
    pub slot: ValueSlot,
    pub kind: ValueKind,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for StoredValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredValue")
            .field("slot", &self.slot)
            .field("kind", &self.kind)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

impl StoredValue {
    /// What other services may hold about this value.
    pub fn meta(&self, scope: &ValueScope) -> Result<ValueMeta> {
        Ok(ValueMeta {
            slot: self.slot.clone(),
            kind: self.kind,
            digest: self.digest(scope)?,
            updated_at: self.updated_at,
        })
    }

    /// Keyed digest of the value in `scope`; see [`alien_deployment::value_digest`].
    pub fn digest(&self, scope: &ValueScope) -> Result<String> {
        alien_deployment::value_digest(scope.as_str(), self.slot.as_str(), &self.value)
            .map_err(|e| ErrorData::internal(format!("Could not digest value: {}", e.message)))
    }

    /// Compact record: version, kind, update time in milliseconds, value.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(10 + self.value.len());
        out.push(RECORD_VERSION);
        out.push(match self.kind {
            ValueKind::Plain => 0,
            ValueKind::Secret => 1,
        });
        out.extend_from_slice(&self.updated_at.timestamp_millis().to_be_bytes());
        out.extend_from_slice(self.value.as_bytes());
        out
    }

    pub(crate) fn decode(slot: ValueSlot, bytes: &[u8]) -> Result<Self> {
        let corrupt =
            |why: &str| ErrorData::internal(format!("Stored value '{slot}' is damaged: {why}"));
        let [version, kind, rest @ ..] = bytes else {
            return Err(corrupt("too short"));
        };
        if *version != RECORD_VERSION {
            return Err(corrupt("unknown record version"));
        }
        let kind = match kind {
            0 => ValueKind::Plain,
            1 => ValueKind::Secret,
            _ => return Err(corrupt("unknown kind")),
        };
        let (millis, value) = rest
            .split_first_chunk::<8>()
            .ok_or_else(|| corrupt("too short"))?;
        let updated_at = Utc
            .timestamp_millis_opt(i64::from_be_bytes(*millis))
            .single()
            .ok_or_else(|| corrupt("bad time"))?;
        let value = String::from_utf8(value.to_vec()).map_err(|_| corrupt("not text"))?;
        Ok(Self {
            slot,
            kind,
            value,
            updated_at,
        })
    }
}

const RECORD_VERSION: u8 = 1;

/// What the manager reports about a stored value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueMeta {
    pub slot: ValueSlot,
    pub kind: ValueKind,
    /// Keyed digest of the value, lowercase hex. Opaque to readers.
    pub digest: String,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_and_slots_accept_the_documented_forms_only() {
        for ok in ["dep:dep_abc123", "dep:a-b.c"] {
            assert!(ok.parse::<ValueScope>().is_ok(), "{ok}");
        }
        for bad in [
            "",
            "dep:",
            "dep:a:b",
            "dep:a/b",
            "other:x",
            "link:key1",
            "reg:op.1",
            "dep_x",
            "dep:a b",
        ] {
            assert!(bad.parse::<ValueScope>().is_err(), "{bad}");
        }
        for ok in [
            "env:API_TOKEN",
            "env:_x1",
            "input:db-url",
            "input:a.b_c",
            "cenv.api.TOKEN",
            "cenv.a.b.X",
            "cenv.my-app_1.v2._X",
            "cenv.a.A1",
            "cenv.a.X.Y",
        ] {
            assert!(ok.parse::<ValueSlot>().is_ok(), "{ok}");
        }
        for bad in [
            "env:",
            "env:1X",
            "env:A-B",
            "env:A B",
            "input:",
            "input:a:b",
            "x:y",
            "API",
            "cenv.",
            "cenv.X",
            "cenv..X",
            "cenv.a.",
            "cenv.a.1X",
            "cenv.a.X-Y",
            "cenv.a:b.X",
            "cenv.-a.X",
            "cenv.a-.X",
            "cenv.a b.X",
            "cenv:a:X",
            "cenv.a/b.X",
        ] {
            assert!(bad.parse::<ValueSlot>().is_err(), "{bad}");
        }
        assert!(format!("env:{}", "A".repeat(129))
            .parse::<ValueSlot>()
            .is_err());
        assert!(format!("cenv.{}.X", "a".repeat(63))
            .parse::<ValueSlot>()
            .is_ok());
        assert!(format!("cenv.{}.X", "a".repeat(64))
            .parse::<ValueSlot>()
            .is_err());
        assert!(format!("cenv.a.{}", "A".repeat(129))
            .parse::<ValueSlot>()
            .is_err());
    }

    #[test]
    fn a_container_env_slot_splits_at_the_last_dot() {
        let parts = |slot: &str| {
            let slot: ValueSlot = slot.parse().unwrap();
            slot.container_env_parts()
                .map(|(container, name)| (container.to_string(), name.to_string()))
        };
        assert_eq!(parts("cenv.a.X"), Some(("a".into(), "X".into())));
        assert_eq!(
            parts("cenv.web-1.TOKEN"),
            Some(("web-1".into(), "TOKEN".into()))
        );
        assert_eq!(parts("cenv.a.b.X"), Some(("a.b".into(), "X".into())));
        assert_eq!(parts("cenv.a.b.c.X"), Some(("a.b.c".into(), "X".into())));
        assert_eq!(parts("env:X"), None);
        assert_eq!(parts("input:a.b"), None);
        assert_eq!(
            ValueSlot::container_env("a.b", "X").unwrap().as_str(),
            "cenv.a.b.X"
        );
        assert!(ValueSlot::container_env("a b", "X").is_err());
        assert!(ValueSlot::container_env("a", "X-Y").is_err());
    }

    #[test]
    fn records_round_trip() {
        let slot: ValueSlot = "env:A".parse().unwrap();
        let original = StoredValue {
            slot: slot.clone(),
            kind: ValueKind::Secret,
            value: "päss\"word\n".into(),
            updated_at: Utc.timestamp_millis_opt(1_762_000_000_123).unwrap(),
        };
        let decoded = StoredValue::decode(slot.clone(), &original.encode()).unwrap();
        assert_eq!(decoded, original);

        for broken in [
            &[][..],
            &[1][..],
            &[9, 0, 0, 0, 0, 0, 0, 0, 0, 0][..],
            &[1, 7, 0, 0, 0, 0, 0, 0, 0, 0][..],
        ] {
            assert!(StoredValue::decode(slot.clone(), broken).is_err());
        }
    }

    #[test]
    fn debug_output_never_includes_the_value() {
        let stored = StoredValue {
            slot: "env:A".parse().unwrap(),
            kind: ValueKind::Secret,
            value: "hunter2".into(),
            updated_at: Utc::now(),
        };
        assert!(!format!("{stored:?}").contains("hunter2"));
    }
}
