use alien_helm::{
    generate_operator_manifest, HelmChart, OperatorManifestOptions, OperatorOutputFormat,
    OperatorPermission, OperatorScope,
};
use alien_operations_sdk::{KubernetesOperationPermissions, PluginManifest};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml::Value as YamlValue;

use super::test_utils;

const TEST_ENCRYPTION_KEY: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn rendered_manifest(
    scope: OperatorScope,
    permission: OperatorPermission,
    kubernetes_operations_enabled: bool,
) -> String {
    rendered_with_custom(
        scope,
        permission,
        kubernetes_operations_enabled,
        &[],
        OperatorOutputFormat::RawManifest,
    )
    .expect("operator manifest should render")
}

fn rendered_with_custom(
    scope: OperatorScope,
    permission: OperatorPermission,
    kubernetes_operations_enabled: bool,
    custom_operation_permissions: &[KubernetesOperationPermissions],
    format: OperatorOutputFormat,
) -> alien_core::Result<String> {
    generate_operator_manifest(OperatorManifestOptions {
        custom_operation_permissions,
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        stack_settings: None,
        project_name: "my-saas",
        environment_name: Some("acme-prod-eu"),
        install_namespace: Some("demo"),
        label_domain: None,
        scope,
        label_selector: None,
        kubernetes_operations_enabled,
        permission,
        format,
    })
}

fn custom_operation(plugin_name: &str) -> KubernetesOperationPermissions {
    // Exercise the author-facing metadata contract, not a hand-built rule.
    let manifest = serde_json::json!({
        "name": plugin_name, "version": "1", "tier": "read-only",
        "binaries": {"amd64": "inspector"},
        "operations": [{"name": "inspect", "kubernetesPermissions": {
            "schemaVersion": 1, "rules": [{
                "apiGroup": "example.com", "resource": "widgets",
                "verbs": ["watch", "get", "list", "get"],
                "resourceNames": ["sample", "sample"], "reason": "Inspect selected widgets"
            }]
        }}]
    });
    let parsed = PluginManifest::parse_and_validate(manifest.to_string().as_bytes()).unwrap();
    let operation = &parsed.operations[0];
    KubernetesOperationPermissions {
        plugin: parsed.name.clone(),
        operation: operation.name.clone(),
        tier: operation.effective_tier(parsed.tier),
        permissions: operation.kubernetes_permissions.clone().unwrap(),
    }
}

#[test]
fn custom_permissions_follow_enabled_consumers_with_stable_scoped_rbac() {
    let first = custom_operation("first");
    let second = custom_operation("second");
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        for enabled in [
            vec![],
            vec![first.clone()],
            vec![first.clone(), second.clone()],
            vec![second.clone()],
            vec![],
        ] {
            let rendered = rendered_with_custom(
                scope,
                OperatorPermission::Diagnostics,
                false,
                &enabled,
                OperatorOutputFormat::RawManifest,
            )
            .unwrap();
            let docs = parse_manifest(&rendered);
            let role = docs
                .iter()
                .find(|doc| doc["kind"] == "Role" || doc["kind"] == "ClusterRole")
                .unwrap();
            let rules: Vec<_> = role["rules"]
                .as_sequence()
                .unwrap()
                .iter()
                .filter(|rule| rule["resources"][0] == "widgets")
                .collect();
            assert_eq!(rules.len(), usize::from(!enabled.is_empty()));
            if let Some(rule) = rules.first() {
                assert_eq!(
                    rule["apiGroups"],
                    serde_yaml::to_value(["example.com"]).unwrap()
                );
                assert_eq!(
                    rule["verbs"],
                    serde_yaml::to_value(["get", "list", "watch"]).unwrap()
                );
                assert_eq!(
                    rule["resourceNames"],
                    serde_yaml::to_value(["sample"]).unwrap()
                );
            }
            for operation in &enabled {
                assert!(rendered.contains(&format!(
                    "{}/inspect: Inspect selected widgets",
                    operation.plugin
                )));
            }
            let mut reordered = enabled.clone();
            reordered.reverse();
            for operation in &mut reordered {
                operation.permissions.rules[0].verbs.reverse();
            }
            assert_eq!(
                rendered,
                rendered_with_custom(
                    scope,
                    OperatorPermission::Diagnostics,
                    false,
                    &reordered,
                    OperatorOutputFormat::RawManifest
                )
                .unwrap()
            );
        }
    }
}

