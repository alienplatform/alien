//! `gitops.enabled` makes a render fail instead of generating credentials.
//!
//! Without cluster access (Argo CD, `helm template`) `lookup` is empty, so a
//! generated encryption key or collector token differs on every render. The
//! GitOps profile refuses those renders; each test renders the real chart.

use super::{helpers::render, test_utils};
use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use serde::Deserialize;

fn chart_files() -> test_utils::LinterFiles {
    let stack = Stack::new("gitops-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    render(&stack, StackSettings::default()).files
}

const EXISTING: &str = "gitops:\n  enabled: true\nmanagement:\n  existingSecret:\n    name: alien-management\nruntime:\n  encryption:\n    existingSecret:\n      name: alien-encryption\n";

#[test]
fn profile_render_with_existing_secrets_generates_nothing() {
    let files = chart_files();
    let rendered = test_utils::helm_template(&files, Some(EXISTING));
    rendered.assert_ok("gitops with existing secrets");
    assert!(
        !rendered.stdout.contains("encryption-key:"),
        "the chart must not render an encryption key Secret entry:\n{}",
        rendered.stdout
    );
    // Rendering twice gives the same manifests: nothing random in the output.
    let again = test_utils::helm_template(&files, Some(EXISTING));
    assert_eq!(rendered.stdout, again.stdout);
}

#[test]
fn profile_refuses_a_generated_encryption_key() {
    let values =
        "gitops:\n  enabled: true\nmanagement:\n  existingSecret:\n    name: alien-management\n";
    let rendered = test_utils::helm_template(&chart_files(), Some(values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered
            .stderr
            .contains("runtime.encryption.existingSecret.name"),
        "{}",
        rendered.stderr
    );
}

#[test]
fn profile_refuses_a_management_token_in_values() {
    let values = "gitops:\n  enabled: true\nmanagement:\n  token: test-credential\nruntime:\n  encryption:\n    existingSecret:\n      name: alien-encryption\n";
    let rendered = test_utils::helm_template(&chart_files(), Some(values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered.stderr.contains("management.existingSecret.name"),
        "{}",
        rendered.stderr
    );
}

#[test]
fn profile_refuses_a_generated_collector_token() {
    let values = format!("{EXISTING}logCollector:\n  enabled: true\n  mode: nodeAgent\n");
    let rendered = test_utils::helm_template(&chart_files(), Some(&values));
    assert!(!rendered.is_ok(), "{}", rendered.stdout);
    assert!(
        rendered.stderr.contains("logCollector.token"),
        "{}",
        rendered.stderr
    );
    let pod_api = format!("{EXISTING}logCollector:\n  enabled: true\n  mode: podApi\n");
    test_utils::helm_template(&chart_files(), Some(&pod_api)).assert_ok("podApi collector");
}

#[test]
fn without_the_profile_a_render_still_generates_credentials() {
    // The default (a Helm CLI install) is unchanged.
    let values = "management:\n  token: test-credential\n";
    test_utils::helm_template(&chart_files(), Some(values)).assert_ok("helm cli install");
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
    assert_eq!(values["runtime"]["image"]["selfUpdate"], false);
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
    assert_eq!(values["runtime"]["image"]["selfUpdate"], false);
    for key in ["existingSecret"] {
        assert!(values["management"][key]["name"].is_string());
        assert!(values["runtime"]["encryption"][key]["name"].is_string());
    }
    assert!(values["infrastructureExistingSecret"].is_string());
    assert!(values["runtime"]["imagePullSecrets"][0]["name"].is_string());
    test_utils::helm_template(&chart, Some(&to_yaml(values))).assert_ok("argo example values");
}

#[test]
fn a_management_url_that_is_not_https_is_refused_unless_allowed() {
    let chart = chart_files();
    let credentials = "management:\n  token: test-credential\n";
    let http = format!("{credentials}  url: http://manager.example.test\n");
    let refused = test_utils::helm_template(&chart, Some(&http));
    assert!(!refused.is_ok(), "{}", refused.stdout);
    assert!(refused.stderr.contains("isn't https"), "{}", refused.stderr);

    let allowed = format!("{http}  allowInsecure: true\n");
    test_utils::helm_template(&chart, Some(&allowed)).assert_ok("http with allowInsecure");

    let https = format!("{credentials}  url: https://manager.example.test\n");
    test_utils::helm_template(&chart, Some(&https)).assert_ok("https url");
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
    assert!(chart.files["Chart.yaml"].contains("kubeVersion: \">=1.27.0-0\"\n"));

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
