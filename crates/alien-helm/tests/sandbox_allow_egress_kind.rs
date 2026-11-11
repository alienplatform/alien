//! A Kubernetes `allow` sandbox is bounded by its NetworkPolicy and nothing else: the policy is
//! what keeps a session off node metadata, the node's credentials and the cluster's private
//! ranges. On a CNI that ignores NetworkPolicy those denials do nothing, so installing the chart
//! there must be refused before anything is provisioned.
//!
//! Both tests run in the `Kind lifecycle` workflow, each against its own disposable cluster:
//! - the refusal on a single-node cluster whose CNI is plain `ptp` with no policy controller, so
//!   NetworkPolicy objects are accepted and ignored;
//! - the admission on the workflow's main cluster, whose kindnet enforces NetworkPolicy.

use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

/// The cluster whose CNI ignores NetworkPolicy.
const UNENFORCED_CONTEXT: &str = "kind-alien-allow-egress-unenforced";
/// The workflow's main cluster, whose CNI enforces NetworkPolicy.
const ENFORCED_CONTEXT: &str = "kind-alien-product-lifecycle";
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
    let (_cleanup, install) = install_allow_chart(UNENFORCED_CONTEXT);
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

/// The gate must not refuse a cluster that does enforce NetworkPolicy: the probe runs, passes,
/// and the session policy is installed.
#[test]
#[ignore = "requires the disposable Kind cluster configured by CI"]
fn allow_egress_install_proceeds_where_network_policy_is_enforced() {
    let (_cleanup, install) = install_allow_chart(ENFORCED_CONTEXT);
    assert!(
        install.status.success(),
        "an enforcing cluster must accept an `allow` sandbox:\n{}\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    // A passing hook Job is deleted with the hook; its completion event remains.
    let job_events = kubectl(&[
        "--namespace",
        NAMESPACE,
        "get",
        "events",
        "--field-selector",
        "involvedObject.kind=Job",
        "-o",
        "jsonpath={range .items[*]}{.involvedObject.name} {.reason}{\"\\n\"}{end}",
    ]);
    assert!(
        job_events
            .lines()
            .any(|line| line.ends_with("-sandbox-cni Completed")),
        "the NetworkPolicy enforcement probe must have run and passed:\n{job_events}"
    );
    kubectl(&[
        "--namespace",
        NAMESPACE,
        "get",
        "networkpolicy",
        "alien-sbx-agent",
    ]);
    resolves_names_but_reaches_no_other_private_host_on_53();
}

/// Install a chart with one `allow` sandbox into `context`, which must be the current context.
/// The returned guard removes everything the install created.
fn install_allow_chart(context: &str) -> (Cleanup, Output) {
    assert_eq!(
        std::env::var("ALIEN_RUN_ALLOW_EGRESS_KIND_E2E").as_deref(),
        Ok("1"),
        "set ALIEN_RUN_ALLOW_EGRESS_KIND_E2E=1 only for a disposable Kind cluster"
    );
    assert_eq!(kubectl(&["config", "current-context"]).trim(), context);
    let cleanup = Cleanup;

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
            "--set",
            "sandbox.clusterDNS={10.96.0.10}",
            "--set-string",
            "sandbox.runtimeClassName=gvisor",
        ])
        .output()
        .expect("run helm install");
    (cleanup, install)
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

/// A sandbox pod under the allow policy resolves names through CoreDNS, and a pod in the cluster
/// that merely listens on port 53 is not reachable. The CNI checks the policy after the Service
/// address is translated, so the DNS pod's own address is what has to be admitted.
fn resolves_names_but_reaches_no_other_private_host_on_53() {
    let pod = |name: &str, labels: &str, command: &str| {
        apply(&format!(
            r#"apiVersion: v1
kind: Pod
metadata:
  name: {name}
  namespace: {NAMESPACE}
  labels:
    {labels}
spec:
  automountServiceAccountToken: false
  containers:
    - name: p
      image: busybox:1.37
      command: ["sh", "-c", "{command}"]
"#
        ));
    };
    pod("sandbox-probe", "alien.dev/sandbox: agent", "sleep 600");
    pod("listener", "app: listener", "httpd -f -p 53");
    pod("plain", "app: plain", "sleep 600");
    for name in ["sandbox-probe", "listener", "plain"] {
        kubectl(&[
            "--namespace",
            NAMESPACE,
            "wait",
            "--for=condition=Ready",
            &format!("pod/{name}"),
            "--timeout=180s",
        ]);
    }
    // Let the CNI program the policy for the new pod.
    std::thread::sleep(std::time::Duration::from_secs(10));
    let exec = |pod: &str, command: &str| {
        Command::new("kubectl")
            .args([
                "--namespace",
                NAMESPACE,
                "exec",
                pod,
                "--",
                "sh",
                "-c",
                command,
            ])
            .output()
            .expect("run kubectl exec")
    };
    let lookup = exec(
        "sandbox-probe",
        "nslookup kubernetes.default.svc.cluster.local",
    );
    assert!(
        lookup.status.success(),
        "a sandbox pod must resolve names through the cluster's DNS:\n{}\n{}",
        String::from_utf8_lossy(&lookup.stdout),
        String::from_utf8_lossy(&lookup.stderr)
    );
    let listener = kubectl(&[
        "--namespace",
        NAMESPACE,
        "get",
        "pod",
        "listener",
        "-o",
        "jsonpath={.status.podIP}",
    ]);
    let reach = format!("nc -w 3 -z {listener} 53");
    assert!(
        exec("plain", &reach).status.success(),
        "the listener must be reachable by a pod the policy does not select"
    );
    assert!(
        !exec("sandbox-probe", &reach).status.success(),
        "a sandbox pod must not reach another pod on port 53"
    );
}
