//! A generic Operator chart: one shared image, no per-project values.
//!
//! `helm install <release> <chart> --set token=<enrollment token>` is all an
//! installer runs. The Operator keeps its identity in the `<release>-identity`
//! Secret (created empty, so upgrade, rollback and apply never touch it) and
//! its working data in an `emptyDir`, so the chart has no volume claim, no
//! hooks and needs no StorageClass.
//!
//! The chart is separate from the per-project chart generator on purpose: it
//! shares none of its identity hooks or its volume.

use std::collections::{BTreeMap, BTreeSet};

use alien_core::access_request_crd::{access_request_crd_names, AccessRequestCrdNames};
use alien_operations_sdk::KubernetesOperationPermissions;
use alien_permissions::operations::kubernetes::{grants_verb, operator_runtime_rules};
use alien_permissions::operations::KubernetesMode;
use indexmap::IndexMap;

use crate::generator::{access_request_crd_doc, HelmChart};

/// Port the OTLP intake Service exposes.
pub const OPERATOR_CHART_OTLP_PORT: u16 = 4318;

/// Namespaces the chart leaves out of log collection unless told otherwise.
pub const DEFAULT_EXCLUDED_NAMESPACES: [&str; 3] =
    ["kube-system", "kube-public", "kube-node-lease"];

/// What the generic chart is rendered from.
pub struct OperatorChartOptions<'a> {
    /// Chart `version`.
    pub chart_version: &'a str,
    /// Chart `appVersion`, shown by `helm list`.
    pub app_version: &'a str,
    /// Default `image.repository`, without a tag or digest.
    pub image_repository: &'a str,
    /// Default `image.digest`, as `sha256:<64 hex>`.
    pub image_digest: &'a str,
    /// The built-in operations the Operator can run. Their Kubernetes
    /// requirements become RBAC at the chosen `permission` tier.
    pub operations: &'a [KubernetesOperationPermissions],
    /// Brand for the approval custom resource; `None` is the default brand.
    pub label_domain: Option<&'a str>,
}

/// Render the generic Operator chart.
pub fn generate_operator_chart(options: OperatorChartOptions<'_>) -> Result<HelmChart, String> {
    validate_digest(options.image_digest)?;
    if options.image_repository.is_empty() || options.image_repository.contains(['@', ' ']) {
        return Err("the image repository must be a name without a tag or digest".into());
    }
    let crd_names = access_request_crd_names(options.label_domain);
    let mut files = IndexMap::new();
    files.insert("Chart.yaml".to_string(), chart_yaml(&options));
    files.insert("values.yaml".to_string(), values_yaml(&options));
    files.insert(
        "values.schema.json".to_string(),
        values_schema_json().map_err(|error| error.to_string())?,
    );
    files.insert("templates/_helpers.tpl".to_string(), HELPERS.to_string());
    files.insert(
        "templates/serviceaccount.yaml".to_string(),
        SERVICE_ACCOUNT.to_string(),
    );
    files.insert(
        "templates/clusterrole.yaml".to_string(),
        cluster_role_template(options.operations, &crd_names),
    );
    files.insert(
        "templates/clusterrolebinding.yaml".to_string(),
        CLUSTER_ROLE_BINDING.to_string(),
    );
    files.insert("templates/role.yaml".to_string(), IDENTITY_ROLE.to_string());
    files.insert(
        "templates/rolebinding.yaml".to_string(),
        IDENTITY_ROLE_BINDING.to_string(),
    );
    files.insert(
        "templates/enrollment-secret.yaml".to_string(),
        ENROLLMENT_SECRET.to_string(),
    );
    files.insert(
        "templates/identity-secret.yaml".to_string(),
        IDENTITY_SECRET.to_string(),
    );
    files.insert(
        "templates/deployment.yaml".to_string(),
        DEPLOYMENT.to_string(),
    );
    files.insert("templates/service.yaml".to_string(), SERVICE.to_string());
    files.insert(
        "templates/access-request-crd.yaml".to_string(),
        access_request_crd_template(&crd_names),
    );
    files.insert("templates/NOTES.txt".to_string(), NOTES.to_string());
    Ok(HelmChart {
        name: "operator".to_string(),
        files,
    })
}

