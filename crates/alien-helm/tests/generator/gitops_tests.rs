//! One install path for Helm, Flux and Argo CD.
//!
//! Argo CD renders offline (`helm template`), so a chart that generates a value at
//! render time would change it on every sync. The chart generates nothing: the
//! Operator creates the values it needs in a Secret it owns. Each test renders the
//! real chart.

use super::{helpers::render, test_utils};
use alien_core::{Postgres, ResourceLifecycle, Stack, StackSettings, Storage};
use serde::Deserialize;

fn chart_files() -> test_utils::LinterFiles {
    let stack = Stack::new("gitops-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Postgres::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    render(&stack, StackSettings::default()).files
}

const CODE_VALUES: &str = "management:\n  url: https://manager.example.com\n  enrollmentCode: ax_enroll_example\n  audience: https://manager.example.com\n  deploymentId: dep_example\nlogCollector:\n  enabled: true\n  mode: nodeAgent\npostgres:\n  metadata:\n    enabled: true\nruntime:\n  image:\n    pullWithManagementToken: true\n";

fn documents(manifest: &str) -> Vec<serde_yaml::Value> {
    serde_yaml::Deserializer::from_str(manifest)
        .filter_map(|document| serde_yaml::Value::deserialize(document).ok())
        .filter(|document| !document.is_null())
        .collect()
}

#[test]
fn two_offline_renders_with_the_same_values_are_identical() {
    let files = chart_files();
    for release in ["app", "another-release"] {
        let first = test_utils::helm_template_for_release(&files, Some(CODE_VALUES), release);
        first.assert_ok("offline render with a code");
        let second = test_utils::helm_template_for_release(&files, Some(CODE_VALUES), release);
        assert_eq!(first.stdout, second.stdout, "{release}: renders differ");
    }
    // An upgrade render without cluster access is just as stable.
    let upgrade_values =
        format!("{CODE_VALUES}logCollector:\n  scope:\n    deploymentLabelValue: gitops-app\n")
            .replace("logCollector:\n  enabled: true\n  mode: nodeAgent\n", "");
    let first = test_utils::helm_template_as_upgrade(&files, Some(&upgrade_values));
    first.assert_ok("offline upgrade render");
    assert_eq!(
        first.stdout,
        test_utils::helm_template_as_upgrade(&files, Some(&upgrade_values)).stdout
    );
}

#[test]
fn no_template_outside_a_hook_draws_a_random_value() {
    let files = chart_files();
    for (path, contents) in &files {
        if !path.starts_with("templates/") {
            continue;
        }
        for function in [
            "randAlphaNum",
            "randAlpha",
            "randNumeric",
            "randAscii",
            "randBytes",
            "uuidv4",
        ] {
            if !contents.contains(function) {
                continue;
            }
            // Only the Helm hook that proves the release history may draw one.
            assert!(
                contents.contains("helm.sh/hook"),
                "{path} draws a random value with {function} and is not a hook"
            );
        }
    }
}

#[test]
fn the_chart_renders_neither_operator_owned_secret() {
    let files = chart_files();
    let rendered = test_utils::helm_template_for_release(&files, Some(CODE_VALUES), "app");
    rendered.assert_ok("code install");
    let documents = documents(&rendered.stdout);
    let secret_names: Vec<&str> = documents
        .iter()
        .filter(|document| document["kind"] == "Secret")
        .map(|document| document["metadata"]["name"].as_str().unwrap())
        .collect();
    // The bootstrap pull Secret is rendered from the code; the Operator's refreshed pull
    // credential and its own Secret are written by the Operator, so a GitOps tool that
    // applies the chart again has nothing of theirs to overwrite.
    assert!(
        secret_names.contains(&"app-bootstrap-pull"),
        "{secret_names:?}"
    );
    assert!(!secret_names.contains(&"app-registry"), "{secret_names:?}");
    assert!(!secret_names.contains(&"app-operator"), "{secret_names:?}");
    for document in &documents {
        for key in ["encryption-key", "collector-token"] {
            assert!(document["stringData"][key].is_null(), "{key} is rendered");
            assert!(document["data"][key].is_null(), "{key} is rendered");
        }
    }
    let deployment = documents
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "app")
        .expect("Operator Deployment");
    let pod = &deployment["spec"]["template"]["spec"];
    let pull: Vec<&str> = pod["imagePullSecrets"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|reference| reference["name"].as_str().unwrap())
        .collect();
    assert_eq!(pull, ["app-bootstrap-pull", "app-registry"]);
    let env = |name: &str| -> Option<String> {
        pod["containers"][0]["env"]
            .as_sequence()?
            .iter()
            .find(|variable| variable["name"] == name)
            .and_then(|variable| variable["value"].as_str().map(str::to_string))
    };
    assert_eq!(
        env("OPERATOR_OWNED_SECRET").as_deref(),
        Some("app-operator")
    );
    assert_eq!(
        env("OPERATOR_REGISTRY_SECRET").as_deref(),
        Some("app-registry")
    );
    assert_eq!(
        env("OPERATOR_OWNED_SECRET_KEYS").as_deref(),
        Some("encryption-key=hex32,collector-token=token48,postgres-metadata-password=token48")
    );
    // The Operator reads its key from its Secret, not from a mounted file.
    assert_eq!(env("OPERATOR_ENCRYPTION_KEY_FILE"), None);
    // Self-update is off unless asked for.
    assert_eq!(env("OPERATOR_SELF_UPDATE_DEPLOYMENT"), None);
    // Every consumer waits on the Operator's Secret.
    for document in &documents {
        if !matches!(document["kind"].as_str(), Some("StatefulSet" | "DaemonSet")) {
            continue;
        }
        let reference = serde_yaml::to_string(document).unwrap();
        assert!(reference.contains("name: app-operator"), "{reference}");
    }
}

