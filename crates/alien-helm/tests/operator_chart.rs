//! The generic Operator chart, rendered with `helm template`.
//!
//! These tests render the real chart with Helm and read the resulting
//! documents, so a template mistake fails here and not on a cluster.

use std::{
    path::Path,
    process::{Command, Output},
};

use alien_helm::{generate_operator_chart, OperatorChartOptions};
use alien_operations_sdk::{
    KubernetesOperationPermissions, KubernetesPermissionRule, KubernetesPermissions, RiskTier,
};
use serde_yaml::Value;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TOKEN: &str = "ax_join_v1_eyJ1IjoiaHR0cHM6Ly9tLmV4YW1wbGUiLCJhIjoiYSIsImMiOiJjIn0";

fn operations() -> Vec<KubernetesOperationPermissions> {
    let rule = |resource: &str, verbs: &[&str], reason: &str| KubernetesPermissionRule {
        api_group: "".into(),
        resource: resource.into(),
        verbs: verbs.iter().map(|verb| verb.to_string()).collect(),
        resource_names: Vec::new(),
        reason: reason.into(),
    };
    vec![
        KubernetesOperationPermissions {
            plugin: "kubernetes".into(),
            operation: "get-pods".into(),
            tier: RiskTier::ReadOnly,
            permissions: KubernetesPermissions {
                schema_version: 1,
                rules: vec![rule("pods", &["get", "list"], "list pods")],
            },
        },
        KubernetesOperationPermissions {
            plugin: "kubernetes".into(),
            operation: "restart".into(),
            tier: RiskTier::Mutating,
            permissions: KubernetesPermissions {
                schema_version: 1,
                rules: vec![rule("pods", &["get", "delete"], "restart a pod")],
            },
        },
    ]
}

fn write_chart(directory: &Path) {
    let operations = operations();
    let chart = generate_operator_chart(OperatorChartOptions {
        chart_version: "1.2.3",
        app_version: "1.2.3",
        image_repository: "registry.example/operator",
        image_digest: DIGEST,
        operations: &operations,
    })
    .unwrap();
    for (path, content) in &chart.files {
        let target = directory.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
    }
}

fn helm(directory: &Path, arguments: &[&str]) -> Output {
    Command::new("helm")
        .arg("template")
        .arg("alien")
        .arg(directory)
        .args(["--namespace", "shop"])
        .args(arguments)
        .output()
        .expect("helm must be installed to run the chart tests")
}

fn render(arguments: &[&str]) -> String {
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path());
    let output = helm(directory.path(), arguments);
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn refusal(arguments: &[&str]) -> String {
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path());
    let output = helm(directory.path(), arguments);
    assert!(
        !output.status.success(),
        "helm unexpectedly accepted {arguments:?}"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn documents(rendered: &str) -> Vec<Value> {
    rendered
        .split("\n---")
        .filter_map(|part| serde_yaml::from_str::<Value>(part).ok())
        .filter(|value| value.get("kind").is_some())
        .collect()
}

/// The rendered documents as parsed YAML, sorted by kind and name. A snapshot of this does not
/// depend on the Helm version's text formatting or document order.
fn canonical(rendered: &str) -> String {
    let mut documents = documents(rendered);
    documents.sort_by_key(|document| {
        (
            document["kind"].as_str().unwrap_or_default().to_string(),
            document["metadata"]["name"].as_str().unwrap_or_default().to_string(),
        )
    });
    documents
        .iter()
        .map(|document| serde_yaml::to_string(document).unwrap())
        .collect::<Vec<_>>()
        .join("---\n")
}

fn kind<'a>(documents: &'a [Value], kind: &str) -> Vec<&'a Value> {
    documents
        .iter()
        .filter(|document| document["kind"].as_str() == Some(kind))
        .collect()
}

fn rules(document: &Value) -> Vec<(String, String, Vec<String>, Vec<String>)> {
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_sequence()
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    document["rules"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|rule| {
            (
                strings(&rule["apiGroups"]).join(","),
                strings(&rule["resources"]).join(","),
                strings(&rule["verbs"]),
                strings(&rule["resourceNames"]),
            )
        })
        .collect()
}

