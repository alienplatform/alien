use std::collections::BTreeMap;

use alien_core::import::ImportSourceKind;

use super::*;

pub(super) async fn run_cloudformation_aws(
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

async fn cloudformation_import_request(
    stack_name: &str,
    region: &str,
    env: &[(String, String)],
    token: &str,
) -> anyhow::Result<StackImportRequest> {
    let values = cloudformation_outputs(stack_name, region, env).await?;
    cloudformation_import_request_from_outputs(stack_name, token, &values)
}

pub(super) async fn cloudformation_outputs(
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

pub(super) fn cloudformation_import_request_from_outputs(
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

pub(super) async fn wait_for_cloudformation_stack_create(
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
