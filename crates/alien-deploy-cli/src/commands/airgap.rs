//! `alien-deploy airgap` — run a deployment with no connection to its manager.
//!
//! `apply` installs or updates from a bundle the vendor built with
//! `alien airgap bundle`; `status` exports the deployment's state and recent
//! logs for the vendor to import with `alien airgap import`.

use std::path::{Path, PathBuf};

use alien_cli_common::airgap::{self, BundleManifest, RegistryAccess, CHART_DIR, TARGET_FILE};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use oci_client::secrets::RegistryAuth;
use tokio::process::Command;

use crate::error::{ErrorData, Result};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Run a deployment with no connection to its manager",
    long_about = "Run a deployment with no connection to its manager.\n\nThe vendor builds a bundle with `alien airgap bundle`. `apply` pushes its images to your registry and installs or updates the deployment; `status` exports its state and recent logs for the vendor.",
    after_help = "EXAMPLES:
    alien-deploy airgap apply data-plane-rel_123.tar --registry registry.internal/vendor -f values.yaml
    alien-deploy airgap status -o status.tar"
)]
pub struct AirgapArgs {
    #[command(subcommand)]
    pub command: AirgapCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AirgapCommand {
    /// Install or update from a bundle
    Apply(ApplyArgs),
    /// Export state and recent logs for the vendor
    Status(StatusArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ApplyArgs {
    /// Bundle file from `alien airgap bundle`
    pub bundle: PathBuf,
    /// Registry (and optional path) to push the bundle's images to, e.g. registry.internal/vendor
    #[arg(long)]
    pub registry: String,
    /// Registry username
    #[arg(long, env = "AIRGAP_REGISTRY_USERNAME")]
    pub registry_username: Option<String>,
    /// Registry password or token
    #[arg(long, env = "AIRGAP_REGISTRY_PASSWORD")]
    pub registry_password: Option<String>,
    /// The registry serves plain HTTP
    #[arg(long)]
    pub insecure_registry: bool,
    /// Namespace (defaults to the application name)
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Helm values for this environment (infrastructure, inputs)
    #[arg(long = "values", short = 'f')]
    pub values: Vec<PathBuf>,
    /// Kubernetes context to use
    #[arg(long)]
    pub kube_context: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct StatusArgs {
    /// Application name (the Helm release; defaults to the only airgapped release in the namespace)
    #[arg(long)]
    pub release: Option<String>,
    /// Namespace
    #[arg(long, short = 'n')]
    pub namespace: String,
    /// Output file
    #[arg(long, short = 'o')]
    pub output: PathBuf,
    /// Log lines to include per pod
    #[arg(long, default_value_t = 500)]
    pub tail: u32,
    /// Kubernetes context to use
    #[arg(long)]
    pub kube_context: Option<String>,
}

pub async fn airgap_command(args: AirgapArgs) -> Result<()> {
    match args.command {
        AirgapCommand::Apply(args) => apply(args).await,
        AirgapCommand::Status(args) => status(args).await,
    }
}

async fn apply(args: ApplyArgs) -> Result<()> {
    let work = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    println!("Checking {}", args.bundle.display());
    airgap::unpack(&args.bundle, work.path()).context(config_error("reading the bundle"))?;
    let manifest = airgap::verify(work.path())
        .await
        .context(config_error("verifying the bundle"))?;
    let namespace = args
        .namespace
        .clone()
        .unwrap_or_else(|| manifest.stack_id.clone());

    println!(
        "Pushing {} images to {}",
        manifest.images.len(),
        args.registry
    );
    let access = RegistryAccess {
        auth: match (&args.registry_username, &args.registry_password) {
            (Some(user), Some(password)) => RegistryAuth::Basic(user.clone(), password.clone()),
            _ => RegistryAuth::Anonymous,
        },
        insecure: args.insecure_registry,
    };
    let mapping = airgap::import_images(
        &work.path().join(airgap::OCI_DIR),
        &manifest.images,
        &args.registry,
        &access,
    )
    .await
    .context(config_error("pushing images"))?;

    let mut target: serde_json::Value = serde_json::from_slice(
        &std::fs::read(work.path().join(TARGET_FILE))
            .into_alien_error()
            .context(config_error("reading the target"))?,
    )
    .into_alien_error()
    .context(config_error("parsing the target"))?;
    airgap::rewrite_references(&mut target, &mapping);
    let operator = manifest
        .images
        .iter()
        .find(|image| image.source == manifest.operator_image)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "bundle".to_string(),
                message: "bundle has no Operator image".to_string(),
            })
        })?;
    let operator_repository = format!(
        "{}/{}",
        args.registry.trim_end_matches('/'),
        operator.repository
    );
    let operator_tag = operator.tag.clone().unwrap_or_else(|| "latest".to_string());

