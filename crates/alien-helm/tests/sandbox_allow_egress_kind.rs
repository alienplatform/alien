//! A Kubernetes `allow` sandbox is bounded by its NetworkPolicy and nothing else: the policy is
//! what keeps a session off node metadata, the node's credentials and the cluster's private
//! ranges. On a CNI that ignores NetworkPolicy those denials do nothing, so installing the chart
//! there must be refused before anything is provisioned.
//!
//! Runs against the disposable Kind cluster the `Kind lifecycle` workflow creates. Kind's default
//! CNI, kindnet, does not enforce NetworkPolicy, which is the cluster this test needs.

use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

const CLUSTER_CONTEXT: &str = "kind-alien-product-lifecycle";
const NAMESPACE: &str = "alien-allow-egress-ci";
const RELEASE: &str = "allow-egress";

/// The probe's subject pods request the runtime class sessions run in. Kind has no gVisor, so the
/// test registers the name against Kind's own `runc` handler: the probe then measures the network,
/// which is what this test is about, instead of failing on a missing runtime class.
const RUNTIME_CLASS: &str = r#"apiVersion: node.k8s.io/v1
kind: RuntimeClass
metadata:
  name: gvisor
handler: runc
"#;

struct Cleanup;

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("helm")
            .args([
                "uninstall",
                RELEASE,
                "--namespace",
                NAMESPACE,
                "--no-hooks",
                "--ignore-not-found",
            ])
            .output();
        let _ = Command::new("kubectl")
            .args([
                "delete",
                "namespace",
                NAMESPACE,
                "--ignore-not-found",
                "--wait=false",
            ])
            .output();
        // A failed hook phase leaves its cluster-scoped admission objects behind.
        for kind in [
            "validatingadmissionpolicy",
            "validatingadmissionpolicybinding",
        ] {
            if let Ok(listed) = Command::new("kubectl")
                .args(["get", kind, "-o", "name"])
                .output()
            {
                for name in String::from_utf8_lossy(&listed.stdout)
                    .lines()
                    .filter(|name| name.contains(&format!("{RELEASE}-sandbox-cni")))
                {
                    let _ = Command::new("kubectl")
                        .args(["delete", name, "--ignore-not-found"])
                        .output();
                }
            }
        }
        let _ = Command::new("kubectl")
            .args(["delete", "runtimeclass", "gvisor", "--ignore-not-found"])
            .output();
    }
}

#[test]
#[ignore = "requires the disposable Kind cluster configured by CI"]
fn allow_egress_install_is_refused_where_network_policy_is_not_enforced() {
    assert_eq!(
        std::env::var("ALIEN_RUN_ALLOW_EGRESS_KIND_E2E").as_deref(),
        Ok("1"),
        "set ALIEN_RUN_ALLOW_EGRESS_KIND_E2E=1 only for a disposable Kind cluster"
    );
    assert_eq!(
        kubectl(&["config", "current-context"]).trim(),
        CLUSTER_CONTEXT
    );
    let _cleanup = Cleanup;

    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path());
    apply(RUNTIME_CLASS);

    let install = Command::new("helm")
        .args([
            "install",
            RELEASE,
            directory.path().to_str().expect("chart path"),
            "--namespace",
            NAMESPACE,
            "--create-namespace",
            "--timeout",
            "6m",
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
            "management.defaultUrl=https://manager.example.test",
        ])
        .output()
        .expect("run helm install");
    let stdout = String::from_utf8_lossy(&install.stdout);
    let stderr = String::from_utf8_lossy(&install.stderr);
    assert!(
        !install.status.success(),
        "helm installed an `allow` sandbox on a cluster whose CNI does not enforce \
         NetworkPolicy, so its egress denials are not in force:\n{stdout}\n{stderr}"
    );
    assert!(
        stderr.contains("pre-install"),
        "the refusal must come from the install gate, not from rendering: {stderr}"
    );

    // The gate's own verdict: the denied subject reached the listener.
    let probe = kubectl(&["--namespace", NAMESPACE, "get", "jobs", "-o", "name"])
        .lines()
        .find(|job| job.ends_with("-sandbox-cni"))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("the NetworkPolicy enforcement probe must run: {stderr}"));
    let logs = kubectl(&["--namespace", NAMESPACE, "logs", &probe]);
    assert!(
        logs.contains("NetworkPolicy did not block traffic"),
        "the probe must refuse because traffic it denied got through:\n{logs}"
    );

    // Nothing behind the gate was provisioned: no Operator to create sessions, and no session
    // policy pretending to bound one.
    assert_eq!(
        kubectl(&["--namespace", NAMESPACE, "get", "deployments", "-o", "name"]).trim(),
        ""
    );
    assert_eq!(
        kubectl(&[
            "--namespace",
            NAMESPACE,
            "get",
            "networkpolicies",
            "-o",
            "name"
        ])
        .trim(),
        ""
    );
}

fn write_chart(directory: &Path) {
    let stack = Stack::new(RELEASE.to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(300),
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
    .expect("an `allow` sandbox stack renders a chart");
    for (path, contents) in chart.files {
        let destination = directory.join(path);
        fs::create_dir_all(destination.parent().expect("chart parent")).expect("chart directory");
        fs::write(destination, contents).expect("chart file");
    }
}

fn apply(manifest: &str) {
    let mut child = Command::new("kubectl")
        .args(["apply", "-f", "-"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("run kubectl apply");
    child
        .stdin
        .take()
        .expect("kubectl stdin")
        .write_all(manifest.as_bytes())
        .expect("write manifest");
    let status = child.wait().expect("wait for kubectl apply");
    assert!(status.success(), "kubectl apply failed:\n{manifest}");
}

fn kubectl(arguments: &[&str]) -> String {
    let output = Command::new("kubectl")
        .args(arguments)
        .output()
        .expect("run kubectl");
    assert!(
        output.status.success(),
        "kubectl {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("kubectl output")
}
