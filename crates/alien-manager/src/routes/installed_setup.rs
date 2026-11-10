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
use alien_error::{AlienError, Context};
use tracing::{info, warn};

use super::stack::{
    can_reconcile_after_import, prepare_setup_stack, resolve_stack, PrepareSetupStack,
};
use super::sync::HelmRelease;
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
    // With no release waiting there is no update to authorize: a chart that
    // differs then is an older chart (after a rollback) or a newer one the
    // deployment has not been pointed at yet.
    let Some(waiting) = deployment.desired_release_id.as_deref() else {
        return ReportDecision::NoOp;
    };
    if waiting != report.release_id {
        return ReportDecision::Refuse(format!(
            "This chart is for release {}; the deployment is waiting for release {waiting}. Upgrade to the chart for release {waiting}.",
            report.release_id
        ));
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

/// Handles a report from the Operator of `deployment`. A report that cannot be
/// acted on never fails the sync that carried it. Returns whether the stored
/// deployment may differ from `deployment`, so the caller re-reads it only then.
pub(super) async fn handle_installed_setup(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
) -> bool {
    if !is_operator_of(subject, deployment) {
        warn!(deployment_id = %deployment.id, "Ignoring a setup report from another subject");
        return false;
    }
    match apply_report(state, subject, deployment, report).await {
        Ok(wrote) => wrote,
        Err(error) => {
            warn!(deployment_id = %deployment.id, error = %error, "Could not process the installed setup report");
            false
        }
    }
}

async fn apply_report(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
) -> Result<bool, AlienError> {
    // Only Helm installs report. Anything else on this field is ignored.
    if deployment.platform != Platform::Kubernetes {
        return Ok(false);
    }
    let (Some(settings), Some(stack_state), Some(metadata)) = (
        deployment.stack_settings.as_ref(),
        deployment.stack_state.as_ref(),
        deployment.runtime_metadata.as_ref(),
    ) else {
        return Ok(false);
    };

    // With no release waiting there is no update to authorize, so nothing in
    // the release is needed: an idle sync only records the Helm release name.
    let Some(waiting) = deployment.desired_release_id.as_deref() else {
        return record_match(state, subject, deployment, report, true).await;
    };

    // Whether a chart can carry the waiting release depends on the release,
    // not on which chart reported, so it is recorded from the waiting
    // release's own target. A waiting release that is gone or cannot be
    // prepared carries no verdict. A store error says nothing about the
    // release, so it keeps the stored verdict and fails this report; the
    // next sync repeats it. A change is written alone; the report repeats on
    // the next sync and is handled against the updated record.
    let waiting_target =
        target_of_release(state, subject, deployment, settings, stack_state, waiting).await?;
    let required = waiting_target
        .as_ref()
        .ok()
        .and_then(|waiting_target| {
            let baseline = metadata.prepared_stack.as_ref()?;
            changes_needing_cloud_setup(baseline, waiting_target, deployment.base_platform)
        })
        .map(|reason| alien_core::CloudSetupRequired {
            release_id: waiting.to_string(),
            reason,
        });
    if required != metadata.cloud_setup_required {
        let mut next = metadata.clone();
        next.cloud_setup_required = required;
        return write(state, subject, deployment, report, next, false).await;
    }

    let target = if report.release_id == waiting {
        waiting_target
    } else {
        target_of_release(
            state,
            subject,
            deployment,
            settings,
            stack_state,
            &report.release_id,
        )
        .await?
    };
    let target = match target {
        Ok(target) => target,
        Err(reason) => return record_refusal(state, subject, deployment, report, reason).await,
    };

    match decide(deployment, report, &target) {
        ReportDecision::NoOp => record_match(state, subject, deployment, report, false).await,
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
                return Ok(false);
            }
            info!(deployment_id = %deployment.id, release_id = %report.release_id, "Chart completes the setup update");
            let mut metadata = metadata.clone();
            metadata.setup_update_authorization = Some(authorization);
            metadata.setup_report_refusal = None;
            record_release(&mut metadata, report);
            write(state, subject, deployment, report, metadata, true).await
        }
    }
}

