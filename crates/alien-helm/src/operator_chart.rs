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
use alien_operations_sdk::{KubernetesOperationPermissions, RiskTier};
use alien_permissions::operations::kubernetes::operator_runtime_rules;
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
    /// The built-in plugins. Empty means the chart does not know them: no plugin is opt-in and
    /// `operations.plugins` takes any name. When given, `operations.plugins` only takes these names.
    pub plugins: &'a [OperatorChartPlugin<'a>],
}

/// A built-in plugin as the chart sees it.
pub struct OperatorChartPlugin<'a> {
    pub name: &'a str,
    /// Off unless `operations.plugins.<name>` names it. Its rules are only in the ClusterRole then,
    /// so an existing install never gains them without the name being set.
    pub opt_in: bool,
    /// On in the chart's own `values.yaml`: the plugin is listed under `operations.plugins` with
    /// its approval rules written out (read-only operations `auto`, every other tier `manual`), so
    /// a plain install can run its read-only operations. Removing it is
    /// `--set operations.plugins.<name>=null`. An opt-in plugin is never on by default.
    pub default_enabled: bool,
}

/// Render the generic Operator chart.
pub fn generate_operator_chart(options: OperatorChartOptions<'_>) -> Result<HelmChart, String> {
    validate_digest(options.image_digest)?;
    if options.image_repository.is_empty() || options.image_repository.contains(['@', ' ']) {
        return Err("the image repository must be a name without a tag or digest".into());
    }
    // The same validation a plugin goes through: it refuses Secrets, exec and
    // wildcards, and a rule that does not fit the operation's tier.
    for operation in options.operations {
        operation.validate().map_err(|error| {
            format!(
                "operation {}/{} has invalid Kubernetes permissions: {error}",
                operation.plugin, operation.operation
            )
        })?;
    }
    for plugin in options.plugins {
        if plugin.opt_in && plugin.default_enabled {
            return Err(format!(
                "plugin {} cannot be both opt-in and on by default",
                plugin.name
            ));
        }
        if plugin.default_enabled
            && !options
                .operations
                .iter()
                .any(|operation| operation.plugin == plugin.name)
        {
            return Err(format!(
                "plugin {} is on by default but has no operations to list",
                plugin.name
            ));
        }
        if plugin.opt_in
            && !options
                .operations
                .iter()
                .any(|operation| operation.plugin == plugin.name)
        {
            return Err(format!(
                "opt-in plugin {} has no Kubernetes operations to grant",
                plugin.name
            ));
        }
    }
    // The approval kind's names come from the brand values at install time, so the generator
    // writes placeholders that Helm fills in (see `operator.crdBrand`).
    let crd_names = placeholder_crd_names();
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
        fill_crd_placeholders(&cluster_role_template(
            options.operations,
            options.plugins,
            &crd_names,
        )),
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
        fill_crd_placeholders(&access_request_crd_template(&crd_names)),
    );
    files.insert("templates/config.yaml".to_string(), CONFIG_MAP.to_string());
    files.insert("templates/NOTES.txt".to_string(), NOTES.to_string());
    files.insert("README.md".to_string(), README.to_string());
    Ok(HelmChart {
        name: "operator".to_string(),
        files,
    })
}

const CRD_GROUP: &str = "@@GROUP@@";
const CRD_KIND: &str = "@@KIND@@";
const CRD_PLURAL: &str = "@@PLURAL@@";
const CRD_SINGULAR: &str = "@@SINGULAR@@";
const CRD_SHORT: &str = "@@SHORT@@";
const CRD_NAME: &str = "@@CRD_NAME@@";

fn placeholder_crd_names() -> AccessRequestCrdNames {
    AccessRequestCrdNames {
        group: CRD_GROUP.into(),
        kind: CRD_KIND.into(),
        plural: CRD_PLURAL.into(),
        singular: CRD_SINGULAR.into(),
        short_name: CRD_SHORT.into(),
        api_version: format!("{CRD_GROUP}/v1alpha1"),
        crd_name: CRD_NAME.into(),
    }
}

