//! The Operator's `patch` on workloads is limited to a restart by the cluster itself: a
//! ValidatingAdmissionPolicy shipped with the rule. Kubernetes RBAC alone would let a patch change
//! a pod template's image, environment, service account or volumes.
//!
//! This installs the rendered chart on a real cluster (Kubernetes 1.30 or later) and patches as
//! the Operator's ServiceAccount: a restart passes, every other change is refused with the
//! policy's message, and another account is not affected.
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
use std::{fs, path::Path, process::Command};

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
            .args(["--namespace", NAMESPACE, "--ignore-not-found"])
            .output();
        let _ = Command::new("kubectl")
            .args(["--context", self.0, "delete", "namespace", NAMESPACE])
            .args(["--ignore-not-found", "--wait=true", "--timeout=120s"])
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
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
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

/// The Operator chart. `with_operation` declares the restart operation (a chart without it is
/// the chart as it was before the feature); `scope` is the install scope. A `zz-broken.yaml`
/// template that fails when applied is switched on by `--set broken=true`: it sorts after the
/// policy objects, so the release fails after they were applied.
fn write_chart(directory: &Path, with_operation: bool, scope: OperatorScope) {
    let operations = [operation()];
    let template = generate_operator_manifest(OperatorManifestOptions {
        custom_operation_permissions: if with_operation { &operations } else { &[] },
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
        scope,
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
        "broken: false\nremoteOperator:\n  serviceAccountAnnotations: {}\nalien:\n  version: test\n",
    )
    .unwrap();
    fs::write(directory.join("templates/operator.yaml"), template).unwrap();
    fs::write(
        directory.join("templates/zz-broken.yaml"),
        "{{- if .Values.broken }}\napiVersion: admissionregistration.k8s.io/v1\nkind: ValidatingAdmissionPolicyBinding\nmetadata: {name: Not_A_Valid_Name}\nspec: {policyName: nothing}\n{{- end }}\n",
    )
    .unwrap();
}

const WORKLOADS: &str = r#"apiVersion: apps/v1
kind: Deployment
metadata: {name: web}
spec:
  replicas: 0
  selector: {matchLabels: {app: web}}
  template:
    metadata: {labels: {app: web}, annotations: {keep: "1"}}
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
# Test only: `kubectl rollout restart` reads the workload before it patches it. The Operator's own
# grant does not include reading a workload by name, so this lets the real command run as it.
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata: {name: operator-get}
rules:
  - {apiGroups: [apps], resources: [deployments, statefulsets, daemonsets], verbs: [get]}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata: {name: operator-get}
subjects: [{kind: ServiceAccount, name: my-saas-operator}]
roleRef: {apiGroup: rbac.authorization.k8s.io, kind: Role, name: operator-get}
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
    api_patch_as(context, user, path, "strategic-merge-patch+json", body)
}

