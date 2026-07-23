//! Setup artifact E2E setup.
//!
//! This module is intentionally separate from the native `e2e::setup` path so
//! CloudFormation/Terraform/Helm tests cannot accidentally validate controller
//! provisioning instead of the setup import path.

use std::{collections::BTreeMap, env, path::Path, sync::Arc, time::Duration};

use alien_azure_clients::azure::resource_graph::ResourceGraphQueryRequest;
use alien_azure_clients::{
    AzureResourceGraphClient, AzureServiceBusManagementClient, AzureTokenCache, ResourceGraphApi,
    ServiceBusManagementApi,
};
use alien_core::{
    import::{
        data::{AwsArtifactRegistryImportData, AwsKvImportData, AwsStorageImportData},
        AzureRemoteStackManagementImportData, AzureServiceBusNamespaceImportData,
        GcpRemoteStackManagementImportData, ImportSourceKind, ImportedResource, StackImportRequest,
        StackImportResponse,
    },
    AwsManagementConfig, AzureClientConfig, AzureCredentials, AzureManagementConfig,
    DeploymentConfig, DeploymentModel as StackDeploymentModel, EnvironmentVariablesSnapshot,
    ExternalBinding, ExternalBindings, GcpClientConfig, GcpCredentials, GcpImpersonationConfig,
    GcpManagementConfig, KubernetesCertificateMode, KubernetesClusterOwnership,
    KubernetesClusterSettings, KubernetesExposureSettings, KubernetesIngressRouteProfile,
    KubernetesRouteProfile, KubernetesRouteProviderOptions, KubernetesSettings, ManagementConfig,
    Platform, Stack, StackSettings, StackState, Worker, WorkerCode,
};
#[cfg(test)]
use alien_core::{Container, ContainerCode, Kv, Queue, ResourceSpec, Storage, Vault};
use alien_gcp_clients::{GcpClientConfigExt, ResourceManagerApi};
use anyhow::Context;
use serde_json::Value;
use tempfile::TempDir;
use tokio::{fs, process::Command};
use tracing::{info, warn};

use crate::{
    build_push::build_and_push_stack_for_registry,
    config::{
        AwsConfig, AzureConfig, GcpConfig, KubernetesClusterMode, KubernetesRuntimeConfig,
        TestConfig,
    },
    deployment::TestDeployment,
    e2e::{self, DeploymentModel, DistributionFlow, TestApp, TestContext},
    helm_values::{runtime_image_pull_secrets, to_helm_values_yaml},
    managed_secret::provision_managed_test_secret,
    manager::TestManager,
};

const DEFAULT_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(600);
const AZURE_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(1_800);
const KUBERNETES_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(1_800);
const KUBERNETES_FULL_STACK_DEPLOYMENT_RUNNING_TIMEOUT: Duration = Duration::from_secs(3_600);

/// Artifact cleanup that sits outside the manager's normal destroy flow.
pub enum DistributionArtifactCleanup {
    CloudFormation {
        stack_name: String,
        region: String,
        env: Vec<(String, String)>,
        retained_resources: Vec<ImportedResource>,
        workdir: Option<TempDir>,
    },
    Terraform {
        workdir: TempDir,
        env: Vec<(String, String)>,
    },
    Helm {
        release: String,
        namespace: String,
        kubeconfig: Option<String>,
        kube_context: Option<String>,
        env: Vec<(String, String)>,
    },
}

impl DistributionArtifactCleanup {
    pub(crate) fn cleanup_order(&self) -> u8 {
        match self {
            // Helm consumes the cluster/setup resources and must be removed
            // before Terraform or CloudFormation destroys them.
            DistributionArtifactCleanup::Helm { .. } => 0,
            DistributionArtifactCleanup::CloudFormation { .. }
            | DistributionArtifactCleanup::Terraform { .. } => 1,
        }
    }

    /// Target-scoped credentials/region the artifact was applied with. Exposed
    /// so distribution tests can make read-only cloud assertions against the
    /// same account the setup artifact provisioned into.
    pub fn command_env(&self) -> &[(String, String)] {
        match self {
            DistributionArtifactCleanup::CloudFormation { env, .. }
            | DistributionArtifactCleanup::Terraform { env, .. }
            | DistributionArtifactCleanup::Helm { env, .. } => env,
        }
    }

    /// Preserve local state and return a credential-free recovery description.
    /// Used when live-resource deletion never reached a safe setup handoff.
    pub(crate) fn preserve_for_recovery(self) -> String {
        match self {
            DistributionArtifactCleanup::CloudFormation {
                stack_name,
                region,
                workdir,
                ..
            } => {
                let workdir = workdir.map(TempDir::keep);
                format!(
                    "CloudFormation stack '{stack_name}' in '{region}' was retained{}",
                    workdir
                        .as_ref()
                        .map(|path| format!("; workdir: {}", path.display()))
                        .unwrap_or_default()
                )
            }
            DistributionArtifactCleanup::Terraform { workdir, .. } => {
                let workdir = workdir.keep();
                format!("Terraform state retained at {}", workdir.display())
            }
            DistributionArtifactCleanup::Helm {
                release,
                namespace,
                kubeconfig,
                ..
            } => format!(
                "Helm release '{release}' in namespace '{namespace}' was retained{}",
                kubeconfig
                    .as_ref()
                    .map(|path| format!("; kubeconfig: {path}"))
                    .unwrap_or_default()
            ),
        }
    }

    pub async fn cleanup(self) -> anyhow::Result<()> {
        match self {
            DistributionArtifactCleanup::CloudFormation {
                stack_name,
                region,
                env,
                retained_resources,
                workdir,
            } => {
                info!(%stack_name, %region, "deleting CloudFormation distribution stack");
                let mut cmd = Command::new("aws");
                cmd.args([
                    "cloudformation",
                    "delete-stack",
                    "--stack-name",
                    &stack_name,
                    "--region",
                    &region,
                ]);
                apply_env(&mut cmd, &env);
                if let Err(error) = run_command(cmd, "aws cloudformation delete-stack").await {
                    let workdir = workdir.map(TempDir::keep);
                    anyhow::bail!(
                        "CloudFormation cleanup failed to start deletion for stack '{stack_name}' in '{region}': {error}{}",
                        recovery_workdir_suffix(workdir.as_deref())
                    );
                }

                let mut stack_deleted = false;
                let mut final_wait_error = None;
                for attempt in 1..=3 {
                    let mut wait = Command::new("aws");
                    wait.args([
                        "cloudformation",
                        "wait",
                        "stack-delete-complete",
                        "--stack-name",
                        &stack_name,
                        "--region",
                        &region,
                    ]);
                    apply_env(&mut wait, &env);
                    match run_command(wait, "aws cloudformation wait stack-delete-complete").await {
                        Ok(()) => {
                            stack_deleted = true;
                            info!(%stack_name, "CloudFormation distribution stack deleted");
                            break;
                        }
                        Err(error) if attempt < 3 => {
                            tracing::warn!(
                                %stack_name,
                                %attempt,
                                %error,
                                "CloudFormation cleanup wait failed; retrying"
                            );
                            tokio::time::sleep(Duration::from_secs(10 * attempt)).await;
                        }
                        Err(error) => {
                            final_wait_error = Some(error);
                        }
                    }
                }

                if !stack_deleted {
                    let workdir = workdir.map(TempDir::keep);
                    let error = final_wait_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "stack deletion did not complete".to_string());
                    anyhow::bail!(
                        "CloudFormation stack '{stack_name}' in '{region}' was retained after cleanup failed: {error}{}",
                        recovery_workdir_suffix(workdir.as_deref())
                    );
                }

                if let Err(error) =
                    cleanup_retained_cloudformation_resources(&env, &retained_resources).await
                {
                    let retained_ids = retained_resources
                        .iter()
                        .map(|resource| resource.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let workdir = workdir.map(TempDir::keep);
                    anyhow::bail!(
                        "CloudFormation stack '{stack_name}' was deleted, but retained resources [{retained_ids}] still need cleanup: {error}{}",
                        recovery_workdir_suffix(workdir.as_deref())
                    );
                }

                Ok(())
            }
            DistributionArtifactCleanup::Terraform { workdir, env } => {
                info!(
                    workdir = %workdir.path().display(),
                    "destroying Terraform setup artifacts"
                );
                for attempt in 1..=3 {
                    let mut cmd = Command::new("terraform");
                    cmd.current_dir(workdir.path()).args([
                        "destroy",
                        "-auto-approve",
                        "-input=false",
                        "-lock-timeout=5m",
                    ]);
                    apply_env(&mut cmd, &env);
                    match run_command(cmd, "terraform destroy").await {
                        Ok(()) => {
                            info!("Terraform setup artifacts destroyed");
                            return Ok(());
                        }
                        Err(error) if attempt < 3 => {
                            tracing::warn!(
                                %attempt,
                                %error,
                                "terraform destroy failed during cleanup; retrying"
                            );
                            tokio::time::sleep(Duration::from_secs(10 * attempt)).await;
                        }
                        Err(error) => {
                            let workdir = workdir.keep();
                            anyhow::bail!(
                                "terraform destroy failed during cleanup: {error}. Terraform state retained at {}",
                                workdir.display()
                            );
                        }
                    }
                }

                unreachable!("Terraform cleanup loop always succeeds or returns its final error")
            }
            DistributionArtifactCleanup::Helm {
                release,
                namespace,
                kubeconfig,
                kube_context,
                env,
            } => {
                let mut errors = Vec::new();
                if let Err(error) = crate::cleanup::cleanup_helm_release(
                    &release,
                    &namespace,
                    kubeconfig.as_deref(),
                    kube_context.as_deref(),
                    &env,
                )
                .await
                {
                    tracing::warn!(%release, %namespace, %error, "helm cleanup failed");
                    errors.push(format!("Helm release cleanup failed: {error}"));
                }
                if let Err(error) = crate::cleanup::cleanup_kubernetes_namespace(
                    &namespace,
                    kubeconfig.as_deref(),
                    kube_context.as_deref(),
                    &env,
                )
                .await
                {
                    tracing::warn!(%namespace, %error, "kubernetes namespace cleanup failed");
                    errors.push(format!("namespace cleanup failed: {error}"));
                }

                if !errors.is_empty() {
                    anyhow::bail!(
                        "Cleanup was incomplete for Helm release '{release}' in namespace '{namespace}'{}: {}",
                        kubeconfig
                            .as_deref()
                            .map(|path| format!("; kubeconfig: {path}"))
                            .unwrap_or_default(),
                        errors.join("; ")
                    );
                }

                Ok(())
            }
        }
    }
}

fn recovery_workdir_suffix(workdir: Option<&Path>) -> String {
    workdir
        .map(|path| format!("; workdir retained at {}", path.display()))
        .unwrap_or_default()
}

async fn cleanup_after_setup_error(
    cleanup: DistributionArtifactCleanup,
    setup_error: anyhow::Error,
) -> anyhow::Error {
    cleanup_after_ordered_setup_error(vec![cleanup], setup_error).await
}

async fn cleanup_after_ordered_setup_error(
    cleanups: Vec<DistributionArtifactCleanup>,
    setup_error: anyhow::Error,
) -> anyhow::Error {
    let mut cleanups = cleanups.into_iter();
    while let Some(cleanup) = cleanups.next() {
        if let Err(cleanup_error) = cleanup.cleanup().await {
            let retained = cleanups
                .map(DistributionArtifactCleanup::preserve_for_recovery)
                .collect::<Vec<_>>()
                .join("\n");
            let recovery = if retained.is_empty() {
                String::new()
            } else {
                format!(
                    " Subsequent distribution artifacts were retained to preserve cleanup order:\n{retained}"
                )
            };
            return setup_error.context(format!(
                "distribution artifact cleanup also failed: {cleanup_error}.{recovery}"
            ));
        }
    }
    setup_error
}

