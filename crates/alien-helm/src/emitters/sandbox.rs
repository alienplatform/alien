//! Sandbox emitter — the NetworkPolicy that bounds a session, and the cluster-scoped RBAC the
//! operator needs to provision one and its broker needs to authenticate one.
//!
//! Both belong to Helm rather than to the operator: the operator creates NetworkPolicies for
//! Containers, but the one that bounds a sandbox session comes with the chart, and a `TokenReview`
//! and the RuntimeClass listing the controller checks before provisioning are cluster-scoped
//! while the operator is namespace-scoped with no cluster-admin.
//! Build has the same shape and is the precedent.
//!
//! No ServiceAccount is emitted, unlike Build's. A sandbox pod runs with
//! `automountServiceAccountToken: false` — a mounted token is a credential the untrusted code can
//! read — so a dedicated account would grant it nothing that `default` does not, and would be a
//! name for the operator and the chart to keep in step for no gain. Build's account exists because
//! a build genuinely needs identity to push images.

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::sandbox_image::AGENT_PORT;
use alien_core::{
    import::EmitContext, ErrorData, ResourceRef, Result, Sandbox,
    SandboxEgress, Stack,
};
use alien_error::AlienError;

/// Label the operator puts on every pod backing a session, and what the policy selects on.
const LABEL_SANDBOX: &str = "alien.dev/sandbox";

/// Path the broker's RBAC lands at. Fixed rather than per-sandbox: the ClusterRole is one per
/// deployment, so a second sandbox rewrites the same file instead of colliding on the name.
const BROKER_RBAC_TEMPLATE: &str = "sandbox-broker-rbac.yaml";

/// Helm helper naming the RuntimeClass of sandbox pods, shared by the operator Deployment and the
/// network-policy probe. Only charts with a sandbox carry it.
const RUNTIME_CLASS_HELPER_TEMPLATE: &str = "_sandbox.tpl";

/// The `sandbox.runtimeClassName` value when set; otherwise the vendor class for the cluster the
/// chart renders for, read from its version string without a `lookup` (Argo CD renders with
/// `helm template`). Anything else fails the render and names the choices.
const RUNTIME_CLASS_HELPER: &str = r#"{{- define "deployment.sandboxRuntimeClassName" -}}
{{- $configured := dig "runtimeClassName" "" (default dict .Values.sandbox) -}}
{{- if $configured -}}
{{- $configured -}}
{{- else if contains "-eks-" .Capabilities.KubeVersion.GitVersion -}}
isolated-sandbox
{{- else if contains "-gke." .Capabilities.KubeVersion.GitVersion -}}
gvisor
{{- else -}}
{{- fail (printf "sandbox.runtimeClassName is not set and cannot be chosen for Kubernetes %s. Set it to the sandboxed RuntimeClass of this cluster: AKS uses kata-vm-isolation, EKS uses isolated-sandbox, GKE uses gvisor, and on-premises clusters use the gVisor or Kata class the administrator installed." .Capabilities.KubeVersion.GitVersion) -}}
{{- end -}}
{{- end -}}
"#;

/// Helm helper naming the namespace of sandbox pods: `sandbox.namespace` when the administrator
/// set one, otherwise the release namespace, where they have always run.
const SANDBOX_NAMESPACE_HELPER: &str = r#"{{- define "deployment.sandboxNamespace" -}}
{{- default .Release.Namespace (dig "namespace" "" (default dict .Values.sandbox)) -}}
{{- end -}}
"#;

