//! Binding values an enforcer computes from its own install.
//!
//! A binding tells a workload which resource to use, such as the bucket
//! behind `ALIEN_UPLOADS_BINDING`. On a protected install the control plane
//! must not choose it, or it could point the vendor's code at another
//! customer's bucket. Each value comes from the first of:
//!
//! 1. [`binding_env`]: setup names each owned resource `<prefix>-<id>`, so for
//!    most types the value follows from the install's prefix, account and
//!    region. The engine and every enforcer use this one function, and a test
//!    in the engine checks that its controllers produce the same values.
//! 2. The enforcer's own cloud reads, for types whose value isn't a name
//!    (a database endpoint, a service URL): it describes the resource it
//!    finds by the derived name or setup's tag in its own install.
//! 3. A signed `config/v1`, for resources outside the customer's cloud.
//!
//! [`resolve_bindings`] applies that order. The control plane supplies none of
//! them.

use std::collections::BTreeMap;

use alien_error::AlienError;
use serde_json::json;

use crate::error::{ErrorData, Result};

/// Where an enforcer runs, as it learned from its own identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingContext {
    /// `aws`, `gcp`, `azure`, `kubernetes` or `machines`.
    pub platform: String,
    /// The install's resource prefix.
    pub prefix: String,
    /// AWS account id or GCP project id.
    pub account: String,
    /// Region, where the platform has one.
    pub region: Option<String>,
}

/// The environment variable that carries binding `id`.
pub fn binding_env_name(id: &str) -> String {
    format!("ALIEN_{}_BINDING", id.replace('-', "_").to_uppercase())
}

/// Binding values derived from the install, and the bindings that need
/// another source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    /// Values by environment name.
    pub values: BTreeMap<String, String>,
    /// Resource id to type, for bindings whose value doesn't follow from the
    /// install's names.
    pub unresolved: BTreeMap<String, String>,
}

/// The value of every binding in `bindings` (resource id to resource type)
/// that follows from the install, by environment name.
pub fn binding_env(
    bindings: &BTreeMap<String, String>,
    context: &BindingContext,
) -> Result<Bindings> {
    let mut derived = Bindings::default();
    for (id, resource_type) in bindings {
        let name = format!("{}-{id}", context.prefix);
        let region = || {
            context
                .region
                .clone()
                .ok_or_else(|| refused(id, resource_type, context, "the enforcer knows no region"))
        };
        let value = match (context.platform.as_str(), resource_type.as_str()) {
            ("aws", "storage") => json!({ "service": "s3", "bucketName": name }),
            ("aws", "kv") => {
                json!({ "service": "dynamodb", "tableName": name, "region": region()? })
            }
            ("aws", "queue") => json!({
                "service": "sqs",
                "queueUrl": format!("https://sqs.{}.amazonaws.com/{}/{name}", region()?, context.account),
            }),
            ("aws", "vault") => json!({ "service": "parameter-store", "vaultPrefix": name }),
            ("gcp", "storage") => json!({ "service": "gcs", "bucketName": name }),
            ("gcp", "vault") => json!({ "service": "secret-manager", "vaultPrefix": name }),
            ("azure", "vault") => json!({ "service": "key-vault", "vaultName": name }),
            _ => {
                derived.unresolved.insert(id.clone(), resource_type.clone());
                continue;
            }
        };
        derived
            .values
            .insert(binding_env_name(id), canonical(&value));
    }
    Ok(derived)
}

/// Every binding's value, by environment name: derived values first, then
/// what the enforcer read from its own cloud (by resource id), then values a
/// verified `config/v1` carries (by resource id). A source never overrides an
/// earlier one, and a binding none of them covers is refused by name.
pub fn resolve_bindings(
    derived: Bindings,
    read: &BTreeMap<String, String>,
    signed_config: &BTreeMap<String, serde_json::Value>,
) -> Result<BTreeMap<String, String>> {
    let mut values = derived.values;
    for (id, resource_type) in &derived.unresolved {
        let value = match (read.get(id), signed_config.get(id)) {
            (Some(value), _) => value.clone(),
            (None, Some(value)) => canonical(value),
            (None, None) => {
                return Err(AlienError::new(ErrorData::WorkloadRefused {
                    workload: id.clone(),
                    reason: format!(
                        "binding '{id}' ({resource_type}) has no value: it doesn't follow from the install's names, the enforcer found no such resource in its install, and no signed config sets it"
                    ),
                }))
            }
        };
        values.insert(binding_env_name(id), value);
    }
    Ok(values)
}

/// JSON with object keys sorted, so a binding's value is the same in every
/// enforcer build, whichever `serde_json` features the build enables.
fn canonical(value: &serde_json::Value) -> String {
    fn sorted(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let keys: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
                serde_json::Value::Object(
                    keys.into_iter()
                        .map(|(key, value)| (key.clone(), sorted(value)))
                        .collect(),
                )
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sorted).collect())
            }
            other => other.clone(),
        }
    }
    sorted(value).to_string()
}

fn refused(
    id: &str,
    resource_type: &str,
    context: &BindingContext,
    reason: &str,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::WorkloadRefused {
        workload: id.to_string(),
        reason: format!(
            "binding '{id}' ({resource_type} on {}): {reason}",
            context.platform
        ),
    })
}
