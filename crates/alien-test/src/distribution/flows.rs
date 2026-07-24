use super::*;

pub(super) async fn run_cloudformation_k8s(
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

pub(super) async fn run_terraform_cloud(
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

pub(super) async fn run_terraform_k8s(
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

pub(super) async fn run_onprem_k8s(
    _prepared: &mut DistributionPrepared,
) -> anyhow::Result<TestContext> {
    anyhow::bail!(
        "On-prem Helm local-import distribution needs a complete external binding fixture for comprehensive-rust"
    )
}

pub(super) async fn import_stack(
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

pub(super) fn context_from_deployment(
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

pub(super) async fn wait_and_finalize(ctx: &mut TestContext) -> anyhow::Result<()> {
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

pub(super) fn deployment_running_timeout(platform: Platform, app: TestApp) -> Duration {
    match (platform, app) {
        (Platform::Kubernetes, TestApp::FullStackMicroservices) => {
            KUBERNETES_FULL_STACK_DEPLOYMENT_RUNNING_TIMEOUT
        }
        (Platform::Azure, _) => AZURE_DEPLOYMENT_RUNNING_TIMEOUT,
        (Platform::Kubernetes, _) => KUBERNETES_DEPLOYMENT_RUNNING_TIMEOUT,
        _ => DEFAULT_DEPLOYMENT_RUNNING_TIMEOUT,
    }
}
