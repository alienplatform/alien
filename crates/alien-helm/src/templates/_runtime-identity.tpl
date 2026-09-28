{{- define "deployment.validateRuntimeIdentity" -}}
{{- $storage := .Values.runtime.data.persistence -}}
{{- $credentialConfigured := or .Values.management.token .Values.management.existingSecret.name -}}
{{- if and $credentialConfigured (not .Values.management.deploymentId) (not $storage.enabled) -}}
  {{- fail "A bootstrap installation requires persistent runtime storage. Set runtime.data.persistence.enabled=true; ephemeral storage requires an existing deployment ID and its deployment credential." -}}
{{- end -}}
{{- $name := include "deployment.fullname" . -}}
{{- $installed := lookup "apps/v1" "Deployment" .Release.Namespace $name -}}
{{- if $installed -}}
  {{- $claim := default (printf "%s-runtime-data" $name) $storage.existingClaim -}}
  {{- range $volume := $installed.spec.template.spec.volumes -}}
    {{- if eq $volume.name "runtime-data" -}}
      {{- $previousClaim := dig "persistentVolumeClaim" "claimName" "" $volume -}}
      {{- if or (and $previousClaim (not $storage.enabled)) (and $storage.enabled (ne $claim $previousClaim)) -}}
        {{- fail "Changing the runtime identity volume would discard the installed deployment state. Preserve the current claim, or migrate the existing state before upgrading." -}}
      {{- end -}}
    {{- end -}}
    {{- if and (eq $volume.name "encryption-key") (ne $volume.secret.secretName (include "deployment.encryptionSecretName" $)) -}}
      {{- fail "Changing the runtime encryption Secret requires a state migration. Preserve the installed Secret reference when upgrading." -}}
    {{- end -}}
  {{- end -}}
{{- end -}}

{{- end -}}

{{- define "deployment.managementUrl" -}}
{{- $endpoint := .Values.management.url -}}
{{- $installed := lookup "apps/v1" "Deployment" .Release.Namespace (include "deployment.fullname" .) -}}
{{- if $installed -}}
  {{- range $container := $installed.spec.template.spec.containers -}}
    {{- if eq $container.name "operator" -}}
      {{- range $variable := $container.env -}}
        {{- if and (eq $variable.name "SYNC_URL") $variable.value -}}
          {{- $endpoint = $variable.value -}}
        {{- end -}}
      {{- end -}}
    {{- end -}}
  {{- end -}}
{{- end -}}
{{- $endpoint -}}
{{- end -}}

{{- define "deployment.runtimeEncryptionKey" -}}
{{- if not (hasKey . "runtimeEncryptionKey") -}}
  {{- $name := include "deployment.fullname" . -}}
  {{- $installed := lookup "v1" "Secret" .Release.Namespace $name -}}
  {{- $stored := "" -}}
  {{- if and $installed (hasKey (default dict $installed.data) "encryption-key") -}}
    {{- $stored = index $installed.data "encryption-key" | b64dec -}}
    {{- if not (regexMatch "^[a-fA-F0-9]{64}$" $stored) -}}
      {{- fail "The installed runtime encryption key is invalid. Restore the original key before upgrading." -}}
    {{- end -}}
  {{- end -}}
  {{- $requested := .Values.runtime.encryption.key -}}
  {{- if and $requested (not (regexMatch "^[a-fA-F0-9]{64}$" $requested)) -}}
    {{- fail "runtime.encryption.key must be exactly 64 hex characters, or empty to generate it on first installation." -}}
  {{- end -}}
  {{- if and $stored $requested (ne $stored $requested) -}}
    {{- fail "Changing the runtime encryption key would make the installed state unreadable. Keep the installed key." -}}
  {{- end -}}
  {{- $key := default $requested $stored -}}
  {{- if not $key -}}
    {{- $claim := default (printf "%s-runtime-data" $name) .Values.runtime.data.persistence.existingClaim -}}
    {{- $identity := lookup "v1" "PersistentVolumeClaim" .Release.Namespace $claim -}}
    {{- if or .Release.IsUpgrade $installed $identity -}}
      {{- fail "Cannot generate a replacement encryption key for an existing installation. Restore its original Secret or provide the original runtime.encryption.key." -}}
    {{- end -}}
    {{- $key = randBytes 32 | sha256sum -}}
  {{- end -}}
  {{- $_ := set . "runtimeEncryptionKey" $key -}}
{{- end -}}
{{- .runtimeEncryptionKey -}}
{{- end -}}