async fn cleanup_retained_cloudformation_resources(
    env: &[(String, String)],
    resources: &[ImportedResource],
) -> anyhow::Result<()> {
    for resource in resources {
        match resource.resource_type.as_ref() {
            "storage" => {
                let data: AwsStorageImportData =
                    serde_json::from_value(resource.import_data.clone()).with_context(|| {
                        format!(
                            "Failed to parse AWS storage import data for '{}'",
                            resource.id
                        )
                    })?;
                cleanup_retained_s3_bucket(env, &data.bucket_name).await?;
            }
            "kv" => {
                let data: AwsKvImportData = serde_json::from_value(resource.import_data.clone())
                    .with_context(|| {
                        format!("Failed to parse AWS KV import data for '{}'", resource.id)
                    })?;
                cleanup_retained_dynamodb_table(env, &data.table_name).await?;
            }
            "artifact-registry" => {
                let data: AwsArtifactRegistryImportData =
                    serde_json::from_value(resource.import_data.clone()).with_context(|| {
                        format!(
                            "Failed to parse AWS artifact registry import data for '{}'",
                            resource.id
                        )
                    })?;
                cleanup_retained_ecr_repository(env, &data.repository_prefix, &data.region).await?;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn cleanup_retained_s3_bucket(env: &[(String, String)], bucket: &str) -> anyhow::Result<()> {
    info!(%bucket, "deleting retained CloudFormation S3 bucket");
    let temp = TempDir::new().context("Failed to create retained S3 cleanup temp dir")?;

    let mut list = Command::new("aws");
    list.args([
        "s3api",
        "list-object-versions",
        "--bucket",
        bucket,
        "--output",
        "json",
    ]);
    apply_env(&mut list, env);
    let output = command_output(list, "aws s3api list-object-versions").await?;
    let versions: Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse S3 list-object-versions response")?;
    let mut objects = Vec::new();

    for field in ["Versions", "DeleteMarkers"] {
        let Some(entries) = versions.get(field).and_then(Value::as_array) else {
            continue;
        };

        for entry in entries {
            let Some(key) = entry.get("Key").and_then(Value::as_str) else {
                continue;
            };
            let mut object = serde_json::Map::new();
            object.insert("Key".to_string(), Value::String(key.to_string()));
            if let Some(version_id) = entry.get("VersionId").and_then(Value::as_str) {
                object.insert(
                    "VersionId".to_string(),
                    Value::String(version_id.to_string()),
                );
            }
            objects.push(Value::Object(object));
        }
    }

    for (index, chunk) in objects.chunks(1000).enumerate() {
        let delete_file = temp.path().join(format!("delete-{index}.json"));
        let payload = serde_json::json!({
            "Objects": chunk,
            "Quiet": true,
        });
        fs::write(
            &delete_file,
            serde_json::to_vec(&payload).context("Failed to serialize S3 delete payload")?,
        )
        .await
        .context("Failed to write S3 delete payload")?;

        let mut delete = Command::new("aws");
        delete.args([
            "s3api",
            "delete-objects",
            "--bucket",
            bucket,
            "--delete",
            &format!("file://{}", delete_file.display()),
        ]);
        apply_env(&mut delete, env);
        run_command(delete, "aws s3api delete-objects").await?;
    }

    let mut delete_bucket = Command::new("aws");
    delete_bucket.args(["s3api", "delete-bucket", "--bucket", bucket]);
    apply_env(&mut delete_bucket, env);
    run_command(delete_bucket, "aws s3api delete-bucket").await?;

    Ok(())
}

async fn cleanup_retained_ecr_repository(
    env: &[(String, String)],
    repository_name: &str,
    region: &str,
) -> anyhow::Result<()> {
    info!(%repository_name, %region, "deleting retained CloudFormation ECR repository");
    let mut delete = Command::new("aws");
    delete.args([
        "ecr",
        "delete-repository",
        "--repository-name",
        repository_name,
        "--region",
        region,
        "--force",
    ]);
    apply_env(&mut delete, env);
    run_command(delete, "aws ecr delete-repository").await?;

    Ok(())
}

async fn cleanup_retained_dynamodb_table(
    env: &[(String, String)],
    table_name: &str,
) -> anyhow::Result<()> {
    info!(%table_name, "deleting retained CloudFormation DynamoDB table");
    let mut delete = Command::new("aws");
    delete.args(["dynamodb", "delete-table", "--table-name", table_name]);
    apply_env(&mut delete, env);
    run_command(delete, "aws dynamodb delete-table").await?;

    let mut wait = Command::new("aws");
    wait.args([
        "dynamodb",
        "wait",
        "table-not-exists",
        "--table-name",
        table_name,
    ]);
    apply_env(&mut wait, env);
    run_command(wait, "aws dynamodb wait table-not-exists").await?;

    Ok(())
}

struct DistributionPrepared {
    manager: Arc<TestManager>,
    config: TestConfig,
    built_stack: Stack,
    rendered_stack: Stack,
    platform: Platform,
    model: DeploymentModel,
    app: TestApp,
    flow: DistributionFlow,
    group_id: String,
    dg_token: String,
}

struct TerraformApplyResult {
    deployment: TestDeployment,
    cleanup: DistributionArtifactCleanup,
    outputs: Value,
    stack_state: StackState,
    stack_settings: StackSettings,
    base_platform: Option<Platform>,
    region: String,
}

struct ImportedTestDeployment {
    deployment: TestDeployment,
    stack_state: StackState,
    stack_settings: StackSettings,
}

#[derive(Debug, Clone)]
struct KubernetesHelmTarget {
    runtime: KubernetesRuntimeConfig,
    namespace: String,
}

pub async fn setup_distribution(
    flow: DistributionFlow,
    app: TestApp,
) -> anyhow::Result<TestContext> {
    let mut prepared = prepare_distribution(flow, app).await?;

    let result = match flow {
        DistributionFlow::CloudFormationAwsPush => run_cloudformation_aws(&mut prepared).await,
        DistributionFlow::CloudFormationEksHelmPull => run_cloudformation_k8s(&mut prepared).await,
        DistributionFlow::TerraformAwsPush => {
            run_terraform_cloud(&mut prepared, alien_terraform::TerraformTarget::Aws).await
        }
        DistributionFlow::TerraformGcpPush => {
            run_terraform_cloud(&mut prepared, alien_terraform::TerraformTarget::Gcp).await
        }
        DistributionFlow::TerraformAzurePush => {
            run_terraform_cloud(&mut prepared, alien_terraform::TerraformTarget::Azure).await
        }
        DistributionFlow::TerraformEksHelmPull => {
            run_terraform_k8s(&mut prepared, alien_terraform::TerraformTarget::Eks).await
        }
        DistributionFlow::TerraformGkeHelmPull => {
            run_terraform_k8s(&mut prepared, alien_terraform::TerraformTarget::Gke).await
        }
        DistributionFlow::TerraformAksHelmPull => {
            run_terraform_k8s(&mut prepared, alien_terraform::TerraformTarget::Aks).await
        }
        DistributionFlow::TerraformOnpremHelmPull => run_onprem_k8s(&mut prepared).await,
    };

    match result {
        Ok(mut ctx) => {
            if let Err(error) = wait_and_finalize(&mut ctx).await {
                if let Err(cleanup_error) = ctx.cleanup().await {
                    return Err(
                        error.context(format!("distribution cleanup also failed: {cleanup_error}"))
                    );
                }
                return Err(error);
            }
            Ok(ctx)
        }
        Err(error) => Err(error),
    }
}

async fn prepare_distribution(
    flow: DistributionFlow,
    app: TestApp,
) -> anyhow::Result<DistributionPrepared> {
    let platform = flow.platform();
    let model = flow.deployment_model();
    let test_name = format!("{}_{}", flow.name(), app);
    info!(%test_name, "Starting distribution E2E setup");

    let config = TestConfig::from_env();
    if !is_distribution_flow_available(flow, &config, app) {
        anyhow::bail!(
            "Skipping {}: platform credentials not available or platform not supported for this distribution flow",
            test_name,
        );
    }
    if let Some(reason) = missing_distribution_flow_config(flow, &config) {
        anyhow::bail!("Skipping {test_name}: {reason}");
    }

    let manager_platforms = manager_platforms_for_flow(flow, &config);
    let manager = if manager_platforms.is_empty() {
        Arc::new(
            TestManager::start()
                .await
                .map_err(|error| anyhow::anyhow!("Failed to start TestManager: {error}"))?,
        )
    } else {
        Arc::new(
            TestManager::start_with_config(&config, &manager_platforms)
                .await
                .map_err(|error| anyhow::anyhow!("Failed to start TestManager: {error}"))?,
        )
    };

    // Managed Kubernetes distributions render and build as Kubernetes. Their
    // registry/setup cloud remains separate from the runtime platform.
    let registry_platform = image_registry_platform_for_flow(flow, &config)?;
    let build_base_platform = flow.kubernetes_base_platform();
    let e2e_root = e2e::e2e_test_apps_root()?;
    let app_path = e2e_root.join(e2e::test_app_path(app));
    let stack_json = e2e::load_stack_json(&app_path, "alien.ts", platform).await?;
    let stack_value = stack_json
        .get(platform.as_str())
        .context("Stack JSON missing platform key")?;
    let stack: Stack =
        serde_json::from_value(stack_value.clone()).context("Failed to deserialize Stack JSON")?;

    let pushed_stack = build_and_push_stack_for_registry(
        stack,
        platform,
        registry_platform,
        build_base_platform,
        &config,
        &app_path,
        &manager,
    )
    .await?;

    if registry_platform == Platform::Aws && config.aws_target.is_some() {
        let tags = e2e::extract_ecr_image_tags(&pushed_stack);
        if !tags.is_empty() {
            crate::build_push::wait_for_ecr_replication(&config, &tags).await?;
        }
    }

    // The manager release must keep the same source stack shape as a normal
    // `alien release`. Setup artifacts render from a derived stack after
    // template mutations add setup-owned resources such as remote management.
    let render_stack = if model == DeploymentModel::Push {
        rewrite_push_distribution_images(pushed_stack.clone(), platform, &config)?
    } else {
        pushed_stack.clone()
    };

    let stack_settings = e2e_stack_settings_for_flow(model, &config, platform)?;
    let management_config =
        render_distribution_management_config(flow, platform, &config, &stack_settings);
    let rendered_stack = apply_render_mutations_with_management_config(
        render_stack,
        platform,
        &stack_settings,
        management_config,
    )
    .await
    .context("Failed to apply distribution render preflights")?;

    create_release(&manager, platform, &pushed_stack).await?;
    let (group_id, dg_token) = create_deployment_group_token(&manager).await?;

    Ok(DistributionPrepared {
        manager,
        config,
        built_stack: pushed_stack,
        rendered_stack,
        platform,
        model,
        app,
        flow,
        group_id,
        dg_token,
    })
}

fn is_distribution_flow_available(
    flow: DistributionFlow,
    config: &TestConfig,
    app: TestApp,
) -> bool {
    if let Some(base_platform) = flow.kubernetes_base_platform() {
        return config.has_platform(base_platform);
    }

    match flow {
        DistributionFlow::TerraformOnpremHelmPull => {
            [Platform::Aws, Platform::Gcp, Platform::Azure]
                .into_iter()
                .any(|platform| config.has_platform(platform))
        }
        _ => e2e::is_platform_available(config, flow.platform(), flow.deployment_model(), app),
    }
}

fn missing_distribution_flow_config(
    flow: DistributionFlow,
    config: &TestConfig,
) -> Option<&'static str> {
    match flow {
        DistributionFlow::CloudFormationEksHelmPull => {
            if !config.has_platform(Platform::Aws) {
                Some("AWS management and target credentials are required")
            } else if config.kubernetes_cluster_mode == KubernetesClusterMode::Existing {
                Some("CloudFormation EKS Helm distribution creates a new EKS cluster")
            } else {
                None
            }
        }
        DistributionFlow::TerraformEksHelmPull => {
            if !config.has_platform(Platform::Aws) {
                Some("AWS management and target credentials are required")
            } else if config.kubernetes_cluster_mode == KubernetesClusterMode::Existing
                && config.kubernetes.eks.is_none()
            {
                Some("ALIEN_TEST_EKS_CLUSTER_NAME and KUBECONFIG are required")
            } else {
                None
            }
        }
        DistributionFlow::TerraformGkeHelmPull => {
            if !config.has_platform(Platform::Gcp) {
                Some("GCP management and target credentials are required")
            } else if config.kubernetes_cluster_mode == KubernetesClusterMode::Existing
                && config.kubernetes.gke.is_none()
            {
                Some(
                    "ALIEN_TEST_GKE_CLUSTER_NAME, ALIEN_TEST_GKE_CLUSTER_LOCATION, and KUBECONFIG are required",
                )
            } else {
                None
            }
        }
        DistributionFlow::TerraformAksHelmPull => {
            if !config.has_platform(Platform::Azure) {
                Some("Azure management and target credentials are required")
            } else if !has_azure_management_oidc(config)
                && !has_local_aks_management_metadata(config)
            {
                Some(
                    "AZURE_MANAGEMENT_OIDC_ISSUER, AZURE_MANAGEMENT_OIDC_SUBJECT, and AZURE_FEDERATED_TOKEN_FILE are required",
                )
            } else if config.kubernetes_cluster_mode == KubernetesClusterMode::Existing
                && config.kubernetes.aks.is_none()
            {
                Some(
                    "ALIEN_TEST_AKS_CLUSTER_NAME, ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP, and KUBECONFIG are required",
                )
            } else {
                None
            }
        }
        DistributionFlow::TerraformAzurePush => {
            if !has_azure_management_oidc(config) && !is_local_azure_direct_target_mode() {
                Some(
                    "AZURE_MANAGEMENT_OIDC_ISSUER, AZURE_MANAGEMENT_OIDC_SUBJECT, and AZURE_FEDERATED_TOKEN_FILE are required",
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

fn has_azure_management_oidc(config: &TestConfig) -> bool {
    has_azure_management_oidc_metadata(config)
        && std::env::var("AZURE_FEDERATED_TOKEN_FILE")
            .ok()
            .filter(|value| !value.is_empty())
            .is_some()
}

fn has_azure_management_oidc_metadata(config: &TestConfig) -> bool {
    config
        .azure_mgmt
        .as_ref()
        .is_some_and(|mgmt| mgmt.oidc_issuer.is_some() && mgmt.oidc_subject.is_some())
}

fn has_local_aks_management_metadata(config: &TestConfig) -> bool {
    is_local_azure_direct_target_mode() && has_azure_management_oidc_metadata(config)
}

fn is_local_azure_direct_target_mode() -> bool {
    std::env::var("AZURE_FEDERATED_TOKEN_FILE")
        .ok()
        .filter(|value| !value.is_empty())
        .is_none()
        && !(std::env::var("IDENTITY_ENDPOINT").is_ok() && std::env::var("IDENTITY_HEADER").is_ok())
}

fn manager_platforms_for_flow(flow: DistributionFlow, config: &TestConfig) -> Vec<Platform> {
    if let Some(base_platform) = flow.kubernetes_base_platform() {
        return vec![base_platform];
    }

    match flow {
        DistributionFlow::TerraformOnpremHelmPull => {
            [Platform::Aws, Platform::Gcp, Platform::Azure]
                .into_iter()
                .filter(|platform| config.has_platform(*platform))
                .collect()
        }
        _ => vec![flow.platform()],
    }
}

fn image_registry_platform_for_flow(
    flow: DistributionFlow,
    config: &TestConfig,
) -> anyhow::Result<Platform> {
    if let Some(base_platform) = flow.kubernetes_base_platform() {
        return Ok(base_platform);
    }

    match flow {
        DistributionFlow::CloudFormationAwsPush | DistributionFlow::TerraformAwsPush => {
            Ok(Platform::Aws)
        }
        DistributionFlow::TerraformGcpPush => Ok(Platform::Gcp),
        DistributionFlow::TerraformAzurePush => Ok(Platform::Azure),
        DistributionFlow::TerraformOnpremHelmPull => {
            [Platform::Aws, Platform::Gcp, Platform::Azure]
                .into_iter()
                .find(|platform| config.has_platform(*platform))
                .context(
                    "on-prem Helm distribution needs at least one cloud artifact registry config",
                )
        }
        DistributionFlow::CloudFormationEksHelmPull
        | DistributionFlow::TerraformEksHelmPull
        | DistributionFlow::TerraformGkeHelmPull
        | DistributionFlow::TerraformAksHelmPull => {
            unreachable!("managed Kubernetes flows returned earlier")
        }
    }
}

fn stack_contains_resource_type(stack: &Stack, resource_type: &str) -> bool {
    stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type().as_ref() == resource_type)
}

fn stack_settings_for_flow(model: DeploymentModel) -> StackSettings {
    let mut settings = StackSettings::default();
    if model == DeploymentModel::Pull {
        settings.deployment_model = StackDeploymentModel::Pull;
    }
    settings
}

fn e2e_stack_settings_for_flow(
    model: DeploymentModel,
    config: &TestConfig,
    platform: Platform,
) -> anyhow::Result<StackSettings> {
    let mut settings = stack_settings_for_flow(model);
    settings.network = config.e2e_network_settings(platform)?;
    Ok(settings)
}

fn stack_settings_for_terraform(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
) -> anyhow::Result<StackSettings> {
    let mut settings =
        e2e_stack_settings_for_flow(prepared.model, &prepared.config, prepared.platform)?;
    if let Some(exposure) = e2e_kubernetes_exposure(prepared.flow, prepared.app, target) {
        let mut kubernetes = settings.kubernetes.unwrap_or_else(|| KubernetesSettings {
            cluster: None,
            exposure: None,
        });
        kubernetes.exposure = Some(exposure);
        settings.kubernetes = Some(kubernetes);
    }
    if target.is_kubernetes()
        && prepared.config.kubernetes_cluster_mode == KubernetesClusterMode::Existing
    {
        set_kubernetes_cluster_ownership(&mut settings, KubernetesClusterOwnership::Existing);
    }

    if prepared.platform != Platform::Azure {
        return Ok(settings);
    }

    let Some(shared_env) = &prepared.config.azure_resources.shared_container_env else {
        return Ok(settings);
    };

    let binding = alien_core::ContainerAppsEnvironmentBinding::new(
        shared_env.environment_name.as_str(),
        shared_env.resource_id.as_str(),
        shared_env.resource_group.as_str(),
        shared_env.default_domain.as_str(),
    );
    let binding = if let Some(static_ip) = &shared_env.static_ip {
        binding.with_static_ip(static_ip.as_str())
    } else {
        binding
    };

    let mut external_bindings = settings.external_bindings.unwrap_or_default();
    external_bindings.insert(
        "default-container-env",
        ExternalBinding::ContainerAppsEnvironment(binding),
    );
    settings.external_bindings = Some(external_bindings);
    info!("Injected shared Container Apps Environment into Terraform stack settings");
    Ok(settings)
}

fn e2e_kubernetes_exposure(
    flow: DistributionFlow,
    app: TestApp,
    target: alien_terraform::TerraformTarget,
) -> Option<KubernetesExposureSettings> {
    if !matches!(
        flow,
        DistributionFlow::TerraformEksHelmPull | DistributionFlow::CloudFormationEksHelmPull
    ) || app != TestApp::FullStackMicroservices
        || target != alien_terraform::TerraformTarget::Eks
    {
        return None;
    }

    Some(KubernetesExposureSettings::Generated {
        route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            controller: Some("eks.amazonaws.com/alb".to_string()),
            ingress_class_name: "alb".to_string(),
            provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                scheme: "internet-facing".to_string(),
                target_type: "ip".to_string(),
                ip_address_type: None,
                subnet_ids: vec![],
            }),
            ..Default::default()
        }),
        certificate: KubernetesCertificateMode::None,
    })
}

fn render_management_config(
    platform: Platform,
    stack_settings: &StackSettings,
) -> Option<ManagementConfig> {
    match stack_settings.deployment_model {
        // Setup artifacts are the actor that imports deployment resources. Push
        // managers and pull agents both need a setup-authored management identity.
        StackDeploymentModel::Push | StackDeploymentModel::Pull => {}
    }

    match platform {
        Platform::Aws => Some(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: String::new(),
        })),
        Platform::Gcp => Some(ManagementConfig::Gcp(GcpManagementConfig {
            service_account_email: String::new(),
        })),
        Platform::Azure => Some(ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: String::new(),
            oidc_issuer: String::new(),
            oidc_subject: String::new(),
        })),
        Platform::Kubernetes | Platform::Local | Platform::Machines | Platform::Test => None,
    }
}

fn render_distribution_management_config(
    flow: DistributionFlow,
    platform: Platform,
    config: &TestConfig,
    stack_settings: &StackSettings,
) -> Option<ManagementConfig> {
    if platform == Platform::Azure
        && flow == DistributionFlow::TerraformAzurePush
        && config.azure_target.is_some()
        && is_local_azure_direct_target_mode()
    {
        return None;
    }

    render_management_config(platform, stack_settings)
}

async fn apply_render_mutations(
    stack: Stack,
    platform: Platform,
    stack_settings: &StackSettings,
) -> anyhow::Result<Stack> {
    apply_render_mutations_with_management_config(
        stack,
        platform,
        stack_settings,
        render_management_config(platform, stack_settings),
    )
    .await
}

async fn apply_render_mutations_with_management_config(
    stack: Stack,
    platform: Platform,
    stack_settings: &StackSettings,
    management_config: Option<ManagementConfig>,
) -> anyhow::Result<Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner.run_template_preflights(&stack, platform).await?;

    let stack_state = StackState::new(platform);
    let config = DeploymentConfig {
        input_values: Default::default(),
        deployment_name: Some(stack.id().to_string()),
        stack_settings: stack_settings.clone(),
        management_config,
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        base_platform: None,
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        public_endpoints: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    runner
        .apply_mutations(stack, &stack_state, &config)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
}

async fn create_release(
    manager: &Arc<TestManager>,
    platform: Platform,
    stack: &Stack,
) -> anyhow::Result<()> {
    let stack_json = serde_json::to_value(stack).context("Failed to serialize stack")?;
    let stack_by_platform = serde_json::json!({ platform.as_str(): stack_json });
    let stack_by_platform_sdk: alien_manager_api::types::StackByPlatform =
        serde_json::from_value(stack_by_platform)
            .context("Failed to convert stack to SDK StackByPlatform")?;

    manager
        .client()
        .create_release()
        .body(alien_manager_api::types::CreateReleaseRequest {
            stack: stack_by_platform_sdk,
            git_metadata: None,
            project_id: "default".to_string(),
        })
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to create release: {error}"))?;

    Ok(())
}

