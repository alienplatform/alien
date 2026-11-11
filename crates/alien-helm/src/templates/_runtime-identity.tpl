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
  {{- range $container := $installed.spec.template.spec.containers -}}
    {{- if eq $container.name "operator" -}}
      {{- range $mount := $container.volumeMounts -}}
        {{- if and (eq $mount.name "encryption-key") (ne $mount.subPath (include "deployment.encryptionSecretKey" $)) -}}
          {{- fail "Changing the runtime encryption Secret key requires a state migration. Preserve the installed key selector when upgrading." -}}
        {{- end -}}
      {{- end -}}
    {{- end -}}
  {{- end -}}
{{- end -}}

{{- end -}}

{{- define "deployment.managementUrl" -}}
{{- $endpoint := .Values.management.defaultUrl -}}
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
{{- $url := default $endpoint .Values.management.url -}}
{{- if and $url (not (hasPrefix "https://" (lower $url))) (not .Values.management.allowInsecure) -}}
  {{- fail (printf "The management URL %q isn't https. The Operator sends its deployment token to it. Switch to https: set management.url (or management.defaultUrl) to the manager's https:// address. For a local or test manager only, set management.allowInsecure=true instead." $url) -}}
{{- end -}}
{{- $url -}}
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

{{- define "deployment.validateGitops" -}}
{{- if dig "enabled" false (default dict .Values.gitops) -}}
  {{- if not .Values.management.existingSecret.name -}}
    {{- fail "gitops.enabled requires management.existingSecret.name: a GitOps tool renders this chart on every sync, so the management token must come from a Secret you manage, not from values." -}}
  {{- end -}}
  {{- if not .Values.runtime.encryption.existingSecret.name -}}
    {{- fail "gitops.enabled requires runtime.encryption.existingSecret.name: without cluster access every render would generate a different encryption key and make the installed state unreadable." -}}
  {{- end -}}
  {{- if and .Values.logCollector.enabled (eq (default "nodeAgent" .Values.logCollector.mode) "nodeAgent") (not .Values.logCollector.token) (not (dig "enabled" false (default dict .Values.remoteOperator))) -}}
    {{- fail "gitops.enabled with logCollector.mode=nodeAgent requires logCollector.token: a generated collector token would change on every render. Use logCollector.mode=podApi, which needs no token." -}}
  {{- end -}}
{{- end -}}
{{- end -}}
