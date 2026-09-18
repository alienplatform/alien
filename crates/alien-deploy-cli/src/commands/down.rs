//! Destroy command — tears down a deployment.

use crate::deployment_tracking::{DeploymentTracker, TrackedLocalDeployment};
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use clap::Parser;
use std::{future::Future, path::PathBuf, str::FromStr};

use super::up::{create_manager_client, push_deletion, read_token_file};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy a deployment and its resources",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien-deploy destroy --name production

    # Resume direct-setup teardown from a fresh machine
    alien-deploy destroy --deployment-id dep_123 --token-file /run/secrets/deployment-token --manager-url https://manager.example.com

    # Force-delete an imported deployment record
    alien-deploy destroy --name production --force-delete-record

    # Destroy a tracked deployment using explicit manager credentials
    alien-deploy destroy --name production --token ax_dg_abc123... --manager-url https://manager.example.com"
)]
pub struct DownArgs {
    /// Locally tracked deployment name
    #[arg(
        long,
        required_unless_present = "deployment_id",
        conflicts_with = "deployment_id"
    )]
    pub name: Option<String>,

    /// Existing direct-setup deployment ID (for recovery without a local tracker)
    #[arg(long, required_unless_present = "name", conflicts_with = "name")]
    pub deployment_id: Option<String>,

    /// Authentication token (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Force deletion — skip resource teardown, just remove the deployment record
    #[arg(long = "force-delete-record", alias = "force")]
    pub force_delete_record: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn down_command(args: DownArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    // ID-only recovery must not depend on the fresh runner having a usable
    // config directory or deployments.json file.
    let mut tracker = args
        .name
        .as_ref()
        .map(|_| DeploymentTracker::new())
        .transpose()?;
    let tracked = args.name.as_deref().and_then(|name| {
        tracker
            .as_ref()
            .and_then(|tracker| tracker.get(name))
            .cloned()
    });

    let (token, manager_url, tracked_platform, deployment_id, tracked_local) = match tracked {
        Some(tracked) => {
            let token = resolve_token(
                args.token.clone(),
                args.token_file.as_ref(),
                embedded_config,
            )?
            .unwrap_or_else(|| tracked.token.clone());
            let url = args
                .manager_url
                .clone()
                .unwrap_or_else(|| tracked.manager_url.clone());
            let platform = tracked.platform.clone();
            (
                token,
                url,
                Some(platform),
                tracked.deployment_id.clone(),
                tracked.local.clone(),
            )
        }
        None => {
            if let Some(name) = args.name.as_deref() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: format!(
                        "Deployment '{name}' is not tracked. Use --deployment-id with explicit manager credentials to recover a direct-setup deployment from another machine."
                    ),
                }));
            }
            let token = resolve_token(
                args.token.clone(),
                args.token_file.as_ref(),
                embedded_config,
            )?
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: "--deployment-id recovery requires --token, --token-file, or the configured token environment variable.".to_string(),
                })
            })?;
            let manager_url = args.manager_url.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "manager_url".to_string(),
                    message:
                        "--deployment-id recovery requires --manager-url (or ALIEN_MANAGER_URL)."
                            .to_string(),
                })
            })?;
            (
                token,
                manager_url,
                None,
                args.deployment_id
                    .clone()
                    .expect("clap requires deployment ID"),
                None,
            )
        }
    };

    let display_deployment = args.name.as_deref().unwrap_or(&deployment_id);

    let display_name = embedded_config
        .and_then(|config| config.display_name.as_deref())
        .unwrap_or("Alien Deploy");
    output::header(&format!("{display_name} — Destroy"));
    output::status("Deployment:", display_deployment);
    output::status("Manager:", &manager_url);

    let client = create_manager_client(&token, &manager_url)?;

    let deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "fetch deployment".to_string(),
        })?;
    let deployment_json = serde_json::to_value(&*deployment)
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "decode deployment".to_string(),
        })?;
    let import_source = deployment_json
        .get("importSource")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let deployment_status = deployment_json
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let remote_platform = deployment_json
        .get("platform")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::DeploymentFailed {
                operation: "read deployment platform".to_string(),
            })
        })?;
    let platform = validate_remote_platform(tracked_platform.as_deref(), remote_platform)?;

    if let Some(source) = &import_source {
        if !args.force_delete_record {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "force_delete_record".to_string(),
                message: format!(
                    "Deployment '{}' was imported from {}. Refusing to tear down customer-owned IaC resources; rerun with --force-delete-record to remove only the manager record.",
                    display_deployment, source
                ),
            }));
        }
    }

    if args.force_delete_record {
        output::step(1, 2, "Force-deleting deployment...");

        client
            .delete_deployment()
            .id(&deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Forget,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::DeploymentFailed {
                operation: "force deletion".to_string(),
            })?;

        output::step(2, 2, "Done!");
        remove_tracked_deployment(tracker.as_mut(), args.name.as_deref())?;
        if import_source.is_some() {
            output::success(
                "Imported deployment record removed. No resource teardown was performed.",
            );
        } else {
            output::success("Deployment force-deleted. No resource teardown was performed.");
        }
        return Ok(());
    }

    if args.deployment_id.is_some() && matches!(platform, Platform::Local | Platform::Machines) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: format!(
                "Fresh-machine teardown recovery is not supported for {platform} deployments. Use the original host and tracked deployment name."
            ),
        }));
    }
    let run_client_side_deletion = requires_client_side_deletion(platform);
    let total_steps = if run_client_side_deletion { 3 } else { 2 };

    if !run_client_side_deletion {
        if deployment_status == "teardown-required" {
            output::step(
                1,
                total_steps,
                "Deletion already requested; continuing setup teardown...",
            );
        } else {
            output::step(1, total_steps, "Requesting deployment deletion...");

            client
                .delete_deployment()
                .id(&deployment_id)
                .body(alien_manager_api::types::DeleteDeploymentRequest {
                    action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
                })
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::DeploymentFailed {
                    operation: "request deletion".to_string(),
                })?;
        }

        remove_tracked_deployment(tracker.as_mut(), args.name.as_deref())?;

        output::step(total_steps, total_steps, "Done!");
        output::success("Deployment deletion requested.");

        return Ok(());
    }

    run_setup_owned_deletion(
        deployment_status,
        || async {
            output::step(1, total_steps, "Requesting deployment deletion...");

            let response = client
                .delete_deployment()
                .id(&deployment_id)
                .body(alien_manager_api::types::DeleteDeploymentRequest {
                    action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
                })
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::DeploymentFailed {
                    operation: "request deletion".to_string(),
                })?;

            // Older managers do not report whether they completed deletion.
            // Preserve their safe behavior by continuing setup teardown.
            Ok(response.cleanup_required.unwrap_or(true))
        },
        || async {
            output::step(
                2,
                total_steps,
                "Loading target credentials and running deletion...",
            );

            let client_config = destroy_client_config(platform, tracked_local.as_ref()).await?;

            if let Some(local) = tracked_local.as_ref().filter(|local| local.service_managed) {
                output::info("Stopping local operator service before cleanup...");
                super::operator::stop_service_if_running().context(
                    ErrorData::OperatorServiceError {
                        message: format!(
                            "Failed to stop local operator service before cleanup for data directory '{}'",
                            local.data_dir
                        ),
                    },
                )?;
            }

            push_deletion(&client, &deployment_id, platform, client_config).await
        },
    )
    .await?;

    if let Some(local) = tracked_local.as_ref().filter(|local| local.service_managed) {
        output::info("Uninstalling local operator service...");
        super::operator::uninstall_service_if_installed().context(
            ErrorData::OperatorServiceError {
                message: format!(
                "Failed to uninstall local operator service after cleanup for data directory '{}'",
                local.data_dir
            ),
            },
        )?;
    }

    remove_tracked_deployment(tracker.as_mut(), args.name.as_deref())?;

    output::step(total_steps, total_steps, "Done!");
    output::success("Deployment destroyed successfully.");

    Ok(())
}

