use std::path::Path;

use alien_core::{KubernetesClusterSettings, Worker, WorkerCode};

use super::*;
use crate::helm_values::{runtime_image_pull_secrets, to_helm_values_yaml};

pub(super) fn set_kubernetes_namespace(settings: &mut StackSettings, namespace: String) {
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

pub(super) fn set_kubernetes_cluster_ownership(
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

pub(super) fn rewrite_push_distribution_images(
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

pub(super) async fn render_helm_chart(prepared: &DistributionPrepared) -> anyhow::Result<TempDir> {
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

pub(super) async fn write_manager_fetch_values(
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

pub(super) fn merge_runtime_values(
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

pub(super) fn runtime_values() -> anyhow::Result<Value> {
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

pub(super) fn merge_chart_service_values(
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

pub(super) fn required_kubernetes_helm_target(
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

pub(super) fn kubernetes_helm_target_from_outputs(
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

pub(super) async fn materialize_kubeconfig_for_helm(
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

pub(super) async fn configure_kubeconfig_auth_for_helm(
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

pub(super) fn random_kubernetes_namespace(prefix: &str) -> String {
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
