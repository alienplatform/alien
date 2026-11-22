//! Destroy command — tears down a deployment.

use crate::deployment_tracking::{DeploymentTracker, TrackedLocalDeployment};
use crate::error::{ErrorData, Result};
use crate::output;
use crate::rerun_credential::{self, InClusterUse, KubeAccess};
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{ClientConfig, Platform, StackState};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::SdkResultExt as ManagerSdkResultExt;
use clap::Parser;
use std::{future::Future, path::PathBuf, str::FromStr};

use super::up::{
    create_manager_client, push_deletion_with, read_token_file,
    refuse_operator_removed_outside_kubernetes,
};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Remove deployment access and retain durable data by default",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien-deploy destroy --name production

    # Resume direct-setup teardown from a fresh machine
    alien-deploy destroy --deployment-id dep_123 --token-file /run/secrets/deployment-token --manager-url https://manager.example.com

    # Explicitly erase durable data
    alien-deploy destroy --name production --erase-data

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

    /// Permanently erase durable data instead of retaining it
    #[arg(long, conflicts_with = "force_delete_record")]
    pub erase_data: bool,

    /// Skip confirmation prompts. With --operator-removed this also confirms a cluster that only
    /// the Operator's Secret vouches for.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Kubernetes namespace of a Kubernetes install (default: the one it was installed in).
    #[arg(long, env = "ALIEN_KUBERNETES_NAMESPACE")]
    pub namespace: Option<String>,

    /// Helm release of a Kubernetes install (default: the one it was installed as).
    #[arg(long, env = "ALIEN_HELM_RELEASE")]
    pub helm_release: Option<String>,

    /// Kubeconfig path for Kubernetes installs. Defaults to KUBECONFIG or kubectl defaults.
    /// A Kubernetes destroy that uses cluster credentials from the kubeconfig (the runtime
    /// cleanup of a removed Operator, an erase) reads this path; credentials that come from the
    /// server URL or from inside the cluster do not use it, and `--operator-removed` refuses it.
    #[arg(long, env = "KUBECONFIG")]
    pub kubeconfig: Option<String>,

    /// Kubernetes context for Kubernetes installs: names the context of the kubeconfig the cluster
    /// calls use (the runtime cleanup of a removed Operator, an erase). Credentials from the server
    /// URL or from inside the cluster do not use it, and `--operator-removed` refuses it there.
    #[arg(long, env = "ALIEN_KUBE_CONTEXT")]
    pub kube_context: Option<String>,

    /// The deployment's Operator is gone (for example the Helm release was already uninstalled):
    /// run its runtime cleanup from here with your cluster credentials instead of waiting for
    /// it. Kubernetes installs only.
    #[arg(long)]
    pub operator_removed: bool,
}

pub async fn down_command(args: DownArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let in_cluster = InClusterUse::default();
    down_command_with(args, embedded_config, &in_cluster)
        .await
        .map_err(|error| in_cluster.explain(error))
}

