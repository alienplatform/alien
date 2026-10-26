//! Live gate edits must not rewrite the Frozen node permission baseline saved by deployment.
use alien_core::{
    ClientConfig, ComputeCluster, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, PermissionProfile, Platform, ReleaseInfo,
    ResourceLifecycle, ResourceStatus, SetupUpdateAuthorization, Stack, StackInputDefinition,
    StackResourceState, StackState, Storage,
};
use alien_preflights::runner::PreflightRunner;

fn stack(with_archive: bool) -> Stack {
    let mut builder = Stack::new("node-gates".into())
        .inputs(vec![StackInputDefinition::deployer_boolean(
            "objectsEnabled",
            "Objects",
            "Create object storage",
            Some(false),
        )])
        .add(
            ComputeCluster::new("compute".into())
                .node_permissions(
                    PermissionProfile::new().resource("objects", ["storage/data-read"]),
                )
                .node_permissions_platforms(vec![Platform::Aws])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Storage::new("objects".into()).build(),
            ResourceLifecycle::Live,
            "objectsEnabled",
        );
    if with_archive {
        builder = builder.add(
            Storage::new("archive".into()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    builder.build()
}

fn config(enabled: bool) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(Default::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .compute_backend(alien_core::ComputeBackend::Horizon(
            alien_core::HorizonConfig {
                url: "https://containers.example.invalid".into(),
                horizon_machine_image: None,
                clusters: Default::default(),
            },
        ))
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    config
        .input_values
        .insert("objectsEnabled".into(), serde_json::json!(enabled));
    config
}

fn client() -> ClientConfig {
    ClientConfig::Local {
        state_directory: "/unused".into(),
    }
}

fn release(stack: Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some("release".into()),
        version: None,
        description: None,
        stack,
    }
}

async fn prepared(declared: Stack, config: &DeploymentConfig) -> alien_core::RuntimeMetadata {
    alien_deployment::prepare_direct_setup_update(
        declared,
        &StackState::new(Platform::Aws),
        config,
        &client(),
        &Default::default(),
    )
    .await
    .expect("setup preparation")
}

async fn repeated_updates(initial_enabled: bool, saved_stack: bool) {
    let declared = stack(false);
    let mut metadata = prepared(declared.clone(), &config(initial_enabled)).await;
    let installed = metadata.prepared_stack.as_ref().unwrap();
    let mut state = StackState::new(Platform::Aws);
    for (id, entry) in installed.resources() {
        let mut resource = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            entry.dependencies.clone(),
        );
        resource.status = ResourceStatus::Running;
        resource.internal_state = Some(serde_json::json!({}));
        state.resources.insert(id.clone(), resource);
    }
    // A legacy release can include the derived permissions but lack a saved prepared stack.
    let old_release = prepared(declared.clone(), &config(true))
        .await
        .prepared_stack
        .unwrap();
    if !saved_stack {
        metadata.prepared_stack = None;
    }
    let mut current = DeploymentState {
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Aws,
        current_release: Some(release(old_release)),
        target_release: Some(release(declared)),
        stack_state: Some(state),
        error: None,
        environment_info: None,
        runtime_metadata: Some(metadata),
        retry_requested: false,
        protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    for enabled in [false, false, true] {
        current.status = DeploymentStatus::UpdatePending;
        current = alien_deployment::step(current, config(enabled), client(), None)
            .await
            .expect("update preflights must not require setup for a Live gate edit")
            .state;
        assert_eq!(current.status, DeploymentStatus::Updating);
        assert!(current.error.is_none(), "{:?}", current.error);
        let metadata = current.runtime_metadata.as_mut().unwrap();
        let updated = metadata.pending_prepared_stack.take().unwrap();
        assert_eq!(updated.resources.contains_key("objects"), enabled);
        // Save the successful update's baseline without executing cloud controllers.
        metadata.prepared_stack = Some(updated);
    }
}

#[tokio::test]
async fn declined_live_node_target_allows_two_updates_and_reenable() {
    for saved in [true, false] {
        repeated_updates(false, saved).await;
    }
}

#[tokio::test]
async fn installed_live_node_target_can_be_declined_twice_and_reenabled() {
    for saved in [true, false] {
        repeated_updates(true, saved).await;
    }
}

#[tokio::test]
async fn node_grants_have_the_same_setup_authorization_projection_after_live_declines() {
    let config = config(false);
    let installed = prepared(stack(false), &config)
        .await
        .prepared_stack
        .unwrap();
    let reimported = prepared(stack(true), &config).await.prepared_stack.unwrap();
    let mut accepted_config = config.clone();
    accepted_config
        .input_values
        .insert("objectsEnabled".into(), serde_json::json!(true));
    let accepted = prepared(stack(true), &accepted_config)
        .await
        .prepared_stack
        .unwrap();
    assert_eq!(
        reimported.resources["compute"].config,
        accepted.resources["compute"].config
    );
    let authorization = SetupUpdateAuthorization {
        nonce: "rerun".into(),
        baseline_frozen_digest: installed.setup_update_digest(),
        target_frozen_digest: reimported.setup_update_digest(),
        release_id: "release".into(),
        setup_target: "target".into(),
        setup_fingerprint: "artifact".into(),
        setup_fingerprint_version: 1,
    };
    let runner = PreflightRunner::new();
    let rejected = runner
        .run_deployment_time_preflights(
            stack(true),
            &StackState::new(Platform::Aws),
            &config,
            &client(),
            Some(&installed),
            None,
            None,
        )
        .await
        .expect_err("adding Frozen archive requires setup");
    assert_eq!(rejected.code, "DEPLOYMENT_SETUP_REQUIRED");
    let (_, summary, authorized) = runner
        .run_deployment_time_preflights(
            stack(true),
            &StackState::new(Platform::Aws),
            &config,
            &client(),
            Some(&installed),
            Some(&authorization),
            Some(InitialSetupAuthority::ImportedHandoff),
        )
        .await
        .expect("setup re-import and update must hash the same retained node grants");
    assert!(summary.success);
    assert!(authorized);
}
