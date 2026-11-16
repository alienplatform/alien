//! What a release that changes a sandbox's egress does at the deployment-time preflights today.
//!
//! These record current behavior through the entry point the deployment update calls
//! (`run_deployment_time_preflights`, alien-deployment `updating.rs`). The installed stack is
//! prepared by the same mutations a stored stack went through. A preflight error stops the update
//! before the executor plans anything, so no resource controller runs for it.
#![cfg(feature = "runtime-checks")]

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, Platform, ResourceLifecycle,
    Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;
use SandboxEgress::{Allow, Internet, None as NoEgress};

// Fixture note: on GCP `internet` is refused at compile time on this revision
// (SANDBOX_EGRESS_GATEWAY_UNAVAILABLE), so `allow` is the widening value there. Changing a
// sandbox's egress also changes which setup-owned network resources the mutations add, so those
// appear in the same error beside the sandbox's own.

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

fn stack(lifecycle: ResourceLifecycle, image: &str, egress: SandboxEgress) -> Stack {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: image.to_string(),
            bundled_from: None,
        })
        .egress(egress)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    Stack::new("stack".to_string())
        .add(sandbox, lifecycle)
        .build()
}

/// The installed stack as the manager stored it: after the mutations.
async fn installed(runner: &PreflightRunner, platform: Platform, stack: Stack) -> Stack {
    runner
        .apply_mutations(stack, &StackState::new(platform), &config())
        .await
        .expect("mutations apply")
}

/// Ok(the stack the update would hand the executor), or the error code and message.
async fn update(platform: Platform, old: Stack, new: Stack) -> Result<Stack, (String, String)> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_test_writer()
        .try_init();
    let runner = PreflightRunner::new();
    let old = installed(&runner, platform, old).await;
    let client = ClientConfig::Local {
        state_directory: "/unused".to_string(),
    };
    let outcome = runner
        .run_deployment_time_preflights(
            new,
            &StackState::new(platform),
            &config(),
            &client,
            Some(&old),
            None,
            None,
        )
        .await
        .map(|(mutated, _, _)| mutated)
        .map_err(|error| (error.code.clone(), error.message.clone()));
    match &outcome {
        Ok(_) => eprintln!("{platform} -> accepted"),
        Err((code, message)) => eprintln!("{platform} -> {code}: {message}"),
    }
    outcome
}

fn egress_of(stack: &Stack) -> SandboxEgress {
    stack.resources["agents"]
        .config
        .downcast_ref::<Sandbox>()
        .expect("sandbox")
        .egress
        .clone()
}

const IMAGE: &str = "us-central1-docker.pkg.dev/proj/agents/sandbox@sha256:aaaa";

/// (a) A Frozen GCP sandbox cannot widen its egress through a release: the Frozen check reports
/// setup-required (`DEPLOYMENT_SETUP_REQUIRED`), and since the preflight returns an error the
/// executor never receives the target.
#[tokio::test]
async fn a_frozen_gcp_sandbox_widening_its_egress_needs_setup() {
    for (from, to) in [(NoEgress, Allow)] {
        let (code, message) = update(
            Platform::Gcp,
            stack(ResourceLifecycle::Frozen, IMAGE, from.clone()),
            stack(ResourceLifecycle::Frozen, IMAGE, to.clone()),
        )
        .await
        .expect_err(&format!("{from:?} -> {to:?} needs setup"));
        assert_eq!(code, "DEPLOYMENT_SETUP_REQUIRED", "{from:?} -> {to:?}");
        assert!(
            message.contains("Frozen resource 'agents' was modified"),
            "{message}"
        );
    }
}

/// (a) Narrowing is refused the same way: the Frozen check compares the whole declaration, so
/// only `code.image` may differ without setup.
#[tokio::test]
async fn a_frozen_gcp_sandbox_narrowing_its_egress_also_needs_setup() {
    let (code, message) = update(
        Platform::Gcp,
        stack(ResourceLifecycle::Frozen, IMAGE, Allow),
        stack(ResourceLifecycle::Frozen, IMAGE, NoEgress),
    )
    .await
    .expect_err("any egress change needs setup");
    assert_eq!(code, "DEPLOYMENT_SETUP_REQUIRED");
    assert!(
        message.contains("Frozen resource 'agents' was modified"),
        "{message}"
    );
}