fn api_patch_as(
    context: &str,
    user: &str,
    path: &str,
    content_type: &str,
    body: &str,
) -> (bool, String) {
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
        .args([
            &format!("Content-Type: application/{content_type}"),
            "-d",
            body,
            &url,
        ])
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

#[test]
#[ignore = "requires a disposable cluster (Kubernetes 1.30 or later) named by ALIEN_WORKLOAD_RESTART_KIND_CONTEXT"]
fn the_cluster_admits_only_a_restart_from_the_operator() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path(), true, OperatorScope::Namespace);

    let rendered = run(
        "helm",
        &[
            "template",
            RELEASE,
            directory.path().to_str().unwrap(),
            "--namespace",
            NAMESPACE,
            "--api-versions",
            "admissionregistration.k8s.io/v1/ValidatingAdmissionPolicy",
        ],
        None,
    );
    assert!(rendered.0);
    assert!(rendered.1.contains("helm.sh/hook: post-install"));
    let (ok, _) = run(
        "helm",
        &[
            "--kube-context",
            &context,
            "install",
            RELEASE,
            directory.path().to_str().unwrap(),
            "--namespace",
            NAMESPACE,
            "--create-namespace",
        ],
        None,
    );
    assert!(ok, "the chart installs on this cluster");
    let (ok, _) = kubectl(&context, &["apply", "-f", "-"], Some(WORKLOADS));
    assert!(ok);

    // The cluster advertises the policy API, so the chart rendered the rule itself.
    let (_, can) = kubectl(
        &context,
        &[
            "auth",
            "can-i",
            "patch",
            "deployments.apps",
            &format!("--as={OPERATOR}"),
        ],
        None,
    );
    assert_eq!(can.trim(), "yes");
    let (_, can) = kubectl(
        &context,
        &[
            "auth",
            "can-i",
            "patch",
            "deployments.apps",
            "--subresource=status",
            &format!("--as={OPERATOR}"),
        ],
        None,
    );
    assert_eq!(can.trim(), "no", "the status subresource is not granted");
    // The binding takes a moment to start enforcing.
    std::thread::sleep(std::time::Duration::from_secs(5));

    // A restart passes, for each kind, and repeats.
    for (kind, name) in [
        ("deployment", "web"),
        ("statefulset", "db"),
        ("daemonset", "agent"),
    ] {
        let (ok, text) = patch(&context, OPERATOR, kind, name, RESTART);
        assert!(ok, "restart of {kind}: {text}");
    }
    let again = RESTART.replace("2026-01-01", "2026-02-01");
    assert!(patch(&context, OPERATOR, "deployment", "web", &again).0);

    // The real command passes: it patches the same annotation.
    for target in ["deployment/web", "statefulset/db", "daemonset/agent"] {
        let (ok, text) = kubectl(
            &context,
            &[
                "rollout",
                "restart",
                target,
                &format!("--as={OPERATOR}"),
                "-v=6",
            ],
            None,
        );
        assert!(
            ok && text.contains("restarted"),
            "kubectl rollout restart {target}: {text}"
        );
    }

    // Server-side apply is an UPDATE as well: a restart applied this way passes, an image change
    // applied this way does not.
    let apply_path = format!(
        "/apis/apps/v1/namespaces/{NAMESPACE}/deployments/web?fieldManager=operator&force=true"
    );
    let (ok, text) = api_patch_as(
        &context,
        OPERATOR,
        &apply_path,
        "apply-patch+yaml",
        r#"{"apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"web"},"spec":{"template":{"metadata":{"annotations":{"kubectl.kubernetes.io/restartedAt":"2026-04-01T00:00:00Z"}}}}}"#,
    );
    assert!(ok, "server-side apply of a restart: {text}");
    let (ok, text) = api_patch_as(
        &context,
        OPERATOR,
        &apply_path,
        "apply-patch+yaml",
        r#"{"apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"web"},"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
    );
    assert!(
        !ok && text.contains("denied request"),
        "server-side apply of an image: {text}"
    );

    // Every other change is refused by the policy.
    let refused = [
        (
            "deployment",
            "web",
            "image",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
        ),
        (
            "deployment",
            "web",
            "environment",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","env":[{"name":"A","value":"b"}]}]}}}}"#,
        ),
        (
            "deployment",
            "web",
            "service account",
            r#"{"spec":{"template":{"spec":{"serviceAccountName":"other"}}}}"#,
        ),
        (
            "deployment",
            "web",
            "volume",
            r#"{"spec":{"template":{"spec":{"volumes":[{"name":"v","emptyDir":{}}]}}}}"#,
        ),
        (
            "deployment",
            "web",
            "privileged",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","securityContext":{"privileged":true}}]}}}}"#,
        ),
        (
            "deployment",
            "web",
            "replicas",
            r#"{"spec":{"replicas":3}}"#,
        ),
        (
            "deployment",
            "web",
            "pod label",
            r#"{"spec":{"template":{"metadata":{"labels":{"extra":"x"}}}}}"#,
        ),
        (
            "deployment",
            "web",
            "pod annotation",
            r#"{"spec":{"template":{"metadata":{"annotations":{"extra":"x"}}}}}"#,
        ),
        (
            "deployment",
            "web",
            "workload label",
            r#"{"metadata":{"labels":{"extra":"x"}}}"#,
        ),
        (
            "deployment",
            "web",
            "workload annotation",
            r#"{"metadata":{"annotations":{"extra":"x"}}}"#,
        ),
        (
            "deployment",
            "web",
            "a spec field that was not set",
            r#"{"spec":{"minReadySeconds":5}}"#,
        ),
        (
            "deployment",
            "web",
            "strategy",
            r#"{"spec":{"strategy":{"type":"Recreate","rollingUpdate":null}}}"#,
        ),
        (
            "deployment",
            "web",
            "restart with image",
            r#"{"spec":{"template":{"metadata":{"annotations":{"kubectl.kubernetes.io/restartedAt":"2026-03-01T00:00:00Z"}},"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
        ),
        (
            "statefulset",
            "db",
            "image",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
        ),
        (
            "statefulset",
            "db",
            "replicas",
            r#"{"spec":{"replicas":2}}"#,
        ),
        (
            "daemonset",
            "agent",
            "host network",
            r#"{"spec":{"template":{"spec":{"hostNetwork":true}}}}"#,
        ),
        (
            "daemonset",
            "agent",
            "update strategy",
            r#"{"spec":{"updateStrategy":{"type":"OnDelete"}}}"#,
        ),
        ("deployment", "web", "paused", r#"{"spec":{"paused":true}}"#),
        (
            "deployment",
            "web",
            "revision history limit",
            r#"{"spec":{"revisionHistoryLimit":1}}"#,
        ),
        (
            "deployment",
            "web",
            "an init container",
            r#"{"spec":{"template":{"spec":{"initContainers":[{"name":"i","image":"busybox"}]}}}}"#,
        ),
        (
            "deployment",
            "web",
            "owner references",
            r#"{"metadata":{"ownerReferences":[{"apiVersion":"v1","kind":"ConfigMap","name":"x","uid":"00000000-0000-0000-0000-000000000000"}]}}"#,
        ),
        (
            "deployment",
            "web",
            "finalizers",
            r#"{"metadata":{"finalizers":["example.com/hold"]}}"#,
        ),
        (
            "deployment",
            "web",
            "the annotation the server sets on a DaemonSet, set on a Deployment",
            r#"{"metadata":{"annotations":{"deprecated.daemonset.template.generation":"9"}}}"#,
        ),
        (
            "deployment",
            "web",
            "removing an existing pod annotation",
            r#"{"spec":{"template":{"metadata":{"annotations":{"keep":null}}}}}"#,
        ),
        (
            "daemonset",
            "agent",
            "a pod template key beside metadata and spec",
            r#"{"metadata":{"labels":{"extra":"x"}}}"#,
        ),
    ];
    for (kind, name, what, body) in refused {
        let (ok, text) = patch(&context, OPERATOR, kind, name, body);
        assert!(!ok, "{what} of {kind} must be refused: {text}");
        assert!(
            text.contains("denied request") && text.contains("restart"),
            "{what} of {kind} is refused by the policy with its message: {text}"
        );
    }

    // The same changes in the other patch formats, and changes the API server may refuse on its
    // own: refused whichever check says so.
    let item = format!("/apis/apps/v1/namespaces/{NAMESPACE}/deployments/web");
    for (what, content_type, body) in [
        (
            "JSON patch of the replicas",
            "json-patch+json",
            r#"[{"op":"replace","path":"/spec/replicas","value":3}]"#,
        ),
        (
            "JSON patch of the image",
            "json-patch+json",
            r#"[{"op":"replace","path":"/spec/template/spec/containers/0/image","value":"busybox"}]"#,
        ),
        (
            "JSON patch adding an annotation next to the restart",
            "json-patch+json",
            r#"[{"op":"add","path":"/spec/template/metadata/annotations/other","value":"x"}]"#,
        ),
        (
            "merge patch of the image",
            "merge-patch+json",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
        ),
        (
            "merge patch removing a pod label",
            "merge-patch+json",
            r#"{"spec":{"template":{"metadata":{"labels":{"app":null}}}}}"#,
        ),
        (
            "an ephemeral container",
            "merge-patch+json",
            r#"{"spec":{"template":{"spec":{"ephemeralContainers":[{"name":"e","image":"busybox"}]}}}}"#,
        ),
    ] {
        let (ok, text) = api_patch_as(&context, OPERATOR, &item, content_type, body);
        assert!(!ok, "{what} must be refused: {text}");
    }
    let (ok, text) = api_patch_as(
        &context,
        OPERATOR,
        &item,
        "json-patch+json",
        r#"[{"op":"add","path":"/spec/template/metadata/annotations/kubectl.kubernetes.io~1restartedAt","value":"2026-05-01T00:00:00Z"}]"#,
    );
    assert!(ok, "a JSON patch of the restart annotation passes: {text}");
    let (ok, text) = api_patch_as(
        &context,
        OPERATOR,
        &format!("/apis/apps/v1/namespaces/{NAMESPACE}/statefulsets/db"),
        "merge-patch+json",
        r#"{"spec":{"volumeClaimTemplates":[{"metadata":{"name":"data"},"spec":{"accessModes":["ReadWriteOnce"],"resources":{"requests":{"storage":"1Gi"}}}}]}}"#,
    );
    assert!(!ok, "volumeClaimTemplates must be refused: {text}");

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
}

