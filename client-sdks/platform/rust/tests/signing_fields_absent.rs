//! A platform that predates signed deployments does not send the signing fields. Responses
//! from it must still decode, and an absent field must read as "not protected".

use alien_platform_api::types::{Deployment, DeploymentListItemResponse};
use serde_json::{json, Value};

fn deployment_without_signing_fields() -> Value {
    json!({
        "id": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "name": "unsigned-example",
        "purpose": "application",
        "status": "running",
        "projectId": "prj_cccccccccccccccccccccccccccc",
        "platform": "local",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": "dg_dddddddddddddddddddddddddddd",
        "releaseChannel": "production",
        "stackSettings": {},
        "retryRequested": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "workspaceId": "ws_eeeeeeeeeeeeeeeeeeeeeeee"
    })
}

#[test]
fn a_deployment_without_signing_fields_decodes_as_unprotected() {
    let deployment: Deployment =
        serde_json::from_value(deployment_without_signing_fields()).unwrap();
    assert_eq!(deployment.protected, None);
    assert_eq!(deployment.signing_fingerprint, None);
    assert_eq!(deployment.install_id, None);
}

#[test]
fn a_deployment_list_item_without_signing_fields_decodes_as_unprotected() {
    let mut item = deployment_without_signing_fields();
    item["updateBlocked"] = json!(false);
    let item: DeploymentListItemResponse = serde_json::from_value(item).unwrap();
    assert_eq!(item.protected, None);
    assert_eq!(item.signing_channel, None);
}

#[test]
fn signing_fields_that_are_sent_are_kept() {
    let mut deployment = deployment_without_signing_fields();
    deployment["protected"] = json!(true);
    deployment["signingFingerprint"] = json!(format!("SHA256:{}", "a".repeat(64)));
    deployment["installId"] = json!("aws:123456789012:us-east-1:dxyz");
    let deployment: Deployment = serde_json::from_value(deployment).unwrap();
    assert_eq!(deployment.protected, Some(true));
    assert!(deployment.signing_fingerprint.is_some());
    assert_eq!(
        deployment.install_id.as_deref(),
        Some("aws:123456789012:us-east-1:dxyz")
    );
}
