//! Install-time check that a root-boot sandbox's namespace admits it.
//!
//! A root-boot sandbox pod needs Pod Security `baseline`. Left to the Operator it would show up
//! as a pool that never fills, long after the install. The hook runs a server-side dry-run of a
//! pod shaped like one, in the namespace the sandboxes will use, and fails the install or
//! upgrade with the namespace and the one command that fixes it. The Operator's own runtime
//! check stays as a second line for a namespace relabelled later.
//!
//! The only right it needs is `create pods` in that namespace (a dry-run stores nothing), so it
//! reads no cluster-scoped object. Pod Security admits or refuses on the pod's shape alone, so
//! the pinned probe image stands in for the sandbox image, which is never pulled.

use super::sandbox_cni::{PROBE_DIGEST, PROBE_REPOSITORY};

pub(crate) const TEMPLATE: &str = "sandbox-root-boot-check.yaml";

const HOOK: &str = "    helm.sh/hook: pre-install,pre-upgrade\n    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded";

pub(crate) fn template() -> String {
    let capabilities = alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_SET
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(",");
    let script = include_str!("sandbox_root_boot_check.py")
        .lines()
        .map(|line| format!("              {line}\n"))
        .collect::<String>();
    let hook = |weight: &str| {
        HOOK.replace(
            "hook-delete",
            &format!("hook-weight: \"{weight}\"\n    helm.sh/hook-delete"),
        )
    };
    format!(
        r#"{{{{- $name := printf "%s-root-boot-check" (include "deployment.fullname" . | trunc 45 | trimSuffix "-") -}}}}
{{{{- $sandboxNamespace := include "deployment.sandboxNamespace" . -}}}}
{{{{- $probeImage := include "deployment.image" (dict "root" . "repository" "{repository}" "digest" "{digest}") -}}}}
{{{{- with dig "cniProbeImage" "" (default dict .Values.sandbox) }}}}{{{{ $probeImage = include "deployment.mirroredImage" (dict "root" $ "image" .) }}}}{{{{ end -}}}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
{hook_sa}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ $sandboxNamespace }}}}
  annotations:
{hook_sa}
rules:
  - apiGroups: [""]
    resources: [pods]
    verbs: [create]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ $sandboxNamespace }}}}
  annotations:
{hook_binding}
subjects:
  - kind: ServiceAccount
    name: {{{{ $name }}}}
    namespace: {{{{ .Release.Namespace }}}}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{{{ $name }}}}
---
# A cluster that denies pod traffic by default leaves the check with no route to the API server.
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
{hook_binding}
spec:
  podSelector:
    matchLabels:
      alien.dev/root-boot-check: {{{{ $name }}}}
  policyTypes: [Egress]
  egress:
    - ports:
        - protocol: TCP
          port: 443
        - protocol: TCP
          port: 6443
---
apiVersion: batch/v1
kind: Job
metadata:
  name: {{{{ $name }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
{hook_job}
spec:
  backoffLimit: 0
  activeDeadlineSeconds: 120
  ttlSecondsAfterFinished: 600
  template:
    metadata:
      labels:
        alien.dev/root-boot-check: {{{{ $name }}}}
    spec:
      serviceAccountName: {{{{ $name }}}}
      restartPolicy: Never
      securityContext:
        runAsNonRoot: true
        runAsUser: 65534
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: check
          image: {{{{ $probeImage | quote }}}}
          terminationMessagePolicy: FallbackToLogsOnError
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
            - name: SANDBOX_NAMESPACE
              value: {{{{ $sandboxNamespace | quote }}}}
            - name: SANDBOX_RUNTIME_CLASS
              value: {{{{ include "deployment.sandboxRuntimeClassName" . | quote }}}}
            - name: PROBE_IMAGE
              value: {{{{ $probeImage | quote }}}}
            - name: BOOT_CAPABILITIES
              value: {capabilities}
          command:
            - python3
            - -c
            - |
{script}"#,
        repository = PROBE_REPOSITORY,
        digest = PROBE_DIGEST,
        hook_sa = hook("-10"),
        hook_binding = hook("-9"),
        hook_job = hook("-8"),
    )
}