fn rule(group: &str, resource: &str, verbs: &[&str]) -> (String, String, Vec<String>, Vec<String>) {
    (
        group.into(),
        resource.into(),
        verbs.iter().map(|verb| verb.to_string()).collect(),
        Vec::new(),
    )
}

#[test]
fn the_default_install_has_no_volume_claim_no_hook_and_no_custom_resource_definition() {
    let rendered = render(&["--set", &format!("token={TOKEN}")]);
    insta::assert_snapshot!("default_chart", canonical(&rendered));
    let documents = documents(&rendered);
    let mut kinds: Vec<_> = documents
        .iter()
        .map(|document| document["kind"].as_str().unwrap())
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [
            "ClusterRole",
            "ClusterRoleBinding",
            "Deployment",
            "Role",
            "RoleBinding",
            "Secret",
            "Secret",
            "ServiceAccount"
        ]
    );
    assert!(!rendered.contains("PersistentVolumeClaim"));
    assert!(!rendered.contains("persistentVolumeClaim"));
    assert!(!rendered.contains("helm.sh/hook"));
    assert!(!rendered.contains("CustomResourceDefinition"));
    assert!(!rendered.contains("accessrequests"));

    let deployment = kind(&documents, "Deployment")[0];
    assert_eq!(deployment["spec"]["replicas"].as_i64(), Some(1));
    assert_eq!(
        deployment["spec"]["strategy"]["type"].as_str(),
        Some("Recreate")
    );
    let volumes = deployment["spec"]["template"]["spec"]["volumes"]
        .as_sequence()
        .unwrap();
    assert!(volumes
        .iter()
        .all(|volume| volume.get("persistentVolumeClaim").is_none()));
    assert!(volumes
        .iter()
        .any(|volume| volume["name"].as_str() == Some("data") && volume.get("emptyDir").is_some()));
}

#[test]
fn the_identity_secret_is_created_empty_and_the_enrollment_secret_holds_the_token() {
    let documents = documents(&render(&["--set", &format!("token={TOKEN}")]));
    let secrets = kind(&documents, "Secret");
    let identity = secrets
        .iter()
        .find(|secret| secret["metadata"]["name"].as_str() == Some("alien-identity"))
        .unwrap();
    assert!(identity.get("data").is_none() && identity.get("stringData").is_none());
    let enrollment = secrets
        .iter()
        .find(|secret| secret["metadata"]["name"].as_str() == Some("alien-enrollment"))
        .unwrap();
    assert_eq!(enrollment["stringData"]["token"].as_str(), Some(TOKEN));
}

#[test]
fn the_cluster_role_is_exactly_the_runtime_reads_log_access_and_read_only_operations() {
    let documents = documents(&render(&["--set", &format!("token={TOKEN}")]));
    let role = kind(&documents, "ClusterRole")[0];
    let mut actual = rules(role);
    actual.sort();
    let mut expected = vec![
        rule("apps", "daemonsets", &["list"]),
        rule("apps", "deployments", &["list"]),
        rule("apps", "statefulsets", &["list"]),
        rule("", "events", &["list"]),
        rule("", "pods", &["get", "list"]),
        rule("", "pods/log", &["get"]),
        rule("metrics.k8s.io", "pods", &["list"]),
    ];
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn remediation_adds_the_write_verbs_of_operations_and_nothing_else() {
    let documents = documents(&render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "permission=remediation",
    ]));
    let actual = rules(kind(&documents, "ClusterRole")[0]);
    assert!(actual.contains(&rule("", "pods", &["get", "list"])));
    // The mutating operation's rule arrives whole, with its read verb, only here.
    assert!(actual.contains(&rule("", "pods", &["delete", "get"])));
    assert_eq!(actual.len(), 8);
}

