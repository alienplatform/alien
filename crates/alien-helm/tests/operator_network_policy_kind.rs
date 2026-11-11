//! The chart's Operator NetworkPolicy, installed on a real cluster, still lets an Operator-labelled
//! Pod resolve names, reach the ports the values name (a database, the manager) and nothing else.
//! A policy that is only rendered can look right and cut the Operator off from its own database.
//!
//! The Pods here carry the labels the policy selects and use a small test image as client and
//! server: the point is what the policy lets through, not the Operator binary.
//!
//! Run against a disposable cluster whose CNI enforces NetworkPolicy:
//! `ALIEN_NETWORK_POLICY_KIND_CONTEXT=kind-<name> cargo test -p alien-helm --test operator_network_policy_kind -- --ignored`

use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_helm::{generate_helm_chart, HelmOptions, HelmRegistry};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const RELEASE: &str = "netpol";
const NAMESPACE: &str = "alien-netpol-kind";
const IMAGE: &str = "registry.k8s.io/e2e-test-images/agnhost:2.47";
/// Stands in for a database: a value in `runtime.networkPolicy.egress.ports`.
const DATABASE_PORT: u16 = 5432;
/// Not in any value: the policy must keep it closed.
const CLOSED_PORT: u16 = 9999;

fn run(context: &str, arguments: &[&str]) -> std::process::Output {
    Command::new("kubectl")
        .arg("--context")
        .arg(context)
        .args(["--namespace", NAMESPACE])
        .args(arguments)
        .output()
        .expect("run kubectl")
}

fn run_ok(context: &str, arguments: &[&str]) -> String {
    let output = run(context, arguments);
    assert!(
        output.status.success(),
        "kubectl {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

/// Whether `from` can open a TCP connection to `target`. agnhost `connect` exits 0 on success.
fn connects(context: &str, from: &str, target: &str) -> bool {
    run(
        context,
        &[
            "exec",
            from,
            "--",
            "/agnhost",
            "connect",
            target,
            "--timeout=4s",
        ],
    )
    .status
    .success()
}

fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_secs(2));
    }
}

struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("kubectl")
            .args(["--context", &self.0, "delete", "namespace", NAMESPACE])
            .args(["--ignore-not-found", "--wait=false"])
            .output();
    }
}

#[test]
#[ignore = "requires a disposable cluster named by ALIEN_NETWORK_POLICY_KIND_CONTEXT"]
fn the_operator_network_policy_keeps_dns_and_the_named_ports_open() {
    let context = std::env::var("ALIEN_NETWORK_POLICY_KIND_CONTEXT")
        .expect("ALIEN_NETWORK_POLICY_KIND_CONTEXT names a disposable cluster");
    assert!(
        context.starts_with("kind-"),
        "refusing to install into a non-kind context: {context}"
    );
    let _cleanup = Cleanup(context.clone());

    let directory = tempfile::tempdir().expect("chart directory");
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

    let _ = Command::new("kubectl")
        .args(["--context", &context, "create", "namespace", NAMESPACE])
        .output();

    // The policy exactly as the chart renders it, with the values an installer sets.
    let rendered = Command::new("helm")
        .args(["template", RELEASE])
        .arg(directory.path())
        .args(["--namespace", NAMESPACE])
        .args(["--show-only", "templates/networkpolicy.yaml"])
        .args(["--set", "runtime.networkPolicy.enabled=true"])
        .args([
            "--set",
            &format!("runtime.networkPolicy.egress.ports={{443,{DATABASE_PORT}}}"),
        ])
        .args(["--set", "management.token=ax_dg_example"])
        .args([
            "--set-string",
            "management.url=https://manager.example.test",
        ])
        .output()
        .expect("run helm template");
    assert!(
        rendered.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let policy: serde_yaml::Value =
        serde_yaml::from_slice(&rendered.stdout).expect("the NetworkPolicy parses");
    let labels = policy["spec"]["podSelector"]["matchLabels"]
        .as_mapping()
        .expect("the policy selects the Operator by labels");
    let label_arguments: Vec<String> = labels
        .iter()
        .map(|(key, value)| format!("{}={}", key.as_str().unwrap(), value.as_str().unwrap()))
        .collect();
    let apply = |manifest: &[u8]| {
        let mut child = Command::new("kubectl")
            .args(["--context", &context, "--namespace", NAMESPACE])
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("run kubectl apply");
        use std::io::Write;
        child.stdin.take().unwrap().write_all(manifest).unwrap();
        assert!(child.wait().unwrap().success(), "kubectl apply failed");
    };
    apply(&rendered.stdout);

    let pod = |name: &str, labels: &str, arguments: &[&str]| {
        let mut command = vec![
            "run",
            name,
            "--restart=Never",
            "--image",
            IMAGE,
            "--labels",
            labels,
            "--command",
            "--",
        ];
        command.extend_from_slice(arguments);
        run_ok(&context, &command);
    };
    // Servers: one on the database port, one on a port no value names.
    pod(
        "database",
        "role=database",
        &[
            "/agnhost",
            "netexec",
            &format!("--http-port={DATABASE_PORT}"),
        ],
    );
    pod(
        "closed",
        "role=closed",
        &["/agnhost", "netexec", &format!("--http-port={CLOSED_PORT}")],
    );
    // The Operator stand-in carries the labels the policy selects; the bystander does not.
    pod("operator", &label_arguments.join(","), &["sleep", "3600"]);
    pod("bystander", "role=bystander", &["sleep", "3600"]);
    for name in ["database", "closed", "operator", "bystander"] {
        run_ok(
            &context,
            &[
                "wait",
                "--for=condition=Ready",
                &format!("pod/{name}"),
                "--timeout=180s",
            ],
        );
    }
    for (name, port) in [("database", DATABASE_PORT), ("closed", CLOSED_PORT)] {
        run_ok(
            &context,
            &[
                "expose",
                "pod",
                name,
                "--port",
                &port.to_string(),
                "--name",
                name,
            ],
        );
    }

    let database = format!("database:{DATABASE_PORT}");
    let closed = format!("closed:{CLOSED_PORT}");

    // Control: a Pod the policy does not select is not restricted, so a failure below is the policy.
    eventually("the bystander reaches the closed port", || {
        connects(&context, "bystander", &closed)
    });

    // Names resolve (the Service name in the same namespace needs cluster DNS) and the
    // database port from the values is open.
    eventually(
        "the Operator cannot reach the database by name: DNS or the named port is blocked",
        || connects(&context, "operator", &database),
    );
    // Control: the policy is enforced on this cluster; a port no value names stays closed.
    assert!(
        !connects(&context, "operator", &closed),
        "the Operator reached a port the values do not name: the cluster does not enforce \
         NetworkPolicy, or the policy allows too much"
    );
}
