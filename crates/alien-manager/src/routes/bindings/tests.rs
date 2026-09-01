use std::collections::HashMap;

use alien_core::{
    Ai, AwsSandboxBinding, BindingValue, ExternalBinding, ExternalBindings, Platform, Resource,
    SandboxCode, SandboxEgress, SandboxSessionPolicy, Stack, StackResourceState, StackSettings,
    StackState,
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
                cors_allowed_origins: Vec::new(),
                encryption_key: None,
            }))
            .maybe_lifecycle(lifecycle)
            .maybe_remote_binding_params(remote_binding_params)
            .dependencies(Vec::new())
            .build(),
    );
    stack_state
}

fn ai_stack_state(binding: AiBinding, platform: Platform) -> StackState {
    let mut stack_state = StackState::new(platform);
    stack_state.resources.insert(
        "models".to_string(),
        StackResourceState::builder()
            .resource_type(Ai::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(Ai::new("models".to_string()).build()))
            .lifecycle(ResourceLifecycle::Frozen)
            .remote_binding_params(serde_json::to_value(binding).unwrap())
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
        cors_allowed_origins: Vec::new(),
        encryption_key: None,
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

fn sandbox_resource() -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu:24.04".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build()
}

fn sandbox_stack_state(binding: SandboxBinding, platform: Platform) -> StackState {
    sandbox_stack_state_with_lifecycle(binding, platform, ResourceLifecycle::Frozen)
}

fn sandbox_stack_state_with_lifecycle(
    binding: SandboxBinding,
    platform: Platform,
    lifecycle: ResourceLifecycle,
) -> StackState {
    let mut stack_state = StackState::new(platform);
    stack_state.resources.insert(
        "agents".to_string(),
        StackResourceState::builder()
            .resource_type(Sandbox::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(sandbox_resource()))
            .lifecycle(lifecycle)
            .remote_binding_params(serde_json::to_value(binding).unwrap())
            .dependencies(Vec::new())
            .build(),
    );
    stack_state
}

fn open_sandbox_binding() -> SandboxBinding {
    let SandboxBinding::Aws(mut binding) = SandboxBinding::aws(
        "arn:aws:lambda:us-east-1:123456789012:microvm-image:stack-agents",
        "3",
        "us-east-1",
    ) else {
        unreachable!("the AWS constructor returns the AWS variant")
    };
    binding.allow_egress = true;
    binding.preview_ports = vec![8080];
    binding.max_lifetime_seconds = Some(1800);
    SandboxBinding::Aws(binding)
}

#[test]
fn remote_sandbox_validation_returns_the_topology_a_session_is_started_from() {
    let deployment = deployment(sandbox_stack_state(open_sandbox_binding(), Platform::Aws));

    let Ok(RemoteSandboxBinding::Aws(binding)) = remote_sandbox_binding(&deployment, "agents")
    else {
        panic!("a running Frozen sandbox with an open-egress binding resolves")
    };

    assert_eq!(
        binding.image_arn,
        "arn:aws:lambda:us-east-1:123456789012:microvm-image:stack-agents"
    );
    assert_eq!(binding.image_version, "3");
    assert_eq!(binding.region, "us-east-1");
    assert_eq!(binding.preview_ports, vec![8080]);
    assert_eq!(binding.max_lifetime_seconds, Some(1800));
    assert_eq!(binding.idle_suspend_seconds, None);
    assert!(
        binding.allow_egress,
        "an empty connector list means open egress, and the client re-checks the pair"
    );
}

/// A Live sandbox's binding is published by its runtime controller once the image build
/// reaches ACTIVE, and must resolve exactly like a Frozen one's — refusing on lifecycle here
/// would make every runtime-provisioned sandbox unreachable over Remote Bindings.
#[test]
fn remote_sandbox_validation_accepts_a_live_sandbox() {
    let deployment = deployment(sandbox_stack_state_with_lifecycle(
        open_sandbox_binding(),
        Platform::Aws,
        ResourceLifecycle::Live,
    ));

    let Ok(RemoteSandboxBinding::Aws(binding)) = remote_sandbox_binding(&deployment, "agents")
    else {
        panic!("a running Live sandbox with an open-egress binding resolves")
    };

    assert_eq!(
        binding.image_arn,
        "arn:aws:lambda:us-east-1:123456789012:microvm-image:stack-agents"
    );
    assert_eq!(binding.image_version, "3");
}

/// `sandbox/remote-execute` withholds `lambda:PassNetworkConnector`, so a session cannot be
/// started on the connector a restricting sandbox declares. Refusing here names the reason
/// instead of surfacing an AccessDenied from inside the caller's own `create()`.
#[test]
fn remote_sandbox_validation_refuses_a_sandbox_that_restricts_egress() {
    let SandboxBinding::Aws(binding) = open_sandbox_binding() else {
        unreachable!("the fixture is the AWS variant")
    };
    let restricted = SandboxBinding::Aws(AwsSandboxBinding {
        allow_egress: false,
        egress_connector_arns: vec![BindingValue::value(
            "arn:aws:lambda:us-east-1:123456789012:network-connector:stack-agents".to_string(),
        )],
        ..binding
    });
    let deployment = deployment(sandbox_stack_state(restricted, Platform::Aws));

    let Err(error) = remote_sandbox_binding(&deployment, "agents") else {
        panic!("a connector cannot be passed with the remote grant")
    };
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(
        error.message.contains("restricts egress"),
        "{}",
        error.message
    );
}

/// Preflight refuses a remote binding by its permission set's platform coverage, while this route
/// hardcodes AWS. Widening either alone brings back a deployment that installs a grant the other
/// end will not honour.
#[test]
fn remote_sandbox_resolve_agrees_with_the_permission_set_platform_coverage() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let deployment = deployment_on_platform(
            sandbox_stack_state(open_sandbox_binding(), platform),
            platform,
        );

        assert_eq!(
            alien_permissions::permission_set_covers_platform("sandbox/remote-execute", platform),
            remote_sandbox_binding(&deployment, "agents").is_ok(),
            "{platform}"
        );
    }
}