#[test]
fn the_code_can_come_from_a_secret_the_user_keeps_out_of_git() {
    let files = chart_files();
    let values = "management:\n  url: https://manager.example.com\n  enrollmentCodeSecret:\n    name: my-code\n    key: code\n  audience: https://manager.example.com\n  deploymentId: dep_example\n";
    let rendered = test_utils::helm_template_for_release(&files, Some(values), "app");
    rendered.assert_ok("code from a Secret");
    assert!(!rendered.stdout.contains("ax_enroll"));
    let documents = documents(&rendered.stdout);
    let deployment = documents
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "app")
        .unwrap();
    let pod = serde_yaml::to_string(&deployment["spec"]["template"]["spec"]).unwrap();
    assert!(pod.contains("secretName: my-code"), "{pod}");
    assert!(pod.contains("key: code"), "{pod}");
    assert!(pod.contains("OPERATOR_ENROLLMENT_CODE_FILE"), "{pod}");
    // A Secret reference is a credential like the code itself: it needs the deployment and audience.
    let incomplete = "management:\n  url: https://manager.example.com\n  enrollmentCodeSecret:\n    name: my-code\n";
    assert!(!test_utils::helm_template_for_release(&files, Some(incomplete), "app").is_ok());
}

#[test]
fn the_removed_install_inputs_are_not_in_the_shipped_values() {
    let files = chart_files();
    let values = &files["values.yaml"];
    for removed in [
        "gitops:",
        "existingSecret:\n    name: \"\"\n    tokenKey",
        "existingSecret:\n      name: \"\"\n      key: encryption-key",
    ] {
        assert!(!values.contains(removed), "values.yaml still has {removed}");
    }
    assert!(!files
        .keys()
        .any(|path| path.contains("gitops") && path.starts_with("templates/")));
}

fn yaml_documents(text: &str) -> Vec<serde_yaml::Value> {
    serde_yaml::Deserializer::from_str(text)
        .map(|document| serde_yaml::Value::deserialize(document).expect("valid YAML document"))
        .collect()
}

fn to_yaml(value: &serde_yaml::Value) -> String {
    serde_yaml::to_string(value).unwrap()
}

#[test]
fn the_flux_example_installs_with_its_own_values() {
    let chart = chart_files();
    let documents = yaml_documents(&chart["examples/gitops/flux.yaml"]);
    let kinds: Vec<_> = documents
        .iter()
        .map(|d| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["OCIRepository", "HelmRelease"]);
    assert_eq!(documents[0]["spec"]["ref"]["semver"], "1.x");
    let values = &documents[1]["spec"]["values"];
    assert_example_values_are_the_install_values(values);
    test_utils::helm_template(&chart, Some(&to_yaml(values))).assert_ok("flux example values");
}