#[test]
fn the_only_secret_access_is_the_identity_secret_in_the_release_namespace() {
    let documents = documents(&render(&["--set", &format!("token={TOKEN}")]));
    let role = kind(&documents, "Role")[0];
    assert_eq!(role["metadata"]["namespace"].as_str(), Some("shop"));
    assert_eq!(
        rules(role),
        vec![(
            "".to_string(),
            "secrets".to_string(),
            vec!["get".into(), "patch".into(), "update".into()],
            vec!["alien-identity".to_string()],
        )]
    );
    let cluster_role = rules(kind(&documents, "ClusterRole")[0]);
    assert!(cluster_role.iter().all(|(_, resource, verbs, _)| {
        !resource.contains("secrets")
            && !resource.contains("exec")
            && !verbs
                .iter()
                .any(|verb| ["create", "watch", "*"].contains(&verb.as_str()))
    }));
}

#[test]
fn access_requests_are_off_by_default_and_on_they_bring_the_kind_and_its_rules() {
    let off = render(&["--set", &format!("token={TOKEN}")]);
    assert!(off.contains("name: OPERATOR_ACCESS_REQUESTS\n              value: \"false\""));
    let on = render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "accessRequests.enabled=true",
    ]);
    let documents = documents(&on);
    let crd = kind(&documents, "CustomResourceDefinition");
    assert_eq!(crd.len(), 1);
    assert_eq!(
        crd[0]["metadata"]["annotations"]["helm.sh/resource-policy"].as_str(),
        Some("keep")
    );
    let actual = rules(kind(&documents, "ClusterRole")[0]);
    assert_eq!(actual.len(), 9);
    assert!(on.contains("name: OPERATOR_ACCESS_REQUESTS\n              value: \"true\""));
}

#[test]
fn install_takes_a_token_or_an_existing_secret_never_both_and_upgrade_needs_neither() {
    assert!(refusal(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "existingSecret=mine"
    ])
    .contains("not both"));

    // Without either, the chart still renders (it does not depend on whether this is an
    // install, which `helm template` and GitOps tools cannot tell): there is just no
    // enrollment Secret, and the Operator uses its saved identity or says to issue a new token.
    let bare = documents(&render(&[]));
    assert_eq!(kind(&bare, "Secret").len(), 1);

    let upgrade = render(&["--is-upgrade"]);
    let documents = documents(&upgrade);
    assert_eq!(
        kind(&documents, "Secret").len(),
        1,
        "only the identity Secret remains"
    );
    assert_eq!(
        kind(&documents, "Secret")[0]["metadata"]["name"].as_str(),
        Some("alien-identity")
    );

    let existing = render(&["--set", "existingSecret=mine"]);
    let documents = documents_of(&existing);
    let deployment = kind(&documents, "Deployment")[0];
    let volumes = deployment["spec"]["template"]["spec"]["volumes"]
        .as_sequence()
        .unwrap();
    assert_eq!(volumes[0]["secret"]["secretName"].as_str(), Some("mine"));
    assert_eq!(kind(&documents, "Secret").len(), 1);
}

fn documents_of(rendered: &str) -> Vec<Value> {
    documents(rendered)
}

#[test]
fn logs_settings_reach_the_operator_and_an_invalid_permission_is_refused() {
    let rendered = render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "logs.namespaces={a,b}",
        "--set",
        "logs.podSelector=app=web",
        "--set",
        "logs.maxStreams=50",
    ]);
    for expected in [
        "OPERATOR_POD_LOG_SCOPE\n              value: \"cluster\"",
        "OPERATOR_POD_LOG_NAMESPACES\n              value: \"a,b\"",
        "OPERATOR_POD_LOG_POD_SELECTOR\n              value: \"app=web\"",
        "OPERATOR_POD_LOG_MAX_STREAMS\n              value: \"50\"",
    ] {
        assert!(rendered.contains(expected), "missing {expected}");
    }
    // The system namespaces are left out by the Operator itself; nothing is set by default.
    assert!(!rendered.contains("OPERATOR_POD_LOG_EXCLUDE_NAMESPACES"));
    assert!(!rendered.contains("INCLUDE_SYSTEM_NAMESPACES"));
    let custom = render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "logs.excludeNamespaces={monitoring}",
        "--set",
        "logs.includeSystemNamespaces=true",
    ]);
    assert!(
        custom.contains("OPERATOR_POD_LOG_EXCLUDE_NAMESPACES\n              value: \"monitoring\"")
    );
    assert!(custom
        .contains("OPERATOR_POD_LOG_INCLUDE_SYSTEM_NAMESPACES\n              value: \"true\""));
    assert!(refusal(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "permission=root"
    ])
    .contains("permission"));
}

