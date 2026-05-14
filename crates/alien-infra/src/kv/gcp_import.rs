//! Importer for GCP KV (Firestore Native database).

use alien_core::{
    import::{data::GcpKvImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::kv::gcp::GcpKvState;
use crate::kv::GcpKvController;

/// GCP Firestore key/value importer.
#[derive(Debug, Default)]
pub struct GcpKvImporter;

impl ResourceImporter for GcpKvImporter {
    type ImportData = GcpKvImportData;

    fn import(&self, data: GcpKvImportData, ctx: &ImportContext<'_>) -> Result<StackResourceState> {
        // `location` is determined by the resource config at runtime.
        // Collection name comes from the source `Kv` resource definition,
        // matching the Terraform binding ref emitter.
        let _ = data.location;
        let controller = GcpKvController {
            state: GcpKvState::Ready,
            database_name: Some(data.database_id),
            collection_name: Some(ctx.resource.config.id().to_string()),
            project_id: Some(data.project_id),
            operation_name: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