fn validate_digest(digest: &str) -> Result<(), String> {
    match digest.strip_prefix("sha256:") {
        Some(hex)
            if hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
        {
            Ok(())
        }
        _ => Err("the image digest must be sha256: followed by 64 lowercase hex characters".into()),
    }
}

fn chart_yaml(options: &OperatorChartOptions<'_>) -> String {
    format!(
        "apiVersion: v2\nname: operator\ndescription: Connects a Kubernetes cluster to its control plane\ntype: application\nversion: {}\nappVersion: {}\n",
        yaml_string(options.chart_version),
        yaml_string(options.app_version)
    )
}

fn values_yaml(options: &OperatorChartOptions<'_>) -> String {
    let excluded = DEFAULT_EXCLUDED_NAMESPACES
        .iter()
        .map(|namespace| format!("    - {namespace}\n"))
        .collect::<String>();
    format!(
        r#"# Enrollment token from the control plane. It is used once, within an hour.
# Required on install unless existingSecret is set; not needed on upgrade.
token: ""
# Name of a Secret in this namespace with the enrollment token under the key `token`.
existingSecret: ""

image:
  repository: {repository}
  # Pinned by digest; whoever installs this chart owns the image version.
  digest: {digest}

# diagnostics: read-only. remediation: also lets approved operations change workloads.
permission: diagnostics

logs:
  enabled: true
  # Read only these namespaces; empty means all except excludeNamespaces.
  namespaces: []
  excludeNamespaces:
{excluded}  # Kubernetes label selector Pods must match, for example "app=web".
  podSelector: ""
  maxStreams: 200

otlp:
  # A ClusterIP Service on port {port} that applications send OTLP to.
  enabled: true

accessRequests:
  # The in-cluster approval flow (a custom resource). Off by default.
  enabled: false

# Operation approval settings passed to the Operator, for example
# {{"kubernetes": {{"approval": {{"get-pods": "auto"}}}}}}.
operations: {{}}

resources:
  requests:
    cpu: 50m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi
"#,
        repository = yaml_string(options.image_repository),
        digest = yaml_string(options.image_digest),
        excluded = excluded,
        port = OPERATOR_CHART_OTLP_PORT,
    )
}

fn values_schema_json() -> serde_json::Result<String> {
    let schema = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "token": {"type": "string", "pattern": "^$|^ax_join_v1_[A-Za-z0-9_-]+$"},
            "existingSecret": {"type": "string"},
            "image": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "repository": {"type": "string", "minLength": 1},
                    "digest": {"type": "string", "pattern": "^sha256:[a-f0-9]{64}$"}
                }
            },
            "permission": {"type": "string", "enum": ["diagnostics", "remediation"]},
            "logs": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "enabled": {"type": "boolean"},
                    "namespaces": {"type": "array", "items": {"type": "string"}},
                    "excludeNamespaces": {"type": "array", "items": {"type": "string"}},
                    "podSelector": {"type": "string"},
                    "maxStreams": {"type": "integer", "minimum": 1, "maximum": 256}
                }
            },
            "otlp": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"enabled": {"type": "boolean"}}
            },
            "accessRequests": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"enabled": {"type": "boolean"}}
            },
            "operations": {"type": "object"},
            "resources": {"type": "object"}
        }
    });
    serde_json::to_string_pretty(&schema).map(|mut text| {
        text.push('\n');
        text
    })
}

/// One normalized rule: what the Operator needs, and why.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RuleScope {
    api_group: String,
    resource: String,
    resource_names: BTreeSet<String>,
}

#[derive(Default)]
struct Grant {
    /// Verbs a read-only (`diagnostics`) install gets.
    read_verbs: BTreeSet<String>,
    /// Verbs only a `remediation` install gets.
    write_verbs: BTreeSet<String>,
    reasons: BTreeSet<String>,
}