#[test]
fn the_chart_passes_helm_lint() {
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path());
    let output = Command::new("helm")
        .arg("lint")
        .arg(directory.path())
        .args(["--set", &format!("token={TOKEN}")])
        .output()
        .expect("helm must be installed to run the chart tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn the_intake_is_off_by_default_and_the_queue_is_capped() {
    let default = render(&["--set", &format!("token={TOKEN}")]);
    let documents_default = documents(&default);
    assert!(kind(&documents_default, "Service").is_empty());
    assert!(!default.contains("OTLP_PORT"));
    assert!(default.contains("OPERATOR_OTLP_MAX_QUEUED_BYTES\n              value: \"268435456\""));
    let on = documents(&render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "otlp.enabled=true",
    ]));
    assert_eq!(kind(&on, "Service").len(), 1);
    let service = kind(&on, "Service")[0];
    assert_eq!(service["spec"]["ports"][0]["port"].as_i64(), Some(4318));
    assert_eq!(service["spec"]["type"].as_str(), Some("ClusterIP"));
}

#[test]
fn scheduling_pull_secrets_and_extra_environment_reach_the_pod_and_global_is_allowed() {
    let rendered = render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "nodeSelector.pool=ops",
        "--set",
        "tolerations[0].key=dedicated",
        "--set",
        "tolerations[0].operator=Exists",
        "--set",
        "imagePullSecrets[0].name=regcred",
        "--set",
        "extraEnv[0].name=HTTPS_PROXY",
        "--set",
        "extraEnv[0].value=http://proxy:3128",
        "--set",
        "global.anything=1",
    ]);
    let documents = documents(&rendered);
    let spec = &kind(&documents, "Deployment")[0]["spec"]["template"]["spec"];
    assert_eq!(spec["nodeSelector"]["pool"].as_str(), Some("ops"));
    assert_eq!(spec["tolerations"][0]["key"].as_str(), Some("dedicated"));
    assert_eq!(
        spec["imagePullSecrets"][0]["name"].as_str(),
        Some("regcred")
    );
    let env = spec["containers"][0]["env"].as_sequence().unwrap();
    assert!(env
        .iter()
        .any(|entry| entry["name"].as_str() == Some("HTTPS_PROXY")
            && entry["value"].as_str() == Some("http://proxy:3128")));
    // The values are checked: an unknown one is refused.
    assert!(refusal(&["--set", &format!("token={TOKEN}"), "--set", "bogus=1"]).contains("bogus"));
}

