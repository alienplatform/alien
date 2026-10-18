//! Setup-owned, short-lived NetworkPolicy enforcement gate.
//!
//! This samples pod-to-pod traffic on the nodes selected by the scheduler. It does
//! not certify every node, address family, or CNI exception for traffic to the host.

pub(super) fn template() -> String {
    let script = include_str!("sandbox_cni_probe.py")
        .lines()
        .map(|line| format!("              {line}\n"))
        .collect::<String>();
    format!(
        r#"{{{{- $name := printf "%s-sandbox-cni" (include "deployment.fullname" . | trunc 51 | trimSuffix "-") -}}}}
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
  - apiGroups: [""]
    resources: [pods]
    verbs: [create, get, delete]
  - apiGroups: [networking.k8s.io]
    resources: [networkpolicies]
    verbs: [create, delete]
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
  activeDeadlineSeconds: 240
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
          image: python:3.13-slim
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
              value: python:3.13-slim
          command:
            - python3
            - -B
            - -c
            - |
{script}"#
    )
}