async fn create_deployment_group_token(
    manager: &Arc<TestManager>,
) -> anyhow::Result<(String, String)> {
    let group = manager
        .client()
        .create_deployment_group()
        .body(alien_manager_api::types::CreateDeploymentGroupRequest {
            name: format!(
                "e2e-distribution-{}",
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            max_deployments: None,
        })
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to create deployment group: {error}"))?
        .into_inner();

    let token = manager
        .client()
        .create_deployment_group_token()
        .id(&group.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to create deployment group token: {error}"))?
        .into_inner()
        .token;

    Ok((group.id, token))
}

async fn run_cloudformation_aws(
    prepared: &mut DistributionPrepared,
) -> anyhow::Result<TestContext> {
    let target = prepared
        .config
        .aws_target
        .as_ref()
        .context("AWS target config is required")?;
    let mgmt = prepared
        .config
        .aws_mgmt
        .as_ref()
        .context("AWS management config is required")?;
    let env = aws_env(target);
    let stack_name = crate::config::e2e_resource_prefix()?;
    let workdir = tempfile::tempdir().context("Failed to create CFN workdir")?;

    let registry = alien_cloudformation::CfRegistry::built_in();
    let template = alien_cloudformation::generate_cloudformation_template(
        &prepared.rendered_stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: e2e_stack_settings_for_flow(
                prepared.model,
                &prepared.config,
                prepared.platform,
            )?,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: alien_cloudformation::RegistrationMode::OutputsFallback,
            description: Some(format!("Alien E2E distribution stack {stack_name}")),
        },
    )
    .map_err(|error| anyhow::anyhow!("CloudFormation render failed: {error}"))?;
    let yaml = alien_cloudformation::to_yaml(&template)
        .map_err(|error| anyhow::anyhow!("CloudFormation serialization failed: {error}"))?;
    let template_path = workdir.path().join("template.yaml");
    fs::write(&template_path, yaml)
        .await
        .context("Failed to write CloudFormation template")?;

    let cleanup = DistributionArtifactCleanup::CloudFormation {
        stack_name: stack_name.clone(),
        region: target.region.clone(),
        env: env.clone(),
        retained_resources: Vec::new(),
        workdir: Some(workdir),
    };

    let role_arn = prepared
        .manager
        .management_config()
        .and_then(|config| match config {
            ManagementConfig::Aws(config) => Some(config.managing_role_arn),
            _ => None,
        })
        .context("AWS management role ARN is required")?;
    let managing_account_id = mgmt.account_id.clone().unwrap_or_default();

    let create_result = async {
        let mut create = Command::new("aws");
        create.args([
            "cloudformation",
            "create-stack",
            "--stack-name",
            &stack_name,
            "--template-body",
            &format!("file://{}", template_path.display()),
            "--capabilities",
            "CAPABILITY_IAM",
            "CAPABILITY_NAMED_IAM",
            "CAPABILITY_AUTO_EXPAND",
            "--region",
            &target.region,
            "--parameters",
            &format!("ParameterKey=Token,ParameterValue={}", prepared.dg_token),
            &format!("ParameterKey=ManagingRoleArn,ParameterValue={role_arn}"),
            &format!("ParameterKey=ManagingAccountId,ParameterValue={managing_account_id}"),
            "ParameterKey=DomainName,ParameterValue=",
            "ParameterKey=HostedZoneId,ParameterValue=",
            "ParameterKey=CertificateArn,ParameterValue=",
        ]);
        apply_env(&mut create, &env);
        run_command(create, "aws cloudformation create-stack").await?;

        let mut wait = Command::new("aws");
        wait.args([
            "cloudformation",
            "wait",
            "stack-create-complete",
            "--stack-name",
            &stack_name,
            "--region",
            &target.region,
        ]);
        apply_env(&mut wait, &env);
        wait_for_cloudformation_stack_create(&stack_name, &target.region, &env).await?;

        let request =
            cloudformation_import_request(&stack_name, &target.region, &env, &prepared.dg_token)
                .await?;
        let retained_resources = request.resources.clone();
        let imported = import_stack(prepared, request).await?;
        Ok((imported.deployment, retained_resources))
    }
    .await;

    match create_result {
        Ok((deployment, retained_resources)) => {
            let cleanup = DistributionArtifactCleanup::CloudFormation {
                stack_name,
                region: target.region.clone(),
                env,
                retained_resources,
                workdir: None,
            };
            Ok(context_from_deployment(prepared, deployment, vec![cleanup]))
        }
        Err(error) => Err(cleanup_after_setup_error(cleanup, error).await),
    }
}

async fn run_cloudformation_k8s(
    prepared: &mut DistributionPrepared,
) -> anyhow::Result<TestContext> {
    let target = prepared
        .config
        .aws_target
        .as_ref()
        .context("AWS target config is required")?;
    let mgmt = prepared
        .config
        .aws_mgmt
        .as_ref()
        .context("AWS management config is required")?;
    let env = aws_env(target);
    let stack_name = crate::config::e2e_resource_prefix()?;
    let workdir = tempfile::tempdir().context("Failed to create CFN EKS workdir")?;
    let kubeconfig_path = workdir.path().join("eks.kubeconfig");
    let helm_namespace = random_kubernetes_namespace("alien-test");

    let mut cleanup = DistributionArtifactCleanup::CloudFormation {
        stack_name: stack_name.clone(),
        region: target.region.clone(),
        env: env.clone(),
        retained_resources: Vec::new(),
        workdir: Some(workdir),
    };

    let create_result = async {
        let registry = alien_cloudformation::CfRegistry::built_in();
        let mut stack_settings =
            stack_settings_for_terraform(prepared, alien_terraform::TerraformTarget::Eks)?;
        set_kubernetes_namespace(&mut stack_settings, helm_namespace.clone());
        let cloudformation_stack = terraform_kubernetes_stack_for_target(
            prepared.built_stack.clone(),
            alien_terraform::TerraformTarget::Eks,
            stack_settings.clone(),
        )
        .await?;
        let template = alien_cloudformation::generate_cloudformation_template(
            &cloudformation_stack,
            alien_cloudformation::CloudFormationOptions {
                registry: &registry,
                target: alien_cloudformation::CloudFormationTarget::Eks,
                stack_settings,
                setup_target: "eks".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                registration: alien_cloudformation::RegistrationMode::OutputsFallback,
                description: Some(format!("Alien E2E EKS distribution stack {stack_name}")),
            },
        )
        .map_err(|error| anyhow::anyhow!("CloudFormation render failed: {error}"))?;
        let yaml = alien_cloudformation::to_yaml(&template)
            .map_err(|error| anyhow::anyhow!("CloudFormation serialization failed: {error}"))?;
        let template_path = match &cleanup {
            DistributionArtifactCleanup::CloudFormation {
                workdir: Some(workdir),
                ..
            } => workdir.path().join("template.yaml"),
            _ => anyhow::bail!("CloudFormation EKS cleanup workdir missing"),
        };
        fs::write(&template_path, yaml)
            .await
            .context("Failed to write CloudFormation template")?;

        let role_arn = prepared
            .manager
            .management_config()
            .and_then(|config| match config {
                ManagementConfig::Aws(config) => Some(config.managing_role_arn),
                _ => None,
            })
            .context("AWS management config is required")?;
        let managing_account_id = mgmt.account_id.clone().unwrap_or_default();

        let mut create = Command::new("aws");
        create.args([
            "cloudformation",
            "create-stack",
            "--stack-name",
            &stack_name,
            "--template-body",
            &format!("file://{}", template_path.display()),
            "--region",
            &target.region,
            "--capabilities",
            "CAPABILITY_IAM",
            "CAPABILITY_NAMED_IAM",
            "CAPABILITY_AUTO_EXPAND",
            "--parameters",
            &format!("ParameterKey=Token,ParameterValue={}", prepared.dg_token),
            &format!("ParameterKey=ManagingRoleArn,ParameterValue={role_arn}"),
            &format!("ParameterKey=ManagingAccountId,ParameterValue={managing_account_id}"),
            "ParameterKey=DomainName,ParameterValue=",
            "ParameterKey=HostedZoneId,ParameterValue=",
            "ParameterKey=CertificateArn,ParameterValue=",
            "ParameterKey=UpdatesMode,ParameterValue=auto",
            "ParameterKey=TelemetryMode,ParameterValue=auto",
            "ParameterKey=HeartbeatsMode,ParameterValue=on",
        ]);
        apply_env(&mut create, &env);
        run_command(create, "aws cloudformation create-stack").await?;

        wait_for_cloudformation_stack_create(&stack_name, &target.region, &env).await?;

        let outputs = cloudformation_outputs(&stack_name, &target.region, &env).await?;
        let request =
            cloudformation_import_request_from_outputs(&stack_name, &prepared.dg_token, &outputs)?;
        let retained_resources = request.resources.clone();
        let base_platform = request.base_platform;
        let region = request.region.clone();
        let imported = import_stack(prepared, request).await?;

        let mut kubeconfig = Command::new("aws");
        kubeconfig.args([
            "eks",
            "update-kubeconfig",
            "--name",
            &format!("{stack_name}-k8s"),
            "--region",
            &target.region,
            "--kubeconfig",
            &kubeconfig_path.to_string_lossy(),
        ]);
        apply_env(&mut kubeconfig, &env);
        run_command(kubeconfig, "aws eks update-kubeconfig").await?;

        Ok::<_, anyhow::Error>((imported, retained_resources, base_platform, region))
    }
    .await;

    let (imported, retained_resources, base_platform, region) = match create_result {
        Ok(result) => result,
        Err(error) => {
            return Err(cleanup_after_setup_error(cleanup, error).await);
        }
    };
    if let DistributionArtifactCleanup::CloudFormation {
        retained_resources: cleanup_retained,
        ..
    } = &mut cleanup
    {
        *cleanup_retained = retained_resources;
    }

    let chart_dir = match render_helm_chart(prepared).await {
        Ok(chart_dir) => chart_dir,
        Err(error) => {
            return Err(cleanup_after_setup_error(cleanup, error).await);
        }
    };
    let helm_target = KubernetesHelmTarget {
        namespace: helm_namespace,
        runtime: KubernetesRuntimeConfig {
            kubeconfig: kubeconfig_path.to_string_lossy().into_owned(),
            kube_context: None,
            namespace_prefix: "alien-test".to_string(),
        },
    };
    let values_file = match write_manager_fetch_values(
        prepared,
        &imported.deployment,
        &imported.stack_state,
        &imported.stack_settings,
        base_platform,
        &region,
        &chart_dir,
        Some(&helm_target),
    )
    .await
    {
        Ok(values_file) => values_file,
        Err(error) => {
            return Err(cleanup_after_setup_error(cleanup, error).await);
        }
    };
    let release = format!("alien-e2e-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let helm_cleanup = DistributionArtifactCleanup::Helm {
        release: release.clone(),
        namespace: helm_target.namespace.clone(),
        kubeconfig: Some(helm_target.runtime.kubeconfig.clone()),
        kube_context: None,
        env: env.clone(),
    };
    let agent_result = crate::operator::TestAlienOperator::helm_install_with_values(
        chart_dir.path(),
        &values_file,
        &release,
        &helm_target.namespace,
        Some(&helm_target.runtime.kubeconfig),
        None,
        &env,
    )
    .await
    .map_err(|error| error.to_string());
    let agent = match agent_result {
        Ok(agent) => agent,
        Err(error) => {
            let error = anyhow::anyhow!(
                "Failed to install CloudFormation Helm distribution runtime: {error}"
            );
            return Err(
                cleanup_after_ordered_setup_error(vec![helm_cleanup, cleanup], error).await,
            );
        }
    };

    let mut ctx =
        context_from_deployment(prepared, imported.deployment, vec![helm_cleanup, cleanup]);
    ctx.agent = Some(agent);
    Ok(ctx)
}

fn set_kubernetes_namespace(settings: &mut StackSettings, namespace: String) {
    let mut kubernetes = settings.kubernetes.take().unwrap_or(KubernetesSettings {
        cluster: None,
        exposure: None,
    });
    let mut cluster = kubernetes.cluster.unwrap_or(KubernetesClusterSettings {
        ownership: KubernetesClusterOwnership::Managed,
        namespace: None,
        cloud: None,
    });
    cluster.namespace = Some(namespace);
    kubernetes.cluster = Some(cluster);
    settings.kubernetes = Some(kubernetes);
}

fn set_kubernetes_cluster_ownership(
    settings: &mut StackSettings,
    ownership: KubernetesClusterOwnership,
) {
    let mut kubernetes = settings.kubernetes.take().unwrap_or(KubernetesSettings {
        cluster: None,
        exposure: None,
    });
    let mut cluster = kubernetes.cluster.unwrap_or(KubernetesClusterSettings {
        ownership,
        namespace: None,
        cloud: None,
    });
    cluster.ownership = ownership;
    kubernetes.cluster = Some(cluster);
    settings.kubernetes = Some(kubernetes);
}

fn rewrite_push_distribution_images(
    mut stack: Stack,
    platform: Platform,
    config: &TestConfig,
) -> anyhow::Result<Stack> {
    let Some(native_host) = native_image_host_for_distribution(platform, config)? else {
        return Ok(stack);
    };

    for (_id, entry) in stack.resources_mut() {
        let Some(worker) = entry.config.downcast_mut::<Worker>() else {
            continue;
        };
        let WorkerCode::Image { image } = &mut worker.code else {
            continue;
        };
        if let Some(rewritten) =
            alien_core::image_rewrite::resolve_native_image_uri(image, &native_host)
        {
            *image = rewritten;
        }
    }

    Ok(stack)
}

fn native_image_host_for_distribution(
    platform: Platform,
    config: &TestConfig,
) -> anyhow::Result<Option<String>> {
    match platform {
        Platform::Aws => {
            let account_id = config
                .aws_mgmt
                .as_ref()
                .and_then(|aws| aws.account_id.as_deref())
                .context("AWS management account ID is required for AWS distribution images")?;
            let region = config
                .aws_target
                .as_ref()
                .or(config.aws_mgmt.as_ref())
                .map(|aws| aws.region.as_str())
                .context("AWS region is required for AWS distribution images")?;
            Ok(Some(format!("{account_id}.dkr.ecr.{region}.amazonaws.com")))
        }
        Platform::Gcp => {
            let host = config
                .e2e_artifact_registry
                .gcp_gar_repository
                .as_deref()
                .and_then(|repository| {
                    let endpoint = alien_core::image_rewrite::strip_url_scheme(repository);
                    endpoint.split_once('/').and_then(|(host, _path)| {
                        if host.contains('.') || host.contains(':') {
                            Some(host.to_string())
                        } else {
                            None
                        }
                    })
                })
                .or_else(|| {
                    config
                        .gcp_mgmt
                        .as_ref()
                        .map(|gcp| format!("{}-docker.pkg.dev", gcp.region))
                })
                .context("GCP Artifact Registry host is required for GCP distribution images")?;
            Ok(Some(host))
        }
        _ => Ok(None),
    }
}

async fn run_terraform_cloud(
    prepared: &mut DistributionPrepared,
    target: alien_terraform::TerraformTarget,
) -> anyhow::Result<TestContext> {
    let result = apply_terraform_and_import(prepared, target, None).await?;
    Ok(context_from_deployment(
        prepared,
        result.deployment,
        vec![result.cleanup],
    ))
}

async fn run_terraform_k8s(
    prepared: &mut DistributionPrepared,
    target: alien_terraform::TerraformTarget,
) -> anyhow::Result<TestContext> {
    let existing_helm_target = match prepared.config.kubernetes_cluster_mode {
        KubernetesClusterMode::Existing => Some(required_kubernetes_helm_target(prepared, target)?),
        KubernetesClusterMode::Create => None,
    };
    let namespace = existing_helm_target
        .as_ref()
        .map(|target| target.namespace.clone())
        .unwrap_or_else(|| random_kubernetes_namespace("alien-test"));
    let result = apply_terraform_and_import(prepared, target, Some(&namespace)).await?;
    let mut helm_target = match existing_helm_target {
        Some(helm_target) => helm_target,
        None => match kubernetes_helm_target_from_outputs(&result.outputs, namespace) {
            Ok(helm_target) => helm_target,
            Err(error) => {
                return Err(cleanup_after_setup_error(result.cleanup, error).await);
            }
        },
    };
    if let Err(error) = materialize_kubeconfig_for_helm(&mut helm_target, &result.cleanup).await {
        return Err(cleanup_after_setup_error(result.cleanup, error).await);
    }
    let mut kubernetes_command_env = result.cleanup.command_env().to_vec();
    if let Err(error) = configure_kubeconfig_auth_for_helm(
        prepared,
        target,
        &helm_target,
        &mut kubernetes_command_env,
    )
    .await
    {
        return Err(cleanup_after_setup_error(result.cleanup, error).await);
    }
    if terraform_handoff_debug_enabled() {
        return stop_before_helm_for_terraform_handoff_debug(
            prepared,
            target,
            result,
            &helm_target,
        );
    }
    let chart_dir = match render_helm_chart(prepared).await {
        Ok(chart_dir) => chart_dir,
        Err(error) => {
            return Err(cleanup_after_setup_error(result.cleanup, error).await);
        }
    };
    let values_file = match write_manager_fetch_values(
        prepared,
        &result.deployment,
        &result.stack_state,
        &result.stack_settings,
        result.base_platform,
        &result.region,
        &chart_dir,
        Some(&helm_target),
    )
    .await
    {
        Ok(values_file) => values_file,
        Err(error) => {
            return Err(cleanup_after_setup_error(result.cleanup, error).await);
        }
    };
    let release = format!("alien-e2e-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let helm_cleanup = DistributionArtifactCleanup::Helm {
        release: release.clone(),
        namespace: helm_target.namespace.clone(),
        kubeconfig: Some(helm_target.runtime.kubeconfig.clone()),
        kube_context: helm_target.runtime.kube_context.clone(),
        env: kubernetes_command_env.clone(),
    };
    let agent_result = crate::operator::TestAlienOperator::helm_install_with_values(
        chart_dir.path(),
        &values_file,
        &release,
        &helm_target.namespace,
        Some(&helm_target.runtime.kubeconfig),
        helm_target.runtime.kube_context.as_deref(),
        &kubernetes_command_env,
    )
    .await
    .map_err(|error| error.to_string());
    let agent = match agent_result {
        Ok(agent) => agent,
        Err(error) => {
            let error = anyhow::anyhow!("Failed to install Helm distribution runtime: {error}");
            return Err(cleanup_after_ordered_setup_error(
                vec![helm_cleanup, result.cleanup],
                error,
            )
            .await);
        }
    };

    let mut ctx = context_from_deployment(
        prepared,
        result.deployment,
        vec![helm_cleanup, result.cleanup],
    );
    ctx.agent = Some(agent);
    Ok(ctx)
}

fn terraform_handoff_debug_enabled() -> bool {
    env::var("ALIEN_E2E_DEBUG_TERRAFORM_HANDOFF")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn stop_before_helm_for_terraform_handoff_debug(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
    result: TerraformApplyResult,
    helm_target: &KubernetesHelmTarget,
) -> anyhow::Result<TestContext> {
    let TerraformApplyResult {
        deployment: _,
        cleanup,
        outputs,
        ..
    } = result;
    let DistributionArtifactCleanup::Terraform { workdir, env: _ } = cleanup else {
        anyhow::bail!("Terraform handoff debug requires Terraform cleanup state");
    };
    let workdir = workdir.keep();

    let mut commands = vec![
        format!("cd {}", shell_quote_path(&workdir)),
        format!(
            "kubectl --kubeconfig {} --context {} config view --minify",
            shell_quote(&helm_target.runtime.kubeconfig),
            shell_quote(helm_target.runtime.kube_context.as_deref().unwrap_or(""))
        ),
        format!(
            "kubectl --kubeconfig {} --context {} get --raw=/readyz",
            shell_quote(&helm_target.runtime.kubeconfig),
            shell_quote(helm_target.runtime.kube_context.as_deref().unwrap_or(""))
        ),
        format!(
            "kubectl --kubeconfig {} --context {} auth can-i create namespaces",
            shell_quote(&helm_target.runtime.kubeconfig),
            shell_quote(helm_target.runtime.kube_context.as_deref().unwrap_or(""))
        ),
    ];

    if target == alien_terraform::TerraformTarget::Eks {
        if let Some(aws) = prepared.config.aws_target.as_ref() {
            if let Ok(cluster_name) = terraform_output_string(&outputs, "kubernetes_kube_context") {
                commands.insert(
                    1,
                    "export AWS_ACCESS_KEY_ID=\"$AWS_TARGET_ACCESS_KEY_ID\"".to_string(),
                );
                commands.insert(
                    2,
                    "export AWS_SECRET_ACCESS_KEY=\"$AWS_TARGET_SECRET_ACCESS_KEY\"".to_string(),
                );
                commands.insert(
                    3,
                    "export AWS_SESSION_TOKEN=\"${AWS_TARGET_SESSION_TOKEN:-}\"".to_string(),
                );
                commands.insert(
                    4,
                    "export AWS_DEFAULT_REGION=\"$AWS_TARGET_REGION\"".to_string(),
                );
                commands.insert(5, "export AWS_REGION=\"$AWS_TARGET_REGION\"".to_string());
                commands.insert(6, "aws sts get-caller-identity".to_string());
                commands.insert(
                    7,
                    format!(
                        "aws eks get-token --cluster-name {} --region {} >/tmp/alien-e2e-eks-token.json",
                        shell_quote(&cluster_name),
                        shell_quote(&aws.region)
                    ),
                );
            }
        }
    }

    commands.push("helm version".to_string());
    commands.push("terraform destroy -auto-approve -input=false -lock-timeout=5m".to_string());

    anyhow::bail!(
        "ALIEN_E2E_DEBUG_TERRAFORM_HANDOFF stopped after Terraform apply and before Helm install.\n\
         Terraform workdir: {}\n\
         Kubeconfig: {}\n\
         Kube context: {}\n\
         Namespace: {}\n\
         No automatic cleanup ran. Run the destroy command below when finished.\n\n{}",
        workdir.display(),
        helm_target.runtime.kubeconfig,
        helm_target.runtime.kube_context.as_deref().unwrap_or("<none>"),
        helm_target.namespace,
        commands.join("\n")
    )
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

async fn run_onprem_k8s(_prepared: &mut DistributionPrepared) -> anyhow::Result<TestContext> {
    anyhow::bail!(
        "On-prem Helm local-import distribution needs a complete external binding fixture for comprehensive-rust"
    )
}

async fn apply_terraform_and_import(
    prepared: &mut DistributionPrepared,
    target: alien_terraform::TerraformTarget,
    kubernetes_namespace: Option<&str>,
) -> anyhow::Result<TerraformApplyResult> {
    let workdir = tempfile::tempdir().context("Failed to create Terraform workdir")?;
    let registry = alien_terraform::TfRegistry::built_in();
    let stack_settings = stack_settings_for_terraform(prepared, target)?;
    let terraform_stack = terraform_stack_for_target(prepared, target, &stack_settings).await?;
    let has_remote_management =
        stack_contains_resource_type(&terraform_stack, "remote-stack-management");
    let module = alien_terraform::generate_terraform_module(
        &terraform_stack,
        target,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .map_err(|error| anyhow::anyhow!("Terraform render failed: {error}"))?;

    for (path, contents) in module.iter() {
        let path = workdir.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, contents).await?;
    }

    let env = terraform_env(&prepared.config, target.cloud_platform())?;
    let tfvars = terraform_tfvars(prepared, target, kubernetes_namespace)?;
    fs::write(
        workdir.path().join("terraform.tfvars.json"),
        serde_json::to_vec_pretty(&tfvars)?,
    )
    .await?;

    let workdir_path = workdir.path().to_path_buf();
    let apply_result = async {
        run_terraform_cmd(
            &workdir_path,
            &env,
            ["init", "-backend=false", "-input=false"],
        )
        .await?;
        run_terraform_cmd(&workdir_path, &env, ["validate"]).await?;
        run_terraform_cmd(
            &workdir_path,
            &env,
            ["apply", "-auto-approve", "-input=false"],
        )
        .await?;

        let outputs = terraform_output_json(&workdir_path, &env).await?;
        if target.cloud_platform() == Platform::Azure && has_remote_management {
            grant_terraform_shared_env_join_permission(&prepared.config, &outputs).await?;
        }
        if target.cloud_platform() == Platform::Gcp {
            wait_for_gcp_management_permissions(&prepared.config, &outputs, has_remote_management)
                .await?;
        }
        if target.cloud_platform() == Platform::Azure
            && has_remote_management
            && has_azure_management_oidc(&prepared.config)
        {
            wait_for_azure_management_permissions(&prepared.config, &outputs).await?;
        } else if target.cloud_platform() == Platform::Azure && has_remote_management {
            info!(
                "Skipping Azure management OIDC permission probe; local AKS run will validate the same management identity through in-cluster workload identity"
            );
        }
        let request = terraform_import_request_from_outputs(&outputs, &prepared.dg_token)?;
        let base_platform = request.base_platform;
        let region = request.region.clone();
        let imported = import_stack(prepared, request).await?;
        Ok::<_, anyhow::Error>((imported, outputs, base_platform, region))
    }
    .await;

    let cleanup = DistributionArtifactCleanup::Terraform {
        workdir,
        env: env.clone(),
    };
    match apply_result {
        Ok((imported, outputs, base_platform, region)) => Ok(TerraformApplyResult {
            deployment: imported.deployment,
            cleanup,
            outputs,
            stack_state: imported.stack_state,
            stack_settings: imported.stack_settings,
            base_platform,
            region,
        }),
        Err(error) => Err(cleanup_after_setup_error(cleanup, error).await),
    }
}

async fn terraform_stack_for_target(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
    stack_settings: &StackSettings,
) -> anyhow::Result<Stack> {
    if !target.is_kubernetes() {
        return Ok(prepared.rendered_stack.clone());
    }

    terraform_kubernetes_stack_for_target(
        prepared.built_stack.clone(),
        target,
        stack_settings.clone(),
    )
    .await
}

async fn terraform_kubernetes_stack_for_target(
    stack: Stack,
    target: alien_terraform::TerraformTarget,
    stack_settings: StackSettings,
) -> anyhow::Result<Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await?;

    let stack_state = StackState::new(Platform::Kubernetes);
    let config = DeploymentConfig {
        input_values: Default::default(),
        deployment_name: Some(stack.id().to_string()),
        stack_settings: stack_settings.clone(),
        management_config: render_management_config(target.cloud_platform(), &stack_settings),
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        base_platform: target.base_platform(),
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        public_endpoints: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    runner
        .apply_mutations(stack, &stack_state, &config)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
}

async fn render_helm_chart(prepared: &DistributionPrepared) -> anyhow::Result<TempDir> {
    let (stack, stack_settings) = render_helm_stack(prepared).await?;
    let registry = alien_helm::HelmRegistry::built_in();
    let chart = alien_helm::generate_helm_chart(
        &stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings,
            chart_name: format!("alien-e2e-{}", prepared.app),
        },
    )
    .map_err(|error| anyhow::anyhow!("Helm chart render failed: {error}"))?;

    let chart_dir = tempfile::tempdir().context("Failed to create Helm chart workdir")?;
    for (path, contents) in &chart.files {
        let path = chart_dir.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, contents).await?;
    }
    Ok(chart_dir)
}

async fn render_helm_stack(
    prepared: &DistributionPrepared,
) -> anyhow::Result<(Stack, StackSettings)> {
    let stack_settings =
        e2e_stack_settings_for_flow(prepared.model, &prepared.config, Platform::Kubernetes)?;
    let stack = apply_render_mutations(
        prepared.built_stack.clone(),
        Platform::Kubernetes,
        &stack_settings,
    )
    .await
    .context("Failed to apply Helm render preflights")?;
    Ok((stack, stack_settings))
}

async fn write_manager_fetch_values(
    prepared: &DistributionPrepared,
    deployment: &TestDeployment,
    stack_state: &StackState,
    stack_settings: &StackSettings,
    base_platform: Option<Platform>,
    region: &str,
    chart_dir: &TempDir,
    helm_target: Option<&KubernetesHelmTarget>,
) -> anyhow::Result<std::path::PathBuf> {
    let (stack, _) = render_helm_stack(prepared).await?;
    let values_yaml =
        alien_helm::render_manager_fetch_values(alien_helm::ManagerFetchHelmValuesOptions {
            deployment_id: &deployment.id,
            deployment_name: &deployment.name,
            manager_url: &prepared.manager.public_url,
            deployment_token: &deployment.token,
            runtime_encryption_key: &crate::operator::generate_encryption_key(),
            stack: &stack,
            stack_state,
            stack_settings,
            base_platform,
            region: Some(region),
            gcp_project_id: prepared
                .config
                .gcp_target
                .as_ref()
                .map(|target| target.project_id.as_str()),
            azure_location: prepared
                .config
                .azure_target
                .as_ref()
                .map(|target| target.region.as_str())
                .or(Some(region)),
        })
        .map_err(|error| {
            anyhow::anyhow!("Failed to render Helm values from import state: {error}")
        })?;
    let values: Value =
        serde_yaml::from_str(&values_yaml).context("Failed to parse rendered Helm values")?;
    write_manager_fetch_values_from_base(prepared, deployment, values, chart_dir, helm_target).await
}

async fn write_manager_fetch_values_from_base(
    prepared: &DistributionPrepared,
    deployment: &TestDeployment,
    mut values: Value,
    chart_dir: &TempDir,
    helm_target: Option<&KubernetesHelmTarget>,
) -> anyhow::Result<std::path::PathBuf> {
    let values_object = values
        .as_object_mut()
        .context("rendered Helm values must be a JSON object")?;
    values_object.insert(
        "management".to_string(),
        serde_json::json!({
            "token": deployment.token.clone(),
            "name": deployment.name.clone(),
            "url": prepared.manager.public_url.clone(),
            "deploymentId": deployment.id.clone(),
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        }),
    );
    values_object.insert("infrastructure".to_string(), Value::Null);
    merge_runtime_values(values_object, runtime_values()?)?;
    if helm_target.is_some() {
        let chart_values = fs::read_to_string(chart_dir.path().join("values.yaml"))
            .await
            .context("Failed to read generated chart values.yaml")?;
        let chart_values: Value =
            serde_yaml::from_str(&chart_values).context("Failed to parse chart values.yaml")?;
        merge_chart_service_values(values_object, &chart_values)?;
    }

    let values_path = chart_dir.path().join("distribution-values.yaml");
    fs::write(&values_path, to_helm_values_yaml(&values)?)
        .await
        .context("Failed to write Helm distribution values")?;
    Ok(values_path)
}

fn merge_runtime_values(
    values_object: &mut serde_json::Map<String, Value>,
    runtime: Value,
) -> anyhow::Result<()> {
    let runtime_object = runtime
        .as_object()
        .context("runtime override values must be a JSON object")?;
    match values_object.get_mut("runtime") {
        Some(Value::Object(existing_runtime)) => {
            for (key, value) in runtime_object {
                existing_runtime.insert(key.clone(), value.clone());
            }
        }
        Some(_) | None => {
            values_object.insert("runtime".to_string(), runtime);
        }
    }
    Ok(())
}

fn runtime_values() -> anyhow::Result<Value> {
    let image = std::env::var("ALIEN_TEST_OVERRIDE_OPERATOR_IMAGE")
        .ok()
        .filter(|image| !image.is_empty())
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-operator:latest".to_string());
    runtime_values_for_image(&image)
}

fn runtime_values_for_image(image: &str) -> anyhow::Result<Value> {
    let (repository, tag) = split_image_tag(&image)?;
    let mut runtime = serde_json::json!({
        "image": {
            "repository": repository,
            "tag": tag,
            "pullPolicy": "IfNotPresent",
        },
        "encryption": {
            "key": crate::operator::generate_encryption_key(),
        }
    });
    if let Some(image_pull_secrets) = runtime_image_pull_secrets(&repository) {
        runtime["imagePullSecrets"] = image_pull_secrets;
    }
    Ok(runtime)
}

fn merge_chart_service_values(
    values_object: &mut serde_json::Map<String, Value>,
    chart_values: &Value,
) -> anyhow::Result<()> {
    let Some(chart_services) = chart_values.get("services") else {
        return Ok(());
    };
    let chart_services = chart_services
        .as_object()
        .context("generated chart values services must be an object")?;

    match values_object.get_mut("services") {
        Some(Value::Object(existing_services)) => {
            for (resource_id, chart_service) in chart_services {
                match existing_services.get_mut(resource_id) {
                    Some(Value::Object(existing_service)) => {
                        let Some(chart_service) = chart_service.as_object() else {
                            continue;
                        };
                        merge_missing_values(existing_service, chart_service);
                    }
                    Some(_) => {}
                    None => {
                        existing_services.insert(resource_id.clone(), chart_service.clone());
                    }
                }
            }
        }
        Some(Value::Null) | None => {
            values_object.insert(
                "services".to_string(),
                Value::Object(chart_services.clone()),
            );
        }
        Some(_) => {
            anyhow::bail!("helm_values services must be an object when provided");
        }
    }

    Ok(())
}

fn merge_missing_values(
    target: &mut serde_json::Map<String, Value>,
    defaults: &serde_json::Map<String, Value>,
) {
    for (key, default_value) in defaults {
        match (target.get_mut(key), default_value) {
            (Some(Value::Object(target_object)), Value::Object(default_object)) => {
                merge_missing_values(target_object, default_object);
            }
            (Some(_), _) => {}
            (None, _) => {
                target.insert(key.clone(), default_value.clone());
            }
        }
    }
}

fn split_image_tag(image: &str) -> anyhow::Result<(String, String)> {
    if image.contains('@') {
        anyhow::bail!(
            "ALIEN_TEST_OVERRIDE_OPERATOR_IMAGE must use a tag for Helm E2E installs; digest references are not supported yet"
        );
    }
    let last_slash = image.rfind('/');
    let last_colon = image.rfind(':');
    if let Some(colon) =
        last_colon.filter(|colon| last_slash.map(|slash| *colon > slash).unwrap_or(true))
    {
        Ok((image[..colon].to_string(), image[colon + 1..].to_string()))
    } else {
        Ok((image.to_string(), "latest".to_string()))
    }
}

fn required_kubernetes_helm_target(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
) -> anyhow::Result<KubernetesHelmTarget> {
    let runtime = match target {
        alien_terraform::TerraformTarget::Eks => prepared
            .config
            .kubernetes
            .eks
            .as_ref()
            .map(|config| config.runtime.clone())
            .context("Terraform EKS Helm distribution requires ALIEN_TEST_EKS_CLUSTER_NAME and KUBECONFIG (or ALIEN_TEST_EKS_KUBECONFIG)")?,
        alien_terraform::TerraformTarget::Gke => prepared
            .config
            .kubernetes
            .gke
            .as_ref()
            .map(|config| config.runtime.clone())
            .context("Terraform GKE Helm distribution requires ALIEN_TEST_GKE_CLUSTER_NAME, ALIEN_TEST_GKE_CLUSTER_LOCATION, and KUBECONFIG (or ALIEN_TEST_GKE_KUBECONFIG)")?,
        alien_terraform::TerraformTarget::Aks => prepared
            .config
            .kubernetes
            .aks
            .as_ref()
            .map(|config| config.runtime.clone())
            .context("Terraform AKS Helm distribution requires ALIEN_TEST_AKS_CLUSTER_NAME, ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP, and KUBECONFIG (or ALIEN_TEST_AKS_KUBECONFIG)")?,
        _ => anyhow::bail!("{} is not a Kubernetes Helm target", target.name()),
    };

    Ok(KubernetesHelmTarget {
        namespace: random_kubernetes_namespace(&runtime.namespace_prefix),
        runtime,
    })
}

fn kubernetes_helm_target_from_outputs(
    outputs: &Value,
    namespace: String,
) -> anyhow::Result<KubernetesHelmTarget> {
    Ok(KubernetesHelmTarget {
        namespace,
        runtime: KubernetesRuntimeConfig {
            kubeconfig: terraform_output_string(outputs, "kubernetes_kubeconfig")?,
            kube_context: terraform_output_string(outputs, "kubernetes_kube_context")
                .ok()
                .filter(|value| !value.is_empty()),
            namespace_prefix: "alien-test".to_string(),
        },
    })
}

async fn materialize_kubeconfig_for_helm(
    target: &mut KubernetesHelmTarget,
    cleanup: &DistributionArtifactCleanup,
) -> anyhow::Result<()> {
    if Path::new(&target.runtime.kubeconfig).exists() {
        return Ok(());
    }
    if !looks_like_kubeconfig_contents(&target.runtime.kubeconfig) {
        return Ok(());
    }

    let DistributionArtifactCleanup::Terraform { workdir, .. } = cleanup else {
        anyhow::bail!("Terraform-generated kubeconfig contents require a Terraform workdir");
    };

    let path = workdir.path().join("kubernetes.kubeconfig");
    write_kubeconfig_file(&path, &target.runtime.kubeconfig).await?;
    target.runtime.kubeconfig = path.to_string_lossy().into_owned();
    Ok(())
}

async fn configure_kubeconfig_auth_for_helm(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
    helm_target: &KubernetesHelmTarget,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    if target != alien_terraform::TerraformTarget::Aks {
        return Ok(());
    }
    if prepared.config.kubernetes_cluster_mode != KubernetesClusterMode::Create {
        return Ok(());
    }

    let azure = prepared
        .config
        .azure_target
        .as_ref()
        .context("Terraform AKS Helm distribution requires Azure target credentials")?;
    upsert_env(env, "AZURE_CLIENT_ID", azure.client_id.clone());
    upsert_env(env, "AZURE_CLIENT_SECRET", azure.client_secret.clone());
    upsert_env(env, "AZURE_TENANT_ID", azure.tenant_id.clone());

    let mut cmd = Command::new("kubelogin");
    cmd.args(["convert-kubeconfig", "-l", "spn"]);
    cmd.env("KUBECONFIG", &helm_target.runtime.kubeconfig);
    apply_env(&mut cmd, env);
    run_command(cmd, "kubelogin convert-kubeconfig").await
}

fn upsert_env(env: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing, _)| existing == key) {
        *existing = value;
    } else {
        env.push((key.to_string(), value));
    }
}

