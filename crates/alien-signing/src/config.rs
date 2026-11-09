//! `config/v1`: developer config for one install.

use std::collections::BTreeMap;

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::envelope::Kind;
use crate::error::{ErrorData, Result};
use crate::ledger::Entry;
use crate::release::{is_loader_env, ReleaseBody};

/// `config/v1`: developer config for one install.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigBody {
    /// Environment values, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Developer input values, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, serde_json::Value>,
    /// Values of bindings to resources outside the customer's cloud, by
    /// resource id. Used only for bindings the enforcer can neither derive
    /// nor find in its own install.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, serde_json::Value>,
    /// Where workloads send telemetry, if anywhere. The control plane can't
    /// point it elsewhere: it is part of the signed config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_endpoint: Option<String>,
    /// The config this one was made from. Whoever signs a change starts from
    /// the newest config the server shows them; naming it here lets the
    /// install refuse a change made on an older one the control plane
    /// served instead, which would silently undo what newer configs set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<ConfigBase>,
}

/// A config statement, by sequence and digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigBase {
    /// Its sequence.
    pub sequence: u64,
    /// `sha256:<hex>` of its payload.
    pub digest: String,
}

impl ConfigBody {
    /// Refuse a config that doesn't build on the install's current config.
    /// It must name a base once the install accepted a config, and the base
    /// can't be older than that config or a different statement at its
    /// sequence. A newer base is fine: the install may have missed the
    /// configs in between.
    pub fn check_base(&self, floor: Option<&Entry>) -> Result<()> {
        let Some(floor) = floor else {
            return Ok(());
        };
        let Some(base) = &self.base else {
            return Err(refused(
                Kind::Config,
                format!(
                    "this install accepted config sequence {} and this one doesn't say which config it was made from",
                    floor.sequence
                ),
            ));
        };
        if base.sequence < floor.sequence
            || (base.sequence == floor.sequence && base.digest != floor.digest)
        {
            return Err(refused(
                Kind::Config,
                format!(
                    "it was made from config sequence {}, but this install runs config sequence {}",
                    base.sequence, floor.sequence
                ),
            ));
        }
        Ok(())
    }

    /// Refuse any name the release doesn't make configurable.
    pub fn check(&self, release: &ReleaseBody) -> Result<()> {
        for name in self.env.keys() {
            if is_loader_env(name) || !release.configurable.env.contains(name) {
                return Err(refused(
                    Kind::Config,
                    format!("env {name} is not configurable in the signed release"),
                ));
            }
        }
        for id in self.inputs.keys() {
            if !release.configurable.inputs.contains(id) {
                return Err(refused(
                    Kind::Config,
                    format!("input '{id}' is not configurable in the signed release"),
                ));
            }
        }
        for id in self.bindings.keys() {
            if !release
                .workloads
                .iter()
                .any(|workload| workload.bindings.contains_key(id))
            {
                return Err(refused(
                    Kind::Config,
                    format!("binding '{id}' is not a binding of the signed release"),
                ));
            }
        }
        if let Some(endpoint) = &self.telemetry_endpoint {
            if !endpoint.starts_with("https://") {
                return Err(refused(
                    Kind::Config,
                    format!("telemetry endpoint '{endpoint}' is not an https URL"),
                ));
            }
        }
        Ok(())
    }
}

fn refused(kind: Kind, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BodyRefused {
        kind: kind.as_str().to_string(),
        reason: reason.into(),
    })
}
