//! Binding values an enforcer computes from its own install.
//!
//! A binding tells a workload which resource to use, such as the bucket
//! behind `ALIEN_UPLOADS_BINDING`. On a protected install the control plane
//! must not choose it, or it could point the vendor's code at another
//! customer's bucket. Setup names each owned resource `<prefix>-<id>`, so the
//! value follows from the install's prefix, account and region. The engine
//! and every enforcer use this one function, and a test in the engine checks
//! that its controllers produce the same values.

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

/// The value of every binding in `bindings` (resource id to resource type),
/// by environment name. A binding whose value doesn't follow from the
/// install is refused: on a protected install nothing else may supply it.
pub fn binding_env(
    bindings: &BTreeMap<String, String>,
    context: &BindingContext,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
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
            ("aws", "kv") => json!({ "service": "dynamodb", "tableName": name, "region": region()? }),
            ("aws", "queue") => json!({
                "service": "sqs",
                "queueUrl": format!("https://sqs.{}.amazonaws.com/{}/{name}", region()?, context.account),
            }),
            ("aws", "vault") => json!({ "service": "parameter-store", "vaultPrefix": name }),
            ("gcp", "storage") => json!({ "service": "gcs", "bucketName": name }),
            ("gcp", "vault") => json!({ "service": "secret-manager", "vaultPrefix": name }),
            _ => {
                return Err(refused(
                    id,
                    resource_type,
                    context,
                    "its value doesn't follow from the install, so a protected install can't use it yet",
                ))
            }
        };
        env.insert(binding_env_name(id), value.to_string());
    }
    Ok(env)
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
