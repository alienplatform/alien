//! Release signing in a chart: rendered only for a stack whose settings pin signing.
//!
//! The chart's `signing.fingerprint` value is pinned by the values schema, so an install that
//! overrides it fails before anything is created. The settings land in their own ConfigMap,
//! `<release>-signing`, beside the install's ledger, `<release>-signing-ledger`. Helm and Argo
//! CD keep both when the release is uninstalled, so a later install in the same namespace finds
//! the settings and floors the cluster already accepted state under.
//!
//! The Operator reads ConfigMaps and writes only the ledger, so it can't rewrite the settings it
//! checks itself against. It derives the install id from the `kube-system` namespace's UID, so
//! it may `get` that namespace and its own, and nothing else cluster-wide.
//!
//! Nothing that picks code is left to values: the Operator runs the image digest the vendor's
//! signed release names, and every other image in the chart's values is pinned by digest in
//! the schema. Registries and tags stay choosable, for mirrors; a digest names the same bytes
//! in any registry.

use alien_core::{ErrorData, Result, StackSettings};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use serde_json::{json, Value};

/// Key of the `<release>-signing` ConfigMap entry that holds the signing settings JSON.
pub const SETTINGS_KEY: &str = "settings";

/// Key of the `<release>-signing-ledger` ConfigMap entry that holds the install's ledger JSON.
pub const LEDGER_KEY: &str = "ledger";

/// What a protected chart pins that the stack doesn't name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedChartPins {
    /// `sha256:<64 hex>` of the Operator image, as the signed release names it in
    /// `components` (`operator`).
    pub operator_digest: String,
}

/// The Operator's core rules in the release namespace on a protected chart. ConfigMaps are
/// read-only except the ledger, which only rises.
pub(crate) const OPERATOR_CORE_RULES: &str = r#"  - apiGroups: [""]
    resources: ["secrets", "services", "pods", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: [""]
    resources: ["configmaps"]
    verbs: ["get", "list", "watch"]
  - apiGroups: [""]
    resources: ["configmaps"]
    resourceNames: [{{ printf "%s-signing-ledger" .Release.Name | quote }}]
    verbs: ["update"]
"#;

/// `alpine/k8s:1.32.0`, which runs the chart's cleanup hooks with the Operator's
/// ServiceAccount.
const CLEANUP_IMAGE_TAG: &str = "1.32.0@sha256:4c28b0f0c6accfa07fa449493ea936291e4ff1270500571d73f75bb406676c3b";

const OPERATOR_IMAGE_VALUES: &str =
    "    repository: registry.example.com/deployment/operator\n    tag: latest\n";
const CLEANUP_IMAGE_VALUES: &str =
    "      image:\n        repository: alpine/k8s\n        tag: \"1.32.0\"\n";

/// Add the signing values, schema, ConfigMaps and namespace read to a chart when the stack
/// settings pin signing. Unprotected charts are left exactly as they are.
pub(crate) fn add_signing(
    files: &mut IndexMap<String, String>,
    settings: &StackSettings,
    pins: Option<&SignedChartPins>,
) -> Result<()> {
    let Some(signing) = &settings.signing else {
        if pins.is_some() {
            return Err(refused(
                "the stack settings don't pin release signing, so the chart has nothing to pin \
                 the Operator image for",
            ));
        }
        return Ok(());
    };
    let pins = pins.ok_or_else(|| {
        refused(
            "a chart with release signing runs the Operator build the signed release names; \
             render it with generate_signed_helm_chart and the release's Operator digest",
        )
    })?;
    if !is_digest(&pins.operator_digest) {
        return Err(refused(format!(
            "Operator digest '{}' is not sha256:<64 lowercase hex>",
            pins.operator_digest
        )));
    }
    // The prefix is the release name, which Helm supplies at install. Check the rest now.
    let checked = signing.settings("release")?;

    let values = file(files, "values.yaml")?;
    replace_once(
        values,
        OPERATOR_IMAGE_VALUES,
        &format!(
            "    repository: registry.example.com/deployment/operator\n    \
             # The Operator build the vendor's signed release names. The tag before '@' is a\n    \
             # label; the digest after it is fixed by the chart.\n    \
             tag: \"latest@{}\"\n",
            pins.operator_digest
        ),
    )?;
    replace_once(
        values,
        CLEANUP_IMAGE_VALUES,
        &format!(
            "      image:\n        repository: alpine/k8s\n        tag: \"{CLEANUP_IMAGE_TAG}\"\n"
        ),
    )?;
    values.push_str(&format!(
        r#"
signing:
  # This cluster runs only releases and configuration signed with the vendor's keys.
  # Compare the fingerprint with the vendor's security page.
  fingerprint: "{}"
"#,
        checked.fingerprint
    ));
    let defaults: Value = serde_yaml::from_str(values)
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "the chart's values.yaml is not YAML".to_string(),
        })?;

    let schema = file(files, "values.schema.json")?;
    let mut document: Value = serde_json::from_str(schema)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "the chart's values schema is not JSON".to_string(),
        })?;
    pin_schema(&mut document, &defaults, &checked.fingerprint)?;
    *schema = serde_json::to_string_pretty(&document)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize the chart's values schema".to_string(),
        })?;
    schema.push('\n');

    files.insert(
        "templates/signing.yaml".to_string(),
        signing_tpl(&checked.channel),
    );
    Ok(())
}

