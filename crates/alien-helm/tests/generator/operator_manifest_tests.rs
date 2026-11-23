use alien_core::{Stack, StackSettings};
use alien_helm::{
    generate_operator_manifest, generate_operator_manifest_with_image_identity,
    generate_product_operator_manifest, HelmChart, OperatorImageIdentityOptions,
    OperatorLogCollectorOptions, OperatorManifestOptions, OperatorOutputFormat, OperatorPermission,
    OperatorScope, ProductOperatorManifestOptions,
};
use alien_operations_sdk::{CanonicalPluginManifest, KubernetesOperationPermissions};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use std::collections::BTreeSet;

use super::{helpers::render, test_utils};

const TEST_ENCRYPTION_KEY: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn standalone_pod_logs_select_only_labeled_pods() {
    let render = |key, value| {
        generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: Some(OperatorLogCollectorOptions {
                image: "fluent/fluent-bit:3.2",
                token: "collector-secret",
                pod_label_key: key,
                pod_label_value: value,
            }),
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        })
    };
    let log_scope = |manifest: &str| {
        let docs = parse_manifest(manifest);
        assert!(docs.iter().all(|doc| doc["kind"] != "DaemonSet"));
        let operator = docs
            .iter()
            .find(|doc| doc["kind"] == "Deployment")
            .expect("operator Deployment");
        let env = operator["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .expect("operator environment");
        let get = |name: &str| {
            env.iter()
                .find(|item| item["name"] == name)
                .expect("Pod log scope environment")["value"]
                .as_str()
                .expect("scope value")
                .to_string()
        };
        (
            get("OPERATOR_POD_LOG_LABEL_KEY"),
            get("OPERATOR_POD_LOG_LABEL_VALUE"),
        )
    };

    let default_manifest = render(None, None).expect("safe default Pod log scope");
    assert_eq!(
        log_scope(&default_manifest),
        (
            "alien.dev/deployment".to_string(),
            "my-saas-operator".to_string()
        )
    );

    let selected_manifest = render(Some("app"), Some("external.agent")).expect("selected Pod logs");
    assert_eq!(
        log_scope(&selected_manifest),
        ("app".to_string(), "external.agent".to_string())
    );
    let docs = parse_manifest(&selected_manifest);
    let operator = docs.iter().find(|doc| doc["kind"] == "Deployment").unwrap();
    assert_eq!(
        operator["spec"]["template"]["metadata"]["labels"]["alien.dev/log-collector-exclude"],
        "true"
    );
    let log_role = docs.iter().find(|doc| {
        doc["kind"] == "Role"
            && doc["rules"]
                .as_sequence()
                .is_some_and(|rules| rules.iter().any(|rule| rule["resources"][0] == "pods/log"))
    });
    assert!(log_role.is_some(), "Pod log reader needs a namespaced Role");

    assert!(render(Some("app"), None).is_err());
    assert!(render(Some("bad/key/extra"), Some("agent")).is_err());
}

fn rendered_manifest(scope: OperatorScope, permission: OperatorPermission) -> String {
    rendered_with_custom(scope, permission, &[], OperatorOutputFormat::RawManifest)
        .expect("operator manifest should render")
}

fn rendered_with_custom(
    scope: OperatorScope,
    permission: OperatorPermission,
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
    let parsed =
        CanonicalPluginManifest::parse_and_validate(manifest.to_string().as_bytes()).unwrap();
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
            &[operation.clone()],
            OperatorOutputFormat::RawManifest
        )
        .is_err());
    }
    operation.permissions.schema_version = 999;
    assert!(rendered_with_custom(
        OperatorScope::Namespace,
        OperatorPermission::Diagnostics,
        &[operation],
        OperatorOutputFormat::RawManifest
    )
    .is_err());
}

#[test]
fn custom_rules_reject_malformed_identifiers_before_rendering() {
    for (group, resource) in [
        ("example..com", "widgets"),
        ("example.-com", "widgets"),
        ("example.com", "1widgets"),
        ("example.com", "custom.widgets"),
    ] {
        let mut operation = custom_operation("inspector");
        operation.permissions.rules[0].api_group = group.to_owned();
        operation.permissions.rules[0].resource = resource.to_owned();
        for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
            for format in [
                OperatorOutputFormat::RawManifest,
                OperatorOutputFormat::HelmTemplate,
            ] {
                assert!(rendered_with_custom(
                    scope,
                    OperatorPermission::Diagnostics,
                    &[operation.clone()],
                    format,
                )
                .is_err());
            }
        }
    }
}

#[test]
fn operation_grants_shared_with_the_runtime_keep_both_reasons() {
    let mut operation = custom_operation("inspector");
    let rule = &mut operation.permissions.rules[0];
    rule.api_group.clear();
    rule.resource = "pods".to_owned();
    rule.verbs = vec!["list".to_owned(), "get".to_owned()];
    rule.resource_names.clear();
    for (enabled, expected_verbs) in [
        (vec![], &["list"][..]),
        (vec![operation.clone()], &["get", "list"][..]),
    ] {
        let manifest = rendered_with_custom(
            OperatorScope::Namespace,
            OperatorPermission::Diagnostics,
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
            .filter(|rule| rule["apiGroups"][0] == "" && rule["resources"][0] == "pods")
            .collect();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0]["verbs"],
            serde_yaml::to_value(expected_verbs).unwrap()
        );
        assert!(manifest
            .contains("# Required by the Operator runtime: Pod status of each observed workload"));
        assert_eq!(
            manifest.contains("inspector/inspect: Inspect selected widgets"),
            !enabled.is_empty()
        );
    }
}

