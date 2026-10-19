//! Setup-owned, short-lived NetworkPolicy enforcement gate.
//!
//! This samples pod-to-pod traffic on the nodes selected by the scheduler, with the
//! subject pods in the sandbox runtime class. It does not certify every node, address
//! family, or CNI exception for traffic to the host.
//!
//! The probe must create pods, which Kubernetes RBAC cannot restrict by content. A
//! ValidatingAdmissionPolicy, created before the probe's account and bound to it alone,
//! admits only token-less, volume-less pods of the pinned probe image under the probe's
//! own names, and the probe refuses to start unless that policy demonstrably rejects a
//! pod that asks for a token. The admission policy API needs Kubernetes 1.30 or later;
//! an older API server refuses the install.

/// Digest-pinned probe image (python:3.13-slim, multi-architecture index).
pub(crate) const PROBE_IMAGE: &str = "docker.io/library/python@sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f";

/// Runtime class the Kubernetes sandbox controller schedules sessions into.
const SANDBOX_RUNTIME_CLASS: &str = "gvisor";

/// Pods the probe creates, named `<job>-<role>`.
const PROBE_ROLES: &[&str] = &[
    "listener",
    "control",
    "denied",
    "control-after",
    "ingress-denied",
    "ingress-control-after",
    "admission-check",
];

pub(crate) fn template() -> String {
    let script = include_str!("sandbox_cni_probe.py")
        .lines()
        .map(|line| format!("              {line}\n"))
        .collect::<String>();
    let pod_names = PROBE_ROLES
        .iter()
        .map(|role| format!("{{{{ printf \"%s-{role}\" $name | quote }}}}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{{{{- $name := printf "%s-sandbox-cni" (include "deployment.fullname" . | trunc 51 | trimSuffix "-") -}}}}
{{{{- $cluster := printf "%s-%s" $name (sha256sum (printf "%s/%s" .Release.Namespace .Release.Name) | trunc 8) -}}}}
apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingAdmissionPolicy
metadata:
  name: {{{{ $cluster }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-12"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
spec:
  failurePolicy: Fail
  matchConstraints:
    resourceRules:
      - apiGroups: [""]
        apiVersions: [v1]
        operations: [CREATE, UPDATE]
        resources: [pods]
  matchConditions:
    - name: probe-account
      expression: {{{{ printf "request.userInfo.username == 'system:serviceaccount:%s:%s'" .Release.Namespace $name | quote }}}}
  validations:
    - expression: {{{{ printf "object.metadata.name.startsWith('%s-')" $name | quote }}}}
      message: probe pods use the probe's own names
    - expression: "object.spec.serviceAccountName == 'default' && has(object.spec.automountServiceAccountToken) && object.spec.automountServiceAccountToken == false"
      message: probe pods carry no service account token
    - expression: "!has(object.spec.volumes) || size(object.spec.volumes) == 0"
      message: probe pods mount nothing
    - expression: "!has(object.spec.initContainers) && !has(object.spec.ephemeralContainers) && size(object.spec.containers) == 1"
      message: probe pods run one container
    - expression: "object.spec.containers.all(c, c.image == '{image}' && (!has(c.securityContext) || !has(c.securityContext.privileged) || c.securityContext.privileged == false))"
      message: probe pods run the pinned probe image unprivileged
    - expression: "(!has(object.spec.hostNetwork) || !object.spec.hostNetwork) && (!has(object.spec.hostPID) || !object.spec.hostPID) && (!has(object.spec.hostIPC) || !object.spec.hostIPC)"
      message: probe pods use no host namespaces
---
apiVersion: admissionregistration.k8s.io/v1
kind: ValidatingAdmissionPolicyBinding
metadata:
  name: {{{{ $cluster }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-12"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
spec:
  policyName: {{{{ $cluster }}}}
  validationActions: [Deny]
  matchResources:
    namespaceSelector:
      matchLabels:
        kubernetes.io/metadata.name: {{{{ .Release.Namespace }}}}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-10"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-10"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
rules:
  # Creation cannot be scoped by name; the admission policy above confines it.
  - apiGroups: [""]
    resources: [pods]
    verbs: [create]
  - apiGroups: [""]
    resources: [pods]
    resourceNames: [{pod_names}]
    verbs: [get, delete]
  - apiGroups: [networking.k8s.io]
    resources: [networkpolicies]
    verbs: [create]
  - apiGroups: [networking.k8s.io]
    resources: [networkpolicies]
    resourceNames: [{{{{ $name | quote }}}}, {{{{ printf "%s-ingress" $name | quote }}}}]
    verbs: [get, delete]
  - apiGroups: [batch]
    resources: [jobs]
    resourceNames: [{{{{ $name | quote }}}}]
    verbs: [get]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-9"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
subjects:
  - kind: ServiceAccount
    name: {{{{ $name }}}}
    namespace: {{{{ .Release.Namespace }}}}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{{{ $name }}}}
---
apiVersion: batch/v1
kind: Job
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-8"
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
spec:
  backoffLimit: 0
  activeDeadlineSeconds: 300
  ttlSecondsAfterFinished: 600
  template:
    spec:
      serviceAccountName: {{{{ $name }}}}
      restartPolicy: Never
      securityContext:
        runAsNonRoot: true
        runAsUser: 65534
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: probe
          image: {image}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: [ALL]
          resources:
            requests:
              cpu: 10m
              memory: 24Mi
            limits:
              cpu: 100m
              memory: 64Mi
          env:
            - name: PROBE_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: PROBE_JOB
              value: {{{{ $name | quote }}}}
            - name: PROBE_IMAGE
              value: {image}
            - name: PROBE_RUNTIME_CLASS
              value: {runtime_class}
          command:
            - python3
            - -B
            - -c
            - |
{script}"#,
        image = PROBE_IMAGE,
        runtime_class = SANDBOX_RUNTIME_CLASS,
    )
}
