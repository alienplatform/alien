//! Release signing in a real cluster: the signing ConfigMaps survive `helm uninstall`, the
//! Operator's ServiceAccount can raise the ledger but not rewrite the settings (R14), and a
//! protected chart refuses values that pick another Operator build (R12). Each attack runs
//! against an unprotected chart too, where it succeeds.
//!
//! Run with `scripts/helm-signing-kind/run.sh`, which creates a disposable kind cluster, sets
//! `ALIEN_HELM_TEST_KUBECONFIG`, and deletes the cluster afterwards.

use alien_core::{Stack, StackSettings, StackSigning};
use alien_helm::{
    generate_helm_chart, generate_signed_helm_chart, HelmOptions, HelmRegistry, SignedChartPins,
    LEDGER_KEY, SETTINGS_KEY,
};
use alien_signing::{Ledger, SigningSettings};
use std::{fs, path::Path, process::Command};

const FINGERPRINT: &str = "SHA256:9f2c3b1a00000000000000000000000000000000000000000000000000000f1a";
const OTHER_FINGERPRINT: &str =
    "SHA256:1111111111111111111111111111111111111111111111111111111111111111";
const OPERATOR_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const UNSIGNED_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RELEASE: &str = "shop";
/// Ends the template's quoted image string and comments out the rest of the line.
const EVIL_REPOSITORY: &str = "evil.example/operator:unsigned\" #";

struct Cluster {
    kubeconfig: String,
}

impl Cluster {
    fn from_env() -> Self {
        Self {
            kubeconfig: std::env::var("ALIEN_HELM_TEST_KUBECONFIG")
                .expect("ALIEN_HELM_TEST_KUBECONFIG for a disposable cluster"),
        }
    }

    fn command(&self, program: &str, arguments: &[&str]) -> std::process::Output {
        Command::new(program)
            .args(arguments)
            .env("KUBECONFIG", &self.kubeconfig)
            .output()
            .unwrap_or_else(|error| panic!("run {program}: {error}"))
    }

    /// Run and require success; returns stdout.
    fn ok(&self, program: &str, arguments: &[&str]) -> String {
        let output = self.command(program, arguments);
        assert!(
            output.status.success(),
            "{program} {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 output")
    }

    /// Run and require a failure whose stderr contains `reason`; returns stderr.
    fn refused(&self, program: &str, arguments: &[&str], reason: &str) -> String {
        let output = self.command(program, arguments);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            !output.status.success(),
            "{program} {arguments:?} succeeded but must be refused"
        );
        assert!(
            stderr.to_lowercase().contains(&reason.to_lowercase()),
            "{program} {arguments:?}: {stderr}"
        );
        stderr
    }

    fn json(&self, arguments: &[&str]) -> serde_json::Value {
        let mut all = arguments.to_vec();
        all.extend(["-o", "json"]);
        serde_json::from_str(&self.ok("kubectl", &all)).expect("kubectl JSON")
    }

    fn delete_namespace(&self, namespace: &str) {
        self.ok(
            "kubectl",
            &[
                "delete",
                "namespace",
                namespace,
                "--ignore-not-found",
                "--wait=true",
            ],
        );
    }
}

fn write_chart(directory: &Path, protected: bool) {
    let stack = Stack::new("signing-test".to_string()).build();
    let registry = HelmRegistry::built_in();
    let options = |stack_settings| HelmOptions {
        registry: &registry,
        stack_settings,
        chart_name: "signing-test".to_string(),
    };
    let chart = if protected {
        let settings = StackSettings {
            signing: Some(StackSigning {
                private_input_custody: false,
                private_machine_api_url: None,
                fingerprint: FINGERPRINT.to_string(),
                channel: "production".to_string(),
            }),
            ..StackSettings::default()
        };
        generate_signed_helm_chart(
            &stack,
            options(settings),
            &SignedChartPins {
                operator_digest: OPERATOR_DIGEST.to_string(),
            },
        )
    } else {
        generate_helm_chart(&stack, options(StackSettings::default()))
    }
    .expect("generate chart");
    for (path, contents) in chart.files {
        let destination = directory.join(path);
        fs::create_dir_all(destination.parent().expect("chart parent")).expect("chart directory");
        fs::write(destination, contents).expect("chart file");
    }
}

