{{- define "deployment.validateRuntimeIdentity" -}}
{{- $storage := .Values.runtime.data.persistence -}}
{{- $credentialConfigured := or .Values.management.token (include "deployment.credentialSecretName" .) -}}
{{- if and $credentialConfigured (not .Values.management.deploymentId) (not $storage.enabled) -}}
  {{- fail "A bootstrap installation requires persistent runtime storage. Set runtime.data.persistence.enabled=true; ephemeral storage requires an existing deployment ID and its deployment credential." -}}
{{- end -}}
{{- if and (or .Values.management.enrollmentCode (include "deployment.enrollmentCodeSecretName" .)) (not $storage.enabled) -}}
  {{- fail "Enrollment keeps the Operator's daily key in runtime storage. Set runtime.data.persistence.enabled=true." -}}
{{- end -}}
{{- $name := include "deployment.fullname" . -}}
{{- if and .Values.management.deploymentId (not .Values.management.enrollmentCode) (not .Values.management.token) (not (include "deployment.credentialSecretName" .)) (not (dig "enabled" false (default dict .Values.airgapped))) -}}
  {{- /* An enrolled Operator runs on the key stored in its volume, so an
  upgrade needs no code. A new installation with no credential would crash-loop. */ -}}
  {{- $installedSecret := lookup "v1" "Secret" .Release.Namespace $name -}}
  {{- $installedClaim := lookup "v1" "PersistentVolumeClaim" .Release.Namespace (default (printf "%s-runtime-data" $name) $storage.existingClaim) -}}
  {{- if not (or .Release.IsUpgrade $installedSecret $installedClaim) -}}
    {{- fail "This is a new installation, and no enrollment code or token was given. Check the kube context and namespace, or get a new enrollment code." -}}
  {{- end -}}
{{- end -}}
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
{{- default $endpoint .Values.management.url -}}
{{- end -}}


{{- /* The Secret the Operator creates and owns. The chart never renders it. It holds
the values the install would otherwise have to generate at render time. */ -}}
{{- define "deployment.operatorSecretName" -}}
{{- printf "%s-operator" (include "deployment.fullname" . | trunc 54 | trimSuffix "-") -}}
{{- end -}}

{{- /* The pull credential the Operator keeps refreshed. The chart does not render it on a new
install; on upgrade it keeps rendering the Secret an earlier chart made, unchanged. */ -}}
{{- define "deployment.registrySecretName" -}}
{{- printf "%s-registry" (include "deployment.fullname" .) -}}
{{- end -}}

{{- /* Pull credential for the first Pod, rendered from the code or token in the values. */ -}}
{{- define "deployment.bootstrapPullSecretName" -}}
{{- printf "%s-bootstrap-pull" (include "deployment.fullname" . | trunc 49 | trimSuffix "-") -}}
{{- end -}}

{{- /* Values carried over from an earlier chart on upgrade. Rendered only while
that migration is needed. */ -}}
{{- define "deployment.migrationSecretName" -}}
{{- printf "%s-migration" (include "deployment.fullname" . | trunc 53 | trimSuffix "-") -}}
{{- end -}}

{{- define "deployment.enrollmentCodeSecretName" -}}
{{- dig "name" "" (default dict .Values.management.enrollmentCodeSecret) -}}
{{- end -}}

{{- /* Migration only (remove one release after migration is reported): an earlier chart took the
management token from `management.existingSecret`, which old GitOps installs still keep in Git.
The Secret is still read as the token source. */ -}}
{{- define "deployment.credentialSecretName" -}}
{{- default (dig "name" "" (default dict .Values.management.existingSecret)) (include "deployment.enrollmentCodeSecretName" .) -}}
{{- end -}}

{{- define "deployment.managementSecretName" -}}
{{- default (include "deployment.fullname" .) (include "deployment.credentialSecretName" .) -}}
{{- end -}}

{{- define "deployment.managementSecretTokenKey" -}}
{{- if include "deployment.enrollmentCodeSecretName" . -}}
{{- default "enrollment-code" (dig "key" "" (default dict .Values.management.enrollmentCodeSecret)) -}}
{{- else if .Values.management.enrollmentCode -}}
enrollment-code
{{- else if dig "name" "" (default dict .Values.management.existingSecret) -}}
{{- default "sync-token" (dig "tokenKey" "" .Values.management.existingSecret) -}}
{{- else -}}
sync-token
{{- end -}}
{{- end -}}

{{- define "deployment.managementCredentialFile" -}}
{{- ternary "enrollment-code" "sync-token" (not (empty (or .Values.management.enrollmentCode (include "deployment.enrollmentCodeSecretName" .)))) -}}
{{- end -}}

{{- /* The in-cluster Postgres databases whose password the Operator owns: JSON list of {id, name}. */ -}}
{{- define "deployment.ownedPostgres" -}}
{{- $owned := list -}}
{{- if not .Values.infrastructureExistingSecret -}}
{{- range $id, $database := .Values.postgres -}}
  {{- if and $database.enabled (not $database.existingPasswordSecret) (not (hasKey (default dict $.Values.infrastructure) $id)) (not (hasKey (default dict (get (default dict $.Values.stackSettings) "externalBindings")) $id)) -}}
    {{- $owned = append $owned (dict "id" $id "name" (include "deployment.postgresName" (dict "root" $ "id" $id))) -}}
  {{- end -}}
{{- end -}}
{{- end -}}
{{- toJson $owned -}}
{{- end -}}

{{- define "deployment.nodeCollector" -}}
{{- if and .Values.logCollector.enabled (eq (default "nodeAgent" .Values.logCollector.mode) "nodeAgent") (not (dig "enabled" false (default dict .Values.remoteOperator))) -}}true{{- end -}}
{{- end -}}

{{- /* The keys the Operator keeps in its Secret, as `key=kind` pairs. */ -}}
{{- define "deployment.operatorSecretKeys" -}}
{{- $keys := list "encryption-key=hex32" -}}
{{- if include "deployment.nodeCollector" . -}}
  {{- $keys = append $keys "collector-token=token48" -}}
{{- end -}}
{{- range $database := include "deployment.ownedPostgres" . | fromJsonArray -}}
  {{- $keys = append $keys (printf "postgres-%s-password=token48" $database.id) -}}
{{- end -}}
{{- join "," $keys -}}
{{- end -}}
{{- /* The Operator image tag to run. This chart installs the Operator version in
.Chart.AppVersion. A semver tag older than that can only be a stale default carried by
helm upgrade --reuse-values from an earlier chart, and an older Operator cannot run this chart,
so it is replaced by the chart's own version (keeping the tag's "v" prefix style). A newer
semver tag and any non-semver tag are kept as set. */ -}}
{{- define "deployment.operatorTag" -}}
{{- $tag := toString .Values.runtime.image.tag -}}
{{- $bare := (split "@" $tag)._0 | trimPrefix "v" -}}
{{- $appVersion := .Chart.AppVersion | trimPrefix "v" -}}
{{- $semver := "^[0-9]+\\.[0-9]+\\.[0-9]+$" -}}
{{- if and (regexMatch $semver $bare) (regexMatch $semver $appVersion) (semverCompare (printf "< %s" $appVersion) $bare) -}}
{{- if hasPrefix "v" $tag }}v{{ end }}{{ $appVersion }}
{{- else -}}
{{- $tag -}}
{{- end -}}
{{- end -}}
