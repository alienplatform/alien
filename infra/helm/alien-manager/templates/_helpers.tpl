{{- define "alien-manager.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "alien-manager.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "alien-manager.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{ include "alien-manager.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "alien-manager.selectorLabels" -}}
app.kubernetes.io/name: {{ include "alien-manager.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "alien-manager.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "alien-manager.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "alien-manager.adminTokenSecret" -}}
{{- default (include "alien-manager.fullname" .) .Values.adminToken.existingSecret.name -}}
{{- end -}}

{{- define "alien-manager.image" -}}
{{- printf "%s:%s" .Values.image.repository (default (printf "v%s" .Chart.AppVersion) .Values.image.tag) -}}
{{- end -}}