/// On a cluster that does not serve the admission policy API (before Kubernetes 1.30) the chart
/// renders no workload `patch` and no policy, and the Role is otherwise unchanged. `helm
/// template` cannot show this, because it does not ask a cluster what it serves.
#[test]
#[ignore = "requires a disposable cluster older than Kubernetes 1.30 named by ALIEN_WORKLOAD_RESTART_OLD_KIND_CONTEXT"]
fn a_cluster_without_the_policy_api_gets_no_workload_patch() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_OLD_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_OLD_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let (ok, _) = run(
        "kubectl",
        &["--context", &context, "create", "namespace", NAMESPACE],
        None,
    );
    assert!(ok);
    let (_, resources) = run(
        "kubectl",
        &[
            "--context",
            &context,
            "get",
            "--raw",
            "/apis/admissionregistration.k8s.io/v1",
        ],
        None,
    );
    assert!(
        !resources.contains("validatingadmissionpolicies"),
        "this cluster serves the policy API; use one older than Kubernetes 1.30"
    );
    let directory = tempfile::tempdir().unwrap();
    write_chart(directory.path(), true, OperatorScope::Namespace);
    let (ok, rendered) = run(
        "helm",
        &[
            "--kube-context",
            &context,
            "install",
            RELEASE,
            directory.path().to_str().unwrap(),
            "--namespace",
            NAMESPACE,
            "--dry-run=server",
        ],
        None,
    );
    assert!(ok, "the chart renders on an older cluster: {rendered}");
    assert!(
        rendered.contains("kind: Role\n"),
        "the Operator's Role is still rendered"
    );
    assert!(!rendered.contains("ValidatingAdmissionPolicy"), "no policy");
    assert!(
        !rendered.contains("verbs: ['patch']"),
        "no workload patch is granted without its policy"
    );
}

