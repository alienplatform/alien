//! The CLI decodes some standalone-manager responses with the generated platform client
//! (`whoami`, deployment groups, deployments). That client requires every field its spec lists
//! as required, so a manager response that omits one makes the CLI fail at decode time. These
//! tests serialize the manager's own response structs and decode them with the client types, so
//! a spec change that adds a required field fails here instead of in a user's terminal.

use alien_manager::routes::deployment_groups::{
    DeploymentGroupResponse, ListDeploymentGroupsResponse,
};
use alien_manager::routes::whoami::{ScopeInfo, WhoamiResponse};
use alien_platform_api::types;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Serialize `value` as the manager would, then decode it as the client type `T`.
fn decode<T: DeserializeOwned>(value: &impl Serialize) -> T {
    let json = serde_json::to_value(value).expect("manager response serializes");
    serde_json::from_value(json.clone()).unwrap_or_else(|error| {
        panic!("the generated client cannot decode the manager response: {error}\nresponse: {json}")
    })
}

fn group() -> DeploymentGroupResponse {
    DeploymentGroupResponse {
        id: "dg_0c29fq4a2yjb7kx3smwdgxlcxd7f".to_string(),
        name: "default".to_string(),
        max_deployments: 100,
        deployment_count: 0,
        created_at: "2026-01-01T00:00:00+00:00".to_string(),
        project_id: "prj_standalone000000000000000000".to_string(),
        workspace_id: "ws_standalone00000000000000".to_string(),
        external_id: None,
        access_approver: "customer".to_string(),
    }
}

#[test]
fn deployment_group_decodes_as_get_deployment_group_response() {
    let _: types::GetDeploymentGroupResponse = decode(&group());
}

#[test]
fn deployment_group_decodes_as_created_deployment_group() {
    let _: types::DeploymentGroup = decode(&group());
}

#[test]
fn deployment_group_list_decodes_as_list_deployment_groups_response() {
    let _: types::ListDeploymentGroupsResponse = decode(&ListDeploymentGroupsResponse {
        items: vec![group()],
        next_cursor: None,
    });
}

#[test]
fn whoami_for_a_group_token_decodes_as_subject() {
    let _: types::Subject = decode(&WhoamiResponse {
        kind: "serviceAccount".to_string(),
        id: "token".to_string(),
        workspace_id: "ws_standalone00000000000000".to_string(),
        workspace_name: "default".to_string(),
        role: "deployment-group.deployer".to_string(),
        scope: ScopeInfo {
            r#type: "deployment-group".to_string(),
            project_id: Some("prj_standalone000000000000000000".to_string()),
            deployment_group_id: Some("dg_0c29fq4a2yjb7kx3smwdgxlcxd7f".to_string()),
            deployment_id: None,
        },
    });
}

#[test]
fn whoami_for_a_deployment_token_decodes_as_subject() {
    let _: types::Subject = decode(&WhoamiResponse {
        kind: "serviceAccount".to_string(),
        id: "token".to_string(),
        workspace_id: "ws_standalone00000000000000".to_string(),
        workspace_name: "default".to_string(),
        role: "deployment.manager".to_string(),
        scope: ScopeInfo {
            r#type: "deployment".to_string(),
            project_id: Some("prj_standalone000000000000000000".to_string()),
            deployment_group_id: None,
            deployment_id: Some("dep_0c29fq4a2yjb7kx3smwdgxlcxd7f".to_string()),
        },
    });
}