#[test]
fn the_argo_example_installs_with_its_own_values() {
    let chart = chart_files();
    let documents = yaml_documents(&chart["examples/gitops/argocd.yaml"]);
    assert_eq!(documents.len(), 1);
    let application = &documents[0];
    assert_eq!(application["kind"], "Application");
    assert_eq!(application["spec"]["source"]["targetRevision"], "1.*");
    assert_eq!(
        application["spec"]["destination"]["namespace"],
        application["spec"]["source"]["chart"]
    );
    let values = &application["spec"]["source"]["helm"]["valuesObject"];
    assert_example_values_are_the_install_values(values);
    let rendered = test_utils::helm_template(&chart, Some(&to_yaml(values)));
    rendered.assert_ok("argo example values");
    // Argo CD renders on every sync: the same values must give the same manifests.
    assert_eq!(
        rendered.stdout,
        test_utils::helm_template(&chart, Some(&to_yaml(values))).stdout
    );
}

/// The GitOps examples set what any install sets (the manager URL, the enrollment code
/// and its deployment, the infrastructure) and nothing else: no mode, no pre-made Secrets.
fn assert_example_values_are_the_install_values(values: &serde_yaml::Value) {
    let keys: Vec<&str> = values
        .as_mapping()
        .unwrap()
        .keys()
        .map(|key| key.as_str().unwrap())
        .collect();
    assert_eq!(keys, ["management", "infrastructure"]);
    let management: Vec<&str> = values["management"]
        .as_mapping()
        .unwrap()
        .keys()
        .map(|key| key.as_str().unwrap())
        .collect();
    assert_eq!(
        management,
        ["url", "enrollmentCode", "deploymentId", "audience"]
    );
}

#[test]
fn the_schema_types_the_postgres_and_exposure_values() {
    use alien_core::{Postgres, ResourceLifecycle};
    let stack = Stack::new("typed-values".to_string())
        .add(
            Postgres::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default()).files;
    let base = "management:\n  token: test-credential\n";

    test_utils::helm_template(
        &chart,
        Some(&format!(
            "{base}postgres:\n  metadata:\n    enabled: true\n    storage: 20Gi\n    existingPasswordSecret: pg-password\n    image:\n      repository: pgvector/pgvector\n      tag: pg16-bookworm\n    recordedImage: pgvector/pgvector:pg16-bookworm\n"
        )),
    )
    .assert_ok("typed postgres values");

    let wrong_type = test_utils::helm_template(
        &chart,
        Some(&format!(
            "{base}postgres:\n  metadata:\n    enabled: \"yes\"\n"
        )),
    );
    assert!(!wrong_type.is_ok(), "{}", wrong_type.stdout);
    assert!(
        wrong_type.stderr.contains("values.schema.json")
            || wrong_type.stderr.contains("don't meet the specifications"),
        "{}",
        wrong_type.stderr
    );
    let misspelt = test_utils::helm_template(
        &chart,
        Some(&format!("{base}postgres:\n  metadata:\n    enable: true\n")),
    );
    assert!(!misspelt.is_ok(), "{}", misspelt.stdout);
    assert!(misspelt.stderr.contains("enable"), "{}", misspelt.stderr);

    let bad_mode = test_utils::helm_template(
        &chart,
        Some(&format!(
            "{base}stackSettings:\n  kubernetes:\n    exposure:\n      mode: public\n"
        )),
    );
    assert!(!bad_mode.is_ok(), "{}", bad_mode.stdout);
    assert!(bad_mode.stderr.contains("mode"), "{}", bad_mode.stderr);
}

