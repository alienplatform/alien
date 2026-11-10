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

/// Link-local space, where node-local resolvers answer: NodeLocal DNSCache at 169.254.20.10 by
/// default, AKS LocalDNS at 169.254.10.10 and .11, or an address the cluster chose. Under `allow`
/// it is reachable on port 53 only. The metadata and credential services in this range (IMDS at
/// .169.254, ECS at .170.2, EKS Pod Identity at .170.23) serve them over HTTP, not port 53; what
/// does answer on 53 there (the AWS VPC resolver at .169.253, the GCP metadata server's resolver)
/// is DNS, the upstream the cluster's own resolver already forwards to.
const LINK_LOCAL_DNS: &str = "169.254.0.0/16";

/// Path the broker's RBAC lands at. Fixed rather than per-sandbox: the ClusterRole is one per
/// deployment, so a second sandbox rewrites the same file instead of colliding on the name.
const BROKER_RBAC_TEMPLATE: &str = "sandbox-broker-rbac.yaml";

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
            // DNS by selector rather than address: the excepts cover the private ranges a
            // cluster's DNS service lives in — 172.20/16 on EKS, 10.0/16 on AKS, 10.96/12 on
            // kubeadm. A cluster with a node-local resolver points pods at a link-local
            // address instead; link-local is admitted on port 53 alone, so resolution keeps
            // working without opening anything else the node serves there.
            let dns_ports = "      ports:\n        - protocol: UDP\n          port: 53\n        - protocol: TCP\n          port: 53\n";
            let dns = format!(
                "    - to:\n        - namespaceSelector:\n            matchLabels:\n              kubernetes.io/metadata.name: kube-system\n          podSelector:\n            matchLabels:\n              k8s-app: kube-dns\n{dns_ports}    - to:\n        - ipBlock:\n            cidr: {LINK_LOCAL_DNS}\n{dns_ports}"
            );
            format!(
                "  egress:\n{dns}    - to:\n        - ipBlock:\n            cidr: 0.0.0.0/0\n            except:\n{excepts}"
            )
        }
    };

    Ok(format!(
        r#"{{{{- $scopeKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}}}
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