static NEXT_REPLICAS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

const PROBE_BODY: &str = r#"{"spec":{"replicas":REPLICAS}}"#;

/// Patches the replicas of `web` as `user` over and over (a different value every time, so an
/// admitted patch is always a change) and records every HTTP status.
struct Probe {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    statuses: std::sync::Arc<std::sync::Mutex<Vec<(std::time::Instant, u16)>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    proxy: std::process::Child,
}

impl Probe {
    fn start(context: &str, user: &str) -> Probe {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};
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
        let url = format!("http://{address}/apis/apps/v1/namespaces/{NAMESPACE}/deployments/web");
        let stop = Arc::new(AtomicBool::new(false));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, seen) = (stop.clone(), statuses.clone());
        let thread = std::thread::spawn(move || {
            // Every probe patch is a change: a repeated value would be a no-op the policy admits.
            while !stop_flag.load(Ordering::SeqCst) {
                let count = NEXT_REPLICAS.fetch_add(1, Ordering::SeqCst);
                let body = PROBE_BODY.replace("REPLICAS", &count.to_string());
                let output = Command::new("curl")
                    .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-X", "PATCH"])
                    .args(["-H", "Content-Type: application/strategic-merge-patch+json"])
                    .args(["-d", &body, &url])
                    .output()
                    .expect("run curl");
                if let Ok(status) = String::from_utf8_lossy(&output.stdout).parse::<u16>() {
                    seen.lock()
                        .unwrap()
                        .push((std::time::Instant::now(), status));
                }
            }
        });
        Probe {
            stop,
            statuses,
            thread: Some(thread),
            proxy,
        }
    }

    /// The statuses seen, in order, with repeats folded.
    fn finish(mut self) -> Seen {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
        let _ = self.proxy.kill();
        let _ = self.proxy.wait();
        let all = self.statuses.lock().unwrap().clone();
        let mut folded: Vec<u16> = all.iter().map(|(_, status)| *status).collect();
        folded.dedup();
        // How long an admitted patch was possible: from the first to the last 200.
        let admitted: Vec<_> = all.iter().filter(|(_, status)| *status == 200).collect();
        println!(
            "probe statuses (HTTP, repeats folded): {folded:?}; {} requests, {} admitted{}",
            all.len(),
            admitted.len(),
            match (admitted.first(), admitted.last()) {
                (Some(first), Some(last)) => format!(" over {} ms", (last.0 - first.0).as_millis()),
                _ => String::new(),
            }
        );
        Seen {
            statuses: folded,
            admitted_ms: match (admitted.first(), admitted.last()) {
                (Some(first), Some(last)) => Some((last.0 - first.0).as_millis()),
                _ => None,
            },
        }
    }
}