#[test]
fn chart_yaml_names_the_kubernetes_versions_and_the_operator_version() {
    use alien_helm::{apply_package_defaults, OperatorImageDefault, PackageDefaults};

    let stack = Stack::new("chart-meta".to_string()).build();
    let chart = alien_helm::generate_helm_chart(
        &stack,
        alien_helm::HelmOptions {
            registry: &alien_helm::HelmRegistry::built_in(),
            stack_settings: StackSettings::default(),
            chart_name: "chart-meta".to_string(),
        },
    )
    .unwrap();
    assert!(!chart.files["Chart.yaml"].contains("kubeVersion"));

    let packaged = |tag: &str| {
        apply_package_defaults(
            chart.clone(),
            &PackageDefaults {
                version: Some("1.4.3003059"),
                management_url: None,
                operator_image: Some(OperatorImageDefault {
                    repository: "ghcr.io/alienplatform/alien-operator",
                    tag,
                }),
                log_collector: None,
                registry_pull_secret: false,
                setup: None,
            },
        )
        .unwrap()
        .files["Chart.yaml"]
            .clone()
    };
    let release = packaged("v3.3.59");
    assert!(release.contains("version: 1.4.3003059\n"), "{release}");
    assert!(release.contains("appVersion: \"3.3.59\"\n"), "{release}");
    // A tag that isn't a release version leaves the chart version as the app version.
    assert!(packaged("dev").contains("appVersion: \"1.4.3003059\"\n"));
}

#[test]
fn the_chart_run_database_runs_as_the_postgres_user_with_limits() {
    use alien_core::{Postgres, ResourceLifecycle};
    let stack = Stack::new("db-hardening".to_string())
        .add(
            Postgres::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default()).files;
    let values = "management:\n  token: test-credential\npostgres:\n  metadata:\n    enabled: true\n    existingPasswordSecret: pg-password\n    image:\n      repository: pgvector/pgvector\n      tag: pg16-bookworm\n    recordedImage: pgvector/pgvector:pg16-bookworm\n";
    let rendered = test_utils::helm_template(&chart, Some(values));
    rendered.assert_ok("database enabled");
    let statefulset = yaml_documents(&rendered.stdout)
        .into_iter()
        .find(|document| document["kind"] == "StatefulSet")
        .expect("the chart runs the database");
    let pod = &statefulset["spec"]["template"]["spec"];
    assert_eq!(pod["securityContext"]["runAsNonRoot"], true);
    assert_eq!(pod["securityContext"]["runAsUser"], 999);
    assert_eq!(pod["securityContext"]["fsGroup"], 999);
    assert_eq!(
        pod["securityContext"]["seccompProfile"]["type"],
        "RuntimeDefault"
    );
    let container = &pod["containers"][0];
    assert_eq!(
        container["securityContext"]["allowPrivilegeEscalation"],
        false
    );
    assert_eq!(
        container["securityContext"]["capabilities"]["drop"][0],
        "ALL"
    );
    assert_eq!(
        container["resources"]["limits"]["memory"],
        container["resources"]["requests"]["memory"]
    );
}

#[test]
fn hook_jobs_are_bounded_and_keep_a_failed_run_for_debugging() {
    let chart = chart_files();
    let values = "management:\n  token: test-credential\n";
    let rendered = test_utils::helm_template(&chart, Some(values));
    rendered.assert_ok("default install");
    let job = yaml_documents(&rendered.stdout)
        .into_iter()
        .find(|document| {
            document["kind"] == "Job"
                && document["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-cleanup"))
        })
        .expect("the chart renders a pre-delete cleanup Job");
    let annotations = to_yaml(&job["metadata"]["annotations"]);
    assert!(
        annotations.contains("before-hook-creation,hook-succeeded"),
        "{annotations}"
    );
    assert!(!annotations.contains("hook-failed"), "{annotations}");
    assert_eq!(job["spec"]["ttlSecondsAfterFinished"], 86400);
    let container = &job["spec"]["template"]["spec"]["containers"][0];
    assert_eq!(container["securityContext"]["readOnlyRootFilesystem"], true);
    assert!(container["resources"]["limits"]["memory"].is_string());
    assert_eq!(container["volumeMounts"][0]["mountPath"], "/tmp");
    assert_eq!(job["spec"]["template"]["spec"]["volumes"][0]["name"], "tmp");

    // The history prune Job renders only on upgrades with a protected history, so its
    // template is checked as written.
    let prune = &chart["templates/runtime-cleanup-history-prune.yaml"];
    for required in [
        "ttlSecondsAfterFinished: 86400",
        "readOnlyRootFilesystem: true",
        "limits:",
        "mountPath: /tmp",
        "deployment.labels",
        "deployment.hookLabels",
    ] {
        assert!(prune.contains(required), "{required}");
    }
    assert!(!prune.contains("hook-failed"));
}