#[test]
fn custom_rules_obey_ceiling_and_validate_before_filtering() {
    let mut operation = custom_operation("restarter");
    operation.tier = alien_operations_sdk::RiskTier::Mutating;
    let rule = &mut operation.permissions.rules[0];
    rule.api_group.clear();
    rule.resource = "pods".into();
    rule.verbs = vec!["delete".into()];
    for (permission, expected) in [
        (OperatorPermission::Diagnostics, false),
        (OperatorPermission::Remediation, true),
    ] {
        let manifest = rendered_with_custom(
            OperatorScope::Namespace,
            permission,
            false,
            &[operation.clone()],
            OperatorOutputFormat::RawManifest,
        )
        .unwrap();
        let docs = parse_manifest(&manifest);
        let role = docs.iter().find(|doc| doc["kind"] == "Role").unwrap();
        assert_eq!(rule_allows(role, "pods", "delete"), expected);
    }
    for invalid_resource in ["secrets", "pods/exec", "*"] {
        operation.permissions.rules[0].resource = invalid_resource.into();
        assert!(rendered_with_custom(
            OperatorScope::Namespace,
            OperatorPermission::Diagnostics,
            false,
            &[operation.clone()],
            OperatorOutputFormat::RawManifest
        )
        .is_err());
    }
    operation.permissions.schema_version = 999;
    assert!(rendered_with_custom(
        OperatorScope::Namespace,
        OperatorPermission::Diagnostics,
        false,
        &[operation],
        OperatorOutputFormat::RawManifest
    )
    .is_err());
}

#[test]
fn shared_builtin_and_custom_grants_are_deduplicated_and_keep_both_reasons() {
    let mut operation = custom_operation("inspector");
    let rule = &mut operation.permissions.rules[0];
    rule.api_group.clear();
    rule.resource = "pods/log".to_owned();
    rule.verbs = vec!["get".to_owned()];
    rule.resource_names.clear();
    for (builtin, custom, expected) in [
        (true, true, 1),
        (true, false, 1),
        (false, true, 1),
        (false, false, 0),
    ] {
        let enabled = if custom {
            vec![operation.clone()]
        } else {
            vec![]
        };
        let manifest = rendered_with_custom(
            OperatorScope::Namespace,
            OperatorPermission::Diagnostics,
            builtin,
            &enabled,
            OperatorOutputFormat::RawManifest,
        )
        .unwrap();
        let docs = parse_manifest(&manifest);
        let role = docs.iter().find(|doc| doc["kind"] == "Role").unwrap();
        let rules: Vec<_> = role["rules"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter(|rule| rule["resources"][0] == "pods/log")
            .collect();
        assert_eq!(rules.len(), expected);
        assert_eq!(manifest.contains("the kubernetes/logs operation."), builtin);
        assert_eq!(
            manifest.contains("inspector/inspect: Inspect selected widgets"),
            custom
        );
    }
}

fn parse_manifest(manifest: &str) -> Vec<YamlValue> {
    serde_yaml::Deserializer::from_str(manifest)
        .map(|doc| YamlValue::deserialize(doc).expect("manifest document should be valid YAML"))
        .filter(|doc| !doc.is_null())
        .collect()
}

fn rule_allows(role: &YamlValue, resource: &str, verb: &str) -> bool {
    role["rules"]
        .as_sequence()
        .expect("RBAC document should contain rules")
        .iter()
        .any(|rule| {
            rule["resources"]
                .as_sequence()
                .is_some_and(|resources| resources.iter().any(|item| item == resource))
                && rule["verbs"]
                    .as_sequence()
                    .is_some_and(|verbs| verbs.iter().any(|item| item == verb))
        })
}

#[test]
fn complete_operator_manifests_intersect_operation_enablement_with_permission_ceiling() {
    let cases = [
        (false, OperatorPermission::Diagnostics, false, false),
        (false, OperatorPermission::Remediation, false, false),
        (true, OperatorPermission::Diagnostics, true, false),
        (true, OperatorPermission::Remediation, true, true),
    ];

    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        for (operations_enabled, permission, expect_logs, expect_writes) in cases {
            let manifest = rendered_manifest(scope, permission, operations_enabled);
            let docs = parse_manifest(&manifest);
            let rbac_kind = if scope == OperatorScope::Namespace {
                "Role"
            } else {
                "ClusterRole"
            };
            let role = docs
                .iter()
                .find(|doc| doc["kind"] == rbac_kind)
                .expect("manifest should contain the scope-appropriate RBAC document");

            assert!(
                rule_allows(role, "pods", "get"),
                "baseline pod inventory must remain available"
            );
            assert!(
                rule_allows(role, "alienaccessrequests", "create"),
                "access-request control resources must remain available"
            );
            assert_eq!(rule_allows(role, "pods/log", "get"), expect_logs);
            assert_eq!(rule_allows(role, "pods", "delete"), expect_writes);
            assert_eq!(
                rule_allows(role, "deployments/scale", "patch"),
                expect_writes
            );

            for rule in role["rules"]
                .as_sequence()
                .expect("RBAC document should contain rules")
            {
                assert!(
                    !rule["resources"]
                        .as_sequence()
                        .expect("rule should contain resources")
                        .iter()
                        .any(|resource| resource == "secrets"),
                    "operator RBAC must never grant access to Secrets"
                );
            }

            assert_eq!(
                manifest.contains("# Required by the kubernetes/logs operation."),
                expect_logs
            );
            assert_eq!(
                manifest.contains("# Required by the kubernetes/restart-pod operation."),
                expect_writes
            );
            assert_eq!(
                manifest.contains("# Required by the kubernetes/scale operation."),
                expect_writes
            );

            if scope == OperatorScope::Namespace {
                assert_eq!(role["metadata"]["namespace"], "demo");
                assert!(docs.iter().all(|doc| doc["kind"] != "ClusterRole"));
            } else {
                assert!(role["metadata"].get("namespace").is_none());
                let binding = docs
                    .iter()
                    .find(|doc| doc["kind"] == "ClusterRoleBinding")
                    .expect("cluster scope should include a ClusterRoleBinding");
                assert_eq!(binding["subjects"][0]["namespace"], "demo");
            }
        }
    }
}