/// Replace the placeholders with the Helm expressions that compute the brand's names.
fn fill_crd_placeholders(text: &str) -> String {
    text.replace(
        CRD_NAME,
        r#"{{ include "operator.crdPlural" . }}.{{ include "operator.crdGroup" . }}"#,
    )
    .replace(CRD_GROUP, r#"{{ include "operator.crdGroup" . }}"#)
    .replace(CRD_KIND, r#"{{ include "operator.crdKind" . }}"#)
    .replace(CRD_PLURAL, r#"{{ include "operator.crdPlural" . }}"#)
    .replace(CRD_SINGULAR, r#"{{ include "operator.crdSingular" . }}"#)
    .replace(CRD_SHORT, r#"{{ include "operator.crdShortName" . }}"#)
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

/// The `operations` value and the note above it. Each plugin that is on by default is listed with
/// the approval of every one of its operations, taken from the operation's tier: read-only is
/// `auto`; anything that changes or deletes is `manual`.
fn default_operations(options: &OperatorChartOptions<'_>) -> (String, String) {
    let defaults: Vec<&OperatorChartPlugin<'_>> = options
        .plugins
        .iter()
        .filter(|plugin| plugin.default_enabled)
        .collect();
    if defaults.is_empty() {
        return (String::new(), "operations: {}\n".to_string());
    }
    let mut yaml = String::from("operations:\n  plugins:\n");
    for plugin in &defaults {
        yaml.push_str(&format!("    {}:\n      approval:\n", plugin.name));
        for operation in options
            .operations
            .iter()
            .filter(|operation| operation.plugin == plugin.name)
        {
            let approval = if operation.tier == RiskTier::ReadOnly {
                "auto"
            } else {
                "manual"
            };
            yaml.push_str(&format!("        {}: {approval}\n", operation.operation));
        }
    }
    let names = defaults
        .iter()
        .map(|plugin| plugin.name)
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!(
            "# On from the first install, and listed below: {names}. Read-only operations run without\n# approval; every operation that changes something needs approval.\n"
        ),
        yaml,
    )
}

fn values_yaml(options: &OperatorChartOptions<'_>) -> String {
    let excluded_names = DEFAULT_EXCLUDED_NAMESPACES.join(", ");
    let (default_note, operations_default) = default_operations(options);
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
  # Read only these namespaces; empty means all namespaces not left out.
  namespaces: []
  # Left out in addition to {excluded_names}.
  excludeNamespaces: []
  # Set to true to read those system namespaces too.
  includeSystemNamespaces: false
  # Kubernetes label selector Pods must match, for example "app=web".
  podSelector: ""
  maxStreams: 200

otlp:
  # A ClusterIP Service on port {port} that applications send OTLP to. Anything in
  # the cluster that can reach it can send telemetry through this Operator, so it is
  # off until you need it.
  enabled: false

accessRequests:
  # The in-cluster approval flow (a custom resource). Off by default. The resource definition is
  # cluster-wide and kept when the release is removed, so approvals survive a reinstall.
  enabled: false
  # Install the approval resource definition with this release. It is cluster-wide, shared by every
  # release with the same brand, and kept when a release is removed. Set false for a second release
  # in a cluster (or a reinstall under another name) and it uses the definition already there.
  # Otherwise Helm stops with "exists and cannot be imported into the current release".
  installDefinition: true

# Ship the Operator under your own name, with no image of your own. All empty means the default
# product name. They become the Operator's packaged configuration (a ConfigMap), so the approval
# resource group, labels and log lines use your name.
brand:
  # The Operator's name, for example "acme-operator": a lowercase slug that starts with a letter,
  # at most 48 characters. It is the `app.kubernetes.io/name` label, which the Deployment's selector
  # uses, so it cannot be changed by `helm upgrade`; reinstall to rename.
  name: ""
  # Short slug for generated resource names, for example "acme".
  brand: ""
  # Shown in logs and startup messages, for example "Acme Operator".
  displayName: ""
  # Prefix of the branded environment variables, for example "ACME".
  envPrefix: ""
  # Label and approval-resource domain, for example "acme.dev"; its first label names the approval kind.
  labelDomain: ""

# Approval rules and settings for the built-in operations, passed to the Operator as JSON.
# Built-in plugins go under `plugins`, by plugin name; `approval` maps an operation name (or `*`)
# to `auto` or `manual`. Operations no rule matches need approval.
{default_note}#
# Add or change a plugin with `--set operations.plugins.<name>.approval.<operation>=auto`; Helm
# merges maps, so this keeps the plugins listed below. Remove a listed plugin with
# `--set operations.plugins.<name>=null`.
#
# This only sets approval and settings. It does not change the permissions the chart grants: those
# follow `permission`, except for an opt-in plugin: it is off until named here (`plugins: {{name: {{}}}}`),
# and naming it adds its rules (its changing rules only with `permission: remediation`). `custom` takes published custom plugins ({{name, version, approval, settings}}).
{operations_default}
resources:
  requests:
    cpu: 50m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi

nodeSelector: {{}}
tolerations: []
# Secrets of registries to pull the Operator image from, for example [{{name: regcred}}].
imagePullSecrets: []
# Extra environment variables for the Operator, for example proxy settings:
# [{{name: HTTPS_PROXY, value: "http://proxy:3128"}}].
# Plugin settings and these variables are rendered as plain environment values in the Deployment,
# visible to anyone who can read it: never put a secret in them.
extraEnv: []
"#,
        repository = yaml_string(options.image_repository),
        digest = yaml_string(options.image_digest),
        excluded_names = excluded_names,
        port = OPERATOR_CHART_OTLP_PORT,
        default_note = default_note,
        operations_default = operations_default,
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
                    "includeSystemNamespaces": {"type": "boolean"},
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
                "properties": {
                    "enabled": {"type": "boolean"},
                    "installDefinition": {"type": "boolean"}
                }
            },
            "operations": {
                "type": "object",
                "properties": {
                    "plugins": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "object",
                            "properties": {
                                "settings": {"type": "object"},
                                "approval": {"type": "object"}
                            }
                        }
                    },
                    "custom": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["name", "version"]
                        }
                    }
                }
            },
            "brand": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "name": {"type": "string", "pattern": "^$|^[a-z]([-a-z0-9]{0,46}[a-z0-9])?$"},
                    "brand": {"type": "string", "pattern": "^[a-z0-9]{0,30}$"},
                    "displayName": {"type": "string", "maxLength": 100},
                    "envPrefix": {"type": "string", "pattern": "^[A-Z][A-Z0-9_]{0,30}$|^$"},
                    "labelDomain": {"type": "string", "maxLength": 100}
                }
            },
            "resources": {"type": "object"},
            "nodeSelector": {"type": "object", "additionalProperties": {"type": "string"}},
            "tolerations": {"type": "array", "items": {"type": "object"}},
            "imagePullSecrets": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {"name": {"type": "string", "minLength": 1}}
                }
            },
            "extraEnv": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {"name": {"type": "string", "minLength": 1}}
                }
            },
            // Helm passes `global` to every subchart, so the chart can be one.
            "global": {"type": "object"},
            "enabled": {"type": "boolean"}
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

