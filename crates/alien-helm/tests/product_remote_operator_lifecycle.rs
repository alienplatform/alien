use alien_core::{Stack, StackSettings};
use alien_helm::{
    generate_product_helm_chart, HelmChart, HelmOptions, HelmRegistry, OperatorLogCollectorOptions,
    OperatorManifestOptions, OperatorOutputFormat, OperatorPermission, OperatorScope,
    ProductOperatorManifestOptions,
};
use alien_terraform::{
    generate_product_terraform_module, TerraformHelmInstall, TerraformOptions,
    TerraformRegistration, TerraformTarget, TfRegistry,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CLUSTER_CONTEXT: &str = "kind-alien-product-lifecycle";
const CRD_NAME: &str = "alienaccessrequests.accessrequests.alien";
const TERRAFORM_RELEASE: &str = "terraform.product-lifecycle-long";
const TERRAFORM_RENAMED_RELEASE: &str = "terraform.product-lifecycle-renamed";
const ENCRYPTION_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENCRYPTION_KEY_BASE64: &str =
    "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZg==";
const COLLECTOR_TOKEN_BASE64: &str = "Y29sbGVjdG9yLXRlcnJhZm9ybQ==";
const ENCRYPTION_KEY_SHA256: &str =
    "a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e";
const GOOD_OPERATOR_IMAGE: &str = "alien-product-lifecycle-operator:local";
const NOT_READY_OPERATOR_IMAGE: &str = "alien-product-lifecycle-operator-not-ready:local";
const OPERATOR_FIXTURE_BASE_IMAGE: &str = "alpine/k8s:1.32.0";
const LOG_COLLECTOR_BASE_IMAGE: &str = "fluent/fluent-bit:3.2";
const LOG_COLLECTOR_IMAGE: &str = "alien-product-lifecycle-collector:local";
const GOOD_RUNTIME_IMAGE_REPOSITORY: &str = "registry.k8s.io/pause";
const GOOD_RUNTIME_IMAGE_TAG: &str = "3.10.1";

struct TestClusterCleanup {
    helm_namespace: String,
    helm_release: String,
    terraform_dir: PathBuf,
}

impl Drop for TestClusterCleanup {
    fn drop(&mut self) {
        let _ = Command::new("terraform")
            .args(["destroy", "-auto-approve", "-input=false", "-no-color"])
            .current_dir(&self.terraform_dir)
            .output();
        let _ = Command::new("helm")
            .args([
                "uninstall",
                &self.helm_release,
                "--namespace",
                &self.helm_namespace,
                "--ignore-not-found",
            ])
            .output();
        let _ = Command::new("kubectl")
            .args([
                "delete",
                "namespace",
                &self.helm_namespace,
                "--ignore-not-found",
            ])
            .output();
        let _ = Command::new("kubectl")
            .args(["delete", "crd", CRD_NAME, "--ignore-not-found"])
            .output();
    }
}

#[test]
#[ignore = "requires the dedicated disposable Kind cluster configured by CI"]
fn product_remote_operator_helm_and_terraform_lifecycle() {
    assert_eq!(
        std::env::var("ALIEN_RUN_PRODUCT_LIFECYCLE_E2E").as_deref(),
        Ok("1"),
        "set ALIEN_RUN_PRODUCT_LIFECYCLE_E2E=1 only for a disposable Kind cluster"
    );
    let context = run_ok("kubectl", ["config", "current-context"], None);
    assert_eq!(context.stdout.trim(), CLUSTER_CONTEXT);

    let temp = tempfile::tempdir().expect("lifecycle temp directory");
    let good_chart_dir = temp.path().join("good-chart");
    let legacy_chart_dir = temp.path().join("legacy-chart");
    let cluster_chart_dir = temp.path().join("cluster-chart");
    let bad_chart_dir = temp.path().join("bad-chart");
    let terraform_failure_chart_dir = temp.path().join("terraform-failure-chart");
    let operator_fixture_dir = temp.path().join("operator-fixture");
    let not_ready_operator_fixture_dir = temp.path().join("operator-fixture-not-ready");
    let log_collector_fixture_dir = temp.path().join("log-collector-fixture");
    fs::create_dir_all(&operator_fixture_dir).expect("create Operator fixture directory");
    fs::create_dir_all(&not_ready_operator_fixture_dir)
        .expect("create not-ready Operator fixture directory");
    fs::create_dir_all(&log_collector_fixture_dir).expect("create log-collector fixture directory");
    fs::write(
        operator_fixture_dir.join("Dockerfile"),
        r#"FROM __OPERATOR_FIXTURE_BASE_IMAGE__
RUN mkdir -p /www && printf ready > /www/ready && chmod -R a+rX /www
USER 1000:1000
ENTRYPOINT []
CMD ["/bin/sh", "-ec", "kubectl --namespace=\"$KUBERNETES_NAMESPACE\" patch configmap \"$OPERATOR_IDENTITY_INITIALIZED_CONFIGMAP\" --type=merge -p '{\"metadata\":{\"labels\":{\"alien.dev/remote-operator-identity-phase\":\"initialized\"}},\"immutable\":true}' && python3 -m http.server 8081 --directory /www"]
"#
        .replace("__OPERATOR_FIXTURE_BASE_IMAGE__", OPERATOR_FIXTURE_BASE_IMAGE),
    )
    .expect("write Operator readiness fixture Dockerfile");
    fs::write(
        not_ready_operator_fixture_dir.join("Dockerfile"),
        format!("FROM {OPERATOR_FIXTURE_BASE_IMAGE}\nUSER 1000:1000\nENTRYPOINT []\nCMD [\"sleep\", \"3600\"]\n"),
    )
    .expect("write not-ready Operator fixture Dockerfile");
    fs::write(
        log_collector_fixture_dir.join("Dockerfile"),
        format!("FROM {LOG_COLLECTOR_BASE_IMAGE}\nCOPY marker /alien-e2e-marker\n"),
    )
    .expect("write log-collector fixture Dockerfile");
    fs::write(log_collector_fixture_dir.join("marker"), "lifecycle-e2e\n")
        .expect("write log-collector fixture marker");
    run_ok("docker", ["pull", OPERATOR_FIXTURE_BASE_IMAGE], None);
    run_ok("docker", ["pull", LOG_COLLECTOR_BASE_IMAGE], None);
    run_ok(
        "docker",
        [
            "build",
            "--tag",
            LOG_COLLECTOR_IMAGE,
            path_str(&log_collector_fixture_dir),
        ],
        None,
    );
    run_ok(
        "docker",
        [
            "build",
            "--tag",
            GOOD_OPERATOR_IMAGE,
            path_str(&operator_fixture_dir),
        ],
        None,
    );
    run_ok(
        "docker",
        [
            "build",
            "--tag",
            NOT_READY_OPERATOR_IMAGE,
            path_str(&not_ready_operator_fixture_dir),
        ],
        None,
    );
    run_ok(
        "kind",
        [
            "load",
            "docker-image",
            GOOD_OPERATOR_IMAGE,
            "--name",
            "alien-product-lifecycle",
        ],
        None,
    );
    run_ok(
        "kind",
        [
            "load",
            "docker-image",
            LOG_COLLECTOR_IMAGE,
            "--name",
            "alien-product-lifecycle",
        ],
        None,
    );
    run_ok(
        "kind",
        [
            "load",
            "docker-image",
            NOT_READY_OPERATOR_IMAGE,
            "--name",
            "alien-product-lifecycle",
        ],
        None,
    );
    write_chart(&good_chart_dir, &product_chart(GOOD_OPERATOR_IMAGE));
    let mut legacy_chart = product_chart(GOOD_OPERATOR_IMAGE);
    legacy_chart
        .files
        .shift_remove("templates/remote-operator-lifecycle-capability.yaml");
    legacy_chart
        .files
        .shift_remove("templates/remote-operator-rollback-guard.yaml");
    legacy_chart
        .files
        .get_mut("templates/remote-operator-checks.yaml")
        .expect("legacy checks template")
        .replace_range(.., "");
    write_chart(&legacy_chart_dir, &legacy_chart);
    write_chart(
        &cluster_chart_dir,
        &product_chart_with_scope(GOOD_OPERATOR_IMAGE, OperatorScope::Cluster),
    );
    write_chart(&bad_chart_dir, &product_chart(NOT_READY_OPERATOR_IMAGE));
    let mut terraform_failure_chart = product_chart(GOOD_OPERATOR_IMAGE);
    terraform_failure_chart.files.insert(
        "templates/e2e-fail-after-identity.yaml".to_string(),
        r#"{{- if .Values.remoteOperator.enabled }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ .Release.Name }}-e2e-fail-after-identity
  annotations:
    "helm.sh/hook": post-upgrade
    "helm.sh/hook-weight": "95"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-failed
spec:
  backoffLimit: 0
  template:
    spec:
      restartPolicy: Never
      containers:
        - name: fail
          image: alpine/k8s:1.32.0
          command: ["/bin/sh", "-ec", "exit 1"]
{{- end }}
"#
        .to_string(),
    );
    write_chart(&terraform_failure_chart_dir, &terraform_failure_chart);

    let helm_namespace = "alien-product-helm-lifecycle".to_string();
    let helm_release = "alien-product".to_string();
    let terraform_dir = temp.path().join("terraform");
    let _cleanup = TestClusterCleanup {
        helm_namespace: helm_namespace.clone(),
        helm_release: helm_release.clone(),
        terraform_dir: terraform_dir.clone(),
    };

    run_ok("kubectl", ["create", "namespace", &helm_namespace], None);
    let installer_role = temp.path().join("product-installer-role.yaml");
    fs::write(
        &installer_role,
        format!(
            r#"apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: product-installer
  namespace: {helm_namespace}
rules:
  - apiGroups: ["*"]
    resources: ["*"]
    verbs: ["*"]
"#
        ),
    )
    .expect("write namespace-scoped product installer Role");
    run_ok(
        "kubectl",
        ["apply", "--filename", path_str(&installer_role)],
        None,
    );
    run_ok(
        "kubectl",
        [
            "create",
            "rolebinding",
            "product-installer",
            "--namespace",
            &helm_namespace,
            "--role=product-installer",
            "--user=product-installer",
        ],
        None,
    );

    let disabled_failure_release = "alien-product-disabled-failure";
    let disabled_failure_capability = format!(
        "{}-lifecycle-v2",
        remote_operator_record_name(&helm_namespace, disabled_failure_release, "remote-operator")
    );
    run_fails(
        "helm",
        [
            "install",
            disabled_failure_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            helm_rollback_on_failure_flag(),
            "--timeout=20s",
            "--set=heartbeat.collection.nodes.enabled=false",
            "--set-string=runtime.image.repository=registry.invalid/alien-product-lifecycle-missing",
            "--set-string=runtime.image.tag=latest",
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
        "an atomic disabled install with an unavailable product runtime must fail",
    );
    run_fails(
        "kubectl",
        [
            "get",
            "configmap",
            &disabled_failure_capability,
            "--namespace",
            &helm_namespace,
        ],
        None,
        "atomic disabled-install cleanup must delete its retained lifecycle capability",
    );
    run_ok(
        "helm",
        [
            "install",
            disabled_failure_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "uninstall",
            disabled_failure_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "install",
            &helm_release,
            path_str(&cluster_chart_dir),
            "--namespace",
            &helm_namespace,
            "--kube-as-user=product-installer",
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "upgrade",
            &helm_release,
            path_str(&cluster_chart_dir),
            "--namespace",
            &helm_namespace,
            "--kube-as-user=product-installer",
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );

    run_fails(
        "kubectl",
        ["get", "crd", CRD_NAME],
        None,
        "disabled install must not create the cluster-scoped CRD",
    );

    let failed_bridge_release = "alien-product-failed-bridge";
    let failed_bridge_capability = format!(
        "{}-lifecycle-v2",
        remote_operator_record_name(&helm_namespace, failed_bridge_release, "remote-operator")
    );
    run_ok(
        "helm",
        [
            "install",
            failed_bridge_release,
            path_str(&legacy_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    run_fails(
        "helm",
        [
            "upgrade",
            failed_bridge_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            helm_rollback_on_failure_flag(),
            "--timeout=20s",
            "--set=heartbeat.collection.nodes.enabled=false",
            "--set-string=runtime.image.repository=registry.invalid/alien-product-lifecycle-missing",
            "--set-string=runtime.image.tag=latest",
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
        "a failed disabled bridge upgrade must roll back to the legacy release",
    );
    assert_output_contains(
        &run_ok(
            "helm",
            [
                "status",
                failed_bridge_release,
                "--namespace",
                &helm_namespace,
            ],
            None,
        ),
        "STATUS: deployed",
    );
    run_fails(
        "kubectl",
        [
            "get",
            "configmap",
            &failed_bridge_capability,
            "--namespace",
            &helm_namespace,
        ],
        None,
        "rollback to a legacy chart must remove the non-identity capability latch",
    );
    run_ok(
        "helm",
        [
            "uninstall",
            failed_bridge_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "install",
            failed_bridge_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "uninstall",
            failed_bridge_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );

    let bridge_release = "alien-product-legacy-bridge";
    run_ok(
        "helm",
        [
            "install",
            bridge_release,
            path_str(&legacy_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    let bridge_credentials = format!("{bridge_release}-remote");
    run_ok(
        "kubectl",
        [
            "create",
            "secret",
            "generic",
            &bridge_credentials,
            "--namespace",
            &helm_namespace,
            "--from-literal=sync-token=sync-bridge",
            &format!("--from-literal=encryption-key={ENCRYPTION_KEY}"),
            "--from-literal=collector-token=collector-bridge",
        ],
        None,
    );
    let rejected_unbridged_enable = helm_upgrade_args(
        bridge_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "30s",
    );
    let rejected_unbridged_enable = run_fails(
        "helm",
        rejected_unbridged_enable.iter().map(String::as_str),
        None,
        "an existing release must record a guard-capable disabled revision before first enable",
    );
    assert!(
        rejected_unbridged_enable
            .diagnostic
            .contains("predates the Remote Operator rollback guard"),
        "{}",
        rejected_unbridged_enable.diagnostic
    );
    run_ok(
        "helm",
        [
            "upgrade",
            bridge_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    let rejected_unpruned_enable = helm_upgrade_args(
        bridge_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "30s",
    );
    let rejected_unpruned_enable = run_fails(
        "helm",
        rejected_unpruned_enable.iter().map(String::as_str),
        None,
        "first enable must reject retained pre-guard Helm history",
    );
    assert!(
        rejected_unpruned_enable
            .diagnostic
            .contains("predates the Remote Operator rollback guard introduced at revision"),
        "{}",
        rejected_unpruned_enable.diagnostic
    );
    run_ok(
        "helm",
        [
            "upgrade",
            bridge_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--history-max=1",
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    let pruned_history = run_ok(
        "helm",
        [
            "history",
            bridge_release,
            "--namespace",
            &helm_namespace,
            "--output=json",
        ],
        None,
    );
    let pruned_history: serde_json::Value =
        serde_json::from_str(&pruned_history.stdout).expect("parse pruned Helm history");
    let pruned_history = pruned_history.as_array().expect("Helm history array");
    assert!(
        !pruned_history.is_empty()
            && pruned_history.iter().all(|entry| {
                entry["revision"]
                    .as_i64()
                    .is_some_and(|revision| revision >= 2)
            }),
        "the bridge upgrade must remove every pre-guard rollback target: {pruned_history:?}"
    );
    let bridged_enable = helm_upgrade_args(
        bridge_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "2m",
    );
    run_ok("helm", bridged_enable.iter().map(String::as_str), None);
    run_fails(
        "helm",
        [
            "rollback",
            bridge_release,
            "1",
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=30s",
        ],
        None,
        "a pruned pre-guard revision must not remain a rollback target",
    );
    run_ok(
        "helm",
        [
            "uninstall",
            bridge_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "secret",
            &bridge_credentials,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );

    let history_mismatch_release = "alien-product-history-mismatch";
    let history_mismatch_credentials = format!("{history_mismatch_release}-remote");
    run_ok(
        "kubectl",
        [
            "create",
            "secret",
            "generic",
            &history_mismatch_credentials,
            "--namespace",
            &helm_namespace,
            "--from-literal=sync-token=sync-history-mismatch",
            &format!("--from-literal=encryption-key={ENCRYPTION_KEY}"),
            "--from-literal=collector-token=collector-history-mismatch",
        ],
        None,
    );
    let mut history_mismatch_install = helm_install_args(
        history_mismatch_release,
        &helm_namespace,
        &good_chart_dir,
        false,
        "2m",
    );
    for argument in &mut history_mismatch_install {
        if argument == "--set=remoteOperator.enabled=true" {
            *argument = "--set=remoteOperator.enabled=false".to_string();
        }
    }
    history_mismatch_install.push("--set=remoteOperator.helmHistoryBackend=configmap".to_string());
    run_ok(
        "helm",
        history_mismatch_install.iter().map(String::as_str),
        None,
    );
    let owned_runtime_sentinel = "history-mismatch-owned-runtime";
    let foreign_runtime_sentinel = "history-mismatch-foreign-runtime";
    for (name, deployment) in [
        (owned_runtime_sentinel, history_mismatch_release),
        (foreign_runtime_sentinel, "another-deployment"),
    ] {
        run_ok(
            "kubectl",
            ["create", "configmap", name, "--namespace", &helm_namespace],
            None,
        );
        run_ok(
            "kubectl",
            [
                "label",
                "configmap",
                name,
                "--namespace",
                &helm_namespace,
                "managed-by=runtime",
                &format!("alien.dev/deployment={deployment}"),
            ],
            None,
        );
    }
    let stale_history_record = format!("sh.helm.release.v1.{history_mismatch_release}.v2");
    run_ok(
        "kubectl",
        [
            "create",
            "configmap",
            &stale_history_record,
            "--namespace",
            &helm_namespace,
            "--from-literal=release=stale-history-record",
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "label",
            "configmap",
            &stale_history_record,
            "--namespace",
            &helm_namespace,
            "owner=helm",
            &format!("name={history_mismatch_release}"),
            "version=2",
            "status=deployed",
        ],
        None,
    );
    let mut history_mismatch_enable = helm_upgrade_args(
        history_mismatch_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "30s",
    );
    history_mismatch_enable.push("--set=remoteOperator.helmHistoryBackend=configmap".to_string());
    run_fails(
        "helm",
        history_mismatch_enable.iter().map(String::as_str),
        None,
        "stale records in the configured backend must not authorize a current upgrade stored elsewhere",
    );
    assert_output_contains(
        &run_ok(
            "helm",
            [
                "status",
                history_mismatch_release,
                "--namespace",
                &helm_namespace,
            ],
            None,
        ),
        "STATUS: deployed",
    );
    let rejected_history_identity = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-record=true",
            "--output=name",
        ],
        None,
    );
    assert!(
        rejected_history_identity.stdout.trim().is_empty(),
        "history-backend rejection must happen before identity preparation: {rejected_history_identity:?}"
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "configmap",
            &stale_history_record,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "uninstall",
            history_mismatch_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_fails(
        "kubectl",
        [
            "get",
            "configmap",
            owned_runtime_sentinel,
            "--namespace",
            &helm_namespace,
        ],
        None,
        "uninstall cleanup must delete runtime resources owned by this deployment",
    );
    run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            foreign_runtime_sentinel,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "configmap",
            foreign_runtime_sentinel,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "secret",
            &history_mismatch_credentials,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );

    let failed_release = "alien-product-failed-upgrade";
    let failed_credentials = format!("{failed_release}-remote");
    run_ok(
        "kubectl",
        [
            "create",
            "secret",
            "generic",
            &failed_credentials,
            "--namespace",
            &helm_namespace,
            "--from-literal=sync-token=sync-failed-upgrade",
            &format!("--from-literal=encryption-key={ENCRYPTION_KEY}"),
            "--from-literal=collector-token=collector-failed-upgrade",
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "install",
            failed_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    let mut failed_non_atomic_upgrade = helm_upgrade_args(
        failed_release,
        &helm_namespace,
        &terraform_failure_chart_dir,
        true,
        0,
        "2m",
    );
    let rollback_on_failure = helm_rollback_on_failure_flag();
    failed_non_atomic_upgrade.retain(|argument| argument != rollback_on_failure);
    run_fails(
        "helm",
        failed_non_atomic_upgrade.iter().map(String::as_str),
        None,
        "a non-atomic post-identity failure must leave an explicitly uninstallable failed release",
    );
    assert_output_contains(
        &run_ok(
            "helm",
            ["status", failed_release, "--namespace", &helm_namespace],
            None,
        ),
        "STATUS: failed",
    );
    let failed_resource_name =
        remote_operator_record_name(&helm_namespace, failed_release, "remote-operator");
    run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            &format!("{failed_resource_name}-initialized"),
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{failed_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "uninstall",
            failed_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    for retained_name in [
        failed_resource_name.clone(),
        format!("{failed_resource_name}-initialized"),
        format!("{failed_resource_name}-complete"),
        format!("{failed_resource_name}-lifecycle-v2"),
    ] {
        run_fails(
            "kubectl",
            [
                "get",
                "configmap",
                &retained_name,
                "--namespace",
                &helm_namespace,
            ],
            None,
            "explicit uninstall of a failed enabled upgrade must clean every retained record",
        );
    }
    run_ok(
        "kubectl",
        [
            "wait",
            "--for=delete",
            &format!("persistentvolumeclaim/{failed_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
            "--timeout=30s",
        ],
        None,
    );
    run_fails(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{failed_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
        ],
        None,
        "explicit uninstall of a failed enabled upgrade must clean the retained identity PVC",
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "secret",
            &failed_credentials,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );

    let atomic_release = "alien-product-atomic";
    let atomic_credentials = format!("{atomic_release}-remote");
    run_ok(
        "kubectl",
        [
            "create",
            "secret",
            "generic",
            &atomic_credentials,
            "--namespace",
            &helm_namespace,
            "--from-literal=sync-token=sync-atomic",
            &format!("--from-literal=encryption-key={ENCRYPTION_KEY}"),
            "--from-literal=collector-token=collector-atomic",
        ],
        None,
    );
    let failed_initial_install = helm_install_args(
        atomic_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        "30s",
    );
    let rejected_initial_enable = run_fails(
        "helm",
        failed_initial_install.iter().map(String::as_str),
        None,
        "a fresh install must establish a disabled rollback-guarded revision before enablement",
    );
    assert!(
        rejected_initial_enable
            .diagnostic
            .contains("cannot be enabled on the initial Helm install"),
        "{}",
        rejected_initial_enable.diagnostic
    );
    run_fails(
        "helm",
        ["status", atomic_release, "--namespace", &helm_namespace],
        None,
        "rejected initial enablement must not create a Helm release",
    );
    run_ok(
        "helm",
        [
            "install",
            atomic_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );
    let mut failed_enable = helm_upgrade_args(
        atomic_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "30s",
    );
    failed_enable.push("--set=runtime.probes.readiness.enabled=true".to_string());
    run_fails(
        "helm",
        failed_enable.iter().map(String::as_str),
        None,
        "an unrelated unready workload must trigger atomic upgrade rollback after identity initialization",
    );
    assert_output_contains(
        &run_ok(
            "helm",
            ["status", atomic_release, "--namespace", &helm_namespace],
            None,
        ),
        "STATUS: deployed",
    );
    let retained_prepared = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-record=true",
            "--output=name",
        ],
        None,
    );
    assert_eq!(
        retained_prepared.stdout.lines().count(),
        1,
        "atomic rollback must retain the prepared identity after initialization starts: {retained_prepared:?}"
    );
    let retained_initialization = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-phase=initialized",
            "--output=name",
        ],
        None,
    );
    assert_eq!(
        retained_initialization.stdout.lines().count(),
        1,
        "atomic rollback must retain the durable initialization record after the pod is gone: {retained_initialization:?}"
    );
    let retained_identity = run_ok(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            "--namespace",
            &helm_namespace,
            "--selector=app.kubernetes.io/component=operator",
            "--output=name",
        ],
        None,
    );
    assert_eq!(
        retained_identity.stdout.lines().count(),
        1,
        "atomic rollback must retain the initialized identity volume: {retained_identity:?}"
    );
    let atomic_resource_name =
        remote_operator_record_name(&helm_namespace, atomic_release, "remote-operator");
    run_ok(
        "helm",
        [
            "uninstall",
            atomic_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    for retained_name in [
        atomic_resource_name.clone(),
        format!("{atomic_resource_name}-initialized"),
        format!("{atomic_resource_name}-complete"),
        format!("{atomic_resource_name}-lifecycle-v2"),
    ] {
        run_fails(
            "kubectl",
            [
                "get",
                "configmap",
                &retained_name,
                "--namespace",
                &helm_namespace,
            ],
            None,
            "uninstall of the preserved disabled release must clean retained lifecycle records",
        );
    }
    run_ok(
        "kubectl",
        [
            "wait",
            "--for=delete",
            &format!("persistentvolumeclaim/{atomic_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
            "--timeout=30s",
        ],
        None,
    );
    run_fails(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{atomic_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
        ],
        None,
        "uninstall of the preserved disabled release must clean the retained identity PVC",
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "secret",
            &atomic_credentials,
            "--namespace",
            &helm_namespace,
        ],
        None,
    );

    let cleanup_render = run_ok(
        "helm",
        [
            "template",
            &helm_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--show-only",
            "templates/remote-operator-cleanup-job.yaml",
        ],
        None,
    );
    let cleanup_job: serde_yaml::Value =
        serde_yaml::from_str(&cleanup_render.stdout).expect("parse cleanup Job");
    let cleanup_job_name = cleanup_job["metadata"]["name"]
        .as_str()
        .expect("cleanup Job name");
    assert!(cleanup_job_name.contains("-cleanup-"));
    let remote_operator_resource_name =
        remote_operator_record_name(&helm_namespace, &helm_release, "remote-operator");
    let foreign_pvc = temp.path().join("foreign-identity-pvc.yaml");
    fs::write(
        &foreign_pvc,
        format!(
            r#"apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {remote_operator_resource_name}-identity
  namespace: {helm_namespace}
spec:
  accessModes: ["ReadWriteOnce"]
  resources:
    requests:
      storage: 1Mi
"#,
        ),
    )
    .expect("write foreign identity PVC");
    run_ok(
        "kubectl",
        ["apply", "--filename", path_str(&foreign_pvc)],
        None,
    );
    run_ok(
        "helm",
        [
            "uninstall",
            &helm_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{remote_operator_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "persistentvolumeclaim",
            &format!("{remote_operator_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
        ],
        None,
    );
    run_ok(
        "helm",
        [
            "install",
            &helm_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--kube-as-user=product-installer",
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );

    let credentials_name = format!("{helm_release}-remote");
    run_ok(
        "kubectl",
        [
            "create",
            "secret",
            "generic",
            &credentials_name,
            "--namespace",
            &helm_namespace,
            "--from-literal=sync-token=sync-v1",
            &format!("--from-literal=encryption-key={ENCRYPTION_KEY}"),
            "--from-literal=collector-token=collector-v1",
        ],
        None,
    );

    let failed_enable = helm_upgrade_args(
        &helm_release,
        &helm_namespace,
        &bad_chart_dir,
        true,
        0,
        "20s",
    );
    run_fails(
        "helm",
        failed_enable.iter().map(String::as_str),
        None,
        "an Operator that never initializes identity must trigger atomic rollback",
    );
    assert_output_contains(
        &run_ok(
            "helm",
            ["status", &helm_release, "--namespace", &helm_namespace],
            None,
        ),
        "STATUS: deployed",
    );
    run_ok("kubectl", ["get", "crd", CRD_NAME], None);
    let prepared = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-record=true",
            "--output=name",
        ],
        None,
    );
    assert_eq!(prepared.stdout.lines().count(), 1, "{prepared:?}");
    let completion = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-phase=complete",
            "--output=name",
        ],
        None,
    );
    assert!(
        completion.stdout.trim().is_empty(),
        "failed enable must not claim identity completion: {completion:?}"
    );
    let initialized = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-phase=initialized",
            "--output=name",
        ],
        None,
    );
    assert_eq!(
        initialized.stdout.lines().count(),
        0,
        "an Operator that never opens its identity must not claim initialization: {initialized:?}"
    );
    let pending = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            "--namespace",
            &helm_namespace,
            "--selector=alien.dev/remote-operator-identity-phase=pending",
            "--output=name",
        ],
        None,
    );
    assert_eq!(
        pending.stdout.lines().count(),
        1,
        "a failed enable must leave only the exact pending marker for explicit cleanup: {pending:?}"
    );

    run_ok(
        "helm",
        [
            "uninstall",
            &helm_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    for selector in [
        "alien.dev/remote-operator-identity-record=true",
        "alien.dev/remote-operator-identity-phase=pending",
        "alien.dev/remote-operator-identity-phase=initialized",
        "alien.dev/remote-operator-identity-phase=complete",
    ] {
        let retained = run_ok(
            "kubectl",
            [
                "get",
                "configmap",
                "--namespace",
                &helm_namespace,
                &format!("--selector={selector}"),
                "--output=name",
            ],
            None,
        );
        assert!(
            retained.stdout.trim().is_empty(),
            "explicit uninstall after a failed enable must retire the prepared identity: {retained:?}"
        );
    }
    run_ok(
        "kubectl",
        [
            "wait",
            "--for=delete",
            &format!("persistentvolumeclaim/{remote_operator_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
            "--timeout=30s",
        ],
        None,
    );

    run_ok(
        "helm",
        [
            "install",
            &helm_release,
            path_str(&good_chart_dir),
            "--namespace",
            &helm_namespace,
            "--kube-as-user=product-installer",
            "--wait",
            "--timeout=2m",
            "--set=heartbeat.collection.nodes.enabled=false",
            &format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
            &format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
            "--set=runtime.probes.liveness.enabled=false",
            "--set=runtime.probes.readiness.enabled=false",
        ],
        None,
    );

    let retry = helm_upgrade_args(
        &helm_release,
        &helm_namespace,
        &good_chart_dir,
        true,
        0,
        "2m",
    );
    run_ok("helm", retry.iter().map(String::as_str), None);
    assert_output_contains(
        &run_ok(
            "kubectl",
            [
                "get",
                "configmap",
                "--namespace",
                &helm_namespace,
                "--selector=alien.dev/remote-operator-identity-phase=complete",
                "--output=name",
            ],
            None,
        ),
        "configmap/",
    );

    run_ok(
        "helm",
        [
            "uninstall",
            &helm_release,
            "--namespace",
            &helm_namespace,
            "--no-hooks",
        ],
        None,
    );
    let mut disabled_reinstall = helm_install_args(
        &helm_release,
        &helm_namespace,
        &good_chart_dir,
        false,
        "30s",
    );
    for argument in &mut disabled_reinstall {
        if argument == "--set=remoteOperator.enabled=true" {
            *argument = "--set=remoteOperator.enabled=false".to_string();
        }
    }
    run_fails(
        "helm",
        disabled_reinstall.iter().map(String::as_str),
        None,
        "a disabled same-name reinstall must reject lifecycle records retained by --no-hooks",
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "configmap",
            remote_operator_resource_name.as_str(),
            &format!("{remote_operator_resource_name}-initialized"),
            &format!("{remote_operator_resource_name}-complete"),
            &format!("{remote_operator_resource_name}-lifecycle-v2"),
            "--namespace",
            &helm_namespace,
            "--ignore-not-found",
        ],
        None,
    );
    run_ok(
        "kubectl",
        [
            "delete",
            "persistentvolumeclaim",
            &format!("{remote_operator_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
            "--ignore-not-found",
            "--wait=true",
        ],
        None,
    );
    run_ok("helm", disabled_reinstall.iter().map(String::as_str), None);
    run_ok("helm", retry.iter().map(String::as_str), None);

    run_ok(
        "kubectl",
        [
            "patch",
            "secret",
            &credentials_name,
            "--namespace",
            &helm_namespace,
            "--type=merge",
            "--patch={\"stringData\":{\"sync-token\":\"sync-v2\"}}",
        ],
        None,
    );
    let rotation = helm_upgrade_args(
        &helm_release,
        &helm_namespace,
        &good_chart_dir,
        false,
        1,
        "2m",
    );
    run_ok("helm", rotation.iter().map(String::as_str), None);

    run_fails(
        "helm",
        [
            "rollback",
            &helm_release,
            "1",
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=30s",
        ],
        None,
        "rollback must not disable a completed Remote Operator identity",
    );
    run_ok(
        "helm",
        [
            "uninstall",
            &helm_release,
            "--namespace",
            &helm_namespace,
            "--wait",
            "--timeout=2m",
        ],
        None,
    );
    run_ok("kubectl", ["get", "crd", CRD_NAME], None);
    for selector in [
        "alien.dev/remote-operator-identity-record=true",
        "alien.dev/remote-operator-identity-phase=initialized",
        "alien.dev/remote-operator-identity-phase=complete",
    ] {
        let retained = run_ok(
            "kubectl",
            [
                "get",
                "configmap",
                "--namespace",
                &helm_namespace,
                &format!("--selector={selector}"),
                "--output=name",
            ],
            None,
        );
        assert!(
            retained.stdout.trim().is_empty(),
            "uninstall must delete retained Remote Operator records: {retained:?}"
        );
    }
    run_ok(
        "kubectl",
        [
            "wait",
            "--for=delete",
            &format!("persistentvolumeclaim/{remote_operator_resource_name}-identity"),
            "--namespace",
            &helm_namespace,
            "--timeout=30s",
        ],
        None,
    );
    let retained_pvc = run_ok(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            "--namespace",
            &helm_namespace,
            "--selector=app.kubernetes.io/component=operator",
            "--output=name",
        ],
        None,
    );
    assert!(
        retained_pvc.stdout.trim().is_empty(),
        "uninstall must delete the retained Remote Operator identity PVC: {retained_pvc:?}"
    );
    run_ok("kubectl", ["delete", "namespace", &helm_namespace], None);

    let terraform_namespace = "alien-product-terraform-lifecycle";
    let terraform_credentials_name = format!("{TERRAFORM_RELEASE}-remote");
    let kubeconfig = temp.path().join("kind-kubeconfig");
    let config = run_ok(
        "kubectl",
        ["config", "view", "--raw", "--flatten", "--minify"],
        None,
    );
    fs::write(&kubeconfig, &config.stdout).expect("write dedicated Kind kubeconfig");
    write_terraform_lifecycle_module(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        &kubeconfig,
    );
    run_ok(
        "terraform",
        ["init", "-backend=false", "-input=false", "-no-color"],
        Some(&terraform_dir),
    );

    run_ok(
        "kubectl",
        ["create", "namespace", terraform_namespace],
        None,
    );
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        false,
        false,
        false,
        false,
    );
    run_ok(
        "terraform",
        ["apply", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    write_terraform_lifecycle_variables_with_release(
        &terraform_dir,
        &terraform_failure_chart_dir,
        terraform_namespace,
        TERRAFORM_RELEASE,
        true,
        false,
        true,
        true,
        true,
        true,
    );
    let failed_post_identity_upgrade = run_fails(
        "terraform",
        [
            "apply",
            "-input=false",
            "-no-color",
            "-auto-approve",
            "-replace=terraform_data.remote_operator_ownership",
        ],
        Some(&terraform_dir),
        "a post-initialization hook failure must roll back to the cleanup-capable disabled release",
    );
    assert!(
        failed_post_identity_upgrade
            .diagnostic
            .contains("post-upgrade hooks failed"),
        "{}",
        failed_post_identity_upgrade.diagnostic
    );
    assert_output_contains(
        &run_ok(
            "helm",
            [
                "status",
                TERRAFORM_RELEASE,
                "--namespace",
                terraform_namespace,
            ],
            None,
        ),
        "STATUS: deployed",
    );
    let terraform_identity_name =
        remote_operator_record_name(terraform_namespace, TERRAFORM_RELEASE, "remote-operator");
    let retained_initialization = run_ok(
        "kubectl",
        [
            "get",
            "configmap",
            &format!("{terraform_identity_name}-initialized"),
            "--namespace",
            terraform_namespace,
            r#"--output=jsonpath={.metadata.labels.alien\.dev/remote-operator-identity-phase},{.immutable}"#,
        ],
        None,
    );
    assert_eq!(retained_initialization.stdout, "initialized,true");
    run_ok(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{terraform_identity_name}-identity"),
            "--namespace",
            terraform_namespace,
        ],
        None,
    );
    run_ok(
        "terraform",
        ["destroy", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    run_ok("kubectl", ["get", "namespace", terraform_namespace], None);
    for retained_name in [
        terraform_identity_name.clone(),
        format!("{terraform_identity_name}-initialized"),
        format!("{terraform_identity_name}-complete"),
        format!("{terraform_identity_name}-lifecycle-v2"),
    ] {
        run_fails(
            "kubectl",
            [
                "get",
                "configmap",
                &retained_name,
                "--namespace",
                terraform_namespace,
            ],
            None,
            "Terraform destroy must clean retained lifecycle records in an externally owned namespace",
        );
    }
    run_ok(
        "kubectl",
        [
            "wait",
            "--for=delete",
            &format!("persistentvolumeclaim/{terraform_identity_name}-identity"),
            "--namespace",
            terraform_namespace,
            "--timeout=30s",
        ],
        None,
    );
    run_fails(
        "kubectl",
        [
            "get",
            "persistentvolumeclaim",
            &format!("{terraform_identity_name}-identity"),
            "--namespace",
            terraform_namespace,
        ],
        None,
        "Terraform destroy must clean the retained identity PVC in an externally owned namespace",
    );
    run_ok(
        "kubectl",
        ["delete", "namespace", terraform_namespace],
        None,
    );

    fs::write(
        &kubeconfig,
        format!(
            r#"apiVersion: v1
kind: Config
clusters:
  - name: unreachable
    cluster:
      server: https://127.0.0.1:9
      insecure-skip-tls-verify: true
contexts:
  - name: {CLUSTER_CONTEXT}
    context:
      cluster: unreachable
      user: unreachable
current-context: {CLUSTER_CONTEXT}
users:
  - name: unreachable
    user:
      token: unreachable
"#
        ),
    )
    .expect("write unreachable kubeconfig for zero-read assertion");
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        false,
        false,
        false,
        false,
        false,
    );
    run_ok(
        "terraform",
        ["plan", "-input=false", "-no-color", "-out=disabled.tfplan"],
        Some(&terraform_dir),
    );
    run_ok(
        "terraform",
        [
            "apply",
            "-input=false",
            "-no-color",
            "-auto-approve",
            "disabled.tfplan",
        ],
        Some(&terraform_dir),
    );
    fs::write(&kubeconfig, config.stdout).expect("restore dedicated Kind kubeconfig");
    run_fails(
        "kubectl",
        ["get", "namespace", terraform_namespace],
        None,
        "an infrastructure-only first apply must not require or create the Helm namespace",
    );
    run_fails(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
        ],
        None,
        "a disabled first apply must not create empty Remote Operator credentials",
    );
    let external_identity_name =
        remote_operator_record_name(terraform_namespace, TERRAFORM_RELEASE, "remote-operator");
    run_ok(
        "kubectl",
        ["create", "namespace", terraform_namespace],
        None,
    );
    run_ok(
        "kubectl",
        [
            "create",
            "configmap",
            &external_identity_name,
            "--namespace",
            terraform_namespace,
        ],
        None,
    );
    run_ok(
        "terraform",
        ["apply", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    run_ok(
        "kubectl",
        ["delete", "namespace", terraform_namespace],
        None,
    );
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        true,
        false,
        false,
        false,
    );
    run_ok(
        "terraform",
        ["apply", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        true,
        true,
        true,
        true,
    );
    let unarmed_enable = run_fails(
        "terraform",
        ["plan", "-input=false", "-no-color"],
        Some(&terraform_dir),
        "first enable after a disabled apply must explicitly arm the ownership latch",
    );
    assert!(
        unarmed_enable
            .diagnostic
            .contains("Remote Operator ownership is pinned in Terraform state")
            && unarmed_enable
                .diagnostic
                .contains("-replace=terraform_data.remote_operator_ownership"),
        "{}",
        unarmed_enable.diagnostic
    );
    run_ok(
        "terraform",
        [
            "apply",
            "-input=false",
            "-no-color",
            "-auto-approve",
            "-replace=terraform_data.remote_operator_ownership",
        ],
        Some(&terraform_dir),
    );
    let installed_encryption_key = run_ok(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
            "--output=jsonpath={.data.encryption-key}",
        ],
        None,
    );
    assert_eq!(installed_encryption_key.stdout, ENCRYPTION_KEY_BASE64);
    write_terraform_lifecycle_variables_with_release(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        TERRAFORM_RELEASE,
        true,
        true,
        true,
        false,
        true,
        false,
    );
    let rejected_collector_clear = run_fails(
        "terraform",
        ["apply", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
        "clearing a required collector token must fail before mutating the credentials Secret",
    );
    assert!(
        rejected_collector_clear
            .diagnostic
            .contains("remote_operator_collector_token is required")
            && rejected_collector_clear
                .diagnostic
                .contains("Operator log collector is enabled"),
        "{}",
        rejected_collector_clear.diagnostic
    );
    let retained_collector_token = run_ok(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
            "--output=jsonpath={.data.collector-token}",
        ],
        None,
    );
    assert_eq!(retained_collector_token.stdout, COLLECTOR_TOKEN_BASE64);
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        true,
        true,
        false,
        true,
    );
    write_terraform_lifecycle_variables_with_release(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        TERRAFORM_RENAMED_RELEASE,
        true,
        true,
        true,
        false,
        true,
        true,
    );
    let rejected_release_move = run_fails(
        "terraform",
        ["plan", "-input=false", "-no-color"],
        Some(&terraform_dir),
        "a managed Remote Operator release name must remain pinned",
    );
    assert!(
        rejected_release_move
            .diagnostic
            .contains("Remote Operator ownership is pinned in Terraform state"),
        "{}",
        rejected_release_move.diagnostic
    );
    write_terraform_lifecycle_variables_with_release(
        &terraform_dir,
        &good_chart_dir,
        "alien-product-terraform-moved",
        TERRAFORM_RELEASE,
        true,
        false,
        true,
        false,
        true,
        true,
    );
    let rejected_namespace_move = run_fails(
        "terraform",
        ["plan", "-input=false", "-no-color"],
        Some(&terraform_dir),
        "a managed Remote Operator namespace must remain pinned",
    );
    assert!(
        rejected_namespace_move
            .diagnostic
            .contains("Remote Operator ownership is pinned in Terraform state"),
        "{}",
        rejected_namespace_move.diagnostic
    );
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        true,
        true,
        false,
        true,
    );
    run_ok(
        "terraform",
        ["apply", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    run_ok(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
        ],
        None,
    );

    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        false,
        true,
        false,
        false,
        false,
    );
    let rejected_disable = run_fails(
        "terraform",
        ["plan", "-input=false", "-no-color"],
        Some(&terraform_dir),
        "an in-place Helm disable must not retire a retained Remote Operator identity",
    );
    assert!(
        rejected_disable
            .diagnostic
            .contains("Remote Operator ownership is pinned in Terraform state"),
        "{}",
        rejected_disable.diagnostic
    );
    run_ok("kubectl", ["get", "namespace", terraform_namespace], None);
    run_ok(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
        ],
        None,
    );

    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        false,
        false,
        false,
        false,
        false,
    );
    let rejected_cleared_disable = run_fails(
        "terraform",
        ["plan", "-input=false", "-no-color"],
        Some(&terraform_dir),
        "clearing every new input must not bypass retained identity detection",
    );
    assert!(
        rejected_cleared_disable
            .diagnostic
            .contains("Remote Operator ownership is pinned in Terraform state"),
        "{}",
        rejected_cleared_disable.diagnostic
    );
    run_ok("kubectl", ["get", "namespace", terraform_namespace], None);
    run_ok(
        "kubectl",
        [
            "get",
            "secret",
            &terraform_credentials_name,
            "--namespace",
            terraform_namespace,
        ],
        None,
    );

    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        false,
        true,
        false,
        false,
        false,
    );
    let destroy = run_ok(
        "terraform",
        ["destroy", "-input=false", "-no-color", "-auto-approve"],
        Some(&terraform_dir),
    );
    let helm_destroyed = destroy
        .stdout
        .find("helm_release.runtime[0]: Destruction complete")
        .expect("Terraform destroy must finish uninstalling Helm");
    let secret_destroy_started = destroy
        .stdout
        .find("kubernetes_secret_v1.remote_operator_credentials[0]: Destroying")
        .expect("Terraform destroy must remove the credentials Secret");
    assert!(
        helm_destroyed < secret_destroy_started,
        "Terraform must uninstall Helm before deleting credentials:\n{}",
        destroy.stdout
    );
    run_fails(
        "kubectl",
        ["get", "namespace", terraform_namespace],
        None,
        "Terraform-owned namespace must be removed by destroy",
    );
    run_ok("kubectl", ["get", "crd", CRD_NAME], None);
}

#[test]
fn terraform_lifecycle_harness_is_formatted_and_parseable_without_a_collector_token() {
    let temp = tempfile::tempdir().expect("Terraform harness temp directory");
    let chart_dir = temp.path().join("chart");
    write_chart(&chart_dir, &product_chart(GOOD_OPERATOR_IMAGE));
    let kubeconfig = temp.path().join("unused-kubeconfig");
    fs::write(&kubeconfig, "apiVersion: v1\nkind: Config\n").expect("write parse-only kubeconfig");
    let terraform_dir = temp.path().join("terraform");
    write_terraform_lifecycle_module(&terraform_dir, &chart_dir, "parse-only", &kubeconfig);

    run_ok(
        "terraform",
        ["fmt", "-check", "-diff", "-recursive", "-no-color"],
        Some(&terraform_dir),
    );
}

fn product_chart(image: &str) -> HelmChart {
    product_chart_with_scope(image, OperatorScope::Namespace)
}

fn product_chart_with_scope(image: &str, scope: OperatorScope) -> HelmChart {
    let stack = Stack::new("product-lifecycle".to_string()).build();
    let registry = HelmRegistry::built_in();
    generate_product_helm_chart(
        &stack,
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: "product-lifecycle".to_string(),
        },
        ProductOperatorManifestOptions {
            manifest: OperatorManifestOptions {
                manager_url: "{{ .Values.management.url }}",
                group_token: "",
                encryption_key: "",
                image,
                log_collector: Some(OperatorLogCollectorOptions {
                    image: LOG_COLLECTOR_IMAGE,
                    token: "",
                }),
                stack_settings: None,
                project_name: "product-lifecycle",
                environment_name: None,
                install_namespace: None,
                label_domain: None,
                scope,
                label_selector: None,
                kubernetes_operations_enabled: true,
                custom_operation_permissions: &[],
                permission: OperatorPermission::Remediation,
                format: OperatorOutputFormat::HelmTemplate,
            },
            credentials_secret_name: "{{ .Values.remoteOperator.existingSecret.name }}",
            credentials_encryption_key_sha256:
                "{{ .Values.remoteOperator.existingSecret.encryptionKeySha256 }}",
            resource_name: None,
        },
    )
    .expect("product lifecycle chart")
}

fn helm_upgrade_args(
    release: &str,
    namespace: &str,
    chart: &Path,
    bootstrap_identity: bool,
    token_revision: u32,
    timeout: &str,
) -> Vec<String> {
    vec![
        "upgrade".to_string(),
        release.to_string(),
        path_str(chart).to_string(),
        "--namespace".to_string(),
        namespace.to_string(),
        helm_rollback_on_failure_flag().to_string(),
        format!("--timeout={timeout}"),
        "--set-string=management.url=https://management.example.test".to_string(),
        "--set=remoteOperator.enabled=true".to_string(),
        format!("--set=remoteOperator.bootstrapIdentity={bootstrap_identity}"),
        format!("--set=remoteOperator.syncTokenRevision={token_revision}"),
        format!("--set-string=remoteOperator.existingSecret.name={release}-remote"),
        format!(
            "--set-string=remoteOperator.existingSecret.encryptionKeySha256={ENCRYPTION_KEY_SHA256}"
        ),
        format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
        format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
        "--set=runtime.probes.liveness.enabled=false".to_string(),
        "--set=runtime.probes.readiness.enabled=false".to_string(),
    ]
}

fn helm_install_args(
    release: &str,
    namespace: &str,
    chart: &Path,
    runtime_readiness_enabled: bool,
    timeout: &str,
) -> Vec<String> {
    vec![
        "install".to_string(),
        release.to_string(),
        path_str(chart).to_string(),
        "--namespace".to_string(),
        namespace.to_string(),
        helm_rollback_on_failure_flag().to_string(),
        format!("--timeout={timeout}"),
        "--set-string=management.url=https://management.example.test".to_string(),
        "--set=remoteOperator.enabled=true".to_string(),
        "--set=remoteOperator.bootstrapIdentity=true".to_string(),
        format!("--set-string=remoteOperator.existingSecret.name={release}-remote"),
        format!(
            "--set-string=remoteOperator.existingSecret.encryptionKeySha256={ENCRYPTION_KEY_SHA256}"
        ),
        format!("--set-string=runtime.image.repository={GOOD_RUNTIME_IMAGE_REPOSITORY}"),
        format!("--set-string=runtime.image.tag={GOOD_RUNTIME_IMAGE_TAG}"),
        "--set=runtime.probes.liveness.enabled=false".to_string(),
        format!("--set=runtime.probes.readiness.enabled={runtime_readiness_enabled}"),
    ]
}

fn helm_rollback_on_failure_flag() -> &'static str {
    let version = run_ok("helm", ["version", "--short"], None);
    let major = version
        .stdout
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .expect("Helm must report a semantic major version");
    if major >= 4 {
        "--rollback-on-failure"
    } else {
        "--atomic"
    }
}

fn write_terraform_lifecycle_module(
    directory: &Path,
    chart: &Path,
    namespace: &str,
    kubeconfig: &Path,
) {
    fs::create_dir_all(directory).expect("create Terraform lifecycle directory");
    let stack = Stack::new("terraform-product-lifecycle".to_string()).build();
    let registry = TfRegistry::built_in();
    let module = generate_product_terraform_module(
        &stack,
        TerraformTarget::Eks,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: Some(TerraformRegistration {
                provider_name: "acme_app".to_string(),
                provider_source: "pkg.example.test/acme/app".to_string(),
                provider_version: "1.0.0".to_string(),
                resource_type: "deployment".to_string(),
                release_id: Some("lifecycle".to_string()),
                setup_target: "kubernetes".to_string(),
                setup_fingerprint: "lifecycle".to_string(),
                setup_fingerprint_version: 1,
            }),
            helm_install: Some(TerraformHelmInstall {
                chart_ref: path_str(chart).to_string(),
                release_name: "terraform-product".to_string(),
            }),
            supported_aws_regions: Vec::new(),
        },
        true,
    )
    .expect("product Terraform lifecycle module");
    let generated_helm = module.get("helm.tf").expect("generated helm.tf");
    assert_eq!(
        generated_helm
            .matches("acme_app_deployment.this.helm_values")
            .count(),
        1
    );
    let generated_helm = generated_helm.replace(
        "acme_app_deployment.this.helm_values",
        "local.provider_helm_values",
    );
    let registration_dependencies = generated_helm
        .lines()
        .filter(|line| line.trim() == "acme_app_deployment.this,")
        .count();
    assert_eq!(registration_dependencies, 1);
    let generated_helm = generated_helm
        .lines()
        .filter(|line| line.trim() != "acme_app_deployment.this,")
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(directory.join("helm.tf"), format!("{generated_helm}\n"))
        .expect("write generated Helm lifecycle resources");

    fs::write(
        directory.join("main.tf"),
        format!(
            r#"terraform {{
  required_version = ">= 1.9.0"
  required_providers {{
    helm = {{
      source  = "hashicorp/helm"
      version = ">= 3.0"
    }}
    kubernetes = {{
      source  = "hashicorp/kubernetes"
      version = ">= 2.30"
    }}
  }}
}}

provider "kubernetes" {{
  config_path    = {kubeconfig:?}
  config_context = {CLUSTER_CONTEXT:?}
}}

provider "helm" {{
  kubernetes = {{
    config_path    = {kubeconfig:?}
    config_context = {CLUSTER_CONTEXT:?}
  }}
}}

locals {{
  provider_helm_values = yamlencode({{
    management = {{
      url = "https://management.example.test"
    }}
    runtime = {{
      image = {{
        repository = {GOOD_RUNTIME_IMAGE_REPOSITORY:?}
        tag        = {GOOD_RUNTIME_IMAGE_TAG:?}
      }}
      probes = {{
        liveness = {{
          enabled = false
        }}
        readiness = {{
          enabled = false
        }}
      }}
    }}
  }})
}}

variable "helm_install_enabled" {{ type = bool }}
variable "helm_release_name" {{ type = string }}
variable "helm_chart" {{ type = string }}
variable "kubernetes_namespace" {{ type = string }}
variable "kubernetes_namespace_create" {{ type = bool }}
variable "remote_operator_enabled" {{ type = bool }}
variable "remote_operator_bootstrap_identity" {{ type = bool }}
variable "remote_operator_sync_token_revision" {{ type = number }}
variable "remote_operator_sync_token" {{
  type      = string
  sensitive = true
}}
variable "remote_operator_encryption_key" {{
  type      = string
  sensitive = true
}}
variable "remote_operator_collector_token" {{
  type      = string
  sensitive = true
  default   = null
}}
"#,
            kubeconfig = path_str(&kubeconfig)
        ),
    )
    .expect("write Terraform lifecycle provider configuration");
    write_terraform_lifecycle_variables(
        directory, chart, namespace, false, false, false, false, false,
    );
}

fn write_terraform_lifecycle_variables(
    directory: &Path,
    chart: &Path,
    namespace: &str,
    helm_install_enabled: bool,
    kubernetes_namespace_create: bool,
    remote_operator_enabled: bool,
    remote_operator_bootstrap_identity: bool,
    include_credentials: bool,
) {
    write_terraform_lifecycle_variables_with_release(
        directory,
        chart,
        namespace,
        TERRAFORM_RELEASE,
        helm_install_enabled,
        kubernetes_namespace_create,
        remote_operator_enabled,
        remote_operator_bootstrap_identity,
        include_credentials,
        include_credentials,
    );
}

#[allow(clippy::too_many_arguments)]
fn write_terraform_lifecycle_variables_with_release(
    directory: &Path,
    chart: &Path,
    namespace: &str,
    release_name: &str,
    helm_install_enabled: bool,
    kubernetes_namespace_create: bool,
    remote_operator_enabled: bool,
    remote_operator_bootstrap_identity: bool,
    include_credentials: bool,
    include_collector_token: bool,
) {
    let sync_token = if include_credentials {
        "\"sync-terraform\"".to_string()
    } else {
        "null".to_string()
    };
    let encryption_key = if include_credentials {
        format!("{ENCRYPTION_KEY:?}")
    } else {
        "null".to_string()
    };
    let collector_token = if include_collector_token {
        "\"collector-terraform\"".to_string()
    } else {
        "null".to_string()
    };
    fs::write(
        directory.join("terraform.tfvars"),
        format!(
            r#"helm_install_enabled                = {helm_install_enabled}
helm_release_name                   = {release_name:?}
helm_chart                          = {chart:?}
kubernetes_namespace                = {namespace:?}
kubernetes_namespace_create         = {kubernetes_namespace_create}
remote_operator_enabled             = {remote_operator_enabled}
remote_operator_bootstrap_identity  = {remote_operator_bootstrap_identity}
remote_operator_sync_token_revision = 0
remote_operator_sync_token          = {sync_token}
remote_operator_encryption_key      = {encryption_key}
remote_operator_collector_token     = {collector_token}
"#,
            chart = path_str(chart),
        ),
    )
    .expect("write Terraform lifecycle variables");
}

fn write_chart(directory: &Path, chart: &HelmChart) {
    for (relative, contents) in &chart.files {
        let path = directory.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create generated chart directory");
        }
        fs::write(path, contents).expect("write generated chart file");
    }
}

fn remote_operator_record_name(namespace: &str, release: &str, record: &str) -> String {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{namespace}/{release}").as_bytes())
    );
    let mut normalized_release = String::with_capacity(release.len());
    let mut replacing = false;
    for character in release.to_ascii_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
            normalized_release.push(character);
            replacing = false;
        } else if !replacing {
            normalized_release.push('-');
            replacing = true;
        }
    }
    let release_prefix = normalized_release
        .chars()
        .take(21)
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("{release_prefix}-{record}-{}", &digest[..16])
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test path must be UTF-8")
}

