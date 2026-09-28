//! A setup rerun must clear a blocked update when the stack carries a gated Live sandbox the
//! deployer declined. The reimport mints the authorization from the stack after declined Live
//! resources are stripped, and the update then presents its target to the runner before that
//! strip, so both sides must hash the same setup-owned state.

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    InitialSetupAuthority, Platform, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, SetupUpdateAuthorization, Stack, StackInputDefinition, StackState,
    Storage,
};
use alien_preflights::runner::PreflightRunner;

fn sandbox() -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://bucket/bundle.zip".to_string(),
        })
        .private_base_image("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base:1".to_string())
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

fn release_stack(with_archive: bool) -> Stack {
    let input = StackInputDefinition::deployer_boolean(
        "sandboxEnabled",
        "Enable the sandbox",
        "Whether to run the sandbox.",
        Some(false),
    );
    let mut stack = Stack::new("stack".to_string()).inputs(vec![input]).add(
        Storage::new("ledger".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    if with_archive {
        stack = stack.add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack
        .add_enabled_when(sandbox(), ResourceLifecycle::Live, "sandboxEnabled")
        .build()
}

fn config(accepted: bool) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    config.input_values.insert(
        "sandboxEnabled".to_string(),
        serde_json::Value::Bool(accepted),
    );
    config
}

async fn rerun_applies(accepted: bool) -> Result<bool, String> {
    let runner = PreflightRunner::new();
    let state = StackState::new(Platform::Aws);
    let client = ClientConfig::Local {
        state_directory: "/unused".to_string(),
    };
    let config = config(accepted);
    let strip = |stack: Stack| {
        alien_deployment::strip_declined_live_resources(
            stack,
            &config.input_values,
            &alien_core::GateAnswers::default(),
            &std::collections::HashSet::new(),
        )
        .expect("gate resolves")
    };
    let prepare = |stack: Stack| {
        let runner = &runner;
        let state = &state;
        let client = &client;
        let config = &config;
        async move {
            let (prepared, _, _) = runner
                .run_deployment_time_preflights(
                    stack,
                    state,
                    config,
                    client,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("prepares");
            prepared
        }
    };

    // Installed: the stored prepared stack is stripped of declined Live resources.
    let installed = strip(prepare(release_stack(false)).await);
    // Setup rerun: the reimport strips the prepared target the same way before hashing.
    let reimported = strip(prepare(release_stack(true)).await);
    let authorization = SetupUpdateAuthorization {
        nonce: "rerun".to_string(),
        baseline_frozen_digest: installed.setup_owned_digest(),
        target_frozen_digest: reimported.setup_owned_digest(),
        release_id: "release".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    };

    // The update step passes the target before the Live strip, as updating.rs does.
    runner
        .run_deployment_time_preflights(
            release_stack(true),
            &state,
            &config,
            &client,
            Some(&installed),
            Some(&authorization),
            None,
        )
        .await
        .map(|(_, _, authorized)| authorized)
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

#[tokio::test]
async fn an_accepted_live_sandbox_rerun_applies() {
    assert_eq!(rerun_applies(true).await, Ok(true));
}

#[tokio::test]
async fn a_declined_live_sandbox_rerun_applies() {
    assert_eq!(rerun_applies(false).await, Ok(true));
}
