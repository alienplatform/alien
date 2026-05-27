//! Setup artifact E2E setup.
//!
//! This module is intentionally separate from the native `e2e::setup` path so
//! CloudFormation/Terraform/Helm tests cannot accidentally validate controller
//! provisioning instead of the setup import path.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use alien_azure_clients::{
    AzureServiceBusManagementClient, AzureTokenCache, ServiceBusManagementApi,
};
#[cfg(test)]
use alien_core::Vault;
use alien_core::{
    import::{
        AzureRemoteStackManagementImportData, AzureServiceBusNamespaceImportData,
        GcpRemoteStackManagementImportData, ImportSourceKind, ImportedResource, StackImportRequest,
        StackImportResponse,
    },
    AwsManagementConfig, AzureClientConfig, AzureCredentials, AzureManagementConfig,
    DeploymentConfig, DeploymentModel as StackDeploymentModel, EnvironmentVariablesSnapshot,
    ExternalBinding, ExternalBindings, GcpClientConfig, GcpCredentials, GcpImpersonationConfig,
    GcpManagementConfig, ManagementConfig, Platform, Stack, StackSettings, StackState, Worker,
    WorkerCode,
};
use alien_gcp_clients::{GcpClientConfigExt, ResourceManagerApi};
use anyhow::Context;
use serde_json::Value;
use tempfile::TempDir;
use tokio::{fs, process::Command};
use tracing::{info, warn};

use crate::{
    build_push::build_and_push_stack,
    config::{AwsConfig, AzureConfig, GcpConfig, KubernetesRuntimeConfig, TestConfig},
    deployment::TestDeployment,
    e2e::{self, DeploymentModel, DistributionFlow, Language, TestContext},
    helm_values::{runtime_image_pull_secrets, to_helm_values_yaml},
    manager::TestManager,
};

/// Artifact cleanup that sits outside the manager's normal destroy flow.
pub enum DistributionArtifactCleanup {
    CloudFormation {
        stack_name: String,
        region: String,
        env: Vec<(String, String)>,
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
    },
}

impl DistributionArtifactCleanup {
    pub async fn cleanup(self) {
        match self {
            DistributionArtifactCleanup::CloudFormation {
                stack_name,
                region,
                env,
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
                    tracing::warn!(%stack_name, %error, "CloudFormation cleanup delete-stack failed");
                    return;
                }

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
                            tracing::warn!(
                                %stack_name,
                                %error,
                                "CloudFormation cleanup wait failed"
                            );
                        }
                    }
                }
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
                            break;
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
                            tracing::warn!(
                                %error,
                                "terraform destroy failed during cleanup"
                            );
                        }
                    }
                }
            }
            DistributionArtifactCleanup::Helm {
                release,
                namespace,
                kubeconfig,
                kube_context,
            } => {
                if let Err(error) = crate::cleanup::cleanup_helm_release(
                    &release,
                    &namespace,
                    kubeconfig.as_deref(),
                    kube_context.as_deref(),
                )
                .await
                {
                    tracing::warn!(%release, %namespace, %error, "helm cleanup failed");
                }
                if let Err(error) = crate::cleanup::cleanup_kubernetes_namespace(
                    &namespace,
                    kubeconfig.as_deref(),
                    kube_context.as_deref(),
                )
                .await
                {
                    tracing::warn!(%namespace, %error, "kubernetes namespace cleanup failed");
                }
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
    language: Language,
    group_id: String,
    dg_token: String,
}

struct TerraformApplyResult {
    deployment: TestDeployment,
    cleanup: DistributionArtifactCleanup,
    outputs: Value,
}

#[derive(Debug, Clone)]
struct KubernetesHelmTarget {
    runtime: KubernetesRuntimeConfig,
    namespace: String,
}

pub async fn setup_distribution(
    flow: DistributionFlow,
    language: Language,
) -> anyhow::Result<TestContext> {
    let mut prepared = prepare_distribution(flow, language).await?;

    let result = match flow {
        DistributionFlow::CloudFormationAwsPush => run_cloudformation_aws(&mut prepared).await,
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
                ctx.cleanup().await;
                return Err(error);
            }
            Ok(ctx)
        }
        Err(error) => Err(error),
    }
}