/// (a) Control: an image-only change and an unchanged declaration roll out without setup.
#[tokio::test]
async fn a_frozen_gcp_sandbox_rolls_an_image_only_change() {
    update(
        Platform::Gcp,
        stack(ResourceLifecycle::Frozen, IMAGE, NoEgress),
        stack(
            ResourceLifecycle::Frozen,
            "ghcr.io/org/sandbox:v2",
            NoEgress,
        ),
    )
    .await
    .expect("image only");
    update(
        Platform::Gcp,
        stack(ResourceLifecycle::Frozen, IMAGE, NoEgress),
        stack(ResourceLifecycle::Frozen, IMAGE, NoEgress),
    )
    .await
    .expect("unchanged");
}

/// (b, on main) A sandbox DECLARED LIVE on GCP is refused at build time by the lifecycle policy
/// ("only AWS and Kubernetes provision a sandbox at runtime"), so on main no release can carry a
/// Live GCP sandbox to the update path. (`SandboxSetupInputsUnchangedCheck` would not stop its
/// egress change: it skips every platform but AWS, and the Frozen check reads Frozen entries.)
#[tokio::test]
async fn a_live_gcp_sandbox_is_refused_before_any_egress_comparison() {
    let (code, _) = update(
        Platform::Gcp,
        stack(ResourceLifecycle::Live, IMAGE, NoEgress),
        stack(ResourceLifecycle::Live, IMAGE, Allow),
    )
    .await
    .expect_err("a Live sandbox is not allowed on GCP");
    assert_eq!(code, "VALIDATION_FAILED");
}

/// (c) A Live AWS sandbox's egress is a setup input: widening it reports setup-required
/// (`SandboxSetupInputsUnchangedCheck`, AWS only); an image-only change rolls out.
#[tokio::test]
async fn a_live_aws_sandbox_widening_its_egress_needs_setup() {
    const IMG: &str = "s3://bucket/sandbox-bundle/v1/bundle.zip";
    for (from, to) in [(NoEgress, Allow)] {
        let (code, message) = update(
            Platform::Aws,
            stack(ResourceLifecycle::Live, IMG, from.clone()),
            stack(ResourceLifecycle::Live, IMG, to.clone()),
        )
        .await
        .expect_err(&format!("{from:?} -> {to:?} needs setup"));
        assert_eq!(code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(message.contains("egress"), "{message}");
    }
}

/// (c) A Live Kubernetes sandbox's egress mode is the chart's NetworkPolicy, checked by the
/// Frozen check (`kubernetes_live_sandbox_chart_changes`), not by the AWS-only setup-inputs check.
#[tokio::test]
async fn a_live_kubernetes_sandbox_widening_its_egress_needs_setup() {
    for (from, to) in [(NoEgress, Allow)] {
        let (code, message) = update(
            Platform::Kubernetes,
            stack(ResourceLifecycle::Live, "app:1", from.clone()),
            stack(ResourceLifecycle::Live, "app:1", to.clone()),
        )
        .await
        .expect_err(&format!("{from:?} -> {to:?} needs setup"));
        assert_eq!(code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(message.contains("egress mode"), "{message}");
    }
}

/// (d) A sandbox the installed stack lacks (what a first template create is, from the
/// preflight's side) is compared with nothing: a Frozen one is "added", which only setup may do.
#[tokio::test]
async fn a_new_frozen_gcp_sandbox_in_a_release_needs_setup() {
    let without = Stack::new("stack".to_string()).build();
    let (code, message) = update(
        Platform::Gcp,
        without,
        stack(ResourceLifecycle::Frozen, IMAGE, Allow),
    )
    .await
    .expect_err("adding a frozen resource needs setup");
    assert_eq!(code, "DEPLOYMENT_SETUP_REQUIRED");
    assert!(message.contains("agents"), "{message}");
}

/// (a) `internet` on GCP never reaches the compatibility checks: a build-time check refuses it
/// (no gateway hosting on GCP), for a first release and for an update alike.
#[tokio::test]
async fn gcp_refuses_internet_egress_before_any_compatibility_check() {
    let (code, message) = update(
        Platform::Gcp,
        stack(ResourceLifecycle::Frozen, IMAGE, NoEgress),
        stack(ResourceLifecycle::Frozen, IMAGE, Internet),
    )
    .await
    .expect_err("internet egress is not available on GCP");
    assert_eq!(code, "VALIDATION_FAILED");
    let _ = message;
}
