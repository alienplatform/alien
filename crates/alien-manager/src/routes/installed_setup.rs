//! A Helm install reports the setup its chart carries on every sync.
//!
//! The report is an observation, never a grant. The manager decides, from its
//! own record of the deployment and the release the chart names, whether the
//! installed chart completes a pending setup update. When it does, the manager
//! records the same one-shot [`SetupUpdateAuthorization`] a setup re-import
//! records, and the deployment's ordinary update applies it.
//!
//! A refusal never fails the sync: nothing is authorized, the deployment stays
//! paused, and the reason is kept on the deployment for its owner to read.

use alien_core::sync::InstalledSetup;
use alien_core::{
    Platform, ResourceLifecycle, SetupReportRefusal, SetupUpdateAuthorization, Stack,
};
use alien_error::AlienError;
use tracing::{info, warn};

use super::stack::{
    can_reconcile_after_import, prepare_setup_stack, resolve_stack, PrepareSetupStack,
};
use super::AppState;
use crate::auth::{Scope, Subject};
use crate::traits::deployment_store::{DeploymentRecord, UpdateImportedDeploymentParams};

/// Whether a `helm upgrade` carries a frozen resource of this type: its
/// controller only observes a Kubernetes object the chart renders. A compute
/// cluster always does. A service account does only without a base cloud; with
/// one, its controller creates a cloud identity, which needs cloud credentials.
fn chart_renders(resource_type: &str, base_platform: Option<Platform>) -> bool {
    match resource_type {
        "compute-cluster" => true,
        "service-account" => base_platform.is_none(),
        _ => false,
    }
}

/// What the manager does with one report.
#[derive(Debug, PartialEq)]
pub(super) enum ReportDecision {
    /// Nothing to do: the chart carries what the deployment already runs, or
    /// the deployment cannot take an authorization right now.
    NoOp,
    /// The chart cannot complete the update; the reason is shown to the owner.
    Refuse(String),
    /// The chart completes the update.
    Mint(SetupUpdateAuthorization),
}

/// Whether `subject` is the Operator of `deployment`. The only place that
/// decides who may send a setup report, so a narrower Operator credential can
/// replace the deployment token here without touching the rest.
pub(super) fn is_operator_of(subject: &Subject, deployment: &DeploymentRecord) -> bool {
    matches!(
        &subject.scope,
        Scope::Deployment { deployment_id, .. } if *deployment_id == deployment.id
    )
}

/// Decides what a report means for `deployment`.
///
/// `target` is the stack the reported release prepares to for this
/// deployment, with its recorded gate answers applied.
pub(super) fn decide(
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
    target: &Stack,
) -> ReportDecision {
    let Some(metadata) = deployment.runtime_metadata.as_ref() else {
        return ReportDecision::NoOp;
    };
    let Some(baseline) = metadata.prepared_stack.as_ref() else {
        return ReportDecision::NoOp;
    };

    // The chart carries what the deployment already runs. Whatever the
    // deployment's status is, there is nothing for a report to change.
    let baseline_digest = baseline.setup_update_digest();
    let target_digest = target.setup_update_digest();
    if baseline_digest == target_digest {
        return ReportDecision::NoOp;
    }

    // Only the release the deployment is waiting for can complete its update.
    if let Some(waiting) = deployment.desired_release_id.as_deref() {
        if waiting != report.release_id {
            return ReportDecision::Refuse(format!(
                "This chart is for release {}; the deployment is waiting for release {waiting}. Upgrade to the chart for release {waiting}.",
                report.release_id
            ));
        }
    }

    // The first report on an existing Helm deployment records its lane; later
    // reports must stay in it.
    if let (Some(target_lane), Some(version)) = (
        deployment.setup_target.as_deref(),
        deployment.setup_fingerprint_version,
    ) {
        if target_lane != report.setup_target || version != report.setup_fingerprint_version {
            return ReportDecision::Refuse(
                "This chart was built for a different setup than the one installed. Upgrade with a chart from the same install channel."
                    .to_string(),
            );
        }
    }

    if let Some(reason) = changes_needing_cloud_setup(baseline, target, deployment.base_platform) {
        return ReportDecision::Refuse(format!(
            "{reason}. A Helm upgrade cannot apply this; run your cloud setup, then upgrade."
        ));
    }

    ReportDecision::Mint(SetupUpdateAuthorization {
        nonce: uuid::Uuid::new_v4().to_string(),
        baseline_frozen_digest: baseline_digest,
        target_frozen_digest: target_digest,
        release_id: report.release_id.clone(),
        setup_target: report.setup_target.clone(),
        setup_fingerprint: report.setup_fingerprint.clone(),
        setup_fingerprint_version: report.setup_fingerprint_version,
    })
}