async fn write_kubeconfig_file(path: &Path, contents: &str) -> anyhow::Result<()> {
    fs::write(path, contents).await.with_context(|| {
        format!(
            "Failed to write Kubernetes kubeconfig to {}",
            path.display()
        )
    })
}

fn looks_like_kubeconfig_contents(value: &str) -> bool {
    let trimmed = value.trim_start();
    (trimmed.starts_with("apiVersion:") || trimmed.starts_with("\"apiVersion\""))
        && (trimmed.contains("\nclusters:") || trimmed.contains("\n\"clusters\""))
        && (trimmed.contains("\nusers:") || trimmed.contains("\n\"users\""))
}

fn random_kubernetes_namespace(prefix: &str) -> String {
    let prefix = sanitize_kubernetes_dns_label(prefix);
    format!("{prefix}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn sanitize_kubernetes_dns_label(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else if !last_dash {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        out.push(next);
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "alien-test".to_string()
    } else {
        out.chars()
            .take(54)
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    }
}

async fn import_stack(
    prepared: &DistributionPrepared,
    request: StackImportRequest,
) -> anyhow::Result<ImportedTestDeployment> {
    let url = format!("{}/v1/stack/import", prepared.manager.url);
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&prepared.dg_token)
        .json(&request)
        .send()
        .await
        .context("Failed to call /v1/stack/import")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("stack import failed with {status}: {body}");
    }

    let response: StackImportResponse =
        serde_json::from_str(&body).context("Failed to parse StackImportResponse")?;
    let dep = prepared
        .manager
        .client()
        .get_deployment()
        .id(&response.deployment_id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to fetch imported deployment: {error}"))?
        .into_inner();

    let token = prepared
        .manager
        .create_deployment_token(&prepared.group_id, &response.deployment_id)
        .await?;

    Ok(ImportedTestDeployment {
        deployment: TestDeployment::new(
            response.deployment_id,
            dep.name,
            prepared.platform.as_str().to_string(),
            None,
            token,
            prepared.manager.clone(),
        ),
        stack_state: response.stack_state,
        stack_settings: response.stack_settings,
    })
}

