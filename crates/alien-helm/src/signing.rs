//! Release signing in a chart: rendered only for a stack whose settings pin signing.
//!
//! The chart's `signing.fingerprint` value is pinned by the values schema, so an install that
//! overrides it fails before anything is created. The value lands in its own ConfigMap,
//! `<release>-signing`, which Helm and Argo CD keep when the release is uninstalled, so a later
//! install in the same namespace finds the settings the cluster already accepted state under.
//! The Operator reads the install id's cluster part from the `kube-system` namespace's UID, so
//! it may `get` that namespace and its own, and nothing else cluster-wide.

use alien_core::{ErrorData, Result, StackSettings};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;

/// Key of the ConfigMap entry that holds the signing settings JSON.
pub const SETTINGS_KEY: &str = "settings";

/// Add the signing values, schema, ConfigMap and namespace read to a chart when the stack
/// settings pin signing. Unprotected charts are left exactly as they are.
pub(crate) fn add_signing(
    files: &mut IndexMap<String, String>,
    settings: &StackSettings,
) -> Result<()> {
    let Some(signing) = &settings.signing else {
        return Ok(());
    };
    // The prefix is the release name, which Helm supplies at install. Check the rest now.
    let checked = signing.settings("release")?;

    let values = files.get_mut("values.yaml").ok_or_else(|| missing("values.yaml"))?;
    values.push_str(&format!(
        r#"
signing:
  # This cluster runs only releases and configuration signed with the vendor's keys.
  # Compare the fingerprint with the vendor's security page.
  fingerprint: "{}"
"#,
        checked.fingerprint
    ));

    let schema = files
        .get_mut("values.schema.json")
        .ok_or_else(|| missing("values.schema.json"))?;
    let mut document: serde_json::Value = serde_json::from_str(schema)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "the chart's values schema is not JSON".to_string(),
        })?;
    let properties = document
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| missing("values.schema.json properties"))?;
    properties.insert(
        "signing".to_string(),
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["fingerprint"],
            "properties": {
                "fingerprint": { "type": "string", "enum": [checked.fingerprint] }
            }
        }),
    );
    if let Some(airgapped) = properties
        .get_mut("airgapped")
        .and_then(|airgapped| airgapped.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
    {
        airgapped.insert(
            "acceptedChart".to_string(),
            serde_json::json!({ "type": "string" }),
        );
    }
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

fn missing(file: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GenericError {
        message: format!("the chart has no {file} to add release signing to"),
    })
}
