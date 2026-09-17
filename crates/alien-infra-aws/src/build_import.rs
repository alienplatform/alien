//! Importer for AWS Build (CodeBuild project).

use alien_core::{
    import::{data::AwsBuildImportData, ImportContext},
    Result, StackResourceState,
};

use crate::build::{AwsBuildController, AwsBuildState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// AWS CodeBuild importer.
#[derive(Debug, Default)]
pub struct AwsBuildImporter;

impl ResourceImporter for AwsBuildImporter {
    type ImportData = AwsBuildImportData;

    fn import(
        &self,
        data: AwsBuildImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsBuildController {
            state: AwsBuildState::Ready,
            project_arn: Some(data.project_arn),
            project_name: Some(data.project_name),
            build_env_vars: Some(data.build_env_vars),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
