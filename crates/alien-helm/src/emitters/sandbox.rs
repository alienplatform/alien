//! Sandbox emitter — the NetworkPolicy that bounds a session, and the cluster-scoped RBAC the
//! broker needs to authenticate one.
//!
//! Both belong to Helm rather than to the operator: the operator does not create
//! NetworkPolicies, and a `TokenReview` is cluster-scoped while the operator is namespace-scoped
//! with no cluster-admin. Build has the same shape and is the precedent.
//!
//! No ServiceAccount is emitted, unlike Build's. A sandbox pod runs with
//! `automountServiceAccountToken: false` — a mounted token is a credential the untrusted code can
//! read — so a dedicated account would grant it nothing that `default` does not, and would be a
//! name for the operator and the chart to keep in step for no gain. Build's account exists because
//! a build genuinely needs identity to push images.

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::sandbox_process::AGENT_PORT;
use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxEgress};
use alien_error::AlienError;

/// Label the operator puts on every pod backing a session, and what the policy selects on.
const LABEL_SANDBOX: &str = "alien.dev/sandbox";

/// Addresses no sandbox may reach, in **either** egress mode.
///
/// The metadata server is the one that matters. gVisor is a kernel boundary, not a network
/// boundary, so it does nothing about routing and link-local has to be denied explicitly rather
/// than assumed unreachable. Cloud Run and Azure block it at the platform; Kubernetes does not.
///
/// **The metadata entry is a `/32`, not the `169.254.0.0/16` it sits in.** GKE puts NodeLocal
/// DNSCache at `169.254.20.10`, so denying the whole range takes DNS out and with it every
/// outbound connection, which makes `allow` indistinguishable from `deny`.
const ALWAYS_DENIED_CIDRS: &[&str] = &[
    "169.254.169.254/32",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
];

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
            network_policy(sandbox),
        );
        fragment
            .extra_templates
            .insert(BROKER_RBAC_TEMPLATE.to_string(), broker_rbac());
        Ok(fragment)
    }
}

/// The policy that makes a sandbox's declared egress real.
///
/// Inbound is the agent port and nothing else, reachable only from pods in this release: that is
/// the port the application drives a session over, and denying it outright leaves every exec and
/// file call dropped on a cluster whose CNI enforces policy. A preview port stays unreachable,
/// because that needs a gateway validating a session-and-port capability and none exists. Under
/// `deny`, `Egress` is listed with no rules — a listed policy type with no rule is how
/// NetworkPolicy spells "none", where omitting the type would mean "unrestricted".
fn network_policy(sandbox: &Sandbox) -> String {
    let egress = match sandbox.egress {
        SandboxEgress::Deny => String::new(),
        // A hostname allowlist is not expressible here — NetworkPolicy matches CIDRs — which is
        // why Kubernetes publishes `domainEgressRules: false` rather than approximating one.
        SandboxEgress::Allow | SandboxEgress::AllowDomains { .. } => {
            let excepts: String = ALWAYS_DENIED_CIDRS
                .iter()
                .map(|cidr| format!("            - {cidr}\n"))
                .collect();
            // DNS first, by selector rather than address. The excepts below cover the private
            // ranges a cluster's DNS service lives in — 172.20/16 on EKS, 10.0/16 on AKS,
            // 10.96/12 on kubeadm — so an address-based rule alone resolves no names anywhere
            // except a cluster whose resolver sits on a link-local address.
            let dns = "    - to:\n        - namespaceSelector:\n            matchLabels:\n              kubernetes.io/metadata.name: kube-system\n          podSelector:\n            matchLabels:\n              k8s-app: kube-dns\n      ports:\n        - protocol: UDP\n          port: 53\n        - protocol: TCP\n          port: 53\n";
            format!(
                "  egress:\n{dns}    - to:\n        - ipBlock:\n            cidr: 0.0.0.0/0\n            except:\n{excepts}"
            )
        }
    };

    format!(
        r#"apiVersion: networking.k8s.io/v1
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
      ports:
        - protocol: TCP
          port: {agent_port}
{egress}"#,
        id = sandbox.id(),
        label = LABEL_SANDBOX,
        agent_port = AGENT_PORT,
    )
}

/// Cluster-scoped RBAC for the session broker.
///
/// A caller proves it is the workload by presenting the token already mounted in its own pod, and
/// the broker checks it with a `TokenReview`. `TokenReview` is a cluster-scoped subresource, so a
/// namespaced Role cannot grant it and authorization fails closed at the first claim without this.
fn broker_rbac() -> String {
    r#"apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "deployment.fullname" . }}-sandbox-broker
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: ["authentication.k8s.io"]
    resources: ["tokenreviews"]
    verbs: ["create"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "deployment.fullname" . }}-sandbox-broker
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "deployment.fullname" . }}-sandbox-broker
"#
    .to_string()
}