/// Install arguments that let the chart install without a manager: a token, and no Pod log
/// collection.
fn install<'a>(chart: &'a str, namespace: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut arguments = vec![
        "upgrade",
        "--install",
        RELEASE,
        chart,
        "-n",
        namespace,
        "--create-namespace",
        "--set",
        "management.token=test-token",
        "--set",
        "management.defaultUrl=https://manager.example.com",
    ];
    arguments.extend_from_slice(extra);
    arguments
}

fn operator_account(cluster: &Cluster, namespace: &str) -> String {
    let deployment = cluster.json(&[
        "-n",
        namespace,
        "get",
        "deployments",
        "-l",
        &format!("app.kubernetes.io/instance={RELEASE}"),
    ]);
    let name = deployment["items"][0]["spec"]["template"]["spec"]["serviceAccountName"]
        .as_str()
        .expect("Operator ServiceAccount");
    format!("system:serviceaccount:{namespace}:{name}")
}

fn operator_image(cluster: &Cluster, namespace: &str) -> String {
    let deployment = cluster.json(&[
        "-n",
        namespace,
        "get",
        "deployments",
        "-l",
        &format!("app.kubernetes.io/instance={RELEASE}"),
    ]);
    deployment["items"][0]["spec"]["template"]["spec"]["containers"][0]["image"]
        .as_str()
        .expect("Operator image")
        .to_string()
}

fn ledger_json(cluster: &Cluster, namespace: &str) -> String {
    let config_map = cluster.json(&[
        "-n",
        namespace,
        "get",
        "configmap",
        &format!("{RELEASE}-signing-ledger"),
    ]);
    config_map["data"][LEDGER_KEY]
        .as_str()
        .expect("ledger entry")
        .to_string()
}

/// Replace the ledger ConfigMap's entry as `account`, the way the Operator raises it
/// (`update`, with the resourceVersion it read).
fn replace_ledger(
    cluster: &Cluster,
    namespace: &str,
    account: &str,
    ledger: &str,
) -> std::process::Output {
    let mut config_map = cluster.json(&[
        "-n",
        namespace,
        "get",
        "configmap",
        &format!("{RELEASE}-signing-ledger"),
    ]);
    config_map["data"][LEDGER_KEY] = ledger.into();
    replace_as(cluster, namespace, account, &config_map)
}

/// `kubectl replace` an object as `account` (an `update`).
fn replace_as(
    cluster: &Cluster,
    namespace: &str,
    account: &str,
    object: &serde_json::Value,
) -> std::process::Output {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    fs::write(file.path(), object.to_string()).expect("write object");
    cluster.command(
        "kubectl",
        &[
            "-n",
            namespace,
            "replace",
            "--as",
            account,
            "-f",
            file.path().to_str().unwrap(),
        ],
    )
}