fn context_from_deployment(
    prepared: &DistributionPrepared,
    deployment: TestDeployment,
    cleanups: Vec<DistributionArtifactCleanup>,
) -> TestContext {
    TestContext {
        deployment,
        manager: prepared.manager.clone(),
        platform: prepared.platform,
        model: prepared.model,
        app: prepared.app,
        agent: None,
        distribution_cleanups: cleanups,
    }
}

async fn wait_and_finalize(ctx: &mut TestContext) -> anyhow::Result<()> {
    let timeout = deployment_running_timeout(ctx.platform, ctx.app);
    ctx.deployment
        .wait_until_running(timeout)
        .await
        .map_err(|error| {
            anyhow::anyhow!("Deployment failed to reach running within {timeout:?}: {error}")
        })?;
    if ctx.model == DeploymentModel::Push
        && matches!(
            ctx.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        )
    {
        provision_managed_test_secret(&ctx.manager, &ctx.deployment).await?;
    }
    Ok(())
}

fn deployment_running_timeout(platform: Platform, app: TestApp) -> Duration {
    match (platform, app) {
        (Platform::Kubernetes, TestApp::FullStackMicroservices) => {
            KUBERNETES_FULL_STACK_DEPLOYMENT_RUNNING_TIMEOUT
        }
        (Platform::Azure, _) => AZURE_DEPLOYMENT_RUNNING_TIMEOUT,
        (Platform::Kubernetes, _) => KUBERNETES_DEPLOYMENT_RUNNING_TIMEOUT,
        _ => DEFAULT_DEPLOYMENT_RUNNING_TIMEOUT,
    }
}

async fn cloudformation_import_request(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
    token: &str,
) -> anyhow::Result<StackImportRequest> {
    let values = cloudformation_outputs(stack_name, region, env).await?;
    cloudformation_import_request_from_outputs(stack_name, token, &values)
}

async fn cloudformation_outputs(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut cmd = Command::new("aws");
    cmd.args([
        "cloudformation",
        "describe-stacks",
        "--stack-name",
        stack_name,
        "--region",
        region,
        "--query",
        "Stacks[0].Outputs",
        "--output",
        "json",
    ]);
    apply_env(&mut cmd, env);
    let output = command_output(cmd, "aws cloudformation describe-stacks").await?;
    let outputs: Vec<BTreeMap<String, String>> = serde_json::from_slice(&output.stdout)?;

    let mut values = BTreeMap::new();
    for output in outputs {
        if let (Some(key), Some(value)) = (output.get("OutputKey"), output.get("OutputValue")) {
            values.insert(key.clone(), value.clone());
        }
    }
    Ok(values)
}

fn cloudformation_import_request_from_outputs(
    stack_name: &str,
    token: &str,
    values: &BTreeMap<String, String>,
) -> anyhow::Result<StackImportRequest> {
    let platform: Platform = values
        .get("DeploymentPlatform")
        .context("DeploymentPlatform output missing")?
        .parse()
        .map_err(|error| anyhow::anyhow!("Invalid DeploymentPlatform output: {error}"))?;
    let base_platform = values
        .get("DeploymentBasePlatform")
        .map(|value| {
            value
                .parse()
                .map_err(|error| anyhow::anyhow!("Invalid DeploymentBasePlatform output: {error}"))
        })
        .transpose()?;
    let resource_prefix = values
        .get("DeploymentResourcePrefix")
        .cloned()
        .context("DeploymentResourcePrefix output missing")?;
    let region = values
        .get("DeploymentRegion")
        .cloned()
        .context("DeploymentRegion output missing")?;
    let setup_target = values
        .get("DeploymentSetupTarget")
        .cloned()
        .context("DeploymentSetupTarget output missing")?;
    let setup_import_format_version = values
        .get("DeploymentSetupImportFormatVersion")
        .map(|value| {
            value
                .parse()
                .context("DeploymentSetupImportFormatVersion output is invalid")
        })
        .transpose()?
        .unwrap_or(1);
    let setup_fingerprint = values
        .get("DeploymentSetupFingerprint")
        .cloned()
        .context("DeploymentSetupFingerprint output missing")?;
    let setup_fingerprint_version: u32 = values
        .get("DeploymentSetupFingerprintVersion")
        .context("DeploymentSetupFingerprintVersion output missing")?
        .parse()
        .context("DeploymentSetupFingerprintVersion output is invalid")?;
    let management_config =
        parse_json_output::<ManagementConfig>(&values, "DeploymentManagementConfig")?;
    let stack_settings = parse_json_output::<StackSettings>(&values, "DeploymentStackSettings")?;
    let resources = parse_cfn_resources(&values)?;

    Ok(StackImportRequest {
        setup_import_format_version,
        deployment_group_token: token.to_string(),
        deployment_name: stack_name.to_string(),
        resource_prefix,
        source_kind: Some(ImportSourceKind::CloudFormation),
        setup_metadata: None,
        release_id: None,
        platform,
        base_platform,
        region,
        setup_target,
        setup_fingerprint,
        setup_fingerprint_version,
        stack_settings,
        management_config: Some(management_config),
        input_values: Default::default(),
        resources,
    })
}

fn parse_cfn_resources(values: &BTreeMap<String, String>) -> anyhow::Result<Vec<ImportedResource>> {
    if let Some(resources) = values.get("DeploymentResources") {
        return serde_json::from_str(resources).context("Failed to parse DeploymentResources");
    }

    let mut chunks = values
        .iter()
        .filter_map(|(key, value)| {
            let suffix = key.strip_prefix("DeploymentResources")?;
            let index = suffix.parse::<usize>().ok()?;
            Some((index, value))
        })
        .collect::<Vec<_>>();
    chunks.sort_by_key(|(index, _)| *index);

    let mut resources = Vec::new();
    for (_index, chunk) in chunks {
        let mut parsed: Vec<ImportedResource> =
            serde_json::from_str(chunk).context("Failed to parse DeploymentResources chunk")?;
        resources.append(&mut parsed);
    }
    Ok(resources)
}

fn parse_json_output<T: serde::de::DeserializeOwned>(
    values: &BTreeMap<String, String>,
    key: &str,
) -> anyhow::Result<T> {
    let value = values
        .get(key)
        .with_context(|| format!("{key} output missing"))?;
    serde_json::from_str(value).with_context(|| format!("Failed to parse {key}"))
}

async fn grant_terraform_shared_env_join_permission(
    config: &TestConfig,
    outputs: &Value,
) -> anyhow::Result<()> {
    let Some(shared_env) = &config.azure_resources.shared_container_env else {
        return Ok(());
    };

    use alien_azure_clients::authorization::{AuthorizationApi, Scope};
    use alien_azure_clients::models::authorization_role_assignments::{
        RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
    };
    use alien_azure_clients::AzureAuthorizationClient;

    let join_role_id = shared_env
        .join_role_definition_id
        .as_ref()
        .context("AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID not set - run terraform apply")?;
    let target = config
        .azure_target
        .as_ref()
        .context("Azure target missing")?;
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(outputs, "deployment_resources")?)?;
    let management = terraform_import_data::<AzureRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;
    let resource_prefix = terraform_output_string(outputs, "deployment_resource_prefix")?;

    let azure_config = AzureClientConfig {
        subscription_id: target.subscription_id.clone(),
        tenant_id: target.tenant_id.clone(),
        region: Some(target.region.clone()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: target.client_id.clone(),
            client_secret: target.client_secret.clone(),
        },
        service_overrides: None,
    };
    let auth_client = AzureAuthorizationClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config.clone()),
    );
    let env_scope = Scope::Resource {
        resource_group_name: shared_env.resource_group.clone(),
        resource_provider: "Microsoft.App".to_string(),
        parent_resource_path: None,
        resource_type: "managedEnvironments".to_string(),
        resource_name: shared_env.environment_name.clone(),
    };
    let scope = env_scope.to_scope_string(&azure_config);

    let assignment_id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "alien:e2e:tf-env-join-assign:{resource_prefix}:{}",
            management.principal_id
        )
        .as_bytes(),
    )
    .to_string();
    let full_assignment_id = auth_client.build_role_assignment_id(&env_scope, assignment_id);

    auth_client
        .create_or_update_role_assignment_by_id(
            full_assignment_id,
            &RoleAssignment {
                id: None,
                name: None,
                type_: None,
                properties: Some(RoleAssignmentProperties {
                    principal_id: management.principal_id.clone(),
                    role_definition_id: join_role_id.clone(),
                    scope: Some(scope),
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some(
                        "E2E test: Terraform management UAMI shared env access".into(),
                    ),
                    created_by: None,
                    created_on: None,
                    updated_by: None,
                    updated_on: None,
                }),
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!("Failed to assign shared env join role: {error}"))?;

    info!(
        principal_id = %management.principal_id,
        shared_env = %shared_env.environment_name,
        "Terraform shared environment permissions granted"
    );

    Ok(())
}

fn terraform_import_request_from_outputs(
    output: &Value,
    token: &str,
) -> anyhow::Result<StackImportRequest> {
    let platform: Platform = terraform_output_string(output, "deployment_platform")?
        .parse()
        .map_err(|error| anyhow::anyhow!("Invalid deployment_platform output: {error}"))?;
    let base_platform = terraform_output_string(output, "deployment_base_platform")
        .ok()
        .and_then(|value| {
            if value.trim().is_empty() || value.trim() == "null" {
                None
            } else {
                Some(value)
            }
        })
        .map(|value| {
            value.parse().map_err(|error| {
                anyhow::anyhow!("Invalid deployment_base_platform output: {error}")
            })
        })
        .transpose()?;
    let resource_prefix = terraform_output_string(output, "deployment_resource_prefix")?;
    let region = terraform_output_string(output, "deployment_region")?;
    let management_config: Option<ManagementConfig> = serde_json::from_str(
        &terraform_output_string(output, "deployment_management_config")?,
    )?;
    let stack_settings: StackSettings = serde_json::from_str(&terraform_output_string(
        output,
        "deployment_stack_settings",
    )?)?;
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(output, "deployment_resources")?)?;
    let setup_target = terraform_output_string(output, "deployment_setup_target")?;
    let setup_fingerprint = terraform_output_string(output, "deployment_setup_fingerprint")?;
    let setup_fingerprint_version =
        terraform_output_u32(output, "deployment_setup_fingerprint_version")?;

    Ok(StackImportRequest {
        setup_import_format_version: 1,
        deployment_group_token: token.to_string(),
        deployment_name: format!("terraform-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        resource_prefix,
        source_kind: Some(ImportSourceKind::Terraform),
        setup_metadata: None,
        release_id: None,
        platform,
        base_platform,
        region,
        setup_target,
        setup_fingerprint,
        setup_fingerprint_version,
        stack_settings,
        management_config,
        input_values: Default::default(),
        resources,
    })
}

async fn terraform_output_json(workdir: &Path, env: &[(String, String)]) -> anyhow::Result<Value> {
    let mut cmd = Command::new("terraform");
    cmd.current_dir(workdir).args(["output", "-json"]);
    apply_env(&mut cmd, env);
    let output = command_output(cmd, "terraform output -json").await?;
    serde_json::from_slice(&output.stdout).context("Failed to parse terraform output JSON")
}

/// Terraform can finish before GCP IAM bindings are visible.
/// Probe the same two-hop chain the manager uses before deployment starts:
/// management credentials -> configured manager SA -> imported stack management SA.
async fn wait_for_gcp_management_permissions(
    config: &TestConfig,
    outputs: &Value,
    has_remote_management: bool,
) -> anyhow::Result<()> {
    if !has_remote_management {
        info!(
            "Skipping GCP management permission probe because Terraform rendered no remote management resource"
        );
        return Ok(());
    }

    let management_config: Option<ManagementConfig> = serde_json::from_str(
        &terraform_output_string(outputs, "deployment_management_config")?,
    )?;
    let Some(management_config) = management_config else {
        info!(
            "Skipping GCP management permission probe because Terraform output has no management config"
        );
        return Ok(());
    };
    let management_service_account_email = match management_config {
        ManagementConfig::Gcp(config) => config.service_account_email,
        other => {
            anyhow::bail!("expected GCP management config, got {other:?}");
        }
    };
    let target = config.gcp_target.as_ref().context("GCP target missing")?;
    let management_source = config.gcp_mgmt.as_ref();
    if management_service_account_email.is_empty() {
        warn!(
            "Skipping GCP management permission probe because no management service account is configured"
        );
        return Ok(());
    }
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(outputs, "deployment_resources")?)?;
    let remote_management = terraform_import_data::<GcpRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;

    let Some(management_source) = management_source else {
        warn!("Skipping GCP management permission probe because GCP management config is missing");
        return Ok(());
    };
    let Some(credentials_json) = management_source.credentials_json.clone() else {
        warn!(
            "Skipping GCP management permission probe because GCP management credentials are missing"
        );
        return Ok(());
    };

    let base_config = GcpClientConfig {
        project_id: management_source.project_id.clone(),
        region: management_source.region.clone(),
        credentials: GcpCredentials::ServiceAccountKey {
            json: credentials_json,
        },
        service_overrides: None,
        project_number: None,
    };
    let http = reqwest::Client::new();

    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let management_config = match base_config
            .impersonate(GcpImpersonationConfig {
                service_account_email: management_service_account_email.clone(),
                target_project_id: Some(target.project_id.clone()),
                target_region: Some(target.region.clone()),
                ..GcpImpersonationConfig::default()
            })
            .await
        {
            Ok(config) => config,
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP management service account impersonation did not propagate for {management_service_account_email} within {timeout:?}: {error}"
                    );
                }
                warn!(
                    service_account_email = %management_service_account_email,
                    attempt,
                    %error,
                    "GCP management service account impersonation is not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(error) => {
                anyhow::bail!("GCP management service account impersonation probe failed: {error}");
            }
        };
        let impersonated_config = match management_config
            .impersonate(GcpImpersonationConfig {
                service_account_email: remote_management.service_account_email.clone(),
                target_project_id: Some(target.project_id.clone()),
                target_region: Some(target.region.clone()),
                ..GcpImpersonationConfig::default()
            })
            .await
        {
            Ok(config) => config,
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP remote stack management service account impersonation did not propagate for {} within {timeout:?}: {error}",
                        remote_management.service_account_email
                    );
                }
                warn!(
                    service_account_email = %remote_management.service_account_email,
                    management_service_account_email = %management_service_account_email,
                    attempt,
                    %error,
                    "GCP remote stack management service account impersonation is not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(error) => {
                anyhow::bail!(
                    "GCP remote stack management service account impersonation probe failed: {error}"
                );
            }
        };

        let resource_manager = alien_gcp_clients::ResourceManagerClient::new(
            http.clone(),
            impersonated_config.clone(),
        );
        let result = resource_manager
            .get_project_metadata(target.project_id.clone())
            .await;

        match result {
            Ok(_) => {
                info!(
                    service_account_email = %remote_management.service_account_email,
                    attempts = attempt,
                    "GCP management IAM permissions are ready"
                );
                return Ok(());
            }
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP management IAM permissions did not propagate for {} within {timeout:?}: {error}",
                        remote_management.service_account_email
                    );
                }
                warn!(
                    service_account_email = %remote_management.service_account_email,
                    attempt,
                    %error,
                    "GCP management IAM permissions are not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Err(error) => {
                anyhow::bail!("GCP management IAM permission probe failed: {error}");
            }
        }
    }
}

