//! An upgrade that would send the Operator's deployment token over plain HTTP fails while the
//! chart renders, before anything on the cluster changes: the release stays at its previous
//! revision and its Pods keep running.
//!
//! The install uses `management.allowInsecure=true`, as an install of an earlier chart (which had
//! no such check) behaves with an `http` URL. The upgrade drops it, as moving to this chart does.
//!
//! Run against a disposable cluster:
//! `ALIEN_MGMT_URL_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test management_url_upgrade_kind -- --ignored`

use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{fs, process::Command};

const RELEASE: &str = "mgmt-url";
const NAMESPACE: &str = "alien-mgmt-url-kind";

struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("helm")
            .args(["--kube-context", &self.0, "uninstall", RELEASE])
            .args(["--namespace", NAMESPACE, "--no-hooks", "--ignore-not-found"])
            .output();
        let _ = Command::new("kubectl")
            .args(["--context", &self.0, "delete", "namespace", NAMESPACE])
            .args(["--ignore-not-found", "--wait=false"])
            .output();
    }
}

fn output(program: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"))
}

fn ok(program: &str, arguments: &[&str]) -> String {
    let result = output(program, arguments);
    assert!(
        result.status.success(),
        "{program} {arguments:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
#[ignore = "requires a disposable cluster named by ALIEN_MGMT_URL_KIND_CONTEXT"]
fn an_upgrade_to_an_http_management_url_fails_before_the_cluster_changes() {
    let context = std::env::var("ALIEN_MGMT_URL_KIND_CONTEXT")
        .expect("ALIEN_MGMT_URL_KIND_CONTEXT names a disposable cluster");
    assert!(context.starts_with("kind-"), "refusing a non-kind context");
    let _cleanup = Cleanup(context.clone());

    let directory = tempfile::tempdir().unwrap();
    let stack = Stack::new(RELEASE.to_string())
        .add(
            Storage::new("data".to_string()).build(),
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
    .expect("generate the chart");
    for (path, contents) in chart.files {
        let destination = directory.path().join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, contents).unwrap();
    }
    let chart_path = directory.path().to_str().unwrap();
    let common = [
        "--kube-context",
        &context,
        "--namespace",
        NAMESPACE,
        "--set",
        "logCollector.enabled=false",
        "--set",
        "heartbeat.collection.nodes.enabled=false",
        "--set",
        "runtime.cleanup.onUninstall.enabled=false",
        "--set-string",
        "management.url=http://manager.example.test",
        "--set",
        "management.token=ax_dg_example",
    ];

    let mut install = vec!["install", RELEASE, chart_path, "--create-namespace"];
    install.extend_from_slice(&common);
    install.extend(["--set", "management.allowInsecure=true"]);
    ok("helm", &install);

    let pods = |context: &str| {
        ok(
            "kubectl",
            &[
                "--context",
                context,
                "--namespace",
                NAMESPACE,
                "get",
                "pods",
                "--output",
                "jsonpath={.items[*].metadata.uid}",
            ],
        )
    };
    let before = pods(&context);
    assert!(!before.is_empty(), "the install created no Pod");

    // The same URL, without the escape hatch.
    let mut upgrade = vec!["upgrade", RELEASE, chart_path];
    upgrade.extend_from_slice(&common);
    let refused = output("helm", &upgrade);
    assert!(
        !refused.status.success(),
        "the upgrade must fail while rendering"
    );
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("management.allowInsecure") && message.contains("https"),
        "the message names the escape hatch and https: {message}"
    );

    let history = ok(
        "helm",
        &[
            "--kube-context",
            &context,
            "--namespace",
            NAMESPACE,
            "history",
            RELEASE,
            "--output",
            "json",
        ],
    );
    let revisions: Vec<serde_json::Value> = serde_json::from_str(&history).unwrap();
    assert_eq!(revisions.len(), 1, "no new revision: {history}");
    assert_eq!(revisions[0]["status"], "deployed");
    assert_eq!(pods(&context), before, "the Pods are unchanged");
}
