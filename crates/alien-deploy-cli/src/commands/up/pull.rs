use super::*;

use crate::commands::operator::generate_encryption_key_public;

pub(super) struct ManagerInstallContext {
    pub(super) manager_url: String,
    pub(super) management_config: Option<ManagementConfig>,
}

/// Discover the manager URL and platform-managed install context via the platform API.
///
/// Calls GET /v1/resolve?platform=X to resolve the manager.
/// The token's scope (DG, project, etc.) provides the project context
/// to the server — no need to call whoami first.
pub(super) async fn discover_manager_install_context(
    base_url: &str,
    token: &str,
    platform: &str,
) -> Result<ManagerInstallContext> {
    let http_client = {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid token format".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?
    };

    let url = format!(
        "{}/v1/resolve?platform={}",
        base_url,
        urlencoding::encode(platform),
    );

    let resp = http_client
        .get(&url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to call /v1/resolve on platform API".to_string(),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to resolve manager via platform API (HTTP {}): {}",
                status, body
            ),
        }));
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveResponse {
        manager_url: String,
        install_context: Option<ResolveInstallContext>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveInstallContext {
        management_config: ManagementConfig,
    }

    let resolved: ResolveResponse =
        resp.json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse /v1/resolve response".to_string(),
            })?;

    Ok(ManagerInstallContext {
        manager_url: resolved.manager_url,
        management_config: resolved
            .install_context
            .map(|context| context.management_config),
    })
}

pub fn create_manager_client(token: &str, manager_url: &str) -> Result<ServerClient> {
    let http_client = create_manager_http_client(token)?;
    Ok(ServerClient::new_with_client(manager_url, http_client))
}

pub(crate) fn create_manager_http_client(token: &str) -> Result<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })
}

pub(super) fn parse_deployment_status(raw_status: &str) -> Result<DeploymentStatus> {
    match raw_status.to_ascii_lowercase().as_str() {
        "pending" => Ok(DeploymentStatus::Pending),
        "preflights-failed" => Ok(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "waiting-for-machines" => Ok(DeploymentStatus::WaitingForMachines),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
        "teardown-required" => Ok(DeploymentStatus::TeardownRequired),
        "teardown-failed" => Ok(DeploymentStatus::TeardownFailed),
        "deleted" => Ok(DeploymentStatus::Deleted),
        "error" => Ok(DeploymentStatus::Error),
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status returned by manager: {raw_status}"),
        })),
    }
}

pub(super) fn deployment_status_str(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::PreflightsFailed => "preflights-failed",
        DeploymentStatus::InitialSetup => "initial-setup",
        DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::WaitingForMachines => "waiting-for-machines",
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
        DeploymentStatus::TeardownRequired => "teardown-required",
        DeploymentStatus::TeardownFailed => "teardown-failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
}

/// Result of initializing with the manager.
pub(super) struct InitResult {
    pub(super) deployment_id: String,
    pub(super) deployment_model: DeploymentModel,
    /// Deployment-scoped token returned by the manager (when using a deployment group token).
    /// If present, this should replace the original token for subsequent requests.
    pub(super) deployment_token: Option<String>,
}

pub(super) async fn initialize_deployment(
    client: &ServerClient,
    _token: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    name: &str,
    stack_settings: &StackSettings,
    input_values: HashMap<String, serde_json::Value>,
) -> Result<InitResult> {
    let body = alien_manager_api::types::InitializeRequest {
        name: Some(name.to_string()),
        platform: Some(sdk_platform(platform)),
        base_platform: base_platform.map(sdk_platform),
        initial_desired_release: alien_manager_api::types::InitialDesiredRelease::Active,
        stack_settings: Some(sdk_stack_settings(stack_settings)?),
        input_values: input_values.into_iter().collect(),
        scope: None,
        permission: None,
        setup_method: None,
    };

    let response = match client.initialize().body(body).send().await {
        Ok(response) => response,
        Err(error) => {
            // Read the error body so server-side rejections surface their own
            // message; the manager-URL hint only applies when the manager was
            // unreachable.
            let error = alien_manager_api::convert_sdk_error_reading_body(error).await;
            let context = if error.code == "COMMUNICATION_ERROR" {
                ErrorData::ConfigurationError {
                    message: "Failed to initialize with manager. Is the manager running? Check that --manager-url is correct.".to_string(),
                }
            } else {
                ErrorData::DeploymentFailed {
                    operation: "initialize".to_string(),
                }
            };
            return Err(error).context(context);
        }
    };

    let init = response.into_inner();
    let deployment_model = manager_deployment_model(init.deployment_model)?;
    Ok(InitResult {
        deployment_id: init.deployment_id,
        deployment_model,
        deployment_token: init.token,
    })
}

