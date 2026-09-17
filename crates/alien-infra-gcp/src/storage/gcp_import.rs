//! Importer for GCP Storage (Cloud Storage bucket).

use alien_core::{
    import::{data::GcpStorageImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::storage::{GcpStorageController, GcpStorageState};

/// GCP Cloud Storage importer.
#[derive(Debug, Default)]
pub struct GcpStorageImporter;

impl ResourceImporter for GcpStorageImporter {
    type ImportData = GcpStorageImportData;

    fn import(
        &self,
        data: GcpStorageImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // self_link, project_id, location are runtime metadata; the
        // controller derives them from `ctx.client_config`.
        let _ = (data.bucket_self_link, data.project_id, data.location);
        let controller = GcpStorageController {
            state: GcpStorageState::Ready,
            bucket_name: Some(data.bucket_name),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
