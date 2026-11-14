//! `alien deploy --token <group token>` against a standalone manager: the CLI validates the token
//! with `whoami`, then fetches the deployment group with the generated platform client. This runs
//! that path against a real in-process manager, with a group and token created over its API.

use alien_cli::deployment_tracking::{validate_token, DeploymentToken};
use alien_manager_api::types::CreateDeploymentGroupRequest;
use alien_test::TestManager;

#[tokio::test]
async fn a_group_token_validates_against_a_standalone_manager() {
    let manager = TestManager::start().await.expect("start manager");

    let group = manager
        .client()
        .create_deployment_group()
        .body(CreateDeploymentGroupRequest {
            name: "customers".to_string(),
            max_deployments: None,
            environment_variables: Default::default(),
            input_values: Default::default(),
        })
        .send()
        .await
        .expect("create group")
        .into_inner();
    let token = manager
        .client()
        .create_deployment_group_token()
        .id(&group.id)
        .send()
        .await
        .expect("create group token")
        .into_inner()
        .token;

    let validated = validate_token(&token, &manager.url)
        .await
        .expect("the CLI accepts a group token from a standalone manager");

    match validated {
        DeploymentToken::DeploymentGroup {
            deployment_group_id,
            deployment_group_name,
            ..
        } => {
            assert_eq!(deployment_group_id, group.id);
            assert_eq!(deployment_group_name, "customers");
        }
        other => panic!("expected a deployment-group token, got {other:?}"),
    }
}
