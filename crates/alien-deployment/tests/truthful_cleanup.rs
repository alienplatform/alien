//! Deployment cleanup preserves failures and waits for setup-owned permission removal.
use alien_aws_clients::{
    iam::{
        IamApi, IamClient, ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi,
        PolicyNames,
    },
    sqs::{CreateQueueRequest, GetQueueUrlRequest, MockSqsApi, SendMessageRequest, SqsApi},
};
use alien_core::permissions::{PermissionProfile, PermissionSet, PermissionsConfig};
use alien_core::{
    AwsClientConfig, AwsCredentials, ClientConfig, DeploymentConfig, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority, Platform, Queue,
    ReleaseInfo, Resource, ResourceLifecycle, ResourceStatus, RuntimeMetadata, ServiceAccount,
    Stack, StackResourceState, StackSettings, StackState,
};
use alien_error::AlienError;
use alien_infra::{deserialize_controller, MockPlatformServiceProvider, StackResourceStateExt};
use serde_json::json;
use std::sync::{Arc, Mutex};
fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build()
}
fn saved_queue(url: &str, name: &str) -> StackState {
    let controller = deserialize_controller(json!({"type":"AwsQueueController", "_controllerStateVersion":1, "state":"ready", "queueUrl":url, "queueName":name})).unwrap();
    let mut resource = StackResourceState::new_pending(
        "queue".into(),
        Resource::new(Queue::new("jobs".into()).build()),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    resource.status = ResourceStatus::Running;
    resource.set_internal_controller(Some(controller)).unwrap();
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("jobs".into(), resource);
    state
}
fn provider(client: Arc<dyn SqsApi>) -> Arc<MockPlatformServiceProvider> {
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_runtime_setup_authority()
        .returning(|_| None);
    provider
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(client.clone()));
    Arc::new(provider)
}
fn fake_config() -> ClientConfig {
    ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "synthetic".into(),
            secret_access_key: "synthetic".into(),
            session_token: None,
        },
        service_overrides: None,
    }))
}
fn deployment(stack: StackState) -> alien_core::DeploymentState {
    alien_core::DeploymentState {
        status: alien_core::DeploymentStatus::DeletePending,
        platform: Platform::Aws,
        current_release: None,
        target_release: None,
        stack_state: Some(stack),
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::CLEANUP_SETUP_DEPLOYMENT_PROTOCOL_VERSION,
    }
}
async fn run_steps(
    mut state: alien_core::DeploymentState,
    cfg: ClientConfig,
    services: Arc<MockPlatformServiceProvider>,
) -> alien_core::DeploymentState {
    for _ in 0..32 {
        state = alien_deployment::step(state, config(), cfg.clone(), Some(services.clone()))
            .await
            .unwrap()
            .state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if matches!(
            state.status,
            alien_core::DeploymentStatus::Deleted | alien_core::DeploymentStatus::DeleteFailed
        ) {
            break;
        }
    }
    println!("deployment={:?}", state.status);
    state
}
fn assert_preserved(state: &alien_core::DeploymentState, url: &str) {
    assert_eq!(state.status, alien_core::DeploymentStatus::DeleteFailed);
    let resource = &state.stack_state.as_ref().unwrap().resources["jobs"];
    assert_eq!(resource.status, ResourceStatus::DeleteFailed);
    assert_eq!(resource.internal_state.as_ref().unwrap()["queueUrl"], url);
    assert_eq!(
        resource.last_failed_state.as_ref().unwrap()["queueUrl"],
        url
    );
    assert!(resource.error.is_some());
}
#[tokio::test]
async fn denied_deployment_delete_preserves_disk_checkpoint_and_explicit_retry() {
    let exists = Arc::new(Mutex::new(true));
    let remote = exists.clone();
    let mut sqs = MockSqsApi::new();
    sqs.expect_delete_queue().times(1).returning(move |_| {
        assert!(*remote.lock().unwrap());
        Err(AlienError::new(
            alien_aws_clients::ErrorData::RemoteAccessDenied {
                resource_type: "queue".into(),
                resource_name: "synthetic-jobs".into(),
            },
        ))
    });
    let services = provider(Arc::new(sqs));
    let url = "https://sqs.us-east-1.amazonaws.com/123456789012/synthetic-jobs";
    let mut state = run_steps(
        deployment(saved_queue(url, "synthetic-jobs")),
        fake_config(),
        services.clone(),
    )
    .await;
    assert!(*exists.lock().unwrap());
    assert_preserved(&state, url);
    state = run_steps(state, fake_config(), services).await;
    assert_preserved(&state, url);
    let remote = exists.clone();
    let mut allowed = MockSqsApi::new();
    allowed
        .expect_delete_queue()
        .times(1)
        .returning(move |actual| {
            assert_eq!(actual, url);
            *remote.lock().unwrap() = false;
            Ok(())
        });
    state.retry_requested = true;
    state = run_steps(state, fake_config(), provider(Arc::new(allowed))).await;
    assert_eq!(state.status, alien_core::DeploymentStatus::Deleted);
    assert!(!*exists.lock().unwrap());
    println!("manual retry after JSON reload: Deleted; independent remote absent");
}

