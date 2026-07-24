use std::collections::HashMap;

use alien_core::{
    ExternalBinding, ExternalBindings, Platform, Resource, Stack, StackResourceState,
    StackSettings, StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;

use super::*;
use crate::auth::Subject;
use crate::traits::{CreateReleaseParams, ReleaseRecord};

#[derive(Default)]
struct StubReleaseStore {
    releases: HashMap<String, ReleaseRecord>,
}

#[async_trait]
impl ReleaseStore for StubReleaseStore {
    async fn create_release(
        &self,
        caller: &Subject,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError> {
        Ok(ReleaseRecord {
            id: "created-release".to_string(),
            workspace_id: caller.workspace_id.clone(),
            project_id: params.project_id,
            stacks: params.stacks,
            git_commit_sha: params.git_commit_sha,
            git_commit_ref: params.git_commit_ref,
            git_commit_message: params.git_commit_message,
            created_at: Utc::now(),
        })
    }

    async fn get_release(
        &self,
        _caller: &Subject,
        id: &str,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
        Ok(self.releases.get(id).cloned())
    }

    async fn get_latest_release(
        &self,
        _caller: &Subject,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
        Ok(self.releases.values().next().cloned())
    }

    async fn list_releases(&self, _caller: &Subject) -> Result<Vec<ReleaseRecord>, AlienError> {
        Ok(self.releases.values().cloned().collect())
    }
}

fn stack_state_with_resource(
    resource_type: &str,
    lifecycle: Option<ResourceLifecycle>,
    status: ResourceStatus,
    remote_binding_params: Option<serde_json::Value>,
) -> StackState {
    let mut stack_state = StackState::new(Platform::Aws);
    stack_state.resources.insert(
        "files".to_string(),
        StackResourceState::builder()
            .resource_type(resource_type.to_string())
            .status(status)
            .config(Resource::new(Storage {
                id: "files".to_string(),
                public_read: false,
                versioning: false,
                lifecycle_rules: Vec::new(),
            }))
            .maybe_lifecycle(lifecycle)
            .maybe_remote_binding_params(remote_binding_params)
            .dependencies(Vec::new())
            .build(),
    );
    stack_state
}

fn deployment(stack_state: StackState) -> DeploymentRecord {
    deployment_on_platform(stack_state, Platform::Aws)
}

fn deployment_on_platform(stack_state: StackState, platform: Platform) -> DeploymentRecord {
    DeploymentRecord {
        id: "deployment".to_string(),
        workspace_id: "default".to_string(),
        project_id: "default".to_string(),
        name: "deployment".to_string(),
        deployment_group_id: "group".to_string(),
        platform,
        deployment_protocol_version: 1,
        base_platform: None,
        status: "running".to_string(),
        stack_settings: None,
        stack_state: Some(stack_state),
        environment_info: None,
        runtime_metadata: None,
        current_release_id: None,
        desired_release_id: None,
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

fn storage() -> Storage {
    Storage {
        id: "files".to_string(),
        public_read: false,
        versioning: false,
        lifecycle_rules: Vec::new(),
    }
}

fn storage_stack(remote_access: bool) -> Stack {
    let builder = Stack::new("stack".to_string());
    if remote_access {
        builder
            .add_with_remote_access(storage(), ResourceLifecycle::Frozen)
            .build()
    } else {
        builder.add(storage(), ResourceLifecycle::Frozen).build()
    }
}

fn release(id: &str, platform: Platform, stack: Stack) -> ReleaseRecord {
    ReleaseRecord {
        id: id.to_string(),
        workspace_id: "default".to_string(),
        project_id: "default".to_string(),
        stacks: HashMap::from([(platform, stack)]),
        git_commit_sha: None,
        git_commit_ref: None,
        git_commit_message: None,
        created_at: Utc::now(),
    }
}

fn lease(client_config: ClientConfig) -> MaterializedCredentialLease {
    MaterializedCredentialLease {
        client_config,
        expires_at: Utc::now() + chrono::Duration::minutes(15),
    }
}

fn azure_sas_parameters() -> HashMap<String, String> {
    HashMap::from([
        (
            "sp".to_string(),
            AZURE_REMOTE_STORAGE_PERMISSIONS.to_string(),
        ),
        ("st".to_string(), "2030-01-01T00:00:00Z".to_string()),
        ("se".to_string(), "2030-01-01T01:00:00Z".to_string()),
        ("skoid".to_string(), "object-id".to_string()),
        ("sktid".to_string(), "tenant-id".to_string()),
        ("skt".to_string(), "2030-01-01T00:00:00Z".to_string()),
        ("ske".to_string(), "2030-01-01T01:00:00Z".to_string()),
        ("sks".to_string(), "b".to_string()),
        ("skv".to_string(), "2023-11-03".to_string()),
        ("spr".to_string(), "https".to_string()),
        ("sv".to_string(), "2023-11-03".to_string()),
        ("sr".to_string(), "c".to_string()),
        ("sig".to_string(), "signature".to_string()),
    ])
}

#[test]
fn remote_storage_validation_accepts_only_running_frozen_storage_with_binding() {
    let binding = StorageBinding::s3("files");
    let deployment = deployment(stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(serde_json::to_value(&binding).unwrap()),
    ));

    assert!(matches!(
        remote_storage_binding(&deployment, "files"),
        Ok(RemoteStorageBinding::S3(RemoteS3StorageBinding { .. }))
    ));
}

#[test]
fn external_storage_binding_is_rejected_even_with_synchronized_params() {
    let binding = StorageBinding::s3("existing-files");
    let mut deployment = deployment(stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(serde_json::to_value(&binding).unwrap()),
    ));
    let mut external_bindings = ExternalBindings::new();
    external_bindings.insert("files", ExternalBinding::Storage(binding));
    deployment.stack_settings = Some(StackSettings {
        external_bindings: Some(external_bindings),
        ..StackSettings::default()
    });

    let error = require_setup_owned_remote_storage(&deployment, "files")
        .expect_err("existing buckets are outside the Remote Bindings v0 contract");
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(error.message.contains("cannot use an external binding"));
    assert!(error.message.contains("created by setup"));
}

#[tokio::test]
async fn remote_access_uses_the_current_release_not_the_desired_release() {
    let mut deployment = deployment(stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
    ));
    deployment.current_release_id = Some("current".to_string());
    deployment.desired_release_id = Some("desired".to_string());
    let store = StubReleaseStore {
        releases: HashMap::from([
            (
                "current".to_string(),
                release("current", Platform::Aws, storage_stack(true)),
            ),
            (
                "desired".to_string(),
                release("desired", Platform::Aws, storage_stack(false)),
            ),
        ]),
    };

    require_current_release_remote_access(&store, &deployment, "files")
        .await
        .expect("the current release explicitly enables remote access");
}

#[tokio::test]
async fn legacy_binding_params_cannot_bypass_a_disabled_current_release() {
    let mut deployment = deployment(stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
    ));
    deployment.current_release_id = Some("current".to_string());
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, storage_stack(false)),
        )]),
    };

    assert!(remote_storage_binding(&deployment, "files").is_ok());
    let error = require_current_release_remote_access(&store, &deployment, "files")
        .await
        .expect_err("stack-state binding params cannot grant access by themselves");
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(error.message.contains("current release"));
    assert!(error.message.contains("not enabled for remote access"));
}

