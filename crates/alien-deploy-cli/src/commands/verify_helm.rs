//! `verify --helm`: check a protected Helm release against the vendor's
//! signed release, with the customer's own kubeconfig.
//!
//! The chart is the one the customer downloaded, hashed as the file map
//! setup signs. The cluster is read for what Helm installed from it (the
//! release record Helm keeps in a Secret), the values the installer set,
//! and the signing settings and ledger ConfigMaps setup created beside it.

use std::collections::BTreeMap;
use std::io::Read as _;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_helm::{LEDGER_KEY, SETTINGS_KEY};
use alien_k8s_clients::{
    ErrorData as KubernetesErrorData, KubernetesClient, KubernetesClientConfig,
    KubernetesClientConfigExt,
};
use alien_signing::{settings::names, InstallId};
use base64::Engine as _;
use serde_json::{Map, Value};

use super::verify::{line, sha256_digest, Choices, Installed, Line, Mark, Signed};
use crate::error::{ErrorData, Result};

/// The setup template a Helm release is.
const HELM: &str = "helm";

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::VerifyFailed {
        message: message.into(),
    })
}

/// What `verify --helm` read from the cluster.
#[derive(Debug, Clone)]
pub struct HelmObserved {
    /// UID of the cluster's `kube-system` namespace.
    pub cluster: String,
    pub namespace: String,
    pub release: String,
    /// The deployed release record, as Helm stores it.
    pub record: Value,
    /// The `<release>-signing` ConfigMap's settings entry.
    pub settings: Option<Vec<u8>>,
    /// The `<release>-signing-ledger` ConfigMap's ledger entry.
    pub ledger: Option<Vec<u8>>,
}

/// The install, the chart digest and the values, ready for the signed checks,
/// and a line saying whether Helm installed exactly the chart in `files`.
pub fn helm_install(
    observed: &HelmObserved,
    files: &BTreeMap<String, String>,
) -> Result<(Signed, Installed, Line)> {
    let install = InstallId::parse(&format!(
        "k8s:{}:{}/{}",
        observed.cluster, observed.namespace, observed.release
    ))
    .context(ErrorData::VerifyFailed {
        message: format!(
            "release {}/{} doesn't name a valid install",
            observed.namespace, observed.release
        ),
    })?;
    let defaults = yaml_object(files, "values.yaml")?;
    // What Helm renders with: the stored chart's values with the
    // installer's on top. An upgrade with `--reuse-values` stores the
    // previous values in the chart, so the chart's values alone aren't
    // the signed values.yaml even when nothing else changed.
    let mut effective = observed.record["chart"]["values"].clone();
    coalesce(&mut effective, &observed.record["config"]);
    let effective = match effective {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        _ => return Err(failed("Helm's stored values are not a mapping")),
    };
    let signed = Signed {
        install,
        prefix: observed.release.clone(),
        settings_name: format!(
            "ConfigMap {}/{}",
            observed.namespace,
            names::kubernetes_settings(&observed.release)
        ),
        settings: observed.settings.clone().map(|bytes| (bytes, None)),
        ledger_name: format!(
            "ConfigMap {}/{}-signing-ledger",
            observed.namespace, observed.release
        ),
        ledger: observed.ledger.clone(),
        signer_lists: Vec::new(),
        releases: Vec::new(),
        platform: "kubernetes",
    };
    let installed = Installed {
        template: HELM,
        digest: sha256_digest(&alien_cli_common::setup_templates::file_map_bytes(files)),
        choices: Choices::Values {
            effective,
            defaults,
        },
    };
    let chart = match installed_chart_differences(&observed.record["chart"], files) {
        Ok(()) => line(
            "Installed chart",
            format!(
                "revision {} is this chart",
                observed.record["version"].as_u64().unwrap_or_default()
            ),
            Mark::Pass,
        ),
        Err(differences) => line("Installed chart", differences.join("; "), Mark::Fail),
    };
    Ok((signed, installed, chart))
}

fn yaml_object(files: &BTreeMap<String, String>, path: &str) -> Result<Map<String, Value>> {
    let text = files
        .get(path)
        .ok_or_else(|| failed(format!("the chart has no {path}")))?;
    let value: Value =
        serde_yaml::from_str(text)
            .into_alien_error()
            .context(ErrorData::VerifyFailed {
                message: format!("the chart's {path} is not YAML"),
            })?;
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        _ => Err(failed(format!("the chart's {path} is not a mapping"))),
    }
}

/// Put `over` on top of `base` as Helm merges values: maps merge key by
/// key, anything else replaces, and `null` removes the key.
fn coalesce(base: &mut Value, over: &Value) {
    let Value::Object(over) = over else {
        if !over.is_null() {
            *base = over.clone();
        }
        return;
    };
    if !base.is_object() {
        *base = Value::Object(Map::new());
    }
    if let Value::Object(base) = base {
        for (key, value) in over {
            match (base.get_mut(key), value) {
                (_, Value::Null) => {
                    base.remove(key);
                }
                (Some(existing @ Value::Object(_)), Value::Object(_)) => coalesce(existing, value),
                _ => {
                    base.insert(key.clone(), value.clone());
                }
            }
        }
    }
}

