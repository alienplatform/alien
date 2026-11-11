//! Sandbox emitter — the NetworkPolicy that bounds a session, and the cluster-scoped RBAC the
//! operator needs to provision one and its broker needs to authenticate one.
//!
//! Both belong to Helm rather than to the operator: the operator does not create
//! NetworkPolicies, and a `TokenReview` and the RuntimeClass listing the controller checks before
//! provisioning are cluster-scoped while the operator is namespace-scoped with no cluster-admin.
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
    import::EmitContext, non_public_ipv4_ranges, ErrorData, ResourceRef, Result, Sandbox,
    SandboxEgress, Stack,
};
use alien_error::AlienError;

/// Label the operator puts on every pod backing a session, and what the policy selects on.
const LABEL_SANDBOX: &str = "alien.dev/sandbox";

/// Reads `sandbox.clusterDNS` for an allow-egress policy and stops the render when it is missing,
/// so an upgrade that would leave sandboxes with no name resolution never applies and the running
/// release keeps its policy.
const CLUSTER_DNS_VALUES: &str = r#"{{- $dns := dig "clusterDNS" (list) (default dict .Values.sandbox) -}}
{{- if not $dns -}}
{{- fail "sandbox.clusterDNS is not set. A sandbox with allow egress needs the address of the cluster DNS service (the nameserver in a pod's /etc/resolv.conf). Find it with: kubectl run dns-probe --rm -i --restart=Never --image=busybox -- grep nameserver /etc/resolv.conf and set sandbox.clusterDNS=[<that address>]." -}}
{{- end -}}
"#;

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
            RUNTIME_CLASS_HELPER.to_string(),
        );
        if network_policy_is_the_only_bound(&sandbox.egress) {
            fragment.extra_templates.insert(
                "sandbox-cni-probe.yaml".to_string(),
                super::sandbox_cni::template(),
            );
        }
        Ok(fragment)
    }
}

/// Whether this egress mode's reach is bounded by the session NetworkPolicy and nothing else, so
/// the chart must prove the cluster enforces NetworkPolicy before it installs or upgrades.
///
/// The proof is the `sandbox-cni-probe.yaml` pre-install and pre-upgrade hook: when it fails, Helm
/// stops before applying any other manifest, so no Operator runs and no session is created. Under
/// `internet` the policy is what confines a session to the gateway. Under `allow` there is no
/// gateway, and the policy's address exclusions are all that keep a session off node metadata,
/// node credentials and private ranges; on a CNI that ignores NetworkPolicy they are not in force.
fn network_policy_is_the_only_bound(egress: &SandboxEgress) -> bool {
    matches!(egress, SandboxEgress::Internet | SandboxEgress::Allow)
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
                r#"        - podSelector:
            matchLabels:
              {{{{ $scopeKey }}}}: {{{{ $scopeValue | quote }}}}
              {{{{ printf "%s/resource" (trimSuffix "/deployment" $scopeKey) }}}}: "{id}"
              managed-by: runtime
"#,
                id = linker.id()
            )
        })
        .collect();
    let mut dns_values = String::new();
    let egress = match sandbox.egress {
        SandboxEgress::None => String::new(),
        // Select the Service's backing pods in this namespace and release. Kubernetes
        // NetworkPolicy selects pods, not Service names; an ipBlock for the Service IP
        // would depend on whether the CNI applies policy before or after DNAT.
        // The operator supplies the gateway IP, so sessions need no DNS exception.
        // Runtime pods carry the operator's resource prefix as their deployment label.
        SandboxEgress::Internet => r#"  egress:
    - to:
        - podSelector:
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
        SandboxEgress::AllowDomains { .. } => {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: format!("generate the Helm chart for sandbox '{}'", sandbox.id),
                reason: "a Kubernetes NetworkPolicy matches addresses, not hostnames".to_string(),
            }));
        }
        SandboxEgress::Allow => {
            // Every range the egress gateway refuses as non-public: private, shared (100.64/10),
            // loopback, all of link-local (instance metadata, ECS and EKS Pod Identity
            // credentials), Azure's wire server, and reserved and multicast space. No rule names
            // an IPv6 block, so no IPv6 destination is reachable.
            let excepts: String = non_public_ipv4_ranges()
                .map(|range| format!("            - {range}\n"))
                .collect();
            // DNS goes to two places, port 53 only. A CNI that checks policy after the Service
            // address is translated (kindnet, Calico, Cilium) sees the DNS pod's address, so the
            // kube-dns pods are admitted by selector. A resolver that is no pod (EKS Auto Mode's
            // service address, a node-local cache) is admitted by the address in
            // `sandbox.clusterDNS`, which Helm cannot look up under Argo CD. Port 53 to any other
            // private host, and every other port on these, stays refused.
            dns_values = CLUSTER_DNS_VALUES.to_string();
            let dns = "    - to:\n        - namespaceSelector:\n            matchLabels:\n              kubernetes.io/metadata.name: kube-system\n          podSelector:\n            matchLabels:\n              k8s-app: kube-dns\n{{- range $dns }}\n        - ipBlock:\n            cidr: {{ printf \"%s/32\" . }}\n{{- end }}\n      ports:\n        - protocol: UDP\n          port: 53\n        - protocol: TCP\n          port: 53\n";
            format!(
                "  egress:\n{dns}    - to:\n        - ipBlock:\n            cidr: 0.0.0.0/0\n            except:\n{excepts}"
            )
        }
    };

    Ok(format!(
        r#"{dns_values}{{{{- $scopeKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}}}
{{{{- $scopeValue := include "deployment.serviceAccountPrefix" . -}}}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: alien-sbx-{id}
  namespace: {{{{ .Release.Namespace }}}}
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
        - podSelector:
            matchLabels:
              app.kubernetes.io/name: {{{{ include "deployment.name" . }}}}
              app.kubernetes.io/instance: {{{{ .Release.Name }}}}
{linkers}      ports:
        - protocol: TCP
          port: {agent_port}
{egress}"#,
        id = sandbox.id(),
        dns_values = dns_values,
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
