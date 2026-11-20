//! Identities a deployment's state could name for an extension that are not the one setup
//! created for it: `check_identity` refuses each before any cloud call. On Azure a client id
//! names nothing setup chooses, so it must be the one the deployment's state records for the
//! extension's managed identity, and minting then reads that identity from ARM (see the Azure
//! mint tests).

use alien_core::{
    AwsEnvironmentInfo, AzureEnvironmentInfo, EnvironmentInfo, GcpEnvironmentInfo, Platform,
    RemoteStackManagement, RemoteStackManagementOutputs, Resource, ResourceLifecycle,
    ResourceOutputs, ResourceStatus, StackResourceState, StackState,
};
use alien_manager::providers::extension_credentials::check_identity;
use alien_manager::{DeploymentRecord, ExtensionIdentity};

const AWS_MGMT: &str = "arn:aws:iam::210987654321:role/test-prefix-management";

fn record(
    platform: Platform,
    prefix: &str,
    mgmt_id: &str,
    access: String,
    env: EnvironmentInfo,
) -> DeploymentRecord {
    let remote_management = RemoteStackManagement::new("management".to_string()).build();
    let mut state = StackState::with_resource_prefix(platform, prefix.to_string());
    state.resources.insert(
        "management".to_string(),
        StackResourceState::builder()
            .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(remote_management))
            .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: mgmt_id.to_string(),
                access_configuration: access,
                legacy_remote_bindings_access: None,
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    let mut value = serde_json::json!({
        "id": "dep_a", "name": "a", "deploymentGroupId": "g", "platform": platform,
        "deploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "status": "running", "stackState": null, "environmentInfo": null,
        "runtimeMetadata": null, "currentReleaseId": null, "desiredReleaseId": null,
        "userEnvironmentVariables": null, "retryRequested": false, "lockedBy": null,
        "lockedAt": null, "createdAt": "2026-11-09T00:00:00Z", "updatedAt": null, "error": null
    });
    value["environmentInfo"] = serde_json::to_value(&env).unwrap();
    let mut record: DeploymentRecord = serde_json::from_value(value).expect("record");
    record.stack_state = Some(state);
    record.environment_info = Some(env);
    record
}

fn aws(prefix: &str) -> DeploymentRecord {
    record(
        Platform::Aws,
        prefix,
        AWS_MGMT,
        AWS_MGMT.to_string(),
        EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "210987654321".to_string(),
            region: "us-east-2".to_string(),
        }),
    )
}

fn gcp() -> DeploymentRecord {
    record(
        Platform::Gcp,
        "test-prefix",
        "mgmt@target-project.iam.gserviceaccount.com",
        serde_json::json!({
            "projectNumber": "987654321",
            "serviceAccountEmail": "mgmt@target-project.iam.gserviceaccount.com"
        })
        .to_string(),
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_id: "target-project".to_string(),
            project_number: "987654321".to_string(),
            region: "us-east1".to_string(),
        }),
    )
}

const AZURE_TENANT: &str = "6c1b1a8e-1f2b-4a5e-9d3c-0a1b2c3d4e5f";
const ACME_CLIENT_ID_A: &str = "0a7d6c8e-3b1f-4c2a-8e9d-aaaaaaaaaaaa";
const ACME_CLIENT_ID_B: &str = "0a7d6c8e-3b1f-4c2a-8e9d-bbbbbbbbbbbb";

/// An Azure deployment with `prefix` in `subscription`, whose state records `client_id` as the
/// client id of acme's identity.
fn azure(prefix: &str, subscription: &str, client_id: &str) -> DeploymentRecord {
    let mut record = record(
        Platform::Azure,
        prefix,
        "management-identity",
        serde_json::json!({
            "uamiClientId": "0a7d6c8e-3b1f-4c2a-8e9d-000000000000",
            "tenantId": AZURE_TENANT
        })
        .to_string(),
        EnvironmentInfo::Azure(AzureEnvironmentInfo {
            tenant_id: AZURE_TENANT.to_string(),
            subscription_id: subscription.to_string(),
            location: "eastus".to_string(),
        }),
    );
    record.stack_state.as_mut().unwrap().resources.insert(
        "acme-ext".to_string(),
        alien_infra::azure_extension_identity_state(prefix, subscription, "acme", client_id),
    );
    record
}

fn ident(extension: &str, identity: &str) -> ExtensionIdentity {
    ExtensionIdentity {
        extension: extension.to_string(),
        id: format!("{extension}-ext"),
        identity: identity.to_string(),
    }
}