async fn update_removal_observation(denied: bool, setup: bool) {
    let permission_text =
        include_str!("../../alien-permissions/permission-sets/queue/data-write.jsonc")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
    let permission: PermissionSet = serde_json::from_str(&permission_text).unwrap();
    let profile = PermissionProfile::new().resource("*", ["queue/data-write"]);
    let old_account = ServiceAccount::from_permission_profile("sender-sa".into(), &profile, |_| {
        Some(permission.clone())
    })
    .unwrap();
    let new_account = ServiceAccount::new("sender-sa".into()).build();
    let installed = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", profile))
        .add(old_account.clone(), ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Live)
        .build();
    let target = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", PermissionProfile::new()))
        .add(new_account, ResourceLifecycle::Frozen)
        .build();
    assert!(
        alien_preflights::compatibility::narrowing::service_account_narrowed(
            &target,
            &installed.resources["sender-sa"].config,
            &target.resources["sender-sa"].config
        )
    );
    let url = "https://sqs.us-east-1.amazonaws.com/123456789012/synthetic-jobs";
    let mut stack_state = saved_queue(url, "synthetic-jobs");
    let mut account = StackResourceState::new_pending(
        "service-account".into(),
        Resource::new(old_account.clone()),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    account.status = ResourceStatus::Running;
    account.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsServiceAccountController","_controllerStateVersion":1,"state":"ready","stackPermissionsApplied":true,"roleName":"synthetic-sender","roleArn":"arn:aws:iam::123456789012:role/synthetic-sender"})).unwrap())).unwrap();
    stack_state.resources.insert("sender-sa".into(), account);
    let exists = Arc::new(Mutex::new(true));
    let remote = exists.clone();
    let mut sqs = MockSqsApi::new();
    sqs.expect_delete_queue().times(1).returning(move |actual| {
        assert_eq!(actual, url);
        assert!(*remote.lock().unwrap());
        if denied {
            Err(AlienError::new(
                alien_aws_clients::ErrorData::RemoteAccessDenied {
                    resource_type: "queue".into(),
                    resource_name: "synthetic-jobs".into(),
                },
            ))
        } else {
            *remote.lock().unwrap() = false;
            Ok(())
        }
    });
    let services = provider(Arc::new(sqs));
    let mut state = deployment(stack_state);
    state.status = if setup {
        DeploymentStatus::InitialSetup
    } else {
        DeploymentStatus::Updating
    };
    state.target_release = Some(ReleaseInfo {
        release_id: Some("synthetic-target".into()),
        version: None,
        description: None,
        stack: target.clone(),
    });
    state.runtime_metadata = Some(RuntimeMetadata {
        initial_setup_authority: if setup {
            alien_core::InitialSetupAuthority::DirectSetup
        } else {
            alien_core::InitialSetupAuthority::ImportedHandoff
        },
        prepared_stack: Some(installed),
        pending_prepared_stack: Some(target),
        ..Default::default()
    });
    for _ in 0..32 {
        state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
            .await
            .unwrap()
            .state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if matches!(
            state.status,
            DeploymentStatus::Running
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::InitialSetupFailed
        ) {
            break;
        }
    }
    assert_eq!(*exists.lock().unwrap(), denied);
    let metadata = state.runtime_metadata.as_ref().unwrap();
    assert_eq!(
        state.status,
        if setup {
            DeploymentStatus::InitialSetupFailed
        } else {
            DeploymentStatus::UpdateFailed
        }
    );
    assert_eq!(
        metadata.prepared_stack.as_ref().unwrap().resources["sender-sa"]
            .config
            .downcast_ref::<ServiceAccount>()
            .unwrap(),
        &old_account
    );
    assert!(
        metadata.pending_prepared_stack.as_ref().unwrap().resources["sender-sa"]
            .config
            .downcast_ref::<ServiceAccount>()
            .unwrap()
            .stack_permission_sets
            .is_empty()
    );
    assert!(state.target_release.is_some());
    let resource = &state.stack_state.as_ref().unwrap().resources["jobs"];
    if denied {
        assert_eq!(resource.status, ResourceStatus::DeleteFailed);
        assert_eq!(
            resource.last_failed_state.as_ref().unwrap()["queueUrl"],
            url
        );
    } else {
        assert_eq!(resource.status, ResourceStatus::Deleted);
        assert_eq!(
            state.error.as_ref().unwrap().code,
            "DEPLOYMENT_SETUP_REQUIRED"
        );
    }
}

#[tokio::test]
async fn update_denial_retains_installed_permissions() {
    update_removal_observation(true, false).await;
}
#[tokio::test]
async fn successful_cleanup_requires_setup_before_permission_completion() {
    update_removal_observation(false, false).await;
}

#[tokio::test]
async fn privileged_setup_cannot_narrow_before_denied_live_cleanup() {
    update_removal_observation(true, true).await;
}

fn live_config(dir: &std::path::Path, fixture: &serde_json::Value, kind: &str) -> AwsClientConfig {
    let data: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.join(format!("aws-{kind}.json"))).expect("read scoped credentials"),
    )
    .expect("parse scoped credentials");
    AwsClientConfig {
        account_id: fixture["account"].as_str().unwrap().into(),
        region: fixture["region"].as_str().unwrap().into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: data["AccessKeyId"].as_str().unwrap().into(),
            secret_access_key: data["SecretAccessKey"].as_str().unwrap().into(),
            session_token: Some(data["SessionToken"].as_str().unwrap().into()),
        },
        service_overrides: None,
    }
}
fn live_client(cfg: AwsClientConfig) -> Arc<dyn SqsApi> {
    let mut builder = reqwest::Client::builder();
    if let Ok(path) =
        std::env::var("ALIEN_LIVE_TEST_CA_CERT").or_else(|_| std::env::var("SSL_CERT_FILE"))
    {
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(&std::fs::read(path).unwrap()).unwrap(),
        );
    }
    Arc::new(alien_aws_clients::sqs::SqsClient::new(
        builder.build().unwrap(),
        alien_aws_clients::AwsCredentialProvider::from_config_sync(cfg),
    ))
}

