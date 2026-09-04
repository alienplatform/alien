use alien_helm::{
    generate_operator_manifest, HelmChart, OperatorManifestOptions, OperatorOutputFormat,
    OperatorPermission, OperatorScope,
};
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
    generate_operator_manifest(OperatorManifestOptions {
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
        format: OperatorOutputFormat::RawManifest,
    })
    .expect("operator manifest should render")
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
    let template = generate_operator_manifest(OperatorManifestOptions {
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
