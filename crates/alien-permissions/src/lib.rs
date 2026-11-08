pub mod error;
pub mod operations;

pub use error::*;

// The evaluator's source file is shared with this crate's integration tests, which load it
// with `mod iam_policy;`. It names the crate as `alien_permissions`, so name the crate that
// way from inside too.
#[cfg(feature = "test-utils")]
extern crate self as alien_permissions;
#[cfg(feature = "test-utils")]
#[path = "../tests/iam_policy/mod.rs"]
pub mod iam_policy;

#[cfg(feature = "infrastructure")]
pub mod aws_key_policy;
#[cfg(feature = "infrastructure")]
pub mod aws_resource_policies;
#[cfg(feature = "infrastructure")]
pub mod generators;
#[cfg(feature = "infrastructure")]
pub mod key_access;
#[cfg(feature = "infrastructure")]
mod infrastructure;
#[cfg(feature = "infrastructure")]
pub mod initial_setup;
#[cfg(feature = "infrastructure")]
pub mod registry;
#[cfg(feature = "infrastructure")]
pub mod variables;
#[cfg(feature = "infrastructure")]
pub use infrastructure::*;
#[cfg(feature = "infrastructure")]
pub use registry::{
    get_permission_set, has_permission_set, list_permission_set_ids,
    management_identity_global_refs, permission_set_covers_platform,
    permission_set_reaches_a_sandbox, permission_set_uses_aws_object_data, AZURE_SANDBOX_DATA_PLANE_ROLE, MANAGEMENT_ROLE_GUARD,
    MICROVM_SESSION_LIFECYCLE_ACTIONS, SANDBOX_SETUP_ROLES_GUARD, SENSITIVE_MICROVM_ACTIONS,
};
#[cfg(feature = "infrastructure")]
pub use variables::VariableInterpolator;

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn operation_permissions_json(request: &str) -> String {
    operations::request_json(request)
}