/// The operator's rights in a separate sandbox namespace, and nowhere else: pods and Secrets for
/// the sandboxes, events to report why a pod is not ready, and the one NetworkPolicy per internet
/// sandbox that admits the gateway Service address. Renders nothing while
/// `sandbox.namespace` is unset, when the release Role already covers the release namespace.
/// There is no ClusterRole: the administrator's namespace is the only place these rights apply.
const SANDBOX_NAMESPACE_RBAC_TEMPLATE: &str = "sandbox-namespace-rbac.yaml";
const SANDBOX_NAMESPACE_RBAC: &str = r#"{{- $sandboxNamespace := dig "namespace" "" (default dict .Values.sandbox) -}}
{{- if and $sandboxNamespace (ne $sandboxNamespace .Release.Namespace) -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.fullname" . | trunc 52 | trimSuffix "-" }}-sandbox
  namespace: {{ $sandboxNamespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["pods", "secrets"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["networkpolicies"]
    verbs: ["get", "list", "create", "update", "delete"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.fullname" . | trunc 52 | trimSuffix "-" }}-sandbox
  namespace: {{ $sandboxNamespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.fullname" . | trunc 52 | trimSuffix "-" }}-sandbox
{{- end }}
"#;

/// The Operator reads where the API server is (the `default/kubernetes` Service and the
/// EndpointSlice of the same name) and tells each sandbox pod, so the pod's startup check can
/// prove those addresses are dropped. Two named objects, read only, in `default`.
const KUBE_API_DISCOVERY_RBAC_TEMPLATE: &str = "sandbox-kube-api-discovery-rbac.yaml";
const KUBE_API_DISCOVERY_RBAC: &str = r#"{{- /* Both objects live in `default`, shared by every install on the cluster, so the name carries a hash of the full release namespace and name (the prefixes alone can collide once truncated). */ -}}
{{- $suffix := printf "kube-api-%s" (printf "%s/%s/%s" .Release.Namespace .Release.Name (include "deployment.fullname" .) | sha256sum | trunc 8) -}}
{{- $name := printf "%s-%s" (include "deployment.fullname" . | trunc (int (sub 62 (len $suffix))) | trimSuffix "-") $suffix -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ $name }}
  namespace: default
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["services"]
    resourceNames: ["kubernetes"]
    verbs: ["get"]
  - apiGroups: ["discovery.k8s.io"]
    resources: ["endpointslices"]
    resourceNames: ["kubernetes"]
    verbs: ["get"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ $name }}
  namespace: default
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ $name }}
"#;

/// The Operator reads the cluster DNS Service address (`kube-system/kube-dns`) so the sandbox
/// gateway can resolve names through it. One named object, read only. Without this grant (or
/// when `sandbox.readClusterDNS` turns it off) the gateway keeps the cluster's default resolver.
const KUBE_DNS_DISCOVERY_RBAC_TEMPLATE: &str = "sandbox-kube-dns-discovery-rbac.yaml";
const KUBE_DNS_DISCOVERY_RBAC: &str = r#"{{- /* The Role lives in `kube-system`, shared by every install on the cluster, so the name carries a hash of the full release namespace and name. `sandbox.readClusterDNS` is `auto` (the default: rendered, except on Google Kubernetes Engine, where a managed `kube-system` can refuse the grant and the gateway keeps the cluster resolver), `true` or `false`. */ -}}
{{- $read := toString (dig "readClusterDNS" "auto" (default dict .Values.sandbox)) -}}
{{- if or (eq $read "true") (and (eq $read "auto") (not (contains "-gke" .Capabilities.KubeVersion.GitVersion))) }}
{{- $suffix := printf "kube-dns-%s" (printf "%s/%s/%s" .Release.Namespace .Release.Name (include "deployment.fullname" .) | sha256sum | trunc 8) -}}
{{- $name := printf "%s-%s" (include "deployment.fullname" . | trunc (int (sub 62 (len $suffix))) | trimSuffix "-") $suffix -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ $name }}
  namespace: kube-system
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["services"]
    resourceNames: ["kube-dns"]
    verbs: ["get"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ $name }}
  namespace: kube-system
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ $name }}
{{- end }}
"#;

#[derive(Debug, Default)]
pub struct SandboxEmitter;

impl HelmEmitter for SandboxEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        // Refused rather than skipped: an empty fragment is a chart with no NetworkPolicy, and a
        // sandbox pod without one has the unrestricted egress this emitter exists to prevent.
        let sandbox = ctx
            .resource
            .config
            .downcast_ref::<Sandbox>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: ctx.resource_id.to_string(),
                    expected: Sandbox::RESOURCE_TYPE,
                    actual: ctx.resource.config.resource_type(),
                })
            })?;

        let mut fragment = HelmFragment::empty();
        fragment.extra_templates.insert(
            format!("sandbox-{}-networkpolicy.yaml", sandbox.id()),
            network_policy(sandbox, ctx.stack)?,
        );
        fragment
            .extra_templates
            .insert(BROKER_RBAC_TEMPLATE.to_string(), broker_rbac());
        fragment.extra_templates.insert(
            RUNTIME_CLASS_HELPER_TEMPLATE.to_string(),
            format!("{RUNTIME_CLASS_HELPER}{SANDBOX_NAMESPACE_HELPER}"),
        );
        fragment.extra_templates.insert(
            SANDBOX_NAMESPACE_RBAC_TEMPLATE.to_string(),
            SANDBOX_NAMESPACE_RBAC.to_string(),
        );
        fragment.extra_templates.insert(
            KUBE_API_DISCOVERY_RBAC_TEMPLATE.to_string(),
            KUBE_API_DISCOVERY_RBAC.to_string(),
        );
        // Every egress mode needs the proof, because the session NetworkPolicy is what bounds a
        // session's reach in each of them. Under `internet` it confines a session to the gateway.
        // Under `allow` there is no gateway and the policy bounds the session's egress. Under
        // `none` it is the only thing keeping a session off the network: `Egress` is listed with
        // no rules. The check only tests pod-to-pod traffic on the sandbox nodes; node metadata
        // is out of its scope. The gateway and metadata checks the Operator runs cover one path
        // each, not this policy.
        //
        // The proof is the `sandbox-cni-probe.yaml` pre-install and pre-upgrade hook: when it
        // fails, Helm stops before applying any other manifest, so no Operator runs and no
        // session is created. On a CNI that ignores NetworkPolicy the policy is not in force.
        fragment.extra_templates.insert(
            "sandbox-cni-probe.yaml".to_string(),
            super::sandbox_cni::template(true),
        );
        fragment.extra_templates.insert(
            KUBE_DNS_DISCOVERY_RBAC_TEMPLATE.to_string(),
            KUBE_DNS_DISCOVERY_RBAC.to_string(),
        );
        // Only a root-boot image needs a namespace that admits Pod Security `baseline`, and only
        // then does the install check for one. A non-root sandbox renders exactly what it did.
        if sandbox.image_user == Some(0) {
            fragment.extra_templates.insert(
                super::sandbox_root_boot_check::TEMPLATE.to_string(),
                super::sandbox_root_boot_check::template(),
            );
        }
        Ok(fragment)
    }
}