fn manager_deployment_model<T: Serialize>(deployment_model: T) -> Result<DeploymentModel> {
    let value = serde_json::to_value(deployment_model)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize manager deployment model".to_string(),
        })?;
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize manager deployment model".to_string(),
        })
}

fn sdk_platform(platform: Platform) -> alien_manager_api::types::Platform {
    match platform {
        Platform::Aws => alien_manager_api::types::Platform::Aws,
        Platform::Gcp => alien_manager_api::types::Platform::Gcp,
        Platform::Azure => alien_manager_api::types::Platform::Azure,
        Platform::Kubernetes => alien_manager_api::types::Platform::Kubernetes,
        Platform::Machines => alien_manager_api::types::Platform::Machines,
        Platform::Local => alien_manager_api::types::Platform::Local,
        Platform::Test => alien_manager_api::types::Platform::Test,
    }
}

pub(super) fn sdk_stack_settings(
    stack_settings: &StackSettings,
) -> Result<alien_manager_api::types::StackSettings> {
    let value = serde_json::to_value(stack_settings)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize stack settings".to_string(),
        })?;
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to convert stack settings for manager API".to_string(),
        })
}

pub(super) async fn run_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    platform: Platform,
    embedded_config: Option<&DeployCliConfig>,
    public_endpoints: Option<&PublicEndpointUrls>,
) -> Result<()> {
    match platform {
        Platform::Kubernetes => {
            run_kubernetes_pull_model(
                client,
                args,
                manager_url,
                token,
                deployment_id,
                deployment_name,
                stack_settings,
            )
            .await
        }
        _ => {
            let data_dir = local_operator_data_dir(args);
            run_local_pull_model(
                args,
                manager_url,
                token,
                deployment_id,
                deployment_name,
                &platform.to_string(),
                embedded_config,
                public_endpoints,
                data_dir.as_deref(),
            )
            .await
        }
    }
}

fn default_foreground_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".alien")
        .join("operator-data")
}

fn local_operator_data_dir(args: &UpArgs) -> Option<String> {
    args.data_dir.clone().or_else(|| {
        if args.foreground {
            Some(default_foreground_data_dir().to_string_lossy().to_string())
        } else {
            Some(crate::commands::operator::default_service_data_dir())
        }
    })
}

pub(super) fn local_tracking_metadata(
    args: &UpArgs,
    platform: Platform,
) -> Option<TrackedLocalDeployment> {
    if platform != Platform::Local {
        return None;
    }

    local_operator_data_dir(args).map(|data_dir| TrackedLocalDeployment {
        data_dir,
        service_managed: !args.foreground,
    })
}

