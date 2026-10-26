//! Setup-owned PostgreSQL databases and their workload bindings.

use alien_core::{Postgres, ResourceLifecycle, Stack};
use serde_json::{json, Map, Value};

pub(crate) fn values(stack: &Stack) -> Value {
    let databases: Map<String, Value> = stack.resources().filter_map(|(id, entry)| {
        let database = entry.config.downcast_ref::<Postgres>()?;
        if entry.lifecycle != ResourceLifecycle::Frozen {
            return None;
        }
        Some((id.clone(), json!({
            "image": {"repository": "pgvector/pgvector", "tag": format!("pg{}-bookworm", database.version)},
            "storage": kubernetes_size(&database.storage, false),
            "storageClassName": "",
            "highAvailability": database.high_availability,
            "encryptionKey": database.encryption_key,
            "resources": {"requests": {
                "cpu": database.cpu.as_deref().unwrap_or("250m"),
                "memory": kubernetes_size(database.memory.as_deref().unwrap_or("256Mi"), true)
            }}
        })))
    }).collect();
    Value::Object(databases)
}

// Postgres accepts GiB/GB aliases and treats unitless memory as GiB.
// Kubernetes uses Gi/G and interprets unitless quantities as bytes.
fn kubernetes_size(value: &str, memory: bool) -> String {
    let value = value.trim();
    if memory {
        if let Ok(gib) = value.parse::<f64>() {
            return format!("{gib}Gi");
        }
    }
    value.strip_suffix('B').unwrap_or(value).to_string()
}

pub(crate) const HELPERS: &str = r#"
{{- define "deployment.postgresName" -}}
{{- printf "%s-pg-%s" (include "deployment.fullname" .root | trunc 28 | trimSuffix "-") (.id | trunc 20 | trimSuffix "-") -}}-{{ printf "%s/%s/%s" .root.Release.Namespace .root.Release.Name .id | sha256sum | trunc 8 }}
{{- end -}}

{{- define "deployment.infrastructure" -}}
{{- $bindings := deepCopy (default dict .Values.infrastructure) -}}
{{- if not .Values.infrastructureExistingSecret -}}
{{- range $id, $database := .Values.postgres -}}
  {{- if not (hasKey $bindings $id) -}}
    {{- $name := include "deployment.postgresName" (dict "root" $ "id" $id) -}}
    {{- $_ := set $bindings $id (dict "type" "postgres" "service" "external" "host" (printf "%s.%s.svc" $name $.Release.Namespace) "port" 5432 "database" $id "username" "postgres" "password" (dict "secretKeyRef" (dict "name" $name "key" "password")) "sslMode" "disable") -}}
  {{- end -}}
{{- end -}}
{{- end -}}
{{- $bindings | toJson -}}
{{- end -}}
"#;

pub(crate) const TEMPLATE: &str = r#"{{- range $id, $database := .Values.postgres }}
{{- if and (not $.Values.infrastructureExistingSecret) (not (hasKey (default dict $.Values.infrastructure) $id)) }}
{{- if $database.highAvailability }}{{ fail "In-cluster Postgres is single-instance; supply an external binding for high availability" }}{{ end }}
{{- if $database.encryptionKey }}{{ fail "In-cluster Postgres uses StorageClass encryption; supply an external binding for an explicit database encryption key" }}{{ end }}
{{- $name := include "deployment.postgresName" (dict "root" $ "id" $id) }}
{{- $labels := include "deployment.labels" $ | fromYaml }}
{{- $_ := set $labels "app.kubernetes.io/name" $name }}
{{- $installed := lookup "apps/v1" "StatefulSet" $.Release.Namespace $name }}
{{- $volume := lookup "v1" "PersistentVolumeClaim" $.Release.Namespace $name }}
{{- $desiredImage := printf "%s:%s" $database.image.repository $database.image.tag }}
{{- $previousImage := "" }}
{{- if $volume }}
  {{- $previousImage = get (default dict $volume.metadata.annotations) "alien.dev/postgres-image" | default "" }}
{{- end }}
{{- if and (empty $previousImage) $installed }}
  {{- $previousImage = (index $installed.spec.template.spec.containers 0).image }}
{{- end }}
{{- if and $volume (empty $previousImage) }}
  {{- fail "Postgres data has no recorded image; restore the original alien.dev/postgres-image PVC annotation before installing" }}
{{- end }}
{{- if and $previousImage (ne (first (splitList "@" $previousImage)) (first (splitList "@" $desiredImage))) }}
  {{- fail "Postgres image changes require an explicit database upgrade; retain the installed image or migrate to an external binding" }}
{{- end }}
{{- $existing := lookup "v1" "Secret" $.Release.Namespace $name }}
{{- $password := "" }}
{{- if $existing }}
  {{- $password = required "Existing Postgres Secret has no password; restore its original password" (index $existing.data "password") }}
{{- else }}
  {{- if lookup "v1" "PersistentVolumeClaim" $.Release.Namespace $name }}
    {{- fail "Postgres data exists but its credentials Secret is missing; restore the original Secret before installing" }}
  {{- end }}
  {{- $password = randAlphaNum 48 | b64enc }}
{{- end }}
---
apiVersion: v1
kind: Secret
metadata:
  name: {{ $name }}
  annotations:
    helm.sh/resource-policy: keep
  labels:
    {{- $labels | toYaml | nindent 4 }}
