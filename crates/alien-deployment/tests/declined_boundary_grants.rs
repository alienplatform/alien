//! Declined setup keys must not leave grants in the boundary of the prepared deployment.
use alien_core::{
    permissions::ManagementPermissions, ClientConfig, DeploymentConfig, DeploymentState,
    DeploymentStatus, EnvironmentVariablesSnapshot, Key, PermissionProfile, PermissionsConfig,
    Platform, ReleaseInfo, ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack,
    StackInputDefinition, StackResourceState, StackState, Worker, WorkerCode,
};
use alien_permissions::{
    generators::{aws_permissions_boundary_covers, aws_stack_permissions_boundary},
    PermissionContext,
};
use serde_json::json;

const KEY: &str = "arn:aws:kms:us-east-1:123456789012:key/00000000-0000-0000-0000-000000000001";

const RETAINED_KEY: &str =
    "arn:aws:kms:us-east-1:123456789012:key/00000000-0000-0000-0000-000000000002";

fn declared() -> Stack {
    Stack::new("boundary-gates".into())
        .inputs(vec![StackInputDefinition::deployer_boolean(
            "keyEnabled",
            "Encryption key",
            "Create the optional encryption key",
            Some(true),
        )])
        .permissions(
            PermissionsConfig::new().with_profile(
                "execution",
                PermissionProfile::new()
                    .resource("optional", ["key/remote-cryptography"])
                    .resource("retained", ["key/remote-cryptography"]),
            ),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("optional", ["key/management"])
                .resource("retained", ["key/management"]),
        ))
        .add(
            Key::new("retained".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Key::new("optional".into()).build(),
            ResourceLifecycle::Frozen,
            "keyEnabled",
        )
        .add(
            Worker::new("jobs".into())
                .permissions("execution".into())
                .code(WorkerCode::Image {
                    image: "example.com/jobs:latest".into(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn config(enabled: bool) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(Default::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    config
        .input_values
        .insert("keyEnabled".into(), json!(enabled));
    config
}

async fn prepare(enabled: bool, previous: &RuntimeMetadata) -> RuntimeMetadata {
    alien_deployment::prepare_direct_setup_update(
        declared(),
        &StackState::new(Platform::Aws),
        &config(enabled),
        &ClientConfig::Local {
            state_directory: "/unused".into(),
        },
        previous,
    )
    .await
    .expect("deployment setup preparation")
}

fn assert_key_grants(metadata: &RuntimeMetadata, enabled: bool) {
    assert_eq!(
        metadata.persisted_gate_answers.get("keyEnabled"),
        Some(&enabled)
    );
    let stack = metadata.prepared_stack.as_ref().unwrap();
    assert_eq!(stack.resources.contains_key("optional"), enabled);
    assert_eq!(
        stack.permissions.profiles["execution"]
            .0
            .contains_key("optional"),
        enabled
    );
    assert_eq!(
        stack
            .management()
            .profile()
            .unwrap()
            .0
            .contains_key("optional"),
        enabled
    );
    assert!(stack.resources.contains_key("retained"));
    assert!(stack.permissions.profiles["execution"]
        .0
        .contains_key("retained"));
    assert!(stack
        .management()
        .profile()
        .unwrap()
        .0
        .contains_key("retained"));
    let context = PermissionContext::new()
        .with_stack_prefix("boundary-gates")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321");
    let mut resolutions = 0;
    let boundary = aws_stack_permissions_boundary(stack, &context, |profile, resource| {
        assert_eq!(profile, "execution");
        let arn = match resource {
            "optional" => {
                assert!(
                    enabled,
                    "a declined key must never require physical ARN resolution"
                );
                KEY
            }
            "retained" => RETAINED_KEY,
            _ => panic!("unexpected resource: {resource}"),
        };
        resolutions += 1;
        Ok(context.clone().with_resource_name(arn))
    })
    .expect("the prepared boundary compiles without absent resource outputs");
    assert_eq!(resolutions, 1 + usize::from(enabled));
    let retained_decrypt = json!({"Version": "2012-10-17", "Statement": [{
        "Effect": "Allow", "Action": "kms:Decrypt", "Resource": RETAINED_KEY,
    }]});
    assert!(aws_permissions_boundary_covers(&boundary, &retained_decrypt).unwrap());
    let decrypt = json!({"Version": "2012-10-17", "Statement": [{
        "Effect": "Allow", "Action": "kms:Decrypt", "Resource": KEY,
    }]});
    assert_eq!(
        aws_permissions_boundary_covers(&boundary, &decrypt).unwrap(),
        enabled
    );
}

#[tokio::test]
async fn declined_setup_key_removes_runtime_and_management_grants_before_boundary_generation() {
    // The true default and accepted control prove that the explicit false answer,
    // rather than an unused profile or missing fixture resource, removes the grant.
    let accepted = prepare(true, &RuntimeMetadata::default()).await;
    assert_key_grants(&accepted, true);
    let declined = prepare(false, &RuntimeMetadata::default()).await;
    assert_key_grants(&declined, false);

    // A later release input cannot reverse a recorded manual-setup decline.
    // Run preparation twice to ensure derived identities do not restore the grant.
    let repeated = prepare(true, &declined).await;
    assert_key_grants(&repeated, false);
    let repeated = prepare(true, &repeated).await;
    assert_key_grants(&repeated, false);
}

#[tokio::test]
async fn release_update_keeps_a_declined_key_out_of_the_pending_boundary() {
    let metadata = prepare(false, &RuntimeMetadata::default()).await;
    let prepared = metadata.prepared_stack.as_ref().unwrap();
    let mut installed = StackState::new(Platform::Aws);
    for (id, entry) in prepared.resources() {
        let mut resource = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.dependencies.clone(),
        );
        resource.status = ResourceStatus::Running;
        resource.internal_state = Some(json!({}));
        installed.resources.insert(id.clone(), resource);
    }
    let release = |version: &str| ReleaseInfo {
        release_id: Some(version.into()),
        version: Some(version.into()),
        description: None,
        stack: declared(),
    };
    let current = DeploymentState {
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Aws,
        current_release: Some(release("1")),
        target_release: Some(release("2")),
        stack_state: Some(installed),
        error: None,
        environment_info: None,
        runtime_metadata: Some(metadata),
        retry_requested: false,
        protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    let mut result = alien_deployment::step(
        current,
        config(false),
        ClientConfig::Local {
            state_directory: "/unused".into(),
        },
        None,
    )
    .await
    .expect("release update preparation")
    .state;
    assert_eq!(
        result.status,
        DeploymentStatus::Updating,
        "{:?}",
        result.error
    );
    assert!(result.error.is_none());
    assert!(!result
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("optional"));
    let metadata = result.runtime_metadata.as_mut().unwrap();
    // Inspect the pending release, not the previously prepared setup baseline.
    metadata.prepared_stack = Some(metadata.pending_prepared_stack.take().unwrap());
    assert_key_grants(metadata, false);
}