async fn down_command_with(
    args: DownArgs,
    embedded_config: Option<&DeployCliConfig>,
    in_cluster: &InClusterUse,
) -> Result<()> {
    let delete_action = if args.erase_data {
        alien_manager_api::types::DeleteDeploymentAction::Erase
    } else {
        alien_manager_api::types::DeleteDeploymentAction::Retain
    };
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
            let explicit = resolve_token(
                args.token.clone(),
                args.token_file.as_ref(),
                embedded_config,
            )?;
            let cli_name = embedded_config
                .and_then(|config| config.name.as_deref())
                .unwrap_or("alien-deploy");
            let rerun = rerun_credential::resolve(
                explicit,
                &tracked,
                &KubeAccess {
                    release: args.helm_release.clone(),
                    namespace: args.namespace.clone(),
                    kubeconfig: args.kubeconfig.clone(),
                    kube_context: args.kube_context.clone(),
                },
                cli_name,
                "destroy",
            )
            .await?;
            in_cluster.note(&tracked.name, &rerun);
            let token = rerun.token;
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
        .into_sdk_error()
        .await
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
    // Before any request or local stop: a refusal later would leave the delete already sent.
    refuse_operator_removed_outside_kubernetes(platform, args.operator_removed)?;
    let site_bound = site_bound_resource_ids(&deployment_json)?;
    // Keys the direct setup created stay: read before teardown removes their state.
    let kept_keys = alien_cli_common::kept_keys::kept_keys(
        deployment_json
            .get("stackState")
            .and_then(|state| serde_json::from_value::<StackState>(state.clone()).ok())
            .as_ref(),
    );

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
            .into_sdk_error()
            .await
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

    if deployment_status == "deleted" {
        finish_tracked_deletion(tracker.as_mut(), args.name.as_deref(), true)?;
        output::success("Deployment data was already erased.");
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
    refuse_a_changed_policy(
        deployment_status,
        args.erase_data,
        alien_deployment::manager_api_transport::recorded_deletion_policy(&deployment_json),
    )?;
    let run_client_side_deletion = requires_client_side_deletion(platform);
    let total_steps = if run_client_side_deletion { 3 } else { 2 };

    if !run_client_side_deletion {
        if needs_no_new_request(deployment_status, args.erase_data) {
            output::step(1, total_steps, "Deletion already requested; continuing...");
        } else {
            output::step(1, total_steps, "Requesting deployment deletion...");

            client
                .delete_deployment()
                .id(&deployment_id)
                .body(alien_manager_api::types::DeleteDeploymentRequest {
                    action: delete_action,
                })
                .send()
                .await
                .into_sdk_error()
                .await
                .context(ErrorData::DeploymentFailed {
                    operation: "request deletion".to_string(),
                })?;
        }

        // Machines cleanup is asynchronous. Keep recovery authority until a later
        // read confirms erasure; a successful request alone does not erase data.
        finish_tracked_deletion(tracker.as_mut(), args.name.as_deref(), false)?;

        output::step(total_steps, total_steps, "Done!");
        output::success("Deployment deletion requested.");

        return Ok(());
    }

    run_setup_owned_deletion(
        deployment_status,
        args.erase_data,
        || async {
            output::step(1, total_steps, "Requesting deployment deletion...");

            let response = client
                .delete_deployment()
                .id(&deployment_id)
                .body(alien_manager_api::types::DeleteDeploymentRequest {
                    action: delete_action,
                })
                .send()
                .await
                .into_sdk_error()
                .await
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
            let (client_config, unsteerable_kube_flags) = if platform == Platform::Kubernetes {
                with_kubernetes_access(
                    client_config,
                    args.kubeconfig.as_deref(),
                    args.kube_context.as_deref(),
                    args.operator_removed,
                )
            } else {
                (client_config, None)
            };

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

            push_deletion_with()
                .client(&client)
                .manager_url(&manager_url)
                .token(&token)
                .deployment_id(&deployment_id)
                .platform(platform)
                .client_config(client_config)
                .operator_removed(args.operator_removed)
                .erase_data(args.erase_data)
                .assume_yes(args.yes)
                .delete_requested(true)
                .maybe_unsteerable_kube_flags(unsteerable_kube_flags)
                .maybe_label_domain(embedded_config.and_then(|c| c.label_domain.clone()))
                .call()
                .await
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

    finish_tracked_deletion(tracker.as_mut(), args.name.as_deref(), args.erase_data)?;

    output::step(total_steps, total_steps, "Done!");
    if args.erase_data && site_bound.is_empty() {
        output::success("Deployment and its durable data erased successfully.");
    } else if args.erase_data {
        output::success("Deployment and the durable data Alien created erased successfully.");
    } else {
        output::success("Deployment stopped. Durable data and recovery tracking retained; use --erase-data to erase later.");
    }
    alien_cli_common::kept_keys::print_kept_keys(&kept_keys);
    if !site_bound.is_empty() {
        output::info(&format!(
            "Released, not erased: {}. These were supplied where the deployment was installed (for example Helm values or the chart's database); remove them there.",
            site_bound.join(", ")
        ));
    }

    Ok(())
}

/// Resources the record shows as adopted from a binding supplied at the site. Teardown
/// releases them and leaves whatever they point at in place, so one already `deleted` by an
/// earlier retain or partial erase was released too, and still isn't erased.
fn site_bound_resource_ids(deployment: &serde_json::Value) -> Result<Vec<String>> {
    let Some(stack_state) = deployment
        .get("stackState")
        .filter(|value| !value.is_null())
    else {
        return Ok(Vec::new());
    };
    let stack_state: StackState = serde_json::from_value(stack_state.clone())
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "decode deployment stack state".to_string(),
        })?;
    let mut ids: Vec<String> = stack_state
        .resources
        .into_iter()
        .filter(|(_, resource)| resource.external_binding)
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    Ok(ids)
}

/// Retained data still needs its original credentials and local state directory
/// for a later explicit erase. Do not discard them after a retain or queued delete.
fn finish_tracked_deletion(
    tracker: Option<&mut DeploymentTracker>,
    tracked_name: Option<&str>,
    erasure_completed: bool,
) -> Result<()> {
    if erasure_completed {
        remove_tracked_deployment(tracker, tracked_name)?;
    }
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

/// A Kubernetes install is deleted on the cluster the person named with `--kubeconfig` and
/// `--kube-context`, not on whatever their shell currently points at. Whenever the command is
/// about to delete on that cluster with `--operator-removed` (the runtime cleanup of a removed
/// Operator, or an erase in the setup teardown of that command), the cluster is checked against
/// the deployment's record (`cluster_check`) first. An erase without the flag is not checked.
/// Credentials that are not a kubeconfig (an API server URL or the in-cluster environment) cannot be steered by these flags: when the flags were given, the
/// second value is the refusal to raise if (and only if) a cluster call follows, so a retain
/// that touches no cluster is not stopped by an exported `KUBECONFIG`.
fn with_kubernetes_access(
    config: ClientConfig,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    operator_removed: bool,
) -> (ClientConfig, Option<String>) {
    let ClientConfig::Kubernetes(mut kubernetes) = config else {
        return (config, None);
    };
    let mut refusal = None;
    match kubernetes.as_mut() {
        alien_core::KubernetesClientConfig::Kubeconfig {
            kubeconfig_path,
            context,
            ..
        } => {
            if let Some(path) = kubeconfig {
                *kubeconfig_path = Some(path.to_string());
            }
            if let Some(name) = kube_context {
                *context = Some(name.to_string());
            }
        }
        // Only `--operator-removed` makes this command act on the cluster the person named; a
        // destroy without it ignores kubeconfig settings these credentials cannot use (an
        // exported KUBECONFIG, say), as it always did.
        _ if operator_removed && (kubeconfig.is_some() || kube_context.is_some()) => {
            refusal = Some("--kubeconfig, KUBECONFIG and --kube-context do not apply: the Kubernetes credentials come from KUBERNETES_SERVER_URL or the in-cluster environment, not from a kubeconfig. Unset those or drop the flags. Nothing was deleted.".to_string());
        }
        _ => {}
    }
    (ClientConfig::Kubernetes(kubernetes), refusal)
}

fn requires_client_side_deletion(platform: Platform) -> bool {
    platform != Platform::Machines
}

/// A deletion this run does not request again. The manager refuses a second request while the
/// first is in progress (`delete-pending`, `deleting`), whatever the policy of the second; once
/// the runtime cleanup is over (`teardown-required`, `retention-required`) a run that does not
/// ask to erase goes on with the recorded deletion. A run that asks to erase a retained
/// deployment is a new request.
fn deletion_already_requested(status: &str, erase_data: bool) -> bool {
    matches!(status, "delete-pending" | "deleting")
        || (!erase_data && matches!(status, "teardown-required" | "retention-required"))
}

/// A deletion that is under way and whose recorded policy a rerun cannot change.
fn deletion_under_way(status: &str) -> bool {
    matches!(status, "delete-pending" | "deleting" | "teardown-required")
}

/// The request this run does not send because the deletion needs none: one already requested
/// (see `deletion_already_requested`), or a retained deployment this run does not erase.
fn needs_no_new_request(status: &str, erase_data: bool) -> bool {
    deletion_already_requested(status, erase_data) || (!erase_data && status == "retained")
}

/// A rerun whose `--erase-data` differs from the policy the first delete request recorded does
/// nothing: the teardown follows the record, and neither silently erasing without the flag on this
/// run's command line nor reporting success for data that was kept is acceptable. This is the fast,
/// clear refusal before any request; the acquire repeats it with the record read under the lock
/// (`acquire_setup_delete_deployment`), which is the one that guarantees it. A record without a
/// policy is not refused.
fn refuse_a_changed_policy(
    status: &str,
    flag_erase: bool,
    recorded_erase: Option<bool>,
) -> Result<()> {
    match recorded_erase {
        Some(recorded_erase) if deletion_under_way(status) && recorded_erase != flag_erase => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "erase_data".to_string(),
                message: alien_deployment::manager_api_transport::changed_policy_message(
                    recorded_erase,
                )
                .to_string(),
            }))
        }
        _ => Ok(()),
    }
}

