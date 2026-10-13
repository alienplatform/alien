//! Runtime checks that validate cloud environment readiness with cloud client access.
//! These checks only run during deployment time and require the runtime-checks feature.

#[cfg(feature = "runtime-checks")]
pub mod aws_compute;
#[cfg(feature = "runtime-checks")]
pub mod aws_lambda;
#[cfg(feature = "runtime-checks")]
pub mod aws_nat;
#[cfg(feature = "runtime-checks")]
pub mod azure_compute;
#[cfg(feature = "runtime-checks")]
pub mod cloud_quotas;
#[cfg(feature = "runtime-checks")]
pub mod gcp_compute;
#[cfg(feature = "runtime-checks")]
pub mod gcp_iam;
#[cfg(feature = "runtime-checks")]
pub use cloud_quotas::CloudQuotasCheck;

// TODO: Implement runtime checks
// #[cfg(feature = "runtime-checks")]
// pub mod cloud_connectivity;
// #[cfg(feature = "runtime-checks")]
// pub mod cloud_apis_enabled;
// #[cfg(feature = "runtime-checks")]
// pub mod sufficient_permissions;

// #[cfg(feature = "runtime-checks")]
// pub use cloud_connectivity::CloudConnectivityCheck;
// #[cfg(feature = "runtime-checks")]
// pub use cloud_apis_enabled::CloudApisEnabledCheck;
// #[cfg(feature = "runtime-checks")]
// pub use sufficient_permissions::SufficientPermissionsCheck;
