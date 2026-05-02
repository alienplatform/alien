//! Registry helpers.
//!
//! Each manager instance owns its registries and configures them at boot.
//! [`crate::routes::registry_proxy::RegistryRoutingTable`] handles upstream
//! routing — no per-request resolver lookup, no upstream call on the path.
//! The authorization concerns at this layer are:
//!
//! 1. **Push authz** — `routes::registry_proxy::require_push_auth` derives
//!    the project_id from the OCI repo path via
//!    [`crate::routes::registry_proxy::RegistryRoutingTable::project_id_for_repo`]
//!    and lets [`crate::auth::Authz::can_push_image`] decide. Each provider
//!    composes repo names as `{prefix}{sep}{name}` (`-` for ECR, `/` for
//!    GAR/ACR/Local), so the routing-table prefix lookup recovers `name`
//!    unambiguously.
//!
//! 2. **Pull validation** — `validate_pull_access` gates pulls on whether
//!    the deployment owns the requested repo (orthogonal to project_id).