#[test]
fn the_node_log_collector_pulls_with_the_install_pull_secrets() {
    let chart = chart_files();
    let rendered_with = |values: &str| {
        let rendered = test_utils::helm_template(&chart, Some(values));
        rendered.assert_ok("node collector");
        let documents = yaml_documents(&rendered.stdout);
        let daemonset = documents
            .iter()
            .find(|document| document["kind"] == "DaemonSet")
            .expect("the node collector DaemonSet")
            .clone();
        let account_name = daemonset["spec"]["template"]["spec"]["serviceAccountName"].clone();
        let service_account = documents
            .iter()
            .find(|document| {
                document["kind"] == "ServiceAccount" && document["metadata"]["name"] == account_name
            })
            .expect("the node collector ServiceAccount")
            .clone();
        (daemonset, service_account)
    };
    let base = "management:\n  token: test-credential\nlogCollector:\n  enabled: true\n  mode: nodeAgent\n";

    let (daemonset, service_account) = rendered_with(&format!(
        "{base}runtime:\n  imagePullSecrets:\n    - name: install-registry\n"
    ));
    assert_eq!(
        daemonset["spec"]["template"]["spec"]["imagePullSecrets"][0]["name"],
        "install-registry"
    );
    assert_eq!(
        service_account["imagePullSecrets"][0]["name"],
        "install-registry"
    );

    // The collector can use its own secret.
    let (daemonset, _) = rendered_with(&format!(
        "{base}  imagePullSecrets:\n    - name: collector-registry\nruntime:\n  imagePullSecrets:\n    - name: install-registry\n"
    ));
    assert_eq!(
        daemonset["spec"]["template"]["spec"]["imagePullSecrets"][0]["name"],
        "collector-registry"
    );
}

#[test]
fn the_routing_rules_of_the_operator_role_can_be_left_out() {
    let chart = chart_files();
    let role_resources = |extra: &str| {
        let values = format!("management:\n  token: test-credential\n{extra}");
        let rendered = test_utils::helm_template(&chart, Some(&values));
        rendered.assert_ok("role");
        let role = yaml_documents(&rendered.stdout)
            .into_iter()
            .find(|document| {
                document["kind"] == "Role"
                    && document["rules"].as_sequence().is_some_and(|rules| {
                        rules
                            .iter()
                            .any(|rule| to_yaml(&rule["resources"]).contains("networkpolicies"))
                    })
            })
            .expect("the Operator Role");
        to_yaml(&role["rules"])
    };
    let default = role_resources("");
    for resource in [
        "ingresses",
        "httproutes",
        "healthcheckpolicies",
        "networkpolicies",
    ] {
        assert!(default.contains(resource), "{resource}");
    }
    let without = role_resources("rbac:\n  routing:\n    enabled: false\n");
    for resource in ["ingresses", "gateways", "httproutes", "healthcheckpolicies"] {
        assert!(!without.contains(resource), "{resource} stays: {without}");
    }
    // Network policies are not routing: containers with network settings need them.
    assert!(without.contains("networkpolicies"));

    // The uninstall cleanup shares the Role: it must not list what the Role leaves out.
    let cleanup = |extra: &str| {
        let values = format!("management:\n  token: test-credential\n{extra}");
        let rendered = test_utils::helm_template(&chart, Some(&values));
        rendered.assert_ok("cleanup");
        rendered.stdout
    };
    let with_routing = cleanup("");
    assert!(with_routing.contains("ingresses.networking.k8s.io"));
    assert!(with_routing.contains("Refusing cleanup: Gateway API discovery failed"));
    let no_routing = cleanup("rbac:\n  routing:\n    enabled: false\n");
    for name in [
        "ingresses.networking.k8s.io",
        "gateway.networking.k8s.io",
        "healthcheckpolicies",
    ] {
        assert!(!no_routing.contains(name), "cleanup still touches {name}");
    }
}

