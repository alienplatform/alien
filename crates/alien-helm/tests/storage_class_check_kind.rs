//! The install-time check for a cluster without a default StorageClass, on a real API server.
//!
//! The Operator's identity claim binds only with a default StorageClass (EKS Auto Mode has none
//! until one is created). Without one the install fails at render time naming the missing
//! StorageClass and the values that fix it, instead of leaving the claim Pending forever.
//!
//! Needs a Kind cluster whose default StorageClass the test removes (use a disposable cluster):
//! `ALIEN_STORAGE_CHECK_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test storage_class_check_kind -- --ignored`

use alien_core::{Stack, StackSettings};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{fs, path::Path, process::Command};

const RELEASE: &str = "scheck";

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
    let stack = Stack::new(RELEASE.to_string()).build();
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

fn install(context: &str, chart: &str, namespace: &str, extra: &[&str]) -> String {
    let mut arguments = vec![
        "--kube-context",
        context,
        "install",
        RELEASE,
        chart,
        "--namespace",
        namespace,
        "--create-namespace",
        "--timeout",
        "15m",
        "--set",
        "logCollector.enabled=false",
        "--set",
        "heartbeat.collection.nodes.enabled=false",
        "--set",
        "runtime.cleanup.onUninstall.enabled=false",
        "--set-string",
        "management.defaultUrl=https://manager.example.test",
    ];
    arguments.extend_from_slice(extra);
    let output = run("helm", &arguments);
    format!("ok={}\n{}", output.status.success(), text(&output))
}

#[test]
#[ignore = "requires a disposable Kind cluster named by ALIEN_STORAGE_CHECK_KIND_CONTEXT"]
fn the_install_names_a_missing_default_storage_class() {
    let context = std::env::var("ALIEN_STORAGE_CHECK_KIND_CONTEXT")
        .expect("ALIEN_STORAGE_CHECK_KIND_CONTEXT names a disposable cluster");
    assert!(context.starts_with("kind-"), "refusing a non-kind context");
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path());
    let chart = directory.path().to_str().expect("path");
    let kubectl = |arguments: &[&str]| {
        let mut full = vec!["--context", context.as_str()];
        full.extend_from_slice(arguments);
        run("kubectl", &full)
    };
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
    // The cluster has no default StorageClass.
    let _ = kubectl(&["delete", "storageclass", "--all"]);

    let refused = install(&context, chart, "scheck-a", &[]);
    assert!(refused.starts_with("ok=false"), "{refused}");
    assert!(refused.contains("no default StorageClass"), "{refused}");
    assert!(
        refused.contains("runtime.data.persistence.storageClassName"),
        "{refused}"
    );
    assert!(
        kubectl(&["-n", "scheck-a", "get", "pvc", "-o", "name"])
            .stdout
            .is_empty(),
        "a refused install leaves no claim"
    );
    uninstall("scheck-a");

    // Naming a StorageClass that does not exist is refused, naming it.
    let missing = install(
        &context,
        chart,
        "scheck-b",
        &[
            "--set-string",
            "runtime.data.persistence.storageClassName=named",
        ],
    );
    assert!(missing.starts_with("ok=false"), "{missing}");
    assert!(missing.contains("\"named\""), "{missing}");
    assert!(
        missing.contains("no StorageClass with that name"),
        "{missing}"
    );
    uninstall("scheck-b");

    // Letting the chart create the class, or skipping the check, passes it.
    let created = install(
        &context,
        chart,
        "scheck-b2",
        &[
            "--set",
            "clusterBootstrap.storageClass.default.enabled=true",
            "--set-string",
            "clusterBootstrap.storageClass.default.name=made",
            "--set-string",
            "clusterBootstrap.storageClass.default.provisioner=kubernetes.io/no-provisioner",
        ],
    );
    assert!(!created.contains("no default StorageClass"), "{created}");
    uninstall("scheck-b2");
    let skipped = install(
        &context,
        chart,
        "scheck-b3",
        &["--set", "runtime.data.persistence.storageClassCheck=false"],
    );
    assert!(!skipped.contains("no default StorageClass"), "{skipped}");
    uninstall("scheck-b3");

    // A default StorageClass makes the plain install pass the check.
    let class = "apiVersion: storage.k8s.io/v1\nkind: StorageClass\nmetadata:\n  name: fast\n  annotations:\n    storageclass.kubernetes.io/is-default-class: \"true\"\nprovisioner: kubernetes.io/no-provisioner\n";
    let path = directory.path().join("class.yaml");
    fs::write(&path, class).expect("class file");
    assert!(kubectl(&["apply", "-f", path.to_str().expect("path")])
        .status
        .success());
    let plain = install(&context, chart, "scheck-c", &[]);
    assert!(!plain.contains("no default StorageClass"), "{plain}");
    uninstall("scheck-c");
    let _ = kubectl(&["delete", "storageclass", "fast"]);
}

