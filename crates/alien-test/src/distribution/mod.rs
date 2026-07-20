//! Setup artifact E2E setup.
//!
//! This module is intentionally separate from the native `e2e::setup` path so
//! CloudFormation/Terraform/Helm tests cannot accidentally validate controller
//! provisioning instead of the setup import path.

use std::{sync::Arc, time::Duration};

use alien_core::{
    import::{ImportedResource, StackImportRequest, StackImportResponse},
    AwsManagementConfig, AzureManagementConfig, DeploymentConfig,
    DeploymentModel as StackDeploymentModel, EnvironmentVariablesSnapshot, ExternalBinding,
    ExternalBindings, GcpManagementConfig, KubernetesCertificateMode, KubernetesClusterOwnership,
    KubernetesExposureSettings, KubernetesIngressRouteProfile, KubernetesRouteProfile,
    KubernetesRouteProviderOptions, KubernetesSettings, ManagementConfig, Platform, Stack,
    StackSettings, StackState,
};
use anyhow::Context;
use serde_json::Value;
use tempfile::TempDir;
use tokio::{fs, process::Command};
use tracing::info;

use crate::{
    build_push::build_and_push_stack_for_registry,
    config::{KubernetesClusterMode, KubernetesRuntimeConfig, TestConfig},
    deployment::TestDeployment,
    e2e::{self, DeploymentModel, DistributionFlow, TestApp, TestContext},
    managed_secret::provision_managed_test_secret,
    manager::TestManager,
};

mod cleanup;
mod cloudformation;
mod env;
mod exec;
mod helm;
mod permissions;
mod terraform;
#[cfg(test)]
mod tests;

use cleanup::*;
use cloudformation::*;
use env::*;
use exec::*;
use helm::*;
use permissions::*;
use terraform::*;

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

    fn command_env(&self) -> &[(String, String)] {
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

async fn run_onprem_k8s(_prepared: &mut DistributionPrepared) -> anyhow::Result<TestContext> {
    anyhow::bail!(
        "On-prem Helm local-import distribution needs a complete external binding fixture for comprehensive-rust"
    )
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
