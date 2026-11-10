//! The chart's own RBAC, installed on a real cluster, lets the Operator do what the Kubernetes
//! sandbox controller does before it provisions anything. Earlier live sandbox tests ran with an
//! administrator kubeconfig, which can never catch a permission the chart forgot.
//!
//! Run against a disposable cluster:
//! `ALIEN_SANDBOX_RBAC_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test sandbox_rbac_kind -- --ignored`

use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{fs, path::Path, process::Command};

const NAMESPACE: &str = "alien-sandbox-rbac";
const RELEASE: &str = "sandbox-rbac";

struct Cleanup<'a> {
    context: &'a str,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        // Best effort on the way out of a disposable cluster; the assertions already ran.
        let _ = Command::new("helm")
            .args(["--kube-context", self.context, "uninstall", RELEASE])
            .args(["--namespace", NAMESPACE, "--ignore-not-found", "--no-hooks"])
            .output();
        let _ = Command::new("kubectl")
            .args(["--context", self.context, "delete", "namespace", NAMESPACE])
            .args(["--ignore-not-found", "--wait=false"])
            .output();
    }
}

fn run(program: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("run {program} {arguments:?}: {error}"))
}

fn run_ok(program: &str, arguments: &[&str]) -> String {
    let output = run(program, arguments);
    assert!(
        output.status.success(),
        "{program} {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("command output is UTF-8")
}

/// `kubectl auth can-i` as `subject`: the API server's own authorization decision.
fn can(context: &str, subject: &str, verb: &str, resource: &str) -> String {
    let output = run(
        "kubectl",
        &[
            "--context",
            context,
            "auth",
            "can-i",
            verb,
            resource,
            "--namespace",
            NAMESPACE,
            &format!("--as={subject}"),
        ],
    );
    // can-i exits 1 for "no"; anything else on stderr is a failed check, not an answer.
    let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        answer == "yes" || answer == "no",
        "can-i {verb} {resource} gave no answer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    answer
}

fn write_chart(directory: &Path) {
    let stack = Stack::new(RELEASE.to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(3600),
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = HelmRegistry::built_in();
    let chart = generate_helm_chart(
        &stack,
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: RELEASE.to_string(),
        },
    )
    .expect("generate the sandbox chart");
    for (path, contents) in chart.files {
        let destination = directory.join(path);
        fs::create_dir_all(destination.parent().expect("chart file parent")).expect("chart dir");
        fs::write(destination, contents).expect("write chart file");
    }
}

#[test]
#[ignore = "requires a disposable cluster named by ALIEN_SANDBOX_RBAC_KIND_CONTEXT"]
fn the_chart_lets_the_operator_check_runtime_classes_before_provisioning() {
    let context = std::env::var("ALIEN_SANDBOX_RBAC_KIND_CONTEXT")
        .expect("ALIEN_SANDBOX_RBAC_KIND_CONTEXT names a disposable cluster");
    assert!(
        context.starts_with("kind-"),
        "refusing to install into a non-kind context: {context}"
    );
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path());
    let chart = directory.path().to_str().expect("chart path");
    let _cleanup = Cleanup { context: &context };

    // No --wait: only the RBAC the chart creates is under test, not the Operator pod.
    run_ok(
        "helm",
        &[
            "--kube-context",
            &context,
            "install",
            RELEASE,
            chart,
            "--namespace",
            NAMESPACE,
            "--create-namespace",
            "--set",
            "logCollector.enabled=false",
            "--set",
            "heartbeat.collection.nodes.enabled=false",
            "--set",
            "runtime.cleanup.onUninstall.enabled=false",
            "--set-string",
            "management.defaultUrl=https://manager.example.test",
        ],
    );

    // The account the chart binds the sandbox ClusterRole to, read back from the cluster.
    let bindings = run_ok(
        "kubectl",
        &[
            "--context",
            &context,
            "get",
            "clusterrolebindings",
            "--output",
            "jsonpath={range .items[*]}{.roleRef.name} {.subjects[0].namespace} {.subjects[0].name}{\"\\n\"}{end}",
        ],
    );
    let operator_account = bindings
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (role, namespace, account) = (fields.next()?, fields.next()?, fields.next()?);
            (role.ends_with("-sandbox-broker") && namespace == NAMESPACE)
                .then(|| account.to_string())
        })
        .next()
        .unwrap_or_else(|| panic!("no sandbox ClusterRoleBinding in {NAMESPACE}:\n{bindings}"));
    let subject = format!("system:serviceaccount:{NAMESPACE}:{operator_account}");

    // The sandbox controller's first call: list RuntimeClasses to find gVisor or Kata.
    assert_eq!(
        can(&context, &subject, "list", "runtimeclasses.node.k8s.io"),
        "yes",
        "the Operator cannot list RuntimeClasses, so every sandbox stays Provisioning"
    );
    // The broker authenticates a session's caller with a TokenReview.
    assert_eq!(
        can(
            &context,
            &subject,
            "create",
            "tokenreviews.authentication.k8s.io"
        ),
        "yes"
    );
    // Reading is all it needs: a namespaced Operator must not be able to change the cluster's
    // runtimes, which would let it weaken every tenant's isolation.
    for verb in ["create", "update", "patch", "delete"] {
        assert_eq!(
            can(&context, &subject, verb, "runtimeclasses.node.k8s.io"),
            "no",
            "the Operator must not {verb} RuntimeClasses"
        );
    }
}
