use alien_core::{Stack, StackSettings};
use alien_helm::{
    generate_product_helm_chart, HelmChart, HelmOptions, HelmRegistry, OperatorManifestOptions,
    OperatorOutputFormat, OperatorPermission, OperatorScope, ProductOperatorManifestOptions,
};
use alien_terraform::{
    generate_product_terraform_module, TerraformHelmInstall, TerraformOptions,
    TerraformRegistration, TerraformTarget, TfRegistry,
};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CLUSTER_CONTEXT: &str = "kind-alien-product-lifecycle";
const CRD_NAME: &str = "alienaccessrequests.accessrequests.alien";
const ENCRYPTION_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ENCRYPTION_KEY_BASE64: &str =
    "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZg==";
const ENCRYPTION_KEY_SHA256: &str =
    "a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e";
const GOOD_OPERATOR_IMAGE: &str = "alien-product-lifecycle-operator:local";
const NOT_READY_OPERATOR_IMAGE: &str = "alien-product-lifecycle-operator-not-ready:local";
const OPERATOR_FIXTURE_BASE_IMAGE: &str = "busybox:1.36.1";
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
    let operator_fixture_dir = temp.path().join("operator-fixture");
    let not_ready_operator_fixture_dir = temp.path().join("operator-fixture-not-ready");
    fs::create_dir_all(&operator_fixture_dir).expect("create Operator fixture directory");
    fs::create_dir_all(&not_ready_operator_fixture_dir)
        .expect("create not-ready Operator fixture directory");
    fs::write(
        operator_fixture_dir.join("Dockerfile"),
        format!(
            "FROM {OPERATOR_FIXTURE_BASE_IMAGE}\nRUN mkdir -p /www && printf ready > /www/ready\nUSER 1000:1000\nCMD [\"httpd\", \"-f\", \"-p\", \"8081\", \"-h\", \"/www\"]\n"
        ),
    )
    .expect("write Operator readiness fixture Dockerfile");
    fs::write(
        not_ready_operator_fixture_dir.join("Dockerfile"),
        format!("FROM {OPERATOR_FIXTURE_BASE_IMAGE}\nUSER 1000:1000\nCMD [\"sleep\", \"3600\"]\n"),
    )
    .expect("write not-ready Operator fixture Dockerfile");
    run_ok("docker", ["pull", OPERATOR_FIXTURE_BASE_IMAGE], None);
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
    run_fails(
        "helm",
        failed_initial_install.iter().map(String::as_str),
        None,
        "an unrelated unready workload must trigger atomic initial-install cleanup",
    );
    run_fails(
        "helm",
        ["status", atomic_release, "--namespace", &helm_namespace],
        None,
        "atomic initial-install failure must remove the Helm release",
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
        "atomic cleanup must retain the prepared identity after initialization starts: {retained_prepared:?}"
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
        "atomic cleanup must retain the durable initialization record after the pod is gone: {retained_initialization:?}"
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
        "atomic cleanup must retain the initialized identity volume: {retained_identity:?}"
    );

    let retry_initial_install = helm_install_args(
        atomic_release,
        &helm_namespace,
        &good_chart_dir,
        false,
        "2m",
    );
    run_ok(
        "helm",
        retry_initial_install.iter().map(String::as_str),
        None,
    );
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
            atomic_release,
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
    let remote_operator_resource_name = cleanup_job_name
        .strip_suffix("-cleanup")
        .expect("cleanup Job name must identify the Remote Operator");
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
        1,
        "failed enable must retain a durable initialization record independently of live pods: {initialized:?}"
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
    let kubeconfig = temp.path().join("kind-kubeconfig");
    let config = run_ok(
        "kubectl",
        ["config", "view", "--raw", "--flatten", "--minify"],
        None,
    );
    fs::write(&kubeconfig, config.stdout).expect("write dedicated Kind kubeconfig");
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
    run_fails(
        "kubectl",
        [
            "get",
            "secret",
            "terraform-product-remote",
            "--namespace",
            terraform_namespace,
        ],
        None,
        "a disabled first apply must not create empty Remote Operator credentials",
    );
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        true,
        true,
        true,
    );
    run_ok(
        "terraform",
        ["plan", "-input=false", "-no-color", "-out=enabled.tfplan"],
        Some(&terraform_dir),
    );
    run_ok(
        "terraform",
        [
            "apply",
            "-input=false",
            "-no-color",
            "-auto-approve",
            "enabled.tfplan",
        ],
        Some(&terraform_dir),
    );
    let installed_encryption_key = run_ok(
        "kubectl",
        [
            "get",
            "secret",
            "terraform-product-remote",
            "--namespace",
            terraform_namespace,
            "--output=jsonpath={.data.encryption-key}",
        ],
        None,
    );
    assert_eq!(installed_encryption_key.stdout, ENCRYPTION_KEY_BASE64);
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
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
            "terraform-product-remote",
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
            .contains("Disabling the product Helm release or Remote Operator in place"),
        "{}",
        rejected_disable.diagnostic
    );
    run_ok("kubectl", ["get", "namespace", terraform_namespace], None);
    run_ok(
        "kubectl",
        [
            "get",
            "secret",
            "terraform-product-remote",
            "--namespace",
            terraform_namespace,
        ],
        None,
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
                log_collector: None,
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
        "--atomic".to_string(),
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
        "--atomic".to_string(),
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
    assert_eq!(registration_dependencies, 2);
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
    write_terraform_lifecycle_variables(directory, chart, namespace, true, false, false, false);
}

fn write_terraform_lifecycle_variables(
    directory: &Path,
    chart: &Path,
    namespace: &str,
    helm_install_enabled: bool,
    remote_operator_enabled: bool,
    remote_operator_bootstrap_identity: bool,
    include_credentials: bool,
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
    fs::write(
        directory.join("terraform.tfvars"),
        format!(
            r#"helm_install_enabled                = {helm_install_enabled}
helm_release_name                   = "terraform-product"
helm_chart                          = {chart:?}
kubernetes_namespace                = {namespace:?}
kubernetes_namespace_create         = true
remote_operator_enabled             = {remote_operator_enabled}
remote_operator_bootstrap_identity  = {remote_operator_bootstrap_identity}
remote_operator_sync_token_revision = 0
remote_operator_sync_token          = {sync_token}
remote_operator_encryption_key      = {encryption_key}
"#,
            chart = path_str(chart),
        ),
    )
    .expect("write Terraform lifecycle variables without a collector token");
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