fn cluster_role_template(
    operations: &[KubernetesOperationPermissions],
    crd_names: &AccessRequestCrdNames,
) -> String {
    let mut rules = BTreeMap::<RuleScope, Grant>::new();
    let mut add = |group: &str,
                   resource: &str,
                   names: BTreeSet<String>,
                   verbs: Vec<String>,
                   reason: String,
                   fixed: bool| {
        let grant = rules
            .entry(RuleScope {
                api_group: group.to_owned(),
                resource: resource.to_owned(),
                resource_names: names,
            })
            .or_default();
        for verb in verbs {
            if fixed || grants_verb(KubernetesMode::Diagnostics, &verb) {
                grant.read_verbs.insert(verb);
            } else {
                grant.write_verbs.insert(verb);
            }
        }
        grant.reasons.insert(reason);
    };
    for rule in operator_runtime_rules()
        .into_iter()
        .filter(|rule| rule.feature.is_none())
    {
        add(
            &rule.api_group,
            &rule.resource,
            BTreeSet::new(),
            rule.verbs,
            format!("the Operator runtime: {}.", rule.reason),
            true,
        );
    }
    for operation in operations {
        for rule in &operation.permissions.rules {
            add(
                &rule.api_group,
                &rule.resource,
                rule.resource_names.iter().cloned().collect(),
                rule.verbs.clone(),
                format!(
                    "{}/{}: {}",
                    operation.plugin, operation.operation, rule.reason
                ),
                false,
            );
        }
    }

    let mut yaml = String::from(
        "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: {{ include \"operator.clusterRoleName\" . }}\n  labels:\n    {{- include \"operator.labels\" . | nindent 4 }}\nrules:\n",
    );
    for (scope, grant) in &rules {
        let comment: String = grant
            .reasons
            .iter()
            .map(|reason| format!("  # Required by {reason}\n"))
            .collect();
        if !grant.read_verbs.is_empty() {
            yaml.push_str(&comment);
            yaml.push_str(&rule_yaml(scope, &grant.read_verbs));
        }
        if !grant.write_verbs.is_empty() {
            yaml.push_str("  {{- if eq .Values.permission \"remediation\" }}\n");
            yaml.push_str(&comment);
            yaml.push_str(&rule_yaml(scope, &grant.write_verbs));
            yaml.push_str("  {{- end }}\n");
        }
    }
    // Reading Pod logs is the one cluster-wide `get` the Operator needs.
    yaml.push_str("  {{- if .Values.logs.enabled }}\n");
    yaml.push_str("  # Required by Pod log collection through the Kubernetes API.\n");
    yaml.push_str("  - apiGroups: [\"\"]\n    resources: [\"pods/log\"]\n    verbs: [\"get\"]\n");
    yaml.push_str("  {{- end }}\n");
    yaml.push_str("  {{- if .Values.accessRequests.enabled }}\n");
    yaml.push_str("  # Required by access-request materialization and approval observation.\n");
    yaml.push_str(&format!(
        "  - apiGroups: [{}]\n    resources: [{}]\n    verbs: [\"get\", \"list\", \"watch\", \"create\", \"update\", \"patch\"]\n",
        yaml_string(&crd_names.group),
        yaml_string(&crd_names.plural)
    ));
    yaml.push_str("  # Required by access-request status reporting.\n");
    yaml.push_str(&format!(
        "  - apiGroups: [{}]\n    resources: [{}]\n    verbs: [\"get\", \"update\", \"patch\"]\n",
        yaml_string(&crd_names.group),
        yaml_string(&format!("{}/status", crd_names.plural))
    ));
    yaml.push_str("  {{- end }}\n");
    yaml
}