struct Seen {
    statuses: Vec<u16>,
    /// From the first to the last admitted patch, if any was.
    admitted_ms: Option<u128>,
}

impl Seen {
    /// The grant is never usable unenforced: no patch was admitted.
    fn assert_never_admitted(&self, when: &str) {
        assert!(
            self.admitted_ms.is_none(),
            "a patch was admitted {when}: {:?}",
            self.statuses
        );
    }

    /// The policy was new: the API server needs a moment to start enforcing it, and patches the
    /// grant admits in that moment are not limited. It must be short and end in the refusal.
    fn assert_only_the_start_up_of_a_new_policy(&self, when: &str) {
        assert_eq!(
            self.statuses.first(),
            Some(&403),
            "{when}: {:?}",
            self.statuses
        );
        assert_eq!(
            self.statuses.last(),
            Some(&422),
            "{when}: {:?}",
            self.statuses
        );
        assert!(
            self.statuses.len() <= 3,
            "{when}: admitted again after refusing: {:?}",
            self.statuses
        );
        assert!(
            self.admitted_ms.unwrap_or(0) < 3000,
            "{when}: the unenforced window is {:?} ms",
            self.admitted_ms
        );
        println!(
            "{when}: patches were admitted for {:?} ms after the grant appeared (the API server starting to enforce the new policy)",
            self.admitted_ms
        );
    }
}

fn can_patch_workloads(context: &str) -> bool {
    let (_, can) = kubectl(
        context,
        &[
            "auth",
            "can-i",
            "patch",
            "deployments.apps",
            &format!("--as={OPERATOR}"),
        ],
        None,
    );
    can.trim() == "yes"
}

fn helm_chart(context: &str, verb: &str, chart: &Path, extra: &[&str]) -> (bool, String) {
    let mut arguments = vec!["--kube-context", context, verb, RELEASE];
    arguments.push(chart.to_str().unwrap());
    arguments.extend(["--namespace", NAMESPACE]);
    arguments.extend(extra);
    run("helm", &arguments, None)
}

fn install_namespace_and_workloads(context: &str) {
    assert!(
        run(
            "kubectl",
            &["--context", context, "create", "namespace", NAMESPACE],
            None
        )
        .0
    );
    assert!(kubectl(context, &["apply", "-f", "-"], Some(WORKLOADS)).0);
}

