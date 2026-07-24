use super::*;

fn legacy_controller(service_account_email: Option<&str>) -> GcpRemoteStackManagementController {
    GcpRemoteStackManagementController {
        setup_managed: None,
        state: GcpRemoteStackManagementState::Ready,
        service_account_email: service_account_email.map(str::to_string),
        service_account_unique_id: Some("1234567890".to_string()),
        role_bound: true,
        impersonation_granted: true,
        applied_management_grant_fingerprint: None,
        remote_storage_bucket_names: Vec::new(),
        _internal_stay_count: None,
    }
}

#[test]
fn legacy_terraform_import_remains_setup_managed() {
    let controller = legacy_controller(Some(
        "a-stack-management-12ab34cd@target-project.iam.gserviceaccount.com",
    ));

    assert!(controller.setup_managed_resources("stack"));
}

#[test]
fn legacy_direct_controller_remains_runtime_owned() {
    let controller = legacy_controller(Some(
        "stack-management@target-project.iam.gserviceaccount.com",
    ));
    assert!(!controller.setup_managed_resources("stack"));

    let failed_before_identity_checkpoint = legacy_controller(None);
    assert!(!failed_before_identity_checkpoint.setup_managed_resources("stack"));
}

#[test]
fn explicit_ownership_overrides_legacy_name_inference() {
    let mut controller = legacy_controller(Some(
        "stack-management@target-project.iam.gserviceaccount.com",
    ));
    controller.setup_managed = Some(true);
    assert!(controller.setup_managed_resources("stack"));

    controller.service_account_email =
        Some("a-stack-management-12ab34cd@target-project.iam.gserviceaccount.com".to_string());
    controller.setup_managed = Some(false);
    assert!(!controller.setup_managed_resources("stack"));
}