#[test]
fn remote_sandbox_validation_refuses_platforms_without_a_durable_parent() {
    for platform in [Platform::Gcp, Platform::Azure, Platform::Local] {
        let deployment = deployment_on_platform(
            sandbox_stack_state(open_sandbox_binding(), platform),
            platform,
        );
        let Err(error) = remote_sandbox_binding(&deployment, "agents") else {
            panic!("only AWS provisions a sandbox parent a setup identity can be scoped to")
        };
        assert_eq!(error.code, "BAD_REQUEST");
        assert!(
            error.message.contains("only supported on AWS"),
            "{platform}"
        );
    }
}

/// The wire contract remote clients decode. `service` selects the variant, so a rename is a silent
/// breakage for every remote client.
#[test]
fn remote_sandbox_response_carries_the_service_tag_and_no_extra_credentials() {
    let response = ResolveBindingResponse::from_sandbox_parts(
        match remote_sandbox_binding(
            &deployment(sandbox_stack_state(open_sandbox_binding(), Platform::Aws)),
            "agents",
        ) {
            Ok(binding) => binding,
            Err(error) => panic!("fixture must resolve: {error}"),
        },
        lease(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: "token".to_string(),
                expires_at: "2026-01-01T00:00:00Z".to_string(),
            },
            service_overrides: None,
        }))),
        "2026-01-01T00:00:00Z".to_string(),
    )
    .expect("an AWS binding pairs with an AWS lease");

    let json = serde_json::to_value(&response).expect("response serializes");
    assert_eq!(json["service"], "sandbox-aws");
    assert_eq!(json["binding"]["imageVersion"], "3");
    assert_eq!(json["binding"]["allowEgress"], true);
    assert_eq!(json["binding"]["previewPorts"], serde_json::json!([8080]));
    assert!(
        json["binding"].get("idleSuspendSeconds").is_none(),
        "an absent ceiling is omitted rather than sent as null"
    );
    assert_eq!(
        json["clientConfig"]["credentials"]["type"],
        "sessionCredentials"
    );
    assert_eq!(json["expiresAt"], "2026-01-01T00:00:00Z");
    assert_eq!(
        format!("{response:?}"),
        "ResolveBindingResponse { lease: \"<redacted>\" }"
    );
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

    let error = require_setup_owned_remote_binding(&deployment, "files")
        .expect_err("existing buckets are outside the Remote Bindings v0 contract");
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(error.message.contains("cannot use an external binding"));
    assert!(error.message.contains("created by setup"));
}

