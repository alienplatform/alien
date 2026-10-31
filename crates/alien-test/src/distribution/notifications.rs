//! A real release update, setup refusal, artifact reapply, and re-import.

use std::time::Duration;

use alien_core::{SetupConfigurationProfile, Stack, Storage};
use alien_manager::auth::Subject;
use anyhow::{ensure, Context};
use serde_json::Value;
use tokio::{
    fs,
    process::Command,
    time::{sleep, Instant},
};
use tracing::info;

use super::{
    apply_env, apply_render_mutations_with_management_config, cloudformation_import_request,
    create_release, e2e_stack_settings_for_flow, import_stack,
    render_distribution_management_config, rewrite_push_distribution_images, run_command,
    run_terraform_cmd, stack_settings_for_terraform, terraform_import_request_from_outputs,
    terraform_output_json, wait_and_finalize, DistributionArtifactCleanup, DistributionFlow,
    TerraformReservation, TestApp, TestContext,
};

fn change_rule(stack: &mut Stack) -> anyhow::Result<()> {
    let (_, resource) = stack
        .resources_mut()
        .find(|(id, _)| id.as_str() == "objects")
        .context("Notification source missing")?;
    let storage = resource
        .config
        .downcast_mut::<Storage>()
        .context("Expected Storage")?;
    ensure!(
        storage.notifications.len() == 3,
        "Fixture must have three source rules"
    );
    storage.notifications[0].prefix = Some("changed/".into());
    Ok(())
}

fn has_setup_required(error: &Value) -> bool {
    error.get("code").and_then(Value::as_str) == Some("DEPLOYMENT_SETUP_REQUIRED")
        || error.get("source").is_some_and(has_setup_required)
}

