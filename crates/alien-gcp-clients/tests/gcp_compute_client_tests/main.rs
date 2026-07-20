//! Comprehensive E2E tests for the GCP Compute Engine client.
//!
//! These tests create real VPC resources in GCP and verify all operations work correctly.
//! Since VPC resources are expensive to set up and take time, we use a single comprehensive
//! test that exercises all APIs in sequence.

mod context;
mod disks;
mod errors;
mod instances;
mod load_balancing;
mod ssl_proxy;
mod vpc;

use crate::context::ComputeTestContext;
use test_context::test_context;

// =============================================================================================
// Basic Framework Test
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_framework_setup_compute(ctx: &mut ComputeTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    assert!(!ctx.region.is_empty(), "Region should not be empty");

    println!(
        "Successfully connected to Compute Engine in project: {} region: {}",
        ctx.project_id, ctx.region
    );
}
