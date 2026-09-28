//! The runner resolves a Live sandbox's gate before the update strips it, and must reach the
//! answer the setup re-import reached, or the rerun's authorization never matches. Each case runs
//! the re-import's and the update's strips in their real order around the preflights.

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, GateAnswers,
    InitialSetupAuthority, Platform, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, SetupUpdateAuthorization, Stack, StackInputDefinition, StackState,
    Storage,
};
use alien_preflights::runner::PreflightRunner;
use std::collections::HashMap;

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

/// `shares_frozen_gate` puts a Frozen store behind the sandbox's input, so the Frozen answer
/// dominates the Live one. `with_archive` is the setup-owned change the rerun authorizes.
fn release_stack(default: Option<bool>, shares_frozen_gate: bool, with_archive: bool) -> Stack {
    let input = StackInputDefinition::deployer_boolean(
        "sandboxEnabled",
        "Enable the sandbox",
        "Whether to run the sandbox.",
        default,
    );
    let mut stack = Stack::new("stack".to_string()).inputs(vec![input]).add(
        Storage::new("ledger".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    if shares_frozen_gate {
        stack = stack.add_enabled_when(
            Storage::new("gated-store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "sandboxEnabled",
        );
    }
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

fn config(input_values: HashMap<String, serde_json::Value>) -> DeploymentConfig {
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
    config.input_values = input_values;
    config
}

struct Case {
    default: Option<bool>,
    shares_frozen_gate: bool,
    value: Option<serde_json::Value>,
    frozen_answer: Option<bool>,
}

/// Returns whether the sandbox survived the re-import, and whether the update was authorized.
async fn rerun(case: Case) -> (bool, Result<bool, String>) {
    let runner = PreflightRunner::new();
    let state = StackState::new(Platform::Aws);
    let client = ClientConfig::Local {
        state_directory: "/unused".to_string(),
    };
    let mut values = HashMap::new();
    if let Some(value) = case.value {
        values.insert("sandboxEnabled".to_string(), value);
    }
    let config = config(values);
    let mut answers = GateAnswers::default();
    if let Some(answer) = case.frozen_answer {
        answers.insert("sandboxEnabled".to_string(), answer);
    }

    // The deployment and re-import paths: Frozen declines before the preflights, Live after.
    let prepare = |declared: Stack| {
        let runner = &runner;
        let state = &state;
        let client = &client;
        let config = &config;
        let answers = &answers;
        async move {
            let frozen_gating = alien_deployment::frozen_gating_inputs(&declared);
            let target = alien_deployment::strip_frozen_declines(declared, answers, &frozen_gating);
            let (prepared, _, _) = runner
                .run_deployment_time_preflights(
                    target,
                    state,
                    config,
                    client,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("prepares");
            alien_deployment::strip_declined_live_resources(
                prepared,
                &config.input_values,
                answers,
                &frozen_gating,
            )
            .expect("gate resolves")
        }
    };

    let installed = prepare(release_stack(case.default, case.shares_frozen_gate, false)).await;
    let reimported = prepare(release_stack(case.default, case.shares_frozen_gate, true)).await;
    let sandbox_kept = reimported.resources().any(|(id, _)| id == "agents");
    let authorization = SetupUpdateAuthorization {
        nonce: "rerun".to_string(),
        baseline_frozen_digest: installed.setup_owned_digest(),
        target_frozen_digest: reimported.setup_owned_digest(),
        release_id: "release".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    };

    // The update path: the Frozen strip runs, the Live strip has not yet.
    let declared = release_stack(case.default, case.shares_frozen_gate, true);
    let frozen_gating = alien_deployment::frozen_gating_inputs(&declared);
    let target = alien_deployment::strip_frozen_declines(declared, &answers, &frozen_gating);
    let authorized = runner
        .run_deployment_time_preflights(
            target,
            &state,
            &config,
            &client,
            Some(&installed),
            Some(&authorization),
            None,
        )
        .await
        .map(|(_, _, authorized)| authorized)
        .map_err(|error| format!("{}: {}", error.code, error.message));
    (sandbox_kept, authorized)
}

#[tokio::test]
async fn a_frozen_yes_keeps_the_sandbox_over_a_default_no() {
    let (kept, authorized) = rerun(Case {
        default: Some(false),
        shares_frozen_gate: true,
        value: None,
        frozen_answer: Some(true),
    })
    .await;
    assert!(kept);
    assert_eq!(authorized, Ok(true));
}

#[tokio::test]
async fn a_frozen_no_strips_the_sandbox_over_a_default_yes() {
    let (kept, authorized) = rerun(Case {
        default: Some(true),
        shares_frozen_gate: true,
        value: None,
        frozen_answer: Some(false),
    })
    .await;
    assert!(!kept);
    assert_eq!(authorized, Ok(true));
}

#[tokio::test]
async fn a_default_no_strips_an_unanswered_live_gate() {
    let (kept, authorized) = rerun(Case {
        default: Some(false),
        shares_frozen_gate: false,
        value: None,
        frozen_answer: None,
    })
    .await;
    assert!(!kept);
    assert_eq!(authorized, Ok(true));
}

#[tokio::test]
async fn a_string_no_strips_the_sandbox_over_a_default_yes() {
    let (kept, authorized) = rerun(Case {
        default: Some(true),
        shares_frozen_gate: false,
        value: Some(serde_json::Value::String("false".to_string())),
        frozen_answer: None,
    })
    .await;
    assert!(!kept);
    assert_eq!(authorized, Ok(true));
}

#[tokio::test]
async fn a_string_yes_keeps_the_sandbox_over_a_default_no() {
    let (kept, authorized) = rerun(Case {
        default: Some(false),
        shares_frozen_gate: false,
        value: Some(serde_json::Value::String("true".to_string())),
        frozen_answer: None,
    })
    .await;
    assert!(kept);
    assert_eq!(authorized, Ok(true));
}
