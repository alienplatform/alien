//! The rerun's authorization is minted from the stack the re-import keeps, but the update reaches
//! the runner before its declined Live resources are stripped. Each case runs both strips in their
//! real order and pins that both sides agree on whether a gated Live sandbox and its repository stay.

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, GateAnswers,
    InitialSetupAuthority, Kv, Platform, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
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
/// dominates the Live one. `with_archive` is a setup-owned change a rerun authorizes.
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

fn config(value: Option<serde_json::Value>) -> DeploymentConfig {
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
    config.input_values = value
        .map(|value| HashMap::from([("sandboxEnabled".to_string(), value)]))
        .unwrap_or_default();
    config
}

fn state() -> StackState {
    StackState::new(Platform::Aws)
}

fn client() -> ClientConfig {
    ClientConfig::Local {
        state_directory: "/unused".to_string(),
    }
}

/// The install and re-import paths: Frozen declines before the preflights, Live after.
async fn prepared(declared: Stack, config: &DeploymentConfig, answers: &GateAnswers) -> Stack {
    let frozen_gating = alien_deployment::frozen_gating_inputs(&declared);
    let target = alien_deployment::strip_frozen_declines(declared, answers, &frozen_gating);
    let (prepared, _, _) = PreflightRunner::new()
        .run_deployment_time_preflights(
            target,
            &state(),
            config,
            &client(),
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

/// The update path: the Frozen strip runs, the Live strip has not yet. `Ok(true)` means the
/// setup authorization matched.
async fn update(
    declared: Stack,
    config: &DeploymentConfig,
    answers: &GateAnswers,
    installed: &Stack,
    authorization: Option<&SetupUpdateAuthorization>,
) -> Result<bool, String> {
    let frozen_gating = alien_deployment::frozen_gating_inputs(&declared);
    let target = alien_deployment::strip_frozen_declines(declared, answers, &frozen_gating);
    PreflightRunner::new()
        .run_deployment_time_preflights(
            target,
            &state(),
            config,
            &client(),
            Some(installed),
            authorization,
            None,
        )
        .await
        .map(|(_, _, authorized)| authorized)
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

fn authorization(installed: &Stack, reimported: &Stack) -> SetupUpdateAuthorization {
    SetupUpdateAuthorization {
        nonce: "rerun".to_string(),
        baseline_frozen_digest: installed.setup_owned_digest(),
        target_frozen_digest: reimported.setup_owned_digest(),
        release_id: "release".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    }
}

struct Case {
    default: Option<bool>,
    shares_frozen_gate: bool,
    value: Option<serde_json::Value>,
    frozen_answer: Option<bool>,
}

/// Returns whether the sandbox survived the re-import, and whether the update was authorized.
async fn rerun(case: Case) -> (bool, Result<bool, String>) {
    let config = config(case.value);
    let mut answers = GateAnswers::default();
    if let Some(answer) = case.frozen_answer {
        answers.insert("sandboxEnabled".to_string(), answer);
    }
    let release = |with_archive| release_stack(case.default, case.shares_frozen_gate, with_archive);

    let installed = prepared(release(false), &config, &answers).await;
    let reimported = prepared(release(true), &config, &answers).await;
    let sandbox_kept = reimported.resources().any(|(id, _)| id == "agents");
    let authorization = authorization(&installed, &reimported);

    let authorized = update(
        release(true),
        &config,
        &answers,
        &installed,
        Some(&authorization),
    )
    .await;
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
async fn a_provided_no_strips_the_sandbox_over_a_default_yes() {
    let (kept, authorized) = rerun(Case {
        default: Some(true),
        shares_frozen_gate: false,
        value: Some(serde_json::Value::Bool(false)),
        frozen_answer: None,
    })
    .await;
    assert!(!kept);
    assert_eq!(authorized, Ok(true));
}

#[tokio::test]
async fn a_provided_yes_keeps_the_sandbox_over_a_default_no() {
    let (kept, authorized) = rerun(Case {
        default: Some(false),
        shares_frozen_gate: false,
        value: Some(serde_json::Value::Bool(true)),
        frozen_answer: None,
    })
    .await;
    assert!(kept);
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

/// A sandbox declined at install had no pull grant rendered. Updates that keep declining it roll
/// with no setup; accepting it needs setup, and that rerun clears the blocked update.
#[tokio::test]
async fn a_sandbox_declined_at_install_needs_setup_only_once_accepted() {
    let answers = GateAnswers::default();
    let declined = config(Some(serde_json::Value::Bool(false)));
    let accepted = config(Some(serde_json::Value::Bool(true)));
    let release = || release_stack(Some(false), false, false);

    let installed = prepared(release(), &declined, &answers).await;
    assert!(!installed.resources().any(|(id, _)| id == "agents"));

    assert_eq!(
        update(release(), &declined, &answers, &installed, None).await,
        Ok(false),
        "a still-declined sandbox is not a change"
    );

    let blocked = update(release(), &accepted, &answers, &installed, None)
        .await
        .expect_err("accepting the sandbox needs its grant");
    assert!(
        blocked.starts_with("DEPLOYMENT_SETUP_REQUIRED"),
        "{blocked}"
    );

    let reimported = prepared(release(), &accepted, &answers).await;
    let rerun = authorization(&installed, &reimported);
    assert_eq!(
        update(release(), &accepted, &answers, &installed, Some(&rerun)).await,
        Ok(true)
    );
}

/// A release that adds a gated Live resource the deployer declines changes nothing setup owns,
/// even though the mutations granted management for it before the strip.
#[tokio::test]
async fn a_new_gated_resource_declined_by_its_release_rolls_without_setup() {
    let answers = GateAnswers::default();
    let declined = config(Some(serde_json::Value::Bool(false)));
    let installed = Stack::new("stack".to_string())
        .inputs(vec![StackInputDefinition::deployer_boolean(
            "sandboxEnabled",
            "Enable the sandbox",
            "Whether to run the sandbox.",
            Some(false),
        )])
        .add(
            Storage::new("ledger".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let installed = prepared(installed.clone(), &declined, &answers).await;
    let release = Stack::new("stack".to_string())
        .inputs(installed.inputs.clone())
        .add(
            Storage::new("ledger".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Live,
            "sandboxEnabled",
        )
        .build();

    assert_eq!(
        update(release, &declined, &answers, &installed, None).await,
        Ok(false)
    );
}

/// A repository change on a sandbox the same update declines needs no grant.
#[tokio::test]
async fn a_repository_change_on_a_sandbox_being_declined_needs_no_setup() {
    let answers = GateAnswers::default();
    let installed = prepared(
        release_stack(Some(false), false, false),
        &config(Some(serde_json::Value::Bool(true))),
        &answers,
    )
    .await;
    assert!(installed.resources().any(|(id, _)| id == "agents"));

    let mut release = release_stack(Some(false), false, false);
    let mut moved = sandbox();
    moved.private_base_image =
        Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/other:1".to_string());
    release.resources.get_mut("agents").expect("sandbox").config = alien_core::Resource::new(moved);

    assert_eq!(
        update(
            release,
            &config(Some(serde_json::Value::Bool(false))),
            &answers,
            &installed,
            None
        )
        .await,
        Ok(false)
    );
}

/// A gate the runner cannot resolve is reported as such, not as a change setup could fix.
#[tokio::test]
async fn an_unresolvable_gate_is_not_reported_as_setup_required() {
    let answers = GateAnswers::default();
    let installed = prepared(
        release_stack(Some(false), false, false),
        &config(None),
        &answers,
    )
    .await;

    let error = update(
        release_stack(None, false, false),
        &config(Some(serde_json::json!("maybe"))),
        &answers,
        &installed,
        None,
    )
    .await
    .expect_err("a non-boolean gate value is refused");
    assert!(
        error.starts_with("DEPLOYMENT_PREREQUISITE_CHECK_FAILED")
            && error.contains("sandboxEnabled"),
        "{error}"
    );
}
