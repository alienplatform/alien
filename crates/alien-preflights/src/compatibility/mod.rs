//! Stack compatibility checks that validate compatibility between old and new stack configurations.
//! These checks run during stack updates to prevent breaking changes.

pub mod frozen_resources_unchanged;
pub mod permission_profiles_unchanged;
pub mod sandbox_private_repository_unchanged;

pub use frozen_resources_unchanged::FrozenResourcesUnchangedCheck;
pub use permission_profiles_unchanged::PermissionProfilesUnchangedCheck;
pub use sandbox_private_repository_unchanged::SandboxPrivateRepositoryUnchangedCheck;