async fn run_local_pull_model(
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    platform: &str,
    embedded_config: Option<&DeployCliConfig>,
    public_endpoints: Option<&PublicEndpointUrls>,
    data_dir: Option<&str>,
) -> Result<()> {
    let encryption_key = args
        .encryption_key
        .clone()
        .unwrap_or_else(|| generate_encryption_key_public());

    // Find or download the alien-operator binary
    let binary_path = find_or_download_operator_binary(embedded_config).await?;

    output::info(&format!("Operator binary: {}", binary_path.display()));

    if args.foreground {
        return run_operator_foreground(
            &binary_path,
            manager_url,
            token,
            deployment_id,
            deployment_name,
            platform,
            &encryption_key,
            data_dir,
            public_endpoints,
            args.enable_local_debug,
            args.local_debug_shell_command.as_deref(),
        )
        .await;
    }

    output::info("Installing alien-operator as a system service...");

    // Delegate to the operator install logic
    let install_args = crate::commands::operator::InstallArgs {
        binary: Some(binary_path),
        sync_url: manager_url.to_string(),
        sync_token: token.to_string(),
        deployment_id: Some(deployment_id.to_string()),
        operator_name: Some(deployment_name.to_string()),
        platform: platform.to_string(),
        data_dir: data_dir.map(ToOwned::to_owned),
        encryption_key: args.encryption_key.clone(),
        public_endpoints: public_endpoints.cloned(),
        enable_local_debug: args.enable_local_debug,
        local_debug_shell_command: args.local_debug_shell_command.clone(),
    };

    crate::commands::operator::install_service(install_args)?;

    output::success("alien-operator installed and running as a system service.");
    output::info("The operator will sync with the manager and deploy updates automatically.");
    output::info("Use 'alien-deploy operator status' to check the service.");

    Ok(())
}

/// Run the operator as a foreground child process (for testing).
async fn run_operator_foreground(
    binary_path: &std::path::Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    operator_name: &str,
    platform: &str,
    encryption_key: &str,
    data_dir_override: Option<&str>,
    public_endpoints: Option<&PublicEndpointUrls>,
    enable_local_debug: bool,
    local_debug_shell_command: Option<&str>,
) -> Result<()> {
    use std::io::Write;

    output::info("Running operator in foreground (Ctrl+C to stop)...");

    let data_dir = if let Some(dir) = data_dir_override {
        std::path::PathBuf::from(dir)
    } else {
        default_foreground_data_dir()
    };

    // The operator rejects `--sync-token`/`--encryption-key` because argv is
    // visible in `ps` / `/proc/<pid>/cmdline`. Write each secret to its own
    // tempfile (0o600 on Unix) and pass the path via `--*-file`. The
    // `NamedTempFile`s must outlive the child process — drop deletes them.
    let mut sync_token_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for sync token".to_string(),
        },
    )?;
    sync_token_file
        .write_all(token.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write sync token".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            sync_token_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let mut encryption_key_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for encryption key".to_string(),
        },
    )?;
    encryption_key_file
        .write_all(encryption_key.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write encryption key".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            encryption_key_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let mut public_endpoints_file = match public_endpoints {
        Some(public_endpoints) => {
            let mut file = tempfile::NamedTempFile::new().into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to create temp file for public endpoints".to_string(),
                },
            )?;
            serde_json::to_writer(&mut file, public_endpoints)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to write public endpoints".to_string(),
                })?;
            Some(file)
        }
        None => None,
    };

    let mut command = tokio::process::Command::new(binary_path);
    command
        .arg("--platform")
        .arg(platform)
        .arg("--sync-url")
        .arg(manager_url)
        .arg("--sync-token-file")
        .arg(sync_token_file.path())
        .arg("--deployment-id")
        .arg(deployment_id)
        .arg("--operator-name")
        .arg(operator_name)
        .arg("--encryption-key-file")
        .arg(encryption_key_file.path())
        .arg("--data-dir")
        .arg(&data_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    if let Some(file) = public_endpoints_file.as_ref() {
        command.arg("--public-endpoints-file").arg(file.path());
    }
    if enable_local_debug {
        command.arg("--enable-local-debug");
    }
    if let Some(shell_command) = local_debug_shell_command {
        command
            .arg("--local-debug-shell-command")
            .arg(shell_command);
    }

    let status =
        command
            .status()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to run operator: {}", binary_path.display()),
            })?;

    // Tempfiles drop here, after the child exits.
    drop(sync_token_file);
    drop(encryption_key_file);
    drop(public_endpoints_file.take());

    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Operator exited with status: {}", status),
        }));
    }

    Ok(())
}