/// The policy that makes a sandbox's declared egress real.
///
/// Inbound is the agent port and nothing else, reachable from the operator and from the
/// workloads that link the sandbox: that is the port an application drives a session over
/// (commands, files, policy delivery), and denying it leaves every such call dropped on a cluster
/// whose CNI enforces policy. A workload that does not link the sandbox gets no reach. A preview port stays unreachable,
/// because that needs a gateway validating a session-and-port capability and none exists. Under
/// `deny`, `Egress` is listed with no rules — a listed policy type with no rule is how
/// NetworkPolicy spells "none", where omitting the type would mean "unrestricted".
fn network_policy(sandbox: &Sandbox, stack: &Stack) -> Result<String> {
    // Runtime pods carry the operator's resource prefix as their deployment label, and their
    // resource id under the same label domain. The id is quoted: a valid id such as `true`
    // would otherwise be read as a YAML boolean, which `matchLabels` refuses.
    let linkers: String = stack
        .linkers(&ResourceRef::new(Sandbox::RESOURCE_TYPE, sandbox.id()))
        .iter()
        .map(|linker| {
            format!(
                r#"        - {{{{ $peerNamespace }}}}podSelector:
            matchLabels:
              {{{{ $scopeKey }}}}: {{{{ $scopeValue | quote }}}}
              {{{{ printf "%s/resource" (trimSuffix "/deployment" $scopeKey) }}}}: "{id}"
              managed-by: runtime
"#,
                id = linker.id()
            )
        })
        .collect();
    let egress = match sandbox.egress {
        SandboxEgress::None => String::new(),
        // Select the Service's backing pods in this namespace and release. Kubernetes
        // NetworkPolicy selects pods, not Service names; an ipBlock for the Service IP
        // would depend on whether the CNI applies policy before or after DNAT.
        // The operator supplies the gateway IP, so sessions need no DNS exception.
        // Runtime pods carry the operator's resource prefix as their deployment label.
        SandboxEgress::Internet => r#"  egress:
    - to:
        - {{ $peerNamespace }}podSelector:
            matchLabels:
              {{ $scopeKey }}: {{ $scopeValue | quote }}
              {{ printf "%s/resource" (trimSuffix "/deployment" $scopeKey) }}: sandbox-egress-gateway
              component: container
              managed-by: runtime
      ports:
        - protocol: TCP
          port: 3128
"#
        .to_string(),
        SandboxEgress::Removed(_) => {
            sandbox.egress.ensure_supported(&sandbox.id)?;
            unreachable!("a removed egress mode is always refused")
        }
    };

    Ok(format!(
        r#"{{{{- $scopeKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}}}
{{{{- $scopeValue := include "deployment.serviceAccountPrefix" . -}}}}
{{{{- $peerNamespace := "" -}}}}
{{{{- if ne (include "deployment.sandboxNamespace" .) .Release.Namespace -}}}}
{{{{- $peerNamespace = printf "namespaceSelector:\n            matchLabels:\n              kubernetes.io/metadata.name: %s\n          " .Release.Namespace -}}}}
{{{{- end -}}}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: alien-sbx-{id}
  namespace: {{{{ include "deployment.sandboxNamespace" . }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
spec:
  podSelector:
    matchLabels:
      {label}: {id}
  policyTypes:
    - Ingress
    - Egress
  ingress:
    - from:
        - {{{{ $peerNamespace }}}}podSelector:
            matchLabels:
              app.kubernetes.io/name: {{{{ include "deployment.name" . }}}}
              app.kubernetes.io/instance: {{{{ .Release.Name }}}}
{linkers}      ports:
        - protocol: TCP
          port: {agent_port}
{egress}"#,
        id = sandbox.id(),
        label = LABEL_SANDBOX,
        agent_port = AGENT_PORT,
    ))
}

/// Cluster-scoped RBAC for the session broker.
///
/// A caller proves it is the workload by presenting the token already mounted in its own pod, and
/// the broker checks it with a `TokenReview`. `TokenReview` is a cluster-scoped subresource, so a
/// namespaced Role cannot grant it and authorization fails closed at the first claim without this.
fn broker_rbac() -> String {
    // Cluster-scoped names must differ per install: one cluster can run several copies of the
    // same chart in different namespaces, or several charts in one namespace, and a shared name
    // would refuse the second install or rebind the role away from the first. The full name is
    // truncated to fit, so the hash covers the namespace and the whole full name.
    r#"{{- $suffix := printf "sandbox-broker-%s" (printf "%s/%s" .Release.Namespace (include "deployment.fullname" .) | sha256sum | trunc 8) -}}
{{- $name := printf "%s-%s" (include "deployment.fullname" . | trunc (int (sub 62 (len $suffix))) | trimSuffix "-") $suffix -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ $name }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: ["authentication.k8s.io"]
    resources: ["tokenreviews"]
    verbs: ["create"]
  # The controller refuses to provision before it has seen a sandboxed RuntimeClass
  # (gVisor or Kata) on the cluster, and RuntimeClasses are cluster-scoped.
  - apiGroups: ["node.k8s.io"]
    resources: ["runtimeclasses"]
    verbs: ["list"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ $name }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ $name }}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{SandboxCode, SandboxLifecyclePolicy};

    /// Under `internet` the gateway peer must be selectable in the release namespace when the
    /// sandbox pods live elsewhere, like the operator and linker peers.
    #[test]
    fn the_gateway_peer_follows_the_release_namespace() {
        let sandbox = Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "example.invalid/agent:1".to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(300),
                idle_pause_seconds: None,
                ..Default::default()
            })
            .build();
        let stack = Stack::new("s".to_string()).build();
        let policy = network_policy(&sandbox, &stack).expect("policy renders");
        let gateway = policy
            .split("sandbox-egress-gateway")
            .next()
            .expect("gateway peer present");
        assert!(
            gateway
                .rsplit("- to:")
                .next()
                .is_some_and(|peer| peer.contains("{{ $peerNamespace }}podSelector")),
            "{policy}"
        );
    }
}