#[test]
#[ignore = "requires a disposable cluster: scripts/helm-signing-kind/run.sh"]
fn protected_chart_keeps_signing_state_and_locks_the_operator() {
    let cluster = Cluster::from_env();
    let namespace = "signing-protected";
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path(), true);
    let chart = directory.path().to_str().unwrap();
    cluster.delete_namespace(namespace);

    cluster.ok(
        "helm",
        &["lint", chart, "--set", "management.token=test-token"],
    );
    cluster.ok("helm", &install(chart, namespace, &[]));

    // The settings and ledger read back through the parsers every enforcer uses.
    let settings = cluster.json(&[
        "-n",
        namespace,
        "get",
        "configmap",
        &format!("{RELEASE}-signing"),
    ]);
    let parsed = SigningSettings::parse(
        settings["data"][SETTINGS_KEY]
            .as_str()
            .expect("settings entry")
            .as_bytes(),
        RELEASE,
    )
    .expect("settings parse");
    assert_eq!(
        parsed,
        SigningSettings {
            private_input_custody: false,
            private_machine_api_url: None,
            fingerprint: FINGERPRINT.to_string(),
            prefix: RELEASE.to_string(),
            channel: "production".to_string(),
        }
    );
    let ledger =
        Ledger::parse(ledger_json(&cluster, namespace).as_bytes(), RELEASE).expect("ledger parse");
    assert_eq!(ledger, Ledger::new(RELEASE));
    assert_eq!(
        operator_image(&cluster, namespace),
        format!("registry.example.com/deployment/operator@{OPERATOR_DIGEST}")
    );

    // R14 on: the Operator's ServiceAccount can't rewrite its settings, by any verb.
    let account = operator_account(&cluster, namespace);
    let settings_name = format!("{RELEASE}-signing");
    for verb in ["update", "patch", "delete"] {
        let allowed = cluster.command(
            "kubectl",
            &[
                "-n",
                namespace,
                "auth",
                "can-i",
                verb,
                &format!("configmap/{settings_name}"),
                "--as",
                &account,
            ],
        );
        assert_eq!(
            String::from_utf8_lossy(&allowed.stdout).trim(),
            "no",
            "{verb}"
        );
    }
    let stderr = cluster.refused(
        "kubectl",
        &[
            "-n",
            namespace,
            "patch",
            "configmap",
            &settings_name,
            "--as",
            &account,
            "--type",
            "merge",
            "-p",
            &format!(
                r#"{{"data":{{"{SETTINGS_KEY}":"{{\"fingerprint\":\"{OTHER_FINGERPRINT}\"}}"}}}}"#
            ),
        ],
        "Forbidden",
    );
    assert!(
        stderr.contains("cannot patch resource \"configmaps\""),
        "{stderr}"
    );
    cluster.refused(
        "kubectl",
        &[
            "-n",
            namespace,
            "create",
            "configmap",
            "planted",
            "--from-literal=a=b",
            "--as",
            &account,
        ],
        "Forbidden",
    );

    // The ledger: the Operator raises it with update, and nothing else.
    let mut raised = Ledger::new(RELEASE);
    raised.fingerprint = Some(FINGERPRINT.to_string());
    let written = replace_ledger(&cluster, namespace, &account, &raised.to_json());
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    cluster.refused(
        "kubectl",
        &[
            "-n",
            namespace,
            "delete",
            "configmap",
            &format!("{RELEASE}-signing-ledger"),
            "--as",
            &account,
        ],
        "Forbidden",
    );
    // The install id's cluster part: kube-system's UID, and nothing about other namespaces.
    cluster.ok(
        "kubectl",
        &["get", "namespace", "kube-system", "--as", &account],
    );
    cluster.ok(
        "kubectl",
        &["get", "namespace", namespace, "--as", &account],
    );
    cluster.refused(
        "kubectl",
        &["get", "namespace", "default", "--as", &account],
        "Forbidden",
    );

    // An upgrade keeps what the Operator raised.
    cluster.ok("helm", &install(chart, namespace, &["--reuse-values"]));
    assert_eq!(ledger_json(&cluster, namespace), raised.to_json());

    // R12 on: values that pick another Operator build, or change the settings, are refused
    // before anything changes.
    for (value, reason) in [
        (
            format!("runtime.image.tag=latest@{UNSIGNED_DIGEST}"),
            "runtime.image.tag",
        ),
        // A repository that ends the quoted image string and comments out the digest.
        (
            format!("runtime.image.repository={EVIL_REPOSITORY}"),
            "runtime.image.repository",
        ),
        (
            format!("logCollector.image.repository={EVIL_REPOSITORY}"),
            "logCollector.image.repository",
        ),
        (
            format!("runtime.cleanup.onUninstall.image.repository={EVIL_REPOSITORY}"),
            "runtime.cleanup.onUninstall.image.repository",
        ),
        (
            format!("runtime.image.repository=evil.example/operator@{UNSIGNED_DIGEST}"),
            "runtime.image.repository",
        ),
        (
            "runtime.image.tag=unsigned".to_string(),
            "runtime.image.tag",
        ),
        (
            "runtime.cleanup.onUninstall.image.tag=1.32.0".to_string(),
            "tag",
        ),
        ("logCollector.image.tag=3.2".to_string(), "tag"),
        (
            "clusterBootstrap.metricsServer.image=registry.example.com/metrics:v1".to_string(),
            "image",
        ),
        (
            format!("signing.fingerprint={OTHER_FINGERPRINT}"),
            "signing.fingerprint",
        ),
    ] {
        cluster.refused(
            "helm",
            &install(chart, namespace, &["--reuse-values", "--set", &value]),
            reason,
        );
    }
    cluster.refused(
        "helm",
        &install(
            chart,
            namespace,
            &[
                "--reuse-values",
                "--set-json",
                r#"serviceAccounts={"manager":{"rbac":{"rules":[{"apiGroups":[""],"resources":["configmaps"],"verbs":["*"]}]}}}"#,
            ],
        ),
        "serviceAccounts",
    );
    assert_eq!(
        operator_image(&cluster, namespace),
        format!("registry.example.com/deployment/operator@{OPERATOR_DIGEST}")
    );
    // A mirror serves the same digest: the registry and tag label may change.
    cluster.ok(
        "helm",
        &install(
            chart,
            namespace,
            &[
                "--reuse-values",
                "--set",
                "runtime.image.repository=mirror.example.com/operator",
                "--set",
                &format!("runtime.image.tag=1.4.0@{OPERATOR_DIGEST}"),
            ],
        ),
    );
    assert_eq!(
        operator_image(&cluster, namespace),
        format!("mirror.example.com/operator@{OPERATOR_DIGEST}")
    );

    // Uninstall keeps both signing ConfigMaps; a reinstall adopts them with the ledger intact.
    cluster.ok("helm", &["uninstall", RELEASE, "-n", namespace, "--wait"]);
    let kept = cluster.json(&["-n", namespace, "get", "configmaps"]);
    let names: Vec<&str> = kept["items"]
        .as_array()
        .expect("ConfigMaps")
        .iter()
        .filter_map(|item| item["metadata"]["name"].as_str())
        .collect();
    assert!(names.contains(&settings_name.as_str()), "{names:?}");
    assert!(
        names.contains(&format!("{RELEASE}-signing-ledger").as_str()),
        "{names:?}"
    );
    assert_eq!(ledger_json(&cluster, namespace), raised.to_json());
    cluster.ok("helm", &install(chart, namespace, &[]));
    assert_eq!(ledger_json(&cluster, namespace), raised.to_json());

    cluster.ok("helm", &["uninstall", RELEASE, "-n", namespace, "--wait"]);
    cluster.delete_namespace(namespace);
}

