//! The NetworkPolicy that bounds the pods the Operator runs extension controllers in.
//!
//! It belongs to the chart, as a sandbox's does: the policy admits the Operator by its pod's
//! labels, which only the chart knows, and it exists before the Operator creates a pod. The
//! Operator creates none where the policy is missing.

/// The port a controller's pod serves steps on, as the Operator creates it.
const CONTROLLER_PORT: u16 = 8080;

/// Selects every controller pod of this release by the labels the Operator gives them. Only the
/// Operator's pod reaches one, on the port it serves steps on. A pod reaches DNS and any address
/// but the node's metadata address, which would answer with the node's identity.
pub(crate) fn network_policy() -> String {
    format!(
        r#"{{{{- $scopeKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}}}
{{{{- $scopeValue := include "deployment.serviceAccountPrefix" . -}}}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{{{ printf "%s-extension-controllers" $scopeValue | trunc 63 | trimSuffix "-" }}}}
  namespace: {{{{ .Release.Namespace }}}}
  labels:
    {{{{- include "deployment.labels" . | nindent 4 }}}}
spec:
  podSelector:
    matchLabels:
      {{{{ $scopeKey }}}}: {{{{ $scopeValue | quote }}}}
      component: extension-controller
      managed-by: runtime
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
          port: {CONTROLLER_PORT}
  egress:
    # By selector: a rule on addresses does not match the cluster's own pods on every CNI.
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
          podSelector:
            matchLabels:
              k8s-app: kube-dns
      ports:
        - protocol: UDP
          port: 53
        - protocol: TCP
          port: 53
    - to:
        - ipBlock:
            cidr: 0.0.0.0/0
            except:
              - 169.254.169.254/32
"#
    )
}
