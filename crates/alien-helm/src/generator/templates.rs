pub(super) fn helpers_tpl() -> String {
    r#"{{- define "deployment.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "deployment.labels" -}}
app.kubernetes.io/name: {{ include "deployment.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "deployment.managerServiceAccountName" -}}
{{- $prefix := default (include "deployment.fullname" .) .Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-manager-sa" $prefix | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.serviceAccountName" -}}
{{- $prefix := default (include "deployment.fullname" .root) .root.Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-%s-sa" $prefix .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.resourceName" -}}
{{- $raw := .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.managementSecretName" -}}
{{- default (include "deployment.fullname" .) .Values.management.existingSecret.name -}}
{{- end -}}

{{- define "deployment.managementSecretTokenKey" -}}
{{- default "sync-token" .Values.management.existingSecret.tokenKey -}}
{{- end -}}

{{- define "deployment.encryptionSecretName" -}}
{{- default (include "deployment.fullname" .) .Values.runtime.encryption.existingSecret.name -}}
{{- end -}}

{{- define "deployment.encryptionSecretKey" -}}
{{- default "encryption-key" .Values.runtime.encryption.existingSecret.key -}}
{{- end -}}

{{- define "deployment.heartbeatNodeClusterRoleName" -}}
{{- printf "%s-heartbeat-nodes" (include "deployment.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
"#
    .to_string()
}

pub(super) fn serviceaccount_tpl() -> String {
    r#"apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.managerServiceAccountName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    {{- with .Values.managerServiceAccount.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with .Values.managerServiceAccount.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- range $name, $account := .Values.serviceAccounts }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
    {{- with $account.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with $account.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- end }}
"#
    .to_string()
}

pub(super) fn role_tpl() -> String {
    r#"{{- $stackSettings := default dict .Values.stackSettings -}}
{{- $exposure := dig "kubernetes" "exposure" dict $stackSettings -}}
{{- $exposureMode := dig "mode" "" $exposure -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "services", "pods", "pods/log", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["networkpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["gateway.networking.k8s.io"]
    resources: ["gateways", "httproutes"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- $route := dig "route" dict $exposure -}}
  {{- $routeApi := dig "routeApi" "" $route -}}
  {{- if and (ne $exposureMode "disabled") (eq $routeApi "gateway") }}
  - apiGroups: ["networking.gke.io"]
    resources: ["healthcheckpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["alb.networking.azure.io"]
    resources: ["healthcheckpolicy"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- end }}
"#
    .to_string()
}

pub(super) fn rolebinding_tpl() -> String {
    r#"apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.fullname" . }}
---
{{- range $name, $account := .Values.serviceAccounts }}
{{- $rbac := default dict $account.rbac }}
{{- $rules := default list $rbac.rules }}
{{- if $rules }}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
rules:
{{- toYaml $rules | nindent 2 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
---
{{- end }}
{{- end }}
"#
    .to_string()
}

pub(super) fn clusterrole_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
{{- end }}
"#
    .to_string()
}

pub(super) fn clusterrolebinding_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
{{- end }}
"#
    .to_string()
}

pub(super) fn secret_tpl() -> String {
    r#"{{- $createManagementSecret := not .Values.management.existingSecret.name -}}
{{- $createEncryptionSecret := not .Values.runtime.encryption.existingSecret.name -}}
{{- if or $createManagementSecret $createEncryptionSecret .Values.infrastructure .Values.logCollector.enabled }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
type: Opaque
stringData:
  {{- if $createManagementSecret }}
  sync-token: {{ .Values.management.token | quote }}
  {{- end }}
  {{- if $createEncryptionSecret }}
  encryption-key: {{ required "runtime.encryption.key or runtime.encryption.existingSecret.name is required" .Values.runtime.encryption.key | quote }}
  {{- end }}
  {{- if .Values.infrastructure }}
  external-bindings.json: {{ toJson .Values.infrastructure | quote }}
  {{- end }}
  {{- if .Values.logCollector.enabled }}
  collector-token: {{ required "logCollector.token is required when logCollector.enabled=true" .Values.logCollector.token | quote }}
  {{- end }}
{{- end }}
"#
    .to_string()
}

pub(super) fn configmap_tpl() -> String {
    r#"{{- $defaultStackSettings := dict "deploymentModel" "pull" "updates" .Values.management.updates "telemetry" .Values.management.telemetry "heartbeats" .Values.management.healthChecks -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
data:
  stack.json: |-
{{ .Files.Get "files/stack.json" | indent 4 }}
  stack-settings.json: {{ toJson (default $defaultStackSettings .Values.stackSettings) | quote }}
  services.json: {{ toJson .Values.services | quote }}
  public-endpoints.json: {{ toJson (default dict .Values.publicEndpoints) | quote }}
"#
    .to_string()
}

pub(super) fn cleanup_job_tpl() -> String {
    r#"{{- $cleanup := dig "cleanup" "onUninstall" dict .Values.runtime -}}
{{- if dig "enabled" true $cleanup }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.fullname" . }}-cleanup
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-delete
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
spec:
  backoffLimit: 1
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      restartPolicy: Never
      containers:
        - name: cleanup
          image: "{{ dig "image" "repository" "alpine/k8s" $cleanup }}:{{ dig "image" "tag" "1.32.0" $cleanup }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" $cleanup }}
          command:
            - /bin/sh
            - -ec
            - |
              selector='managed-by=runtime'
              kubectl -n {{ .Release.Namespace | quote }} delete deployments.apps,statefulsets.apps,daemonsets.apps,services,configmaps,secrets,networkpolicies.networking.k8s.io,ingresses.networking.k8s.io -l "$selector" --ignore-not-found=true
              if kubectl api-resources --api-group gateway.networking.k8s.io --no-headers 2>/dev/null | awk '{print $1}' | grep -qx 'httproutes'; then
                kubectl -n {{ .Release.Namespace | quote }} delete httproutes.gateway.networking.k8s.io -l "$selector" --ignore-not-found=true
              fi
              if kubectl api-resources --api-group gateway.networking.k8s.io --no-headers 2>/dev/null | awk '{print $1}' | grep -qx 'gateways'; then
                kubectl -n {{ .Release.Namespace | quote }} delete gateways.gateway.networking.k8s.io -l "$selector" --ignore-not-found=true
              fi
              {{- if dig "deletePersistentVolumeClaims" false $cleanup }}
              kubectl -n {{ .Release.Namespace | quote }} delete persistentvolumeclaims -l "$selector" --ignore-not-found=true
              {{- else }}
              echo "Preserving runtime PersistentVolumeClaims. Set runtime.cleanup.onUninstall.deletePersistentVolumeClaims=true to delete them."
              {{- end }}
{{- end }}
"#
    .to_string()
}

pub(super) fn deployment_tpl() -> String {
    r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.runtime.replicas }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
        {{- with .Values.runtime.podLabels }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- with .Values.runtime.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      automountServiceAccountToken: {{ .Values.runtime.automountServiceAccountToken }}
      securityContext:
        {{- toYaml .Values.runtime.security.podSecurityContext | nindent 8 }}
      {{- with .Values.runtime.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.topologySpreadConstraints }}
      topologySpreadConstraints:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- if .Values.runtime.scheduling.priorityClassName }}
      priorityClassName: {{ .Values.runtime.scheduling.priorityClassName | quote }}
      {{- end }}
      {{- if .Values.runtime.scheduling.runtimeClassName }}
      runtimeClassName: {{ .Values.runtime.scheduling.runtimeClassName | quote }}
      {{- end }}
      containers:
        - name: operator
          image: "{{ .Values.runtime.image.repository }}:{{ .Values.runtime.image.tag }}"
          imagePullPolicy: {{ .Values.runtime.image.pullPolicy }}
          securityContext:
            {{- toYaml .Values.runtime.security.containerSecurityContext | nindent 12 }}
          env:
            - name: PLATFORM
              value: kubernetes
            {{- if .Values.basePlatform }}
            - name: OPERATOR_BASE_PLATFORM
              value: {{ .Values.basePlatform | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "aws") .Values.basePlatformConfig.aws.region }}
            - name: AWS_REGION
              value: {{ .Values.basePlatformConfig.aws.region | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.projectId }}
            - name: GCP_PROJECT_ID
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            - name: GOOGLE_CLOUD_PROJECT
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.region }}
            - name: GCP_REGION
              value: {{ .Values.basePlatformConfig.gcp.region | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.subscriptionId }}
            - name: AZURE_SUBSCRIPTION_ID
              value: {{ .Values.basePlatformConfig.azure.subscriptionId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.tenantId }}
            - name: AZURE_TENANT_ID
              value: {{ .Values.basePlatformConfig.azure.tenantId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.location }}
            - name: AZURE_REGION
              value: {{ .Values.basePlatformConfig.azure.location | quote }}
            {{- end }}
            - name: SYNC_URL
              value: {{ .Values.management.url | quote }}
            - name: OPERATOR_NAME
              value: {{ .Values.management.name | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
            - name: OPERATOR_SETUP_METHOD
              value: "helm"
            - name: DATA_DIR
              value: {{ .Values.runtime.data.mountPath | quote }}
            - name: SYNC_TOKEN_FILE
              value: /etc/deployment/secrets/sync-token
            - name: OPERATOR_ENCRYPTION_KEY_FILE
              value: /etc/deployment/secrets/encryption-key
            - name: STACK_SETTINGS_FILE
              value: /etc/deployment/config/stack-settings.json
            - name: PUBLIC_ENDPOINTS_FILE
              value: /etc/deployment/config/public-endpoints.json
            {{- if .Values.infrastructure }}
            - name: EXTERNAL_BINDINGS_FILE
              value: /etc/deployment/secrets/external-bindings.json
            {{- end }}
            - name: SYNC_INTERVAL
              value: "30"
            - name: OTLP_PORT
              value: {{ .Values.runtime.api.port | quote }}
            - name: OTLP_HOST
              value: {{ .Values.runtime.api.bindHost | quote }}
            {{- if .Values.logCollector.enabled }}
            - name: COLLECTOR_TOKEN_FILE
              value: /etc/deployment/secrets/collector-token
            {{- end }}
          ports:
            - name: otlp
              containerPort: {{ .Values.runtime.api.port }}
          {{- if .Values.runtime.probes.liveness.enabled }}
          livenessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.liveness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.liveness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.liveness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.liveness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.liveness.failureThreshold }}
          {{- end }}
          {{- if .Values.runtime.probes.readiness.enabled }}
          readinessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.readiness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.readiness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.readiness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.readiness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.readiness.failureThreshold }}
          {{- end }}
          volumeMounts:
            - name: config
              mountPath: /etc/deployment/config
              readOnly: true
            - name: management-token
              mountPath: /etc/deployment/secrets/sync-token
              subPath: sync-token
              readOnly: true
            - name: encryption-key
              mountPath: /etc/deployment/secrets/encryption-key
              subPath: {{ include "deployment.encryptionSecretKey" . }}
              readOnly: true
            {{- if .Values.infrastructure }}
            - name: external-bindings
              mountPath: /etc/deployment/secrets/external-bindings.json
              subPath: external-bindings.json
              readOnly: true
            {{- end }}
            {{- if .Values.logCollector.enabled }}
            - name: collector-token
              mountPath: /etc/deployment/secrets/collector-token
              subPath: collector-token
              readOnly: true
            {{- end }}
            {{- if .Values.runtime.tmp.enabled }}
            - name: tmp
              mountPath: /tmp
            {{- end }}
            - name: runtime-data
              mountPath: {{ .Values.runtime.data.mountPath | quote }}
          resources:
            {{- toYaml .Values.runtime.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "deployment.fullname" . }}
        - name: management-token
          secret:
            secretName: {{ include "deployment.managementSecretName" . }}
            items:
              - key: {{ include "deployment.managementSecretTokenKey" . }}
                path: sync-token
            defaultMode: 384
        - name: encryption-key
          secret:
            secretName: {{ include "deployment.encryptionSecretName" . }}
            defaultMode: 384
        {{- if .Values.infrastructure }}
        - name: external-bindings
          secret:
            secretName: {{ include "deployment.fullname" . }}
            items:
              - key: external-bindings.json
                path: external-bindings.json
            defaultMode: 384
        {{- end }}
        {{- if .Values.logCollector.enabled }}
        - name: collector-token
          secret:
            secretName: {{ include "deployment.fullname" . }}
            items:
              - key: collector-token
                path: collector-token
            defaultMode: 384
        {{- end }}
        {{- if .Values.runtime.tmp.enabled }}
        - name: tmp
          emptyDir:
            sizeLimit: {{ .Values.runtime.tmp.sizeLimit | quote }}
        {{- end }}
        - name: runtime-data
          {{- if .Values.runtime.data.persistence.enabled }}
          persistentVolumeClaim:
            claimName: {{ default (printf "%s-runtime-data" (include "deployment.fullname" .)) .Values.runtime.data.persistence.existingClaim }}
          {{- else }}
          emptyDir: {}
          {{- end }}
"#
    .to_string()
}

pub(super) fn service_tpl() -> String {
    r#"{{- if or .Values.runtime.api.enabled .Values.logCollector.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  type: {{ .Values.runtime.api.service.type }}
  selector:
    app.kubernetes.io/name: {{ include "deployment.name" . }}
    app.kubernetes.io/instance: {{ .Release.Name }}
  ports:
    - name: http
      port: {{ .Values.runtime.api.port }}
      targetPort: otlp
{{- end }}
"#
    .to_string()
}

pub(super) fn whitelabeled_log_collector_serviceaccount_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
automountServiceAccountToken: true
{{- end }}
"#
    .to_string()
}

pub(super) fn whitelabeled_log_collector_role_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
{{- end }}
"#
    .to_string()
}