fn parse_manifest(manifest: &str) -> Vec<YamlValue> {
    serde_yaml::Deserializer::from_str(manifest)
        .map(|doc| YamlValue::deserialize(doc).expect("manifest document should be valid YAML"))
        .filter(|doc| !doc.is_null())
        .collect()
}

#[test]
fn standalone_manifests_bind_no_dynamic_workload_access() {
    // A standalone Remote Operator has no installed release that could approve
    // a dynamic container image, so neither scope binds the dynamic Role.
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        let manifest = rendered_manifest(scope, OperatorPermission::Remediation);
        let docs = parse_manifest(&manifest);
        assert!(!docs.iter().any(|doc| {
            doc["metadata"]["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("alien-dc-"))
        }));
        let grants = permission_grants(&manifest);
        assert!(!grants
            .iter()
            .any(|(_, resource, _, _)| resource == "secrets"));
        // Without enabled operations, everything outside the access-request CRD is a listing.
        assert!(grants
            .iter()
            .filter(|(group, ..)| group != "accessrequests.alien")
            .all(|(_, _, verb, _)| verb == "list"));
        assert!(!manifest.contains("OPERATOR_DYNAMIC_CONTAINERS"));
    }
}

fn permission_grants(manifest: &str) -> BTreeSet<(String, String, String, Vec<String>)> {
    let mut grants = BTreeSet::new();
    for doc in parse_manifest(manifest) {
        if doc["kind"] != "Role" && doc["kind"] != "ClusterRole" {
            continue;
        }
        for rule in doc["rules"].as_sequence().unwrap() {
            let mut names: Vec<_> = rule["resourceNames"]
                .as_sequence()
                .into_iter()
                .flatten()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect();
            names.sort();
            for group in rule["apiGroups"].as_sequence().unwrap() {
                for resource in rule["resources"].as_sequence().unwrap() {
                    for verb in rule["verbs"].as_sequence().unwrap() {
                        grants.insert((
                            group.as_str().unwrap().to_owned(),
                            resource.as_str().unwrap().to_owned(),
                            verb.as_str().unwrap().to_owned(),
                            names.clone(),
                        ));
                    }
                }
            }
        }
    }
    grants
}

