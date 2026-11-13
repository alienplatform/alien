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
//! Nothing that picks code or the Operator's rights is left to values: the Operator runs the
//! image digest the vendor's signed release names, every other image in the chart's values is
//! pinned by digest, workload RBAC is the signed stack's, and chart-provisioned databases
//! (picked by tag) stay off. Templates render each image as `repository@<digest>` through
//! `quote`, with the digest written into the template, and the schema holds repositories to the
//! image reference grammar. Registries stay choosable, for mirrors; a digest names the same
//! bytes in any registry.

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
const CLEANUP_IMAGE_TAG: &str =
    "1.32.0@sha256:4c28b0f0c6accfa07fa449493ea936291e4ff1270500571d73f75bb406676c3b";

/// metrics-server v0.8.1, which cluster bootstrap can install in `kube-system`.
const METRICS_SERVER_IMAGE: &str = "registry.k8s.io/metrics-server/metrics-server:v0.8.1@sha256:b2d2efaf5ac3b366ed0f839d2412a2c4279d4fc2a2a733f12c52133faed36c41";

const OPERATOR_IMAGE_VALUES: &str =
    "    repository: registry.example.com/deployment/operator\n    tag: latest\n";
const CLEANUP_IMAGE_VALUES: &str =
    "      image:\n        repository: alpine/k8s\n        tag: \"1.32.0\"\n";
const METRICS_SERVER_IMAGE_VALUES: &str =
    "    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n";

/// Image references the generator renders inside a quoted string, by template. A value with
/// `"` or `#` could end that string and drop what follows, so a protected chart renders
/// `repository@<digest>` through `quote`, with the digest written into the template.
const OPERATOR_IMAGE_TEMPLATE: &str = r#"image: {{ include "deployment.image" (dict "root" $ "repository" .Values.runtime.image.repository "tag" .Values.runtime.image.tag "digest" (dig "digest" "" .Values.runtime.image)) | quote }}"#;
const LOG_COLLECTOR_IMAGE_TEMPLATE: &str = r#"image: {{ include "deployment.image" (dict "root" $ "repository" .Values.logCollector.image.repository "tag" .Values.logCollector.image.tag "digest" (dig "digest" "" .Values.logCollector.image)) | quote }}"#;
const CLEANUP_IMAGE_TEMPLATE: &str = r#"image: {{ include "deployment.image" (dict "root" $ "repository" (dig "image" "repository" "alpine/k8s" $cleanup) "tag" (dig "image" "tag" "1.32.0" $cleanup) "digest" (dig "image" "digest" "" $cleanup)) | quote }}"#;

/// A registry host (with an optional port) and repository path, as image references allow
/// them: no tag, digest, quote, comment or whitespace.
const REPOSITORY_PATTERN: &str = r"(?:[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*(?::[0-9]+)?/)?[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*(?:/[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*)*";

/// An image tag.
const TAG_PATTERN: &str = "[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}";

/// Refuse a product chart, with the Remote Operator in the same release, for a stack whose
/// settings pin release signing. A protected install uses the chart signed with the release,
/// which [`crate::generate_signed_helm_chart`] renders.
pub(crate) fn refuse_remote_operator(settings: &StackSettings) -> Result<()> {
    if settings.signing.is_some() {
        return Err(refused(
            "the stack settings pin release signing, so it has no product chart with the Remote \
             Operator; a protected install uses the chart signed with the release, rendered by \
             generate_signed_helm_chart",
        ));
    }
    Ok(())
}

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
    replace_once(
        values,
        METRICS_SERVER_IMAGE_VALUES,
        &format!("    image: {METRICS_SERVER_IMAGE}\n"),
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
    let defaults: Value =
        serde_yaml::from_str(values)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "the chart's values.yaml is not YAML".to_string(),
            })?;

    pin_templates(files, &defaults, &pins.operator_digest)?;

    let schema = file(files, "values.schema.json")?;
    let mut document: Value = serde_json::from_str(schema).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "the chart's values schema is not JSON".to_string(),
        },
    )?;
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

