//! Project lookup for decisions that start from a project id rather than a
//! stored record, such as the registry proxy, where a repository name carries
//! the id of the project that owns it.

use async_trait::async_trait;

use alien_error::AlienError;

use crate::auth::Subject;

/// A project and the workspace that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub workspace_id: String,
}

#[async_trait]
pub trait ProjectStore: Send + Sync {
    /// The project `project_id` as `caller` may look it up. `None` when no such
    /// project exists or `caller` cannot see it; callers treat both as a denial.
    async fn get_project(
        &self,
        caller: &Subject,
        project_id: &str,
    ) -> Result<Option<ProjectRecord>, AlienError>;
}
