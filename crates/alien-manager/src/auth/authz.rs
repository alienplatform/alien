//! Authorization trait. The default implementation
//! ([`crate::providers::oss_authz::OssAuthz`]) ships in this crate; embedders
//! that need different policy inject their own impl via the builder.
//!
//! Methods take fully-loaded entity references where possible — the caller
//! has already loaded the row from the store, and passing it in lets the
//! impl inspect every field without re-querying. Create paths take a context
//! struct instead because the entity does not exist yet.

use crate::auth::Subject;
use crate::traits::{
    deployment_store::{DeploymentGroupRecord, DeploymentRecord},
    release_store::ReleaseRecord,
};

/// Context for create operations. Carries the parent identifiers + workspace
/// the create is targeting; the entity itself does not exist yet.
#[derive(Debug, Clone)]
pub struct DeploymentCreateCtx<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub deployment_group_id: Option<&'a str>,
}

/// Authorization decisions for the manager HTTP surface.
///
/// **All `can_*` methods return `bool`.** Callers translate `false` into a
/// structured forbidden response (`fail-fast, fail-loud`). No method takes a
/// raw ID where an entity reference is available.
pub trait Authz: Send + Sync {
    // -- Releases ----------------------------------------------------------
    fn can_create_release(&self, subject: &Subject, project_id: &str) -> bool;
    fn can_read_release(&self, subject: &Subject, release: &ReleaseRecord) -> bool;
    fn can_export_release(&self, subject: &Subject, release: &ReleaseRecord) -> bool;

    // -- Deployments -------------------------------------------------------
    fn can_create_deployment(&self, subject: &Subject, ctx: DeploymentCreateCtx<'_>) -> bool;
    fn can_read_deployment(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;
    fn can_update_deployment(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;
    fn can_delete_deployment(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;

    // -- Deployment groups -------------------------------------------------
    fn can_create_deployment_group(&self, subject: &Subject, project_id: &str) -> bool;
    fn can_read_deployment_group(&self, subject: &Subject, dg: &DeploymentGroupRecord) -> bool;
    fn can_update_deployment_group(&self, subject: &Subject, dg: &DeploymentGroupRecord) -> bool;
    fn can_delete_deployment_group(&self, subject: &Subject, dg: &DeploymentGroupRecord) -> bool;

    // -- Commands ----------------------------------------------------------
    fn can_dispatch_command(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;
    fn can_read_command(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;

    // -- Sync protocol -----------------------------------------------------
    fn can_sync_deployment(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;
    fn can_acquire_deployments(
        &self,
        subject: &Subject,
        deployments: &[DeploymentRecord],
    ) -> bool;

    // -- Telemetry ingest --------------------------------------------------
    /// Telemetry decisions take only the deployment ID. Authorization is a
    /// scope check on the bearer (the validator already bound `Subject` to a
    /// workspace and a `Scope::Deployment`); loading the full record was
    /// historically required only because policy needed `deployment.id`, which
    /// is the same string we already have in the `Subject`.
    fn can_ingest_telemetry_for(
        &self,
        subject: &Subject,
        deployment_id: &str,
    ) -> bool;

    // -- Registry proxy ----------------------------------------------------
    /// Push: caller has write access on the project carrying the repo.
    fn can_push_image(&self, subject: &Subject, project_id: &str, repo_name: &str) -> bool;
    /// Generic "can act on deployment" — used by registry-proxy pull after a
    /// structural "is this repo in this deployment's stack" check done by the
    /// handler.
    fn can_act_on_deployment(&self, subject: &Subject, deployment: &DeploymentRecord) -> bool;
}
