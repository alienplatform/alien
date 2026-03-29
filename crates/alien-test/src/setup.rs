//! Target account setup for E2E tests.
//!
//! In a full E2E flow the target cloud account must be prepared before
//! deployments can land there (e.g. running `alien-deploy-cli` to provision
//! IAM roles, networking, etc.). This module provides a stub entry point
//! that will be fleshed out once the deploy-CLI is integrated into the test
//! harness.

use alien_core::Platform;
use tracing::info;

use crate::config::TestConfig;

/// Provision the target account for the given platform.
///
/// Currently a no-op stub. Future implementation will shell out to
/// `alien-deploy-cli` (or invoke it as a library) to set up IAM roles,
/// VPCs, security groups, and other prerequisites in the target account.
pub async fn setup_target(
    config: &TestConfig,
    platform: Platform,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config.has_platform(platform) {
        return Err(format!(
            "Cannot set up target for {}: missing management or target credentials",
            platform.as_str()
        )
        .into());
    }

    info!(
        platform = %platform.as_str(),
        "setup_target: stub -- target account setup not yet implemented"
    );
    Ok(())
}
