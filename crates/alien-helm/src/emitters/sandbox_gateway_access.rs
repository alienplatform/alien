//! Exact signing-key reads for sandbox-management workloads, and the operator's node list
//! for gateway address exclusions. Replica configuration stays readable only by the
//! controller and by its projected gateway workload.

use std::collections::BTreeSet;

use alien_core::{Result, Sandbox, SandboxEgress, Stack};
use alien_error::AlienError;

pub(crate) fn template(stack: &Stack) -> Result<Option<String>> {
    let internet: BTreeSet<_> = stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .filter(|sandbox| sandbox.egress == SandboxEgress::Internet)
                .map(|_| id.as_str())
        })
        .collect();
    if internet.is_empty() {
        return Ok(None);
    }
    // The operator excludes node and pod ranges from gateway egress, which needs the node
    // list even when node heartbeat collection (and its ClusterRole) is turned off.
    let mut template = String::from(
        r#"{{- $nodes := printf "%s-egress-nodes-%s" (include "deployment.fullname" . | trunc 40 | trimSuffix "-") (sha256sum (printf "%s/%s" .Release.Namespace .Release.Name) | trunc 8) -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ $nodes }}
rules:
  - apiGroups: [""]
    resources: ["nodes"]
    verbs: ["list"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ $nodes }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ $nodes }}
---
"#,
    );
    for (name, profile) in stack.permission_profiles() {
        // Kubernetes RBAC cannot subtract one Secret from an existing namespace-wide
        // grant. Refuse this combination rather than expose the gateway's replica key.
        if profile
            .0
            .values()
            .flatten()
            .any(|set| matches!(set.id(), "vault/data-read" | "vault/data-write"))
        {
            return Err(AlienError::new(alien_core::ErrorData::GenericError {
                message: format!("Kubernetes internet sandboxes cannot use permission profile '{name}' with namespace-wide Secret access; the gateway replica configuration must remain private. Use scoped external-vault permissions instead of vault/data-read or vault/data-write."),
            }));
        }
        let manages = profile.0.iter().any(|(target, sets)| {
            (target == "*" || internet.contains(target.as_str()))
                && sets.iter().any(|set| set.id() == "sandbox/management")
        });
        if !manages {
            continue;
        }
        // JSON string syntax is valid as a Go template string and preserves escaping.
        let profile = serde_json::to_string(name).expect("profile name serializes");
        template.push_str(&format!(r#"{{{{ $account := include "deployment.serviceAccountName" (dict "root" . "name" {profile}) -}}}}
{{{{- $role := printf "%s-%s-egress" ($account | trunc 46 | trimSuffix "-") (sha256sum {profile} | trunc 8) -}}}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{{{ $role }}}}
  namespace: {{{{ .Release.Namespace }}}}
rules:
  - apiGroups: [""]
    resources: [secrets]
    resourceNames: [{{{{ printf "%s-secrets-sandbox-egress-signing-key" (include "deployment.serviceAccountPrefix" .) | quote }}}}]
    verbs: [get]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{{{ $role }}}}
  namespace: {{{{ .Release.Namespace }}}}
subjects:
  - kind: ServiceAccount
    name: {{{{ $account }}}}
    namespace: {{{{ .Release.Namespace }}}}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{{{ $role }}}}
---
"#));
    }
    Ok(Some(template))
}