/// Pin the signing settings and the digest of every image in the chart's values. A tag may
/// change (a mirror may retag), the digest after its `@` may not, and Kubernetes pulls
/// `repository:tag@digest` by the digest.
fn pin_schema(schema: &mut Value, defaults: &Value, fingerprint: &str) -> Result<()> {
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| missing("values.schema.json properties"))?;
    properties.insert(
        "signing".to_string(),
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["fingerprint"],
            "properties": {
                "fingerprint": { "type": "string", "const": fingerprint }
            }
        }),
    );
    // `alien-deploy sync` records the chart a protected air-gapped site accepted.
    if let Some(airgapped) = properties
        .get_mut("airgapped")
        .and_then(|airgapped| airgapped.get_mut("properties"))
        .and_then(Value::as_object_mut)
    {
        airgapped.insert("acceptedChart".to_string(), json!({ "type": "string" }));
    }

    let mut images = Vec::new();
    find_images(defaults, &mut Vec::new(), &mut images);
    for (path, image) in images {
        let digest = image
            .get("tag")
            .and_then(Value::as_str)
            .and_then(|tag| tag.rsplit_once('@'))
            .map(|(_, digest)| digest)
            .filter(|digest| is_digest(digest))
            .ok_or_else(|| {
                refused(format!(
                    "image value '{}' has no default pinned by digest, so a protected chart \
                     would let values pick the code it runs",
                    path.join(".")
                ))
            })?;
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        let node = schema_at(schema, &path)?;
        let image_properties = node
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| missing(&format!("values.schema.json {} properties", path.join("."))))?;
        image_properties.insert(
            "tag".to_string(),
            json!({
                "type": "string",
                "pattern": format!("^[A-Za-z0-9_][A-Za-z0-9_.-]{{0,127}}@{digest}$"),
            }),
        );
        node["required"] = json!(["repository", "tag"]);
    }
    Ok(())
}

/// Every `image` mapping with a `repository` in the values, by path.
fn find_images<'a>(value: &'a Value, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, &'a Value)>) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, child) in object {
        path.push(key.clone());
        if key == "image" && child.get("repository").is_some() {
            out.push((path.clone(), child));
        } else {
            find_images(child, path, out);
        }
        path.pop();
    }
}

/// The schema node for a values path, following `properties`.
fn schema_at<'a>(schema: &'a mut Value, path: &[&str]) -> Result<&'a mut Value> {
    let mut node = schema;
    for segment in path {
        node = node
            .get_mut("properties")
            .and_then(|properties| properties.get_mut(*segment))
            .ok_or_else(|| missing(&format!("values.schema.json {}", path.join("."))))?;
    }
    Ok(node)
}

fn signing_tpl(channel: &str) -> String {
    format!(
        r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: {{{{ printf "%s-signing" .Release.Name }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
  annotations:
    # Kept on uninstall: a reinstall finds the settings the cluster accepted state under.
    helm.sh/resource-policy: keep
    argocd.argoproj.io/sync-options: Delete=false
data:
  {SETTINGS_KEY}: {{{{ dict "fingerprint" .Values.signing.fingerprint "prefix" .Release.Name "channel" "{channel}" | toJson | quote }}}}
---
{{{{- /* The Operator raises the ledger as it accepts newer statements; an upgrade keeps what
it holds. Rendering without cluster access (helm template, Argo CD) writes the initial
ledger, and the Operator raises it again from its own database. */}}}}
{{{{- $ledgerName := printf "%s-signing-ledger" .Release.Name }}}}
{{{{- $ledger := lookup "v1" "ConfigMap" .Release.Namespace $ledgerName }}}}
{{{{- $current := index (default dict (dig "data" dict $ledger)) "{LEDGER_KEY}" }}}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{{{ $ledgerName }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
  annotations:
    helm.sh/resource-policy: keep
    argocd.argoproj.io/sync-options: Delete=false
data:
  {LEDGER_KEY}: {{{{ default (dict "prefix" .Release.Name "created" true | toJson) $current | quote }}}}
---
# The Operator derives the install id from the kube-system namespace's UID and its own
# namespace and release, so it may read those two namespaces and no others.
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{{{ printf "%s-%s-signing-namespaces" .Release.Namespace .Release.Name | trunc 63 | trimSuffix "-" }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
rules:
  - apiGroups: [""]
    resources: ["namespaces"]
    resourceNames: ["kube-system", {{{{ .Release.Namespace | quote }}}}]
    verbs: ["get"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{{{ printf "%s-%s-signing-namespaces" .Release.Namespace .Release.Name | trunc 63 | trimSuffix "-" }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
subjects:
  - kind: ServiceAccount
    name: {{{{ include "deployment.managerServiceAccountName" . }}}}
    namespace: {{{{ .Release.Namespace }}}}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{{{ printf "%s-%s-signing-namespaces" .Release.Namespace .Release.Name | trunc 63 | trimSuffix "-" }}}}
"#
    )
}

fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn file<'a>(files: &'a mut IndexMap<String, String>, name: &str) -> Result<&'a mut String> {
    files.get_mut(name).ok_or_else(|| missing(name))
}

/// Replace the generator's own line, failing loudly if it moved.
fn replace_once(contents: &mut String, expected: &str, replacement: &str) -> Result<()> {
    if contents.matches(expected).count() != 1 {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "release signing expected exactly one {expected:?} in the generated chart"
            ),
        }));
    }
    *contents = contents.replacen(expected, replacement, 1);
    Ok(())
}

fn refused(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: message.into(),
    })
}

fn missing(file: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: format!("the chart has no {file} to add release signing to"),
    })
}