/// Change one rule while preserving the second rule and the other source bucket.
/// The manager must refuse the release before the generated package repairs setup.
pub async fn verify_rule_update(ctx: &mut TestContext) -> anyhow::Result<()> {
    ensure!(
        ctx.app == TestApp::StorageNotifications,
        "Notification fixture required"
    );
    let mut prepared = ctx
        .distribution_setup
        .clone()
        .context("Distribution setup required")?;
    let old_release = prepared.release_id.clone();
    change_rule(&mut prepared.built_stack)?;
    prepared.release_id =
        create_release(&prepared.manager, prepared.platform, &prepared.built_stack).await?;
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let deployment = ctx
            .manager
            .client()
            .get_deployment()
            .id(&ctx.deployment.id)
            .send()
            .await?;
        if deployment.status.ends_with("-failed") || deployment.status == "error" {
            let error = serde_json::to_value(&deployment.error)?;
            ensure!(has_setup_required(&error), "Wrong update refusal: {error}");
            ensure!(
                deployment.current_release_id.as_deref() == Some(old_release.as_str()),
                "Refused update must not replace the installed release"
            );
            info!("Frozen notification update correctly refused with DEPLOYMENT_SETUP_REQUIRED");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Frozen notification update was not refused"
        );
        sleep(Duration::from_secs(2)).await;
    }

    let settings =
        e2e_stack_settings_for_flow(prepared.model, &prepared.config, prepared.platform)?;
    let management = render_distribution_management_config(
        prepared.flow,
        prepared.platform,
        &prepared.config,
        &settings,
    );
    let rendered = rewrite_push_distribution_images(
        prepared.built_stack.clone(),
        prepared.platform,
        &prepared.config,
    )?;
    prepared.rendered_stack = apply_render_mutations_with_management_config(
        rendered,
        prepared.platform,
        &settings,
        management,
    )
    .await?;

    let mut request = match ctx.distribution_cleanups.as_mut_slice() {
        [DistributionArtifactCleanup::CloudFormation {
            stack_name,
            region,
            env,
            template_bucket,
            workdir,
            ..
        }] => {
            let registry = alien_cloudformation::CfRegistry::built_in();
            let template = alien_cloudformation::generate_cloudformation_template_with_profile(
                &prepared.rendered_stack,
                alien_cloudformation::CloudFormationOptions {
                    registry: &registry,
                    target: alien_cloudformation::CloudFormationTarget::Aws,
                    stack_settings: settings,
                    setup_target: "aws".into(),
                    setup_fingerprint: "test".into(),
                    setup_fingerprint_version: 1,
                    registration: alien_cloudformation::RegistrationMode::OutputsFallback,
                    description: Some(format!("Alien E2E distribution stack {stack_name}")),
                    management_trust: Default::default(),
                },
                SetupConfigurationProfile::Advanced,
            )?;
            if workdir.is_none() {
                *workdir = Some(tempfile::tempdir()?);
            }
            let path = workdir
                .as_ref()
                .context("CloudFormation workdir missing")?
                .path()
                .join("updated.yaml");
            fs::write(&path, alien_cloudformation::to_yaml(&template)?).await?;
            let mut update = Command::new("aws");
            update.args([
                "cloudformation",
                "update-stack",
                "--stack-name",
                stack_name,
                "--region",
                region,
                "--capabilities",
                "CAPABILITY_IAM",
                "CAPABILITY_NAMED_IAM",
                "CAPABILITY_AUTO_EXPAND",
            ]);
            if let Some(bucket) = template_bucket {
                let mut upload = Command::new("aws");
                upload.args([
                    "s3",
                    "cp",
                    &path.to_string_lossy(),
                    &format!("s3://{bucket}/updated.yaml"),
                    "--region",
                    region,
                ]);
                apply_env(&mut upload, env);
                run_command(upload, "upload changed notification setup").await?;
                update.args([
                    "--template-url",
                    &format!("https://{bucket}.s3.{region}.amazonaws.com/updated.yaml"),
                ]);
            } else {
                update.args(["--template-body", &format!("file://{}", path.display())]);
            }
            let parameters: Vec<_> = template
                .parameters
                .keys()
                .map(|key| format!("ParameterKey={key},UsePreviousValue=true"))
                .collect();
            if !parameters.is_empty() {
                update.arg("--parameters").args(parameters);
            }
            apply_env(&mut update, env);
            run_command(update, "update notification CloudFormation setup").await?;
            let mut wait = Command::new("aws");
            wait.args([
                "cloudformation",
                "wait",
                "stack-update-complete",
                "--stack-name",
                stack_name,
                "--region",
                region,
            ]);
            apply_env(&mut wait, env);
            run_command(wait, "wait for notification setup update").await?;
            cloudformation_import_request(stack_name, region, env, &prepared.dg_token).await?
        }
        [DistributionArtifactCleanup::Terraform { workdir, env }] => {
            let target = match prepared.flow {
                DistributionFlow::TerraformAwsPush => alien_terraform::TerraformTarget::Aws,
                DistributionFlow::TerraformGcpPush => alien_terraform::TerraformTarget::Gcp,
                DistributionFlow::TerraformAzurePush => alien_terraform::TerraformTarget::Azure,
                _ => anyhow::bail!("Cloud Terraform setup required"),
            };
            let registry = alien_terraform::TfRegistry::built_in();
            let module = alien_terraform::generate_product_terraform_module_with_profile(
                &prepared.rendered_stack,
                target,
                alien_terraform::TerraformOptions {
                    display_name: None,
                    registry: &registry,
                    stack_settings: stack_settings_for_terraform(&prepared, target)?,
                    registration: None,
                    helm_install: None,
                    supported_aws_regions: Vec::new(),
                },
                false,
                SetupConfigurationProfile::Advanced,
            )?;
            for (path, contents) in module.iter() {
                let path = workdir.path().join(path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::write(path, contents).await?;
            }
            // Preserve the original tfvars and state: this must update the same deployment.
            run_terraform_cmd(workdir.path(), env, ["validate"]).await?;
            run_terraform_cmd(
                workdir.path(),
                env,
                ["apply", "-auto-approve", "-input=false"],
            )
            .await?;
            let outputs = terraform_output_json(workdir.path(), env).await?;
            // A deployment reserved before its first setup keeps the reserved lane.
            let record = prepared
                .manager
                .deployment_store()
                .get_deployment(&Subject::system(), &ctx.deployment.id)
                .await
                .map_err(|error| anyhow::anyhow!("Failed to read deployment: {error}"))?
                .context("Deployment missing before setup rerun")?;
            let reservation = match (record.setup_target, record.setup_fingerprint_version) {
                (Some(setup_target), Some(setup_fingerprint_version))
                    if !setup_target.is_empty() =>
                {
                    Some(TerraformReservation {
                        deployment_id: ctx.deployment.id.clone(),
                        deployment_name: ctx.deployment.name.clone(),
                        setup_target,
                        setup_fingerprint_version,
                    })
                }
                _ => None,
            };
            terraform_import_request_from_outputs(
                &outputs,
                &prepared.dg_token,
                &ctx.deployment.name,
                reservation.as_ref(),
            )?
        }
        _ => anyhow::bail!("Exactly one cloud setup artifact is required"),
    };
    request.release_id = Some(prepared.release_id.clone());
    if let [DistributionArtifactCleanup::CloudFormation {
        retained_resources, ..
    }] = ctx.distribution_cleanups.as_mut_slice()
    {
        *retained_resources = request.resources.clone();
    }
    let imported = import_stack(&prepared, request, None).await?;
    ensure!(
        imported.deployment.id == ctx.deployment.id,
        "Setup rerun must preserve deployment identity"
    );
    ctx.deployment = imported.deployment;
    wait_and_finalize(ctx).await?;
    let deployment = ctx
        .manager
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await?;
    ensure!(
        deployment.current_release_id.as_deref() == Some(prepared.release_id.as_str()),
        "Setup rerun must promote the changed release"
    );
    ctx.distribution_setup = Some(prepared);
    info!("Changed notification setup reapplied and re-imported into the same deployment");
    Ok(())
}
