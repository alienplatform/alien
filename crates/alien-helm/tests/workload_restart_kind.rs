//! The Operator's `patch` on workloads is limited to a restart by the cluster itself: a
//! ValidatingAdmissionPolicy shipped with the rule. Kubernetes RBAC alone would let a patch change
//! a pod template's image, environment, service account or volumes.
//!
//! This installs the rendered chart on a real cluster (Kubernetes 1.30 or later) and patches as
//! the Operator's ServiceAccount: a restart passes, every other change is refused with the
//! policy's message, and another account is not affected. It also checks that the policy compares
//! every field the cluster's API schema defines, so a field added by a newer Kubernetes fails here
//! instead of going unchecked.
//!
//! Run against a disposable cluster:
//! `ALIEN_WORKLOAD_RESTART_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test workload_restart_kind -- --ignored --nocapture`

use alien_helm::{
    generate_operator_manifest, OperatorManifestOptions, OperatorOutputFormat, OperatorPermission,
    OperatorScope,
};
use alien_operations_sdk::{
    KubernetesOperationPermissions, KubernetesPermissionRule, KubernetesPermissions, RiskTier,
};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

const RELEASE: &str = "restart-policy";
const NAMESPACE: &str = "alien-restart-policy";
const OPERATOR: &str = "system:serviceaccount:alien-restart-policy:my-saas-operator";
const OTHER: &str = "system:serviceaccount:alien-restart-policy:other";
const RESTART: &str = r#"{"spec":{"template":{"metadata":{"annotations":{"kubectl.kubernetes.io/restartedAt":"2026-01-01T00:00:00Z"}}}}}"#;

struct Cleanup<'a>(&'a str);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = Command::new("helm")
            .args(["--kube-context", self.0, "uninstall", RELEASE])
            .args(["--namespace", NAMESPACE, "--ignore-not-found", "--no-hooks"])
            .output();
        let _ = Command::new("kubectl")
            .args(["--context", self.0, "delete", "namespace", NAMESPACE])
            .args(["--ignore-not-found", "--wait=false"])
            .output();
    }
}

fn run(program: &str, arguments: &[&str], stdin: Option<&str>) -> (bool, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("run {program} {arguments:?}: {error}"));
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("$ {program} {}\n{}", arguments.join(" "), text.trim_end());
    (output.status.success(), text)
}

fn kubectl(context: &str, arguments: &[&str], stdin: Option<&str>) -> (bool, String) {
    let mut all = vec!["--context", context, "--namespace", NAMESPACE];
    all.extend(arguments);
    run("kubectl", &all, stdin)
}

fn operation() -> KubernetesOperationPermissions {
    KubernetesOperationPermissions {
        plugin: "restarter".into(),
        operation: "restart".into(),
        tier: RiskTier::Mutating,
        permissions: KubernetesPermissions {
            schema_version: 1,
            rules: ["deployments", "statefulsets", "daemonsets"]
                .iter()
                .map(|kind| KubernetesPermissionRule {
                    api_group: "apps".into(),
                    resource: (*kind).into(),
                    verbs: vec!["patch".into()],
                    resource_names: vec![],
                    reason: "Restart workloads".into(),
                })
                .collect(),
        },
    }
}