#[tokio::test]
#[ignore = "requires an explicitly supplied disposable queue and scoped denial/observer credentials"]
async fn live_denied_delete_preserves_checkpoint() {
    let dir = std::path::PathBuf::from(std::env::var("ALIEN_DELETE_TEST_DIR").unwrap());
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("fixture.json")).unwrap()).unwrap();
    let url = fixture["queueUrl"].as_str().unwrap();
    let name = fixture["queueName"].as_str().unwrap();
    let denied = live_config(&dir, &fixture, "runtime-denied");
    let observer = live_client(live_config(&dir, &fixture, "runtime-allowed"));
    let observed = observer
        .get_queue_attributes(
            url,
            alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
                .attribute_names(vec!["QueueArn".into()])
                .build(),
        )
        .await;
    assert!(
        observed.is_ok(),
        "independent observer must find the fixture before deletion"
    );
    let started = std::time::Instant::now();
    let state = run_steps(
        deployment(saved_queue(url, name)),
        ClientConfig::Aws(Box::new(denied.clone())),
        provider(live_client(denied)),
    )
    .await;
    let elapsed = started.elapsed();
    assert_preserved(&state, url);
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "permanent denial should not exhaust retry backoff"
    );
    let observed = observer
        .get_queue_attributes(
            url,
            alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
                .attribute_names(vec!["QueueArn".into()])
                .build(),
        )
        .await;
    assert!(
        observed.is_ok(),
        "independent observer must still find the fixture after denied deletion"
    );
    println!(
        "live denial retained checkpoint and remote object; elapsed_ms={}",
        elapsed.as_millis()
    );
}

