//! The engine and the enforcers must agree on binding values.
//!
//! On a protected install the verifier, the Operator and machines don't take
//! binding values from the control plane: they derive them from the install's
//! prefix, account and region (`alien_signing::binding_env`). A workload gets
//! the same value either way only while each controller names what it
//! creates the way `binding_env` assumes. Each controller test of a type
//! `binding_env` derives calls [`assert_enforcers_derive`] after a real create
//! flow, so a controller that changes its naming fails here.

use std::collections::BTreeMap;

use alien_core::Platform;
use alien_signing::{binding_env, binding_env_name, BindingContext};

/// The install the test executor runs controllers in: resource prefix
/// `test` and the mock client configs' account, project and region.
fn test_install(platform: Platform) -> BindingContext {
    let (platform, account, region) = match platform {
        Platform::Aws => ("aws", "123456789012", "us-east-1"),
        Platform::Gcp => ("gcp", "test-project-123", "us-central1"),
        Platform::Azure => ("azure", "12345678-1234-1234-1234-123456789012", "eastus"),
        other => panic!("binding_env derives no bindings on {other:?}"),
    };
    BindingContext {
        platform: platform.to_string(),
        prefix: "test".to_string(),
        account: account.to_string(),
        region: Some(region.to_string()),
    }
}

/// Assert that `engine`, a controller's `get_binding_params` after creating
/// resource `id` of `resource_type`, equals the value the enforcers derive.
pub(crate) fn assert_enforcers_derive(
    platform: Platform,
    resource_type: &str,
    id: &str,
    engine: serde_json::Value,
) {
    let derived = binding_env(
        &BTreeMap::from([(id.to_string(), resource_type.to_string())]),
        &test_install(platform),
    )
    .expect("binding_env derives this type");
    assert!(
        derived.unresolved.is_empty(),
        "binding_env doesn't derive {resource_type} on {platform:?}"
    );
    let value = derived
        .values
        .get(&binding_env_name(id))
        .expect("binding_env sets the binding's variable");
    let value: serde_json::Value = serde_json::from_str(value).expect("binding_env writes JSON");
    assert_eq!(
        engine, value,
        "the engine's {resource_type} binding differs from what the enforcers derive"
    );
}