#[test]
fn remote_key_validation_returns_only_concrete_provider_topology() {
    let cases = [
        (
            Platform::Aws,
            KeyBinding::aws_kms("arn:aws:kms:us-east-1:123:key/abc", Some("us-east-1")),
        ),
        (
            Platform::Gcp,
            KeyBinding::gcp_cloud_kms(
                "projects/example/locations/us/keyRings/data/cryptoKeys/customer",
            ),
        ),
        (
            Platform::Azure,
            KeyBinding::azure_key_vault("https://example.vault.azure.net/keys/customer/version"),
        ),
    ];

    for (platform, binding) in cases {
        let mut state = stack_state_with_resource(
            Key::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::to_value(binding).unwrap()),
        );
        state.platform = platform;
        assert!(remote_key_binding(&deployment_on_platform(state, platform), "files").is_ok());
    }
}

#[test]
fn remote_ai_validation_accepts_each_managed_provider_and_rejects_external_keys() {
    let cases = [
        (Platform::Aws, AiBinding::bedrock("us-east-1")),
        (
            Platform::Gcp,
            AiBinding::vertex("customer-project", "us-central1"),
        ),
        (
            Platform::Azure,
            AiBinding::foundry("https://customer.services.ai.azure.com", "customer-ai"),
        ),
    ];

    for (platform, binding) in cases {
        let deployment = deployment_on_platform(ai_stack_state(binding, platform), platform);
        assert!(remote_ai_binding(&deployment, "models").is_ok());
    }

    let deployment = deployment_on_platform(
        ai_stack_state(AiBinding::external("anthropic", "secret"), Platform::Aws),
        Platform::Aws,
    );
    let error = remote_ai_binding(&deployment, "models")
        .err()
        .expect("external API-key bindings must never be published remotely");
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(error.message.contains("external binding"));
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
async fn deployment_level_ai_selector_requires_exactly_one_remote_ai() {
    let one_ai = Stack::new("stack".to_string())
        .add_with_remote_access(
            Ai::new("models".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(storage(), ResourceLifecycle::Frozen)
        .build();
    let mut deployment = deployment(StackState::new(Platform::Aws));
    deployment.current_release_id = Some("current".to_string());
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, one_ai),
        )]),
    };
    assert_eq!(
        unique_current_release_remote_ai(&store, &deployment)
            .await
            .expect("one remote AI and an unrelated resource is unambiguous"),
        "models"
    );
    assert!(matches!(
        require_current_release_remote_access(&store, &deployment, "models")
            .await
            .expect("an unrelated remote binding does not make AI ambiguous"),
        alien_core::remote_bindings::RemoteBindingKind::Ai
    ));

    let no_ai = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, storage_stack(false)),
        )]),
    };
    let error = unique_current_release_remote_ai(&no_ai, &deployment)
        .await
        .expect_err("zero remote AI resources must fail");
    assert!(error.message.contains("no Frozen AI"));

    let two_ai = Stack::new("stack".to_string())
        .add_with_remote_access(
            Ai::new("primary".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            Ai::new("secondary".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, two_ai),
        )]),
    };
    let error = unique_current_release_remote_ai(&store, &deployment)
        .await
        .expect_err("multiple remote AI resources must fail");
    assert!(error.message.contains("more than one"));
}

#[tokio::test]
async fn key_resolution_rechecks_that_no_sibling_is_remotely_published() {
    let stack = Stack::new("stack".to_string())
        .add_with_remote_access(
            Key {
                id: "customer-key".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(storage(), ResourceLifecycle::Frozen)
        .build();
    let mut deployment = deployment(StackState::new(Platform::Aws));
    deployment.current_release_id = Some("current".to_string());
    let store = StubReleaseStore {
        releases: HashMap::from([(
            "current".to_string(),
            release("current", Platform::Aws, stack),
        )]),
    };

    let error = require_current_release_remote_access(&store, &deployment, "customer-key")
        .await
        .expect_err("resolver must repeat the one-remote-resource rule");
    assert!(error.message.contains("only remoteAccess resource"));
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
            credentials: AzureCredentials::AccessToken {
                token: "storage-token".to_string(),
            },
            service_overrides: None,
        }))),
        "2030-01-01T00:00:00Z".to_string(),
    )
    .expect("short-lived Azure access token should be accepted");
    let azure = serde_json::to_value(azure).unwrap();
    assert_eq!(
        azure.pointer("/clientConfig/credentials/type"),
        Some(&serde_json::json!("accessToken"))
    );
    assert_eq!(
        azure.pointer("/clientConfig/credentials/token"),
        Some(&serde_json::json!("storage-token"))
    );
    assert!(azure.pointer("/clientConfig/credentials/sas").is_none());
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

    let azure_error = RemoteAzureClientConfig::try_from(AzureClientConfig {
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
    })
    .err()
    .expect("refreshable Azure scoped credentials must not enter a remote response");
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