#[test]
fn attribution_cannot_inject_grants_into_rendered_yaml() {
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        let mut operation = custom_operation("inspector");
        operation.permissions.rules[0].reason = "Inspect widgets — 状態".to_owned();
        let mut expected =
            permission_grants(&rendered_manifest(scope, OperatorPermission::Diagnostics));
        for verb in ["get", "list", "watch"] {
            expected.insert((
                "example.com".to_owned(),
                "widgets".to_owned(),
                verb.to_owned(),
                vec!["sample".to_owned()],
            ));
        }
        let valid = rendered_with_custom(
            scope,
            OperatorPermission::Diagnostics,
            &[operation.clone()],
            OperatorOutputFormat::RawManifest,
        )
        .unwrap();
        assert_eq!(permission_grants(&valid), expected);

        for line_break in ['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'] {
            let attribution = format!("Inspect{line_break}  - apiGroups: [\"\"]{line_break}    resources: [\"secrets\"]{line_break}    verbs: [\"get\"]{line_break}  #");
            for field in ["plugin", "operation", "reason"] {
                let mut malicious = operation.clone();
                match field {
                    "plugin" => malicious.plugin = attribution.clone(),
                    "operation" => malicious.operation = attribution.clone(),
                    _ => malicious.permissions.rules[0].reason = attribution.clone(),
                }
                for format in [
                    OperatorOutputFormat::RawManifest,
                    OperatorOutputFormat::HelmTemplate,
                ] {
                    let rendered = rendered_with_custom(
                        scope,
                        OperatorPermission::Diagnostics,
                        &[malicious.clone()],
                        format,
                    );
                    if format == OperatorOutputFormat::RawManifest {
                        if let Ok(manifest) = &rendered {
                            // Inspect actual YAML semantics, not just the string:
                            // an accepted comment must never add another grant.
                            assert_eq!(
                                permission_grants(manifest),
                                expected,
                                "attribution changed the emitted grants"
                            );
                        }
                    }
                    assert!(rendered.is_err(), "accepted {line_break:?} in {field}");
                }
            }
        }
    }
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
fn operator_rbac_without_operations_is_the_runtime_and_access_request_rules() {
    let mut expected = BTreeSet::new();
    // These grants follow the runtime call sites, independently of the shared
    // rule source used by the renderer.
    for (group, resource) in [
        ("apps", "deployments"),
        ("apps", "statefulsets"),
        ("apps", "daemonsets"),
        ("", "pods"),
        ("", "events"),
        ("metrics.k8s.io", "pods"),
    ] {
        expected.insert((
            group.to_owned(),
            resource.to_owned(),
            "list".to_owned(),
            vec![],
        ));
    }
    for (resource, verbs) in [
        (
            "alienaccessrequests",
            &["create", "get", "list", "patch", "update", "watch"][..],
        ),
        ("alienaccessrequests/status", &["get", "patch", "update"]),
    ] {
        for verb in verbs {
            expected.insert((
                "accessrequests.alien".to_owned(),
                resource.to_owned(),
                (*verb).to_owned(),
                vec![],
            ));
        }
    }
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        for permission in [
            OperatorPermission::Diagnostics,
            OperatorPermission::Remediation,
        ] {
            let docs = parse_manifest(&rendered_manifest(scope, permission));
            let rbac_kind = if scope == OperatorScope::Namespace {
                "Role"
            } else {
                "ClusterRole"
            };
            let role = docs
                .iter()
                .find(|doc| {
                    doc["kind"] == rbac_kind && doc["metadata"]["name"] == "my-saas-operator"
                })
                .expect("manifest should contain the scope-appropriate operator RBAC document");
            let role_yaml = serde_yaml::to_string(role).unwrap();
            assert_eq!(permission_grants(&role_yaml), expected);
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

#[test]
fn operator_template_can_reference_setup_owned_credentials() {
    let template = generate_product_operator_manifest(ProductOperatorManifestOptions {
        manifest: OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "",
            encryption_key: "",
            image: "registry.example.com/operator:test",
            log_collector: Some(OperatorLogCollectorOptions {
                image: "registry.example.com/collector:test",
                token: "",
                pod_label_key: None,
                pod_label_value: None,
            }),
            stack_settings: None,
            project_name: "my-saas",
            environment_name: None,
            install_namespace: None,
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            permission: OperatorPermission::Remediation,
            format: OperatorOutputFormat::HelmTemplate,
        },
        credentials_secret_name:
            "{{ required \"remoteOperator.existingSecret.name is required\" .Values.remoteOperator.existingSecret.name }}",
        credentials_encryption_key_sha256:
            "{{ required \"remoteOperator.existingSecret.encryptionKeySha256 is required\" .Values.remoteOperator.existingSecret.encryptionKeySha256 }}",
        resource_name: Some("my-saas-operator"),
    })
    .expect("operator template should accept an existing credential Secret");

    let chart = HelmChart {
        name: "operator-test".to_string(),
        files: IndexMap::from([
            (
                "Chart.yaml".to_string(),
                "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n".to_string(),
            ),
            (
                "values.yaml".to_string(),
                "remoteOperator:\n  existingSecret:\n    name: setup-owned\nlogCollector:\n  enabled: true\n  mode: podApi\n  scope:\n    deploymentLabelKey: alien.dev/deployment\n    deploymentLabelValue: my-saas-operator\n".to_string(),
            ),
            ("templates/byoc-operator.yaml".to_string(), template),
        ]),
    };

    let rendered = test_utils::helm_template(&chart.files, None);
    rendered.assert_ok("existing credential Secret helm template");
    assert!(
        !rendered
            .stdout
            .contains("OPERATOR_IDENTITY_INITIALIZED_CONFIGMAP"),
        "the reusable product-operator template must not reference host-chart lifecycle helpers"
    );
    let documents = parse_manifest(&rendered.stdout);
    assert!(
        documents
            .iter()
            .all(|document| document["kind"] != "Secret"),
        "the product release must not copy bootstrap credentials into Helm-owned values"
    );
    let deployment = documents
        .iter()
        .find(|document| document["kind"] == "Deployment")
        .expect("operator Deployment");
    let credential_volume = deployment["spec"]["template"]["spec"]["volumes"]
        .as_sequence()
        .expect("deployment volumes")
        .iter()
        .find(|volume| volume["name"] == "credentials")
        .expect("credential volume");
    assert_eq!(credential_volume["secret"]["secretName"], "setup-owned");
    assert!(documents
        .iter()
        .all(|document| document["kind"] != "DaemonSet"));
    let env = deployment["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .expect("operator environment");
    assert!(env.iter().any(|item| {
        item["name"] == "OPERATOR_POD_LOG_LABEL_KEY" && item["value"] == "alien.dev/deployment"
    }));
}

#[test]
fn operator_manifest_reports_exact_package_image_identity() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let image = format!("registry.example.com/operator@{digest}");
    let manifest = generate_operator_manifest_with_image_identity(
        OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_ENCRYPTION_KEY,
            image: &image,
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            permission: OperatorPermission::Remediation,
            format: OperatorOutputFormat::RawManifest,
        },
        OperatorImageIdentityOptions::Package {
            package_id: "pkg_operator",
            package_version: "1.2.3",
        },
    )
    .expect("exact image identity should render");

    let documents = parse_manifest(&manifest);
    let deployment = documents
        .iter()
        .find(|document| document["kind"] == "Deployment")
        .expect("operator Deployment");
    let env = &deployment["spec"]["template"]["spec"]["containers"][0]["env"];
    let env_value = |name: &str| {
        env.as_sequence()
            .expect("env sequence")
            .iter()
            .find(|entry| entry["name"].as_str() == Some(name))
            .and_then(|entry| entry["value"].as_str())
    };

    assert_eq!(env_value("ALIEN_OPERATOR_IMAGE_SOURCE"), Some("package"));
    assert_eq!(
        env_value("ALIEN_OPERATOR_IMAGE_RECEIPT"),
        Some(image.as_str())
    );
    assert_eq!(env_value("ALIEN_OPERATOR_IMAGE"), None);
    assert_eq!(
        env_value("ALIEN_OPERATOR_IMAGE_DIGEST"),
        Some(digest.as_str())
    );
    assert_eq!(
        env_value("ALIEN_OPERATOR_IMAGE_PACKAGE_ID"),
        Some("pkg_operator")
    );
    assert_eq!(
        env_value("ALIEN_OPERATOR_IMAGE_PACKAGE_VERSION"),
        Some("1.2.3")
    );
}