/// The grant (the `patch` rule) is never usable without an enforcing policy: not while a chart
/// that has the feature is installed over one that has not, not when the upgrade fails after the
/// policy was applied, and not while the release is uninstalled.
///
/// A probe patches the replicas of a workload as the Operator the whole time. It sees 403 while
/// there is no grant; the first answer after that must be the policy's refusal, never an
/// admitted patch.
#[test]
#[ignore = "requires a disposable cluster (Kubernetes 1.30 or later) named by ALIEN_WORKLOAD_RESTART_KIND_CONTEXT"]
fn the_grant_never_exists_without_an_enforcing_policy() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let before = tempfile::tempdir().unwrap();
    let after = tempfile::tempdir().unwrap();
    write_chart(before.path(), false, OperatorScope::Namespace);
    write_chart(after.path(), true, OperatorScope::Namespace);
    install_namespace_and_workloads(&context);

    // A fresh install of the chart with the feature.
    let probe = Probe::start(&context, OPERATOR);
    let (ok, _) = helm_chart(&context, "install", after.path(), &[]);
    assert!(ok);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let seen = probe.finish();
    assert!(can_patch_workloads(&context));
    seen.assert_only_the_start_up_of_a_new_policy("install");
    // Hook objects stay out of the release's manifest.
    let (_, manifest) = run(
        "helm",
        &[
            "--kube-context",
            &context,
            "get",
            "manifest",
            RELEASE,
            "--namespace",
            NAMESPACE,
        ],
        None,
    );
    assert!(manifest.contains("ValidatingAdmissionPolicy"));
    assert!(!manifest.contains("verbs: ['patch']") && !manifest.contains("- patch"));

    // Uninstall: the pre-delete hook removes the grant before the policy goes.
    let probe = Probe::start(&context, OPERATOR);
    let (ok, _) = run(
        "helm",
        &[
            "--kube-context",
            &context,
            "uninstall",
            RELEASE,
            "--namespace",
            NAMESPACE,
        ],
        None,
    );
    assert!(ok);
    std::thread::sleep(std::time::Duration::from_secs(2));
    let seen = probe.finish();
    seen.assert_never_admitted("during uninstall");
    assert_eq!(seen.statuses.last(), Some(&403));
    assert!(!can_patch_workloads(&context));
    for kind in ["role", "rolebinding", "clusterrole", "clusterrolebinding"] {
        let (_, left) = kubectl(
            &context,
            &["get", kind, "-A", "--no-headers", "-o", "name"],
            None,
        );
        assert!(!left.contains("restart"), "uninstall left a {kind}: {left}");
    }
    let (_, policies) = run(
        "kubectl",
        &[
            "--context",
            &context,
            "get",
            "validatingadmissionpolicies,validatingadmissionpolicybindings",
            "-o",
            "name",
        ],
        None,
    );
    assert!(
        !policies.contains("restart"),
        "uninstall left a policy: {policies}"
    );

    // An upgrade from a chart without the feature.
    assert!(helm_chart(&context, "install", before.path(), &[]).0);
    assert!(!can_patch_workloads(&context));
    let probe = Probe::start(&context, OPERATOR);
    let (ok, _) = helm_chart(&context, "upgrade", after.path(), &[]);
    assert!(ok);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let seen = probe.finish();
    assert!(can_patch_workloads(&context));
    seen.assert_only_the_start_up_of_a_new_policy("upgrade from a chart without the feature");

    // A second upgrade replaces the grant (the hook deletes it before it creates it again) and
    // never leaves it without the policy.
    let probe = Probe::start(&context, OPERATOR);
    assert!(
        helm_chart(
            &context,
            "upgrade",
            after.path(),
            &["--set", "alien.version=next"]
        )
        .0
    );
    std::thread::sleep(std::time::Duration::from_secs(2));
    let seen = probe.finish();
    seen.assert_never_admitted("during a second upgrade (the policy is already enforcing)");
    assert!(can_patch_workloads(&context));

    // A failed upgrade, after the policy objects were applied, creates no new grant: start from
    // the chart without the feature.
    assert!(
        run(
            "helm",
            &[
                "--kube-context",
                &context,
                "uninstall",
                RELEASE,
                "--namespace",
                NAMESPACE
            ],
            None
        )
        .0
    );
    assert!(helm_chart(&context, "install", before.path(), &[]).0);
    let (ok, text) = helm_chart(&context, "upgrade", after.path(), &["--set", "broken=true"]);
    assert!(!ok, "the broken upgrade must fail: {text}");
    let (_, policies) = run(
        "kubectl",
        &[
            "--context",
            &context,
            "get",
            "validatingadmissionpolicies",
            "-o",
            "name",
        ],
        None,
    );
    assert!(
        policies.contains("restart"),
        "the policy was applied before the failure: {policies}"
    );
    assert!(
        !can_patch_workloads(&context),
        "a failed upgrade leaves no grant"
    );
    // And the next good upgrade completes it.
    assert!(helm_chart(&context, "upgrade", after.path(), &[]).0);
    assert!(can_patch_workloads(&context));
    // A rollback to the revision of the chart without the feature, recorded as it is: Helm runs
    // the hooks of the revision it returns to, and that chart has none, so the grant its
    // successor created stays while the rollback removes the policy.
    let (ok, _) = run(
        "helm",
        &[
            "--kube-context",
            &context,
            "rollback",
            RELEASE,
            "1",
            "--namespace",
            NAMESPACE,
        ],
        None,
    );
    assert!(ok);
    println!(
        "after a rollback to a chart without the feature: grant present = {}",
        can_patch_workloads(&context)
    );
}

