mod stack;
pub use stack::*;

pub mod permissions;
pub use permissions::*;

mod platform;
pub use platform::*;

mod build_targets;
pub use build_targets::*;

mod error;
pub use error::*;

mod resource;
pub use resource::*;

mod load_balancer;
pub use load_balancer::*;

mod resources;
pub use resources::*;

pub mod events;
pub use events::*;

pub mod app_events;
pub use app_events::*;

mod id_utils;
pub use id_utils::*;

mod stack_state;
pub use stack_state::*;

mod stack_settings;
pub use stack_settings::*;

pub mod bindings;
pub use bindings::*;

mod external_bindings;
pub use external_bindings::*;

mod client_config;
pub use client_config::*;

mod deployment;
pub use deployment::*;

mod dev_status;
pub use dev_status::*;

pub mod presigned;
pub use presigned::*;

pub mod arc_types;
pub use arc_types::*;

pub mod instance_catalog;

pub use alien_macros::alien_event;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Workaround for wasm-bindgen interpreter panics with static constructors.
// This is required because alien-core uses typetag, which depends on inventory,
// which uses static constructors that need to be explicitly called in WASM.
// See: https://docs.rs/inventory/0.3.21/inventory/index.html#webassembly-and-constructors
#[cfg(all(target_arch = "wasm32", target_family = "wasm"))]
mod wasm_constructor_workaround {
    extern "C" {
        // This function is implicitly provided by the linker when targeting Wasm
        // and is responsible for running static constructors (used by typetag/inventory).
        pub(super) fn __wasm_call_ctors();
    }
}

/// Initialize WASM constructors required by typetag/inventory.
///
/// This function must be called at the entry point of any WASM binary that uses alien-core,
/// before any code that relies on typetag's trait object deserialization.
///
/// # Safety
///
/// This function is safe to call multiple times (constructors are idempotent),
/// but should ideally be called exactly once at module initialization.
///
/// # Example
///
/// ```ignore
/// #[event(fetch)]
/// async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<Response> {
///     alien_core::init_wasm_constructors();
///     // ... rest of your code
/// }
/// ```
#[cfg(all(target_arch = "wasm32", target_family = "wasm"))]
pub fn init_wasm_constructors() {
    unsafe {
        wasm_constructor_workaround::__wasm_call_ctors();
    }
}

/// No-op on non-WASM targets
#[cfg(not(all(target_arch = "wasm32", target_family = "wasm")))]
pub fn init_wasm_constructors() {
    // No-op on non-WASM targets
}