/// The first change a Helm upgrade cannot carry, if any.
///
/// A Helm upgrade carries Kubernetes objects the chart renders. A change to a
/// resource that lives in the customer's cloud, to what the management
/// identity may do, or to the permissions of workloads that reach cloud
/// services, needs the cloud setup.
fn changes_needing_cloud_setup(
    baseline: &Stack,
    target: &Stack,
    base_platform: Option<Platform>,
) -> Option<String> {
    for (id, entry) in baseline.resources().chain(target.resources()) {
        if entry.lifecycle != ResourceLifecycle::Frozen {
            continue;
        }
        let installed = baseline.resources.get(id);
        let wanted = target.resources.get(id);
        let unchanged = match (installed, wanted) {
            (Some(a), Some(b)) => a.config == b.config && a.lifecycle == b.lifecycle,
            _ => false,
        };
        if unchanged {
            continue;
        }
        if !chart_renders(entry.config.resource_type().as_ref(), base_platform) {
            return Some(format!(
                "This release adds or changes '{id}', which lives in your cloud account"
            ));
        }
    }
    if baseline.permissions.management != target.permissions.management {
        return Some("This release changes what the management identity may do".to_string());
    }
    if base_platform.is_some() && baseline.permissions.profiles != target.permissions.profiles {
        return Some("This release changes cloud permissions".to_string());
    }
    None
}

/// Handles a report from the Operator of `deployment`. Returns nothing: a
/// report that cannot be acted on never fails the sync that carried it.
pub(super) async fn handle_installed_setup(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
) {
    if !is_operator_of(subject, deployment) {
        warn!(deployment_id = %deployment.id, "Ignoring a setup report from another subject");
        return;
    }
    if let Err(error) = apply_report(state, subject, deployment, report).await {
        warn!(deployment_id = %deployment.id, error = %error, "Could not process the installed setup report");
    }
}

async fn apply_report(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
) -> Result<(), AlienError> {
    // Only Helm installs report. Anything else on this field is ignored.
    if deployment.platform != Platform::Kubernetes {
        return Ok(());
    }
    let (Some(settings), Some(stack_state), Some(metadata)) = (
        deployment.stack_settings.as_ref(),
        deployment.stack_state.as_ref(),
        deployment.runtime_metadata.as_ref(),
    ) else {
        return Ok(());
    };

    let release = state
        .release_store
        .get_release(subject, &report.release_id)
        .await?;
    let Some(release) = release else {
        return record_refusal(
            state,
            subject,
            deployment,
            report,
            format!(
                "Release {} is unknown. Upgrade to the chart version the deployment is waiting for.",
                report.release_id
            ),
        )
        .await;
    };

    let target = match prepare_target(&release, deployment, settings, stack_state).await {
        Ok(target) => target,
        Err(error) => {
            return record_refusal(state, subject, deployment, report, error.to_string()).await
        }
    };

    match decide(deployment, report, &target) {
        ReportDecision::NoOp => {
            // The chart matches what runs; a stale refusal no longer applies.
            if metadata.setup_report_refusal.is_some() {
                let mut metadata = metadata.clone();
                metadata.setup_report_refusal = None;
                write(state, subject, deployment, report, metadata, false).await?;
            }
            Ok(())
        }
        ReportDecision::Refuse(reason) => {
            record_refusal(state, subject, deployment, report, reason).await
        }
        ReportDecision::Mint(authorization) => {
            let already = metadata
                .setup_update_authorization
                .as_ref()
                .is_some_and(|existing| {
                    existing.release_id == authorization.release_id
                        && existing.baseline_frozen_digest == authorization.baseline_frozen_digest
                        && existing.target_frozen_digest == authorization.target_frozen_digest
                        && existing.setup_fingerprint == authorization.setup_fingerprint
                });
            // An update in flight finishes first; the report repeats every sync.
            if already || !can_reconcile_after_import(deployment) {
                return Ok(());
            }
            info!(deployment_id = %deployment.id, release_id = %report.release_id, "Chart completes the setup update");
            let mut metadata = metadata.clone();
            metadata.setup_update_authorization = Some(authorization);
            metadata.setup_report_refusal = None;
            write(state, subject, deployment, report, metadata, true).await
        }
    }
}