type: Opaque
data:
  password: {{ $password | quote }}
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ $name }}
  annotations:
    helm.sh/resource-policy: keep
    alien.dev/postgres-image: {{ $desiredImage | quote }}
  labels:
    {{- $labels | toYaml | nindent 4 }}
spec:
  accessModes: [ReadWriteOnce]
  {{- if $database.storageClassName }}
  storageClassName: {{ $database.storageClassName | quote }}
  {{- end }}
  resources:
    requests:
      storage: {{ $database.storage | quote }}
---
apiVersion: v1
kind: Service
metadata:
  name: {{ $name }}
  labels:
    {{- $labels | toYaml | nindent 4 }}
spec:
  selector:
    app.kubernetes.io/component: {{ $name }}
  ports:
    - name: postgres
      port: 5432
      targetPort: postgres
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: {{ $name }}
  labels:
    {{- $labels | toYaml | nindent 4 }}
spec:
  serviceName: {{ $name }}
  replicas: 1
  selector:
    matchLabels:
      app.kubernetes.io/component: {{ $name }}
  template:
    metadata:
      labels:
        app.kubernetes.io/component: {{ $name }}
        {{- $labels | toYaml | nindent 8 }}
    spec:
      automountServiceAccountToken: false
      {{- with $.Values.runtime.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      containers:
        - name: postgres
          image: {{ printf "%s:%s" $database.image.repository $database.image.tag | quote }}
          ports:
            - name: postgres
              containerPort: 5432
          env:
            - name: POSTGRES_DB
              value: {{ $id | quote }}
            - name: POSTGRES_USER
              value: postgres
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ $name }}
                  key: password
            - name: PGDATA
              value: /var/lib/postgresql/data/pgdata
          readinessProbe:
            exec:
              command: [pg_isready, -U, postgres, -d, {{ $id | quote }}]
            periodSeconds: 5
          resources:
            {{- toYaml $database.resources | nindent 12 }}
          volumeMounts:
            - name: data
              mountPath: /var/lib/postgresql/data
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: {{ $name }}
{{- end }}
{{- end }}
"#;

pub(crate) const README: &str = r#"
## PostgreSQL

Frozen Postgres resources run as single-instance StatefulSets with pgvector and a persistent volume. Helm owns these databases; application releases do not replace them. The cluster needs a default StorageClass, or set `postgres.<resource>.storageClassName` before installing. Storage encryption comes from that StorageClass. For high availability or a separately managed encryption key, use your own database.

The password is generated once in a Kubernetes Secret and supplied to linked workloads through `secretKeyRef`. Upgrades reuse that Secret. Image changes require a database upgrade plan; the chart records the image on the retained PVC and refuses to change it automatically, including after uninstall. For older volumes without this annotation, upgrade the chart while the original StatefulSet still exists; if it has already been removed, restore the `alien.dev/postgres-image` PVC annotation from the original installation before reinstalling.

To use your own PostgreSQL, create its password Secret in the installation namespace first, then set an external binding:

```yaml
infrastructure:
  database:
    type: postgres
    service: external
    host: postgres.example.com
    port: 5432
    database: app
    username: app
    password:
      secretRef:
        name: existing-database-credentials
        key: password
    sslMode: verify-full
```

Replace `database` with the resource ID in your stack. External bindings are passed through unchanged and create no database or volume. The referenced Secret must be available when workloads start. Use `sslMode: disable` only when plaintext connections are intended on your cluster's private network.

Uninstall removes the database StatefulSet and Service but retains its PVC and credentials Secret together. Back up the data before deleting either retained object. Reinstalling the same release reuses them; deleting only the credentials Secret does not reset the database password. Removing the namespace deletes the retained data too.
"#;