fn run_ok<I, S>(program: &str, args: I, current_dir: Option<&Path>) -> CommandOutput
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run(program, args, current_dir);
    assert!(output.status.success(), "{}", output.diagnostic);
    output
}

fn run_fails<I, S>(
    program: &str,
    args: I,
    current_dir: Option<&Path>,
    reason: &str,
) -> CommandOutput
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run(program, args, current_dir);
    assert!(!output.status.success(), "{reason}: {}", output.diagnostic);
    output
}

fn assert_output_contains(output: &CommandOutput, expected: &str) {
    assert!(
        output.stdout.contains(expected),
        "expected {expected:?}: {}",
        output.diagnostic
    );
}

#[derive(Debug)]
struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    diagnostic: String,
}

fn run<I, S>(program: &str, args: I, current_dir: Option<&Path>) -> CommandOutput
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let mut command = Command::new(program);
    command.args(&args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    let Output {
        status,
        stdout,
        stderr,
    } = command
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"));
    let stdout = String::from_utf8_lossy(&stdout).to_string();
    let stderr = String::from_utf8_lossy(&stderr).to_string();
    let diagnostic = format!(
        "command: {} {}\nstatus: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        program,
        args.iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    CommandOutput {
        status,
        stdout,
        diagnostic,
    }
}