/// Runs the manager-ownership part of the rendered runtime cleanup script against a fake
/// `kubectl` that returns the given Deployment metadata fields, one per line, in the order the
/// script asks for them. Returns (exit success, stderr).
fn run_manager_ownership_check(script: &str, fields: [&str; 6]) -> (bool, String) {
    let start = script
        .find("manager_name=")
        .expect("script names the manager Deployment");
    let end = script
        .find("kubectl -n \"$namespace\" delete deployment \"$manager_name\"")
        .expect("script deletes the manager Deployment");
    let snippet = &script[start..end];
    let dir = std::env::temp_dir().join(format!(
        "alien-ownership-{}-{}",
        std::process::id(),
        fields.join("_")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // The fake answers `get deployment ... -o name` (resource_exists) and the jsonpath read.
    let kubectl = format!(
        "#!/bin/sh\ncase \"$*\" in\n  *jsonpath*) printf '%s\\n%s\\n%s\\n%s\\n%s\\n%s\\n' '{}' '{}' '{}' '{}' '{}' '{}' ;;\n  *) echo deployment.apps/x ;;\nesac\n",
        fields[0], fields[1], fields[2], fields[3], fields[4], fields[5]
    );
    std::fs::write(dir.join("kubectl"), kubectl).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("kubectl"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let program = format!(
        "set -e\nnamespace=ns\nscratch={dir}\nexpected_release_name=rel\nexpected_release_namespace=ns\nexpected_release_service=Helm\nresource_exists() {{ kubectl get \"$1\" -o name >/dev/null || {{ echo fake kubectl failed >&2; exit 9; }}; }}\n{snippet}\nfi\necho OWNED\n",
        dir = dir.display(),
    );
    // The snippet's heredocs keep the indentation the YAML block scalar strips.
    let program: String = program
        .lines()
        .map(|line| line.trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(dir.join("check.sh"), program).unwrap();
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    let output = std::process::Command::new("sh")
        .arg(dir.join("check.sh"))
        .env("PATH", path)
        .output()
        .unwrap();
    (
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("OWNED"),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_cleanup_hook_accepts_the_chart_release_label_when_argo_adds_no_helm_annotations() {
    let chart = chart_files();
    let rendered =
        test_utils::helm_template(&chart, Some("management:\n  token: test-credential\n"));
    rendered.assert_ok("default install");
    let documents = yaml_documents(&rendered.stdout);
    let manager = documents
        .iter()
        .find(|d| {
            d["kind"] == "Deployment"
                && d["metadata"]["labels"]["alien.dev/helm-release"].is_string()
        })
        .expect("the manager Deployment carries the chart's own release label");
    assert_eq!(
        manager["metadata"]["labels"]["alien.dev/helm-release"],
        "test-release"
    );
    let job = documents
        .iter()
        .find(|d| {
            d["kind"] == "Job"
                && d["metadata"]["name"]
                    .as_str()
                    .is_some_and(|n| n.ends_with("-cleanup"))
        })
        .expect("cleanup Job");
    let script = job["spec"]["template"]["spec"]["containers"][0]["command"][2]
        .as_str()
        .expect("script")
        .to_string();

    // Argo CD: no Helm annotations, `instance` rewritten to the Application name, but the
    // chart's label and the namespace match.
    let (ok, stderr) = run_manager_ownership_check(&script, ["", "", "", "my-app", "rel", "ns"]);
    assert!(ok, "label proof refused: {stderr}");
    // Helm: annotations and labels, no chart label (an install from before the label existed).
    let (ok, stderr) = run_manager_ownership_check(&script, ["rel", "ns", "Helm", "rel", "", "ns"]);
    assert!(ok, "helm proof refused: {stderr}");
    // Fail closed: another release's label, another namespace, nothing at all.
    for fields in [
        ["", "", "", "my-app", "other", "ns"],
        ["", "", "", "my-app", "rel", "other-ns"],
        ["", "", "", "rel", "", "ns"],
    ] {
        let (ok, stderr) = run_manager_ownership_check(&script, fields);
        assert!(!ok, "{fields:?} was accepted");
        assert!(
            stderr.contains("not owned by this exact Helm release"),
            "{stderr}"
        );
    }
}