    println!(
        "Installing {} in namespace {}",
        manifest.stack_id, namespace
    );
    helm_install(
        &args,
        &manifest,
        &work.path().join(&manifest.chart),
        &namespace,
        &operator_repository,
        &operator_tag,
    )
    .await?;
    write_target(&args, &manifest, &namespace, &target).await?;

    println!(
        "Applied release {} (bundle {}). The Operator rolls it out now; check with `alien-deploy airgap status -n {namespace} -o status.tar`.",
        manifest.release_id, manifest.sequence
    );
    Ok(())
}

async fn helm_install(
    args: &ApplyArgs,
    manifest: &BundleManifest,
    chart: &Path,
    namespace: &str,
    operator_repository: &str,
    operator_tag: &str,
) -> Result<()> {
    let mut cmd = Command::new("helm");
    cmd.arg("upgrade")
        .arg("--install")
        .arg(&manifest.stack_id)
        .arg(chart)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("--reset-then-reuse-values")
        .args(["--set", "airgapped.enabled=true"])
        .args([
            "--set-string",
            &format!("management.deploymentId={}", manifest.deployment_id),
        ])
        .args([
            "--set-string",
            &format!("management.name={}", manifest.deployment_name),
        ])
        .args([
            "--set-string",
            &format!("runtime.image.repository={operator_repository}"),
        ])
        .args(["--set-string", &format!("runtime.image.tag={operator_tag}")])
        .args(["--set", "runtime.image.pullWithManagementToken=false"])
        .args(["--set", "runtime.image.selfUpdate=false"])
        .args(["--set", "logCollector.enabled=false"])
        .args(["--set", "tunnel.enabled=false"])
        .args(["--wait", "--timeout", "300s"]);
    for values in &args.values {
        cmd.arg("-f").arg(values);
    }
    if let Some(context) = &args.kube_context {
        cmd.arg("--kube-context").arg(context);
    }
    run(cmd, "helm upgrade --install").await.map(|_| ())
}

async fn write_target(
    args: &ApplyArgs,
    manifest: &BundleManifest,
    namespace: &str,
    target: &serde_json::Value,
) -> Result<()> {
    let dir = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    let target_path = dir.path().join(airgap::TARGET_FILE);
    std::fs::write(
        &target_path,
        serde_json::to_vec(target)
            .into_alien_error()
            .context(config_error("encoding the target"))?,
    )
    .into_alien_error()
    .context(config_error("writing the target"))?;

    let secret_name = format!("{}-airgap-target", manifest.stack_id);
    let mut create = kubectl(args.kube_context.as_deref());
    create
        .args(["create", "secret", "generic", &secret_name, "-n", namespace])
        .arg(format!("--from-file=target.json={}", target_path.display()))
        .arg(format!("--from-literal=sequence={}", manifest.sequence))
        .args(["--dry-run=client", "-o", "yaml"]);
    let yaml = run(create, "kubectl create secret").await?;

    let apply_path = dir.path().join("secret.yaml");
    std::fs::write(&apply_path, yaml)
        .into_alien_error()
        .context(config_error("writing the Secret manifest"))?;
    let mut apply = kubectl(args.kube_context.as_deref());
    apply
        .args(["apply", "-n", namespace, "-f"])
        .arg(&apply_path);
    run(apply, "kubectl apply").await.map(|_| ())
}