async fn run_kubernetes_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
) -> Result<()> {
    output::info("Kubernetes platform detected — installing alien-operator with Helm.");
    let stack = fetch_kubernetes_release_stack(client, deployment_id).await?;
    let namespace = args
        .namespace
        .clone()
        .unwrap_or_else(|| format!("alien-{}", sanitize_kubernetes_dns_label(deployment_name)));
    let release = args
        .helm_release
        .clone()
        .unwrap_or_else(|| "alien-operator".to_string());
    let operator_image = args
        .operator_image
        .clone()
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-operator:latest".to_string());

    let chart_dir = render_kubernetes_helm_chart(&stack, stack_settings, deployment_name)?;
    let values_file = write_kubernetes_helm_values(
        chart_dir.path(),
        manager_url,
        token,
        deployment_id,
        deployment_name,
        stack_settings,
        &operator_image,
    )?;

    helm_upgrade_install(
        chart_dir.path(),
        &values_file,
        &release,
        &namespace,
        args.kubeconfig.as_deref(),
        args.kube_context.as_deref(),
    )
    .await?;

    output::success(&format!(
        "alien-operator Helm release '{}' is installed in namespace '{}'.",
        release, namespace
    ));
    output::info(&format!("Deployment ID: {}", deployment_id));

    Ok(())
}

async fn fetch_kubernetes_release_stack(
    client: &ServerClient,
    deployment_id: &str,
) -> Result<Stack> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let release_id = deployment
        .desired_release_id
        .or(deployment.current_release_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Deployment has no release to install as a Kubernetes Helm chart"
                    .to_string(),
            })
        })?;
    let release = client
        .get_release()
        .id(&release_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value = release.stack.kubernetes.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Release '{release_id}' does not contain a Kubernetes stack"),
        })
    })?;

    serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse Kubernetes stack from release '{release_id}'"),
        })
}

fn render_kubernetes_helm_chart(
    stack: &Stack,
    stack_settings: &StackSettings,
    deployment_name: &str,
) -> Result<tempfile::TempDir> {
    let chart_dir =
        tempfile::tempdir()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to create temporary Helm chart directory".to_string(),
            })?;
    let registry = alien_helm::HelmRegistry::built_in();
    let mut helm_settings = stack_settings.clone();
    // Helm charts install the Kubernetes operator, which always polls the manager.
    helm_settings.deployment_model = DeploymentModel::Pull;
    let chart = alien_helm::generate_helm_chart(
        stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings: helm_settings,
            chart_name: sanitize_kubernetes_dns_label(deployment_name),
        },
    )
    .map_err(|error| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to generate Kubernetes Helm chart: {error}"),
        })
    })?;

    for (relative_path, contents) in chart.files {
        let path = chart_dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "create directory".to_string(),
                    file_path: parent.display().to_string(),
                    reason: "Failed to create Helm chart output directory".to_string(),
                },
            )?;
        }
        std::fs::write(&path, contents).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "write".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to write generated Helm chart file".to_string(),
            },
        )?;
    }

    Ok(chart_dir)
}

fn write_kubernetes_helm_values(
    chart_dir: &Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    operator_image: &str,
) -> Result<PathBuf> {
    let (repository, tag) = split_image_tag(operator_image)?;
    let mut helm_settings = stack_settings.clone();
    // Helm values are consumed by the Kubernetes operator, which always runs pull-model.
    helm_settings.deployment_model = DeploymentModel::Pull;
    let values = serde_json::json!({
        "management": {
            "token": token,
            "name": deployment_name,
            "url": manager_url,
            "deploymentId": deployment_id,
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        },
        "runtime": {
            "image": {
                "repository": repository,
                "tag": tag,
                "pullPolicy": "IfNotPresent",
            },
            "encryption": {
                "key": generate_encryption_key_public(),
            }
        },
        "stackSettings": helm_settings,
        "infrastructure": null,
    });
    let values_path = chart_dir.join("alien-deploy-values.json");
    let contents = serde_json::to_string_pretty(&values)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize Helm values".to_string(),
        })?;
    std::fs::write(&values_path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: values_path.display().to_string(),
            reason: "Failed to write Helm values file".to_string(),
        })?;
    Ok(values_path)
}