/// A cluster-wide install patches in any namespace; its policy has no namespace selector.
#[test]
#[ignore = "requires a disposable cluster (Kubernetes 1.30 or later) named by ALIEN_WORKLOAD_RESTART_KIND_CONTEXT"]
fn a_cluster_wide_install_is_limited_to_a_restart_in_every_namespace() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let _other = CleanupNamespace(&context, "alien-restart-other");
    let chart = tempfile::tempdir().unwrap();
    write_chart(chart.path(), true, OperatorScope::Cluster);
    install_namespace_and_workloads(&context);
    assert!(helm_chart(&context, "install", chart.path(), &[]).0);
    assert!(
        run(
            "kubectl",
            &[
                "--context",
                &context,
                "create",
                "namespace",
                "alien-restart-other"
            ],
            None
        )
        .0
    );
    assert!(
        run(
            "kubectl",
            &[
                "--context",
                &context,
                "--namespace",
                "alien-restart-other",
                "apply",
                "-f",
                "-"
            ],
            Some(WORKLOADS.split("---\n").next().unwrap()),
        )
        .0
    );
    std::thread::sleep(std::time::Duration::from_secs(5));
    let path = "/apis/apps/v1/namespaces/alien-restart-other/deployments/web";
    assert!(
        api_patch(&context, OPERATOR, path, RESTART).0,
        "a restart in another namespace"
    );
    let (ok, text) = api_patch(
        &context,
        OPERATOR,
        path,
        r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
    );
    assert!(
        !ok && text.contains("denied request"),
        "an image in another namespace: {text}"
    );
}

struct CleanupNamespace<'a>(&'a str, &'a str);

impl Drop for CleanupNamespace<'_> {
    fn drop(&mut self) {
        let _ = Command::new("kubectl")
            .args(["--context", self.0, "delete", "namespace", self.1])
            .args(["--ignore-not-found", "--wait=true", "--timeout=120s"])
            .output();
    }
}

/// A namespaced install is bound to its own namespace: its Role does not reach another one, and
/// the binding does not match another namespace's workloads.
#[test]
#[ignore = "requires a disposable cluster (Kubernetes 1.30 or later) named by ALIEN_WORKLOAD_RESTART_KIND_CONTEXT"]
fn a_namespaced_install_reaches_and_limits_only_its_own_namespace() {
    let context = std::env::var("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT")
        .expect("ALIEN_WORKLOAD_RESTART_KIND_CONTEXT names the cluster");
    let _cleanup = Cleanup(&context);
    let _other = CleanupNamespace(&context, "alien-restart-other");
    let chart = tempfile::tempdir().unwrap();
    write_chart(chart.path(), true, OperatorScope::Namespace);
    install_namespace_and_workloads(&context);
    assert!(helm_chart(&context, "install", chart.path(), &[]).0);
    assert!(
        run(
            "kubectl",
            &[
                "--context",
                &context,
                "create",
                "namespace",
                "alien-restart-other"
            ],
            None
        )
        .0
    );
    assert!(
        run(
            "kubectl",
            &[
                "--context",
                &context,
                "--namespace",
                "alien-restart-other",
                "apply",
                "-f",
                "-"
            ],
            Some(WORKLOADS.split("---\n").next().unwrap()),
        )
        .0
    );
    std::thread::sleep(std::time::Duration::from_secs(5));
    let path = "/apis/apps/v1/namespaces/alien-restart-other/deployments/web";
    let (ok, text) = api_patch(&context, OPERATOR, path, RESTART);
    assert!(
        !ok && text.contains("forbidden") && !text.contains("denied request"),
        "{text}"
    );
    // Nobody else is limited there.
    let (ok, text) = run(
        "kubectl",
        &[
            "--context",
            &context,
            "--namespace",
            "alien-restart-other",
            "patch",
            "deployment",
            "web",
            "--type=merge",
            "-p",
            r#"{"spec":{"template":{"spec":{"containers":[{"name":"main","image":"busybox"}]}}}}"#,
        ],
        None,
    );
    assert!(
        ok,
        "an account that is not the Operator is not limited: {}",
        text
    );
}
