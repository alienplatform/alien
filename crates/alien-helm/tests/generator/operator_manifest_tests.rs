use alien_helm::{
    generate_operator_manifest, HelmChart, OperatorManifestOptions, OperatorOutputFormat,
    OperatorPermission, OperatorScope,
};
use indexmap::IndexMap;

use super::test_utils;

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
