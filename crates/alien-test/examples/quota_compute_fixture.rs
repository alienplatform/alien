//! Ephemeral quota-short ComputeCluster deployment for a real deploy-CLI run.
//! Setup may start automatically once the record exists. Use target credentials,
//! capture the preflight failure, and drive normal deletion before stopping.

use alien_client_config::ClientConfigExt;
use alien_core::{CapacityGroup, ClientConfig, ComputeCluster, Platform, ResourceLifecycle, Stack};
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
    let environment = std::env::vars().collect();
    ClientConfig::from_env(Platform::Aws, &environment).await?;
    let manager = TestManager::start().await?;
    let stack = Stack::new("q5-compute".into())
        .add(
            ComputeCluster::new("compute".into())
                .capacity_group(CapacityGroup {
                    group_id: "general".into(),
                    instance_type: Some("m7g.xlarge".into()),
                    profile: None,
                    min_size: 0,
                    max_size: 10,
                    scale_policy: None,
                    nested_virtualization: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
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
            name: "q5-compute".into(),
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
            "name": "q5-compute", "platform": "aws", "deploymentGroupId": group.id,
            "resourcePrefix": "q5-compute-flow", "environmentVariables": [], "inputValues": {},
            "stackSettings": {"deploymentModel": "push", "heartbeats": "off", "telemetry": "off",
                "network": {"type": "use-default"}}
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
        "Quota-short ComputeCluster fixture ready; manager and credentials stored in private output directory"
    );
    tokio::signal::ctrl_c().await?;
    manager.stop().await;
    Ok(())
}