/// The stack the reported release installs for this deployment: the same
/// preparation a setup re-import does, with the deployment's recorded gate
/// answers.
async fn prepare_target(
    release: &crate::traits::ReleaseRecord,
    deployment: &DeploymentRecord,
    settings: &alien_core::StackSettings,
    stack_state: &alien_core::StackState,
) -> Result<Stack, AlienError> {
    let source = resolve_stack(release, deployment.platform).map_err(|e| e.into_generic())?;
    let persisted = deployment
        .runtime_metadata
        .as_ref()
        .map(|metadata| metadata.persisted_gate_answers.clone())
        .unwrap_or_default();
    let frozen_gating = alien_deployment::frozen_gating_inputs(source, deployment.platform);
    let existing_resource_ids = stack_state
        .resources
        .iter()
        .filter(|(_, resource)| resource.status != alien_core::ResourceStatus::Deleted)
        .map(|(id, _)| id.clone())
        .collect();
    // The report carries no inputs, so answers are what the deployment holds:
    // a frozen gate is answered once, and a stored value that contradicts its
    // answer is refused, as on CloudFormation.
    let mut answers = persisted;
    answers.extend(
        alien_deployment::enforce_frozen_gate_fixity(alien_deployment::FrozenGateFixityCheck {
            persisted: &answers,
            stack: source,
            still_frozen_gating: &frozen_gating,
            existing_resource_ids: &existing_resource_ids,
            input_values: &deployment.input_values,
        })
        .map_err(|e| e.into_generic())?,
    );

    let mut settings = settings.clone();
    let stripped = alien_deployment::strip_frozen_declines_with_compute_settings(
        source.clone(),
        &answers,
        &frozen_gating,
        settings.compute.as_mut(),
        deployment.platform,
    );
    let prepared = prepare_setup_stack(
        stripped,
        &PrepareSetupStack {
            platform: deployment.platform,
            base_platform: deployment.base_platform,
            deployment_name: &deployment.name,
            management_config: deployment.management_config.as_ref(),
            imported_quota_activation: false,
        },
        &settings,
    )
    .await
    .map_err(|e| e.into_generic())?;
    alien_deployment::strip_declined_live_resources(
        prepared,
        &deployment.input_values,
        &answers,
        &frozen_gating,
        deployment.platform,
    )
    .map_err(|e| e.into_generic())
}

/// Keeps the refusal on the deployment, once per distinct reason.
async fn record_refusal(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
    reason: String,
) -> Result<(), AlienError> {
    let Some(metadata) = deployment.runtime_metadata.as_ref() else {
        return Ok(());
    };
    if metadata
        .setup_report_refusal
        .as_ref()
        .is_some_and(|refusal| {
            refusal.reason == reason && refusal.reported_release_id == report.release_id
        })
    {
        return Ok(());
    }
    warn!(deployment_id = %deployment.id, %reason, "Declined the installed setup report");
    let mut metadata = metadata.clone();
    metadata.setup_report_refusal = Some(SetupReportRefusal {
        reason,
        reported_release_id: report.release_id.clone(),
        at: chrono::Utc::now(),
    });
    write(state, subject, deployment, report, metadata, false).await
}