/// The stack `release_id` prepares to for this deployment, or the reason it
/// cannot be prepared. The outer error is a failure to read the release, which
/// says nothing about the release itself.
async fn target_of_release(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    settings: &alien_core::StackSettings,
    stack_state: &alien_core::StackState,
    release_id: &str,
) -> Result<Result<Stack, String>, AlienError> {
    let release = state
        .release_store
        .get_release(subject, release_id)
        .await
        .context(crate::error::ErrorData::InternalError {
            message: format!("Could not read release {release_id}"),
        })
        .map_err(|error| error.into_generic())?;
    let Some(release) = release else {
        return Ok(Err(format!(
            "Release {release_id} is unknown. Upgrade to the chart version the deployment is waiting for."
        )));
    };
    Ok(prepare_target(&release, deployment, settings, stack_state)
        .await
        .map_err(|error| error.to_string()))
}

/// The chart matches what runs, or no update waits: a stale refusal no longer
/// applies, and the Helm release the report names is recorded. With no release
/// waiting (`no_release_waits`) the cloud setup verdict no longer applies
/// either; otherwise it stays, as it belongs to the waiting release. Writes
/// only when something changed.
async fn record_match(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
    no_release_waits: bool,
) -> Result<bool, AlienError> {
    let Some(metadata) = deployment.runtime_metadata.as_ref() else {
        return Ok(false);
    };
    let mut next = metadata.clone();
    next.setup_report_refusal = None;
    if no_release_waits {
        next.cloud_setup_required = None;
    }
    let release_changed = record_release(&mut next, report);
    if release_changed
        || metadata.setup_report_refusal.is_some()
        || (no_release_waits && metadata.cloud_setup_required.is_some())
    {
        return write(state, subject, deployment, report, next, false).await;
    }
    Ok(false)
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
            // A deployment installed with the quota API keeps it, as a setup
            // re-import does; otherwise the rebuilt target would drop a
            // resource the baseline holds and read as a cloud change.
            imported_quota_activation: stack_state.resources.contains_key("enable-cloud-quotas"),
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

/// Records the Helm release the report names, so the exact upgrade command can
/// be shown. The names end up in a shell command the customer pastes, so a
/// value Helm and Kubernetes would not accept is dropped. Returns whether the
/// stored release changed.
fn record_release(metadata: &mut alien_core::RuntimeMetadata, report: &InstalledSetup) -> bool {
    let (Some(name), Some(namespace)) = (report.release_name.clone(), report.namespace.clone())
    else {
        return false;
    };
    if !(HelmRelease {
        name: name.clone(),
        namespace: namespace.clone(),
    })
    .is_valid()
    {
        return false;
    }
    let release = alien_core::InstalledHelmRelease { name, namespace };
    if metadata.installed_helm_release.as_ref() == Some(&release) {
        return false;
    }
    metadata.installed_helm_release = Some(release);
    true
}

/// Keeps the refusal on the deployment, once per distinct reason.
async fn record_refusal(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    report: &InstalledSetup,
    reason: String,
) -> Result<bool, AlienError> {
    let Some(metadata) = deployment.runtime_metadata.as_ref() else {
        return Ok(false);
    };
    let mut metadata = metadata.clone();
    let release_changed = record_release(&mut metadata, report);
    if !release_changed
        && metadata
            .setup_report_refusal
            .as_ref()
            .is_some_and(|refusal| {
                refusal.reason == reason && refusal.reported_release_id == report.release_id
            })
    {
        return Ok(false);
    }
    warn!(deployment_id = %deployment.id, %reason, "Declined the installed setup report");
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
) -> Result<bool, AlienError> {
    let (Some(stack_settings), Some(stack_state)) = (
        deployment.stack_settings.clone(),
        deployment.stack_state.clone(),
    ) else {
        return Ok(false);
    };
    let lane_known = deployment.setup_target.is_some();
    let result = state
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
                expected_updated_at: deployment.updated_at,
                input_values: deployment.input_values.clone(),
            },
        )
        .await;
    match result {
        Ok(_) => Ok(true),
        // A checkpoint or settings change landed after the record was read.
        // Nothing was written; the report repeats on the next sync.
        Err(error) if error.code == "DEPLOYMENT_CHANGED_CONCURRENTLY" => {
            info!(deployment_id = %deployment.id, "Deployment changed while handling the setup report; the next report retries");
            // Nothing was written, but the stored record is newer than the
            // caller's copy, so the caller must re-read it.
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::routes::charts::{self, ChartSettings};
    use crate::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
    use crate::traits::{
        CreateDeploymentGroupParams, CreateImportedDeploymentParams, CreateReleaseParams,
        DeploymentStore, ReleaseStore,
    };
    use alien_core::{RuntimeMetadata, ServiceAccount, StackState, Storage};
    use axum::{body::to_bytes, http::Method};
    use chrono::Utc;
    use std::{collections::BTreeMap, sync::Arc};

    pub(in crate::routes) fn baseline() -> Stack {
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

    pub(in crate::routes) fn deployment(prepared: Stack) -> DeploymentRecord {
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
            release_name: None,
            namespace: None,
        }
    }

    #[tokio::test]
    async fn a_gcp_deployment_installed_with_the_quota_api_keeps_it_in_the_target() {
        let source = baseline();
        let release = crate::traits::ReleaseRecord {
            id: "rel_2".to_string(),
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            stacks: std::collections::HashMap::from([(Platform::Kubernetes, source.clone())]),
            git_commit_sha: None,
            git_commit_ref: None,
            git_commit_message: None,
            created_at: Utc::now(),
        };
        let mut record = deployment(source.clone());
        record.base_platform = Some(Platform::Gcp);
        let settings = alien_core::StackSettings::default();

        let without = StackState::new(Platform::Kubernetes);
        let target = prepare_target(&release, &record, &settings, &without)
            .await
            .unwrap();
        assert!(!target.resources.contains_key("enable-cloud-quotas"));

        let mut installed = StackState::new(Platform::Kubernetes);
        installed.resources.insert(
            "enable-cloud-quotas".to_string(),
            alien_core::StackResourceState::new_pending(
                "service-activation".to_string(),
                alien_core::Resource::new(
                    alien_core::ServiceActivation::new("enable-cloud-quotas".to_string())
                        .service_name("cloudquotas.googleapis.com".to_string())
                        .build(),
                ),
                Some(ResourceLifecycle::Frozen),
                Vec::new(),
            ),
        );
        let target = prepare_target(&release, &record, &settings, &installed)
            .await
            .unwrap();
        assert!(target.resources.contains_key("enable-cloud-quotas"));
    }

    #[test]
    fn the_reported_release_is_recorded_only_when_helm_would_accept_it() {
        let named = |name: &str, namespace: &str| InstalledSetup {
            release_name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            ..report("rel_2")
        };
        let mut metadata = RuntimeMetadata::default();
        assert!(record_release(&mut metadata, &named("shop", "apps")));
        assert!(!record_release(&mut metadata, &named("shop", "apps")));
        // A name that would put shell syntax into the pasted command is dropped.
        assert!(!record_release(
            &mut metadata,
            &named("x; rm -rf ~", "apps")
        ));
        assert!(!record_release(&mut metadata, &named("shop", "$(id)")));
        assert!(!record_release(&mut metadata, &report("rel_2")));
        let release = metadata.installed_helm_release.unwrap();
        assert_eq!(
            (release.name.as_str(), release.namespace.as_str()),
            ("shop", "apps")
        );
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
    fn a_chart_that_differs_while_no_release_waits_mints_nothing() {
        let installed = baseline();
        let target = with_service_account(&installed);
        let mut record = deployment(installed);
        record.desired_release_id = None;
        record.status = "running".to_string();
        assert_eq!(
            decide(&record, &report("rel_1"), &target),
            ReportDecision::NoOp
        );
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

    /// An idle Helm deployment syncs every few seconds: with no release
    /// waiting and nothing new in the report, the manager makes no store call
    /// for the report. The mock store has no expectations, so any call fails.
    #[tokio::test]
    async fn an_idle_setup_report_makes_no_store_calls() {
        let temp = tempfile::tempdir().expect("test directory");
        let db = std::sync::Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(
                &temp.path().join("manager.db").to_string_lossy(),
            )
            .await
            .expect("database"),
        );
        let state = crate::routes::sync::tests::test_app_state(
            temp.path(),
            std::sync::Arc::new(crate::traits::deployment_store::MockDeploymentStore::new()),
            std::sync::Arc::new(crate::stores::sqlite::SqliteReleaseStore::new(db.clone())),
            std::sync::Arc::new(
                crate::providers::local_credentials::LocalCredentialResolver::new(
                    temp.path().to_path_buf(),
                ),
            ),
            db,
        )
        .await;
        let subject = Subject {
            scope: Scope::Deployment {
                project_id: "project".to_string(),
                deployment_id: "dep_idle".to_string(),
            },
            ..Subject::system()
        };
        let mut record = deployment(baseline());
        record.id = "dep_idle".to_string();
        record.desired_release_id = None;
        record.runtime_metadata = Some(alien_core::RuntimeMetadata {
            installed_helm_release: Some(alien_core::InstalledHelmRelease {
                name: "acme".to_string(),
                namespace: "apps".to_string(),
            }),
            ..Default::default()
        });
        let report = alien_core::sync::InstalledSetup {
            release_id: "rel_unknown".to_string(),
            setup_target: "helm".to_string(),
            setup_fingerprint: "fp".to_string(),
            setup_fingerprint_version: 1,
            release_name: Some("acme".to_string()),
            namespace: Some("apps".to_string()),
        };
        let wrote = handle_installed_setup(&state, &subject, &record, &report).await;
        assert!(!wrote);
    }

    struct Fixture {
        state: AppState,
        subject: Subject,
        record: DeploymentRecord,
        installed_release: String,
        waiting_release: String,
        written: std::sync::Arc<std::sync::Mutex<Vec<alien_core::RuntimeMetadata>>>,
        db: std::sync::Arc<crate::stores::sqlite::SqliteDatabase>,
        _temp: tempfile::TempDir,
    }

    /// A deployment on release R1 whose prepared stack matches R1, waiting for
    /// a release that adds a cloud resource. The store records every metadata
    /// write and then fails, so a test sees exactly what would be written.
    async fn waiting_fixture() -> Fixture {
        waiting_fixture_with(false).await
    }

    async fn waiting_fixture_with(conflict: bool) -> Fixture {
        use crate::traits::{CreateReleaseParams, ReleaseStore};
        use std::sync::{Arc, Mutex};

        let temp = tempfile::tempdir().expect("test directory");
        let db = Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(
                &temp.path().join("manager.db").to_string_lossy(),
            )
            .await
            .expect("database"),
        );
        let releases = Arc::new(crate::stores::sqlite::SqliteReleaseStore::new(db.clone()));
        let publish = |stack: Stack| {
            let releases = releases.clone();
            async move {
                releases
                    .create_release(
                        &Subject::system(),
                        CreateReleaseParams {
                            project_id: "project".to_string(),
                            stacks: std::collections::HashMap::from([(
                                Platform::Kubernetes,
                                stack,
                            )]),
                            git_commit_sha: None,
                            git_commit_ref: None,
                            git_commit_message: None,
                        },
                    )
                    .await
                    .expect("release")
                    .id
            }
        };
        let installed_release = publish(baseline()).await;
        let waiting_release = publish(with_storage(&baseline())).await;

        let mut record = deployment(baseline());
        record.desired_release_id = Some(waiting_release.clone());
        // The recorded baseline is what the installed release prepares to.
        let installed = releases
            .get_release(&Subject::system(), &installed_release)
            .await
            .expect("lookup")
            .expect("release");
        let prepared = prepare_target(
            &installed,
            &record,
            record.stack_settings.as_ref().expect("settings"),
            record.stack_state.as_ref().expect("stack state"),
        )
        .await
        .expect("prepared baseline");
        record
            .runtime_metadata
            .as_mut()
            .expect("metadata")
            .prepared_stack = Some(prepared);

        let written = Arc::new(Mutex::new(Vec::new()));
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        let sink = written.clone();
        store
            .expect_update_imported_stack_state()
            .returning(move |_, _, params| {
                sink.lock().expect("log").push(params.runtime_metadata);
                Err(if conflict {
                    AlienError::new(crate::error::ErrorData::DeploymentChangedConcurrently {
                        deployment_id: "dep_1".to_string(),
                    })
                    .into_generic()
                } else {
                    crate::error::ErrorData::internal("stop").into_generic()
                })
            });
        let state = crate::routes::sync::tests::test_app_state(
            temp.path(),
            Arc::new(store),
            releases,
            Arc::new(
                crate::providers::local_credentials::LocalCredentialResolver::new(
                    temp.path().to_path_buf(),
                ),
            ),
            db.clone(),
        )
        .await;
        let subject = Subject {
            scope: Scope::Deployment {
                project_id: "project".to_string(),
                deployment_id: "dep_1".to_string(),
            },
            ..Subject::system()
        };
        Fixture {
            state,
            subject,
            record,
            installed_release,
            waiting_release,
            written,
            db,
            _temp: temp,
        }
    }

    /// A stale chart reports while the waiting release adds a cloud resource:
    /// the manager records, from the waiting release itself, that a chart
    /// upgrade cannot carry it.
    #[tokio::test]
    async fn the_waiting_release_decides_whether_a_chart_can_carry_it() {
        let f = waiting_fixture().await;
        // The installed chart still reports the old release.
        let mut stale = report(&f.installed_release);
        stale.release_name = None;
        handle_installed_setup(&f.state, &f.subject, &f.record, &stale).await;

        let written = f.written.lock().expect("log");
        assert_eq!(written.len(), 1, "one write records the verdict");
        let required = written[0]
            .cloud_setup_required
            .as_ref()
            .expect("the verdict is recorded");
        assert_eq!(required.release_id, f.waiting_release);
        assert!(required.reason.contains("archive"), "{}", required.reason);
    }

    /// The verdict is recorded; the installed chart keeps reporting what runs.
    /// Every later sync leaves the record alone: no write, no cleared verdict.
    #[tokio::test]
    async fn a_recorded_verdict_stays_while_the_release_still_waits() {
        let mut f = waiting_fixture().await;
        f.record
            .runtime_metadata
            .as_mut()
            .expect("metadata")
            .cloud_setup_required = Some(alien_core::CloudSetupRequired {
            release_id: f.waiting_release.clone(),
            reason: "This release adds or changes 'archive', which lives in your cloud account"
                .to_string(),
        });
        let mut installed = report(&f.installed_release);
        installed.release_name = None;
        for _ in 0..2 {
            let wrote = handle_installed_setup(&f.state, &f.subject, &f.record, &installed).await;
            assert!(!wrote);
        }
        assert!(
            f.written.lock().expect("log").is_empty(),
            "a settled verdict is not written again"
        );
    }

    /// The waiting release is gone: the verdict recorded for it no longer
    /// applies and is cleared.
    #[tokio::test]
    async fn a_verdict_for_a_release_that_cannot_be_prepared_is_cleared() {
        let mut f = waiting_fixture().await;
        let metadata = f.record.runtime_metadata.as_mut().expect("metadata");
        metadata.cloud_setup_required = Some(alien_core::CloudSetupRequired {
            release_id: "rel_gone".to_string(),
            reason: "stale".to_string(),
        });
        f.record.desired_release_id = Some("rel_gone".to_string());
        let mut installed = report(&f.installed_release);
        installed.release_name = None;
        handle_installed_setup(&f.state, &f.subject, &f.record, &installed).await;

        let written = f.written.lock().expect("log");
        assert_eq!(written.len(), 1);
        assert!(written[0].cloud_setup_required.is_none());
    }

    /// Reading the waiting release fails (a store error, not a missing
    /// release): that says nothing about the release, so the stored verdict
    /// stays and nothing is written. The next sync repeats the report.
    #[tokio::test]
    async fn a_store_error_reading_the_waiting_release_keeps_the_verdict() {
        let mut f = waiting_fixture().await;
        f.record
            .runtime_metadata
            .as_mut()
            .expect("metadata")
            .cloud_setup_required = Some(alien_core::CloudSetupRequired {
            release_id: f.waiting_release.clone(),
            reason: "kept".to_string(),
        });
        f.db.execute("ALTER TABLE releases RENAME TO releases_unavailable")
            .await
            .expect("break the release table");
        let mut installed = report(&f.installed_release);
        installed.release_name = None;
        let result = apply_report(&f.state, &f.subject, &f.record, &installed).await;
        assert!(result.is_err(), "the store error surfaces: {result:?}");
        assert!(
            f.written.lock().expect("log").is_empty(),
            "a store error writes nothing, so the verdict stays"
        );
    }

    /// A write that loses to a concurrent change wrote nothing, but the record
    /// is no longer the one the sync read. The caller must re-read it, or it
    /// would copy the stale metadata (including a grant a concurrent writer
    /// just stored) into the Operator's state report.
    #[tokio::test]
    async fn a_write_that_loses_a_race_tells_the_caller_to_reread() {
        let f = waiting_fixture_with(true).await;
        let mut stale = report(&f.installed_release);
        stale.release_name = None;
        let changed = handle_installed_setup(&f.state, &f.subject, &f.record, &stale).await;
        assert_eq!(
            f.written.lock().expect("log").len(),
            1,
            "the write was attempted"
        );
        assert!(changed, "the stored record moved, so it must be re-read");
    }

    /// Fetches one path from the manager's chart registry, as Helm does.
    async fn pull_chart_path(state: &AppState, path: &str) -> Vec<u8> {
        let response = charts::serve(state, &Subject::system(), &Method::GET, path).await;
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("chart response body")
            .to_vec();
        assert!(
            status.is_success(),
            "GET {path}: {status} {}",
            String::from_utf8_lossy(&body)
        );
        body
    }

    /// What the Operator installed from the chart this manager serves for
    /// `release_id` would report: the chart is pulled over the registry
    /// routes, rendered with `helm template`, and the report is read from the
    /// Operator container's environment and the chart's ConfigMap, the way the
    /// Operator reads them.
    async fn report_from_served_chart(
        state: &AppState,
        chart_name: &str,
        release_id: &str,
    ) -> InstalledSetup {
        let tags: serde_json::Value = serde_json::from_slice(
            &pull_chart_path(state, &format!("charts/{chart_name}/tags/list")).await,
        )
        .expect("tag list");
        let mut chart = None;
        for tag in tags["tags"].as_array().expect("tags") {
            let tag = tag.as_str().expect("tag");
            let manifest: serde_json::Value = serde_json::from_slice(
                &pull_chart_path(state, &format!("charts/{chart_name}/manifests/{tag}")).await,
            )
            .expect("manifest");
            if manifest["annotations"]["org.opencontainers.image.revision"] == release_id {
                let digest = manifest["layers"][0]["digest"].as_str().expect("layer");
                chart = Some(
                    pull_chart_path(state, &format!("charts/{chart_name}/blobs/{digest}")).await,
                );
            }
        }
        let chart = chart.unwrap_or_else(|| panic!("no chart is served for {release_id}"));

        let directory = tempfile::tempdir().expect("chart directory");
        let archive = directory.path().join("chart.tgz");
        std::fs::write(&archive, chart).expect("write chart");
        let output = std::process::Command::new("helm")
            .arg("template")
            .arg("shop")
            .arg(&archive)
            .args(["--namespace", "apps"])
            .args(["--set", "management.token=ax_dg_example"])
            .args([
                "--set",
                "runtime.encryption.key=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ])
            .output()
            .expect("run helm template (helm must be installed)");
        let rendered = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "helm template failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // `- name: X` followed by `value: "..."`, as the chart renders env
        // entries; the ConfigMap holds the release in `setup-release-id`.
        let unquote = |value: &str| value.trim().trim_matches('"').to_string();
        let mut env = BTreeMap::new();
        let mut release_file = None;
        let mut lines = rendered.lines().map(str::trim).peekable();
        while let Some(line) = lines.next() {
            if let Some(name) = line.strip_prefix("- name: ") {
                if let Some(value) = lines.peek().and_then(|next| next.strip_prefix("value: ")) {
                    env.insert(name.to_string(), unquote(value));
                }
            } else if let Some(value) = line.strip_prefix("setup-release-id: ") {
                release_file = Some(unquote(value));
            }
        }
        let setup_env = |name: &str| {
            env.get(name).cloned().unwrap_or_else(|| {
                panic!(
                    "the chart served for release {release_id} gives the Operator no {name}, so the installed setup is never reported; Operator env: {:?}",
                    env.keys().collect::<Vec<_>>()
                )
            })
        };
        let setup_target = setup_env("OPERATOR_SETUP_TARGET");
        let setup_fingerprint = setup_env("OPERATOR_SETUP_FINGERPRINT");
        InstalledSetup {
            release_id: release_file.unwrap_or_else(|| {
                panic!("the chart served for release {release_id} records no setup release")
            }),
            setup_target,
            setup_fingerprint,
            setup_fingerprint_version: setup_env("OPERATOR_SETUP_FINGERPRINT_VERSION")
                .parse()
                .expect("fingerprint version"),
            release_name: Some(setup_env("OPERATOR_HELM_RELEASE")),
            namespace: Some(setup_env("KUBERNETES_NAMESPACE")),
        }
    }

    /// The deployment as the SQLite store returns it.
    async fn stored(deployments: &SqliteDeploymentStore, id: &str) -> DeploymentRecord {
        deployments
            .get_deployment(&Subject::system(), id)
            .await
            .expect("read deployment")
            .expect("deployment exists")
    }

    /// A Helm install of the standalone manager's own chart, paused on a
    /// release that adds a frozen service account. Upgrading to the chart the
    /// manager serves for that release completes the update; the chart for the
    /// installed release does not. Everything goes through the SQLite store.
    #[tokio::test]
    async fn the_chart_this_manager_serves_completes_a_setup_update() {
        let temp = tempfile::tempdir().expect("test directory");
        let db = Arc::new(
            SqliteDatabase::new(&temp.path().join("manager.db").to_string_lossy())
                .await
                .expect("database"),
        );
        let releases = Arc::new(SqliteReleaseStore::new(db.clone()));
        let deployments = Arc::new(SqliteDeploymentStore::new(db.clone()));
        let publish = |stack: Stack| {
            let releases = releases.clone();
            async move {
                releases
                    .create_release(
                        &Subject::system(),
                        CreateReleaseParams {
                            project_id: "default".to_string(),
                            stacks: std::collections::HashMap::from([(
                                Platform::Kubernetes,
                                stack,
                            )]),
                            git_commit_sha: None,
                            git_commit_ref: None,
                            git_commit_message: None,
                        },
                    )
                    .await
                    .expect("release")
            }
        };
        let installed = publish(baseline()).await;
        let waiting = publish(with_service_account(&baseline())).await;

        let group = deployments
            .create_deployment_group(
                &Subject::system(),
                CreateDeploymentGroupParams {
                    name: "customers".to_string(),
                    max_deployments: 10,
                    setup: Default::default(),
                },
            )
            .await
            .expect("deployment group");
        let template = deployment(baseline());
        let prepared = prepare_target(
            &installed,
            &template,
            template.stack_settings.as_ref().expect("settings"),
            template.stack_state.as_ref().expect("stack state"),
        )
        .await
        .expect("prepared baseline");
        let created = deployments
            .create_with_state(
                &Subject::system(),
                CreateImportedDeploymentParams {
                    deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                    name: template.name.clone(),
                    deployment_group_id: group.id,
                    platform: Platform::Kubernetes,
                    base_platform: None,
                    stack_settings: Default::default(),
                    stack_state: StackState::new(Platform::Kubernetes),
                    environment_info: None,
                    runtime_metadata: RuntimeMetadata {
                        prepared_stack: Some(prepared.clone()),
                        ..RuntimeMetadata::default()
                    },
                    status: "update-failed".to_string(),
                    current_release_id: Some(installed.id.clone()),
                    desired_release_id: Some(waiting.id.clone()),
                    import_source: None,
                    setup_metadata: None,
                    setup_target: String::new(),
                    setup_fingerprint: String::new(),
                    setup_fingerprint_version: 0,
                    deployment_token: None,
                    management_config: None,
                    input_values: Default::default(),
                },
            )
            .await
            .expect("deployment");
        // A Helm install registers without a setup lane; its first report
        // records one.
        db.execute(
            "UPDATE deployments SET setup_target = NULL, setup_fingerprint = NULL, setup_fingerprint_version = NULL",
        )
        .await
        .expect("clear the setup lane");

        let mut state = crate::routes::sync::tests::test_app_state(
            temp.path(),
            deployments.clone(),
            releases.clone(),
            Arc::new(
                crate::providers::local_credentials::LocalCredentialResolver::new(
                    temp.path().to_path_buf(),
                ),
            ),
            db.clone(),
        )
        .await;
        state.charts = Some(Arc::new(ChartSettings::new(
            Some("alien-operator:test".to_string()),
            false,
        )));
        let operator = Subject {
            scope: Scope::Deployment {
                project_id: created.project_id.clone(),
                deployment_id: created.id.clone(),
            },
            ..Subject::system()
        };

        // Before the upgrade the installed chart reports the installed
        // release: nothing is authorized and the update stays paused.
        let before = report_from_served_chart(&state, "app", &installed.id).await;
        assert_eq!(before.release_id, installed.id);
        assert_eq!(before.setup_target, "kubernetes");
        assert_eq!(before.release_name.as_deref(), Some("shop"));
        assert_eq!(before.namespace.as_deref(), Some("apps"));
        handle_installed_setup(
            &state,
            &operator,
            &stored(&deployments, &created.id).await,
            &before,
        )
        .await;
        let record = stored(&deployments, &created.id).await;
        assert_eq!(record.status, "update-failed");
        let metadata = record.runtime_metadata.expect("metadata");
        assert!(metadata.setup_update_authorization.is_none());
        assert_eq!(
            metadata.installed_helm_release,
            Some(alien_core::InstalledHelmRelease {
                name: "shop".to_string(),
                namespace: "apps".to_string(),
            })
        );
        assert_eq!(record.setup_target.as_deref(), Some("kubernetes"));
        assert_eq!(
            record.setup_fingerprint_version,
            Some(before.setup_fingerprint_version)
        );

        // `helm upgrade` to the chart for the waiting release: a different
        // setup, so the Operator restarts and reports it, and the manager
        // authorizes exactly this change.
        let after = report_from_served_chart(&state, "app", &waiting.id).await;
        assert_eq!(after.release_id, waiting.id);
        assert_ne!(
            after.setup_fingerprint, before.setup_fingerprint,
            "a chart with a different setup must restart the Operator"
        );
        handle_installed_setup(
            &state,
            &operator,
            &stored(&deployments, &created.id).await,
            &after,
        )
        .await;
        let record = stored(&deployments, &created.id).await;
        assert_eq!(record.status, "update-pending");
        assert_eq!(
            record.setup_fingerprint.as_deref(),
            Some(after.setup_fingerprint.as_str())
        );
        let authorization = record
            .runtime_metadata
            .expect("metadata")
            .setup_update_authorization
            .expect("the upgrade authorizes the setup update");
        assert_eq!(authorization.release_id, waiting.id);
        assert_eq!(authorization.setup_fingerprint, after.setup_fingerprint);
        assert_eq!(authorization.setup_target, "kubernetes");
        assert_eq!(
            authorization.baseline_frozen_digest,
            prepared.setup_update_digest()
        );

        // A newer release adds a cloud resource. The stored verdict and the
        // refusal of the now-stale chart survive the SQLite round trip.
        let cloud = publish(with_storage(&with_service_account(&baseline()))).await;
        deployments
            .set_deployment_desired_release(&Subject::system(), &created.id, &cloud.id)
            .await
            .expect("point at the cloud release");
        handle_installed_setup(
            &state,
            &operator,
            &stored(&deployments, &created.id).await,
            &after,
        )
        .await;
        let required = stored(&deployments, &created.id)
            .await
            .runtime_metadata
            .expect("metadata")
            .cloud_setup_required
            .expect("the cloud setup verdict is stored");
        assert_eq!(required.release_id, cloud.id);
        handle_installed_setup(
            &state,
            &operator,
            &stored(&deployments, &created.id).await,
            &after,
        )
        .await;
        let refusal = stored(&deployments, &created.id)
            .await
            .runtime_metadata
            .expect("metadata")
            .setup_report_refusal
            .expect("the stale chart's refusal is stored");
        assert_eq!(refusal.reported_release_id, waiting.id);
        assert!(refusal.reason.contains(&cloud.id), "{}", refusal.reason);

        // A release that changes only Live code keeps the chart's setup: the
        // Operator's pod template, and so the running Operator, stays as it is.
        let live_code = |image: &str| {
            Stack::new("live-code".to_string())
                .permission("runtime", alien_core::permissions::PermissionProfile::new())
                .add(
                    ServiceAccount::new("execution-sa".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(
                    alien_core::Worker::new("api".to_string())
                        .code(alien_core::WorkerCode::Image {
                            image: image.to_string(),
                        })
                        .permissions("runtime".to_string())
                        .build(),
                    ResourceLifecycle::Live,
                )
                .build()
        };
        let first = publish(live_code("example.com/api:1")).await;
        let second = publish(live_code("example.com/api:2")).await;
        let first = report_from_served_chart(&state, "live-code", &first.id).await;
        let second = report_from_served_chart(&state, "live-code", &second.id).await;
        assert_ne!(first.release_id, second.release_id);
        assert_eq!(
            first.setup_fingerprint, second.setup_fingerprint,
            "a Live code change must not change the chart's setup"
        );
    }
}
