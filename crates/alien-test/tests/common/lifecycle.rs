//! Lifecycle checks: destroy.
//!
//! These should always run last in the test suite since `check_destroy`
//! tears down the deployment.

use alien_test::TestDeployment;
use anyhow::bail;
use tracing::info;

/// Verify that destroying a deployment works.
///
/// Validates:
/// 1. The destroy API call succeeds
/// 2. The deployment reaches a terminal status (destroyed/deleted)
///
/// Does NOT check URL reachability — cloud resources (API Gateway, Cloud Run,
/// Container Apps) can take several minutes for DNS propagation after deletion.
///
/// This must be the last check in any test, as it terminates the deployment.
pub async fn check_destroy(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!(deployment_id = %deployment.id, "Checking deployment destroy");

    deployment
        .destroy()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to destroy deployment: {}", e))?;

    // Wait for the deployment to reach a terminal status via the manager API.
    let max_wait = std::time::Duration::from_secs(120);
    let start = std::time::Instant::now();

    while start.elapsed() < max_wait {
        let resp = deployment
            .manager()
            .client()
            .get_deployment()
            .id(&deployment.id)
            .send()
            .await;

        match resp {
            Ok(dep) => {
                let status = dep.status.as_str();
                info!(deployment_id = %deployment.id, %status, "Polling destroy status");
                if status == "destroyed" || status == "deleted" {
                    info!(deployment_id = %deployment.id, "Destroy check passed");
                    return Ok(());
                }
                if status == "failed" || status.ends_with("-failed") {
                    bail!(
                        "Deployment {} entered failed state during destroy: {}",
                        deployment.id,
                        status,
                    );
                }
            }
            Err(e) => {
                // 404 means the deployment was already cleaned up
                info!(
                    deployment_id = %deployment.id,
                    error = %e,
                    "Get deployment returned error (may be deleted)"
                );
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    bail!(
        "Deployment {} did not reach terminal status within {}s after destroy",
        deployment.id,
        max_wait.as_secs()
    );
}