#[test]
fn compiled_operation_grants_match_rendered_rbac_in_both_scopes_and_modes() {
    use alien_permissions::operations::{self, CatalogPlugin, KubernetesMode};

    let mut operation = custom_operation("inspector");
    operation.tier = alien_operations_sdk::RiskTier::Mutating;
    operation
        .permissions
        .rules
        .push(alien_operations_sdk::KubernetesPermissionRule {
            api_group: "".into(),
            resource: "pods".into(),
            verbs: vec!["get".into(), "delete".into()],
            resource_names: vec!["selected-pod".into()],
            reason: "Inspect and restart the selected pod".into(),
        });
    let catalog: CatalogPlugin = serde_json::from_value(serde_json::json!({
        "name": operation.plugin, "enabled": true, "operations": [{
            "name": operation.operation, "kubernetesPermissions": operation.permissions
        }]
    }))
    .unwrap();
    let grants = operations::kubernetes::collect(&[catalog]);
    let role_grants = |manifest: &str| {
        let docs = parse_manifest(manifest);
        let role = docs
            .iter()
            .find(|doc| doc["kind"] == "Role" || doc["kind"] == "ClusterRole")
            .unwrap();
        role["rules"]
            .as_sequence()
            .unwrap()
            .iter()
            .flat_map(|rule| {
                let names = rule["resourceNames"]
                    .as_sequence()
                    .map(|names| {
                        names
                            .iter()
                            .map(|name| name.as_str().unwrap().to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                rule["verbs"]
                    .as_sequence()
                    .unwrap()
                    .iter()
                    .map(move |verb| {
                        (
                            rule["apiGroups"][0].as_str().unwrap().to_string(),
                            rule["resources"][0].as_str().unwrap().to_string(),
                            verb.as_str().unwrap().to_string(),
                            names.clone(),
                        )
                    })
            })
            .collect::<BTreeSet<_>>()
    };
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        for (permission, mode) in [
            (OperatorPermission::Diagnostics, KubernetesMode::Diagnostics),
            (OperatorPermission::Remediation, KubernetesMode::Remediation),
        ] {
            let baseline =
                rendered_with_custom(scope, permission, &[], OperatorOutputFormat::RawManifest)
                    .unwrap();
            let mut expected = role_grants(&baseline);
            for grant in operations::kubernetes::compile(&grants, mode).unwrap() {
                for verb in grant.verbs {
                    expected.insert((
                        grant.api_group.clone(),
                        grant.resource.clone(),
                        verb,
                        grant.resource_names.clone(),
                    ));
                }
            }
            let actual = rendered_with_custom(
                scope,
                permission,
                &[operation.clone()],
                OperatorOutputFormat::RawManifest,
            )
            .unwrap();
            assert_eq!(role_grants(&actual), expected);
        }
    }
}

#[test]
fn chart_version_keeps_pod_templates_but_image_changes_roll_them() {
    let stack = Stack::new("stable-operator".into()).build();
    let chart = render(&stack, StackSettings::default());
    let values = "logCollector:\n  enabled: true\n  mode: nodeAgent\n";
    let workloads = |files: &IndexMap<String, String>, values: &str| {
        let rendered = test_utils::helm_template(files, Some(values));
        rendered.assert_ok("workload version upgrade");
        parse_manifest(&rendered.stdout)
            .into_iter()
            .filter(|doc| !doc["spec"]["template"].is_null())
            .map(|doc| (doc["metadata"]["name"].as_str().unwrap().to_owned(), doc))
            .collect::<IndexMap<_, _>>()
    };
    let before = workloads(&chart.files, values);
    assert!(before.values().any(|doc| doc["kind"] == "Deployment"));
    assert!(before.values().any(|doc| doc["kind"] == "DaemonSet"));
    assert!(before.values().any(|doc| doc["kind"] == "Job"));
    let mut upgraded = chart.files.clone();
    let mut metadata: YamlValue = serde_yaml::from_str(&upgraded["Chart.yaml"]).unwrap();
    metadata["version"] = YamlValue::String("99.0.0".into());
    upgraded.insert(
        "Chart.yaml".into(),
        serde_yaml::to_string(&metadata).unwrap(),
    );
    let after = workloads(&upgraded, values);
    assert_eq!(before.len(), after.len());
    for (name, original) in &before {
        assert_eq!(
            original["spec"]["selector"],
            after[name]["spec"]["selector"]
        );
        assert_eq!(
            original["spec"]["template"], after[name]["spec"]["template"],
            "{name}"
        );
        assert_ne!(
            original["metadata"]["labels"]["helm.sh/chart"],
            after[name]["metadata"]["labels"]["helm.sh/chart"]
        );
    }
    let changed_image = workloads(
        &upgraded,
        &format!("{values}  image:\n    repository: registry.example.com/collector\n    tag: next\nruntime:\n  image:\n    repository: registry.example.com/operator\n    tag: next\n"),
    );
    for (name, original) in &after {
        if original["kind"] == "Deployment" || original["kind"] == "DaemonSet" {
            assert_ne!(
                original["spec"]["template"], changed_image[name]["spec"]["template"],
                "{name}"
            );
            assert_eq!(
                original["spec"]["selector"],
                changed_image[name]["spec"]["selector"]
            );
        }
    }
}

const POLICY_API: &str = "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy";
const WORKLOAD_KINDS: [&str; 3] = ["deployments", "statefulsets", "daemonsets"];

/// An operation that may patch the given workload kinds, declared as a plugin author would.
fn restart_operation(kinds: &[&str], names: &[&str]) -> KubernetesOperationPermissions {
    let mut operation = custom_operation("restarter");
    operation.tier = alien_operations_sdk::RiskTier::Mutating;
    operation.permissions.rules = kinds
        .iter()
        .map(|kind| alien_operations_sdk::KubernetesPermissionRule {
            api_group: "apps".into(),
            resource: (*kind).into(),
            verbs: vec!["patch".into()],
            resource_names: names.iter().map(|name| (*name).into()).collect(),
            reason: "Restart workloads".into(),
        })
        .collect();
    operation
}

/// The Operator template as a Helm chart, rendered for release `rel` in namespace `ns` on a
/// cluster serving `api_versions`.
fn rendered_operator_chart(
    scope: OperatorScope,
    permission: OperatorPermission,
    operations: &[KubernetesOperationPermissions],
    api_versions: &[&str],
) -> Vec<YamlValue> {
    rendered_operator_chart_with_values(scope, permission, operations, api_versions, None)
}

fn rendered_operator_chart_with_values(
    scope: OperatorScope,
    permission: OperatorPermission,
    operations: &[KubernetesOperationPermissions],
    api_versions: &[&str],
    values: Option<&str>,
) -> Vec<YamlValue> {
    let template = generate_operator_manifest(OperatorManifestOptions {
        custom_operation_permissions: operations,
        manager_url: "https://manager.example.com",
        group_token: "test-token",
        encryption_key: TEST_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        stack_settings: None,
        project_name: "my-saas",
        environment_name: None,
        install_namespace: None,
        label_domain: None,
        scope,
        label_selector: None,
        permission,
        format: OperatorOutputFormat::HelmTemplate,
    })
    .expect("operator template should render");
    let files = IndexMap::from([
        (
            "Chart.yaml".to_string(),
            "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n".to_string(),
        ),
        (
            "values.yaml".to_string(),
            "remoteOperator:\n  serviceAccountAnnotations: {}\nalien:\n  version: test\n"
                .to_string(),
        ),
        ("templates/operator.yaml".to_string(), template),
    ]);
    let rendered =
        test_utils::helm_template_with_api_versions(&files, values, "rel", "ns1", api_versions);
    rendered.assert_ok("operator chart helm template");
    parse_manifest(&rendered.stdout)
}

fn hook_events(doc: &YamlValue) -> Vec<String> {
    doc["metadata"]["annotations"]["helm.sh/hook"]
        .as_str()
        .map(|events| events.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn grants_workload_patch(role: &YamlValue) -> BTreeSet<String> {
    role["rules"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter(|rule| {
            rule["apiGroups"][0] == "apps"
                && rule["verbs"]
                    .as_sequence()
                    .is_some_and(|verbs| verbs.iter().any(|verb| verb == "patch"))
        })
        .flat_map(|rule| rule["resources"].as_sequence().into_iter().flatten())
        .filter_map(|resource| resource.as_str())
        .filter(|resource| WORKLOAD_KINDS.contains(resource))
        .map(str::to_owned)
        .collect()
}

/// Resources the chart's grant lets the Operator `patch` in the `apps` group. The rule is in a
/// Role or ClusterRole of the release (inert alone, and removed by a rollback to a chart without
/// it); it reaches the Operator only through a binding run as a `post-install,post-upgrade,
/// post-rollback` hook, and a `pre-install,pre-upgrade,pre-rollback,pre-delete` hook of the same
/// name removes any earlier binding again. No
/// binding of the release itself may name such a role.
fn patchable_workloads(docs: &[YamlValue]) -> BTreeSet<String> {
    let is_role = |doc: &YamlValue| doc["kind"] == "Role" || doc["kind"] == "ClusterRole";
    let is_binding =
        |doc: &YamlValue| doc["kind"] == "RoleBinding" || doc["kind"] == "ClusterRoleBinding";
    // The throwaway probe account (see `restart_probe_hooks`) is the only hook role.
    let is_probe = |doc: &YamlValue| {
        doc["metadata"]["name"]
            .as_str()
            .is_some_and(|name| name.ends_with("-probe"))
    };
    for role in docs.iter().filter(|doc| is_role(doc) && !is_probe(doc)) {
        assert!(hook_events(role).is_empty(), "the patch rule is not a hook");
    }
    let hook_bindings = |event: &str| -> Vec<&YamlValue> {
        docs.iter()
            .filter(|doc| {
                is_binding(doc) && !is_probe(doc) && hook_events(doc).iter().any(|e| e == event)
            })
            .collect()
    };
    let install = hook_bindings("post-install");
    let delete = hook_bindings("pre-delete");
    for binding in &install {
        assert_eq!(
            hook_events(binding),
            ["post-install", "post-upgrade", "post-rollback"],
            "the grant is bound after the release objects are applied"
        );
        assert_eq!(
            binding["metadata"]["annotations"]["helm.sh/hook-delete-policy"],
            "before-hook-creation"
        );
    }
    for binding in &delete {
        // A binding an earlier revision left behind must be gone before the
        // Role and the policy are applied again, so the copy also runs before
        // every install, upgrade and rollback.
        assert_eq!(
            hook_events(binding),
            ["pre-install", "pre-upgrade", "pre-rollback", "pre-delete"],
            "no binding survives into the apply phase"
        );
        assert_eq!(
            binding["metadata"]["annotations"]["helm.sh/hook-delete-policy"],
            "before-hook-creation,hook-succeeded"
        );
    }
    let names = |bindings: &[&YamlValue]| -> BTreeSet<String> {
        bindings
            .iter()
            .map(|doc| doc["metadata"]["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(&install),
        names(&delete),
        "uninstall removes what the hook created"
    );
    let bound: BTreeSet<String> = install
        .iter()
        .map(|doc| doc["roleRef"]["name"].as_str().unwrap().to_owned())
        .collect();
    let mut granted = BTreeSet::new();
    for role in docs.iter().filter(|doc| is_role(doc) && !is_probe(doc)) {
        let patch = grants_workload_patch(role);
        if patch.is_empty() {
            continue;
        }
        assert!(
            bound.contains(role["metadata"]["name"].as_str().unwrap()),
            "a release role with a workload patch has a hook binding"
        );
        granted.extend(patch);
    }
    for binding in docs
        .iter()
        .filter(|doc| is_binding(doc) && !is_probe(doc) && hook_events(doc).is_empty())
    {
        let role = docs.iter().find(|doc| {
            is_role(doc)
                && doc["kind"] == binding["roleRef"]["kind"]
                && doc["metadata"]["name"] == binding["roleRef"]["name"]
        });
        if let Some(role) = role {
            assert!(
                grants_workload_patch(role).is_empty(),
                "a release binding gives the Operator a workload patch"
            );
        }
    }
    granted
}

/// Resources a bound policy of the chart limits, and whether every policy has a binding.
fn restricted_workloads(docs: &[YamlValue]) -> BTreeSet<String> {
    let policies: Vec<_> = docs
        .iter()
        .filter(|doc| doc["kind"] == "ValidatingAdmissionPolicy")
        .collect();
    let bindings: BTreeSet<_> = docs
        .iter()
        .filter(|doc| doc["kind"] == "ValidatingAdmissionPolicyBinding")
        .map(|doc| doc["spec"]["policyName"].as_str().unwrap().to_owned())
        .collect();
    let names: BTreeSet<_> = policies
        .iter()
        .map(|doc| doc["metadata"]["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names, bindings,
        "every policy needs its binding and the reverse"
    );
    policies
        .iter()
        .flat_map(|doc| {
            doc["spec"]["matchConstraints"]["resourceRules"]
                .as_sequence()
                .unwrap()
        })
        .flat_map(|rule| rule["resources"].as_sequence().unwrap())
        .map(|resource| resource.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn workload_patch_is_rendered_if_and_only_if_its_restart_policy_is() {
    let cases: Vec<Vec<&str>> = vec![
        vec![],
        vec!["deployments"],
        vec!["statefulsets", "daemonsets"],
        WORKLOAD_KINDS.to_vec(),
    ];
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        for kinds in &cases {
            let operations: Vec<_> = (!kinds.is_empty())
                .then(|| restart_operation(kinds, &[]))
                .into_iter()
                .collect();
            for permission in [
                OperatorPermission::Diagnostics,
                OperatorPermission::Remediation,
            ] {
                for api_versions in [&[][..], &[POLICY_API][..]] {
                    let docs =
                        rendered_operator_chart(scope, permission, &operations, api_versions);
                    let patched = patchable_workloads(&docs);
                    let restricted = restricted_workloads(&docs);
                    let expected: BTreeSet<String> = if permission
                        == OperatorPermission::Remediation
                        && !api_versions.is_empty()
                    {
                        kinds.iter().map(|kind| (*kind).to_owned()).collect()
                    } else {
                        BTreeSet::new()
                    };
                    let context =
                        format!("{scope:?} {kinds:?} {permission:?} api={api_versions:?}");
                    assert_eq!(patched, expected, "patch rules: {context}");
                    // The policy also covers Deployments, for the probe Deployment.
                    let mut covered = expected.clone();
                    if !covered.is_empty() {
                        covered.insert("deployments".to_owned());
                    }
                    assert_eq!(restricted, covered, "restart policies: {context}");
                }
            }
        }
    }
}

#[test]
fn raw_manifests_never_render_a_workload_patch_or_policy() {
    let operations = [restart_operation(&WORKLOAD_KINDS, &[])];
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        let manifest = rendered_with_custom(
            scope,
            OperatorPermission::Remediation,
            &operations,
            OperatorOutputFormat::RawManifest,
        )
        .unwrap();
        let docs = parse_manifest(&manifest);
        assert!(patchable_workloads(&docs).is_empty());
        assert!(restricted_workloads(&docs).is_empty());
    }
}

#[test]
fn the_restart_probe_pulls_its_image_with_the_charts_pull_secrets() {
    let operations = [restart_operation(&["deployments"], &["web"])];
    let docs = rendered_operator_chart_with_values(
        OperatorScope::Namespace,
        OperatorPermission::Remediation,
        &operations,
        &[POLICY_API],
        Some("runtime:\n  imagePullSecrets:\n    - name: regcred\n"),
    );
    for kind in ["Job", "Deployment"] {
        let probes: Vec<_> = docs
            .iter()
            .filter(|doc| {
                doc["kind"] == kind
                    && doc["metadata"]["name"]
                        .as_str()
                        .is_some_and(|name| name.ends_with("-probe"))
            })
            .collect();
        assert_eq!(probes.len(), 1, "one probe {kind}");
        assert_eq!(
            probes[0]["spec"]["template"]["spec"]["imagePullSecrets"][0]["name"], "regcred",
            "probe {kind}"
        );
    }
}

#[test]
fn restart_policy_admits_only_the_operator_account_restarting_in_its_namespace() {
    let operations = [restart_operation(&["deployments"], &["web"])];
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        let docs = rendered_operator_chart(
            scope,
            OperatorPermission::Remediation,
            &operations,
            &[POLICY_API],
        );
        let policy = docs
            .iter()
            .find(|doc| doc["kind"] == "ValidatingAdmissionPolicy")
            .expect("a restart policy");
        let spec = &policy["spec"];
        assert_eq!(spec["failurePolicy"], "Fail");
        let rule = &spec["matchConstraints"]["resourceRules"][0];
        assert_eq!(
            rule["operations"],
            serde_yaml::to_value(["UPDATE"]).unwrap()
        );
        assert_eq!(rule["apiGroups"], serde_yaml::to_value(["apps"]).unwrap());
        assert_eq!(
            rule["resources"],
            serde_yaml::to_value(["deployments"]).unwrap()
        );
        assert_eq!(
            spec["matchConditions"][0]["expression"],
            "request.userInfo.username in ['system:serviceaccount:ns1:my-saas-operator', 'system:serviceaccount:ns1:my-saas-operator-restart-c6aa0b61-probe']"
        );
        // A standalone Operator has no dynamic workloads to exempt.
        assert_eq!(spec["matchConditions"].as_sequence().unwrap().len(), 1);
        let expressions: String = spec["validations"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|validation| validation["expression"].as_str().unwrap())
            .collect();
        assert!(expressions.contains("kubectl.kubernetes.io/restartedAt"));
        assert!(expressions.contains("object.spec.template.spec == oldObject.spec.template.spec"));
        let binding = docs
            .iter()
            .find(|doc| doc["kind"] == "ValidatingAdmissionPolicyBinding")
            .unwrap();
        assert_eq!(
            binding["spec"]["validationActions"],
            serde_yaml::to_value(["Deny"]).unwrap()
        );
        let selector = &binding["spec"]["matchResources"]["namespaceSelector"]["matchLabels"];
        if scope == OperatorScope::Namespace {
            assert_eq!(selector["kubernetes.io/metadata.name"], "ns1");
        } else {
            assert!(binding["spec"]["matchResources"].is_null());
        }
        // The rule keeps its resource name limit.
        let role = docs
            .iter()
            .find(|doc| {
                (doc["kind"] == "Role" || doc["kind"] == "ClusterRole")
                    && hook_events(doc).is_empty()
                    && !grants_workload_patch(doc).is_empty()
            })
            .unwrap();
        let patch_rule = role["rules"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|rule| rule["verbs"] == serde_yaml::to_value(["patch"]).unwrap())
            .unwrap();
        assert_eq!(
            patch_rule["resourceNames"],
            serde_yaml::to_value(["web"]).unwrap()
        );
    }
}

/// The hooks that prove the policy is enforced before the Operator is bound to its patch rule.
#[test]
fn the_probe_runs_before_the_binding_and_names_what_the_policy_matches() {
    let operations = [restart_operation(&["deployments"], &["web"])];
    for scope in [OperatorScope::Namespace, OperatorScope::Cluster] {
        let docs = rendered_operator_chart(
            scope,
            OperatorPermission::Remediation,
            &operations,
            &[POLICY_API],
        );
        let hook = |kind: &str| {
            docs.iter()
                .find(|doc| {
                    doc["kind"] == kind
                        && doc["metadata"]["namespace"] == "ns1"
                        && doc["metadata"]["name"]
                            .as_str()
                            .is_some_and(|name| name.ends_with("-probe"))
                })
                .unwrap_or_else(|| panic!("no probe {kind}"))
        };
        // The check counts the API servers from their identity Leases: the
        // probe account may list Leases in kube-system, nothing else there.
        let in_kube_system = |kind: &str| {
            docs.iter()
                .find(|doc| {
                    doc["kind"] == kind
                        && doc["metadata"]["namespace"] == "kube-system"
                        && doc["metadata"]["name"]
                            .as_str()
                            .is_some_and(|name| name.ends_with("-probe"))
                })
                .unwrap_or_else(|| panic!("no kube-system probe {kind}"))
        };
        assert_eq!(
            in_kube_system("Role")["rules"],
            serde_yaml::to_value(vec![serde_yaml::to_value(serde_json::json!({
                "apiGroups": ["coordination.k8s.io"],
                "resources": ["leases"],
                "verbs": ["list"],
            }))
            .unwrap()])
            .unwrap()
        );
        let lease_binding = in_kube_system("RoleBinding");
        assert_eq!(lease_binding["subjects"][0]["namespace"], "ns1");
        assert_eq!(lease_binding["roleRef"]["kind"], "Role");
        assert_eq!(
            in_kube_system("Role")["metadata"]["annotations"]["helm.sh/hook-weight"],
            "-40"
        );
        let weight = |doc: &YamlValue| -> i32 {
            doc["metadata"]["annotations"]["helm.sh/hook-weight"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        };
        let probe = hook("ServiceAccount")["metadata"]["name"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(probe.len() <= 63, "{probe}");
        // The policy admits the probe account like the Operator's.
        let policy = docs
            .iter()
            .find(|doc| doc["kind"] == "ValidatingAdmissionPolicy")
            .unwrap();
        let policy_name = policy["metadata"]["name"].as_str().unwrap();
        assert!(policy["spec"]["matchConditions"][0]["expression"]
            .as_str()
            .unwrap()
            .contains(&format!("system:serviceaccount:ns1:{probe}")));
        // The probe account can patch one Deployment, nothing else.
        let role = hook("Role");
        assert_eq!(
            role["rules"],
            serde_yaml::to_value(vec![serde_yaml::to_value(serde_json::json!({
                "apiGroups": ["apps"],
                "resources": ["deployments"],
                "resourceNames": [probe],
                "verbs": ["patch"],
            }))
            .unwrap()])
            .unwrap()
        );
        assert_eq!(role["metadata"]["namespace"], "ns1");
        assert_eq!(hook("RoleBinding")["subjects"][0]["name"], probe.as_str());
        assert_eq!(hook("Deployment")["spec"]["replicas"], 0);
        let job = hook("Job");
        let args: Vec<_> = job["spec"]["template"]["spec"]["containers"][0]["args"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            args,
            [
                "verify-workload-patch-policy".to_owned(),
                "--namespace=ns1".to_owned(),
                format!("--deployment={probe}"),
                format!("--policy={policy_name}"),
            ]
        );
        assert_eq!(
            job["spec"]["template"]["spec"]["serviceAccountName"],
            probe.as_str()
        );
        // A failed probe fails the release (Helm waits for the Job) and leaves its log.
        assert_eq!(job["spec"]["backoffLimit"], 0);
        assert!(
            !job["metadata"]["annotations"]["helm.sh/hook-delete-policy"]
                .as_str()
                .unwrap()
                .contains("hook-failed")
        );
        // Order: account objects, Deployment, Job, then the Operator's binding (weight 0).
        assert!(weight(hook("ServiceAccount")) < weight(hook("Deployment")));
        assert!(weight(hook("Role")) < weight(hook("Deployment")));
        assert!(weight(hook("RoleBinding")) < weight(hook("Deployment")));
        assert!(weight(hook("Deployment")) < weight(job));
        assert!(weight(job) < 0);
        let operator_binding = docs
            .iter()
            .find(|doc| {
                doc["kind"]
                    .as_str()
                    .is_some_and(|k| k.ends_with("RoleBinding"))
                    && !doc["metadata"]["name"]
                        .as_str()
                        .unwrap()
                        .ends_with("-probe")
                    && hook_events(doc).iter().any(|e| e == "post-install")
            })
            .unwrap();
        assert!(operator_binding["metadata"]["annotations"]["helm.sh/hook-weight"].is_null());
        // No probe where there is no policy.
        for (permission, api_versions) in [
            (OperatorPermission::Diagnostics, &[POLICY_API][..]),
            (OperatorPermission::Remediation, &[][..]),
        ] {
            let docs = rendered_operator_chart(scope, permission, &operations, api_versions);
            assert!(
                !docs.iter().any(|doc| doc["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-probe"))),
                "{permission:?} {api_versions:?}"
            );
        }
    }
}