#[tokio::test]
async fn remote_access_fails_closed_when_current_release_context_is_missing() {
    let stack_state = stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
    );
    let store = StubReleaseStore::default();

    let no_current_release = deployment(stack_state.clone());
    let error = require_current_release_remote_access(&store, &no_current_release, "files")
        .await
        .expect_err("missing current release must deny access");
    assert_eq!(error.code, "BAD_REQUEST");

    let mut missing_release = deployment(stack_state.clone());
    missing_release.current_release_id = Some("missing".to_string());
    let error = require_current_release_remote_access(&store, &missing_release, "files")
        .await
        .expect_err("a dangling current release id must deny access");
    assert_eq!(error.code, "INTERNAL_ERROR");

    let mut missing_platform_stack = deployment(stack_state.clone());
    missing_platform_stack.current_release_id = Some("current".to_string());
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Gcp, storage_stack(true)),
        )]),
    };
    let error = require_current_release_remote_access(&store, &missing_platform_stack, "files")
        .await
        .expect_err("missing platform stack must deny access");
    assert_eq!(error.code, "INTERNAL_ERROR");

    let mut missing_resource = deployment(stack_state);
    missing_resource.current_release_id = Some("current".to_string());
    let empty_stack = Stack::new("stack".to_string()).build();
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, empty_stack),
        )]),
    };
    let error = require_current_release_remote_access(&store, &missing_resource, "files")
        .await
        .expect_err("resource absent from the current release must deny access");
    assert_eq!(error.code, "BAD_REQUEST");
}

