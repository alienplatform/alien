//! `observe/metrics-read` is a new grant. A new AWS install with a worker gets it; an install
//! set up before it existed keeps its management profile and updates normally, and only a setup
//! rerun adds the grant.

use alien_core::{
    permissions::ManagementPermissions, ClientConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority, PermissionProfile,
    PermissionsConfig, Platform, ResourceLifecycle, SetupUpdateAuthorization, Stack, StackSettings,
    StackState, Storage, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

const METRICS_READ: &str = "observe/metrics-read";

fn release(with_archive: bool) -> Stack {
    let mut stack = Stack::new("stack".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "example.com/api:v1".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        );
    if with_archive {
        stack = stack.add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack.build()
}

fn config(setup_run: bool) -> DeploymentConfig {
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    config.setup_run = setup_run;
    config
}

fn client() -> ClientConfig {
    ClientConfig::Local {
        state_directory: "/unused".to_string(),
    }
}

fn grants_metrics_read(stack: &Stack) -> bool {
    stack
        .management()
        .profile()
        .is_some_and(|profile| profile.0.values().flatten().any(|g| g.id() == METRICS_READ))
}

/// What setup installs for a new deployment: the release prepared with no installed stack.
async fn fresh_install(release: Stack) -> Stack {
    PreflightRunner::new()
        .run_deployment_time_preflights(
            release,
            &StackState::new(Platform::Aws),
            &config(true),
            &client(),
            None,
            None,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .await
        .expect("a new install prepares")
        .0
}

/// The same install as it was prepared before the grant existed.
async fn install_from_before_the_grant(release: Stack) -> Stack {
    let mut installed = fresh_install(release).await;
    let ManagementPermissions::Extend(profile) = &mut installed.permissions.management else {
        panic!("the install has a management profile");
    };
    for grants in profile.0.values_mut() {
        grants.retain(|grant| grant.id() != METRICS_READ);
    }
    assert!(!grants_metrics_read(&installed));
    installed
}

async fn update(
    release: Stack,
    installed: &Stack,
    authorization: Option<&SetupUpdateAuthorization>,
    setup_authority: Option<InitialSetupAuthority>,
) -> Result<(Stack, bool), String> {
    PreflightRunner::new()
        .run_deployment_time_preflights(
            release,
            &StackState::new(Platform::Aws),
            &config(false),
            &client(),
            Some(installed),
            authorization,
            setup_authority,
        )
        .await
        .map(|(prepared, _, authorized)| (prepared, authorized))
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

#[tokio::test]
async fn a_new_install_with_a_worker_gets_the_grant() {
    assert!(grants_metrics_read(&fresh_install(release(false)).await));
}

#[tokio::test]
async fn an_existing_install_updates_with_its_profile_unchanged() {
    let installed = install_from_before_the_grant(release(false)).await;

    for authority in [None, Some(InitialSetupAuthority::DirectSetup)] {
        let (prepared, _) = update(release(false), &installed, None, authority)
            .await
            .unwrap_or_else(|error| panic!("the update of an existing install must run: {error}"));
        assert!(!grants_metrics_read(&prepared));
        assert_eq!(prepared.management(), installed.management());
    }
}

#[tokio::test]
async fn a_setup_rerun_adds_the_grant_and_the_update_it_authorizes_keeps_it() {
    let installed = install_from_before_the_grant(release(false)).await;
    let reimported = fresh_install(release(true)).await;
    assert!(grants_metrics_read(&reimported));
    let authorization = SetupUpdateAuthorization {
        nonce: "rerun".to_string(),
        baseline_frozen_digest: installed.setup_update_digest(),
        target_frozen_digest: reimported.setup_update_digest(),
        release_id: "release".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    };

    let (prepared, authorized) = update(release(true), &installed, Some(&authorization), None)
        .await
        .expect("the update the rerun authorized runs");
    assert!(authorized, "the rerun's authorization matches the target");
    assert!(grants_metrics_read(&prepared));

    let (next, _) = update(release(true), &prepared, None, None)
        .await
        .expect("the next update of the rerun install runs");
    assert!(grants_metrics_read(&next));
}

#[tokio::test]
async fn a_frozen_change_without_a_rerun_still_needs_setup() {
    let installed = install_from_before_the_grant(release(false)).await;
    let error = update(release(true), &installed, None, None)
        .await
        .expect_err("a new frozen resource needs setup");
    assert!(error.contains("setup"), "{error}");
}
