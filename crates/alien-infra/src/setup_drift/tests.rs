use super::*;
use crate::core::controller_test::SingleControllerExecutor;
use crate::remote_stack_management::management_permissions_revision;
use crate::{AwsNetworkController, AwsRemoteStackManagementController};
use alien_core::{Network, NetworkSettings, Platform, RemoteStackManagement};

const DIRECT: InitialSetupAuthority = InitialSetupAuthority::DirectSetup;

/// An AWS install whose management controller recorded `revision` and `digests`.
async fn install(
    revision: Option<String>,
    digests: BTreeMap<String, String>,
) -> SingleControllerExecutor {
    let mut controller = AwsRemoteStackManagementController::mock_ready("test");
    controller.management_permissions_revision = revision;
    controller.management_permission_set_digests = digests;
    // A setup run installed the permissions boundary.
    controller.permissions_boundary_arn = Some("arn:aws:iam::1:policy/test-boundary".to_string());
    controller.permissions_boundary_document = Some(serde_json::json!({ "Version": "2012-10-17" }));
    SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .controller(controller)
        .platform(Platform::Aws)
        .initial_setup_authority(DIRECT)
        .build()
        .await
        .expect("executor builds")
}

fn drift(
    executor: &SingleControllerExecutor,
    authority: InitialSetupAuthority,
) -> Vec<SetupDriftReason> {
    executor
        .with_context(|ctx| setup_drift(ctx.desired_stack, ctx.state, ctx.platform, authority))
        .expect("drift")
}

/// What this binary would record for the install's stack.
fn current(executor: &SingleControllerExecutor) -> (String, BTreeMap<String, String>) {
    executor.with_context(|ctx| {
        (
            management_permissions_revision(ctx)
                .unwrap()
                .expect("a management profile"),
            management_permission_set_digests(ctx.desired_stack).unwrap(),
        )
    })
}

#[tokio::test]
async fn an_install_that_recorded_what_this_binary_would_install_is_current() {
    let probe = install(None, BTreeMap::new()).await;
    let (revision, digests) = current(&probe);
    let executor = install(Some(revision), digests).await;
    assert_eq!(drift(&executor, DIRECT), vec![]);
}

/// The 11-03 case: a permission set's body changed and its id did not. The recorded revision was
/// computed from the old body, so it differs from the one this binary computes.
#[tokio::test]
async fn a_changed_permission_set_body_under_the_same_id_is_drift() {
    let probe = install(None, BTreeMap::new()).await;
    let (_, digests) = current(&probe);
    let changed_id = digests.keys().next().expect("a set").clone();
    let mut recorded = digests.clone();
    recorded.insert(changed_id.clone(), "digest-of-the-older-body".to_string());
    let executor = install(Some("revision-of-the-older-body".to_string()), recorded).await;

    let reasons = drift(&executor, DIRECT);
    assert_eq!(
        reasons,
        vec![SetupDriftReason {
            code: SetupDriftCode::ManagementPermissionsChanged,
            resource: Some(changed_id.clone()),
            detail: format!("Management permission set '{changed_id}' has changed"),
        }]
    );
}

/// An install from before per-set digests were recorded still says its permissions changed.
#[tokio::test]
async fn a_recorded_revision_without_digests_is_drift_without_naming_a_set() {
    let executor = install(Some("older".to_string()), BTreeMap::new()).await;
    let reasons = drift(&executor, DIRECT);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_eq!(
        reasons[0].code,
        SetupDriftCode::ManagementPermissionsChanged
    );
    assert_eq!(reasons[0].resource.as_deref(), Some("management"));
}

/// An old-scheme install with no revision at all counts as behind once.
#[tokio::test]
async fn an_install_with_no_recorded_revision_is_behind_once() {
    let executor = install(None, BTreeMap::new()).await;
    let reasons = drift(&executor, DIRECT);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_eq!(
        reasons[0].code,
        SetupDriftCode::ManagementPermissionsUnrecorded
    );
}

/// Imports (template engines) never recorded a revision and refresh grants themselves: the
/// management grants are not judged there.
#[tokio::test]
async fn a_template_install_has_no_management_drift() {
    let executor = install(None, BTreeMap::new()).await;
    assert_eq!(
        drift(&executor, InitialSetupAuthority::ImportedHandoff),
        vec![]
    );
}

fn denied_alarms(mut executor: SingleControllerExecutor) -> SingleControllerExecutor {
    let mut state = executor.stack_state().resources["management"].clone();
    let mut internal = state.internal_state.clone().expect("internal state");
    internal["alarms"] = serde_json::json!({ "names": [], "denied": "REMOTE_ACCESS_DENIED 403" });
    state.internal_state = Some(internal);
    // The same record under a second resource, as a compute cluster keeps it.
    executor.insert_saved_resource("compute", state);
    executor
}