#[test]
fn remote_storage_validation_rejects_unsupported_and_mismatched_platforms() {
    let s3 = serde_json::to_value(StorageBinding::s3("files")).unwrap();
    let gcs = serde_json::to_value(StorageBinding::gcs("files")).unwrap();
    let local = deployment_on_platform(
        stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(s3.clone()),
        ),
        Platform::Local,
    );
    assert!(remote_storage_binding(&local, "files").is_err());

    let mismatched = deployment(stack_state_with_resource(
        Storage::RESOURCE_TYPE.as_ref(),
        Some(ResourceLifecycle::Frozen),
        ResourceStatus::Running,
        Some(gcs),
    ));
    assert!(remote_storage_binding(&mismatched, "files").is_err());
}

#[test]
fn remote_binding_deployment_status_gate_is_post_handoff_only() {
    for status in [
        "running",
        "refresh-failed",
        "update-pending",
        "updating",
        "update-failed",
    ] {
        assert!(
            deployment_status_allows_remote_bindings(deployment_status_from_record(status)),
            "{status}"
        );
    }
    for status in [
        "pending",
        "preflights-failed",
        "initial-setup",
        "initial-setup-failed",
        "provisioning",
        "waiting-for-machines",
        "provisioning-failed",
        "delete-pending",
        "deleting",
        "delete-failed",
        "teardown-required",
        "teardown-failed",
        "deleted",
        "error",
    ] {
        assert!(
            !deployment_status_allows_remote_bindings(deployment_status_from_record(status)),
            "{status}"
        );
    }
    assert!(!deployment_status_allows_remote_bindings(
        deployment_status_from_record("future-or-corrupt-status")
    ));
}

#[test]
fn aws_remote_binding_expiry_uses_provider_expiry_and_rejects_expired_sessions() {
    let now = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(
        remote_binding_expiry(now + chrono::Duration::minutes(15), now).unwrap(),
        now + chrono::Duration::minutes(15)
    );
    assert!(remote_binding_expiry(now - chrono::Duration::seconds(1), now).is_err());
}

#[test]
fn remote_storage_validation_rejects_missing_non_storage_non_frozen_non_running_and_non_remote() {
    let rejected = [
        stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            None,
        ),
        stack_state_with_resource(
            "queue",
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::json!({"service": "s3"})),
        ),
        stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Live),
            ResourceStatus::Running,
            Some(serde_json::json!({"service": "s3"})),
        ),
        stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Provisioning,
            Some(serde_json::json!({"service": "s3"})),
        ),
    ];

    for stack_state in rejected {
        assert!(remote_storage_binding(&deployment(stack_state), "files").is_err());
    }

    assert!(
        remote_storage_binding(&deployment(StackState::new(Platform::Aws)), "missing").is_err()
    );
}