fn remove_tracked_deployment(
    tracker: Option<&mut DeploymentTracker>,
    tracked_name: Option<&str>,
) -> Result<()> {
    if let (Some(tracker), Some(name)) = (tracker, tracked_name) {
        tracker.remove(name)?;
    }
    Ok(())
}

fn validate_remote_platform(
    tracked_platform: Option<&str>,
    remote_platform: &str,
) -> Result<Platform> {
    let platform = Platform::from_str(remote_platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;
    if let Some(tracked_platform) = tracked_platform {
        let parsed_tracked_platform = Platform::from_str(tracked_platform).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: e,
            })
        })?;
        if parsed_tracked_platform != platform {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "Tracked platform '{tracked_platform}' does not match manager platform '{remote_platform}'."
                ),
            }));
        }
    }
    Ok(platform)
}

async fn destroy_client_config(
    platform: Platform,
    tracked_local: Option<&TrackedLocalDeployment>,
) -> Result<ClientConfig> {
    if platform == Platform::Local {
        let state_directory = tracked_local
            .map(|local| local.data_dir.clone())
            .or_else(|| std::env::var("ALIEN_LOCAL_STATE_DIRECTORY").ok())
            .unwrap_or_else(super::operator::default_service_data_dir);

        return Ok(ClientConfig::Local { state_directory });
    }

    if platform == Platform::Machines {
        return Ok(ClientConfig::Machines);
    }

    ClientConfig::from_std_env(platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                platform
            ),
        })
}

fn requires_client_side_deletion(platform: Platform) -> bool {
    platform != Platform::Machines
}