#[test]
fn operator_template_accepts_cloud_identity_values() {
    let custom = custom_operation("inspector");
    let template = generate_operator_manifest(OperatorManifestOptions {
        custom_operation_permissions: &[custom],
        manager_url: "https://manager.example.com",
        group_token:
            "{{ required \"remoteOperator.registrationToken is required\" .Values.remoteOperator.registrationToken }}",
        encryption_key:
            "{{ required \"remoteOperator.encryptionKey is required\" .Values.remoteOperator.encryptionKey }}",
        image: "registry.example.com/operator:test",
        log_collector: None,
        stack_settings: None,
        project_name: "my-saas",
        environment_name: None,
        install_namespace: None,
        label_domain: None,
        scope: OperatorScope::Namespace,
        label_selector: None,
        kubernetes_operations_enabled: true,
        permission: OperatorPermission::Remediation,
        format: OperatorOutputFormat::HelmTemplate,
    })
    .expect("operator template should render");

    let chart = HelmChart {
        name: "operator-test".to_string(),
        files: IndexMap::from([
            (
                "Chart.yaml".to_string(),
                "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n".to_string(),
            ),
            (
                "values.yaml".to_string(),
                r#"remoteOperator:
  registrationToken: test-token
  encryptionKey: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  serviceAccountAnnotations:
    iam.gke.io/gcp-service-account: operator@example.iam.gserviceaccount.com
  podLabels:
    azure.workload.identity/use: "true"
alien:
  version: test
"#
                .to_string(),
            ),
            ("templates/byoc-operator.yaml".to_string(), template),
        ]),
    };

    test_utils::helm_lint(&chart.files).assert_ok("remote operator cloud identity helm lint");
    let rendered = test_utils::helm_template(&chart.files, None);
    rendered.assert_ok("remote operator cloud identity helm template");
    let documents = parse_manifest(&rendered.stdout);
    let role = documents.iter().find(|doc| doc["kind"] == "Role").unwrap();
    let rule = role["rules"]
        .as_sequence()
        .unwrap()
        .iter()
        .find(|rule| rule["resources"][0] == "widgets")
        .unwrap();
    assert_eq!(rule["apiGroups"][0], "example.com");
    assert_eq!(
        rule["resourceNames"],
        serde_yaml::to_value(["sample"]).unwrap()
    );
    assert_eq!(
        rule["verbs"],
        serde_yaml::to_value(["get", "list", "watch"]).unwrap()
    );
    assert!(
        rendered
            .stdout
            .contains("iam.gke.io/gcp-service-account: operator@example.iam.gserviceaccount.com"),
        "cloud identity annotation must be attached to the Operator ServiceAccount"
    );
    assert!(
        rendered
            .stdout
            .contains("azure.workload.identity/use: \"true\""),
        "AKS workload identity label must be attached to the Operator pod"
    );
}
