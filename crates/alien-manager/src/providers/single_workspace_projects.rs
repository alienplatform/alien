//! Default [`ProjectStore`]: the standalone manager serves one workspace,
//! `"default"`, so every project in its registry belongs to it.

use async_trait::async_trait;

use alien_error::AlienError;

use crate::auth::Subject;
use crate::traits::{ProjectRecord, ProjectStore};

/// The workspace every standalone-manager record carries.
pub const DEFAULT_WORKSPACE_ID: &str = "default";

pub struct SingleWorkspaceProjects;

#[async_trait]
impl ProjectStore for SingleWorkspaceProjects {
    async fn get_project(
        &self,
        _caller: &Subject,
        project_id: &str,
    ) -> Result<Option<ProjectRecord>, AlienError> {
        Ok(Some(ProjectRecord {
            id: project_id.to_string(),
            workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
        }))
    }
}