/// How the chart Helm stored differs from the chart's files: templates,
/// schema and other files byte for byte, and the chart's name and version
/// as parsed (Helm keeps Chart.yaml parsed, not as bytes). Values are
/// checked as the values in effect, against the signed allow-list.
fn installed_chart_differences(
    stored: &Value,
    files: &BTreeMap<String, String>,
) -> std::result::Result<(), Vec<String>> {
    let mut differences = Vec::new();
    let decode = |entry: &Value| -> Option<(String, String)> {
        let name = entry["name"].as_str()?.to_string();
        let data = base64::engine::general_purpose::STANDARD
            .decode(entry["data"].as_str()?)
            .ok()?;
        Some((name, String::from_utf8(data).ok()?))
    };
    let stored_files = |key: &str| -> std::result::Result<BTreeMap<String, String>, String> {
        stored[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|entry| decode(entry).ok_or_else(|| format!("a stored {key} entry can't be read")))
            .collect()
    };
    let mut expected_templates = BTreeMap::new();
    let mut expected_files = BTreeMap::new();
    for (path, contents) in files {
        match path.as_str() {
            "Chart.yaml" | "values.yaml" | "values.schema.json" => {}
            _ if path.starts_with("templates/") => {
                expected_templates.insert(path.clone(), contents.clone());
            }
            _ => {
                expected_files.insert(path.clone(), contents.clone());
            }
        }
    }
    for (key, expected) in [
        ("templates", &expected_templates),
        ("files", &expected_files),
    ] {
        match stored_files(key) {
            Ok(found) => {
                for path in expected.keys().chain(found.keys()) {
                    if expected.get(path) != found.get(path)
                        && !differences.iter().any(|d: &String| d.starts_with(path))
                    {
                        differences.push(match (expected.get(path), found.get(path)) {
                            (Some(_), Some(_)) => format!("{path} differs from the chart"),
                            (Some(_), None) => format!("{path} is not installed"),
                            _ => format!("{path} is installed but not in the chart"),
                        });
                    }
                }
            }
            Err(reason) => differences.push(reason),
        }
    }
    let schema = stored["schema"]
        .as_str()
        .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok());
    if schema.as_deref() != files.get("values.schema.json").map(String::as_str) {
        differences.push("values.schema.json differs from the chart".to_string());
    }
    match yaml_object(files, "Chart.yaml") {
        Ok(chart) => {
            for field in ["name", "version"] {
                if stored["metadata"][field] != chart.get(field).cloned().unwrap_or(Value::Null) {
                    differences.push(format!("Chart.yaml {field} differs from the chart"));
                }
            }
        }
        Err(error) => differences.push(error.message.clone()),
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences)
    }
}

/// Read the release, its values and its signing ConfigMaps from the cluster
/// the kubeconfig points at.
pub async fn observe(release: &str, namespace: &str) -> Result<HelmObserved> {
    let config =
        KubernetesClientConfig::try_kubeconfig()
            .await
            .context(ErrorData::ConfigurationError {
                message: "Could not load the kubeconfig".to_string(),
            })?;
    let config =
        alien_infra::resolve_kubeconfig(&config)
            .await
            .context(ErrorData::ConfigurationError {
                message: "Could not read the kubeconfig".to_string(),
            })?;
    let client = KubernetesClient::new(config)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Could not connect to the cluster".to_string(),
        })?;
    let cluster = client
        .get_namespace("kube-system")
        .await
        .context(ErrorData::VerifyFailed {
            message: "could not read namespace kube-system, whose UID names the cluster"
                .to_string(),
        })?
        .metadata
        .uid
        .ok_or_else(|| failed("namespace kube-system has no UID"))?;
    let secrets = client
        .list_secrets(
            namespace,
            Some(format!("owner=helm,name={release},status=deployed")),
            None,
        )
        .await
        .context(ErrorData::VerifyFailed {
            message: format!("could not list Helm's records of release {namespace}/{release}"),
        })?;
    let [secret] = secrets.items.as_slice() else {
        return Err(failed(format!(
            "release {namespace}/{release} has {} deployed revisions; expected exactly one",
            secrets.items.len()
        )));
    };
    let stored = secret
        .data
        .as_ref()
        .and_then(|data| data.get("release"))
        .ok_or_else(|| {
            failed(format!(
                "Helm's record of {namespace}/{release} has no release"
            ))
        })?;
    let record = decode_release(&stored.0)?;
    Ok(HelmObserved {
        cluster,
        namespace: namespace.to_string(),
        release: release.to_string(),
        record,
        settings: config_map_entry(
            &client,
            namespace,
            &names::kubernetes_settings(release),
            SETTINGS_KEY,
        )
        .await?,
        ledger: config_map_entry(
            &client,
            namespace,
            &format!("{release}-signing-ledger"),
            LEDGER_KEY,
        )
        .await?,
    })
}

/// Helm's stored release: base64 of gzipped JSON.
pub fn decode_release(stored: &[u8]) -> Result<Value> {
    let gzipped = base64::engine::general_purpose::STANDARD
        .decode(stored)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: "Helm's release record is not base64".to_string(),
        })?;
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(gzipped.as_slice())
        .read_to_end(&mut json)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: "Helm's release record is not gzip".to_string(),
        })?;
    serde_json::from_slice(&json)
        .into_alien_error()
        .context(ErrorData::VerifyFailed {
            message: "Helm's release record is not JSON".to_string(),
        })
}

/// Entry `key` of ConfigMap `name`; `None` when the ConfigMap doesn't exist.
async fn config_map_entry(
    client: &KubernetesClient,
    namespace: &str,
    name: &str,
    key: &str,
) -> Result<Option<Vec<u8>>> {
    match client.get_config_map(namespace, name).await {
        Ok(config_map) => config_map
            .data
            .and_then(|mut data| data.remove(key))
            .map(|value| Some(value.into_bytes()))
            .ok_or_else(|| failed(format!("ConfigMap {namespace}/{name} has no '{key}' entry"))),
        Err(error)
            if matches!(
                error.error,
                Some(KubernetesErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error).context(ErrorData::VerifyFailed {
            message: format!("could not read ConfigMap {namespace}/{name}"),
        }),
    }
}

#[cfg(test)]
#[path = "verify_helm_tests.rs"]
mod tests;