fn rule_yaml(scope: &RuleScope, verbs: &BTreeSet<String>) -> String {
    let mut yaml = format!(
        "  - apiGroups: [{}]\n    resources: [{}]\n    verbs: [{}]\n",
        yaml_string(&scope.api_group),
        yaml_string(&scope.resource),
        verbs
            .iter()
            .map(|verb| yaml_string(verb))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !scope.resource_names.is_empty() {
        yaml.push_str(&format!(
            "    resourceNames: [{}]\n",
            scope
                .resource_names
                .iter()
                .map(|name| yaml_string(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    yaml
}

fn access_request_crd_template(names: &AccessRequestCrdNames) -> String {
    let crd = access_request_crd_doc(names);
    // The kind outlives the release: removing the chart must not delete every
    // access request an installer approved.
    let crd = crd.replacen(
        "metadata:\n",
        "metadata:\n  annotations:\n    helm.sh/resource-policy: keep\n",
        1,
    );
    format!("{{{{- if .Values.accessRequests.enabled }}}}\n{crd}{{{{- end }}}}\n")
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

const HELPERS: &str = r#"{{- define "operator.name" -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "operator.clusterRoleName" -}}
{{- printf "%s-%s" .Release.Name .Release.Namespace | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "operator.identitySecret" -}}
{{- printf "%s-identity" .Release.Name -}}
{{- end -}}

{{- define "operator.enrollmentSecret" -}}
{{- default (printf "%s-enrollment" .Release.Name) .Values.existingSecret -}}
{{- end -}}

{{- define "operator.selectorLabels" -}}
app.kubernetes.io/name: operator
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "operator.labels" -}}
{{ include "operator.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | quote }}
{{- end -}}
"#;

const SERVICE_ACCOUNT: &str = r#"apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "operator.name" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
"#;

const CLUSTER_ROLE_BINDING: &str = r#"apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "operator.clusterRoleName" . }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "operator.clusterRoleName" . }}
subjects:
  - kind: ServiceAccount
    name: {{ include "operator.name" . }}
    namespace: {{ .Release.Namespace }}
"#;

/// The only Secret the Operator can touch, by name, in the release namespace.
const IDENTITY_ROLE: &str = r#"apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "operator.name" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
rules:
  # Required by the Operator's saved identity and local key.
  - apiGroups: ['']
    resources: ['secrets']
    verbs: ['get', 'patch', 'update']
    resourceNames: [{{ include "operator.identitySecret" . | quote }}]
"#;

const IDENTITY_ROLE_BINDING: &str = r#"apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "operator.name" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "operator.name" . }}
subjects:
  - kind: ServiceAccount
    name: {{ include "operator.name" . }}
    namespace: {{ .Release.Namespace }}
"#;

/// Only when a token is given. A secrets controller that owns this Secret
/// never sees the identity, which lives in a different one.
const ENROLLMENT_SECRET: &str = r#"{{- if and .Values.token (not .Values.existingSecret) }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ printf "%s-enrollment" .Release.Name }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
type: Opaque
stringData:
  token: {{ .Values.token | quote }}
{{- end }}
"#;

/// Created with no data: the Operator writes its identity and local key here,
/// and a manifest without data never overwrites them on upgrade or rollback.
const IDENTITY_SECRET: &str = r#"apiVersion: v1
kind: Secret
metadata:
  name: {{ include "operator.identitySecret" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
type: Opaque
"#;

const DEPLOYMENT: &str = r#"{{- if and .Values.token .Values.existingSecret }}
{{- fail "Set either token or existingSecret, not both." }}
{{- end }}
{{- if and .Release.IsInstall (not .Values.token) (not .Values.existingSecret) }}
{{- fail "Install with --set token=<enrollment token> or --set existingSecret=<name of a Secret with the key `token`>." }}
{{- end }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "operator.name" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "operator.selectorLabels" . | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "operator.selectorLabels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "operator.name" . }}
      automountServiceAccountToken: true
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: operator
          image: {{ printf "%s@%s" .Values.image.repository .Values.image.digest | quote }}
          imagePullPolicy: IfNotPresent
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: ["ALL"]
          env:
            - name: PLATFORM
              value: "kubernetes"
            - name: KUBERNETES_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: OPERATOR_SCOPE
              value: {{ .Release.Namespace | quote }}
            - name: OPERATOR_OBSERVE_ALL_NAMESPACES
              value: "true"
            - name: OPERATOR_PERMISSION
              value: {{ .Values.permission | quote }}
            - name: OPERATOR_INITIAL_DESIRED_RELEASE
              value: "none"
            - name: OPERATOR_SETUP_METHOD
              value: "manual"
            - name: DATA_DIR
              value: "/var/lib/operator"
            - name: OPERATOR_DATA_PERSISTENT
              value: "false"
            - name: OPERATOR_IDENTITY_SECRET
              value: {{ include "operator.identitySecret" . | quote }}
            - name: OPERATOR_ENROLLMENT_TOKEN_FILE
              value: "/etc/alien/enrollment/token"
            - name: OPERATOR_ACCESS_REQUESTS
              value: {{ .Values.accessRequests.enabled | quote }}
            - name: SYNC_INTERVAL
              value: "30"
            - name: OPERATOR_READINESS_PORT
              value: "8081"
            {{- if .Values.logs.enabled }}
            - name: OPERATOR_POD_LOG_SCOPE
              value: "cluster"
            - name: OPERATOR_POD_LOG_MAX_STREAMS
              value: {{ .Values.logs.maxStreams | quote }}
            {{- with .Values.logs.namespaces }}
            - name: OPERATOR_POD_LOG_NAMESPACES
              value: {{ join "," . | quote }}
            {{- end }}
            {{- with .Values.logs.excludeNamespaces }}
            - name: OPERATOR_POD_LOG_EXCLUDE_NAMESPACES
              value: {{ join "," . | quote }}
            {{- end }}
            {{- with .Values.logs.podSelector }}
            - name: OPERATOR_POD_LOG_POD_SELECTOR
              value: {{ . | quote }}
            {{- end }}
            {{- end }}
            {{- if .Values.otlp.enabled }}
            - name: OTLP_HOST
              value: "0.0.0.0"
            - name: OTLP_PORT
              value: "4318"
            {{- end }}
            {{- with .Values.operations }}
            - name: OPERATOR_OPERATIONS
              value: {{ toJson . | quote }}
            {{- end }}
          ports:
            - name: readiness
              containerPort: 8081
            {{- if .Values.otlp.enabled }}
            - name: otlp
              containerPort: 4318
            {{- end }}
          readinessProbe:
            httpGet:
              path: /ready
              port: readiness
            periodSeconds: 2
            failureThreshold: 150
          volumeMounts:
            - name: enrollment
              mountPath: /etc/alien/enrollment
              readOnly: true
            - name: data
              mountPath: /var/lib/operator
            - name: tmp
              mountPath: /tmp
          resources:
            {{- toYaml .Values.resources | nindent 12 }}
      volumes:
        # Absent after the code was used and the release was upgraded without
        # it: the identity in the Secret is then all the Operator needs.
        - name: enrollment
          secret:
            secretName: {{ include "operator.enrollmentSecret" . }}
            optional: true
            defaultMode: 288
        # A cache. The identity and the key to this cache live in the Secret.
        - name: data
          emptyDir:
            sizeLimit: 1Gi
        - name: tmp
          emptyDir:
            sizeLimit: 64Mi
"#;

const SERVICE: &str = r#"{{- if .Values.otlp.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "operator.name" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
spec:
  type: ClusterIP
  selector:
    {{- include "operator.selectorLabels" . | nindent 4 }}
  ports:
    - name: otlp
      port: 4318
      targetPort: otlp
{{- end }}
"#;

const NOTES: &str = r#"The operator is starting. It connects this cluster to its control plane within a minute.

  kubectl -n {{ .Release.Namespace }} logs deploy/{{ include "operator.name" . }}
{{- if .Values.otlp.enabled }}

Applications can send OTLP over HTTP to:
  http://{{ include "operator.name" . }}.{{ .Release.Namespace }}.svc:4318
{{- end }}

The enrollment token works once. To reinstall, issue a new one.
"#;