fn write_chart(directory: &Path) {
    let template = generate_operator_manifest(OperatorManifestOptions {
        custom_operation_permissions: &[operation()],
        manager_url: "https://manager.example.com",
        group_token: "test-token",
        encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
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
    .unwrap();
    fs::create_dir_all(directory.join("templates")).unwrap();
    fs::write(
        directory.join("Chart.yaml"),
        "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n",
    )
    .unwrap();
    fs::write(
        directory.join("values.yaml"),
        "remoteOperator:\n  serviceAccountAnnotations: {}\nalien:\n  version: test\n",
    )
    .unwrap();
    fs::write(directory.join("templates/operator.yaml"), template).unwrap();
}

const WORKLOADS: &str = r#"apiVersion: apps/v1
kind: Deployment
metadata: {name: web}
spec:
  replicas: 0
  selector: {matchLabels: {app: web}}
  template:
    metadata: {labels: {app: web}}
    spec:
      containers: [{name: main, image: "registry.k8s.io/pause:3.10"}]
---
apiVersion: apps/v1
kind: StatefulSet
metadata: {name: db}
spec:
  replicas: 0
  serviceName: db
  selector: {matchLabels: {app: db}}
  template:
    metadata: {labels: {app: db}}
    spec:
      containers: [{name: main, image: "registry.k8s.io/pause:3.10"}]
---
apiVersion: apps/v1
kind: DaemonSet
metadata: {name: agent}
spec:
  selector: {matchLabels: {app: agent}}
  template:
    metadata: {labels: {app: agent}}
    spec:
      nodeSelector: {does-not-exist: "true"}
      containers: [{name: main, image: "registry.k8s.io/pause:3.10"}]
---
apiVersion: v1
kind: ServiceAccount
metadata: {name: other}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata: {name: other-patch}
rules:
  - {apiGroups: [apps], resources: [deployments], verbs: [get, patch]}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata: {name: other-patch}
subjects: [{kind: ServiceAccount, name: other}]
roleRef: {apiGroup: rbac.authorization.k8s.io, kind: Role, name: other-patch}
"#;

/// Send one PATCH to the API server as `user`, the way the Operator does: no read first (the
/// Operator may not `get`). `kubectl patch` reads the object first, so this goes through
/// `kubectl proxy`, which carries the impersonation. Returns (accepted, response body).
fn api_patch(context: &str, user: &str, path: &str, body: &str) -> (bool, String) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let as_user = format!("--as={user}");
    let mut proxy = Command::new("kubectl")
        .args(["--context", context, &as_user, "proxy", "--port=0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("start kubectl proxy");
    let mut line = String::new();
    BufReader::new(proxy.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let address = line.trim().rsplit(' ').next().unwrap().to_owned();
    let url = format!("http://{address}{path}");
    let output = Command::new("curl")
        .args(["-s", "-w", "\n%{http_code}", "-X", "PATCH", "-H"])
        .args(["Content-Type: application/strategic-merge-patch+json", "-d", body, &url])
        .output()
        .expect("run curl");
    let _ = proxy.kill();
    let _ = proxy.wait();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let (response, status) = text.rsplit_once('\n').unwrap();
    let message = serde_json::from_str::<serde_json::Value>(response)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "(patched)".to_owned());
    println!("PATCH {path} as {user}\n  {body}\n  -> HTTP {status}: {message}");
    (status == "200", message)
}

fn patch(context: &str, user: &str, kind: &str, name: &str, body: &str) -> (bool, String) {
    let resource = match kind {
        "deployment" => "deployments",
        "statefulset" => "statefulsets",
        _ => "daemonsets",
    };
    api_patch(
        context,
        user,
        &format!("/apis/apps/v1/namespaces/{NAMESPACE}/{resource}/{name}"),
        body,
    )
}

fn schema_properties(schema: &serde_json::Value, name: &str) -> BTreeSet<String> {
    schema["components"]["schemas"][name]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("no schema {name}"))
        .keys()
        .cloned()
        .collect()
}

#[test]
#[ignore = "requires a disposable cluster (Kubernetes 1.30 or later) named by ALIEN_WORKLOAD_RESTART_KIND_CONTEXT"]
fn the_cluster_admits_only_a_restart_from_the_operator() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path());

    let rendered = run(
        "helm",
        &[
            "template", RELEASE, directory.path().to_str().unwrap(), "--namespace", NAMESPACE,
            "--api-versions", "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy",
        ],
        None,
    );
    assert!(rendered.0);
    let rendered = rendered.1;
    let (ok, _) = run(
        "helm",
        &[
            "--kube-context", &context, "install", RELEASE,
            directory.path().to_str().unwrap(), "--namespace", NAMESPACE, "--create-namespace",
        ],
        None,
    );
    assert!(ok, "the chart installs on this cluster");
    let (ok, _) = kubectl(&context, &["apply", "-f", "-"], Some(WORKLOADS));
    assert!(ok);

    // The cluster advertises the policy API, so the chart rendered the rule itself.
    let (_, can) = kubectl(
        &context,
        &["auth", "can-i", "patch", "deployments.apps", &format!("--as={OPERATOR}")],
        None,
    );
    assert_eq!(can.trim(), "yes");
    let (_, can) = kubectl(
        &context,
        &["auth", "can-i", "patch", "deployments.apps", "--subresource=status", &format!("--as={OPERATOR}")],
        None,
    );
    assert_eq!(can.trim(), "no", "the status subresource is not granted");
    // The binding takes a moment to start enforcing.
    std::thread::sleep(std::time::Duration::from_secs(5));

    // A restart passes, for each kind, and repeats.
    for (kind, name) in [("deployment", "web"), ("statefulset", "db"), ("daemonset", "agent")] {
        let (ok, text) = patch(&context, OPERATOR, kind, name, RESTART);
        assert!(ok, "restart of {kind}: {text}");
    }
    let again = RESTART.replace("2026-01-01", "2026-02-01");
    assert!(patch(&context, OPERATOR, "deployment", "web", &again).0);

    // Every other change is refused by the policy.
    let refused = [
        ("deployment", "web", "image", r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#),
        ("deployment", "web", "environment", r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","env":[{"name":"A","value":"b"}]}]}}}}"#),
        ("deployment", "web", "service account", r#"{"spec":{"template":{"spec":{"serviceAccountName":"other"}}}}"#),
        ("deployment", "web", "volume", r#"{"spec":{"template":{"spec":{"volumes":[{"name":"v","emptyDir":{}}]}}}}"#),
        ("deployment", "web", "privileged", r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","securityContext":{"privileged":true}}]}}}}"#),
        ("deployment", "web", "replicas", r#"{"spec":{"replicas":3}}"#),
        ("deployment", "web", "pod label", r#"{"spec":{"template":{"metadata":{"labels":{"extra":"x"}}}}}"#),
        ("deployment", "web", "pod annotation", r#"{"spec":{"template":{"metadata":{"annotations":{"extra":"x"}}}}}"#),
        ("deployment", "web", "workload label", r#"{"metadata":{"labels":{"extra":"x"}}}"#),
        ("deployment", "web", "workload annotation", r#"{"metadata":{"annotations":{"extra":"x"}}}"#),
        ("deployment", "web", "strategy", r#"{"spec":{"strategy":{"type":"Recreate","rollingUpdate":null}}}"#),
        ("deployment", "web", "restart with image", r#"{"spec":{"template":{"metadata":{"annotations":{"kubectl.kubernetes.io/restartedAt":"2026-03-01T00:00:00Z"}},"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#),
        ("statefulset", "db", "image", r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#),
        ("statefulset", "db", "replicas", r#"{"spec":{"replicas":2}}"#),
        ("daemonset", "agent", "host network", r#"{"spec":{"template":{"spec":{"hostNetwork":true}}}}"#),
        ("daemonset", "agent", "update strategy", r#"{"spec":{"updateStrategy":{"type":"OnDelete"}}}"#),
    ];
    for (kind, name, what, body) in refused {
        let (ok, text) = patch(&context, OPERATOR, kind, name, body);
        assert!(!ok, "{what} of {kind} must be refused: {text}");
        assert!(
            text.contains("denied request") && text.contains("restart"),
            "{what} of {kind} is refused by the policy with its message: {text}"
        );
    }

    // The status and scale subresources are not part of the grant.
    for subresource in ["status", "scale"] {
        let (ok, text) = api_patch(
            &context,
            OPERATOR,
            &format!("/apis/apps/v1/namespaces/{NAMESPACE}/deployments/web/{subresource}"),
            r#"{"spec":{"replicas":2}}"#,
        );
        assert!(!ok && text.contains("forbidden"), "{subresource}: {text}");
    }

    // Another account is not affected by the policy.
    let (ok, text) = patch(
        &context,
        OTHER,
        "deployment",
        "web",
        r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
    );
    assert!(ok, "another account is not limited: {text}");

    // The policy compares every field the cluster defines.
    let (ok, schema) = run(
        "kubectl",
        &["--context", &context, "get", "--raw", "/openapi/v3/apis/apps/v1"],
        None,
    );
    assert!(ok);
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();
    for (kind, spec) in [
        ("deployments", "io.k8s.api.apps.v1.DeploymentSpec"),
        ("statefulsets", "io.k8s.api.apps.v1.StatefulSetSpec"),
        ("daemonsets", "io.k8s.api.apps.v1.DaemonSetSpec"),
    ] {
        let policy = rendered
            .split("---")
            .find(|document| {
                document.contains("kind: ValidatingAdmissionPolicy\n")
                    && document.contains(&format!("resources: ['{kind}']"))
            })
            .unwrap_or_else(|| panic!("a policy for {kind}"));
        for property in schema_properties(&schema, spec) {
            if property == "template" {
                continue;
            }
            assert!(
                policy.contains(&format!("object.spec.?{property} == oldObject.spec.?{property}")),
                "the {kind} policy does not compare spec.{property}"
            );
        }
    }
    for property in schema_properties(&schema, "io.k8s.apimachinery.pkg.apis.meta.v1.ObjectMeta") {
        if property == "annotations" {
            continue;
        }
        assert!(
            rendered.contains(&format!(
                "object.spec.template.metadata.?{property} == oldObject.spec.template.metadata.?{property}"
            )),
            "the policy does not compare the pod template's metadata.{property}"
        );
    }
}
