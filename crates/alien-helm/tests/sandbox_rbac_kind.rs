//! The chart's own RBAC, installed on a real cluster, lets the Operator do what the Kubernetes
//! sandbox controller does before it provisions anything. Earlier live sandbox tests ran with an
//! administrator kubeconfig, which can never catch a permission the chart forgot.
//!
//! Installs of the same chart go into one cluster the ways a vendor runs copies: the same release
//! name in two namespaces (one copy per tenant), and two long release names in one namespace that
//! differ only past the length a cluster-scoped name can keep. Each must get its own
//! cluster-scoped grant, bound to its own Operator.
//!
//! Run against a disposable cluster:
//! `ALIEN_SANDBOX_RBAC_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test sandbox_rbac_kind -- --ignored`

use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const NAMESPACES: [&str; 2] = ["alien-sandbox-rbac-a", "alien-sandbox-rbac-b"];
const RELEASE: &str = "sandbox-rbac";
/// Release names that contain the chart name are used as the full name unchanged, and these two
/// agree on their first 45 characters, past where the role name truncates the full name.
const LONG_RELEASES: [&str; 2] = [
    "sandbox-rbac-tenant-with-a-long-install-name-one",
    "sandbox-rbac-tenant-with-a-long-install-name-two",
];

/// Every (release, namespace) the test installs.
fn installs() -> Vec<(&'static str, &'static str)> {
    let mut installs: Vec<_> = NAMESPACES
        .iter()
        .map(|namespace| (RELEASE, *namespace))
        .collect();
    installs.extend(
        LONG_RELEASES
            .iter()
            .map(|release| (*release, NAMESPACES[0])),
    );
    installs
}

struct Cleanup<'a> {
    context: &'a str,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        // Best effort on the way out of a disposable cluster; the assertions already ran.
        for (release, namespace) in installs() {
            let _ = Command::new("helm")
                .args(["--kube-context", self.context, "uninstall", release])
                .args(["--namespace", namespace, "--ignore-not-found", "--no-hooks"])
                .output();
        }
        for namespace in NAMESPACES {
            let _ = Command::new("kubectl")
                .args(["--context", self.context, "delete", "namespace", namespace])
                .args(["--ignore-not-found", "--wait=false"])
                .output();
        }
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
            &format!("--as={subject}"),
        ],
    );
    // can-i exits 1 for "no"; anything else is a failed check, not an answer.
    let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        answer == "yes" || answer == "no",
        "can-i {verb} {resource} gave no answer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    answer
}

/// The API server's authorizer learns a new binding from its informer, a moment after the
/// install returns. A grant the chart does not make never shows up within the bound.
fn eventually_can(context: &str, subject: &str, verb: &str, resource: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if can(context, subject, verb, resource) == "yes" {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_secs(1));
    }
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

/// Every sandbox ClusterRoleBinding in the cluster: (binding, role, release, namespace, account).
fn sandbox_bindings(context: &str) -> Vec<(String, String, String, String, String)> {
    let listing = run_ok(
        "kubectl",
        &[
            "--context",
            context,
            "get",
            "clusterrolebindings",
            "--output",
            "jsonpath={range .items[*]}{.metadata.name} {.roleRef.name} {.metadata.labels.app\\.kubernetes\\.io/instance} {.subjects[0].namespace} {.subjects[0].name}{\"\\n\"}{end}",
        ],
    );
    listing
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                [binding, role, release, namespace, account]
                    if binding.contains("-sandbox-broker") =>
                {
                    Some((
                        binding.to_string(),
                        role.to_string(),
                        release.to_string(),
                        namespace.to_string(),
                        account.to_string(),
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

#[test]
#[ignore = "requires a disposable cluster named by ALIEN_SANDBOX_RBAC_KIND_CONTEXT"]
fn each_install_lets_its_own_operator_check_runtime_classes() {
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

    // No --wait: only the RBAC the chart creates is under test, not the Operator pod. A later
    // install fails here if it shares a cluster-scoped name with an earlier one.
    for (release, namespace) in installs() {
        run_ok(
            "helm",
            &[
                "--kube-context",
                &context,
                "install",
                release,
                chart,
                "--namespace",
                namespace,
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
    }

    let bindings = sandbox_bindings(&context);
    assert_eq!(
        bindings.len(),
        installs().len(),
        "each install needs its own sandbox ClusterRoleBinding: {bindings:?}"
    );
    for (release, namespace) in installs() {
        let (binding, role, _, _, account) = bindings
            .iter()
            .find(|(_, _, binding_release, subject_namespace, _)| {
                binding_release == release && subject_namespace == namespace
            })
            .unwrap_or_else(|| {
                panic!("no sandbox binding for {release} in {namespace}: {bindings:?}")
            });
        assert_eq!(binding, role, "each binding names its own install's role");
        let subject = format!("system:serviceaccount:{namespace}:{account}");

        // The sandbox controller's first call: list RuntimeClasses to find gVisor or Kata.
        assert!(
            eventually_can(&context, &subject, "list", "runtimeclasses.node.k8s.io"),
            "{release} in {namespace}: the Operator cannot list RuntimeClasses, so every sandbox \
             stays Provisioning"
        );
        // The broker authenticates a session's caller with a TokenReview.
        assert!(
            eventually_can(
                &context,
                &subject,
                "create",
                "tokenreviews.authentication.k8s.io"
            ),
            "{release} in {namespace}: the broker cannot authenticate callers"
        );
        // Reading is all it needs: a namespaced Operator must not be able to change the
        // cluster's runtimes, which would let it weaken every tenant's isolation.
        for verb in ["create", "update", "patch", "delete"] {
            assert_eq!(
                can(&context, &subject, verb, "runtimeclasses.node.k8s.io"),
                "no",
                "{release} in {namespace}: the Operator must not {verb} RuntimeClasses"
            );
        }
    }
}
