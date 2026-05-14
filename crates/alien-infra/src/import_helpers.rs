//! Shared helpers for `ResourceImporter` implementations.
//!
//! Each importer constructs a typed controller (`Aws…Controller`, `Gcp…Controller`, etc.)
//! out of the typed `ImportData` payload and hands it here. We do the rest:
//!
//! * Drive the controller through `serialize_controller` to capture its
//!   `internal_state` JSON.
//! * Read [`ResourceController::get_outputs`] and
//!   [`ResourceController::get_binding_params`] off the same controller — same
//!   computation that runs after a successful provisioning step.
//! * Build a [`StackResourceState`] with controller state and outputs so the
//!   manager can continue the normal lifecycle from the imported point.
//!
//! Keeping this in one place keeps the per-importer files boring data-mapping
//! layers — no `set_internal_controller` boilerplate.

use alien_core::{
    import::ImportContext, ErrorData as CoreErrorData, ResourceStatus, Result, StackResourceState,
};
use alien_error::AlienError;

use crate::core::{serialize_controller, ResourceController};

/// Build a `StackResourceState` for an imported resource.
///
/// The supplied `controller` must already be in its terminal "Ready" state
/// (i.e. all identifying fields populated, state machine pointing at the
/// running terminal). Outputs and binding params are derived from the
/// controller; serialization preserves the `type` discriminator so
/// [`crate::core::deserialize_controller`] round-trips the value.
pub fn make_imported_state<C>(controller: C, ctx: &ImportContext<'_>) -> Result<StackResourceState>
where
    C: ResourceController + 'static,
{
    let outputs = controller.get_outputs();
    let binding = controller.get_binding_params().map_err(|err| {
        AlienError::new(CoreErrorData::GenericError {
            message: format!(
                "binding params extraction failed for resource '{}': {}",
                ctx.resource_id, err
            ),
        })
    })?;
    let internal_state = serialize_controller(&controller).map_err(|err| {
        AlienError::new(CoreErrorData::JsonSerializationFailed {
            reason: format!(
                "controller serialization failed for resource '{}': {}",
                ctx.resource_id, err
            ),
        })
    })?;

    let resource_type = ctx.resource.config.resource_type().to_string();

    Ok(StackResourceState::builder()
        .resource_type(resource_type)
        .status(ResourceStatus::Running)
        .config(ctx.resource.config.clone())
        .internal_state(internal_state)
        .maybe_outputs(outputs)
        .maybe_remote_binding_params(binding)
        .lifecycle(ctx.resource.lifecycle)
        .dependencies(Vec::new())
        .build())
}