/// Installers that can only act inside their namespace, on any cluster (read-only: `--dry-run=server`).
///
/// `ALIEN_STORAGE_CHECK_ADMIN_KUBECONFIG` is an admin kubeconfig, `ALIEN_STORAGE_CHECK_NS_KUBECONFIG`
/// a kubeconfig whose user has a Role in `ALIEN_STORAGE_CHECK_NAMESPACE` only (every namespaced
/// resource, nothing cluster-wide). `ALIEN_STORAGE_CHECK_FLIP_DEFAULT=<class>` makes the test remove that
/// default StorageClass's default annotation for the no-default case and restore it afterwards.
#[test]
#[ignore = "requires a cluster, an admin kubeconfig and a namespace-only kubeconfig"]
fn a_namespace_only_installer_and_a_bound_claim_do_not_list_storage_classes() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"));
    let admin = var("ALIEN_STORAGE_CHECK_ADMIN_KUBECONFIG");
    let restricted = var("ALIEN_STORAGE_CHECK_NS_KUBECONFIG");
    let namespace = var("ALIEN_STORAGE_CHECK_NAMESPACE");
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path());
    let chart = directory.path().to_str().expect("path");
    let dry_run = |kubeconfig: &str, extra: &[&str]| {
        let mut arguments = vec![
            "--kubeconfig",
            kubeconfig,
            "install",
            RELEASE,
            chart,
            "--namespace",
            namespace.as_str(),
            "--dry-run=server",
            "--timeout",
            "15m",
            "--set",
            "logCollector.enabled=false",
            "--set",
            "heartbeat.collection.nodes.enabled=false",
            "--set",
            "runtime.cleanup.onUninstall.enabled=false",
            "--set-string",
            "management.defaultUrl=https://manager.example.test",
        ];
        arguments.extend_from_slice(extra);
        let output = run("helm", &arguments);
        format!("ok={}\n{}", output.status.success(), text(&output))
    };
    let kubectl = |arguments: &[&str]| {
        let mut full = vec!["--kubeconfig", admin.as_str()];
        full.extend_from_slice(arguments);
        run("kubectl", &full)
    };

    // The claim is already bound (a Kubernetes object named as the chart names it): an upgrade by an
    // installer without cluster-wide read access reads no StorageClass.
    let rendered = run(
        "helm",
        &[
            "template",
            RELEASE,
            chart,
            "--namespace",
            &namespace,
            "--set-string",
            "management.defaultUrl=https://manager.example.test",
        ],
    );
    let rendered = text(&rendered);
    let claim_name = rendered
        .lines()
        .skip_while(|line| !line.starts_with("kind: PersistentVolumeClaim"))
        .find_map(|line| line.trim().strip_prefix("name: ").map(str::to_string))
        .expect("the rendered claim name");
    let claim = format!("apiVersion: v1\nkind: PersistentVolumeClaim\nmetadata:\n  name: {claim_name}\n  namespace: {namespace}\n  labels:\n    app.kubernetes.io/managed-by: Helm\n  annotations:\n    meta.helm.sh/release-name: {RELEASE}\n    meta.helm.sh/release-namespace: {namespace}\nspec:\n  accessModes: [ReadWriteOnce]\n  resources:\n    requests:\n      storage: 1Gi\n  volumeName: not-a-real-volume\n");
    let claim_path = directory.path().join("claim.yaml");
    fs::write(&claim_path, claim).expect("claim file");
    assert!(
        kubectl(&["apply", "-f", claim_path.to_str().expect("path")])
            .status
            .success()
    );
    let bound = dry_run(&restricted, &[]);
    let _ = kubectl(&[
        "-n",
        &namespace,
        "delete",
        "pvc",
        &claim_name,
        "--wait=false",
    ]);
    assert!(bound.starts_with("ok=true"), "{bound}");

    // A fresh claim by a namespace-only installer: Helm cannot skip a refused list, so the check is
    // switched off by value, and refuses without it.
    let refused = dry_run(&restricted, &[]);
    assert!(
        refused.starts_with("ok=false") && refused.contains("forbidden"),
        "{refused}"
    );
    let off = dry_run(
        &restricted,
        &["--set", "runtime.data.persistence.storageClassCheck=false"],
    );
    assert!(off.starts_with("ok=true"), "{off}");

    // A named class that does not exist is refused, naming it.
    let missing = dry_run(
        &admin,
        &[
            "--set-string",
            "runtime.data.persistence.storageClassName=no-such-class",
        ],
    );
    assert!(missing.starts_with("ok=false"), "{missing}");
    assert!(
        missing.contains("\"no-such-class\"") && missing.contains("no StorageClass with that name"),
        "{missing}"
    );

    // No default class: only when asked to flip one.
    if let Ok(class) = std::env::var("ALIEN_STORAGE_CHECK_FLIP_DEFAULT") {
        let key = "storageclass.kubernetes.io/is-default-class";
        assert!(kubectl(&[
            "annotate",
            "storageclass",
            &class,
            &format!("{key}=false"),
            "--overwrite"
        ])
        .status
        .success());
        let none = dry_run(&admin, &[]);
        let restored = kubectl(&[
            "annotate",
            "storageclass",
            &class,
            &format!("{key}=true"),
            "--overwrite",
        ]);
        assert!(
            restored.status.success(),
            "restore the default annotation on {class}"
        );
        assert!(
            none.starts_with("ok=false") && none.contains("no default StorageClass"),
            "{none}"
        );
    }
    let with_default = dry_run(&admin, &[]);
    assert!(with_default.starts_with("ok=true"), "{with_default}");
}