/// Which installs get a rule.
#[derive(Clone, Copy)]
enum Tier {
    /// Every install: the Operator's own needs and read-only operations.
    Always,
    /// Only `permission: remediation`: operations that change things.
    Remediation,
}

#[derive(Default)]
struct Grant {
    /// Verbs and the operations that need them, for every install.
    always: Section,
    /// Verbs and the operations that need them, for `remediation` installs.
    remediation: Section,
}

#[derive(Default)]
struct Section {
    verbs: BTreeSet<String>,
    reasons: BTreeSet<String>,
}

fn cluster_role_template(
    operations: &[KubernetesOperationPermissions],
    plugins: &[OperatorChartPlugin<'_>],
    crd_names: &AccessRequestCrdNames,
) -> String {
    let mut rules = BTreeMap::<RuleScope, Grant>::new();
    let mut add = |group: &str,
                   resource: &str,
                   names: BTreeSet<String>,
                   verbs: Vec<String>,
                   reason: String,
                   tier: Tier| {
        let grant = rules
            .entry(RuleScope {
                api_group: group.to_owned(),
                resource: resource.to_owned(),
                resource_names: names,
            })
            .or_default();
        let section = match tier {
            Tier::Always => &mut grant.always,
            Tier::Remediation => &mut grant.remediation,
        };
        section.verbs.extend(verbs);
        section.reasons.insert(reason);
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
            Tier::Always,
        );
    }
    // An operation's rules follow its tier as a whole, not verb by verb: a
    // read-only install gets the read-only operations and nothing of the others.
    let is_opt_in = |plugin: &str| {
        plugins
            .iter()
            .any(|candidate| candidate.opt_in && candidate.name == plugin)
    };
    for operation in operations
        .iter()
        .filter(|operation| !is_opt_in(&operation.plugin))
    {
        let tier = match operation.tier {
            RiskTier::ReadOnly => Tier::Always,
            RiskTier::Mutating | RiskTier::Destructive => Tier::Remediation,
        };
        for rule in &operation.permissions.rules {
            add(
                &rule.api_group,
                &rule.resource,
                rule.resource_names.iter().cloned().collect(),
                rule.verbs.clone(),
                format!(
                    "operation {}/{}: {}",
                    operation.plugin, operation.operation, rule.reason
                ),
                tier,
            );
        }
    }

    let mut yaml = String::new();
    if !plugins.is_empty() {
        // Helm's own schema wording for a bad property name points at the wrong path, so an
        // unknown plugin name is refused here with a message the chart owns.
        let names: Vec<&str> = plugins.iter().map(|plugin| plugin.name).collect();
        let list = names
            .iter()
            .map(|name| serde_json::to_string(name).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(" ");
        yaml.push_str(&format!(
            "{{{{- range $name, $_ := (default dict (.Values.operations).plugins) }}}}\n{{{{- if not (has $name (list {list})) }}}}\n{{{{- fail (printf \"operations.plugins.%s is not a plugin of this chart. Known plugins: %s\" $name {known}) }}}}\n{{{{- end }}}}\n{{{{- end }}}}\n",
            known = serde_json::to_string(&names.join(", ")).unwrap_or_default()
        ));
    }
    yaml.push_str(
        "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  name: {{ include \"operator.clusterRoleName\" . }}\n  labels:\n    {{- include \"operator.labels\" . | nindent 4 }}\nrules:\n",
    );
    for (scope, grant) in &rules {
        let comment = |section: &Section| -> String {
            section
                .reasons
                .iter()
                .map(|reason| format!("  # Required by {reason}\n"))
                .collect()
        };
        if !grant.always.verbs.is_empty() {
            yaml.push_str(&comment(&grant.always));
            yaml.push_str(&rule_yaml(scope, &grant.always.verbs));
        }
        if !grant.remediation.verbs.is_empty() {
            yaml.push_str("  {{- if eq .Values.permission \"remediation\" }}\n");
            yaml.push_str(&comment(&grant.remediation));
            yaml.push_str(&rule_yaml(scope, &grant.remediation.verbs));
            yaml.push_str("  {{- end }}\n");
        }
    }
    // An opt-in plugin's rules exist only when the plugin is named under `operations.plugins`,
    // and its changing rules only at `remediation` as for any operation.
    for plugin in plugins.iter().filter(|plugin| plugin.opt_in) {
        yaml.push_str(&format!(
            "  {{{{- if hasKey (default dict (.Values.operations).plugins) {} }}}}\n",
            serde_json::to_string(plugin.name).unwrap_or_default()
        ));
        for operation in operations
            .iter()
            .filter(|operation| operation.plugin == plugin.name)
        {
            let remediation = !matches!(operation.tier, RiskTier::ReadOnly);
            if remediation {
                yaml.push_str("  {{- if eq .Values.permission \"remediation\" }}\n");
            }
            for rule in &operation.permissions.rules {
                yaml.push_str(&format!(
                    "  # Required by operation {}/{}: {}\n",
                    operation.plugin, operation.operation, rule.reason
                ));
                let scope = RuleScope {
                    api_group: rule.api_group.clone(),
                    resource: rule.resource.clone(),
                    resource_names: rule.resource_names.iter().cloned().collect(),
                };
                yaml.push_str(&rule_yaml(&scope, &rule.verbs.iter().cloned().collect()));
            }
            if remediation {
                yaml.push_str("  {{- end }}\n");
            }
        }
        yaml.push_str("  {{- end }}\n");
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
    format!("{{{{- if and .Values.accessRequests.enabled .Values.accessRequests.installDefinition }}}}\n{crd}{{{{- end }}}}\n")
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

const HELPERS: &str = r#"{{- /* The name every object derives from. Installed on its own, the release name, so `helm install alien` gives `alien`. As a subchart the release name belongs to the parent, whose own objects may use it, so the chart's name is added. */ -}}
{{- define "operator.name" -}}
{{- if contains "/charts/" .Template.BasePath -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- /* Cluster-wide names cannot repeat across namespaces. Up to 63 characters they are the release and namespace as they are; longer ones keep the start and end with a short hash of the whole name, so two long names that differ only at the end stay apart. */ -}}
{{- define "operator.clusterRoleName" -}}
{{- $full := printf "%s-%s" (include "operator.name" .) .Release.Namespace -}}
{{- if le (len $full) 63 -}}
{{- $full -}}
{{- else -}}
{{- printf "%s-%s" ($full | trunc 54 | trimSuffix "-") (sha256sum $full | trunc 8) -}}
{{- end -}}
{{- end -}}

{{- define "operator.identitySecret" -}}
{{- printf "%s-identity" (include "operator.name" .) -}}
{{- end -}}

{{- define "operator.enrollmentSecret" -}}
{{- default (printf "%s-enrollment" (include "operator.name" .)) .Values.existingSecret -}}
{{- end -}}

{{- define "operator.configName" -}}
{{- printf "%s-config" (include "operator.name" .) -}}
{{- end -}}

{{- define "operator.brandName" -}}
{{- default "operator" .Values.brand.name -}}
{{- end -}}

{{- define "operator.branded" -}}
{{- if or .Values.brand.name .Values.brand.brand .Values.brand.displayName .Values.brand.envPrefix .Values.brand.labelDomain -}}true{{- end -}}
{{- end -}}

{{- /* The approval kind's brand: the label domain's first label, else the brand slug, else the default. Mirrors the Operator's own rule. */ -}}
{{- define "operator.crdBrand" -}}
{{- $source := default (default "alien" .Values.brand.brand) (trim .Values.brand.labelDomain) -}}
{{- $slug := regexReplaceAll "[^a-z0-9]" (lower (index (splitList "." $source) 0)) "" -}}
{{- default "alien" $slug -}}
{{- end -}}
{{- define "operator.crdGroup" -}}accessrequests.{{ include "operator.crdBrand" . }}{{- end -}}
{{- define "operator.crdPlural" -}}{{ include "operator.crdBrand" . }}accessrequests{{- end -}}
{{- define "operator.crdSingular" -}}{{ include "operator.crdBrand" . }}accessrequest{{- end -}}
{{- define "operator.crdShortName" -}}{{ include "operator.crdBrand" . }}ar{{- end -}}
{{- define "operator.crdKind" -}}{{ $b := include "operator.crdBrand" . }}{{ upper (substr 0 1 $b) }}{{ substr 1 (len $b) $b }}AccessRequest{{- end -}}

{{- define "operator.selectorLabels" -}}
app.kubernetes.io/name: {{ include "operator.brandName" . | quote }}
app.kubernetes.io/instance: {{ include "operator.name" . }}
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
  name: {{ printf "%s-enrollment" (include "operator.name" .) }}
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
{{- if and (include "operator.branded" .) .Values.accessRequests.enabled (not (trim .Values.brand.labelDomain)) }}
{{- fail "brand.labelDomain is required when brand values and accessRequests.enabled are both set: it names the approval resource's group (for example acme.dev). Without it the resource would be named after the default product." }}
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
      annotations:
        # A new token on upgrade restarts the pod, which then uses it.
        checksum/enrollment: {{ include (print $.Template.BasePath "/enrollment-secret.yaml") . | sha256sum }}
        {{- if include "operator.branded" . }}
        checksum/config: {{ include (print $.Template.BasePath "/config.yaml") . | sha256sum }}
        {{- end }}
    spec:
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
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
              value: "/etc/operator/enrollment/token"
            {{- if include "operator.branded" . }}
            - name: OPERATOR_CONFIG_FILE
              value: "/etc/operator/config/operator-config.json"
            {{- end }}
            - name: OPERATOR_ACCESS_REQUESTS
              value: {{ .Values.accessRequests.enabled | quote }}
            - name: OPERATOR_OTLP_MAX_QUEUED_BYTES
              value: "268435456"
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
            {{- if .Values.logs.includeSystemNamespaces }}
            - name: OPERATOR_POD_LOG_INCLUDE_SYSTEM_NAMESPACES
              value: "true"
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
            {{- with .Values.extraEnv }}
            {{- toYaml . | nindent 12 }}
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
              mountPath: /etc/operator/enrollment
              readOnly: true
            {{- if include "operator.branded" . }}
            - name: config
              mountPath: /etc/operator/config
              readOnly: true
            {{- end }}
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
        {{- if include "operator.branded" . }}
        - name: config
          configMap:
            name: {{ include "operator.configName" . }}
        {{- end }}
        # A cache. The identity and the key to this cache live in the Secret.
        - name: data
          emptyDir:
            sizeLimit: 1Gi
        - name: tmp
          emptyDir:
            sizeLimit: 64Mi
"#;

/// The Operator's packaged configuration, as the per-project image carries it, so a vendor can ship
/// the Operator under its own name from values alone.
const CONFIG_MAP: &str = r#"{{- if include "operator.branded" . }}
{{- $config := dict "syncIntervalSecs" 30 }}
{{- range $key, $value := dict "name" .Values.brand.name "brand" .Values.brand.brand "displayName" .Values.brand.displayName "envPrefix" .Values.brand.envPrefix "labelDomain" .Values.brand.labelDomain }}
{{- if $value }}{{- $_ := set $config $key $value }}{{- end }}
{{- end }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "operator.configName" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "operator.labels" . | nindent 4 }}
data:
  operator-config.json: {{ toJson $config | quote }}
{{- end }}
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

{{- if .Values.accessRequests.enabled }}

The approval resource definition is cluster-wide and kept when this release is removed. If Helm
stopped with "exists and cannot be imported into the current release", another release already owns
it: install again with `--set accessRequests.installDefinition=false`.
{{- end }}

About the enrollment token:
  * It works once, within an hour. The operator then keeps its identity in the Secret
    {{ include "operator.identitySecret" . }}; upgrades without a token leave it alone.
  * It names the control plane this operator will obey, so treat it as a trust root.
    `--set token=...` stays in the Helm release history; prefer `--set existingSecret=<name>`
    (a Secret in this namespace with the key `token`) and delete that Secret after the install.
  * `helm uninstall` deletes the identity Secret. Installing again, or restoring that Secret from
    an old backup, needs a new token.
  * With `existingSecret`, replacing the token inside that Secret does not restart the pod. Run
    `kubectl -n {{ .Release.Namespace }} rollout restart deploy/{{ include "operator.name" . }}`
    after replacing it.
"#;

const README: &str = r#"# Operator

Connects a Kubernetes cluster to its control plane: environment status, logs from the whole
cluster, an optional OTLP intake and the control plane's built-in operations.

    helm install alien oci://<registry>/operator --namespace alien --create-namespace \
      --set existingSecret=<name of a Secret with the key `token`>

or `--set token=<enrollment token>`.

## The enrollment token

- It works once, within an hour. The Operator then keeps its identity in the Secret
  `<release>-identity` (created empty by the chart) and its working data in an `emptyDir`, so
  there is no volume claim and no StorageClass is needed.
- It names the control plane this Operator will obey, so it is a trust root. `--set token=...`
  stays in the Helm release history; prefer `existingSecret`, and delete that Secret after the
  install.
- `helm upgrade` needs no token. A new `--set token=...` on upgrade restarts the pod, which uses
  it. With `existingSecret`, replacing the token in that Secret does not restart the pod: run
  `kubectl rollout restart deploy/<release>` afterwards.
- An enrolled Operator keeps the manager address it enrolled with. If the control plane later moves
  to a new hostname, a token for the new address is ignored until the old hostname is retired;
  install again with a newly issued token then.
- `helm uninstall` deletes the identity Secret. Installing again, or restoring the identity
  Secret from an old backup, needs a new token.

## Values

`token`, `existingSecret`, `image.repository`, `image.digest`, `permission`
(`diagnostics` or `remediation`), `logs.*`, `otlp.enabled`, `accessRequests.enabled`,
`accessRequests.installDefinition`, `operations`, `resources`, `nodeSelector`, `tolerations`, `imagePullSecrets`, `extraEnv`, `brand.*`.
See `values.yaml`.

`operations` lists the plugins that are on from the first install, each with the approval of every
operation: read-only operations are `auto`, everything that changes something is `manual`. A
plain install can therefore run its read-only operations at once. Add a plugin with
`--set operations.plugins.<name>.approval.<operation>=auto` (Helm merges maps, so the listed ones
stay), or remove a listed one with `--set operations.plugins.<name>=null`. The chart's permissions
do not depend on this value.
 The `brand.*` values ship the Operator under your own name with no image of your
own: they become the Operator's packaged configuration (a ConfigMap), so the release's labels, the
approval resource group and the log lines use that name:

    helm install acme-operator oci://<registry>/operator --namespace acme --create-namespace \
      --set existingSecret=acme-enrollment \
      --set brand.name=acme-operator --set brand.brand=acme --set brand.labelDomain=acme.dev

`brand.name` is in the Deployment's selector, so `helm upgrade` cannot change it. With
`accessRequests.enabled` and any `brand.*` value, `brand.labelDomain` is required (the render fails
without it). The approval resource definition is cluster-wide and kept on uninstall. A second release in the
same cluster with the same brand (or a reinstall under another release name) sets
`accessRequests.installDefinition=false` and uses the definition already there; without it Helm
stops with `exists and cannot be imported into the current release: invalid ownership metadata`.

There is no self-update: whoever installs the chart owns the image version.
"#;