async fn prepare_distribution(
    flow: DistributionFlow,
    language: Language,
) -> anyhow::Result<DistributionPrepared> {
    let platform = flow.platform();
    let model = flow.deployment_model();
    let test_name = format!("{}_{}", flow.name(), language);
    info!(%test_name, "Starting distribution E2E setup");

    let config = TestConfig::from_env();
    if !e2e::is_platform_available(&config, platform, model, language) {
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

    let build_platform = build_platform_for_flow(flow, &config)?;
    let e2e_root = e2e::e2e_test_apps_root()?;
    let app_path = e2e_root.join(e2e::test_app_path(language));
    let stack_json = e2e::load_stack_json(&app_path, "alien.ts", build_platform).await?;
    let stack_value = stack_json
        .get(build_platform.as_str())
        .context("Stack JSON missing build platform key")?;
    let stack: Stack =
        serde_json::from_value(stack_value.clone()).context("Failed to deserialize Stack JSON")?;

    let pushed_stack =
        build_and_push_stack(stack, build_platform, &config, &app_path, &manager).await?;

    if build_platform == Platform::Aws && config.aws_target.is_some() {
        let tags = e2e::extract_ecr_image_tags(&pushed_stack);
        if !tags.is_empty() {
            crate::build_push::wait_for_ecr_replication(&config, &tags).await?;
        }
    }

    // The manager release must keep the same source stack shape as a normal
    // `alien release`. Setup artifacts render from a derived stack after
    // template mutations add setup-owned resources such as remote management.
    let render_stack = if model == DeploymentModel::Push {
        rewrite_push_distribution_images(pushed_stack.clone(), build_platform, &config)?
    } else {
        pushed_stack.clone()
    };

    let stack_settings = e2e_stack_settings_for_flow(model, &config, build_platform)?;
    let rendered_stack = apply_render_mutations(render_stack, build_platform, &stack_settings)
        .await
        .context("Failed to apply distribution render preflights")?;

    create_release(&manager, build_platform, &pushed_stack).await?;
    let (group_id, dg_token) = create_deployment_group_token(&manager).await?;

    Ok(DistributionPrepared {
        manager,
        config,
        built_stack: pushed_stack,
        rendered_stack,
        platform,
        model,
        language,
        group_id,
        dg_token,
    })
}

fn missing_distribution_flow_config(
    flow: DistributionFlow,
    config: &TestConfig,
) -> Option<&'static str> {
    match flow {
        DistributionFlow::TerraformEksHelmPull => {
            if !config.has_platform(Platform::Aws) {
                Some("AWS management and target credentials are required")
            } else if config.kubernetes.eks.is_none() {
                Some("ALIEN_TEST_EKS_CLUSTER_NAME and KUBECONFIG are required")
            } else {
                None
            }
        }
        DistributionFlow::TerraformGkeHelmPull => {
            if !config.has_platform(Platform::Gcp) {
                Some("GCP management and target credentials are required")
            } else if config.kubernetes.gke.is_none() {
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
            } else if !has_azure_management_oidc(config) {
                Some(
                    "AZURE_MANAGEMENT_OIDC_ISSUER, AZURE_MANAGEMENT_OIDC_SUBJECT, and AZURE_FEDERATED_TOKEN_FILE are required",
                )
            } else if config.kubernetes.aks.is_none() {
                Some(
                    "ALIEN_TEST_AKS_CLUSTER_NAME, ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP, and KUBECONFIG are required",
                )
            } else {
                None
            }
        }
        DistributionFlow::TerraformAzurePush => {
            if !has_azure_management_oidc(config) {
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
    config
        .azure_mgmt
        .as_ref()
        .is_some_and(|mgmt| mgmt.oidc_issuer.is_some() && mgmt.oidc_subject.is_some())
        && std::env::var("AZURE_FEDERATED_TOKEN_FILE")
            .ok()
            .filter(|value| !value.is_empty())
            .is_some()
}

fn manager_platforms_for_flow(flow: DistributionFlow, config: &TestConfig) -> Vec<Platform> {
    match flow {
        DistributionFlow::TerraformEksHelmPull => vec![Platform::Aws],
        DistributionFlow::TerraformGkeHelmPull => vec![Platform::Gcp],
        DistributionFlow::TerraformAksHelmPull => vec![Platform::Azure],
        DistributionFlow::TerraformOnpremHelmPull => {
            [Platform::Aws, Platform::Gcp, Platform::Azure]
                .into_iter()
                .filter(|platform| config.has_platform(*platform))
                .collect()
        }
        _ => vec![flow.platform()],
    }
}

fn build_platform_for_flow(
    flow: DistributionFlow,
    config: &TestConfig,
) -> anyhow::Result<Platform> {
    match flow {
        DistributionFlow::CloudFormationAwsPush
        | DistributionFlow::TerraformAwsPush
        | DistributionFlow::TerraformEksHelmPull => Ok(Platform::Aws),
        DistributionFlow::TerraformGcpPush | DistributionFlow::TerraformGkeHelmPull => {
            Ok(Platform::Gcp)
        }
        DistributionFlow::TerraformAzurePush | DistributionFlow::TerraformAksHelmPull => {
            Ok(Platform::Azure)
        }
        DistributionFlow::TerraformOnpremHelmPull => {
            [Platform::Aws, Platform::Gcp, Platform::Azure]
                .into_iter()
                .find(|platform| config.has_platform(*platform))
                .context(
                    "on-prem Helm distribution needs at least one cloud artifact registry config",
                )
        }
    }
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

fn stack_settings_for_terraform(prepared: &DistributionPrepared) -> anyhow::Result<StackSettings> {
    let mut settings =
        e2e_stack_settings_for_flow(prepared.model, &prepared.config, prepared.platform)?;
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

fn render_management_config(
    platform: Platform,
    stack_settings: &StackSettings,
) -> Option<ManagementConfig> {
    if stack_settings.deployment_model != StackDeploymentModel::Push {
        return None;
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
        Platform::Kubernetes | Platform::Local | Platform::Test => None,
    }
}

async fn apply_render_mutations(
    stack: Stack,
    platform: Platform,
    stack_settings: &StackSettings,
) -> anyhow::Result<Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner.run_template_preflights(&stack, platform).await?;

    let stack_state = StackState::new(platform);
    let config = DeploymentConfig {
        deployment_name: Some(stack.id().to_string()),
        stack_settings: stack_settings.clone(),
        management_config: render_management_config(platform, stack_settings),
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        base_platform: None,
        public_urls: None,
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
        run_command(wait, "aws cloudformation wait stack-create-complete").await?;

        let request =
            cloudformation_import_request(&stack_name, &target.region, &env, &prepared.dg_token)
                .await?;
        import_stack(prepared, request).await
    }
    .await;

    match create_result {
        Ok(deployment) => Ok(context_from_deployment(prepared, deployment, vec![cleanup])),
        Err(error) => {
            cleanup.cleanup().await;
            Err(error)
        }
    }
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
    let helm_target = required_kubernetes_helm_target(prepared, target)?;
    let result = apply_terraform_and_import(prepared, target, Some(&helm_target.namespace)).await?;
    let chart_dir = match render_helm_chart(prepared).await {
        Ok(chart_dir) => chart_dir,
        Err(error) => {
            result.cleanup.cleanup().await;
            return Err(error);
        }
    };
    let values_file = match write_manager_fetch_values(
        prepared,
        &result.deployment,
        &result.outputs,
        &chart_dir,
        Some(&helm_target),
    )
    .await
    {
        Ok(values_file) => values_file,
        Err(error) => {
            result.cleanup.cleanup().await;
            return Err(error);
        }
    };
    let release = format!("alien-e2e-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let agent_result = crate::agent::TestAlienAgent::helm_install_with_values(
        chart_dir.path(),
        &values_file,
        &release,
        &helm_target.namespace,
        Some(&helm_target.runtime.kubeconfig),
        helm_target.runtime.kube_context.as_deref(),
    )
    .await
    .map_err(|error| error.to_string());
    let agent = match agent_result {
        Ok(agent) => agent,
        Err(error) => {
            result.cleanup.cleanup().await;
            return Err(anyhow::anyhow!(
                "Failed to install Helm distribution runtime: {error}"
            ));
        }
    };

    let mut ctx = context_from_deployment(
        prepared,
        result.deployment,
        vec![
            result.cleanup,
            DistributionArtifactCleanup::Helm {
                release,
                namespace: helm_target.namespace,
                kubeconfig: Some(helm_target.runtime.kubeconfig),
                kube_context: helm_target.runtime.kube_context,
            },
        ],
    );
    ctx.agent = Some(agent);
    Ok(ctx)
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
    let stack_settings = stack_settings_for_terraform(prepared)?;
    let module = alien_terraform::generate_terraform_module(
        &prepared.rendered_stack,
        target,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings,
            registration: None,
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

    let env = terraform_env(&prepared.config, target.platform())?;
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
        if target.platform() == Platform::Azure {
            grant_terraform_shared_env_join_permission(&prepared.config, &outputs).await?;
        }
        if target.platform() == Platform::Gcp {
            wait_for_gcp_management_permissions(&prepared.config, &outputs).await?;
        }
        if target.platform() == Platform::Azure {
            wait_for_azure_management_permissions(&prepared.config, &outputs).await?;
        }
        let request = terraform_import_request_from_outputs(&outputs, &prepared.dg_token)?;
        let deployment = import_stack(prepared, request).await?;
        Ok::<_, anyhow::Error>((deployment, outputs))
    }
    .await;

    let cleanup = DistributionArtifactCleanup::Terraform {
        workdir,
        env: env.clone(),
    };
    match apply_result {
        Ok((deployment, outputs)) => Ok(TerraformApplyResult {
            deployment,
            cleanup,
            outputs,
        }),
        Err(error) => {
            cleanup.cleanup().await;
            Err(error)
        }
    }
}

async fn render_helm_chart(prepared: &DistributionPrepared) -> anyhow::Result<TempDir> {
    let stack_settings =
        e2e_stack_settings_for_flow(prepared.model, &prepared.config, Platform::Kubernetes)?;
    let stack = apply_render_mutations(
        prepared.built_stack.clone(),
        Platform::Kubernetes,
        &stack_settings,
    )
    .await
    .context("Failed to apply Helm render preflights")?;
    let registry = alien_helm::HelmRegistry::built_in();
    let chart = alien_helm::generate_helm_chart(
        &stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings,
            chart_name: format!("alien-e2e-{}", prepared.language),
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

async fn write_manager_fetch_values(
    prepared: &DistributionPrepared,
    deployment: &TestDeployment,
    terraform_outputs: &Value,
    chart_dir: &TempDir,
    helm_target: Option<&KubernetesHelmTarget>,
) -> anyhow::Result<std::path::PathBuf> {
    let mut values = terraform_helm_values(terraform_outputs)?;
    let values_object = values
        .as_object_mut()
        .context("helm_values output must be a JSON object")?;
    values_object.insert(
        "management".to_string(),
        serde_json::json!({
            "token": deployment.token.clone(),
            "name": deployment.name.clone(),
            "url": prepared.manager.url.clone(),
            "deploymentId": deployment.id.clone(),
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        }),
    );
    values_object.insert("infrastructure".to_string(), Value::Null);
    values_object.insert("runtime".to_string(), runtime_values()?);
    if let Some(helm_target) = helm_target {
        if !values_object.contains_key("services") {
            let chart_values = fs::read_to_string(chart_dir.path().join("values.yaml"))
                .await
                .context("Failed to read generated chart values.yaml")?;
            let chart_values: Value =
                serde_yaml::from_str(&chart_values).context("Failed to parse chart values.yaml")?;
            if let Some(services) = chart_values.get("services") {
                values_object.insert("services".to_string(), services.clone());
            }
        }
        apply_kubernetes_service_values(values_object, helm_target);
    }

    let values_path = chart_dir.path().join("distribution-values.yaml");
    fs::write(&values_path, to_helm_values_yaml(&values)?)
        .await
        .context("Failed to write Helm distribution values")?;
    Ok(values_path)
}

fn terraform_helm_values(outputs: &Value) -> anyhow::Result<Value> {
    serde_json::from_str(&terraform_output_string(outputs, "helm_values")?)
        .context("Failed to parse terraform output helm_values")
}

fn runtime_values() -> anyhow::Result<Value> {
    let image = std::env::var("ALIEN_TEST_OVERRIDE_AGENT_IMAGE")
        .ok()
        .filter(|image| !image.is_empty())
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-agent:latest".to_string());
    let (repository, tag) = split_image_tag(&image)?;
    let mut runtime = serde_json::json!({
        "image": {
            "repository": repository,
            "tag": tag,
            "pullPolicy": "IfNotPresent",
        }
    });
    if let Some(image_pull_secrets) = runtime_image_pull_secrets(&repository) {
        runtime["imagePullSecrets"] = image_pull_secrets;
    }
    Ok(runtime)
}

fn split_image_tag(image: &str) -> anyhow::Result<(String, String)> {
    if image.contains('@') {
        anyhow::bail!(
            "ALIEN_TEST_OVERRIDE_AGENT_IMAGE must use a tag for Helm E2E installs; digest references are not supported yet"
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

fn apply_kubernetes_service_values(
    values_object: &mut serde_json::Map<String, Value>,
    helm_target: &KubernetesHelmTarget,
) {
    let Some(services) = values_object
        .get_mut("services")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for (resource_id, service) in services {
        let Some(service) = service.as_object_mut() else {
            continue;
        };
        if let Some(class_name) = &helm_target.runtime.ingress_class {
            service
                .entry("ingress".to_string())
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .map(|ingress| {
                    ingress.insert("className".to_string(), Value::String(class_name.clone()));
                });
        }
        if let Some(tls_secret_name) = &helm_target.runtime.tls_secret_name {
            service.insert(
                "tls".to_string(),
                serde_json::json!({
                    "enabled": true,
                    "secretName": tls_secret_name,
                }),
            );
        }
        if let Some(host_suffix) = &helm_target.runtime.public_host_suffix {
            let host = format!(
                "{}-{}.{}",
                sanitize_kubernetes_dns_label(resource_id),
                helm_target.namespace,
                host_suffix.trim_start_matches('.')
            );
            let scheme = if helm_target.runtime.tls_secret_name.is_some() {
                "https"
            } else {
                "http"
            };
            service.insert("host".to_string(), Value::String(host.clone()));
            service.insert(
                "publicUrl".to_string(),
                Value::String(format!("{scheme}://{host}")),
            );
        }
    }
}

async fn import_stack(
    prepared: &DistributionPrepared,
    request: StackImportRequest,
) -> anyhow::Result<TestDeployment> {
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

    Ok(TestDeployment::new(
        response.deployment_id,
        dep.name,
        prepared.platform.as_str().to_string(),
        None,
        token,
        prepared.manager.clone(),
    ))
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
        language: prepared.language,
        agent: None,
        distribution_cleanups: cleanups,
    }
}

async fn wait_and_finalize(ctx: &mut TestContext) -> anyhow::Result<()> {
    ctx.deployment
        .wait_until_running(Duration::from_secs(600))
        .await
        .map_err(|error| anyhow::anyhow!("Deployment failed to reach running: {error}"))?;
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

async fn provision_managed_test_secret(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
) -> anyhow::Result<()> {
    let vault_name = "secrets";
    let secret_key = "MANAGED_TEST_SECRET";
    let url = format!(
        "{}/v1/deployments/{}/vault/{}/secrets/{}",
        manager.url, deployment.id, vault_name, secret_key
    );
    let response = manager
        .http_client()
        .put(&url)
        .json(&serde_json::json!({ "value": "e2e-test-managed-secret-value" }))
        .send()
        .await
        .context("Failed to call vault set secret API")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to provision managed test secret ({status}): {body}");
    }
    Ok(())
}

async fn cloudformation_import_request(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
    token: &str,
) -> anyhow::Result<StackImportRequest> {
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

    let platform: Platform = values
        .get("DeploymentPlatform")
        .context("DeploymentPlatform output missing")?
        .parse()
        .map_err(|error| anyhow::anyhow!("Invalid DeploymentPlatform output: {error}"))?;
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
        setup_import_format_version: 1,
        deployment_group_token: token.to_string(),
        deployment_name: stack_name.to_string(),
        resource_prefix,
        source_kind: Some(ImportSourceKind::CloudFormation),
        release_id: None,
        platform,
        base_platform: None,
        region,
        setup_target,
        setup_fingerprint,
        setup_fingerprint_version,
        stack_settings,
        management_config,
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
    let management_config: ManagementConfig = serde_json::from_str(&terraform_output_string(
        output,
        "deployment_management_config",
    )?)?;
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
        release_id: None,
        platform,
        base_platform,
        region,
        setup_target,
        setup_fingerprint,
        setup_fingerprint_version,
        stack_settings,
        management_config,
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
) -> anyhow::Result<()> {
    let target = config.gcp_target.as_ref().context("GCP target missing")?;
    let management_source = config.gcp_mgmt.as_ref();
    let management_config: ManagementConfig = serde_json::from_str(&terraform_output_string(
        outputs,
        "deployment_management_config",
    )?)?;
    let management_service_account_email = match management_config {
        ManagementConfig::Gcp(config) => config.service_account_email,
        other => {
            anyhow::bail!("expected GCP management config, got {other:?}");
        }
    };
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

/// Terraform can finish before Azure federated credentials and role
/// assignments are visible to ARM. Probe the imported management identity
/// against the first live worker dependency so deployment starts only after
/// the same identity can perform the Service Bus control-plane operation.
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
    let service_bus = terraform_import_data::<AzureServiceBusNamespaceImportData>(
        &resources,
        "azure_service_bus_namespace",
    )?;

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
    let service_bus_client = AzureServiceBusManagementClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config),
    );
    let probe_queue_name = format!(
        "{}-iam-probe",
        terraform_output_string(outputs, "deployment_resource_prefix")?
    );

    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
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
                            "Azure management IAM permissions are ready"
                        );
                        return Ok(());
                    }
                    Err(error) if azure_management_permission_probe_should_retry(&error) => {
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

    match target.platform() {
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
            "kubernetes_namespace".to_string(),
            Value::String(namespace.to_string()),
        );
        match target {
            alien_terraform::TerraformTarget::Eks => {
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
            alien_terraform::TerraformTarget::Gke => {
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
            alien_terraform::TerraformTarget::Aks => {
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
    if let Some(credentials) = &config.credentials_json {
        let file = tempfile::NamedTempFile::new()
            .context("Failed to create temporary GCP credentials file")?;
        std::fs::write(file.path(), credentials)?;
        let (_file, path) = file.keep()?;
        env.push((
            "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
            path.display().to_string(),
        ));
    }
    Ok(env)
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
    use alien_core::permissions::PermissionProfile;
    use alien_core::ResourceLifecycle;

    fn contains_resource_type(stack: &Stack, resource_type: &str) -> bool {
        stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type().as_ref() == resource_type)
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
            },
        )
        .expect("Terraform generation should succeed");
        let rendered = module
            .iter()
            .map(|(_, contents)| contents)
            .collect::<String>();

        assert!(rendered.contains("roles/run.admin"));
        assert!(rendered.contains("roles/iam.serviceAccountUser"));
        assert!(rendered.contains("roles/artifactregistry.reader"));
        assert!(rendered.contains("roles/cloudbuild.builds.editor"));
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
        assert_eq!(
            rendered
                .matches("role    = \"roles/secretmanager.viewer\"")
                .count(),
            1,
            "global management vault heartbeat binding should be emitted once"
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
}
