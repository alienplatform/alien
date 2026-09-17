use alien_core::{import::ImportContext, ResourceStatus, Result, StackResourceState};
use alien_error::AlienError;
use serde::de::DeserializeOwned;

use crate::{serialize_controller, ResourceController};

pub trait ResourceImporter: Send + Sync {
    type ImportData: DeserializeOwned + Send + Sync;

    fn import(&self, data: Self::ImportData, ctx: &ImportContext<'_>)
        -> Result<StackResourceState>;

    fn merge_reimport(
        &self,
        _existing: StackResourceState,
        imported: StackResourceState,
        _ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        Ok(imported)
    }
}

pub fn make_imported_state<C>(controller: C, ctx: &ImportContext<'_>) -> Result<StackResourceState>
where
    C: ResourceController + 'static,
{
    make_imported_state_with_status(controller, ctx, ResourceStatus::Running)
}

pub fn make_imported_state_with_status<C>(
    controller: C,
    ctx: &ImportContext<'_>,
    status: ResourceStatus,
) -> Result<StackResourceState>
where
    C: ResourceController + 'static,
{
    let outputs = controller.get_outputs();
    let remote_binding_params = if ctx.resource.publishes_binding_params() {
        controller.get_binding_params().map_err(|err| {
            AlienError::new(alien_core::ErrorData::GenericError {
                message: format!(
                    "binding params extraction failed for resource '{}': {}",
                    ctx.resource_id, err,
                ),
            })
        })?
    } else {
        None
    };
    let internal_state = serialize_controller(&controller).map_err(|err| {
        AlienError::new(alien_core::ErrorData::JsonSerializationFailed {
            reason: format!(
                "controller serialization failed for resource '{}': {}",
                ctx.resource_id, err,
            ),
        })
    })?;

    Ok(StackResourceState::builder()
        .resource_type(ctx.resource.config.resource_type().to_string())
        .status(status)
        .config(ctx.resource.config.clone())
        .internal_state(internal_state)
        .maybe_outputs(outputs)
        .maybe_remote_binding_params(remote_binding_params)
        .lifecycle(ctx.resource.lifecycle)
        .dependencies(ctx.resource.combined_dependencies())
        .build())
}