async fn run_setup_owned_deletion<Request, RequestFuture, Acquire, AcquireFuture>(
    status: &str,
    erase_data: bool,
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
    if status == "retained" && !erase_data {
        return Ok(());
    }
    // A deletion that was already requested (an earlier run of this command that did not
    // finish: it was refused on the wrong cluster, timed out, or the Operator was removed) is
    // not requested again: the manager refuses a second request while the first is in progress.
    let cleanup_required = if deletion_already_requested(status, erase_data) {
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

    fn kubeconfig_credentials() -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(alien_core::KubernetesClientConfig::Kubeconfig {
            kubeconfig_path: None,
            context: Some("ambient".to_string()),
            cluster: None,
            user: None,
            namespace: None,
            additional_headers: None,
        }))
    }

    /// The named kubeconfig and context win over the ambient ones on every Kubernetes destroy.
    #[test]
    fn kube_flags_steer_a_kubeconfig_destroy() {
        let (ClientConfig::Kubernetes(config), refusal) = with_kubernetes_access(
            kubeconfig_credentials(),
            Some("/tmp/kc"),
            Some("install"),
            false,
        ) else {
            panic!("kubernetes")
        };
        assert!(refusal.is_none());
        let alien_core::KubernetesClientConfig::Kubeconfig {
            kubeconfig_path,
            context,
            ..
        } = *config
        else {
            panic!("kubeconfig")
        };
        assert_eq!(kubeconfig_path.as_deref(), Some("/tmp/kc"));
        assert_eq!(context.as_deref(), Some("install"));
    }

    /// With `--operator-removed`, credentials the flags cannot steer carry a refusal when a flag
    /// is given (raised only if a cluster call follows). Without it, nothing is refused: the
    /// flags (an exported `KUBECONFIG`, say) are ignored for those credentials, as before.
    #[test]
    fn kube_flags_are_refused_only_with_the_flag_for_credentials_they_cannot_steer() {
        let in_cluster =
            ClientConfig::Kubernetes(Box::new(alien_core::KubernetesClientConfig::InCluster {
                namespace: None,
                additional_headers: None,
            }));
        for (kubeconfig, context) in [(Some("/tmp/kc"), None), (None, Some("install"))] {
            assert!(
                with_kubernetes_access(in_cluster.clone(), kubeconfig, context, true)
                    .1
                    .is_some()
            );
            assert!(
                with_kubernetes_access(in_cluster.clone(), kubeconfig, context, false)
                    .1
                    .is_none()
            );
        }
        assert!(with_kubernetes_access(in_cluster, None, None, true)
            .1
            .is_none());
    }

    #[tokio::test]
    async fn retained_local_deployment_can_reload_original_authority_for_later_erase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deployments.json");
        let local = TrackedLocalDeployment {
            data_dir: dir.path().join("durable-data").display().to_string(),
            service_managed: false,
        };
        let mut tracker = DeploymentTracker::with_path(path.clone()).unwrap();
        tracker
            .track(
                "uploads".into(),
                "dep_uploads".into(),
                Some("original-token".into()),
                "https://manager.example.com".into(),
                "local".into(),
                None,
                Some(local.clone()),
            )
            .unwrap();
        finish_tracked_deletion(Some(&mut tracker), Some("uploads"), false).unwrap();
        drop(tracker);

        // A subsequent CLI process must recover the same identity, credentials,
        // and data directory before it can reopen retained deletion as erase.
        let mut recovered = DeploymentTracker::with_path(path.clone()).unwrap();
        let tracked = recovered.get("uploads").expect("retained recovery entry");
        assert_eq!(tracked.deployment_id, "dep_uploads");
        assert_eq!(tracked.token.as_deref(), Some("original-token"));
        assert_eq!(tracked.manager_url, "https://manager.example.com");
        assert_eq!(
            destroy_client_config(Platform::Local, tracked.local.as_ref())
                .await
                .unwrap(),
            ClientConfig::Local {
                state_directory: local.data_dir
            }
        );
        run_setup_owned_deletion("retained", true, || async { Ok(true) }, || async { Ok(()) })
            .await
            .unwrap();
        finish_tracked_deletion(Some(&mut recovered), Some("uploads"), true).unwrap();
        assert!(DeploymentTracker::with_path(path)
            .unwrap()
            .get("uploads")
            .is_none());
    }

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

    /// A database released by an earlier attempt (a retain, or an erase that failed on another
    /// resource) is still the site's, so a later erase must keep saying it wasn't erased.
    #[test]
    fn released_bindings_stay_named_after_an_earlier_attempt() {
        let postgres = |status: &str| {
            serde_json::json!({
                "type": "postgres",
                "status": status,
                "config": {"type": "postgres", "id": "database", "version": "17", "storage": "20Gi", "highAvailability": false},
                "lifecycle": "frozen",
                "externalBinding": true
            })
        };
        let deployment = |database: serde_json::Value| {
            serde_json::json!({
                "stackState": {
                    "platform": "kubernetes",
                    "resourcePrefix": "site",
                    "resources": {
                        "database": database,
                        "uploads": {
                            "type": "storage",
                            "status": "delete-failed",
                            "config": {"type": "storage", "id": "uploads"},
                            "lifecycle": "frozen"
                        }
                    }
                }
            })
        };

        for status in ["running", "deleted"] {
            assert_eq!(
                site_bound_resource_ids(&deployment(postgres(status))).unwrap(),
                vec!["database".to_string()],
                "a {status} binding"
            );
        }

        let mut unbound = postgres("running");
        unbound.as_object_mut().unwrap().remove("externalBinding");
        assert!(site_bound_resource_ids(&deployment(unbound))
            .unwrap()
            .is_empty());
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
            false,
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
            "retention-required",
            false,
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

    /// Running the command again after a run that did not finish (refused on the wrong cluster,
    /// timed out, Operator removed) goes straight on: the manager refuses a second delete request.
    #[tokio::test]
    async fn a_deletion_already_requested_is_not_requested_again() {
        for status in ["delete-pending", "deleting"] {
            // With and without --erase-data: the manager refuses a second request in these
            // statuses whatever its policy.
            for erase in [false, true] {
                let actions = std::sync::Mutex::new(Vec::new());
                run_setup_owned_deletion(
                    status,
                    erase,
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
                .expect("an earlier run already requested the deletion");
                assert_eq!(
                    *actions.lock().unwrap(),
                    ["acquire"],
                    "{status}, erase {erase}"
                );
            }
        }
    }

    /// The policy of the first request applies to the whole deletion: a rerun whose flag differs
    /// does nothing and names the command that continues it.
    #[test]
    fn a_rerun_with_another_erase_flag_is_refused_naming_the_command_to_continue() {
        use alien_deployment::manager_api_transport::recorded_deletion_policy;
        let recorded =
            |policy: &str| serde_json::json!({"runtimeMetadata": {"deletionPolicy": policy}});
        assert_eq!(recorded_deletion_policy(&recorded("erase")), Some(true));
        assert_eq!(recorded_deletion_policy(&recorded("retain")), Some(false));
        assert_eq!(recorded_deletion_policy(&serde_json::json!({})), None);

        for status in ["delete-pending", "deleting", "teardown-required"] {
            // Same choice: goes on.
            refuse_a_changed_policy(status, true, Some(true)).expect(status);
            refuse_a_changed_policy(status, false, Some(false)).expect(status);
            // Recorded erase, rerun without the flag: refused, names --erase-data.
            let refused = refuse_a_changed_policy(status, false, Some(true)).expect_err(status);
            let text = refused.message.clone();
            assert!(
                text.contains("requested with data erase"),
                "{status}: {text}"
            );
            assert!(
                text.contains("Run destroy again with --erase-data to continue"),
                "{status}: {text}"
            );
            // Recorded retain, rerun with the flag: refused, names the command without it.
            let refused = refuse_a_changed_policy(status, true, Some(false)).expect_err(status);
            let text = refused.message.clone();
            assert!(
                text.contains("requested with data retained"),
                "{status}: {text}"
            );
            assert!(
                text.contains("Run destroy again without --erase-data to finish it"),
                "{status}: {text}"
            );
        }
        // A retained deployment asked to erase is a new request, not a changed choice; a
        // failed teardown is re-armed by a request.
        for status in ["retention-required", "retained", "teardown-failed"] {
            refuse_a_changed_policy(status, true, Some(false)).expect(status);
        }
        // A record without a policy is never refused: the teardown reads it as retain.
        for status in ["delete-pending", "deleting", "teardown-required"] {
            refuse_a_changed_policy(status, true, None).expect(status);
            refuse_a_changed_policy(status, false, None).expect(status);
        }
        assert!(!deletion_already_requested("retention-required", true));
    }

    fn deployment_json(status: &str, policy: Option<&str>) -> serde_json::Value {
        let mut record = serde_json::json!({
            "id": "dep_1",
            "name": "example",
            "status": status,
            "platform": "aws",
            "deploymentGroupId": "dg_1",
            "deploymentProtocolVersion": 1,
            "projectId": "prj_1",
            "workspaceId": "ws_1",
            "retryRequested": false,
            "createdAt": "2026-07-16T00:00:00Z"
        });
        if let Some(policy) = policy {
            record["runtimeMetadata"] = serde_json::json!({"deletionPolicy": policy});
        }
        record
    }

    /// Through the command itself: a rerun whose flag differs from the recorded policy sends no
    /// delete request and takes no lock. Without the refusal the command would go on to the
    /// acquire (and erase or retain against the recorded choice).
    #[tokio::test]
    async fn a_rerun_with_another_erase_flag_sends_no_request_and_takes_no_lock() {
        use httpmock::prelude::*;
        for (status, policy, erase_flag, names) in [
            (
                "delete-pending",
                "erase",
                false,
                "requested with data erase",
            ),
            ("deleting", "retain", true, "requested with data retained"),
            (
                "teardown-required",
                "erase",
                false,
                "requested with data erase",
            ),
            (
                "teardown-required",
                "retain",
                true,
                "requested with data retained",
            ),
        ] {
            let server = MockServer::start_async().await;
            let read = server
                .mock_async(|when, then| {
                    when.method(GET).path("/v1/deployments/dep_1");
                    then.status(200)
                        .json_body(deployment_json(status, Some(policy)));
                })
                .await;
            let request = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/deployments/dep_1/delete");
                    then.status(200)
                        .json_body(serde_json::json!({"cleanupRequired": true}));
                })
                .await;
            let acquire = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/acquire");
                    then.status(200)
                        .json_body(serde_json::json!({"deployments": [], "notAcquired": []}));
                })
                .await;
            let mut argv = vec![
                "destroy".to_string(),
                "--deployment-id".into(),
                "dep_1".into(),
                "--token".into(),
                "ax_test".into(),
                "--manager-url".into(),
                server.base_url(),
                "--yes".into(),
            ];
            if erase_flag {
                argv.push("--erase-data".into());
            }
            let args = DownArgs::try_parse_from(argv).expect("arguments");
            let error = down_command_with(args, None, &InClusterUse::default())
                .await
                .expect_err("a changed policy is refused");
            assert!(
                error.message.contains(names),
                "{status}/{policy}: {}",
                error.message
            );
            read.assert_hits_async(1).await;
            request.assert_hits_async(0).await;
            acquire.assert_hits_async(0).await;
        }
    }

    fn record_for(platform: &str) -> serde_json::Value {
        let mut record = deployment_json("running", None);
        record["platform"] = serde_json::json!(platform);
        record
    }

    /// Mounts the deployment read and counts every request that could change it.
    async fn manager_that_counts_deletes<'a>(
        server: &'a httpmock::MockServer,
        platform: &str,
    ) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
        use httpmock::prelude::*;
        let record = record_for(platform);
        let read = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/dep_1");
                then.status(200).json_body(record);
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/deployments/dep_1/delete");
                then.status(200)
                    .json_body(serde_json::json!({"cleanupRequired": true}));
            })
            .await;
        (read, delete)
    }

    fn assert_refused_as_kubernetes_only(error: &alien_error::AlienError<ErrorData>) {
        assert!(
            error.message.contains("Kubernetes installs only"),
            "{}",
            error.message
        );
    }

    /// Through the command itself: `--operator-removed` on a platform other than Kubernetes is
    /// refused before the delete request, so a refused run leaves the deployment record as it
    /// was (a rerun is not held to the refused run's `--erase-data`).
    #[tokio::test]
    async fn operator_removed_outside_kubernetes_sends_no_delete_request() {
        for (platform, erase) in [
            ("aws", false),
            ("aws", true),
            ("gcp", true),
            ("azure", true),
            ("machines", false),
            ("machines", true),
            ("local", false),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let (read, delete) = manager_that_counts_deletes(&server, platform).await;
            let mut argv = vec![
                "destroy".to_string(),
                "--deployment-id".into(),
                "dep_1".into(),
                "--token".into(),
                "ax_test".into(),
                "--manager-url".into(),
                server.base_url(),
                "--yes".into(),
                "--operator-removed".into(),
            ];
            if erase {
                argv.push("--erase-data".into());
            }
            let args = DownArgs::try_parse_from(argv).expect("arguments");
            let error = down_command_with(args, None, &InClusterUse::default())
                .await
                .expect_err("refused");
            assert_eq!(
                delete.hits_async().await,
                0,
                "{platform} erase={erase}: a delete request was sent before the refusal"
            );
            assert_refused_as_kubernetes_only(&error);
            read.assert_hits_async(1).await;
        }
    }

    /// A tracked, service-managed local install: the refusal comes before the local Operator
    /// service is stopped (which comes after the delete request), and before any request.
    #[tokio::test]
    async fn operator_removed_on_a_tracked_local_install_stops_nothing_and_sends_nothing() {
        static CONFIG_HOME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CONFIG_HOME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("alien");
        std::fs::create_dir_all(&config_dir).unwrap();
        let server = httpmock::MockServer::start_async().await;
        let (read, delete) = manager_that_counts_deletes(&server, "local").await;
        let mut tracker =
            DeploymentTracker::with_path(config_dir.join("deployments.json")).unwrap();
        tracker
            .track(
                "uploads".into(),
                "dep_1".into(),
                Some("ax_test".into()),
                server.base_url(),
                "local".into(),
                None,
                Some(TrackedLocalDeployment {
                    data_dir: dir.path().join("data").display().to_string(),
                    service_managed: true,
                }),
            )
            .unwrap();
        drop(tracker);
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        let args = DownArgs::try_parse_from([
            "destroy",
            "--name",
            "uploads",
            "--yes",
            "--operator-removed",
        ])
        .expect("arguments");
        let result = down_command_with(args, None, &InClusterUse::default()).await;
        match previous {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        assert_eq!(
            delete.hits_async().await,
            0,
            "a delete request was sent before the refusal"
        );
        assert_refused_as_kubernetes_only(&result.expect_err("refused"));
        read.assert_hits_async(1).await;
        // The tracked entry is untouched: nothing was started, stopped or forgotten.
        assert!(
            DeploymentTracker::with_path(config_dir.join("deployments.json"))
                .unwrap()
                .get("uploads")
                .is_some()
        );
    }

    /// The Machines path asks the same question as the setup-owned path (one rule, no copy).
    #[test]
    fn a_deletion_that_needs_no_new_request_is_decided_in_one_place() {
        for status in ["delete-pending", "deleting"] {
            assert!(needs_no_new_request(status, false), "{status}");
            assert!(needs_no_new_request(status, true), "{status}");
        }
        for status in ["teardown-required", "retention-required"] {
            assert!(needs_no_new_request(status, false), "{status}");
            assert!(!needs_no_new_request(status, true), "{status}");
        }
        assert!(needs_no_new_request("retained", false));
        assert!(!needs_no_new_request("retained", true));
        assert!(!needs_no_new_request("running", false));
        assert!(!needs_no_new_request("teardown-failed", false));
    }

    #[tokio::test]
    async fn explicit_erase_reopens_a_retained_deployment() {
        let actions = std::sync::Mutex::new(Vec::new());
        run_setup_owned_deletion(
            "retained",
            true,
            || async {
                actions.lock().unwrap().push("request-erase");
                Ok(true)
            },
            || async {
                actions.lock().unwrap().push("acquire");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*actions.lock().unwrap(), ["request-erase", "acquire"]);
    }

    #[tokio::test]
    async fn completed_manager_cleanup_skips_setup_teardown() {
        let actions = std::sync::Mutex::new(Vec::new());

        run_setup_owned_deletion(
            "preflights-failed",
            false,
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