fn gcp_management_permission_probe_should_retry(error: &alien_gcp_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "REMOTE_ACCESS_DENIED" | "RATE_LIMIT_EXCEEDED" | "REMOTE_SERVICE_UNAVAILABLE" | "TIMEOUT"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum AzureManagementPermissionProbe {
    ServiceBus(AzureServiceBusNamespaceImportData),
    ResourceGraph,
}

fn azure_management_permission_probe(
    resources: &[ImportedResource],
) -> anyhow::Result<AzureManagementPermissionProbe> {
    if let Some(service_bus) = optional_terraform_import_data::<AzureServiceBusNamespaceImportData>(
        resources,
        "azure_service_bus_namespace",
    )? {
        return Ok(AzureManagementPermissionProbe::ServiceBus(service_bus));
    }

    Ok(AzureManagementPermissionProbe::ResourceGraph)
}

/// Terraform can finish before Azure federated credentials and role
/// assignments are visible to ARM. When the stack has Service Bus management
/// permissions, exercise them directly. Otherwise query Resource Graph using
/// the baseline observe permission that every management profile receives.
async fn wait_for_azure_management_permissions(
    config: &TestConfig,
    outputs: &Value,
) -> anyhow::Result<()> {
    let target = config
        .azure_target
        .as_ref()
        .context("Azure target missing")?;
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(outputs, "deployment_resources")?)?;

    let management = terraform_import_data::<AzureRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;
    let probe = azure_management_permission_probe(&resources)?;

    let token_file = std::env::var("AZURE_FEDERATED_TOKEN_FILE")
        .ok()
        .filter(|value| !value.is_empty())
        .context("AZURE_FEDERATED_TOKEN_FILE is required for Azure management permission probe")?;

    let azure_config = AzureClientConfig {
        subscription_id: management.subscription_id.clone(),
        tenant_id: management.tenant_id.clone(),
        region: Some(target.region.clone()),
        credentials: AzureCredentials::WorkloadIdentity {
            client_id: management.client_id.clone(),
            tenant_id: management.tenant_id.clone(),
            federated_token_file: token_file,
            authority_host: std::env::var("AZURE_AUTHORITY_HOST")
                .unwrap_or_else(|_| "https://login.microsoftonline.com/".to_string()),
        },
        service_overrides: None,
    };
    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    match probe {
        AzureManagementPermissionProbe::ServiceBus(service_bus) => {
            let service_bus_client = AzureServiceBusManagementClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(azure_config),
            );
            let probe_queue_name = format!(
                "{}-iam-probe",
                terraform_output_string(outputs, "deployment_resource_prefix")?
            );

            loop {
                attempt += 1;
                let create_result = service_bus_client
                    .create_or_update_queue(
                        service_bus.resource_group.clone(),
                        service_bus.namespace_name.clone(),
                        probe_queue_name.clone(),
                        alien_azure_clients::models::queue::SbQueueProperties::default(),
                    )
                    .await;

                match create_result {
                    Ok(_) => {
                        match service_bus_client
                            .delete_queue(
                                service_bus.resource_group.clone(),
                                service_bus.namespace_name.clone(),
                                probe_queue_name.clone(),
                            )
                            .await
                        {
                            Ok(()) => {
                                info!(
                                    client_id = %management.client_id,
                                    attempts = attempt,
                                    "Azure management Service Bus permissions are ready"
                                );
                                return Ok(());
                            }
                            Err(error)
                                if azure_management_permission_probe_should_retry(&error) =>
                            {
                                if started.elapsed() >= timeout {
                                    anyhow::bail!(
                                        "Azure management IAM delete permissions did not propagate for {} within {timeout:?}: {error}",
                                        management.client_id
                                    );
                                }
                                warn!(
                                    client_id = %management.client_id,
                                    attempt,
                                    %error,
                                    "Azure management IAM delete permissions are not ready yet"
                                );
                            }
                            Err(error) => {
                                anyhow::bail!("Azure management IAM delete probe failed: {error}");
                            }
                        }
                    }
                    Err(error) if azure_management_permission_probe_should_retry(&error) => {
                        if started.elapsed() >= timeout {
                            anyhow::bail!(
                                "Azure management IAM permissions did not propagate for {} within {timeout:?}: {error}",
                                management.client_id
                            );
                        }
                        warn!(
                            client_id = %management.client_id,
                            attempt,
                            %error,
                            "Azure management IAM permissions are not ready yet"
                        );
                    }
                    Err(error) => {
                        anyhow::bail!("Azure management IAM permission probe failed: {error}");
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
        AzureManagementPermissionProbe::ResourceGraph => {
            let resource_graph_client = AzureResourceGraphClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(azure_config),
            );
            let request = ResourceGraphQueryRequest::for_subscription(
                management.subscription_id.clone(),
                "Resources | take 1 | project id",
            );

            loop {
                attempt += 1;
                match resource_graph_client.resources(request.clone()).await {
                    Ok(_) => {
                        info!(
                            client_id = %management.client_id,
                            attempts = attempt,
                            "Azure management Resource Graph permissions are ready"
                        );
                        return Ok(());
                    }
                    Err(error) if azure_management_permission_probe_should_retry(&error) => {
                        if started.elapsed() >= timeout {
                            anyhow::bail!(
                                "Azure management Resource Graph permissions did not propagate for {} within {timeout:?}: {error}",
                                management.client_id
                            );
                        }
                        warn!(
                            client_id = %management.client_id,
                            attempt,
                            %error,
                            "Azure management Resource Graph permissions are not ready yet"
                        );
                    }
                    Err(error) => {
                        anyhow::bail!(
                            "Azure management Resource Graph permission probe failed: {error}"
                        );
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

fn optional_terraform_import_data<T>(
    resources: &[ImportedResource],
    resource_type: &str,
) -> anyhow::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    resources
        .iter()
        .find(|resource| resource.resource_type.as_ref() == resource_type)
        .map(|resource| {
            serde_json::from_value(resource.import_data.clone())
                .with_context(|| format!("Failed to parse {resource_type} import data"))
        })
        .transpose()
}

fn terraform_import_data<T>(
    resources: &[ImportedResource],
    resource_type: &str,
) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let resource = resources
        .iter()
        .find(|resource| resource.resource_type.as_ref() == resource_type)
        .with_context(|| format!("Terraform output missing {resource_type} import resource"))?;
    serde_json::from_value(resource.import_data.clone())
        .with_context(|| format!("Failed to parse {resource_type} import data"))
}

fn azure_management_permission_probe_should_retry(error: &alien_azure_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "AUTHENTICATION_ERROR"
            | "AUTHENTICATION_FAILED"
            | "HTTP_RESPONSE_ERROR"
            | "REMOTE_ACCESS_DENIED"
            | "REMOTE_SERVICE_UNAVAILABLE"
            | "TIMEOUT"
            | "RATE_LIMIT_EXCEEDED"
    )
}

fn terraform_output_string(outputs: &Value, key: &str) -> anyhow::Result<String> {
    outputs
        .get(key)
        .and_then(|output| output.get("value"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .with_context(|| format!("terraform output {key} missing or not a string"))
}

fn terraform_output_u32(outputs: &Value, key: &str) -> anyhow::Result<u32> {
    let value = outputs
        .get(key)
        .and_then(|output| output.get("value"))
        .with_context(|| format!("terraform output {key} missing"))?;

    if let Some(number) = value.as_u64() {
        return u32::try_from(number)
            .with_context(|| format!("terraform output {key} is too large for u32"));
    }

    if let Some(string) = value.as_str() {
        return string
            .parse()
            .with_context(|| format!("terraform output {key} is not a valid u32"));
    }

    anyhow::bail!("terraform output {key} is not a number or string")
}

/// The gate answers the enabled-demo e2e applies: the four `*On` inputs true,
/// the four `*Off` inputs false. Tuple keys are the Terraform variable names the
/// generator emits for each input id (`input_` + snake_case), so a change to
/// `stack_input_variable_name` must be mirrored here. Empty for every other app.
fn enabled_demo_gate_answers(app: TestApp) -> &'static [(&'static str, bool)] {
    match app {
        TestApp::EnabledDemo => &[
            ("input_kv_on", true),
            ("input_kv_off", false),
            ("input_storage_on", true),
            ("input_storage_off", false),
            ("input_queue_on", true),
            ("input_queue_off", false),
            ("input_vault_on", true),
            ("input_vault_off", false),
        ],
        _ => &[],
    }
}

fn terraform_tfvars(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
    kubernetes_namespace: Option<&str>,
) -> anyhow::Result<Value> {
    let mut vars = serde_json::Map::new();
    vars.insert(
        "name".to_string(),
        Value::String(format!("e2e-{}", &uuid::Uuid::new_v4().to_string()[..8])),
    );
    vars.insert(
        "resource_prefix".to_string(),
        Value::String(crate::config::e2e_resource_prefix()?),
    );
    vars.insert(
        "token".to_string(),
        Value::String(prepared.dg_token.clone()),
    );
    vars.insert(
        "manager_url".to_string(),
        Value::String(prepared.manager.url.clone()),
    );

    // Answer deployer gate inputs at apply time. Terraform auto-loads
    // terraform.tfvars.json, so a `input_<snake(id)>` key here is how the
    // harness threads a `.enabled(input)` answer into the applied artifact.
    for (tfvar, answer) in enabled_demo_gate_answers(prepared.app) {
        vars.insert(tfvar.to_string(), Value::Bool(*answer));
    }

    match target.cloud_platform() {
        Platform::Aws => {
            let target = prepared
                .config
                .aws_target
                .as_ref()
                .context("AWS target missing")?;
            vars.insert(
                "aws_region".to_string(),
                Value::String(target.region.clone()),
            );
            if let Some(ManagementConfig::Aws(config)) = prepared.manager.management_config() {
                vars.insert(
                    "managing_role_arn".to_string(),
                    Value::String(config.managing_role_arn),
                );
            }
            if let Some(account_id) = prepared
                .config
                .aws_mgmt
                .as_ref()
                .and_then(|config| config.account_id.clone())
            {
                vars.insert("managing_account_id".to_string(), Value::String(account_id));
            }
        }
        Platform::Gcp => {
            let target = prepared
                .config
                .gcp_target
                .as_ref()
                .context("GCP target missing")?;
            vars.insert(
                "gcp_project".to_string(),
                Value::String(target.project_id.clone()),
            );
            vars.insert(
                "gcp_region".to_string(),
                Value::String(target.region.clone()),
            );
            if let Some(email) = prepared
                .config
                .gcp_mgmt
                .as_ref()
                .and_then(|config| config.management_identity_email.clone())
            {
                vars.insert(
                    "managing_service_account_email".to_string(),
                    Value::String(email),
                );
            }
        }
        Platform::Azure => {
            let azure_target = prepared
                .config
                .azure_target
                .as_ref()
                .context("Azure target missing")?;
            insert_azure_tfvars(
                &mut vars,
                azure_target,
                prepared.config.azure_mgmt.as_ref(),
                target,
            );
        }
        _ => {}
    }

    if target.is_kubernetes() {
        let namespace =
            kubernetes_namespace.context("Kubernetes Terraform target missing namespace")?;
        vars.insert(
            "kubernetes_cluster_mode".to_string(),
            Value::String(
                match prepared.config.kubernetes_cluster_mode {
                    KubernetesClusterMode::Existing => "existing",
                    KubernetesClusterMode::Create => "create",
                }
                .to_string(),
            ),
        );
        vars.insert(
            "kubernetes_namespace".to_string(),
            Value::String(namespace.to_string()),
        );
        match target {
            alien_terraform::TerraformTarget::Eks
                if prepared.config.kubernetes_cluster_mode == KubernetesClusterMode::Existing =>
            {
                let eks = prepared
                    .config
                    .kubernetes
                    .eks
                    .as_ref()
                    .context("ALIEN_TEST_EKS_CLUSTER_NAME and KUBECONFIG are required for EKS Helm distribution")?;
                vars.insert(
                    "eks_cluster_name".to_string(),
                    Value::String(eks.cluster_name.clone()),
                );
            }
            alien_terraform::TerraformTarget::Gke
                if prepared.config.kubernetes_cluster_mode == KubernetesClusterMode::Existing =>
            {
                let gke = prepared
                    .config
                    .kubernetes
                    .gke
                    .as_ref()
                    .context("ALIEN_TEST_GKE_CLUSTER_NAME, ALIEN_TEST_GKE_CLUSTER_LOCATION, and KUBECONFIG are required for GKE Helm distribution")?;
                vars.insert(
                    "gke_cluster_name".to_string(),
                    Value::String(gke.cluster_name.clone()),
                );
                vars.insert(
                    "gke_cluster_location".to_string(),
                    Value::String(gke.cluster_location.clone()),
                );
            }
            alien_terraform::TerraformTarget::Aks
                if prepared.config.kubernetes_cluster_mode == KubernetesClusterMode::Existing =>
            {
                let aks = prepared
                    .config
                    .kubernetes
                    .aks
                    .as_ref()
                    .context("ALIEN_TEST_AKS_CLUSTER_NAME, ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP, and KUBECONFIG are required for AKS Helm distribution")?;
                vars.insert(
                    "aks_cluster_name".to_string(),
                    Value::String(aks.cluster_name.clone()),
                );
                vars.insert(
                    "aks_cluster_resource_group_name".to_string(),
                    Value::String(aks.cluster_resource_group_name.clone()),
                );
            }
            _ => {}
        }
    }

    Ok(Value::Object(vars))
}

fn insert_azure_tfvars(
    vars: &mut serde_json::Map<String, Value>,
    azure_target: &AzureConfig,
    azure_mgmt: Option<&AzureConfig>,
    target: alien_terraform::TerraformTarget,
) {
    vars.insert(
        "azure_subscription_id".to_string(),
        Value::String(azure_target.subscription_id.clone()),
    );
    if target == alien_terraform::TerraformTarget::Aks {
        vars.insert(
            "azure_tenant_id".to_string(),
            Value::String(azure_target.tenant_id.clone()),
        );
    }
    vars.insert(
        "azure_location".to_string(),
        Value::String(azure_target.region.clone()),
    );
    vars.insert(
        "azure_resource_group_name".to_string(),
        Value::String(format!(
            "alien-e2e-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        )),
    );
    if let Some(mgmt) = azure_mgmt {
        vars.insert(
            "azure_managing_tenant_id".to_string(),
            Value::String(mgmt.tenant_id.clone()),
        );
        if let Some(issuer) = &mgmt.oidc_issuer {
            vars.insert(
                "azure_oidc_issuer".to_string(),
                Value::String(issuer.clone()),
            );
        }
        if let Some(subject) = &mgmt.oidc_subject {
            vars.insert(
                "azure_oidc_subject".to_string(),
                Value::String(subject.clone()),
            );
        }
    }
    if target == alien_terraform::TerraformTarget::Aks {
        vars.insert(
            "aks_oidc_issuer_url".to_string(),
            Value::String(String::new()),
        );
    }
}

fn aws_env(config: &AwsConfig) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "AWS_ACCESS_KEY_ID".to_string(),
            config.access_key_id.clone(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            config.secret_access_key.clone(),
        ),
        ("AWS_REGION".to_string(), config.region.clone()),
        ("AWS_DEFAULT_REGION".to_string(), config.region.clone()),
    ];
    if let Some(token) = &config.session_token {
        env.push(("AWS_SESSION_TOKEN".to_string(), token.clone()));
    }
    env
}

fn terraform_env(config: &TestConfig, platform: Platform) -> anyhow::Result<Vec<(String, String)>> {
    match platform {
        Platform::Aws => Ok(aws_env(
            config.aws_target.as_ref().context("AWS target missing")?,
        )),
        Platform::Gcp => gcp_env(config.gcp_target.as_ref().context("GCP target missing")?),
        Platform::Azure => Ok(azure_env(
            config
                .azure_target
                .as_ref()
                .context("Azure target missing")?,
        )),
        _ => Ok(Vec::new()),
    }
}

fn gcp_env(config: &GcpConfig) -> anyhow::Result<Vec<(String, String)>> {
    let mut env = vec![
        ("GOOGLE_PROJECT".to_string(), config.project_id.clone()),
        ("GOOGLE_REGION".to_string(), config.region.clone()),
    ];
    if let Some(path) = gke_auth_plugin_path_env() {
        env.push(("PATH".to_string(), path));
    }
    if let Some(credentials) = &config.credentials_json {
        let file = tempfile::NamedTempFile::new()
            .context("Failed to create temporary GCP credentials file")?;
        std::fs::write(file.path(), credentials)?;
        let (_file, path) = file.keep()?;
        env.push((
            "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
            path.display().to_string(),
        ));
    } else if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if !path.trim().is_empty() {
            env.push(("GOOGLE_APPLICATION_CREDENTIALS".to_string(), path));
        }
    }
    Ok(env)
}

fn gke_auth_plugin_path_env() -> Option<String> {
    if std::process::Command::new("gke-gcloud-auth-plugin")
        .arg("--version")
        .output()
        .is_ok()
    {
        return None;
    }

    let output = std::process::Command::new("gcloud")
        .args(["info", "--format=value(installation.sdk_root)"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sdk_root = String::from_utf8(output.stdout).ok()?;
    let sdk_root = sdk_root.trim();
    if sdk_root.is_empty() {
        return None;
    }
    let plugin_dir = Path::new(sdk_root).join("bin");
    if !plugin_dir.join("gke-gcloud-auth-plugin").exists() {
        return None;
    }
    let existing = std::env::var("PATH").unwrap_or_default();
    Some(format!("{}:{existing}", plugin_dir.display()))
}

fn azure_env(config: &AzureConfig) -> Vec<(String, String)> {
    vec![
        (
            "ARM_SUBSCRIPTION_ID".to_string(),
            config.subscription_id.clone(),
        ),
        ("ARM_TENANT_ID".to_string(), config.tenant_id.clone()),
        ("ARM_CLIENT_ID".to_string(), config.client_id.clone()),
        (
            "ARM_CLIENT_SECRET".to_string(),
            config.client_secret.clone(),
        ),
    ]
}

async fn run_terraform_cmd<const N: usize>(
    workdir: &Path,
    env: &[(String, String)],
    args: [&str; N],
) -> anyhow::Result<()> {
    let mut cmd = Command::new("terraform");
    cmd.current_dir(workdir).args(args);
    apply_env(&mut cmd, env);
    run_command(cmd, "terraform").await
}

async fn wait_for_cloudformation_stack_create(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
) -> anyhow::Result<()> {
    let mut wait = Command::new("aws");
    wait.args([
        "cloudformation",
        "wait",
        "stack-create-complete",
        "--stack-name",
        stack_name,
        "--region",
        region,
    ]);
    apply_env(&mut wait, env);

    match run_command(wait, "aws cloudformation wait stack-create-complete").await {
        Ok(()) => Ok(()),
        Err(error) => {
            let events = describe_cloudformation_stack_events(stack_name, region, env)
                .await
                .unwrap_or_else(|events_error| {
                    format!("failed to describe stack events after wait failure: {events_error}")
                });
            Err(error).with_context(|| {
                format!(
                    "CloudFormation stack {stack_name} did not reach CREATE_COMPLETE. Recent events:\n{events}"
                )
            })
        }
    }
}

async fn describe_cloudformation_stack_events(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
) -> anyhow::Result<String> {
    let mut cmd = Command::new("aws");
    cmd.args([
        "cloudformation",
        "describe-stack-events",
        "--stack-name",
        stack_name,
        "--region",
        region,
        "--max-items",
        "40",
        "--query",
        "StackEvents[].{Time:Timestamp,Status:ResourceStatus,Type:ResourceType,LogicalId:LogicalResourceId,Reason:ResourceStatusReason}",
        "--output",
        "table",
    ]);
    apply_env(&mut cmd, env);
    let output = command_output(cmd, "aws cloudformation describe-stack-events").await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_command(mut cmd: Command, label: &str) -> anyhow::Result<()> {
    let output = cmd
        .output()
        .await
        .with_context(|| format!("failed to start {label}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

async fn command_output(mut cmd: Command, label: &str) -> anyhow::Result<std::process::Output> {
    let output = cmd
        .output()
        .await
        .with_context(|| format!("failed to start {label}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn apply_env(cmd: &mut Command, env: &[(String, String)]) {
    for (key, value) in env {
        cmd.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionProfile};
    use alien_core::ResourceLifecycle;

    fn empty_test_config() -> TestConfig {
        TestConfig {
            aws_mgmt: None,
            aws_target: None,
            aws_resources: crate::config::AwsTestResources {
                s3_bucket: None,
                command_kv_table: None,
                lambda_image: None,
                lambda_execution_role_arn: None,
                ecr_push_role_arn: None,
                ecr_pull_role_arn: None,
                ecr_repository: None,
            },
            gcp_mgmt: None,
            gcp_target: None,
            gcp_resources: crate::config::GcpTestResources {
                gcs_bucket: None,
                cloudrun_image: None,
                gar_repository: None,
            },
            azure_mgmt: None,
            azure_target: None,
            azure_resources: crate::config::AzureTestResources {
                resource_group: None,
                storage_account: None,
                blob_container: None,
                container_app_image: None,
                managed_environment_name: None,
                registry_name: None,
                acr_repository: None,
                shared_container_env: None,
            },
            e2e_artifact_registry: crate::config::E2eArtifactRegistryConfig {
                aws_ar_push_role_arn: None,
                aws_ar_pull_role_arn: None,
                gcp_gar_repository: None,
                gcp_ar_pull_sa_email: None,
                gcp_ar_push_sa_email: None,
                azure_acr_repository: None,
            },
            kubernetes: crate::config::KubernetesTestConfig::default(),
            e2e_network_mode: crate::config::E2eNetworkMode::None,
            kubernetes_cluster_mode: KubernetesClusterMode::Existing,
        }
    }

    fn test_config_with_platform(platform: Platform) -> TestConfig {
        let mut config = empty_test_config();
        match platform {
            Platform::Aws => {
                let credentials = AwsConfig {
                    access_key_id: "test".to_string(),
                    secret_access_key: "test".to_string(),
                    session_token: None,
                    region: "us-east-1".to_string(),
                    account_id: Some("123456789012".to_string()),
                };
                config.aws_mgmt = Some(credentials.clone());
                config.aws_target = Some(credentials);
            }
            Platform::Gcp => {
                let credentials = GcpConfig {
                    project_id: "test-project".to_string(),
                    region: "us-central1".to_string(),
                    credentials_json: Some("{}".to_string()),
                    management_identity_email: None,
                    management_identity_unique_id: None,
                };
                config.gcp_mgmt = Some(credentials.clone());
                config.gcp_target = Some(credentials);
            }
            Platform::Azure => {
                let credentials = AzureConfig {
                    subscription_id: "test-subscription".to_string(),
                    tenant_id: "test-tenant".to_string(),
                    client_id: "test-client".to_string(),
                    client_secret: "test-secret".to_string(),
                    region: "eastus".to_string(),
                    principal_id: Some("test-principal".to_string()),
                    oidc_issuer: None,
                    oidc_subject: None,
                };
                config.azure_mgmt = Some(credentials.clone());
                config.azure_target = Some(credentials);
            }
            other => panic!("unsupported test platform: {other}"),
        }
        config
    }

    #[test]
    fn managed_kubernetes_distribution_availability_uses_base_cloud() {
        for (flow, base_platform) in [
            (DistributionFlow::CloudFormationEksHelmPull, Platform::Aws),
            (DistributionFlow::TerraformEksHelmPull, Platform::Aws),
            (DistributionFlow::TerraformGkeHelmPull, Platform::Gcp),
            (DistributionFlow::TerraformAksHelmPull, Platform::Azure),
        ] {
            assert!(is_distribution_flow_available(
                flow,
                &test_config_with_platform(base_platform),
                TestApp::RuntimeLessMixed,
            ));
            assert!(!is_distribution_flow_available(
                flow,
                &empty_test_config(),
                TestApp::RuntimeLessMixed,
            ));
        }
    }

    #[test]
    fn onprem_distribution_requires_a_cloud_registry_platform() {
        assert!(!is_distribution_flow_available(
            DistributionFlow::TerraformOnpremHelmPull,
            &empty_test_config(),
            TestApp::RuntimeLessMixed,
        ));
        assert!(is_distribution_flow_available(
            DistributionFlow::TerraformOnpremHelmPull,
            &test_config_with_platform(Platform::Gcp),
            TestApp::RuntimeLessMixed,
        ));
    }

    fn contains_resource_type(stack: &Stack, resource_type: &str) -> bool {
        stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type().as_ref() == resource_type)
    }

    fn imported_resource<T: serde::Serialize>(
        resource_type: &'static str,
        data: &T,
    ) -> ImportedResource {
        ImportedResource {
            id: resource_type.to_string(),
            resource_type: alien_core::ResourceType::from_static(resource_type),
            import_data: serde_json::to_value(data).expect("import data should serialize"),
        }
    }

    #[test]
    fn azure_management_probe_uses_service_bus_when_stack_emits_it() {
        let service_bus = AzureServiceBusNamespaceImportData {
            subscription_id: "subscription".to_string(),
            resource_group: "resource-group".to_string(),
            namespace_name: "namespace".to_string(),
            endpoint: "namespace.servicebus.windows.net".to_string(),
        };
        let resources = vec![imported_resource(
            "azure_service_bus_namespace",
            &service_bus,
        )];

        let probe = azure_management_permission_probe(&resources)
            .expect("probe resource should be selected");

        assert_eq!(
            probe,
            AzureManagementPermissionProbe::ServiceBus(service_bus)
        );
    }

    #[test]
    fn azure_management_probe_uses_resource_graph_when_stack_has_no_service_bus() {
        let resources = Vec::new();

        let probe = azure_management_permission_probe(&resources)
            .expect("probe resource should be selected");

        assert_eq!(probe, AzureManagementPermissionProbe::ResourceGraph);
    }

    #[test]
    fn azure_management_probe_rejects_malformed_service_bus_import_data() {
        let resources = vec![ImportedResource {
            id: "azure_service_bus_namespace".to_string(),
            resource_type: alien_core::ResourceType::from_static("azure_service_bus_namespace"),
            import_data: serde_json::json!({"resourceGroup": "resource-group"}),
        }];

        let error = azure_management_permission_probe(&resources)
            .expect_err("malformed Service Bus data must not silently fall back");

        assert!(
            error
                .to_string()
                .contains("Failed to parse azure_service_bus_namespace import data"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn distribution_wait_budget_accounts_for_slow_cloud_control_planes() {
        assert_eq!(
            deployment_running_timeout(Platform::Azure, TestApp::ComprehensiveRust),
            Duration::from_secs(1_800)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Kubernetes, TestApp::ComprehensiveRust),
            Duration::from_secs(1_800)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Kubernetes, TestApp::FullStackMicroservices),
            Duration::from_secs(3_600)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Aws, TestApp::ComprehensiveRust),
            Duration::from_secs(600)
        );
        assert_eq!(
            deployment_running_timeout(Platform::Gcp, TestApp::ComprehensiveRust),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn existing_kubernetes_cluster_mode_marks_cluster_as_existing() {
        let mut settings = StackSettings {
            kubernetes: Some(KubernetesSettings {
                cluster: Some(KubernetesClusterSettings {
                    ownership: KubernetesClusterOwnership::Managed,
                    namespace: Some("alien-worker-runtime".to_string()),
                    cloud: None,
                }),
                exposure: Some(KubernetesExposureSettings::Disabled),
            }),
            ..StackSettings::default()
        };

        set_kubernetes_cluster_ownership(&mut settings, KubernetesClusterOwnership::Existing);

        let kubernetes = settings.kubernetes.expect("kubernetes settings");
        let cluster = kubernetes.cluster.expect("cluster settings");
        assert_eq!(cluster.ownership, KubernetesClusterOwnership::Existing);
        assert_eq!(cluster.namespace.as_deref(), Some("alien-worker-runtime"));
        assert_eq!(
            kubernetes.exposure,
            Some(KubernetesExposureSettings::Disabled)
        );
    }

    #[tokio::test]
    async fn gke_existing_cluster_render_does_not_add_cloud_network() {
        let source_stack = Stack::new("gke-existing-cluster-source".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["worker/execute"]),
            )
            .add(
                Container::new("api".to_string())
                    .permissions("execution".to_string())
                    .code(ContainerCode::Image {
                        image: "manager.example.com/alien-e2e:tag".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".to_string(),
                        desired: "0.25".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "128Mi".to_string(),
                        desired: "128Mi".to_string(),
                    })
                    .port(8080)
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let mut stack_settings = stack_settings_for_flow(DeploymentModel::Pull);
        set_kubernetes_cluster_ownership(&mut stack_settings, KubernetesClusterOwnership::Existing);

        let rendered_stack = terraform_kubernetes_stack_for_target(
            source_stack,
            alien_terraform::TerraformTarget::Gke,
            stack_settings,
        )
        .await
        .expect("GKE existing-cluster Terraform render preflights should pass");

        assert!(
            contains_resource_type(&rendered_stack, "kubernetes-cluster"),
            "GKE render should still add the KubernetesCluster handoff resource"
        );
        assert!(
            !contains_resource_type(&rendered_stack, "network"),
            "GKE existing-cluster render must not add a setup-owned GCP VPC for Kubernetes workloads"
        );
    }

    #[tokio::test]
    async fn distribution_source_stack_remains_valid_after_setup_render_mutations() {
        let source_stack = Stack::new("distribution-source".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["worker/execute"]),
            )
            .add(
                Worker::new("alien-rs-worker".to_string())
                    .permissions("execution".to_string())
                    .code(WorkerCode::Image {
                        image: "manager.example.com/alien-e2e:tag".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

        assert!(
            !contains_resource_type(&source_stack, "remote-stack-management"),
            "release/source stack must not contain setup-authored resources"
        );

        let runner = alien_preflights::runner::PreflightRunner::new();
        runner
            .run_template_preflights(&source_stack, Platform::Aws)
            .await
            .expect("release/source stack should pass setup import preflights");

        let rendered_stack =
            apply_render_mutations(source_stack.clone(), Platform::Aws, &stack_settings)
                .await
                .expect("distribution render mutations should succeed");
        assert!(
            contains_resource_type(&rendered_stack, "remote-stack-management"),
            "rendered setup artifact stack should include remote management"
        );
        assert!(
            runner
                .run_template_preflights(&rendered_stack, Platform::Aws)
                .await
                .is_err(),
            "rendered setup stack is not a valid release/source stack"
        );
    }

    #[tokio::test]
    async fn gcp_management_probe_skips_direct_target_without_remote_management() {
        wait_for_gcp_management_permissions(
            &empty_test_config(),
            &serde_json::json!({
                "deployment_management_config": {
                    "value": "null"
                }
            }),
            false,
        )
        .await
        .expect("direct-target GCP setup should not require management config");
    }

    #[tokio::test]
    async fn gcp_management_probe_skips_null_management_output() {
        wait_for_gcp_management_permissions(
            &empty_test_config(),
            &serde_json::json!({
                "deployment_management_config": {
                    "value": "null"
                }
            }),
            true,
        )
        .await
        .expect("null Terraform management config should not require a GCP management probe");
    }

    #[tokio::test]
    async fn terraform_output_kubeconfig_is_materialized_for_helm() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let cleanup = DistributionArtifactCleanup::Terraform {
            workdir,
            env: Vec::new(),
        };
        let kubeconfig = r#""apiVersion": "v1"
"clusters": []
"contexts": []
"current-context": "test"
"kind": "Config"
"users": []
"#;
        let mut target = KubernetesHelmTarget {
            namespace: "alien-test".to_string(),
            runtime: KubernetesRuntimeConfig {
                kubeconfig: kubeconfig.to_string(),
                kube_context: Some("test".to_string()),
                namespace_prefix: "alien-test".to_string(),
            },
        };

        materialize_kubeconfig_for_helm(&mut target, &cleanup)
            .await
            .expect("kubeconfig should be written");

        assert_ne!(target.runtime.kubeconfig, kubeconfig);
        assert_eq!(
            std::fs::read_to_string(&target.runtime.kubeconfig).expect("kubeconfig file"),
            kubeconfig
        );
    }

    #[tokio::test]
    async fn existing_kubeconfig_path_is_left_unchanged() {
        let workdir = tempfile::tempdir().expect("tempdir");
        let kubeconfig_path = workdir.path().join("existing.kubeconfig");
        std::fs::write(&kubeconfig_path, "apiVersion: v1\n").expect("write kubeconfig");
        let cleanup = DistributionArtifactCleanup::Terraform {
            workdir,
            env: Vec::new(),
        };
        let mut target = KubernetesHelmTarget {
            namespace: "alien-test".to_string(),
            runtime: KubernetesRuntimeConfig {
                kubeconfig: kubeconfig_path.to_string_lossy().into_owned(),
                kube_context: None,
                namespace_prefix: "alien-test".to_string(),
            },
        };

        materialize_kubeconfig_for_helm(&mut target, &cleanup)
            .await
            .expect("existing path should be accepted");

        assert_eq!(target.runtime.kubeconfig, kubeconfig_path.to_string_lossy());
    }

    #[tokio::test]
    async fn eks_pull_distribution_values_include_manager_irsa() {
        let source_stack = Stack::new("distribution-eks-pull".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["worker/execute"]),
            )
            .add(
                Worker::new("alien-rs-worker".to_string())
                    .permissions("execution".to_string())
                    .code(WorkerCode::Image {
                        image: "manager.example.com/alien-e2e:tag".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Pull);

        let rendered_stack = terraform_kubernetes_stack_for_target(
            source_stack,
            alien_terraform::TerraformTarget::Eks,
            stack_settings.clone(),
        )
        .await
        .expect("Kubernetes Terraform render mutations should succeed");
        assert!(
            contains_resource_type(&rendered_stack, "remote-stack-management"),
            "pull-mode Kubernetes setup needs a manager cloud identity"
        );

        let registry = alien_terraform::TfRegistry::built_in();
        let module = alien_terraform::generate_terraform_module(
            &rendered_stack,
            alien_terraform::TerraformTarget::Eks,
            alien_terraform::TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings,
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Terraform generation should succeed");
        let rendered = module
            .iter()
            .map(|(_, contents)| contents)
            .collect::<String>();

        assert!(
            rendered.contains("helm_manager_service_account = {"),
            "Terraform locals should include the manager service account values handed to Helm"
        );
        assert!(
            rendered.contains("\"eks.amazonaws.com/role-arn\" = aws_iam_role.management.arn"),
            "Helm values must annotate the manager service account with its IRSA role"
        );
        assert!(
            rendered.contains("id   = \"management\""),
            "rendered Terraform should include the management identity resource"
        );
        assert!(
            rendered.contains("eks:DescribeCluster"),
            "management role must be able to read the EKS cluster for cloud metadata heartbeat"
        );
        assert!(
            rendered.contains("arn:aws:eks:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:cluster/${local.kubernetes_cluster_name}"),
            "EKS cluster read must follow the Terraform-selected cluster name, not only the resource prefix"
        );
        assert!(
            rendered.contains("resource \"aws_iam_openid_connect_provider\" \"eks\"")
                && rendered.contains("data \"tls_certificate\" \"eks_oidc\""),
            "Terraform setup must create the EKS OIDC provider before Helm handoff"
        );
        assert!(
            rendered.contains("aws_iam_openid_connect_provider.eks[0].arn"),
            "IRSA trust must depend on the Terraform-managed OIDC provider"
        );
        assert!(
            !rendered.contains("iam:CreateOpenIDConnectProvider")
                && !rendered.contains("iam:UpdateAssumeRolePolicy"),
            "pull-agent management policy must not include workload identity bootstrap permissions"
        );
    }

    #[tokio::test]
    async fn eks_pull_distribution_does_not_carry_cloud_compute_cluster() {
        let source_stack = Stack::new("distribution-eks-containers".to_string())
            .permission("execution", PermissionProfile::new())
            .add(
                Container::new("api".to_string())
                    .permissions("execution".to_string())
                    .code(ContainerCode::Image {
                        image: "manager.example.com/api:tag".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".to_string(),
                        desired: "0.25".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "128Mi".to_string(),
                        desired: "128Mi".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Pull);

        let rendered_stack = terraform_kubernetes_stack_for_target(
            source_stack,
            alien_terraform::TerraformTarget::Eks,
            stack_settings.clone(),
        )
        .await
        .expect("Kubernetes Terraform render mutations should succeed");

        assert!(
            contains_resource_type(&rendered_stack, "kubernetes-cluster"),
            "Kubernetes Terraform setup should include the cluster substrate"
        );
        assert!(
            contains_resource_type(&rendered_stack, "remote-stack-management"),
            "Kubernetes Terraform setup should include the cloud management identity"
        );
        assert!(
            !contains_resource_type(&rendered_stack, "compute-cluster"),
            "Kubernetes Terraform setup must not reuse the cloud VM compute substrate"
        );
        let secrets = rendered_stack
            .resources
            .get("secrets")
            .expect("Kubernetes setup should include the managed secrets vault");
        assert!(
            secrets
                .dependencies
                .iter()
                .all(|dependency| dependency.id() != "management-sa"),
            "cloud-backed Kubernetes setup uses remote-stack-management, not a stack-local management-sa"
        );

        let registry = alien_terraform::TfRegistry::built_in();
        alien_terraform::generate_terraform_module(
            &rendered_stack,
            alien_terraform::TerraformTarget::Eks,
            alien_terraform::TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings,
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Terraform generation should not require a compute-cluster emitter");
    }

    fn azure_config(
        subscription_id: &str,
        tenant_id: &str,
        region: &str,
        oidc_issuer: Option<&str>,
        oidc_subject: Option<&str>,
    ) -> AzureConfig {
        AzureConfig {
            subscription_id: subscription_id.to_string(),
            tenant_id: tenant_id.to_string(),
            client_id: "client-id".to_string(),
            client_secret: "client-secret".to_string(),
            region: region.to_string(),
            principal_id: None,
            oidc_issuer: oidc_issuer.map(ToString::to_string),
            oidc_subject: oidc_subject.map(ToString::to_string),
        }
    }

    #[tokio::test]
    async fn gcp_distribution_render_grants_live_worker_provision() {
        let stack = Stack::new("distribution-gcp".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["worker/execute"]),
            )
            .add(
                Worker::new("alien-rs-fn".to_string())
                    .permissions("execution".to_string())
                    .code(WorkerCode::Image {
                        image: "us-central1-docker.pkg.dev/project/repo/alien-rs-fn:tag"
                            .to_string(),
                    })
                    // Cloud Run gen2 rejects < 512 MiB; the `WorkerMemoryCheck`
                    // preflight enforces that at plan time. The default
                    // `memory_mb` (256) is below the floor, so we set it
                    // explicitly here to keep the fixture valid for GCP.
                    .memory_mb(512)
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

        let rendered_stack = apply_render_mutations(stack, Platform::Gcp, &stack_settings)
            .await
            .expect("distribution render mutations should succeed");
        let registry = alien_terraform::TfRegistry::built_in();
        let module = alien_terraform::generate_terraform_module(
            &rendered_stack,
            alien_terraform::TerraformTarget::Gcp,
            alien_terraform::TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings,
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Terraform generation should succeed");
        let rendered = module
            .iter()
            .map(|(_, contents)| contents)
            .collect::<String>();

        assert!(rendered.contains(
            "resource \"google_project_iam_custom_role\" \"gcp_role_manage_cloud_run_services\""
        ));
        assert!(rendered.contains("run.services.update"));
        assert!(rendered.contains("roles/iam.serviceAccountUser"));
        assert!(rendered.contains("roles/artifactregistry.reader"));
    }

    #[tokio::test]
    async fn gcp_distribution_render_scopes_vault_management_roles_per_vault() {
        let stack = Stack::new("distribution-gcp-vaults".to_string())
            .add(
                Vault::new("alien-vault".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

        let rendered_stack = apply_render_mutations(stack, Platform::Gcp, &stack_settings)
            .await
            .expect("distribution render mutations should succeed");
        let registry = alien_terraform::TfRegistry::built_in();
        let module = alien_terraform::generate_terraform_module(
            &rendered_stack,
            alien_terraform::TerraformTarget::Gcp,
            alien_terraform::TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings,
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Terraform generation should succeed");
        let rendered = module
            .iter()
            .map(|(_, contents)| contents)
            .collect::<String>();
        let iam_member_declarations = rendered
            .lines()
            .filter(|line| line.contains("resource \"google_project_iam_member\""))
            .collect::<Vec<_>>();
        let unique_iam_member_declarations = iam_member_declarations
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(
            unique_iam_member_declarations.len(),
            iam_member_declarations.len(),
            "GCP IAM member declarations should be unique: {iam_member_declarations:?}"
        );
        let viewer_bindings = rendered
            .matches("role    = \"roles/secretmanager.viewer\"")
            .count();
        assert_eq!(
            viewer_bindings, 4,
            "GCP vault heartbeat/management bindings should be emitted once per target scope"
        );
        assert_eq!(
            rendered
                .matches("title       = \"ResourceVaultSecretsHeartbeat\"")
                .count(),
            2,
            "resource-scoped vault heartbeat conditions should be emitted once per generated vault"
        );
    }

    #[test]
    fn azure_tfvars_include_oidc_federated_identity_inputs() {
        let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
        let azure_mgmt = azure_config(
            "mgmt-sub",
            "mgmt-tenant",
            "eastus2",
            Some("https://issuer.example.com"),
            Some("system:serviceaccount:alien:manager"),
        );
        let mut vars = serde_json::Map::new();

        insert_azure_tfvars(
            &mut vars,
            &azure_target,
            Some(&azure_mgmt),
            alien_terraform::TerraformTarget::Azure,
        );

        assert_eq!(
            vars.get("azure_subscription_id").and_then(Value::as_str),
            Some("target-sub")
        );
        assert!(vars.get("azure_tenant_id").is_none());
        assert_eq!(
            vars.get("azure_managing_tenant_id").and_then(Value::as_str),
            Some("mgmt-tenant")
        );
        assert_eq!(
            vars.get("azure_oidc_issuer").and_then(Value::as_str),
            Some("https://issuer.example.com")
        );
        assert_eq!(
            vars.get("azure_oidc_subject").and_then(Value::as_str),
            Some("system:serviceaccount:alien:manager")
        );
        assert!(vars.get("azure_management_principal_id").is_none());
    }

    #[test]
    fn azure_tfvars_include_target_tenant_for_aks() {
        let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
        let mut vars = serde_json::Map::new();

        insert_azure_tfvars(
            &mut vars,
            &azure_target,
            None,
            alien_terraform::TerraformTarget::Aks,
        );

        assert_eq!(
            vars.get("azure_tenant_id").and_then(Value::as_str),
            Some("target-tenant")
        );
    }

    #[test]
    fn azure_tfvars_omit_oidc_inputs_when_management_config_is_missing() {
        let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None);
        let mut vars = serde_json::Map::new();

        insert_azure_tfvars(
            &mut vars,
            &azure_target,
            None,
            alien_terraform::TerraformTarget::Azure,
        );

        assert!(vars.get("azure_management_principal_id").is_none());
        assert!(vars.get("azure_oidc_issuer").is_none());
        assert!(vars.get("azure_oidc_subject").is_none());
    }

    #[tokio::test]
    async fn azure_direct_terraform_render_omits_remote_management_and_oidc() {
        let source_stack = Stack::new("distribution-azure-direct".to_string())
            .add(
                Storage::new("files".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

        let rendered_stack = apply_render_mutations_with_management_config(
            source_stack,
            Platform::Azure,
            &stack_settings,
            None,
        )
        .await
        .expect("Azure direct-target render mutations should succeed");
        assert!(
            !contains_resource_type(&rendered_stack, "remote-stack-management"),
            "direct-target setup should not create remote stack management"
        );

        let registry = alien_terraform::TfRegistry::built_in();
        let module = alien_terraform::generate_terraform_module(
            &rendered_stack,
            alien_terraform::TerraformTarget::Azure,
            alien_terraform::TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings,
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Terraform generation should succeed");
        let rendered = module
            .iter()
            .map(|(_, contents)| contents)
            .collect::<String>();

        assert!(rendered.contains("deployment_management_config = null"));
        assert!(!rendered.contains("variable \"azure_oidc_issuer\""));
        assert!(!rendered.contains("variable \"azure_oidc_subject\""));
        assert!(!rendered
            .contains("resource \"azurerm_federated_identity_credential\" \"management_fic\""));
    }

    #[tokio::test]
    async fn azure_push_distribution_render_grants_setup_heartbeats_to_management() {
        let source_stack = Stack::new("distribution-azure-rsm".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["worker/execute"]),
            )
            .add(
                Worker::new("api".to_string())
                    .permissions("execution".to_string())
                    .code(WorkerCode::Image {
                        image: "manager.example.com/api:tag".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                Storage::new("files".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .add(
                Kv::new("state".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .add(
                Queue::new("commands".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_settings = stack_settings_for_flow(DeploymentModel::Push);

        let rendered_stack = apply_render_mutations(source_stack, Platform::Azure, &stack_settings)
            .await
            .expect("Azure push render mutations should succeed");

        let ManagementPermissions::Extend(management_profile) = rendered_stack.management() else {
            panic!("Azure push render should generate management permissions");
        };
        let global_permission_ids: Vec<_> = management_profile
            .0
            .get("*")
            .expect("management profile should include global permissions")
            .iter()
            .map(|permission| permission.id().to_string())
            .collect();

        for expected in [
            "azure-resource-group/heartbeat",
            "azure-storage-account/heartbeat",
            "azure-service-bus-namespace/heartbeat",
            "observe/observe",
            "service-account/heartbeat",
            "service-activation/heartbeat",
        ] {
            assert!(
                global_permission_ids.contains(&expected.to_string()),
                "Azure management profile should include {expected}"
            );
        }
        assert!(
            !global_permission_ids
                .iter()
                .any(|permission| permission.contains("azure_")
                    || permission.contains("service_activation")),
            "Azure management profile must use permission-set IDs, not Rust resource type names"
        );
    }

    #[test]
    fn runtime_values_include_valid_agent_encryption_key() {
        let values = runtime_values().expect("runtime values should build");
        let key = values
            .pointer("/encryption/key")
            .and_then(Value::as_str)
            .expect("runtime encryption key should be present");

        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn runtime_values_use_exact_operator_image() {
        temp_env::with_var(
            "ALIEN_TEST_OVERRIDE_OPERATOR_IMAGE",
            Some("ghcr.io/alienplatform/alien-operator:test-head"),
            || {
                let values = runtime_values()
                    .expect("runtime values should use the requested operator image");
                let yaml = to_helm_values_yaml(&serde_json::json!({
                    "runtime": values,
                }))
                .expect("runtime values should render as Helm values");
                let rendered: Value =
                    serde_yaml::from_str(&yaml).expect("rendered Helm values should parse");

                assert_eq!(
                    rendered.pointer("/runtime/image/repository"),
                    Some(&Value::from("ghcr.io/alienplatform/alien-operator"))
                );
                assert_eq!(
                    rendered.pointer("/runtime/image/tag"),
                    Some(&Value::from("test-head"))
                );
                assert_eq!(
                    rendered.pointer("/runtime/image/pullPolicy"),
                    Some(&Value::from("IfNotPresent"))
                );
            },
        );
    }

    #[test]
    fn runtime_values_preserve_existing_pod_labels() {
        let mut values = serde_json::json!({
            "runtime": {
                "podLabels": {
                    "azure.workload.identity/use": "true"
                }
            }
        });
        let values_object = values.as_object_mut().expect("values object");
        merge_runtime_values(
            values_object,
            serde_json::json!({
                "image": {
                    "repository": "ghcr.io/alienplatform/alien-operator",
                    "tag": "test",
                    "pullPolicy": "IfNotPresent"
                },
                "encryption": {
                    "key": "abcd"
                }
            }),
        )
        .expect("runtime values should merge");

        assert_eq!(
            values.pointer("/runtime/podLabels/azure.workload.identity~1use"),
            Some(&Value::from("true"))
        );
        assert_eq!(
            values.pointer("/runtime/image/tag"),
            Some(&Value::from("test"))
        );
    }

    #[test]
    fn manager_fetch_values_keep_chart_service_routes() {
        let mut values = serde_json::json!({
            "serviceAccounts": {},
            "stackSettings": {},
        });
        let values_object = values.as_object_mut().expect("values object");
        let chart_values = serde_json::json!({
            "services": {
                "alien-rs-worker": {
                    "type": "clusterIp",
                    "port": 80,
                    "targetPort": 8080,
                    "component": "worker",
                },
            },
        });

        merge_chart_service_values(values_object, &chart_values)
            .expect("chart services should merge");

        assert_eq!(
            values.pointer("/services/alien-rs-worker/targetPort"),
            Some(&Value::from(8080))
        );
        assert_eq!(
            values.pointer("/services/alien-rs-worker/component"),
            Some(&Value::from("worker"))
        );
    }

    #[test]
    fn manager_fetch_values_merge_empty_service_map_from_setup() {
        let mut values = serde_json::json!({
            "services": {},
        });
        let values_object = values.as_object_mut().expect("values object");
        let chart_values = serde_json::json!({
            "services": {
                "alien-rs-worker": {
                    "type": "clusterIp",
                    "port": 80,
                    "targetPort": 8080,
                    "component": "worker",
                },
            },
        });

        merge_chart_service_values(values_object, &chart_values)
            .expect("chart services should merge into an empty setup map");

        assert_eq!(
            values.pointer("/services/alien-rs-worker/port"),
            Some(&Value::from(80))
        );
    }

    #[test]
    fn manager_fetch_values_preserve_service_overrides() {
        let mut values = serde_json::json!({
            "services": {
                "alien-rs-worker": {
                    "type": "nodePort",
                    "port": 8081,
                },
            },
        });
        let values_object = values.as_object_mut().expect("values object");
        let chart_values = serde_json::json!({
            "services": {
                "alien-rs-worker": {
                    "type": "clusterIp",
                    "port": 80,
                    "targetPort": 8080,
                    "component": "worker",
                },
            },
        });

        merge_chart_service_values(values_object, &chart_values)
            .expect("chart defaults should merge under explicit overrides");

        assert_eq!(
            values.pointer("/services/alien-rs-worker/type"),
            Some(&Value::from("nodePort"))
        );
        assert_eq!(
            values.pointer("/services/alien-rs-worker/port"),
            Some(&Value::from(8081))
        );
        assert_eq!(
            values.pointer("/services/alien-rs-worker/targetPort"),
            Some(&Value::from(8080))
        );
    }
}