/// Render every image from values as `repository@<digest>`, quoted, with the digest the chart
/// pins, and refuse a chart that still interpolates a value inside a quoted image string.
fn pin_templates(
    files: &mut IndexMap<String, String>,
    defaults: &Value,
    operator_digest: &str,
) -> Result<()> {
    let log_collector_digest = defaults["logCollector"]["image"]["tag"]
        .as_str()
        .and_then(|tag| tag.rsplit_once('@'))
        .map(|(_, digest)| digest)
        .filter(|digest| is_digest(digest))
        .ok_or_else(|| missing("values.yaml logCollector.image digest"))?;
    let cleanup_digest = CLEANUP_IMAGE_TAG
        .rsplit_once('@')
        .map(|(_, digest)| digest)
        .ok_or_else(|| missing("the cleanup image digest"))?;
    // The same include the chart renders with, so a site's `global.imageRegistry` mirror still
    // applies; the digest is the literal one written here, never a value.
    let pinned = |repository: &str, digest: &str| {
        format!(
            r#"image: {{{{ include "deployment.image" (dict "root" $ "repository" {repository} "tag" "" "digest" "{digest}") | quote }}}}"#
        )
    };
    for (path, template, replacement) in [
        (
            "templates/deployment.yaml",
            OPERATOR_IMAGE_TEMPLATE,
            pinned(".Values.runtime.image.repository", operator_digest),
        ),
        (
            "templates/whitelabeled-log-collector-daemonset.yaml",
            LOG_COLLECTOR_IMAGE_TEMPLATE,
            pinned(
                ".Values.logCollector.image.repository",
                log_collector_digest,
            ),
        ),
        (
            "templates/cleanup-job.yaml",
            CLEANUP_IMAGE_TEMPLATE,
            pinned(
                r#"(dig "image" "repository" "alpine/k8s" $cleanup)"#,
                cleanup_digest,
            ),
        ),
        (
            "templates/runtime-cleanup-history-prune.yaml",
            CLEANUP_IMAGE_TEMPLATE,
            pinned(
                r#"(dig "image" "repository" "alpine/k8s" $cleanup)"#,
                cleanup_digest,
            ),
        ),
    ] {
        replace_once(file(files, path)?, template, &replacement)?;
    }
    for (path, contents) in files.iter() {
        if !path.starts_with("templates/") {
            continue;
        }
        if let Some(line) = contents.lines().find(|line| {
            line.trim_start().starts_with("image:")
                && (line.contains("\"{{") || line.contains("'{{"))
        }) {
            return Err(refused(format!(
                "{path} renders an image from values inside a quoted string ({}), which a \
                 value could end early; a protected chart can't pin it",
                line.trim()
            )));
        }
    }
    Ok(())
}

/// Top-level values a customer may set on a protected chart, for the signed release's
/// allow-list: every key of `values.yaml` except those the schema fixes whole. Inside the
/// allowed keys, the schema still fixes every image digest and keeps Postgres off.
pub fn allowed_values(chart: &crate::HelmChart) -> Result<Vec<String>> {
    let values = chart
        .files
        .get("values.yaml")
        .ok_or_else(|| missing("values.yaml"))?;
    let values: serde_yaml::Mapping =
        serde_yaml::from_str(values)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "the chart's values.yaml is not a YAML mapping".to_string(),
            })?;
    Ok(values
        .keys()
        .filter_map(|key| key.as_str())
        .filter(|key| !PINNED_WHOLE.contains(key))
        .map(str::to_string)
        .collect())
}