#[test]
fn response_contract_constructs_only_materialized_provider_credentials() {
    let aws = ResolveBindingResponse::from_parts(
        RemoteStorageBinding::S3(RemoteS3StorageBinding {
            bucket_name: "bucket".to_string(),
        }),
        lease(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: "session".to_string(),
                expires_at: "2030-01-01T00:00:00Z".to_string(),
            },
            service_overrides: None,
        }))),
        "2030-01-01T00:00:00Z".to_string(),
    )
    .expect("short-lived AWS session should be accepted");
    let aws = serde_json::to_value(aws).unwrap();
    assert_eq!(
        aws.pointer("/clientConfig/credentials/type"),
        Some(&serde_json::json!("sessionCredentials"))
    );
    assert!(aws.pointer("/clientConfig/serviceOverrides").is_none());

    let gcp = ResolveBindingResponse::from_parts(
        RemoteStorageBinding::Gcs(RemoteGcsStorageBinding {
            bucket_name: "bucket".to_string(),
        }),
        lease(ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "token".to_string(),
            },
            service_overrides: None,
            project_number: Some("123".to_string()),
        }))),
        "2030-01-01T00:00:00Z".to_string(),
    )
    .expect("short-lived GCP access token should be accepted");
    let gcp = serde_json::to_value(gcp).unwrap();
    assert_eq!(
        gcp.pointer("/clientConfig/credentials/type"),
        Some(&serde_json::json!("accessToken"))
    );
    assert_eq!(
        gcp.pointer("/clientConfig/projectNumber"),
        Some(&serde_json::json!("123"))
    );

    let azure = ResolveBindingResponse::from_parts(
        RemoteStorageBinding::Blob(RemoteBlobStorageBinding {
            account_name: "account".to_string(),
            container_name: "container".to_string(),
        }),
        lease(ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::SasToken {
                query_parameters: azure_sas_parameters(),
            },
            service_overrides: None,
        }))),
        "2030-01-01T00:00:00Z".to_string(),
    )
    .expect("exact Azure storage-scope token should be accepted");
    let azure = serde_json::to_value(azure).unwrap();
    assert_eq!(
        azure.pointer("/clientConfig/credentials/type"),
        Some(&serde_json::json!("containerSas"))
    );
    assert_eq!(
        azure.pointer("/clientConfig/credentials/sas/accountName"),
        Some(&serde_json::json!("account"))
    );
    assert_eq!(
        azure.pointer("/clientConfig/credentials/sas/containerName"),
        Some(&serde_json::json!("container"))
    );
    assert_eq!(
        azure.pointer("/clientConfig/credentials/sas/signedResource"),
        Some(&serde_json::json!("c"))
    );
}

#[test]
fn response_contract_rejects_refreshable_static_and_overbroad_credentials() {
    let aws_error = RemoteAwsClientConfig::try_from(AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIA".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        },
        service_overrides: None,
    })
    .err()
    .expect("static AWS access keys must not enter a remote response");
    assert_eq!(aws_error.code, "INTERNAL_ERROR");

    let gcp_error = RemoteGcpClientConfig::try_from(GcpClientConfig {
        project_id: "project".to_string(),
        region: "us-central1".to_string(),
        credentials: GcpCredentials::ServiceMetadata,
        service_overrides: None,
        project_number: None,
    })
    .err()
    .expect("refreshable GCP metadata credentials must not enter a remote response");
    assert_eq!(gcp_error.code, "INTERNAL_ERROR");

    let binding = RemoteBlobStorageBinding {
        account_name: "account".to_string(),
        container_name: "container".to_string(),
    };
    let azure_error = RemoteAzureClientConfig::try_from((
        AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ScopedAccessTokens {
                tokens: HashMap::from([(
                    "https://management.azure.com/.default".to_string(),
                    "management".to_string(),
                )]),
            },
            service_overrides: None,
        },
        &binding,
    ))
    .err()
    .expect("non-storage Azure scopes must not enter a remote response");
    assert_eq!(azure_error.code, "INTERNAL_ERROR");
}

#[test]
fn resolve_response_debug_redacts_binding_and_credentials() {
    let response = ResolveBindingResponse::from_parts(
        RemoteStorageBinding::S3(RemoteS3StorageBinding {
            bucket_name: "sensitive-bucket".to_string(),
        }),
        lease(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: "AKIASECRET".to_string(),
                secret_access_key: "TOP_SECRET".to_string(),
                session_token: "SESSION_SECRET".to_string(),
                expires_at: "2099-01-01T00:00:00Z".to_string(),
            },
            service_overrides: None,
        }))),
        "2099-01-01T00:00:00Z".to_string(),
    )
    .expect("short-lived AWS session should construct a response");

    let debug = format!("{response:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("sensitive-bucket"));
    assert!(!debug.contains("AKIASECRET"));
    assert!(!debug.contains("TOP_SECRET"));
    assert!(!debug.contains("SESSION_SECRET"));
}
