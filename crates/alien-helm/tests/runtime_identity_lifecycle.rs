//! Real Helm upgrades against an explicitly selected disposable cluster.
//! The workload fixture verifies persisted state and its encryption-key fingerprint;
//! registration and release reconciliation are covered by Operator integration tests.

use alien_core::{Stack, StackSettings};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{fs, path::Path, process::Command};

fn run(program: &str, arguments: &[&str], kubeconfig: &str) -> String {
    let output = Command::new(program)
        .args(arguments)
        .env("KUBECONFIG", kubeconfig)
        .output()
        .expect("run lifecycle command");
    assert!(
        output.status.success(),
        "{program} {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("command output")
}

fn rejected(arguments: &[&str], kubeconfig: &str, reason: &str) {
    let output = Command::new("helm")
        .args(arguments)
        .env("KUBECONFIG", kubeconfig)
        .output()
        .expect("run rejected Helm upgrade");
    assert!(!output.status.success(), "unsafe upgrade succeeded");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(reason),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_chart(directory: &Path) {
    let stack = Stack::new("identity-test".to_string()).build();
    let registry = HelmRegistry::built_in();
    let chart = generate_helm_chart(
        &stack,
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: "identity-test".to_string(),
        },
    )
    .expect("generate runtime chart");
    for (path, mut contents) in chart.files {
        if path == "templates/deployment.yaml" {
            // Run a bounded state fixture through the real generated Pod volumes.
            contents = contents.replace(
                "          imagePullPolicy:",
                r#"          command: ["/bin/sh", "-ec"]
          args:
            - |
              state=/var/lib/deployment-operator
              key=$(sha256sum /etc/deployment/secrets/encryption-key | cut -d ' ' -f 1)
              if [ -f "$state/key-fingerprint" ]; then
                test "$(cat "$state/key-fingerprint")" = "$key"
              else
                printf '%s' "$key" > "$state/key-fingerprint"
                cat /proc/sys/kernel/random/uuid > "$state/identity"
              fi
              echo "identity=$(cat "$state/identity")"
              sleep 3600
          imagePullPolicy:"#,
            );
        }
        let destination = directory.join(path);
        fs::create_dir_all(destination.parent().expect("chart parent")).expect("chart directory");
        fs::write(destination, contents).expect("chart file");
    }
}

#[test]
#[ignore = "requires kind-helm-defaults-1060 and ALIEN_HELM_TEST_KUBECONFIG"]
fn runtime_identity_survives_upgrade_and_pod_replacement() {
    let kubeconfig =
        std::env::var("ALIEN_HELM_TEST_KUBECONFIG").expect("disposable cluster kubeconfig");
    assert_eq!(
        run("kubectl", &["config", "current-context"], &kubeconfig).trim(),
        "kind-helm-defaults-1060"
    );
    let directory = tempfile::tempdir().expect("chart temporary directory");
    write_chart(directory.path());
    let chart = directory.path().to_str().expect("chart path");
    let namespace = "runtime-identity-test";
    let release = "runtime-identity";
    let name = "runtime-identity";
    let common = [
        "--namespace",
        namespace,
        "--wait",
        "--timeout",
        "90s",
        "--set-string",
        "runtime.image.repository=busybox",
        "--set-string",
        "runtime.image.tag=1.37.0",
        "--set",
        "runtime.probes.liveness.enabled=false",
        "--set",
        "runtime.probes.readiness.enabled=false",
        "--set",
        "heartbeat.collection.nodes.enabled=false",
        "--set",
        "runtime.cleanup.onUninstall.enabled=false",
        "--set-string",
        "management.defaultUrl=https://default-manager.example.test",
    ];
    let mut install = vec!["install", release, chart, "--create-namespace"];
    install.extend(common);
    run("helm", &install, &kubeconfig);
    let state = || {
        run(
            "kubectl",
            &[
                "--namespace",
                namespace,
                "exec",
                &format!("deployment/{name}"),
                "--",
                "cat",
                "/var/lib/deployment-operator/identity",
            ],
            &kubeconfig,
        )
    };
    let key = || {
        run(
            "kubectl",
            &[
                "--namespace",
                namespace,
                "get",
                "secret",
                name,
                "-o",
                "jsonpath={.data.encryption-key}",
            ],
            &kubeconfig,
        )
    };
    let original_identity = state();
    let original_key = key();
    let mut upgrade = vec!["upgrade", release, chart];
    upgrade.extend(common);
    // A different annotation forces a replacement Pod rather than a no-op upgrade.
    upgrade.extend(["--set-string", "runtime.podAnnotations.revision=second"]);
    upgrade.extend([
        "--set-string",
        "management.url=https://corrected-manager.example.test",
    ]);
    run("helm", &upgrade, &kubeconfig);
    let deployment: serde_json::Value = serde_json::from_str(&run(
        "kubectl",
        &[
            "--namespace",
            namespace,
            "get",
            "deployment",
            name,
            "-o",
            "json",
        ],
        &kubeconfig,
    ))
    .expect("installed Deployment");
    let variables = deployment["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .expect("runtime environment");
    assert_eq!(
        variables
            .iter()
            .find(|variable| variable["name"] == "SYNC_URL")
            .expect("management endpoint")["value"],
        "https://corrected-manager.example.test"
    );
    assert_eq!(state(), original_identity);
    assert_eq!(key(), original_key);
    let mut preserve_endpoint = vec!["upgrade", release, chart, "--reset-values"];
    preserve_endpoint.extend(common);
    preserve_endpoint.extend([
        "--set-string",
        "management.defaultUrl=https://new-default-manager.example.test",
        "--set-string",
        "runtime.podAnnotations.revision=third",
    ]);
    run("helm", &preserve_endpoint, &kubeconfig);
    let deployment: serde_json::Value = serde_json::from_str(&run(
        "kubectl",
        &[
            "--namespace",
            namespace,
            "get",
            "deployment",
            name,
            "-o",
            "json",
        ],
        &kubeconfig,
    ))
    .expect("installed Deployment after clearing the override");
    assert_eq!(
        deployment["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .expect("runtime environment")
            .iter()
            .find(|variable| variable["name"] == "SYNC_URL")
            .expect("preserved management endpoint")["value"],
        "https://corrected-manager.example.test"
    );
    assert_eq!(state(), original_identity);
    assert_eq!(key(), original_key);
    run(
        "kubectl",
        &[
            "--namespace",
            namespace,
            "rollout",
            "restart",
            &format!("deployment/{name}"),
        ],
        &kubeconfig,
    );
    run(
        "kubectl",
        &[
            "--namespace",
            namespace,
            "rollout",
            "status",
            &format!("deployment/{name}"),
            "--timeout=90s",
        ],
        &kubeconfig,
    );
    assert_eq!(state(), original_identity);
    assert_eq!(key(), original_key);
    let mut unsafe_upgrade = upgrade.clone();
    unsafe_upgrade.extend([
        "--set-string",
        "runtime.encryption.key=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ]);
    rejected(
        &unsafe_upgrade,
        &kubeconfig,
        "Changing the runtime encryption key",
    );
    let mut unsafe_upgrade = upgrade.clone();
    unsafe_upgrade.extend(["--set", "runtime.data.persistence.enabled=false"]);
    rejected(
        &unsafe_upgrade,
        &kubeconfig,
        "Changing the runtime identity volume",
    );
    let mut unsafe_upgrade = upgrade.clone();
    unsafe_upgrade.extend([
        "--set-string",
        "runtime.encryption.existingSecret.name=runtime-identity",
        "--set-string",
        "runtime.encryption.existingSecret.key=another-key",
    ]);
    rejected(
        &unsafe_upgrade,
        &kubeconfig,
        "Preserve the installed key selector",
    );
    assert_eq!(state(), original_identity);
    assert_eq!(key(), original_key);
    run(
        "helm",
        &[
            "rollback",
            release,
            "1",
            "--namespace",
            namespace,
            "--wait",
            "--timeout",
            "90s",
        ],
        &kubeconfig,
    );
    assert_eq!(state(), original_identity);
    assert_eq!(key(), original_key);
    let logs = run(
        "kubectl",
        &[
            "--namespace",
            namespace,
            "logs",
            &format!("deployment/{name}"),
        ],
        &kubeconfig,
    );
    assert!(logs.contains(original_identity.trim()));
    run(
        "helm",
        &["uninstall", release, "--namespace", namespace, "--wait"],
        &kubeconfig,
    );
    run(
        "kubectl",
        &[
            "delete",
            "namespace",
            namespace,
            "--wait=true",
            "--timeout=90s",
        ],
        &kubeconfig,
    );
}