/// The store update a re-import makes, with the deployment's own state kept.
/// The lane is recorded here, from the first report that reaches a write.
async fn write(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
    runtime_metadata: alien_core::RuntimeMetadata,
    schedule_reconciliation: bool,
) -> Result<(), AlienError> {
    // The store update writes the whole snapshot, and release lookup and
    // preparation took time. If the record moved since it was read (a
    // checkpoint, a settings change), writing now would put old state over it.
    // Skip: the report repeats on the next sync.
    let Some(fresh) = state
        .deployment_store
        .get_deployment(subject, &deployment.id)
        .await?
    else {
        return Ok(());
    };
    if fresh.updated_at != deployment.updated_at || fresh.status != deployment.status {
        info!(deployment_id = %deployment.id, "Deployment changed while handling the setup report; the next report retries");
        return Ok(());
    }
    let (Some(stack_settings), Some(stack_state)) = (
        deployment.stack_settings.clone(),
        deployment.stack_state.clone(),
    ) else {
        return Ok(());
    };
    let lane_known = deployment.setup_target.is_some();
    state
        .deployment_store
        .update_imported_stack_state(
            subject,
            &deployment.id,
            UpdateImportedDeploymentParams {
                stack_settings,
                stack_state,
                environment_info: deployment.environment_info.clone(),
                runtime_metadata,
                setup_metadata: deployment.setup_metadata.clone(),
                current_release_id: None,
                setup_target: if lane_known {
                    deployment.setup_target.clone().unwrap_or_default()
                } else {
                    report.setup_target.clone()
                },
                setup_fingerprint: if schedule_reconciliation || !lane_known {
                    report.setup_fingerprint.clone()
                } else {
                    deployment.setup_fingerprint.clone().unwrap_or_default()
                },
                setup_fingerprint_version: if lane_known {
                    deployment.setup_fingerprint_version.unwrap_or_default()
                } else {
                    report.setup_fingerprint_version
                },
                activation_status: None,
                schedule_reconciliation,
                input_values: deployment.input_values.clone(),
            },
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{RuntimeMetadata, ServiceAccount, StackState, Storage};
    use chrono::Utc;

    fn baseline() -> Stack {
        Stack::new("app".to_string())
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    fn with_service_account(stack: &Stack) -> Stack {
        let mut next = stack.clone();
        next.resources.extend(
            Stack::new("extra".to_string())
                .add(
                    ServiceAccount::new("reader-sa".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
                .resources,
        );
        next
    }

    fn with_storage(stack: &Stack) -> Stack {
        let mut next = stack.clone();
        next.resources.extend(
            Stack::new("extra".to_string())
                .add(
                    Storage::new("archive".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
                .resources,
        );
        next
    }

    fn deployment(prepared: Stack) -> DeploymentRecord {
        DeploymentRecord {
            supplied_stacks: None,
            id: "dep_1".to_string(),
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            name: "acme".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Kubernetes,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "update-failed".to_string(),
            stack_settings: Some(Default::default()),
            stack_state: Some(StackState::new(Platform::Kubernetes)),
            environment_info: None,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(prepared),
                ..RuntimeMetadata::default()
            }),
            current_release_id: Some("rel_1".to_string()),
            desired_release_id: Some("rel_2".to_string()),
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn report(release_id: &str) -> InstalledSetup {
        InstalledSetup {
            release_id: release_id.to_string(),
            setup_target: "helm".to_string(),
            setup_fingerprint: "fp_2".to_string(),
            setup_fingerprint_version: 1,
        }
    }

    #[test]
    fn a_chart_that_adds_a_chart_rendered_resource_mints_for_exactly_that_change() {
        let installed = baseline();
        let target = with_service_account(&installed);
        let decision = decide(&deployment(installed.clone()), &report("rel_2"), &target);

        let ReportDecision::Mint(authorization) = decision else {
            panic!("expected a mint, got {decision:?}");
        };
        assert_eq!(
            authorization.baseline_frozen_digest,
            installed.setup_update_digest()
        );
        assert_eq!(
            authorization.target_frozen_digest,
            target.setup_update_digest()
        );
        assert_eq!(authorization.release_id, "rel_2");
        assert_eq!(authorization.setup_fingerprint, "fp_2");
    }

    #[test]
    fn a_chart_carrying_what_already_runs_does_nothing_whatever_the_status() {
        let installed = baseline();
        for status in ["running", "updating", "update-failed", "update-pending"] {
            let mut record = deployment(installed.clone());
            record.status = status.to_string();
            assert_eq!(
                decide(&record, &report("rel_2"), &installed),
                ReportDecision::NoOp,
                "status {status}"
            );
        }
    }

    #[test]
    fn a_chart_for_an_older_release_is_refused_with_the_release_to_use() {
        let installed = baseline();
        let target = with_service_account(&installed);
        let ReportDecision::Refuse(reason) =
            decide(&deployment(installed), &report("rel_0"), &target)
        else {
            panic!("expected a refusal");
        };
        assert!(
            reason.contains("rel_0") && reason.contains("rel_2"),
            "{reason}"
        );
    }

    #[test]
    fn a_change_that_lives_in_the_cloud_is_refused_with_the_cloud_setup_message() {
        let installed = baseline();
        let target = with_storage(&installed);
        let ReportDecision::Refuse(reason) =
            decide(&deployment(installed), &report("rel_2"), &target)
        else {
            panic!("expected a refusal");
        };
        assert!(reason.contains("archive"), "{reason}");
        assert!(
            reason.contains("run your cloud setup, then upgrade"),
            "{reason}"
        );
    }

    #[test]
    fn a_service_account_on_a_base_cloud_is_refused_because_it_needs_a_cloud_identity() {
        let installed = baseline();
        let target = with_service_account(&installed);
        let mut on_cloud = deployment(installed);
        on_cloud.base_platform = Some(Platform::Aws);
        let ReportDecision::Refuse(reason) = decide(&on_cloud, &report("rel_2"), &target) else {
            panic!("expected a refusal");
        };
        assert!(reason.contains("reader-sa"), "{reason}");
        assert!(reason.contains("run your cloud setup"), "{reason}");
    }

    #[test]
    fn a_report_in_another_setup_lane_is_refused_and_a_matching_lane_mints() {
        let installed = baseline();
        let target = with_service_account(&installed);
        let mut record = deployment(installed);
        record.setup_target = Some("cloudformation".to_string());
        record.setup_fingerprint_version = Some(1);
        assert!(matches!(
            decide(&record, &report("rel_2"), &target),
            ReportDecision::Refuse(_)
        ));
        record.setup_target = Some("helm".to_string());
        assert!(matches!(
            decide(&record, &report("rel_2"), &target),
            ReportDecision::Mint(_)
        ));
    }

    #[test]
    fn only_the_deployments_own_operator_may_report() {
        let record = deployment(baseline());
        let own = Subject {
            scope: Scope::Deployment {
                project_id: "project".to_string(),
                deployment_id: "dep_1".to_string(),
            },
            ..Subject::system()
        };
        let other = Subject {
            scope: Scope::Deployment {
                project_id: "project".to_string(),
                deployment_id: "dep_2".to_string(),
            },
            ..Subject::system()
        };
        assert!(is_operator_of(&own, &record));
        assert!(!is_operator_of(&other, &record));
        assert!(!is_operator_of(&Subject::system(), &record));
    }
}