pub(super) fn whitelabeled_log_collector_rolebinding_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
    namespace: {{ .Release.Namespace }}
{{- end }}
"#
    .to_string()
}

pub(super) fn whitelabeled_log_collector_configmap_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
data:
  collector.conf: |
    [SERVICE]
        Flush        2
        Log_Level    info
        Parsers_File parsers.conf
        storage.path /buffers
        storage.sync normal
        storage.backlog.mem_limit 64M

    [INPUT]
        Name              tail
        Path              /var/log/pods/{{ .Release.Namespace }}_*/*/*.log
        Exclude_Path      /var/log/pods/{{ .Release.Namespace }}_{{ include "deployment.fullname" . }}-*/*/*.log
        Path_Key          filename
        multiline.parser  docker, cri
        Tag               kube.*
        DB                /buffers/{{ include "deployment.fullname" . }}-whitelabeled-log-collector.db
        Mem_Buf_Limit     64MB
        Skip_Long_Lines   On
        Read_from_Head    On
        Refresh_Interval  5
        storage.type      filesystem

    [FILTER]
        Name                kubernetes
        Match               kube.*
        Merge_Log           Off
        Keep_Log            On
        Labels              On
        Annotations         Off

    {{- if and .Values.logCollector.scope.deploymentLabelKey .Values.logCollector.scope.deploymentLabelValue }}
    [FILTER]
        Name                grep
        Match               kube.*
        Regex               $kubernetes['labels']['{{ .Values.logCollector.scope.deploymentLabelKey }}'] ^{{ .Values.logCollector.scope.deploymentLabelValue }}$
    {{- end }}

    [OUTPUT]
        Name          http
        Match         kube.*
        Host          {{ include "deployment.fullname" . }}.{{ .Release.Namespace }}.svc.cluster.local
        Port          {{ .Values.runtime.api.port }}
        URI           /internal/logs
        Format        json
        Json_Date_Key observed_at
        Header        Authorization Bearer ${COLLECTOR_TOKEN}

  parsers.conf: |
    [PARSER]
        Name        cri
        Format      regex
        Regex       ^(?<time>[^ ]+) (?<stream>stdout|stderr) (?<logtag>[^ ]*) (?<log>.*)$
        Time_Key    time
        Time_Format %Y-%m-%dT%H:%M:%S.%L%z
{{- end }}
"#
    .to_string()
}

pub(super) fn whitelabeled_log_collector_daemonset_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
spec:
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
      app.kubernetes.io/component: whitelabeled-log-collector
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
        app.kubernetes.io/component: whitelabeled-log-collector
    spec:
      serviceAccountName: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
      tolerations:
        - operator: Exists
      containers:
        - name: collector
          image: "{{ .Values.logCollector.image.repository }}:{{ .Values.logCollector.image.tag }}"
          imagePullPolicy: {{ .Values.logCollector.image.pullPolicy }}
          args:
            - -c
            - /collector/etc/collector.conf
          env:
            - name: COLLECTOR_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "deployment.fullname" . }}
                  key: collector-token
          volumeMounts:
            - name: config
              mountPath: /collector/etc
              readOnly: true
            - name: varlog
              mountPath: /var/log
              readOnly: true
            - name: dockercontainers
              mountPath: /var/lib/docker/containers
              readOnly: true
            - name: buffers
              mountPath: /buffers
          resources:
            {{- toYaml .Values.logCollector.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
        - name: varlog
          hostPath:
            path: /var/log
            type: Directory
        # Docker-runtime clusters symlink pod logs to /var/lib/docker/containers;
        # mount it so fluent-bit can follow them. DirectoryOrCreate is harmless on
        # containerd nodes where the path doesn't exist.
        - name: dockercontainers
          hostPath:
            path: /var/lib/docker/containers
            type: DirectoryOrCreate
        - name: buffers
          emptyDir: {}
{{- end }}
"#
    .to_string()
}

pub(super) fn pvc_tpl() -> String {
    r#"{{- if and .Values.runtime.data.persistence.enabled (not .Values.runtime.data.persistence.existingClaim) }}
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ printf "%s-runtime-data" (include "deployment.fullname" .) }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  accessModes:
    {{- toYaml .Values.runtime.data.persistence.accessModes | nindent 4 }}
  {{- if .Values.runtime.data.persistence.storageClassName }}
  storageClassName: {{ .Values.runtime.data.persistence.storageClassName | quote }}
  {{- end }}
  resources:
    requests:
      storage: {{ .Values.runtime.data.persistence.size | quote }}
{{- end }}
"#
    .to_string()
}

pub(super) fn poddisruptionbudget_tpl() -> String {
    r#"{{- if .Values.runtime.pdb.enabled }}
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  {{- if hasKey .Values.runtime.pdb "maxUnavailable" }}
  maxUnavailable: {{ .Values.runtime.pdb.maxUnavailable }}
  {{- else }}
  minAvailable: {{ .Values.runtime.pdb.minAvailable }}
  {{- end }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}
"#
    .to_string()
}

pub(super) fn networkpolicy_tpl() -> String {
    r#"{{- if .Values.runtime.networkPolicy.enabled }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  policyTypes:
    {{- if .Values.runtime.networkPolicy.ingress.enabled }}
    - Ingress
    {{- end }}
    {{- if .Values.runtime.networkPolicy.egress.enabled }}
    - Egress
    {{- end }}
  {{- if .Values.runtime.networkPolicy.ingress.enabled }}
  ingress:
    - {}
  {{- end }}
  {{- if .Values.runtime.networkPolicy.egress.enabled }}
  egress:
    - {}
  {{- end }}
{{- end }}
"#
    .to_string()
}
mod workload;
pub(super) use workload::*;
