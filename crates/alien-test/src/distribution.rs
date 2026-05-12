//! Distribution artifact E2E setup.
//!
//! This module is intentionally separate from the native `e2e::setup` path so
//! CloudFormation/Terraform/Helm tests cannot accidentally validate controller
//! provisioning instead of the distribution import path.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use alien_azure_clients::{
    AzureServiceBusManagementClient, AzureTokenCache, ServiceBusManagementApi,
};
use alien_core::{
    import::{
        AzureRemoteStackManagementImportData, AzureServiceBusNamespaceImportData, ImportSourceKind,
        ImportedResource, StackImportRequest, StackImportResponse,
    },
    AwsManagementConfig, AzureClientConfig, AzureCredentials, AzureManagementConfig,
    DeploymentConfig, DeploymentModel as StackDeploymentModel, EnvironmentVariablesSnapshot,
    ExternalBindings, Function, FunctionCode, GcpClientConfig, GcpCredentials,
    GcpImpersonationConfig, GcpManagementConfig, ManagementConfig, Platform, Stack, StackSettings,
    StackState,
};
use alien_gcp_clients::{CloudRunApi, CloudRunClient, GcpClientConfigExt, ResourceManagerApi};
use anyhow::Context;
use serde_json::Value;
use tempfile::TempDir;
use tokio::{fs, process::Command};
use tracing::{info, warn};