#[test]
/// Another deployment's, another extension's, the management or remote bindings role, any
/// other role, and malformed ARNs are refused; the role setup names is not.
fn aws_identities_setup_did_not_create_are_refused() {
    let a = aws("test-prefix");
    check_identity(
        &a,
        &ident(
            "acme",
            "arn:aws:iam::210987654321:role/test-prefix-acme-ext",
        ),
    )
    .unwrap();
    for (label, identity) in [
        (
            "deployment B",
            "arn:aws:iam::210987654321:role/other-prefix-acme-ext",
        ),
        (
            "extension Y",
            "arn:aws:iam::210987654321:role/test-prefix-billing-ext",
        ),
        ("management", AWS_MGMT),
        (
            "remote bindings",
            "arn:aws:iam::210987654321:role/test-prefix-access",
        ),
        (
            "arbitrary",
            "arn:aws:iam::210987654321:role/OrganizationAccountAccessRole",
        ),
        (
            "path",
            "arn:aws:iam::210987654321:role/evil/test-prefix-acme-ext",
        ),
        (
            "trailing",
            "arn:aws:iam::210987654321:role/test-prefix-acme-ext/",
        ),
        (
            "extra colon",
            "arn:aws:iam::210987654321:role/test-prefix-acme-ext:x",
        ),
        (
            "other account",
            "arn:aws:iam::999999999999:role/test-prefix-acme-ext",
        ),
        ("empty", ""),
        (
            "user",
            "arn:aws:iam::210987654321:user/test-prefix-acme-ext",
        ),
    ] {
        assert!(
            check_identity(&a, &ident("acme", identity)).is_err(),
            "{label}: {identity} was accepted"
        );
    }
}

#[test]
/// A long name is pinned as setup cuts it, and the uncut name is refused.
fn aws_long_names_follow_the_terraform_cut() {
    let prefix = "customer-provided-resource-prefix";
    let a = aws(prefix);
    let name = alien_core::extension_identity_aws_role_name(
        prefix,
        "contoso-backup-and-restore-for-postgres-ext",
    );
    assert_eq!(name.len(), 64);
    check_identity(
        &a,
        &ident(
            "contoso-backup-and-restore-for-postgres",
            &format!("arn:aws:iam::210987654321:role/{name}"),
        ),
    )
    .unwrap();
    // CloudFormation's uncut name, which IAM itself can't create past 64 characters.
    let uncut = format!(
        "arn:aws:iam::210987654321:role/{prefix}-contoso-backup-and-restore-for-postgres-ext"
    );
    assert!(check_identity(
        &a,
        &ident("contoso-backup-and-restore-for-postgres", &uncut)
    )
    .is_err());
}

#[test]
/// The same for service accounts, the default compute account, and suffix tricks.
fn gcp_identities_setup_did_not_create_are_refused() {
    let g = gcp();
    let legit = format!(
        "{}@target-project.iam.gserviceaccount.com",
        alien_core::extension_identity_gcp_account_id("test-prefix", "acme-ext")
    );
    check_identity(&g, &ident("acme", &legit)).unwrap();
    let other_dep = format!(
        "{}@target-project.iam.gserviceaccount.com",
        alien_core::extension_identity_gcp_account_id("other-prefix", "acme-ext")
    );
    let other_ext = format!(
        "{}@target-project.iam.gserviceaccount.com",
        alien_core::extension_identity_gcp_account_id("test-prefix", "billing-ext")
    );
    for (label, identity) in [
        ("deployment B", other_dep.as_str()),
        ("extension Y", other_ext.as_str()),
        ("management", "mgmt@target-project.iam.gserviceaccount.com"),
        (
            "default compute",
            "987654321-compute@developer.gserviceaccount.com",
        ),
        (
            "other project",
            &legit.replace("@target-project.", "@other."),
        ),
        ("suffix trick", &format!("x{legit}")),
    ] {
        assert!(
            check_identity(&g, &ident("acme", identity)).is_err(),
            "{label}: {identity} was accepted"
        );
    }
}

#[test]
/// A record copied into deployment A's state from deployment B, whose acme identity trusts the
/// same federated token, is refused though the step presents the client id that record holds:
/// it names B's managed identity, or one in another subscription. A's own record is accepted,
/// and a client id the step presents that A's stored record does not hold is refused.
fn an_azure_identity_from_another_deployment_is_refused() {
    let a = azure("test-prefix", "sub-a", ACME_CLIENT_ID_A);
    let b = azure("other-prefix", "sub-a", ACME_CLIENT_ID_B);
    check_identity(&a, &ident("acme", ACME_CLIENT_ID_A)).unwrap();
    check_identity(&b, &ident("acme", ACME_CLIENT_ID_B)).unwrap();
    check_identity(&a, &ident("acme", &ACME_CLIENT_ID_A.to_uppercase())).unwrap();

    for (label, prefix, subscription) in [
        ("B's record", "other-prefix", "sub-a"),
        ("a record in another subscription", "test-prefix", "sub-b"),
    ] {
        let mut copied = a.clone();
        copied.stack_state.as_mut().unwrap().resources.insert(
            "acme-ext".to_string(),
            alien_infra::azure_extension_identity_state(
                prefix,
                subscription,
                "acme",
                ACME_CLIENT_ID_B,
            ),
        );
        let error = check_identity(&copied, &ident("acme", ACME_CLIENT_ID_B))
            .expect_err(label)
            .to_string();
        assert!(
            error.contains("not the managed identity 'test-prefix-acme-ext'"),
            "{label}: {error}"
        );
    }

    let error = check_identity(&a, &ident("acme", ACME_CLIENT_ID_B))
        .expect_err("a client id A's stored record does not hold")
        .to_string();
    assert!(
        error.contains("is not the client id the deployment records"),
        "{error}"
    );

    let mut unrecorded = a.clone();
    unrecorded
        .stack_state
        .as_mut()
        .unwrap()
        .resources
        .remove("acme-ext");
    assert!(check_identity(&unrecorded, &ident("acme", ACME_CLIENT_ID_A)).is_err());
}
