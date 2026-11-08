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

/// What an enforcer knows about the resource a workload runs as, to check the workload's own
/// binding against. The binding tells the workload's code where to reach itself, so every field
/// that picks a destination has to come from the install, not from the control plane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnBindingFacts {
    /// Where the enforcer runs.
    pub context: Option<BindingContext>,
    /// The function, service, Container App or machine container the binding must name.
    pub resource_name: String,
    /// The Azure resource group of the install.
    pub resource_group: Option<String>,
    /// The Cloud Run location the service is in: the one the enforcer is configured with.
    pub location: Option<String>,
    /// The addresses the enforcer reads from the cloud for the resource: the only ones any URL
    /// field (`privateUrl`, `publicUrl`, a Lambda function's `url`) may carry. A name the
    /// control plane picked, such as a custom or generated domain, is in none of them.
    pub urls: Vec<String>,
}

/// Check `value`, a workload's own binding, against `facts`: the binding of the platform the
/// workload runs on, naming the workload's own resource and nothing else, with no field the
/// install doesn't decide and every URL one of `facts.urls`.
pub fn check_own_binding(value: &str, facts: &OwnBindingFacts) -> std::result::Result<(), String> {
    let context = facts
        .context
        .as_ref()
        .ok_or("the enforcer knows no install to check it against")?;
    let binding: serde_json::Value = serde_json::from_str(value)
        .map_err(|_| "the own binding isn't a JSON object".to_string())?;
    let object = binding
        .as_object()
        .ok_or("the own binding isn't a JSON object")?;
    let text = |key: &str| object.get(key).and_then(|value| value.as_str());
    let expect = |key: &str, expected: &str| match text(key) {
        Some(found) if found == expected => Ok(()),
        _ => Err(format!(
            "its {key} is {}, not {expected}",
            object
                .get(key)
                .map_or("missing".to_string(), |v| v.to_string())
        )),
    };
    let url = |key: &str, required: bool| match object.get(key) {
        None if !required => Ok(()),
        None => Err(format!("it has no {key}")),
        Some(found) => {
            let found = found
                .as_str()
                .ok_or_else(|| format!("its {key} isn't a URL"))?;
            let same = |url: &String| url.trim_end_matches('/') == found.trim_end_matches('/');
            if facts.urls.iter().any(same) {
                Ok(())
            } else {
                Err(format!(
                    "its {key} is {found}, which is no URL the cloud reports for {}",
                    facts.resource_name
                ))
            }
        }
    };
    let only = |keys: &[&str]| {
        for key in object.keys() {
            if !keys.contains(&key.as_str()) {
                return Err(format!(
                    "it has a field {key} the platform's binding doesn't"
                ));
            }
        }
        Ok(())
    };
    let name = facts.resource_name.as_str();
    match context.platform.as_str() {
        "aws" => {
            only(&["service", "workerName", "region", "url"])?;
            expect("service", "lambda")?;
            expect("workerName", name)?;
            expect(
                "region",
                context
                    .region
                    .as_deref()
                    .ok_or("the enforcer knows no region")?,
            )?;
            // Lambda is invoked by name through the cloud API. The enforcer reads no address for
            // it, so the binding carries none: an API Gateway address can't be told from one the
            // control plane created.
            url("url", false)?;
        }
        "gcp" => {
            only(&[
                "service",
                "projectId",
                "serviceName",
                "location",
                "privateUrl",
                "publicUrl",
            ])?;
            expect("service", "cloudrun")?;
            expect("projectId", &context.account)?;
            expect("serviceName", name)?;
            expect(
                "location",
                facts
                    .location
                    .as_deref()
                    .ok_or("the enforcer found no such service to read its location from")?,
            )?;
            url("privateUrl", true)?;
            url("publicUrl", false)?;
        }
        "azure" => {
            only(&[
                "service",
                "subscriptionId",
                "resourceGroupName",
                "containerAppName",
                "privateUrl",
                "publicUrl",
            ])?;
            expect("service", "containerapp")?;
            expect("subscriptionId", &context.account)?;
            expect(
                "resourceGroupName",
                facts
                    .resource_group
                    .as_deref()
                    .ok_or("the enforcer knows no resource group")?,
            )?;
            expect("containerAppName", name)?;
            url("privateUrl", true)?;
            url("publicUrl", false)?;
        }
        "machines" => {
            only(&[
                "service",
                "containerName",
                "internalUrl",
                "internalUrls",
                "publicUrl",
            ])?;
            // The backend's name, not an address: any plain identifier is fine.
            if let Some(service) = object.get("service") {
                let plain = service.as_str().is_some_and(|found| {
                    !found.is_empty()
                        && found.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                });
                if !plain {
                    return Err(format!("field service is {service}, not a plain name"));
                }
            }
            expect("containerName", name)?;
            // A machine container is reached at `<name>.svc` inside its cluster.
            let internal = |found: &serde_json::Value, key: &str| {
                let found = found.as_str().unwrap_or_default();
                let parsed = url::Url::parse(found).ok().filter(|parsed| {
                    parsed.scheme() == "http"
                        && parsed.host_str() == Some(&format!("{name}.svc"))
                        && parsed.port().is_some()
                        && parsed.username().is_empty()
                        && parsed.password().is_none()
                        && matches!(parsed.path(), "" | "/")
                        && parsed.query().is_none()
                        && parsed.fragment().is_none()
                });
                if parsed.is_some() {
                    Ok(())
                } else {
                    Err(format!(
                        "its {key} is {found}, not an address of {name}.svc"
                    ))
                }
            };
            if let Some(found) = object.get("internalUrl") {
                internal(found, "internalUrl")?;
            }
            for found in object
                .get("internalUrls")
                .and_then(|urls| urls.as_object())
                .into_iter()
                .flat_map(|urls| urls.values())
            {
                internal(found, "internalUrls")?;
            }
            if object
                .get("internalUrls")
                .is_some_and(|urls| !urls.is_object())
            {
                return Err("its internalUrls isn't a map".to_string());
            }
            url("publicUrl", false)?;
        }
        other => return Err(format!("{other} has no own binding the enforcer can check")),
    }
    Ok(())
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
