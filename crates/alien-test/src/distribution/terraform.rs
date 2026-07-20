use std::{env, path::Path};

use alien_azure_clients::AzureTokenCache;
use alien_core::import::{AzureRemoteStackManagementImportData, ImportSourceKind};
use alien_core::{AzureClientConfig, AzureCredentials};

use super::*;
use crate::config::AzureConfig;

pub(super) fn terraform_handoff_debug_enabled() -> bool {
    env::var("ALIEN_E2E_DEBUG_TERRAFORM_HANDOFF")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub(super) fn stop_before_helm_for_terraform_handoff_debug(
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

pub(super) async fn apply_terraform_and_import(
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

pub(super) async fn terraform_kubernetes_stack_for_target(
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

pub(super) fn optional_terraform_import_data<T>(
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

pub(super) fn terraform_import_data<T>(
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

pub(super) fn terraform_output_string(outputs: &Value, key: &str) -> anyhow::Result<String> {
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

pub(super) fn insert_azure_tfvars(
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