async fn setup_scoped_grant_cleanup(
    legacy_capture: bool,
    removed_profile: bool,
    legacy_checkpoint: Option<u32>,
    pruned: bool,
    frozen: bool,
) {
    let permission_text =
        include_str!("../../alien-permissions/permission-sets/queue/data-write.jsonc")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
    let permission: PermissionSet = serde_json::from_str(&permission_text).unwrap();
    let mut custom = permission.clone();
    custom.id = "queue/custom-send".to_string();
    let profile = PermissionProfile::new().resource(
        "jobs",
        [
            alien_core::permissions::PermissionSetReference::from_name("queue/data-write"),
            alien_core::permissions::PermissionSetReference::Inline(custom),
        ],
    );
    let mut old_account =
        ServiceAccount::from_permission_profile("sender-sa".into(), &profile, |_| {
            Some(permission.clone())
        })
        .unwrap();
    if legacy_capture {
        old_account.resource_permission_sets.clear();
    }
    let mut installed = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", profile))
        .add(old_account.clone(), ResourceLifecycle::Frozen)
        .add(
            Queue::new("jobs".into()).build(),
            if frozen {
                ResourceLifecycle::Frozen
            } else {
                ResourceLifecycle::Live
            },
        )
        .build();
    if frozen {
        installed
            .resources
            .insert("other-jobs".into(), installed.resources["jobs"].clone());
        installed.resources.get_mut("other-jobs").unwrap().config =
            Resource::new(Queue::new("other-jobs".into()).build());
        installed.resources.insert(
            "removed-sa".into(),
            installed.resources["sender-sa"].clone(),
        );
        installed.resources.get_mut("removed-sa").unwrap().config =
            Resource::new(ServiceAccount::new("removed-sa".into()).build());
    }
    let desired = Stack::new("synthetic".into())
        .permissions(if removed_profile {
            PermissionsConfig::new()
        } else {
            PermissionsConfig::new().with_profile("sender", PermissionProfile::new())
        })
        .add(
            ServiceAccount::new("sender-sa".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let mut stack_state = saved_queue("synthetic-url", "synthetic-jobs");
    if frozen {
        stack_state.resources.get_mut("jobs").unwrap().lifecycle = Some(ResourceLifecycle::Frozen);
    }
    let mut account = StackResourceState::new_pending(
        "service-account".into(),
        Resource::new(old_account),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    account.status = ResourceStatus::Running;
    account.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsServiceAccountController","_controllerStateVersion":1,"state":"ready","stackPermissionsApplied":true,"roleName":"synthetic-sender","roleArn":"arn:aws:iam::123456789012:role/synthetic-sender"})).unwrap())).unwrap();
    stack_state.resources.insert("sender-sa".into(), account);
    let removed_role_exists = Arc::new(Mutex::new(frozen));
    if frozen {
        let mut other = stack_state.resources["jobs"].clone();
        other.config = installed.resources["other-jobs"].config.clone();
        other.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsQueueController", "_controllerStateVersion":1, "state":"ready", "queueUrl":"synthetic-other-url", "queueName":"synthetic-other-jobs"})).unwrap())).unwrap();
        stack_state.resources.insert("other-jobs".into(), other);
        let mut removed = stack_state.resources["sender-sa"].clone();
        removed.config = installed.resources["removed-sa"].config.clone();
        removed.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsServiceAccountController", "_controllerStateVersion":1, "state":"ready", "stackPermissionsApplied":true, "roleName":"synthetic-removed", "roleArn":"arn:aws:iam::123456789012:role/synthetic-removed"})).unwrap())).unwrap();
        stack_state.resources.insert("removed-sa".into(), removed);
    }
    let current = "deployment-jobs-queue-data-write".to_string();
    let legacy = "alien-jobs-queue-data-write".to_string();
    let policies = Arc::new(Mutex::new(vec![
        "deployment-permissions".into(),
        current,
        legacy,
        "deployment-jobs-queue-custom-send".into(),
        "alien-jobs-queue-custom-send".into(),
        "unrelated-metadata-control".into(),
    ]));
    let exists = Arc::new(Mutex::new(true));
    let setup_started = Arc::new(Mutex::new(!frozen));
    let setup = setup_started.clone();
    let remote = exists.clone();
    let mut sqs = MockSqsApi::new();
    sqs.expect_delete_queue()
        .times(if frozen { 2 } else { 1 })
        .returning(move |_| {
            assert!(
                *setup.lock().unwrap(),
                "runtime must never delete a Frozen queue"
            );
            *remote.lock().unwrap() = false;
            Ok(())
        });
    let mut iam = MockIamApi::new();
    if frozen {
        iam.expect_list_attached_role_policies().returning(|role| {
            assert_eq!(role, "synthetic-removed");
            Ok(serde_json::from_value(json!({"ListAttachedRolePoliciesResult": {"AttachedPolicies": {"member": []}, "IsTruncated": false}})).unwrap())
        });
        let setup = setup_started.clone();
        let removed = removed_role_exists.clone();
        iam.expect_delete_role().times(1).returning(move |role| {
            assert!(
                *setup.lock().unwrap(),
                "runtime must never delete a Frozen role"
            );
            assert_eq!(role, "synthetic-removed");
            *removed.lock().unwrap() = false;
            Ok(())
        });
    }
    let remote = exists.clone();
    iam.expect_update_assume_role_policy()
        .times(1)
        .returning(move |_, _| {
            assert!(
                frozen || !*remote.lock().unwrap(),
                "no IAM mutation before live cleanup"
            );
            Ok(())
        });
    let retained = policies.clone();
    let setup = setup_started.clone();
    let remote = exists.clone();
    iam.expect_delete_role_policy()
        .returning(move |role, name| {
            assert!(
                *setup.lock().unwrap(),
                "runtime must never delete Frozen role policies"
            );
            if role == "synthetic-removed" {
                return Ok(());
            }
            assert_eq!(role, "synthetic-sender");
            assert!(
                frozen || !*remote.lock().unwrap(),
                "no policy removal before live cleanup"
            );
            assert_ne!(name, "unrelated-metadata-control");
            retained.lock().unwrap().retain(|policy| policy != name);
            Ok(())
        });
    let retained = policies.clone();
    iam.expect_list_role_policies().returning(move |role| {
        Ok(ListRolePoliciesResponse {
            list_role_policies_result: ListRolePoliciesResult {
                policy_names: Some(PolicyNames {
                    member: if role == "synthetic-removed" {
                        vec!["deployment-permissions".into()]
                    } else {
                        retained.lock().unwrap().clone()
                    },
                }),
                is_truncated: Some(false),
                marker: None,
            },
        })
    });
    let sqs = Arc::new(sqs);
    let iam = Arc::new(iam);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    services
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    let services = Arc::new(services);
    let mut state = deployment(stack_state);
    state.status = DeploymentStatus::InitialSetup;
    state.target_release = Some(ReleaseInfo {
        release_id: Some("desired".into()),
        version: None,
        description: None,
        stack: desired.clone(),
    });
    state.runtime_metadata = Some(RuntimeMetadata {
        initial_setup_authority: InitialSetupAuthority::DirectSetup,
        prepared_stack: Some(installed),
        pending_prepared_stack: Some(desired),
        ..Default::default()
    });
    if frozen {
        state.status = DeploymentStatus::Updating;
        for _ in 0..32 {
            state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
                .await
                .unwrap()
                .state;
            state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
            if matches!(
                state.status,
                DeploymentStatus::UpdateFailed | DeploymentStatus::Running
            ) {
                break;
            }
        }
        assert_eq!(state.status, DeploymentStatus::UpdateFailed);
        assert_eq!(
            state.error.as_ref().unwrap().code,
            "DEPLOYMENT_SETUP_REQUIRED"
        );
        assert!(*exists.lock().unwrap());
        assert!(*removed_role_exists.lock().unwrap());
        let saved = state.stack_state.as_ref().unwrap();
        assert_eq!(
            saved.resources["jobs"].internal_state.as_ref().unwrap()["queueUrl"],
            "synthetic-url"
        );
        assert_eq!(
            saved.resources["other-jobs"]
                .internal_state
                .as_ref()
                .unwrap()["queueUrl"],
            "synthetic-other-url"
        );
        assert_eq!(
            saved.resources["removed-sa"]
                .internal_state
                .as_ref()
                .unwrap()["roleName"],
            "synthetic-removed"
        );
        state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
            .await
            .unwrap()
            .state;
        assert_eq!(
            state.status,
            DeploymentStatus::UpdateFailed,
            "no implicit retry"
        );
        *setup_started.lock().unwrap() = true;
        state.status = DeploymentStatus::InitialSetup;
        state.error = None;
    }
    if pruned {
        // The prepared target excludes a declined gate while the release still
        // declares it. Exercise the real updater's deletion and pruning path.
        let mut declared = state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .clone()
            .unwrap();
        declared.resources.get_mut("jobs").unwrap().enabled_when = Some("queueEnabled".into());
        state.target_release.as_mut().unwrap().stack = declared;
        state.status = DeploymentStatus::Updating;
        for _ in 0..32 {
            state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
                .await
                .unwrap()
                .state;
            state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
            if state.status == DeploymentStatus::UpdateFailed {
                break;
            }
        }
        assert_eq!(state.status, DeploymentStatus::UpdateFailed);
        assert_eq!(
            state.error.as_ref().unwrap().code,
            "DEPLOYMENT_SETUP_REQUIRED"
        );
        assert!(!state
            .stack_state
            .as_ref()
            .unwrap()
            .resources
            .contains_key("jobs"));
        assert!(!*exists.lock().unwrap());
        state.status = DeploymentStatus::InitialSetup;
        state.error = None;
    }
    if let Some(version) = legacy_checkpoint {
        // Produced by the previous executor's failed update followed by its
        // real direct-setup preparation, before any new-format metadata existed.
        state = serde_json::from_str(include_str!("fixtures/legacy_cleanup_setup.json")).unwrap();
        state.protocol_version = version;
        state.status = DeploymentStatus::InitialSetupFailed;
        state.retry_requested = true;
        state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
            .await
            .unwrap()
            .state;
    }
    for _ in 0..32 {
        state = alien_deployment::step(state, config(), fake_config(), Some(services.clone()))
            .await
            .unwrap()
            .state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if state.status == DeploymentStatus::Provisioning {
            break;
        }
        assert_ne!(state.status, DeploymentStatus::InitialSetupFailed);
    }
    assert_eq!(state.status, DeploymentStatus::Provisioning);
    if legacy_checkpoint.is_some() {
        assert_eq!(
            state.protocol_version,
            alien_core::CLEANUP_SETUP_DEPLOYMENT_PROTOCOL_VERSION
        );
    }
    assert!(!*exists.lock().unwrap());
    assert!(!*removed_role_exists.lock().unwrap());
    assert_eq!(*policies.lock().unwrap(), ["unrelated-metadata-control"]);
    assert!(state
        .runtime_metadata
        .unwrap()
        .pending_prepared_stack
        .is_none());
}

#[tokio::test]
async fn setup_drains_live_resources_then_removes_current_and_legacy_owned_grants() {
    setup_scoped_grant_cleanup(false, false, None, false, false).await;
}

#[tokio::test]
async fn setup_reconciles_legacy_uncaptured_inline_grants() {
    setup_scoped_grant_cleanup(true, false, None, false, false).await;
}

#[tokio::test]
async fn setup_removes_profile_grants_from_a_retained_identity() {
    setup_scoped_grant_cleanup(false, true, None, false, false).await;
}

async fn live_until(
    mut state: alien_core::DeploymentState,
    cfg: ClientConfig,
    services: Arc<MockPlatformServiceProvider>,
    terminal: alien_core::DeploymentStatus,
) -> alien_core::DeploymentState {
    for _ in 0..64 {
        let result =
            alien_deployment::step(state, config(), cfg.clone(), Some(services.clone())).await;
        if let Err(error) = &result {
            let dir = std::path::PathBuf::from(std::env::var("ALIEN_DELETE_TEST_DIR").unwrap());
            std::fs::write(
                dir.join("live-step-error.json"),
                serde_json::to_vec(error).unwrap(),
            )
            .unwrap();
            eprintln!("live deployment step error code={}", error.code);
        }
        assert!(result.is_ok(), "live deployment step returned an error");
        let result = result.unwrap();
        state = serde_json::from_slice(&serde_json::to_vec(&result.state).unwrap()).unwrap();
        if state.status == terminal {
            return state;
        }
        assert!(
            !matches!(
                state.status,
                alien_core::DeploymentStatus::InitialSetupFailed
                    | alien_core::DeploymentStatus::ProvisioningFailed
            ),
            "live setup failed"
        );
        if let Some(delay) = result.suggested_delay_ms {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
    }
    panic!("live deployment did not reach the expected terminal status");
}

async fn assert_live_queue_absent(client: &dyn SqsApi, url: &str) {
    // DeleteQueue is asynchronous and may take up to 60 seconds. Only the
    // provider's explicit absence response satisfies the observer assertion.
    tokio::time::timeout(std::time::Duration::from_secs(65), async {
        loop {
            match client
                .get_queue_attributes(
                    url,
                    alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
                        .attribute_names(vec!["QueueArn".into()])
                        .build(),
                )
                .await
            {
                Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => return,
                Err(error) => panic!("absence observer failed: code={}", error.code),
                Ok(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
            }
        }
    })
    .await
    .expect("observer must confirm queue absence within the deletion window");
}

#[tokio::test]
#[ignore = "mutates only an explicitly supplied disposable queue and sender role; requires scoped setup and preissued workload credentials"]
async fn live_setup_removes_permissions_after_cleanup() {
    let dir = std::path::PathBuf::from(std::env::var("ALIEN_DELETE_TEST_DIR").unwrap());
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("fixture.json")).unwrap()).unwrap();
    let url = fixture["queueUrl"].as_str().unwrap();
    let name = fixture["queueName"].as_str().unwrap();
    let role = fixture["senderRoleName"].as_str().unwrap();
    let setup_cfg = live_config(&dir, &fixture, "setup");
    let allowed_cfg = live_config(&dir, &fixture, "runtime-allowed");
    let allowed = live_client(allowed_cfg.clone());
    let workload = live_client(live_config(&dir, &fixture, "workload"));
    let iam: Arc<dyn IamApi> = Arc::new(IamClient::new(
        reqwest::Client::new(),
        alien_aws_clients::AwsCredentialProvider::from_config_sync(setup_cfg.clone()),
    ));
    let queue_api = allowed.clone();
    let iam_api = iam.clone();
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(queue_api.clone()));
    services
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam_api.clone()));
    let services = Arc::new(services);
    let permission_text =
        include_str!("../../alien-permissions/permission-sets/queue/data-write.jsonc")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
    let permission: PermissionSet = serde_json::from_str(&permission_text).unwrap();
    let profile = PermissionProfile::new()
        .resource("*", ["queue/data-write"])
        .resource("jobs", ["queue/data-write"]);
    let old_account = ServiceAccount::from_permission_profile("sender-sa".into(), &profile, |_| {
        Some(permission.clone())
    })
    .unwrap();
    let empty_account = ServiceAccount::new("sender-sa".into()).build();
    let old = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", profile))
        .add(old_account, ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Live)
        .build();
    let empty = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", PermissionProfile::new()))
        .add(empty_account.clone(), ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Live)
        .build();
    let desired = Stack::new("synthetic".into())
        .permissions(PermissionsConfig::new().with_profile("sender", PermissionProfile::new()))
        .add(empty_account.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut stack_state = saved_queue(url, name);
    stack_state.resource_prefix = fixture["prefix"].as_str().unwrap().into();
    let mut account = StackResourceState::new_pending(
        "service-account".into(),
        Resource::new(empty_account),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    account.status = ResourceStatus::Running;
    account.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsServiceAccountController","_controllerStateVersion":1,"state":"ready","stackPermissionsApplied":true,"roleName":role,"roleArn":fixture["senderRoleArn"]})).unwrap())).unwrap();
    stack_state.resources.insert("sender-sa".into(), account);
    // Match the canonical installed definitions that real deployment preflights
    // produce. Retry runs those preflights again before advancing the checkpoint.
    let runner = alien_preflights::runner::PreflightRunner::new();
    let old = runner
        .apply_mutations(old, &stack_state, &config())
        .await
        .unwrap();
    let empty = runner
        .apply_mutations(empty, &stack_state, &config())
        .await
        .unwrap();
    let desired = runner
        .apply_mutations(desired, &stack_state, &config())
        .await
        .unwrap();
    let mut state = deployment(stack_state);
    state.status = DeploymentStatus::InitialSetup;
    state.target_release = Some(ReleaseInfo {
        release_id: Some("installed".into()),
        version: None,
        description: None,
        stack: old.clone(),
    });
    state.runtime_metadata = Some(RuntimeMetadata {
        initial_setup_authority: InitialSetupAuthority::DirectSetup,
        prepared_stack: Some(empty),
        pending_prepared_stack: Some(old),
        ..Default::default()
    });
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(setup_cfg.clone())),
        services.clone(),
        DeploymentStatus::Provisioning,
    )
    .await;
    // Historical per-resource policies are seeded explicitly alongside the real
    // controller-installed stack policy, so the proof covers both ownership names.
    let historical = json!({"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:SendMessage","Resource":fixture["queueArn"]}]}).to_string();
    for policy in [
        "deployment-jobs-queue-data-write",
        "alien-jobs-queue-data-write",
    ] {
        assert!(
            iam.put_role_policy(role, policy, &historical).await.is_ok(),
            "install exact historical fixture grant"
        );
    }
    let mut positive = false;
    for _ in 0..24 {
        if workload
            .send_message(
                url,
                SendMessageRequest::builder()
                    .message_body("before cleanup".into())
                    .build(),
            )
            .await
            .is_ok()
        {
            positive = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    assert!(
        positive,
        "preissued workload session must send before removal"
    );
    println!("actual InitialSetup installed stack permissions; unchanged workload Send succeeded");
    state.current_release = state.target_release.take();
    state.target_release = Some(ReleaseInfo {
        release_id: Some("desired".into()),
        version: None,
        description: None,
        stack: desired.clone(),
    });
    state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .pending_prepared_stack = Some(desired);
    state.status = DeploymentStatus::Updating;
    let denied_cfg = live_config(&dir, &fixture, "runtime-denied");
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(denied_cfg.clone())),
        provider(live_client(denied_cfg.clone())),
        DeploymentStatus::UpdateFailed,
    )
    .await;
    assert_eq!(
        state.stack_state.as_ref().unwrap().resources["jobs"].status,
        ResourceStatus::DeleteFailed
    );
    std::fs::write(
        dir.join("live-update-failed.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    state = serde_json::from_slice(&std::fs::read(dir.join("live-update-failed.json")).unwrap())
        .unwrap();
    state = alien_deployment::step(
        state,
        config(),
        ClientConfig::Aws(Box::new(denied_cfg.clone())),
        Some(provider(live_client(denied_cfg))),
    )
    .await
    .unwrap()
    .state;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);
    state.retry_requested = true;
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(allowed_cfg.clone())),
        provider(allowed.clone()),
        DeploymentStatus::UpdateFailed,
    )
    .await;
    assert_eq!(
        state.error.as_ref().unwrap().code,
        "DEPLOYMENT_SETUP_REQUIRED"
    );
    assert_eq!(
        state.stack_state.as_ref().unwrap().resources["jobs"].status,
        ResourceStatus::Deleted
    );
    assert_live_queue_absent(allowed.as_ref(), url).await;
    println!("runtime removed queue and retained setup obligation; independent observer confirmed absence");
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    assert!(
        allowed
            .create_queue(
                CreateQueueRequest::builder()
                    .queue_name(name.into())
                    .build()
            )
            .await
            .is_ok(),
        "recreate only the exact disposable fixture after AWS deletion window"
    );
    assert!(
        workload
            .send_message(
                url,
                SendMessageRequest::builder()
                    .message_body("stale authority control".into())
                    .build()
            )
            .await
            .is_ok(),
        "same session still holds grants before setup"
    );
    state.status = DeploymentStatus::InitialSetup;
    state.error = None;
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(setup_cfg)),
        services,
        DeploymentStatus::Provisioning,
    )
    .await;
    assert!(state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .is_none());
    let policies = iam.list_role_policies(role).await;
    assert!(policies.is_ok(), "inspect only fixture role policies");
    let policies = policies
        .unwrap()
        .list_role_policies_result
        .policy_names
        .unwrap()
        .member;
    assert!(policies.contains(&"unrelated-metadata-control".to_string()));
    for removed in [
        "deployment-permissions",
        "deployment-jobs-queue-data-write",
        "alien-jobs-queue-data-write",
    ] {
        assert!(!policies.iter().any(|policy| policy == removed));
    }
    let mut denied = false;
    for _ in 0..24 {
        let result = workload
            .send_message(
                url,
                SendMessageRequest::builder()
                    .message_body("after setup".into())
                    .build(),
            )
            .await;
        match result {
            Ok(_) => println!(
                "negative probe: Send still succeeds at {}",
                chrono::Utc::now()
            ),
            Err(error) => {
                println!(
                    "negative probe: code={} http={:?} at {}",
                    error.code,
                    error.http_status_code,
                    chrono::Utc::now()
                );
                if error.code == "REMOTE_ACCESS_DENIED" {
                    denied = true;
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    assert!(denied, "same preissued session must lose Send after setup");
    assert!(
        workload
            .get_queue_url(
                GetQueueUrlRequest::builder()
                    .queue_name(name.into())
                    .build()
            )
            .await
            .is_ok(),
        "unrelated metadata grant remains usable"
    );
    assert!(
        allowed.delete_queue(url).await.is_ok(),
        "clean up exact disposable queue"
    );
    assert_live_queue_absent(allowed.as_ref(), url).await;
    println!("actual InitialSetup removed stack/current/legacy grants; unchanged session Send denied; unrelated GetQueueUrl succeeded; queue cleanup confirmed");
}

#[tokio::test]
#[ignore = "requires explicitly supplied workload credentials and disposable queue"]
async fn live_workload_permission_probe() {
    let dir = std::path::PathBuf::from(std::env::var("ALIEN_DELETE_TEST_DIR").unwrap());
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("fixture.json")).unwrap()).unwrap();
    let workload = live_client(live_config(&dir, &fixture, "workload"));
    let result = workload
        .send_message(
            fixture["queueUrl"].as_str().unwrap(),
            alien_aws_clients::sqs::SendMessageRequest::builder()
                .message_body("negative control".into())
                .build(),
        )
        .await;
    match result {
        Ok(_) => panic!("workload retained Send permission"),
        Err(error) => {
            println!(
                "workload Send error_code={} http_status={:?}",
                error.code, error.http_status_code
            );
            assert_eq!(error.code, "REMOTE_ACCESS_DENIED");
        }
    }
}

#[tokio::test]
async fn setup_resumes_legacy_prepared_desired_and_stale_pending_checkpoint() {
    setup_scoped_grant_cleanup(false, false, Some(1), false, false).await;
}

#[tokio::test]
async fn future_protocol_is_rejected_before_any_provider_access() {
    let mut state = deployment(StackState::new(Platform::Aws));
    state.status = alien_core::DeploymentStatus::InitialSetup;
    state.protocol_version = alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION + 1;
    let version = state.protocol_version;
    let error = alien_deployment::step(
        state,
        config(),
        fake_config(),
        Some(Arc::new(MockPlatformServiceProvider::new())),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INCOMPATIBLE_DEPLOYMENT_PROTOCOL");
    println!("protocol {version} rejected before provider access");
}

#[tokio::test]
async fn unchanged_running_protocol_one_remains_usable_without_upgrade() {
    let stack = alien_core::Stack::new("synthetic".into()).build();
    let mut state = deployment(StackState::new(Platform::Test));
    state.platform = Platform::Test;
    state.status = alien_core::DeploymentStatus::Running;
    state.protocol_version = 1;
    state.runtime_metadata = Some(alien_core::RuntimeMetadata {
        prepared_stack: Some(stack),
        ..Default::default()
    });
    let result = alien_deployment::step(
        state,
        config(),
        ClientConfig::Test,
        Some(Arc::new(MockPlatformServiceProvider::new())),
    )
    .await
    .unwrap();
    assert_eq!(result.state.status, alien_core::DeploymentStatus::Running);
    assert_eq!(result.state.protocol_version, 1);
}

#[tokio::test]
async fn setup_removes_historical_grants_after_deleted_resource_record_was_pruned() {
    setup_scoped_grant_cleanup(false, false, None, true, false).await;
}

#[tokio::test]
async fn setup_resumes_protocol_two_legacy_setup_representation() {
    setup_scoped_grant_cleanup(false, false, Some(2), false, false).await;
}

#[tokio::test]
async fn runtime_preserves_removed_frozen_resources_for_privileged_setup() {
    setup_scoped_grant_cleanup(false, false, None, false, true).await;
}

#[tokio::test]
#[ignore = "requires two disposable Frozen queues, removed/retained roles and exact scoped credentials"]
async fn live_frozen_removal_requires_setup_and_preserves_owned_grants() {
    let dir = std::path::PathBuf::from(std::env::var("ALIEN_DELETE_TEST_DIR").unwrap());
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("fixture.json")).unwrap()).unwrap();
    let queues = fixture["queues"].as_array().unwrap();
    assert_eq!(queues.len(), 2);
    let setup_cfg = live_config(&dir, &fixture, "setup");
    let runtime_cfg = live_config(&dir, &fixture, "runtime-denied");
    let observer = live_client(live_config(&dir, &fixture, "observer"));
    let setup_sqs = live_client(setup_cfg.clone());
    let mut http = reqwest::Client::builder();
    if let Ok(path) =
        std::env::var("ALIEN_LIVE_TEST_CA_CERT").or_else(|_| std::env::var("SSL_CERT_FILE"))
    {
        http = http.add_root_certificate(
            reqwest::Certificate::from_pem(&std::fs::read(path).unwrap()).unwrap(),
        );
    }
    let iam: Arc<dyn IamApi> = Arc::new(IamClient::new(
        http.build().unwrap(),
        alien_aws_clients::AwsCredentialProvider::from_config_sync(setup_cfg.clone()),
    ));
    let role = fixture["senderRoleName"].as_str().unwrap();
    let removed_role = fixture["removedRoleName"].as_str().unwrap();
    let permission_text =
        include_str!("../../alien-permissions/permission-sets/queue/data-write.jsonc")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
    let permission: PermissionSet = serde_json::from_str(&permission_text).unwrap();
    let profile = PermissionProfile::new()
        .resource("jobs", ["queue/data-write"])
        .resource("other-jobs", ["queue/data-write"]);
    let account = ServiceAccount::from_permission_profile("sender-sa".into(), &profile, |_| {
        Some(permission.clone())
    })
    .unwrap();
    let removed_account = ServiceAccount::new("removed-sa".into()).build();
    let installed = Stack::new("frozen-cleanup".into())
        .permissions(PermissionsConfig::new().with_profile("sender", profile))
        .add(account.clone(), ResourceLifecycle::Frozen)
        .add(removed_account.clone(), ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Queue::new("other-jobs".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let desired = Stack::new("frozen-cleanup".into())
        .permissions(PermissionsConfig::new().with_profile("sender", PermissionProfile::new()))
        .add(
            ServiceAccount::new("sender-sa".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let mut saved = StackState::new(Platform::Aws);
    saved.resource_prefix = fixture["prefix"].as_str().unwrap().into();
    for (id, queue) in ["jobs", "other-jobs"].into_iter().zip(queues) {
        let mut record = saved_queue(
            queue["url"].as_str().unwrap(),
            queue["name"].as_str().unwrap(),
        )
        .resources
        .remove("jobs")
        .unwrap();
        record.config = installed.resources[id].config.clone();
        record.lifecycle = Some(ResourceLifecycle::Frozen);
        saved.resources.insert(id.into(), record);
        observer
            .get_queue_url(
                alien_aws_clients::sqs::GetQueueUrlRequest::builder()
                    .queue_name(queue["name"].as_str().unwrap().into())
                    .build(),
            )
            .await
            .expect("independent observer sees initial queue");
    }
    for (id, config, name, arn) in [
        ("sender-sa", account, role, &fixture["senderRoleArn"]),
        (
            "removed-sa",
            removed_account,
            removed_role,
            &fixture["removedRoleArn"],
        ),
    ] {
        let mut record = StackResourceState::new_pending(
            "service-account".into(),
            Resource::new(config),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        record.status = ResourceStatus::Running;
        record.set_internal_controller(Some(deserialize_controller(json!({"type":"AwsServiceAccountController","_controllerStateVersion":1,"state":"ready","stackPermissionsApplied":true,"roleName":name,"roleArn":arn})).unwrap())).unwrap();
        saved.resources.insert(id.into(), record);
    }
    // Historical grants are exact fixture policies, not discovered or adopted by prefix.
    for (id, queue) in ["jobs", "other-jobs"].into_iter().zip(queues) {
        let document = json!({"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:SendMessage","Resource":queue["arn"]}]}).to_string();
        for name in [
            format!("deployment-{id}-queue-data-write"),
            format!("alien-{id}-queue-data-write"),
        ] {
            iam.put_role_policy(role, &name, &document)
                .await
                .expect("seed exact historical grant");
        }
    }
    let mut state = deployment(saved);
    state.status = DeploymentStatus::Updating;
    state.target_release = Some(ReleaseInfo {
        release_id: Some("desired".into()),
        version: None,
        description: None,
        stack: desired.clone(),
    });
    state.runtime_metadata = Some(RuntimeMetadata {
        initial_setup_authority: InitialSetupAuthority::DirectSetup,
        prepared_stack: Some(installed),
        pending_prepared_stack: Some(desired),
        ..Default::default()
    });
    // A runtime provider with no mutation clients makes accidental Frozen access a hard failure.
    let mut runtime = MockPlatformServiceProvider::new();
    runtime.expect_runtime_setup_authority().returning(|_| None);
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(runtime_cfg.clone())),
        Arc::new(runtime),
        DeploymentStatus::UpdateFailed,
    )
    .await;
    assert_eq!(
        state.error.as_ref().unwrap().code,
        "DEPLOYMENT_SETUP_REQUIRED"
    );
    std::fs::write(
        dir.join("frozen-setup-required.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    state = serde_json::from_slice(&std::fs::read(dir.join("frozen-setup-required.json")).unwrap())
        .unwrap();
    let saved = state.stack_state.as_ref().unwrap();
    for (id, queue) in ["jobs", "other-jobs"].into_iter().zip(queues) {
        assert_eq!(
            saved.resources[id].internal_state.as_ref().unwrap()["queueUrl"],
            queue["url"]
        );
        observer
            .get_queue_url(
                alien_aws_clients::sqs::GetQueueUrlRequest::builder()
                    .queue_name(queue["name"].as_str().unwrap().into())
                    .build(),
            )
            .await
            .expect("runtime preserved queue");
    }
    assert_eq!(
        saved.resources["removed-sa"]
            .internal_state
            .as_ref()
            .unwrap()["roleName"],
        removed_role
    );
    iam.get_role(removed_role)
        .await
        .expect("runtime preserved removed role");
    state = alien_deployment::step(
        state,
        config(),
        ClientConfig::Aws(Box::new(runtime_cfg)),
        Some(Arc::new(MockPlatformServiceProvider::new())),
    )
    .await
    .unwrap()
    .state;
    assert_eq!(
        state.status,
        DeploymentStatus::UpdateFailed,
        "no implicit retry"
    );
    println!("runtime made no Frozen mutations; both queue IDs and removed role remain tracked; setup required persists across disk reload");
    let mut setup = MockPlatformServiceProvider::new();
    let sqs = setup_sqs.clone();
    setup
        .expect_get_aws_sqs_client()
        .returning(move |_| Ok(sqs.clone()));
    let iam_client = iam.clone();
    setup
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam_client.clone()));
    state.status = DeploymentStatus::InitialSetup;
    state.error = None;
    state = live_until(
        state,
        ClientConfig::Aws(Box::new(setup_cfg)),
        Arc::new(setup),
        DeploymentStatus::Provisioning,
    )
    .await;
    for queue in queues {
        assert_live_queue_absent(observer.as_ref(), queue["url"].as_str().unwrap()).await;
    }
    let missing = iam
        .get_role(removed_role)
        .await
        .expect_err("removed role must be absent");
    assert_eq!(missing.code, "REMOTE_RESOURCE_NOT_FOUND");
    let names = iam
        .list_role_policies(role)
        .await
        .unwrap()
        .list_role_policies_result
        .policy_names
        .unwrap()
        .member;
    assert!(names.contains(&"unrelated-metadata-control".to_string()));
    for id in ["jobs", "other-jobs"] {
        for name in [
            format!("deployment-{id}-queue-data-write"),
            format!("alien-{id}-queue-data-write"),
        ] {
            assert!(!names.contains(&name));
        }
    }
    assert!(state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .is_none());
    std::fs::write(
        dir.join("frozen-setup-complete.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    println!("privileged setup deleted both Frozen queues and role; owned historical grants removed; unrelated policy preserved");
}