async fn status(args: StatusArgs) -> Result<()> {
    let release = match &args.release {
        Some(release) => release.clone(),
        None => find_release(&args).await?,
    };
    let mut get = kubectl(args.kube_context.as_deref());
    get.args([
        "get",
        "secret",
        &format!("{release}-airgap-status"),
        "-n",
        &args.namespace,
        "-o",
        "jsonpath={.data.status\\.json}",
    ]);
    let encoded = run(get, "kubectl get secret").await?;
    let status = base64_decode(String::from_utf8_lossy(&encoded).trim())?;

    let mut pods = kubectl(args.kube_context.as_deref());
    pods.args(["get", "pods", "-n", &args.namespace, "-o", "name"]);
    let pods = String::from_utf8_lossy(&run(pods, "kubectl get pods").await?).to_string();
    let mut logs = Vec::new();
    for pod in pods.lines().filter(|line| !line.is_empty()) {
        let mut cmd = kubectl(args.kube_context.as_deref());
        cmd.args([
            "logs",
            pod,
            "-n",
            &args.namespace,
            "--all-containers",
            "--timestamps",
            &format!("--tail={}", args.tail),
        ]);
        let output = run(cmd, "kubectl logs").await?;
        let pod_name = pod.trim_start_matches("pod/");
        let workload = pod_workload(pod_name);
        for line in String::from_utf8_lossy(&output).lines() {
            let Some((timestamp, message)) = line.split_once(' ') else {
                continue;
            };
            if chrono::DateTime::parse_from_rfc3339(timestamp).is_err() {
                continue;
            }
            logs.push(serde_json::json!({
                "timestamp": timestamp,
                "resource": workload,
                "message": message,
                "attributes": { "k8s.pod.name": pod_name, "k8s.namespace.name": args.namespace },
            }));
        }
    }

    let dir = tempfile::tempdir()
        .into_alien_error()
        .context(config_error("creating a working directory"))?;
    std::fs::write(dir.path().join("status.json"), &status)
        .into_alien_error()
        .context(config_error("writing status"))?;
    let lines: Vec<String> = logs.iter().map(|log| log.to_string()).collect();
    std::fs::write(dir.path().join("logs.jsonl"), lines.join("\n"))
        .into_alien_error()
        .context(config_error("writing logs"))?;
    airgap::pack(dir.path(), &args.output).context(config_error("writing the status file"))?;
    println!(
        "Wrote {} ({} log lines). Send it to the vendor; they import it with `alien airgap import`.",
        args.output.display(),
        logs.len()
    );
    Ok(())
}

/// The air-gapped release in the namespace, from its status Secret.
async fn find_release(args: &StatusArgs) -> Result<String> {
    let mut cmd = kubectl(args.kube_context.as_deref());
    cmd.args(["get", "secrets", "-n", &args.namespace, "-o", "name"]);
    let names = String::from_utf8_lossy(&run(cmd, "kubectl get secrets").await?).to_string();
    let releases: Vec<&str> = names
        .lines()
        .filter_map(|name| {
            name.trim_start_matches("secret/")
                .strip_suffix("-airgap-status")
        })
        .collect();
    match releases.as_slice() {
        [release] => Ok(release.to_string()),
        [] => Err(AlienError::new(ErrorData::ValidationError {
            field: "namespace".to_string(),
            message: format!(
                "no air-gapped deployment has reported status in namespace '{}' yet",
                args.namespace
            ),
        })),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "release".to_string(),
            message: format!(
                "several deployments here ({}); pass --release",
                releases.join(", ")
            ),
        })),
    }
}

/// Workload name from a pod name (`api-7c4dc4b499-kggj2` → `api`).
fn pod_workload(pod: &str) -> String {
    let parts: Vec<&str> = pod.split('-').collect();
    if parts.len() > 2 {
        parts[..parts.len() - 2].join("-")
    } else {
        pod.to_string()
    }
}

fn kubectl(context: Option<&str>) -> Command {
    let mut cmd = Command::new("kubectl");
    if let Some(context) = context {
        cmd.arg("--context").arg(context);
    }
    cmd
}

async fn run(mut cmd: Command, what: &str) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(config_error(format!("running {what}; is it installed?")))?;
    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{what} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }));
    }
    Ok(output.stdout)
}

fn base64_decode(value: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .into_alien_error()
        .context(config_error("decoding the status Secret"))
}

fn config_error(message: impl Into<String>) -> ErrorData {
    ErrorData::ConfigurationError {
        message: message.into(),
    }
}
