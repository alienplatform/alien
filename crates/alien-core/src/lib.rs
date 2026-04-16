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

pub mod embedded_config;
pub mod sync;

pub mod commands_types;
pub use commands_types::*;

pub mod file_utils;
pub mod image_rewrite;
pub mod instance_catalog;

pub use alien_macros::alien_event;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// No-op kept for backward compatibility. Previously required for typetag/inventory
/// WASM constructor initialization, but no longer needed since typetag was removed.
pub fn init_wasm_constructors() {
    // No-op: typetag/inventory dependency has been removed
}