/// Values a protected chart's schema fixes to the chart's own defaults.
const PINNED_WHOLE: [&str; 2] = ["signing", "serviceAccounts"];

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

    // Workload RBAC comes from the signed stack's permission profiles, never from values: an
    // entry named like the Operator's own ServiceAccount would bind its rules to the Operator.
    let accounts = defaults
        .get("serviceAccounts")
        .cloned()
        .ok_or_else(|| missing("values.yaml serviceAccounts"))?;
    *schema_at(schema, &["serviceAccounts"])? = json!({ "const": accounts });

    // A chart-provisioned database runs an image picked by tag, with no digest the vendor
    // signed. Protected installs bind an external database instead.
    if let Some(databases) = defaults.get("postgres").and_then(Value::as_object) {
        let pinned: serde_json::Map<String, Value> = databases
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    json!({
                        "type": "object",
                        "properties": { "enabled": { "const": false } }
                    }),
                )
            })
            .collect();
        *schema_at(schema, &["postgres"])? = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": pinned
        });
    }

    let mut images = Vec::new();
    find_images(defaults, &mut Vec::new(), &mut images);
    for (path, image) in images {
        let (field, reference) = match image {
            Value::String(reference) => (None, reference.as_str()),
            image => (
                Some("tag"),
                image.get("tag").and_then(Value::as_str).unwrap_or_default(),
            ),
        };
        let digest = reference
            .rsplit_once('@')
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
        // `image: null`, or null anywhere above it, drops the key, and the templates' own
        // defaults carry no digest.
        for depth in 0..path.len() {
            require(schema_at(schema, &path[..depth])?, path[depth]);
        }
        match field {
            Some(field) => {
                let node = schema_at(schema, &path)?;
                let image_properties = node
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        missing(&format!("values.schema.json {} properties", path.join(".")))
                    })?;
                image_properties.insert(
                    field.to_string(),
                    json!({
                        "type": "string",
                        "pattern": format!("^{TAG_PATTERN}@{digest}$"),
                    }),
                );
                image_properties.insert(
                    "repository".to_string(),
                    json!({
                        "type": "string",
                        "pattern": format!("^{REPOSITORY_PATTERN}$"),
                    }),
                );
                node["required"] = json!(["repository", "tag"]);
            }
            None => {
                *schema_at(schema, &path)? = json!({
                    "type": "string",
                    "pattern": format!("^{REPOSITORY_PATTERN}(?::{TAG_PATTERN})?@{digest}$"),
                });
            }
        }
    }
    Ok(())
}

/// Add `key` to a schema node's `required` list.
fn require(node: &mut Value, key: &str) {
    let mut required: Vec<Value> = node
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !required.iter().any(|name| name == key) {
        required.push(json!(key));
    }
    node["required"] = Value::Array(required);
}

