//! A resource no platform controller or external binding can create fails the
//! release, through the preflight runner and the built-in registry. A Key on
//! Kubernetes used to pass release and fail during setup.

use alien_core::{Key, Platform, ResourceLifecycle, Stack, Storage};
use alien_preflights::runner::PreflightRunner;

fn stack(with_key: bool) -> Stack {
    let builder = Stack::new("keyed".to_string()).add(
        Storage::new("data".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    if with_key {
        builder
            .add(
                Key::new("signing".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    } else {
        builder.build()
    }
}

#[tokio::test]
async fn a_key_on_kubernetes_fails_the_release_and_names_it() {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack(true), Platform::Kubernetes)
        .await
        .expect("compile-time checks run");
    assert!(!summary.success, "{summary:?}");
    let rendered = format!("{summary:?}");
    assert!(rendered.contains("signing"), "{rendered}");
}

#[tokio::test]
async fn the_same_stack_without_the_key_passes() {
    let summary = PreflightRunner::new()
        .run_compile_time_checks(&stack(false), Platform::Kubernetes)
        .await
        .expect("compile-time checks run");
    assert!(summary.success, "{summary:?}");
}