async fn run_setup_owned_deletion<Request, RequestFuture, Acquire, AcquireFuture>(
    status: &str,
    request_deletion: Request,
    acquire_and_delete: Acquire,
) -> Result<()>
where
    Request: FnOnce() -> RequestFuture,
    RequestFuture: Future<Output = Result<bool>>,
    Acquire: FnOnce() -> AcquireFuture,
    AcquireFuture: Future<Output = Result<()>>,
{
    // Teardown-required already has active setup-owned work. A teardown-failed delete operation is
    // terminal, so the manager API must re-arm that canonical operation before the CLI acquires it.
    let cleanup_required = if status == "teardown-required" {
        true
    } else {
        request_deletion().await?
    };

    if cleanup_required {
        acquire_and_delete().await?;
    }

    Ok(())
}

fn resolve_token(
    explicit_token: Option<String>,
    token_file: Option<&PathBuf>,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<Option<String>> {
    Ok(explicit_token
        .map(Ok)
        .or_else(|| token_file.map(|path| read_token_file(path)))
        .transpose()?
        .or_else(|| {
            embedded_config
                .and_then(|c| c.token_env_var.as_ref())
                .and_then(|env_var| std::env::var(env_var).ok())
        })
        .or_else(|| embedded_config.and_then(|c| c.token.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroy_accepts_explicit_recovery_selector_without_name() {
        let args = DownArgs::try_parse_from([
            "destroy",
            "--deployment-id",
            "dep_recovery",
            "--token",
            "ax_deployment_test",
            "--manager-url",
            "https://manager.example.com",
            "--yes",
        ])
        .expect("fresh-machine recovery arguments should parse");

        assert_eq!(args.deployment_id.as_deref(), Some("dep_recovery"));
        assert!(args.name.is_none());
    }

    #[test]
    fn destroy_requires_exactly_one_local_or_remote_selector() {
        DownArgs::try_parse_from(["destroy", "--yes"])
            .expect_err("destroy without a selector must fail");
        DownArgs::try_parse_from([
            "destroy",
            "--name",
            "production",
            "--deployment-id",
            "dep_recovery",
        ])
        .expect_err("name and deployment ID must be mutually exclusive");
    }

    #[test]
    fn tracked_platform_comparison_is_case_insensitive() {
        assert_eq!(
            validate_remote_platform(Some("AWS"), "aws").expect("same platform"),
            Platform::Aws
        );
        validate_remote_platform(Some("gcp"), "aws")
            .expect_err("different platforms must still be rejected");
    }

    #[tokio::test]
    async fn local_destroy_uses_tracked_data_dir() {
        let local = TrackedLocalDeployment {
            data_dir: "/tmp/alien-tracked-state".to_string(),
            service_managed: true,
        };

        let config = destroy_client_config(Platform::Local, Some(&local))
            .await
            .expect("local config should resolve");

        assert_eq!(
            config,
            ClientConfig::Local {
                state_directory: "/tmp/alien-tracked-state".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn machines_destroy_uses_manager_side_client_config() {
        let config = destroy_client_config(Platform::Machines, None)
            .await
            .expect("machines config should resolve without local credentials");

        assert_eq!(config, ClientConfig::Machines);
    }

    #[test]
    fn machines_destroy_does_not_run_client_side_teardown() {
        assert!(!requires_client_side_deletion(Platform::Machines));
    }

    #[test]
    fn cloud_destroy_runs_client_side_teardown() {
        assert!(requires_client_side_deletion(Platform::Aws));
        assert!(requires_client_side_deletion(Platform::Gcp));
        assert!(requires_client_side_deletion(Platform::Azure));
    }

    #[tokio::test]
    async fn failed_setup_teardown_rearms_before_acquiring() {
        let actions = std::sync::Mutex::new(Vec::new());

        run_setup_owned_deletion(
            "teardown-failed",
            || async {
                actions.lock().unwrap().push("request");
                Ok(true)
            },
            || async {
                actions.lock().unwrap().push("acquire");
                Ok(())
            },
        )
        .await
        .expect("failed teardown should be retried");

        assert_eq!(*actions.lock().unwrap(), ["request", "acquire"]);
    }

    #[tokio::test]
    async fn active_setup_teardown_continues_directly_to_acquisition() {
        let actions = std::sync::Mutex::new(Vec::new());

        run_setup_owned_deletion(
            "teardown-required",
            || async {
                actions.lock().unwrap().push("request");
                Ok(true)
            },
            || async {
                actions.lock().unwrap().push("acquire");
                Ok(())
            },
        )
        .await
        .expect("active teardown should continue");

        assert_eq!(*actions.lock().unwrap(), ["acquire"]);
    }

    #[tokio::test]
    async fn completed_manager_cleanup_skips_setup_teardown() {
        let actions = std::sync::Mutex::new(Vec::new());

        run_setup_owned_deletion(
            "preflights-failed",
            || async {
                actions.lock().unwrap().push("request");
                Ok(false)
            },
            || async {
                actions.lock().unwrap().push("acquire");
                Ok(())
            },
        )
        .await
        .expect("completed manager cleanup should need no setup teardown");

        assert_eq!(*actions.lock().unwrap(), ["request"]);
    }
}