/// Without a way to read the installed grants, a recorded denial is the signal.
#[tokio::test]
async fn a_recorded_denial_is_drift_when_the_installed_grants_cannot_be_read() {
    let executor = denied_alarms(install(None, BTreeMap::new()).await);
    let reasons = drift(&executor, InitialSetupAuthority::ImportedHandoff);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_eq!(reasons[0].code, SetupDriftCode::GrantDenied);
    assert_eq!(reasons[0].resource.as_deref(), Some("compute"));
}

/// Running setup again with grants that are already current cannot lift a denial, so on its own
/// it does not make `alien deploy` offer a setup update forever.
#[tokio::test]
async fn a_denial_alone_does_not_make_a_current_direct_install_behind() {
    let probe = install(None, BTreeMap::new()).await;
    let (revision, digests) = current(&probe);
    let executor = denied_alarms(install(Some(revision), digests).await);
    assert_eq!(drift(&executor, DIRECT), vec![]);
}

/// Next to another reason, the denial is listed so the person sees what the update fixes.
#[tokio::test]
async fn a_denial_is_listed_next_to_content_drift() {
    let executor = denied_alarms(install(Some("older".to_string()), BTreeMap::new()).await);
    let codes: Vec<_> = drift(&executor, DIRECT)
        .into_iter()
        .map(|r| r.code)
        .collect();
    assert_eq!(
        codes,
        vec![
            SetupDriftCode::ManagementPermissionsChanged,
            SetupDriftCode::GrantDenied
        ]
    );
}

/// The flow-log reasons only: the fixture's own management resource has no recorded revision.
fn flow_log_drift(executor: &SingleControllerExecutor) -> Vec<SetupDriftReason> {
    drift(executor, DIRECT)
        .into_iter()
        .filter(|reason| reason.code == SetupDriftCode::FlowLogMissing)
        .collect()
}

async fn network(flow_logs: Option<serde_json::Value>, byo: bool) -> SingleControllerExecutor {
    let mut controller = AwsNetworkController::mock_ready("vpc-1", 2);
    controller.is_byo_vpc = byo;
    controller.flow_logs =
        flow_logs.map(|value| serde_json::from_value(value).expect("flow log record"));
    SingleControllerExecutor::builder()
        .resource(
            Network::new("net".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
        )
        .controller(controller)
        .platform(Platform::Aws)
        .initial_setup_authority(DIRECT)
        .build()
        .await
        .expect("executor builds")
}

#[tokio::test]
async fn a_missing_flow_log_on_an_alien_created_vpc_is_drift() {
    let reasons = flow_log_drift(&network(None, false).await);
    assert_eq!(
        reasons,
        vec![SetupDriftReason {
            code: SetupDriftCode::FlowLogMissing,
            resource: Some("net".to_string()),
            detail: "The VPC has no flow log; setup adds one".to_string(),
        }]
    );
}

#[tokio::test]
async fn a_delivering_flow_log_is_current() {
    let complete = serde_json::json!({ "flowLogId": "fl-1", "delivering": true });
    assert_eq!(
        flow_log_drift(&network(Some(complete), false).await),
        vec![]
    );
}

#[tokio::test]
async fn a_vpc_the_customer_brought_never_needs_a_flow_log() {
    assert_eq!(flow_log_drift(&network(None, true).await), vec![]);
}

/// The management revision can match while the permissions boundary was never installed: the
/// install is not current.
#[tokio::test]
async fn matching_management_permissions_do_not_hide_a_missing_boundary() {
    let probe = install(None, BTreeMap::new()).await;
    let (revision, digests) = current(&probe);
    let mut controller = AwsRemoteStackManagementController::mock_ready("test");
    controller.management_permissions_revision = Some(revision);
    controller.management_permission_set_digests = digests;
    controller.permissions_boundary_arn = None;
    controller.permissions_boundary_document = None;
    let executor = SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .controller(controller)
        .platform(Platform::Aws)
        .initial_setup_authority(DIRECT)
        .build()
        .await
        .expect("executor builds");
    assert_eq!(
        drift(&executor, DIRECT),
        vec![SetupDriftReason {
            code: SetupDriftCode::PermissionsBoundaryMissing,
            resource: Some("management".to_string()),
            detail: "The permissions boundary is not installed; setup installs it".to_string(),
        }]
    );
}

/// A template install's boundary is the template's: not judged here.
#[tokio::test]
async fn a_template_install_is_not_judged_on_its_boundary() {
    let executor = install(None, BTreeMap::new()).await;
    assert_eq!(
        drift(&executor, InitialSetupAuthority::ImportedHandoff),
        vec![]
    );
}
