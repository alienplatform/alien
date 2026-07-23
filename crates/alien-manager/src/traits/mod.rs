pub mod auth_validator;
pub mod credential_resolver;
pub mod deployment_store;
pub mod release_store;
pub mod server_bindings;
pub mod telemetry_backend;
pub mod token_store;

/// Workspace/project default value used by OSS rows and as the `serde(default)`
/// for [`release_store::ReleaseRecord`] / [`deployment_store::DeploymentRecord`]
/// / [`deployment_store::DeploymentGroupRecord`] when reading older snapshots
/// that predate the schema migration.
pub(crate) fn default_string() -> String {
    "default".to_string()
}

pub use auth_validator::{AuthValidator, TokenType};
pub use credential_resolver::{
    CredentialResolver, GcpCredentialAccessBoundarySource, RemoteStorageCredentialSource,
    ResolvedCredentials,
};
pub(crate) use deployment_store::deployment_status_from_record;
pub use deployment_store::{
    AcquiredDeployment, CreateDeploymentGroupParams, CreateDeploymentParams,
    CreateImportedDeploymentParams, DeploymentAcquireMode, DeploymentFilter, DeploymentGroupRecord,
    DeploymentRecord, DeploymentStore, ReconcileData, UpdateImportedDeploymentParams,
};
pub use release_store::{CreateReleaseParams, ReleaseRecord, ReleaseStore};
pub use server_bindings::ServerBindings;
pub use telemetry_backend::{TelemetryBackend, TelemetryCaller, TelemetrySignal};
pub use token_store::{CreateTokenParams, TokenRecord, TokenStore};
