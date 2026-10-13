//! Start an ephemeral manager with a Network-only AWS deployment for CLI quota proof.
//! This only creates local API records. Run the deploy CLI separately with target
//! credentials, then drive normal deletion before stopping this fixture.

use alien_core::Stack;
use alien_test::manager::TestManager;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(std::env::args().nth(1).expect("private output directory"));
    std::fs::create_dir_all(&output)?;
    #[cfg(unix)]
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))?;
    let manager = TestManager::start().await?;
    let stack = Stack::new("q5-vpc".into()).build();
    let release = manager
        .client()
        .create_release()
        .body(alien_manager_api::types::CreateReleaseRequest {
            stack: serde_json::from_value(serde_json::json!({"aws": stack}))?,
            git_metadata: None,
            project_id: "default".into(),
            channel: None,
        })
        .send()
        .await?
        .into_inner();
    let group = manager
        .client()
        .create_deployment_group()
        .body(alien_manager_api::types::CreateDeploymentGroupRequest {
            name: "q5-vpc".into(),
            max_deployments: None,
            environment_variables: Default::default(),
            input_values: Default::default(),
        })
        .send()
        .await?
        .into_inner();
    let deployment = manager
        .client()
        .create_deployment()
        .body(serde_json::from_value::<
            alien_manager_api::types::CreateDeploymentRequest,
        >(serde_json::json!({
            "name": "q5-vpc", "platform": "aws", "deploymentGroupId": group.id,
            "resourcePrefix": "q5-vpc-flow", "environmentVariables": [], "inputValues": {},
            "stackSettings": {"deploymentModel": "push", "heartbeats": "off", "telemetry": "off",
                "network": {"type": "create", "availabilityZones": 2}}
        }))?)
        .send()
        .await?
        .into_inner();
    std::fs::write(output.join("manager-url"), &manager.url)?;
    std::fs::write(output.join("admin-token"), &manager.admin_token)?;
    std::fs::write(
        output.join("deployment-token"),
        deployment.token.as_deref().ok_or("missing token")?,
    )?;
    std::fs::write(output.join("deployment-id"), &deployment.deployment.id)?;
    std::fs::write(output.join("release-id"), &release.id)?;
    println!(
        "Network-only fixture ready; manager and credentials stored in private output directory"
    );
    tokio::signal::ctrl_c().await?;
    manager.stop().await;
    Ok(())
}