#[test]
fn a_new_token_on_upgrade_restarts_the_pod_so_it_is_used() {
    let annotation = |arguments: &[&str]| {
        let documents = documents(&render(arguments));
        kind(&documents, "Deployment")[0]["spec"]["template"]["metadata"]["annotations"]
            ["checksum/enrollment"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let first = annotation(&["--set", &format!("token={TOKEN}")]);
    let second = annotation(&["--set", &format!("token={TOKEN}x")]);
    let none = annotation(&[]);
    assert_ne!(first, second);
    assert_ne!(first, none);
}

#[test]
fn operations_that_could_read_secrets_or_exec_are_refused_at_generation() {
    let bad = |resource: &str, verbs: &[&str]| {
        let mut operations = operations();
        operations[0].permissions.rules[0].resource = resource.into();
        operations[0].permissions.rules[0].verbs = verbs.iter().map(|v| v.to_string()).collect();
        generate_operator_chart(OperatorChartOptions {
            chart_version: "1.0.0",
            app_version: "1.0.0",
            image_repository: "registry.example/operator",
            image_digest: DIGEST,
            operations: &operations,
        })
    };
    assert!(bad("secrets", &["get"]).is_err());
    assert!(bad("pods/exec", &["create"]).is_err());
    assert!(bad("*", &["get"]).is_err());
    assert!(
        bad("pods", &["delete"]).is_err(),
        "a read-only operation cannot delete"
    );
    assert!(bad("pods", &["get"]).is_ok());
}

fn render_release(release: &str, arguments: &[&str]) -> String {
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path());
    let output = Command::new("helm")
        .args(["template", release])
        .arg(directory.path())
        .args(["--namespace", "shop"])
        .args(arguments)
        .output()
        .expect("helm must be installed to run the chart tests");
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const BRAND: [&str; 10] = [
    "--set",
    "brand.name=acme-operator",
    "--set",
    "brand.brand=acme",
    "--set",
    "brand.displayName=Acme Operator",
    "--set",
    "brand.envPrefix=ACME",
    "--set",
    "brand.labelDomain=acme.dev",
];

#[test]
fn a_branded_render_has_no_product_name_in_any_resource_name_label_or_group() {
    let mut arguments = vec!["--set".to_string(), format!("token={TOKEN}")];
    arguments.extend(BRAND.iter().map(|argument| argument.to_string()));
    arguments.extend([
        "--set".to_string(),
        "accessRequests.enabled=true".to_string(),
    ]);
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let rendered = render_release("acme", &arguments);
    assert!(
        !rendered.to_lowercase().contains("alien"),
        "a branded render must not name the product:\n{}",
        rendered
            .lines()
            .filter(|line| line.to_lowercase().contains("alien"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    insta::assert_snapshot!("branded_chart", canonical(&rendered));

    // The packaged configuration reaches the Operator through the file it already reads.
    let documents = documents(&rendered);
    let config = kind(&documents, "ConfigMap")[0];
    let json: serde_json::Value =
        serde_json::from_str(config["data"]["operator-config.json"].as_str().unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "syncIntervalSecs": 30, "name": "acme-operator", "brand": "acme",
            "displayName": "Acme Operator", "envPrefix": "ACME", "labelDomain": "acme.dev"
        })
    );
    let deployment = kind(&documents, "Deployment")[0];
    let container = &deployment["spec"]["template"]["spec"]["containers"][0];
    assert!(container["env"].as_sequence().unwrap().iter().any(|entry| {
        entry["name"].as_str() == Some("OPERATOR_CONFIG_FILE")
            && entry["value"].as_str() == Some("/etc/operator/config/operator-config.json")
    }));
    assert_eq!(
        deployment["metadata"]["labels"]["app.kubernetes.io/name"].as_str(),
        Some("acme-operator")
    );
}

#[test]
fn the_approval_kind_is_named_the_way_the_operator_names_it() {
    for domain in ["acme.dev", "My Cool App", "my-startup.io", "Globex Corp"] {
        let rendered = render_release(
            "acme",
            &[
                "--set",
                &format!("token={TOKEN}"),
                "--set",
                &format!("brand.labelDomain={domain}"),
                "--set",
                "accessRequests.enabled=true",
            ],
        );
        let expected = alien_core::access_request_crd::access_request_crd_names(Some(domain));
        let documents = documents(&rendered);
        let crd = kind(&documents, "CustomResourceDefinition")[0];
        assert_eq!(
            crd["metadata"]["name"].as_str(),
            Some(expected.crd_name.as_str()),
            "{domain}"
        );
        assert_eq!(
            crd["spec"]["group"].as_str(),
            Some(expected.group.as_str()),
            "{domain}"
        );
        assert_eq!(
            crd["spec"]["names"]["kind"].as_str(),
            Some(expected.kind.as_str()),
            "{domain}"
        );
        assert_eq!(
            crd["spec"]["names"]["plural"].as_str(),
            Some(expected.plural.as_str()),
            "{domain}"
        );
        assert_eq!(
            crd["spec"]["names"]["singular"].as_str(),
            Some(expected.singular.as_str()),
            "{domain}"
        );
        assert_eq!(
            crd["spec"]["names"]["shortNames"][0].as_str(),
            Some(expected.short_name.as_str()),
            "{domain}"
        );
        let role_rules = rules(kind(&documents, "ClusterRole")[0]);
        assert!(
            role_rules.contains(&rule(
                &expected.group,
                &expected.plural,
                &["get", "list", "watch", "create", "update", "patch"]
            )),
            "{domain}"
        );
    }
    // Unbranded keeps the default names.
    let default = render_release(
        "acme",
        &[
            "--set",
            &format!("token={TOKEN}"),
            "--set",
            "accessRequests.enabled=true",
        ],
    );
    assert!(default.contains("group: accessrequests.alien"));
}

#[test]
fn the_chart_works_as_a_dependency_under_an_alias_and_stays_unbranded_without_brand_values() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(parent.path().join("charts")).unwrap();
    write_chart(&parent.path().join("charts/operator"));
    std::fs::write(
        parent.path().join("Chart.yaml"),
        "apiVersion: v2\nname: acme-suite\nversion: 1.0.0\ndependencies:\n  - name: operator\n    alias: acme-operator\n    version: 1.2.3\n",
    )
    .unwrap();
    std::fs::write(
        parent.path().join("values.yaml"),
        format!(
            "global: {{team: acme}}\nacme-operator:\n  token: {TOKEN}\n  brand:\n    name: acme-operator\n    brand: acme\n    displayName: Acme Operator\n    labelDomain: acme.dev\n"
        ),
    )
    .unwrap();
    let output = Command::new("helm")
        .args(["template", "acme"])
        .arg(parent.path())
        .args(["--namespace", "shop"])
        .output()
        .expect("helm must be installed to run the chart tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert!(!rendered.to_lowercase().contains("alien"), "{rendered}");
    let documents = documents(&rendered);
    assert_eq!(kind(&documents, "Deployment").len(), 1);
    assert_eq!(kind(&documents, "ConfigMap").len(), 1);
}

/// A parent chart that names its own objects after the release, as many do.
/// Installed as a subchart, the Operator's objects must not take those names.
#[test]
fn as_a_subchart_no_object_takes_the_name_of_a_parent_object() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(
        parent.path().join("Chart.yaml"),
        "apiVersion: v2\nname: shop\nversion: 0.1.0\n",
    )
    .unwrap();
    std::fs::create_dir_all(parent.path().join("templates")).unwrap();
    std::fs::write(
        parent.path().join("templates/app.yaml"),
        r#"apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ .Release.Name }}
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ .Release.Name }}
spec:
  selector:
    matchLabels: {app: shop}
  template:
    metadata:
      labels: {app: shop}
    spec:
      containers: [{name: app, image: shop}]
---
apiVersion: v1
kind: Secret
metadata:
  name: {{ .Release.Name }}-identity
---
apiVersion: v1
kind: Service
metadata:
  name: {{ .Release.Name }}
spec:
  ports: [{port: 80}]
"#,
    )
    .unwrap();
    write_chart(&parent.path().join("charts/operator"));
    let output = helm(
        parent.path(),
        &["--set", &format!("operator.token={TOKEN}")],
    );
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    let all = documents(&rendered);
    let mut seen = std::collections::BTreeSet::new();
    for document in &all {
        let key = (
            document["kind"].as_str().unwrap().to_string(),
            document["metadata"]["name"].as_str().unwrap().to_string(),
        );
        assert!(seen.insert(key.clone()), "two objects are named {key:?}");
    }
    // The Operator's objects all carry the chart's name; the parent's stay as they were.
    let deployments = kind(&all, "Deployment");
    let names: Vec<_> = deployments
        .iter()
        .map(|d| d["metadata"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alien") && names.contains(&"alien-operator"), "{names:?}");
}

/// Installed on its own the names are the release name, as before.
#[test]
fn installed_on_its_own_the_names_are_the_release_name() {
    let rendered = render(&["--set", &format!("token={TOKEN}")]);
    let all = documents(&rendered);
    let deployment = kind(&all, "Deployment");
    assert_eq!(deployment[0]["metadata"]["name"].as_str(), Some("alien"));
    let secrets: Vec<_> = kind(&all, "Secret")
        .iter()
        .map(|s| s["metadata"]["name"].as_str().unwrap().to_string())
        .collect();
    assert!(secrets.contains(&"alien-identity".to_string()) && secrets.contains(&"alien-enrollment".to_string()));
}

#[test]
fn a_branded_chart_with_approvals_needs_a_label_domain() {
    let message = refusal(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "brand.name=acme-operator",
        "--set",
        "accessRequests.enabled=true",
    ]);
    assert!(message.contains("brand.labelDomain is required"), "{message}");
    // With the domain it renders, and without approvals no domain is needed.
    render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "brand.name=acme-operator",
        "--set",
        "accessRequests.enabled=true",
        "--set",
        "brand.labelDomain=acme.dev",
    ]);
    render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        "brand.name=acme-operator",
    ]);
}