async fn helm_upgrade_install(
    chart_dir: &Path,
    values_file: &Path,
    release: &str,
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new("helm");
    cmd.arg("upgrade")
        .arg("--install")
        .arg(release)
        .arg(chart_dir)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("-f")
        .arg(values_file)
        .arg("--wait")
        .arg("--timeout")
        .arg("300s");

    if let Some(kubeconfig) = kubeconfig {
        cmd.env("KUBECONFIG", kubeconfig);
    }
    if let Some(context) = kube_context {
        cmd.arg("--kube-context").arg(context);
    }

    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to execute helm. Ensure Helm is installed and available on PATH."
                .to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Helm upgrade/install failed: {stderr}"),
        }));
    }

    Ok(())
}

pub(super) fn split_image_tag(image: &str) -> Result<(String, String)> {
    if image.contains('@') {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "operator-image".to_string(),
            message: "Kubernetes Helm installs require a tag-based operator image, not a digest"
                .to_string(),
        }));
    }
    let last_slash = image.rfind('/').unwrap_or(0);
    let tag_separator = image[last_slash..].rfind(':').map(|idx| last_slash + idx);
    let Some(separator) = tag_separator else {
        return Ok((image.to_string(), "latest".to_string()));
    };
    Ok((
        image[..separator].to_string(),
        image[separator + 1..].to_string(),
    ))
}

pub(super) fn sanitize_kubernetes_dns_label(value: &str) -> String {
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
        if out.len() == 63 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "alien".to_string()
    } else {
        out
    }
}

/// Default releases URL for downloading binaries.
const DEFAULT_RELEASES_URL: &str = "https://releases.alien.dev";

/// Find the alien-operator binary locally, or download it from the releases URL.
async fn find_or_download_operator_binary(
    embedded_config: Option<&DeployCliConfig>,
) -> Result<std::path::PathBuf> {
    // Try to find it locally first
    if let Ok(path) = crate::commands::operator::which_operator_binary() {
        return Ok(path);
    }

    // Download to ~/.alien/bin/alien-operator
    let home = dirs::home_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine home directory".to_string(),
        })
    })?;

    let bin_dir = home.join(".alien").join("bin");
    std::fs::create_dir_all(&bin_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: bin_dir.display().to_string(),
            reason: "Failed to create ~/.alien/bin directory".to_string(),
        })?;

    let binary_path = bin_dir.join("alien-operator");

    let (os, arch) = detect_os_arch()?;
    let url = if let Some(url) = embedded_config.and_then(|config| config.agent_binary_url.as_ref())
    {
        url.clone()
    } else {
        let releases_url = std::env::var("ALIEN_RELEASES_URL")
            .unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());
        format!(
            "{}/alien-operator/latest/{}-{}/alien-operator",
            releases_url, os, arch
        )
    };

    output::info(&format!("Downloading alien-operator from {}...", url));

    let response =
        reqwest::get(&url)
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to download alien-operator from {}", url),
            })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to download alien-operator: HTTP {}",
                response.status()
            ),
        }));
    }

    let bytes =
        response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read alien-operator download response".to_string(),
            })?;

    std::fs::write(&binary_path, &bytes)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: binary_path.display().to_string(),
            reason: "Failed to write alien-operator binary".to_string(),
        })?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: binary_path.display().to_string(),
                reason: "Failed to make alien-operator executable".to_string(),
            })?;
    }

    output::success("alien-operator downloaded successfully.");

    Ok(binary_path)
}

fn detect_os_arch() -> Result<(&'static str, &'static str)> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported OS: {}", std::env::consts::OS),
        }));
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported architecture: {}", std::env::consts::ARCH),
        }));
    };

    Ok((os, arch))
}
