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
const ENCRYPTION_KEY_SHA256: &str =
    "a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e";
const GOOD_OPERATOR_IMAGE: &str = "registry.k8s.io/pause:3.10.1";
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
    let bad_chart_dir = temp.path().join("bad-chart");
    run_ok("docker", ["pull", GOOD_OPERATOR_IMAGE], None);
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
    write_chart(&good_chart_dir, &product_chart(GOOD_OPERATOR_IMAGE));
    write_chart(
        &bad_chart_dir,
        &product_chart("registry.invalid/alien/operator:missing"),
    );

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
    run_fails(
        "kubectl",
        ["get", "crd", CRD_NAME],
        None,
        "disabled install must not create the cluster-scoped CRD",
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
        "an unavailable Operator image must trigger atomic rollback",
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
        "failed enable must retain only the prepared identity record: {completion:?}"
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
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
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
    write_terraform_lifecycle_variables(
        &terraform_dir,
        &good_chart_dir,
        terraform_namespace,
        true,
        false,
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
                scope: OperatorScope::Namespace,
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
    write_terraform_lifecycle_variables(directory, chart, namespace, false, false);
}

fn write_terraform_lifecycle_variables(
    directory: &Path,
    chart: &Path,
    namespace: &str,
    remote_operator_enabled: bool,
    remote_operator_bootstrap_identity: bool,
) {
    fs::write(
        directory.join("terraform.tfvars"),
        format!(
            r#"helm_install_enabled                = true
helm_release_name                   = "terraform-product"
helm_chart                          = {chart:?}
kubernetes_namespace                = {namespace:?}
kubernetes_namespace_create         = true
remote_operator_enabled             = {remote_operator_enabled}
remote_operator_bootstrap_identity  = {remote_operator_bootstrap_identity}
remote_operator_sync_token_revision = 0
remote_operator_sync_token          = "sync-terraform"
remote_operator_encryption_key      = {ENCRYPTION_KEY:?}
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

fn run_fails<I, S>(program: &str, args: I, current_dir: Option<&Path>, reason: &str)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run(program, args, current_dir);
    assert!(!output.status.success(), "{reason}: {}", output.diagnostic);
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