use crate::{
    build_push::build_and_push_stack,
    config::{AwsConfig, AzureConfig, GcpConfig, TestConfig},
    deployment::TestDeployment,
    e2e::{self, DeploymentModel, DistributionFlow, Language, TestContext},
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
                    "destroying Terraform distribution artifacts"
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
                            info!("Terraform distribution artifacts destroyed");
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
            } => {
                if let Err(error) = crate::cleanup::cleanup_helm_release(
                    &release,
                    &namespace,
                    kubeconfig.as_deref(),
                )
                .await
                {
                    tracing::warn!(%release, %namespace, %error, "helm cleanup failed");
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

    let render_stack = if model == DeploymentModel::Push {
        rewrite_push_distribution_images(pushed_stack.clone(), build_platform, &config)?
    } else {
        pushed_stack.clone()
    };

    let stack_settings = stack_settings_for_flow(model);
    let rendered_stack = apply_render_mutations(render_stack, build_platform, &stack_settings)
        .await
        .context("Failed to apply distribution render preflights")?;

    create_release(&manager, build_platform, &rendered_stack).await?;
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
            oidc_issuer: None,
            oidc_subject: None,
            management_principal_id: None,
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
    let stack_name = format!("alien-e2e-cfn-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let workdir = tempfile::tempdir().context("Failed to create CFN workdir")?;

    let registry = alien_cloudformation::CfRegistry::built_in();
    let template = alien_cloudformation::generate_cloudformation_template(
        &prepared.rendered_stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            stack_settings: stack_settings_for_flow(prepared.model),
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
            &format!(
                "ParameterKey=DeploymentGroupToken,ParameterValue={}",
                prepared.dg_token
            ),
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
        let Some(function) = entry.config.downcast_mut::<Function>() else {
            continue;
        };
        let FunctionCode::Image { image } = &mut function.code else {
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
    let result = apply_terraform_and_import(prepared, target).await?;
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
    let kubeconfig = required_kubeconfig(target)?;
    let result = apply_terraform_and_import(prepared, target).await?;
    let chart_dir = match render_helm_chart(prepared).await {
        Ok(chart_dir) => chart_dir,
        Err(error) => {
            result.cleanup.cleanup().await;
            return Err(error);
        }
    };
    let values_file =
        match write_manager_fetch_values(prepared, &result.deployment, &result.outputs, &chart_dir)
            .await
        {
            Ok(values_file) => values_file,
            Err(error) => {
                result.cleanup.cleanup().await;
                return Err(error);
            }
        };
    let release = format!("alien-e2e-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let namespace = "alien-test".to_string();
    let agent_result = crate::agent::TestAlienAgent::helm_install_with_values(
        chart_dir.path(),
        &values_file,
        &release,
        &namespace,
        Some(&kubeconfig),
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

    let mut ctx = context_from_deployment(prepared, result.deployment, vec![result.cleanup]);
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
) -> anyhow::Result<TerraformApplyResult> {
    let workdir = tempfile::tempdir().context("Failed to create Terraform workdir")?;
    let registry = alien_terraform::TfRegistry::built_in();
    let module = alien_terraform::generate_terraform_module(
        &prepared.rendered_stack,
        target,
        alien_terraform::TerraformOptions {
            registry: &registry,
            stack_settings: stack_settings_for_flow(prepared.model),
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
    let tfvars = terraform_tfvars(prepared, target)?;
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
    let stack_settings = stack_settings_for_flow(prepared.model);
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
) -> anyhow::Result<std::path::PathBuf> {
    let mut values = terraform_helm_values(terraform_outputs)?;
    let values_object = values
        .as_object_mut()
        .context("alien_helm_values output must be a JSON object")?;
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

    let values_path = chart_dir.path().join("distribution-values.yaml");
    fs::write(&values_path, serde_yaml::to_string(&values)?)
        .await
        .context("Failed to write Helm distribution values")?;
    Ok(values_path)
}

fn terraform_helm_values(outputs: &Value) -> anyhow::Result<Value> {
    serde_json::from_str(&terraform_output_string(outputs, "alien_helm_values")?)
        .context("Failed to parse terraform output alien_helm_values")
}

fn runtime_values() -> anyhow::Result<Value> {
    let image = std::env::var("ALIEN_TEST_OVERRIDE_AGENT_IMAGE")
        .ok()
        .filter(|image| !image.is_empty())
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-agent:latest".to_string());
    let (repository, tag) = split_image_tag(&image)?;
    Ok(serde_json::json!({
        "image": {
            "repository": repository,
            "tag": tag,
            "pullPolicy": "IfNotPresent",
        }
    }))
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

fn required_kubeconfig(target: alien_terraform::TerraformTarget) -> anyhow::Result<String> {
    let kubeconfig = std::env::var("KUBECONFIG").with_context(|| {
        format!(
            "Terraform target '{}' Helm distribution requires KUBECONFIG so the test installs into an explicit cluster",
            target.name()
        )
    })?;
    if kubeconfig.trim().is_empty() {
        anyhow::bail!(
            "Terraform target '{}' Helm distribution requires non-empty KUBECONFIG",
            target.name()
        );
    }
    Ok(kubeconfig)
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
        provision_external_secret(&ctx.manager, &ctx.deployment).await?;
    }
    Ok(())
}

async fn provision_external_secret(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
) -> anyhow::Result<()> {
    let url = format!(
        "{}/v1/deployments/{}/vault/alien-vault/secrets/EXTERNAL_TEST_SECRET",
        manager.url, deployment.id
    );
    let response = manager
        .http_client()
        .put(&url)
        .json(&serde_json::json!({ "value": "e2e-test-external-secret-value" }))
        .send()
        .await
        .context("Failed to call vault set secret API")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to provision external secret ({status}): {body}");
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
    let stack_prefix = values
        .get("DeploymentStackPrefix")
        .cloned()
        .context("DeploymentStackPrefix output missing")?;
    let region = values
        .get("DeploymentRegion")
        .cloned()
        .context("DeploymentRegion output missing")?;
    let management_config =
        parse_json_output::<ManagementConfig>(&values, "DeploymentManagementConfig")?;
    let stack_settings = parse_json_output::<StackSettings>(&values, "DeploymentStackSettings")?;
    let resources = parse_cfn_resources(&values)?;

    Ok(StackImportRequest {
        deployment_group_token: token.to_string(),
        deployment_name: stack_name.to_string(),
        stack_prefix,
        source_kind: Some(ImportSourceKind::CloudFormation),
        release_id: None,
        platform,
        region,
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

fn terraform_import_request_from_outputs(
    output: &Value,
    token: &str,
) -> anyhow::Result<StackImportRequest> {
    let platform: Platform = terraform_output_string(output, "deployment_platform")?
        .parse()
        .map_err(|error| anyhow::anyhow!("Invalid deployment_platform output: {error}"))?;
    let stack_prefix = terraform_output_string(output, "deployment_stack_prefix")?;
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

    Ok(StackImportRequest {
        deployment_group_token: token.to_string(),
        deployment_name: format!("terraform-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        stack_prefix,
        source_kind: Some(ImportSourceKind::Terraform),
        release_id: None,
        platform,
        region,
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

/// Terraform can finish before GCP custom roles and IAM bindings are visible
/// to Cloud Run. Probe the imported management identity before deployment
/// starts so Terraform distribution tests do not spend the deployment timeout
/// retrying predictable 403s.
async fn wait_for_gcp_management_permissions(
    config: &TestConfig,
    outputs: &Value,
) -> anyhow::Result<()> {
    let target = config.gcp_target.as_ref().context("GCP target missing")?;
    let management_config: ManagementConfig = serde_json::from_str(&terraform_output_string(
        outputs,
        "deployment_management_config",
    )?)?;
    let service_account_email = match management_config {
        ManagementConfig::Gcp(config) => config.service_account_email,
        other => {
            anyhow::bail!("expected GCP management config, got {other:?}");
        }
    };

    let Some(credentials_json) = target.credentials_json.clone() else {
        return Ok(());
    };

    let base_config = GcpClientConfig {
        project_id: target.project_id.clone(),
        region: target.region.clone(),
        credentials: GcpCredentials::ServiceAccountKey {
            json: credentials_json,
        },
        service_overrides: None,
        project_number: None,
    };
    let http = reqwest::Client::new();
    let probe_name = format!(
        "{}-iam-propagation-probe",
        terraform_output_string(outputs, "deployment_stack_prefix")?
    );
    let probe_service = alien_gcp_clients::gcp::cloudrun::Service {
        description: Some("Alien Terraform IAM propagation probe".to_string()),
        template: Some(alien_gcp_clients::gcp::cloudrun::RevisionTemplate {
            containers: vec![alien_gcp_clients::gcp::cloudrun::Container {
                image: "us-docker.pkg.dev/cloudrun/container/hello".to_string(),
                ports: vec![alien_gcp_clients::gcp::cloudrun::ContainerPort {
                    name: Some("http1".to_string()),
                    container_port: Some(8080),
                }],
                ..Default::default()
            }],
            timeout: Some("60s".to_string()),
            ..Default::default()
        }),
        invoker_iam_disabled: Some(true),
        ..Default::default()
    };

    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let impersonated_config = match base_config
            .impersonate(GcpImpersonationConfig {
                service_account_email: service_account_email.clone(),
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
                        "GCP management service account impersonation did not propagate for {service_account_email} within {timeout:?}: {error}"
                    );
                }
                warn!(
                    %service_account_email,
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

        let resource_manager = alien_gcp_clients::ResourceManagerClient::new(
            http.clone(),
            impersonated_config.clone(),
        );
        let cloud_run = CloudRunClient::new(http.clone(), impersonated_config);
        let project_result = resource_manager
            .get_project_metadata(target.project_id.clone())
            .await;
        let result = match project_result {
            Ok(_) => {
                cloud_run
                    .create_service(
                        target.region.clone(),
                        probe_name.clone(),
                        probe_service.clone(),
                        Some(true),
                    )
                    .await
            }
            Err(error) => Err(error),
        };

        match result {
            Ok(_) => {
                info!(
                    %service_account_email,
                    attempts = attempt,
                    "GCP management IAM permissions are ready"
                );
                return Ok(());
            }
            Err(error) if gcp_management_permission_probe_passed_auth(&error) => {
                info!(
                    %service_account_email,
                    attempts = attempt,
                    "GCP management IAM permissions are ready"
                );
                return Ok(());
            }
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP management IAM permissions did not propagate for {service_account_email} within {timeout:?}: {error}"
                    );
                }
                warn!(
                    %service_account_email,
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

fn gcp_management_permission_probe_passed_auth(error: &alien_gcp_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "INVALID_INPUT" | "REMOTE_RESOURCE_CONFLICT"
    )
}

fn gcp_management_permission_probe_should_retry(error: &alien_gcp_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "REMOTE_ACCESS_DENIED" | "RATE_LIMIT_EXCEEDED" | "REMOTE_SERVICE_UNAVAILABLE" | "TIMEOUT"
    )
}

/// Terraform can finish before Azure federated credentials and role
/// assignments are visible to ARM. Probe the imported management identity
/// against the first live function dependency so deployment starts only after
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

    let management = azure_import_data::<AzureRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;
    let service_bus = azure_import_data::<AzureServiceBusNamespaceImportData>(
        &resources,
        "azure_service_bus_namespace",
    )?;

    let Some(token_file) = std::env::var("AZURE_FEDERATED_TOKEN_FILE")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        warn!("Skipping Azure management permission probe because AZURE_FEDERATED_TOKEN_FILE is not set");
        return Ok(());
    };

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
        terraform_output_string(outputs, "deployment_stack_prefix")?
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

fn azure_import_data<T>(resources: &[ImportedResource], resource_type: &str) -> anyhow::Result<T>
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

fn terraform_tfvars(
    prepared: &DistributionPrepared,
    target: alien_terraform::TerraformTarget,
) -> anyhow::Result<Value> {
    let mut vars = serde_json::Map::new();
    vars.insert(
        "stack_name".to_string(),
        Value::String(format!(
            "alien-e2e-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        )),
    );
    vars.insert(
        "deployment_group_token".to_string(),
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
        vars.insert(
            "kubernetes_namespace".to_string(),
            Value::String("alien-test".to_string()),
        );
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
        if mgmt.oidc_issuer.is_none() {
            if let Some(principal_id) = &mgmt.management_sp_object_id {
                vars.insert(
                    "azure_management_principal_id".to_string(),
                    Value::String(principal_id.clone()),
                );
            }
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

    fn azure_config(
        subscription_id: &str,
        tenant_id: &str,
        region: &str,
        oidc_issuer: Option<&str>,
        oidc_subject: Option<&str>,
        management_sp_object_id: Option<&str>,
    ) -> AzureConfig {
        AzureConfig {
            subscription_id: subscription_id.to_string(),
            tenant_id: tenant_id.to_string(),
            client_id: "client-id".to_string(),
            client_secret: "client-secret".to_string(),
            region: region.to_string(),
            oidc_issuer: oidc_issuer.map(ToString::to_string),
            oidc_subject: oidc_subject.map(ToString::to_string),
            management_sp_client_id: None,
            management_sp_client_secret: None,
            management_sp_object_id: management_sp_object_id.map(ToString::to_string),
        }
    }

    #[tokio::test]
    async fn gcp_distribution_render_grants_live_function_provision() {
        let stack = Stack::new("distribution-gcp".to_string())
            .permission(
                "execution",
                PermissionProfile::new().global(["function/execute"]),
            )
            .add(
                Function::new("alien-rs-fn".to_string())
                    .permissions("execution".to_string())
                    .code(FunctionCode::Image {
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

        assert!(rendered
            .contains("google_project_iam_custom_role.remote_stack_management_functionprovision"));
        assert!(rendered.contains("\"resourcemanager.projects.get\""));
        assert!(rendered.contains("\"run.services.create\""));
        assert!(rendered.contains("\"iam.serviceAccounts.actAs\""));
    }

    #[test]
    fn azure_tfvars_include_oidc_federated_identity_inputs() {
        let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None, None);
        let azure_mgmt = azure_config(
            "mgmt-sub",
            "mgmt-tenant",
            "eastus2",
            Some("https://issuer.example.com"),
            Some("system:serviceaccount:alien:manager"),
            Some("fallback-principal"),
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
    fn azure_tfvars_include_local_fallback_principal_without_oidc() {
        let azure_target = azure_config("target-sub", "target-tenant", "eastus", None, None, None);
        let azure_mgmt = azure_config(
            "mgmt-sub",
            "mgmt-tenant",
            "eastus2",
            None,
            None,
            Some("fallback-principal"),
        );
        let mut vars = serde_json::Map::new();

        insert_azure_tfvars(
            &mut vars,
            &azure_target,
            Some(&azure_mgmt),
            alien_terraform::TerraformTarget::Azure,
        );

        assert_eq!(
            vars.get("azure_management_principal_id")
                .and_then(Value::as_str),
            Some("fallback-principal")
        );
        assert!(vars.get("azure_oidc_issuer").is_none());
        assert!(vars.get("azure_oidc_subject").is_none());
    }
}
