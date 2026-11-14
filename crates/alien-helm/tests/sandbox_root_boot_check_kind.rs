//! The install-time check for a root-boot sandbox's namespace, on a real API server.
//!
//! A root-boot sandbox needs a namespace that admits Pod Security `baseline`. The chart's
//! pre-install hook dry-runs a root-boot pod there, so a namespace that refuses it fails the
//! install with the fix, instead of leaving a pool that never fills. A namespace that does not
//! exist fails the install too, at the Role the chart puts in it.
//!
//! Needs a Kind cluster with a RuntimeClass named `alien-kind-runc` and the probe image loaded
//! (`kind load docker-image <the pinned python image>`):
//! `ALIEN_ROOT_BOOT_CHECK_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test sandbox_root_boot_check_kind -- --ignored`

use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{fs, path::Path, process::Command};

const RELEASE: &str = "rbcheck";

fn run(program: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("run {program} {arguments:?}: {error}"))
}

fn text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn write_chart(directory: &Path) {
    let mut sandbox = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu:24.04".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(3600),
            idle_pause_seconds: None,
        })
        .build();
    sandbox.image_user = Some(0);
    let stack = Stack::new(RELEASE.to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
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
    .expect("generate the chart");
    for (path, contents) in chart.files {
        let destination = directory.join(path);
        fs::create_dir_all(destination.parent().expect("parent")).expect("chart dir");
        fs::write(destination, contents).expect("write chart file");
    }
}

fn install(context: &str, chart: &str, release_namespace: &str, sandbox_namespace: &str) -> String {
    let output = run(
        "helm",
        &[
            "--kube-context",
            context,
            "install",
            RELEASE,
            chart,
            "--namespace",
            release_namespace,
            "--create-namespace",
            "--set",
            "logCollector.enabled=false",
            "--set",
            "heartbeat.collection.nodes.enabled=false",
            "--set",
            "runtime.cleanup.onUninstall.enabled=false",
            "--set-string",
            "management.defaultUrl=https://manager.example.test",
            "--set-string",
            "sandbox.runtimeClassName=alien-kind-runc",
            "--set-string",
            &format!("sandbox.namespace={sandbox_namespace}"),
        ],
    );
    format!("ok={}\n{}", output.status.success(), text(&output))
}

fn check_job_log(context: &str, release_namespace: &str) -> String {
    let jobs = run(
        "kubectl",
        &[
            "--context",
            context,
            "-n",
            release_namespace,
            "get",
            "jobs",
            "-o",
            "name",
        ],
    );
    let job = text(&jobs)
        .lines()
        .find(|line| line.contains("root-boot-check"))
        .expect("the check Job is kept for its log")
        .to_string();
    text(&run(
        "kubectl",
        &["--context", context, "-n", release_namespace, "logs", &job],
    ))
}

#[test]
#[ignore = "requires a Kind cluster named by ALIEN_ROOT_BOOT_CHECK_KIND_CONTEXT"]
fn the_install_checks_that_the_sandbox_namespace_admits_root_boot() {
    let context = std::env::var("ALIEN_ROOT_BOOT_CHECK_KIND_CONTEXT")
        .expect("ALIEN_ROOT_BOOT_CHECK_KIND_CONTEXT names a disposable cluster");
    assert!(context.starts_with("kind-"), "refusing a non-kind context");
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path());
    let chart = directory.path().to_str().expect("path");
    let kubectl = |arguments: &[&str]| {
        let mut full = vec!["--context", context.as_str()];
        full.extend_from_slice(arguments);
        run("kubectl", &full)
    };
    for namespace in ["rbc-restricted", "rbc-baseline"] {
        let _ = kubectl(&["delete", "namespace", namespace, "--ignore-not-found"]);
        assert!(kubectl(&["create", "namespace", namespace])
            .status
            .success());
    }
    kubectl(&[
        "label",
        "namespace",
        "rbc-restricted",
        "pod-security.kubernetes.io/enforce=restricted",
    ]);
    kubectl(&[
        "label",
        "namespace",
        "rbc-baseline",
        "pod-security.kubernetes.io/enforce=baseline",
    ]);
    let uninstall = |namespace: &str| {
        let _ = run(
            "helm",
            &[
                "--kube-context",
                &context,
                "uninstall",
                RELEASE,
                "--namespace",
                namespace,
                "--no-hooks",
            ],
        );
        let _ = kubectl(&[
            "delete",
            "namespace",
            namespace,
            "--ignore-not-found",
            "--wait=false",
        ]);
    };

    // A restricted namespace refuses root-boot pods: the install fails and the check says why.
    let refused = install(&context, chart, "rbc-release-a", "rbc-restricted");
    assert!(refused.starts_with("ok=false"), "{refused}");
    let log = check_job_log(&context, "rbc-release-a");
    assert!(log.contains("sandbox.namespace=rbc-restricted"), "{log}");
    assert!(
        log.contains("pod-security.kubernetes.io/enforce=baseline"),
        "{log}"
    );
    assert!(log.contains("violates PodSecurity"), "{log}");
    uninstall("rbc-release-a");

    // A namespace that does not exist fails the install, naming it.
    let missing = install(&context, chart, "rbc-release-b", "rbc-missing");
    assert!(missing.starts_with("ok=false"), "{missing}");
    assert!(missing.contains("rbc-missing"), "{missing}");
    uninstall("rbc-release-b");

    // A baseline namespace passes the check and the install completes.
    let admitted = install(&context, chart, "rbc-release-c", "rbc-baseline");
    assert!(admitted.starts_with("ok=true"), "{admitted}");
    uninstall("rbc-release-c");
    let _ = kubectl(&["delete", "namespace", "rbc-restricted", "--wait=false"]);
}
