//! The release-time check for resources Kubernetes can't create must agree
//! with the controllers `alien-infra` registers.

#![cfg(feature = "kubernetes")]

use alien_core::Platform;
use alien_infra::ResourceRegistry;
use alien_preflights::compile_time::resource_controller_available::KUBERNETES_CONTROLLER_TYPES;

#[test]
fn the_release_check_lists_the_kubernetes_controllers() {
    let mut registered: Vec<String> = ResourceRegistry::with_built_ins()
        .registered_controllers()
        .filter(|(_, platform)| *platform == Platform::Kubernetes)
        .map(|(resource_type, _)| resource_type.to_string())
        .collect();
    registered.sort();
    let mut listed: Vec<String> = KUBERNETES_CONTROLLER_TYPES
        .iter()
        .map(|t| t.to_string())
        .collect();
    listed.sort();
    assert_eq!(
        registered, listed,
        "update KUBERNETES_CONTROLLER_TYPES in alien-preflights to match the registry"
    );
}