#[test]
fn the_brand_name_is_a_short_slug_that_starts_with_a_letter() {
    for bad in ["1acme", "-acme", "acme-", "Acme", &"a".repeat(49)] {
        let message = refusal(&[
            "--set",
            &format!("token={TOKEN}"),
            "--set",
            &format!("brand.name={bad}"),
        ]);
        assert!(message.contains("brand"), "{bad}: {message}");
    }
    render(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set",
        &format!("brand.name=a{}", "b".repeat(47)),
    ]);
}

#[test]
fn the_name_label_is_quoted_so_a_numeric_looking_brand_stays_a_string() {
    let rendered = render(&["--set", &format!("token={TOKEN}")]);
    assert!(rendered.contains("app.kubernetes.io/name: \"operator\""), "{rendered}");
}

/// Two copies of the chart under different aliases of one parent release, both
/// without a brand: each Service must reach only its own Operator.
#[test]
fn two_aliased_copies_do_not_share_pod_labels() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(parent.path().join("charts")).unwrap();
    write_chart(&parent.path().join("charts/operator"));
    std::fs::write(
        parent.path().join("Chart.yaml"),
        "apiVersion: v2\nname: suite\nversion: 1.0.0\ndependencies:\n  - name: operator\n    alias: east\n    version: 1.2.3\n  - name: operator\n    alias: west\n    version: 1.2.3\n",
    )
    .unwrap();
    std::fs::write(
        parent.path().join("values.yaml"),
        format!(
            "east:\n  token: {TOKEN}\n  otlp: {{enabled: true}}\nwest:\n  token: {TOKEN}\n  otlp: {{enabled: true}}\n"
        ),
    )
    .unwrap();
    let output = helm(parent.path(), &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let all = documents(&String::from_utf8(output.stdout).unwrap());
    let selectors: Vec<_> = kind(&all, "Service")
        .iter()
        .map(|service| format!("{:?}", service["spec"]["selector"]))
        .collect();
    assert_eq!(selectors.len(), 2);
    assert_ne!(selectors[0], selectors[1], "both Services select both Operators");
    let pod_labels: Vec<_> = kind(&all, "Deployment")
        .iter()
        .map(|d| format!("{:?}", d["spec"]["template"]["metadata"]["labels"]))
        .collect();
    assert_ne!(pod_labels[0], pod_labels[1]);
}