#[test]
#[ignore = "requires a disposable cluster: scripts/helm-signing-kind/run.sh"]
fn unprotected_chart_leaves_the_operator_and_its_image_open() {
    let cluster = Cluster::from_env();
    let namespace = "signing-unprotected";
    let directory = tempfile::tempdir().expect("chart directory");
    write_chart(directory.path(), false);
    let chart = directory.path().to_str().unwrap();
    cluster.delete_namespace(namespace);

    // R12 off: values pick an unsigned Operator build and Helm installs it.
    cluster.ok(
        "helm",
        &install(
            chart,
            namespace,
            &[
                "--set",
                "runtime.image.repository=attacker.example.com/operator",
                "--set",
                "runtime.image.tag=unsigned",
            ],
        ),
    );
    assert_eq!(
        operator_image(&cluster, namespace),
        "attacker.example.com/operator:unsigned"
    );
    // The same through the repository alone: the value ends the quoted string.
    cluster.ok(
        "helm",
        &install(
            chart,
            namespace,
            &[
                "--set",
                &format!("runtime.image.repository={EVIL_REPOSITORY}"),
            ],
        ),
    );
    assert_eq!(
        operator_image(&cluster, namespace),
        "evil.example/operator:unsigned"
    );

    // R14 off: the Operator's ServiceAccount may rewrite a ConfigMap of that name.
    let account = operator_account(&cluster, namespace);
    cluster.ok(
        "kubectl",
        &[
            "-n",
            namespace,
            "create",
            "configmap",
            &format!("{RELEASE}-signing"),
            &format!("--from-literal={SETTINGS_KEY}={{}}"),
        ],
    );
    cluster.ok(
        "kubectl",
        &[
            "-n",
            namespace,
            "patch",
            "configmap",
            &format!("{RELEASE}-signing"),
            "--as",
            &account,
            "--type",
            "merge",
            "-p",
            &format!(
                r#"{{"data":{{"{SETTINGS_KEY}":"{{\"fingerprint\":\"{OTHER_FINGERPRINT}\"}}"}}}}"#
            ),
        ],
    );

    cluster.ok("helm", &["uninstall", RELEASE, "-n", namespace, "--wait"]);
    cluster.delete_namespace(namespace);
}