/// Every image in the values, by path: a mapping under `image` with a `repository`, or a
/// string reference under `image`. Postgres is pinned off as a whole.
fn find_images<'a>(
    value: &'a Value,
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, &'a Value)>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, child) in object {
        if path.is_empty() && key == "postgres" {
            continue;
        }
        path.push(key.clone());
        if key == "image" && (child.get("repository").is_some() || child.is_string()) {
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
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apply_package_defaults, generate_helm_chart, generate_product_helm_chart,
        generate_signed_helm_chart, HelmOptions, HelmRegistry, OperatorImageDefault,
        OperatorManifestOptions, OperatorOutputFormat, OperatorPermission, OperatorScope,
        PackageDefaults, ProductOperatorManifestOptions,
    };
    use alien_core::{Stack, StackSigning};

    const FINGERPRINT: &str =
        "SHA256:9f2c3b1a00000000000000000000000000000000000000000000000000000f1a";
    const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn protected() -> StackSettings {
        StackSettings {
            signing: Some(StackSigning {
                fingerprint: FINGERPRINT.to_string(),
                channel: "production".to_string(),
            }),
            ..StackSettings::default()
        }
    }

    fn render(settings: StackSettings, pins: Option<&SignedChartPins>) -> Result<crate::HelmChart> {
        let stack = Stack::new("signing-test".to_string()).build();
        let registry = HelmRegistry::built_in();
        let options = HelmOptions {
            registry: &registry,
            stack_settings: settings,
            chart_name: "signing-test".to_string(),
        };
        match pins {
            Some(pins) => generate_signed_helm_chart(&stack, options, pins),
            None => generate_helm_chart(&stack, options),
        }
    }

    fn operator_tag(chart: &crate::HelmChart) -> String {
        let values: Value = serde_yaml::from_str(&chart.files["values.yaml"]).unwrap();
        values["runtime"]["image"]["tag"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_protected_stack_needs_the_signed_operator_digest() {
        let error = render(protected(), None).unwrap_err();
        assert!(
            error.to_string().contains("generate_signed_helm_chart"),
            "{error}"
        );

        let bad = SignedChartPins {
            operator_digest: "sha256:latest".to_string(),
        };
        let error = render(protected(), Some(&bad)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("is not sha256:<64 lowercase hex>"),
            "{error}"
        );

        let pins = SignedChartPins {
            operator_digest: DIGEST.to_string(),
        };
        let error = render(StackSettings::default(), Some(&pins)).unwrap_err();
        assert!(
            error.to_string().contains("don't pin release signing"),
            "{error}"
        );
    }

    #[test]
    fn a_protected_render_refuses_an_image_without_a_digest() {
        let mut files = render(StackSettings::default(), None).unwrap().files;
        files
            .get_mut("values.yaml")
            .unwrap()
            .push_str("\nextra:\n  image:\n    repository: example.com/tool\n    tag: v1\n");
        let pins = SignedChartPins {
            operator_digest: DIGEST.to_string(),
        };
        let error = add_signing(&mut files, &protected(), Some(&pins)).unwrap_err();
        assert!(
            error.to_string().contains("image value 'extra.image'"),
            "{error}"
        );
    }

    #[test]
    fn a_protected_render_refuses_an_image_interpolated_inside_a_string() {
        let mut files = render(StackSettings::default(), None).unwrap().files;
        files.insert(
            "templates/extra.yaml".to_string(),
            "spec:\n  image: \"{{ .Values.extra.repository }}:v1\"\n".to_string(),
        );
        let pins = SignedChartPins {
            operator_digest: DIGEST.to_string(),
        };
        let error = add_signing(&mut files, &protected(), Some(&pins)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("templates/extra.yaml renders an image from values"),
            "{error}"
        );
    }

    #[test]
    fn the_allow_list_leaves_out_exactly_the_values_the_schema_fixes() {
        let pins = SignedChartPins {
            operator_digest: DIGEST.to_string(),
        };
        let chart = render(protected(), Some(&pins)).unwrap();
        let allowed = allowed_values(&chart).unwrap();
        let values: Value = serde_yaml::from_str(&chart.files["values.yaml"]).unwrap();
        let schema: Value = serde_json::from_str(&chart.files["values.schema.json"]).unwrap();
        for key in values.as_object().unwrap().keys() {
            let property = &schema["properties"][key.as_str()];
            let fixed = property.get("const").is_some()
                || property["properties"].as_object().is_some_and(|fields| {
                    !fields.is_empty() && fields.values().all(|field| field.get("const").is_some())
                });
            assert_eq!(!allowed.contains(key), fixed, "{key}: {property}");
        }
        assert!(allowed.contains(&"runtime".to_string()));
    }

    #[test]
    fn packaging_a_protected_chart_keeps_the_signed_operator_digest() {
        let pins = SignedChartPins {
            operator_digest: DIGEST.to_string(),
        };
        let chart = render(protected(), Some(&pins)).unwrap();
        fn defaults(tag: &str) -> PackageDefaults<'_> {
            PackageDefaults {
                operator_image: Some(OperatorImageDefault {
                    repository: "mirror.example.com/operator",
                    tag,
                }),
                ..PackageDefaults::default()
            }
        }
        let packaged = apply_package_defaults(chart.clone(), &defaults("1.4.0")).unwrap();
        assert_eq!(operator_tag(&packaged), format!("1.4.0@{DIGEST}"));
        let same =
            apply_package_defaults(chart.clone(), &defaults(&format!("1.4.0@{DIGEST}"))).unwrap();
        assert_eq!(operator_tag(&same), format!("1.4.0@{DIGEST}"));
        let other = format!("1.4.0@sha256:{}", "b".repeat(64));
        let error = apply_package_defaults(chart, &defaults(&other)).unwrap_err();
        assert!(error.to_string().contains("pins Operator image"), "{error}");
    }

    #[test]
    fn the_product_chart_refuses_a_protected_stack() {
        let stack = Stack::new("signing-test".to_string()).build();
        let registry = HelmRegistry::built_in();
        let options = HelmOptions {
            registry: &registry,
            stack_settings: protected(),
            chart_name: "signing-test".to_string(),
        };
        let remote_operator = ProductOperatorManifestOptions {
            manifest: OperatorManifestOptions {
                manager_url: r#"{{ include "deployment.managementUrl" . }}"#,
                group_token: "",
                encryption_key: "",
                image: "registry.example.com/deployment/operator:latest",
                log_collector: None,
                stack_settings: None,
                project_name: "signing-test",
                environment_name: None,
                install_namespace: None,
                label_domain: None,
                scope: OperatorScope::Namespace,
                label_selector: None,
                custom_operation_permissions: &[],
                permission: OperatorPermission::Remediation,
                format: OperatorOutputFormat::HelmTemplate,
            },
            credentials_secret_name: "{{ .Values.remoteOperator.existingSecret.name }}",
            credentials_encryption_key_sha256:
                "{{ .Values.remoteOperator.existingSecret.encryptionKeySha256 }}",
            resource_name: None,
        };
        let error = generate_product_helm_chart(&stack, options, remote_operator).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("a protected install uses the chart signed with the release"),
            "{error}"
        );
    }
}