/// The same long release name in two namespaces must give two cluster-wide
/// names, and short names stay as they were.
#[test]
fn long_release_names_in_different_namespaces_keep_separate_cluster_role_names() {
    let release = "r".repeat(53);
    let role_name = |namespace: &str| {
        let directory = tempfile::tempdir().unwrap();
        write_chart(directory.path());
        let output = Command::new("helm")
            .args(["template", &release])
            .arg(directory.path())
            .args(["--namespace", namespace, "--set", &format!("token={TOKEN}")])
            .output()
            .expect("helm must be installed to run the chart tests");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let all = documents(&String::from_utf8(output.stdout).unwrap());
        let name = kind(&all, "ClusterRole")[0]["metadata"]["name"]
            .as_str()
            .unwrap()
            .to_string();
        let binding = kind(&all, "ClusterRoleBinding")[0]["roleRef"]["name"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(name, binding);
        name
    };
    let (east, west) = (role_name("production-east"), role_name("production-west"));
    assert_ne!(east, west);
    assert!(east.len() <= 63 && west.len() <= 63);
    let all = documents(&render(&["--set", &format!("token={TOKEN}")]));
    assert_eq!(
        kind(&all, "ClusterRole")[0]["metadata"]["name"].as_str(),
        Some("alien-shop")
    );
}

#[test]
fn a_second_release_can_leave_out_the_shared_approval_definition() {
    let arguments = |extra: &[&'static str]| {
        let mut all = vec!["--set".to_string(), format!("token={TOKEN}")];
        all.extend(["--set".into(), "accessRequests.enabled=true".into()]);
        all.extend(extra.iter().map(|s| s.to_string()));
        all
    };
    let render_with = |extra: &[&'static str]| {
        let arguments = arguments(extra);
        let refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
        render(&refs)
    };
    let first = documents(&render_with(&[]));
    assert_eq!(kind(&first, "CustomResourceDefinition").len(), 1);
    let second = render_with(&["--set", "accessRequests.installDefinition=false"]);
    let second_docs = documents(&second);
    assert!(kind(&second_docs, "CustomResourceDefinition").is_empty());
    // The Operator still runs the approval loop and still has the rules for it.
    assert!(second.contains("name: OPERATOR_ACCESS_REQUESTS\n              value: \"true\""));
    assert_eq!(rules(kind(&second_docs, "ClusterRole")[0]).len(), 9);
}

#[test]
fn the_documented_operations_example_reaches_the_operator_and_a_wrong_shape_is_refused() {
    // The example in the values file comment, as a values file.
    let directory = tempfile::tempdir().unwrap();
    let values = directory.path().join("values.yaml");
    std::fs::write(
        &values,
        "operations:\n  plugins:\n    kubernetes:\n      approval:\n        get-pods: auto\n",
    )
    .unwrap();
    let rendered = render(&[
        "--set",
        &format!("token={TOKEN}"),
        "-f",
        values.to_str().unwrap(),
    ]);
    let deployment = documents(&rendered)
        .into_iter()
        .find(|document| document["kind"] == "Deployment")
        .unwrap();
    let env = deployment["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .unwrap();
    let value = env
        .iter()
        .find(|entry| entry["name"] == "OPERATOR_OPERATIONS")
        .and_then(|entry| entry["value"].as_str())
        .expect("OPERATOR_OPERATIONS is set");
    let parsed: serde_json::Value = serde_json::from_str(value).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({"plugins": {"kubernetes": {"approval": {"get-pods": "auto"}}}})
    );
    // The permissions do not depend on `operations`.
    let without = render(&["--set", &format!("token={TOKEN}")]);
    let rules = |rendered: &str| {
        documents(rendered)
            .into_iter()
            .find(|document| document["kind"] == "ClusterRole")
            .unwrap()["rules"]
            .clone()
    };
    assert_eq!(rules(&rendered), rules(&without));

    // A plugin that is not an object is refused at install, not at runtime.
    let error = refusal(&[
        "--set",
        &format!("token={TOKEN}"),
        "--set-json",
        "operations={\"plugins\":{\"kubernetes\":\"auto\"}}",
    ]);
    assert!(error.contains("operations"), "{error}");
}
