//! A manager authenticates a bearer token by decoding the platform's `whoami` answer, and reads
//! each deployment's saved installation. A value the vendored spec lacks makes the decode fail, so
//! a one-use Kubernetes Terraform setup key would be refused and its deployment unreadable.

use alien_platform_api::types::{DeploymentInstallation, Subject};
use serde_json::json;

#[test]
fn whoami_of_a_setup_key_decodes() {
    let body = json!({
        "kind": "serviceAccount",
        "id": "apikey_01",
        "workspaceId": "ws_01",
        "workspaceName": "example",
        "scope": { "type": "deployment", "deploymentId": "dep_01", "projectId": "prj_01" },
        "role": "deployment.manager",
        "purpose": "setup-enrollment",
    });
    let subject: Subject = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(serde_json::to_value(&subject).unwrap()["purpose"], body["purpose"]);
}

#[test]
fn an_installation_on_the_setup_key_decodes() {
    let body = json!({
        "method": "terraform",
        "registrationMode": "enrollment-key-v1",
        "terraform": { "target": "eks", "hcl": "module \"deployment\" {}", "isAzureTarget": false },
    });
    let installation: DeploymentInstallation = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&installation).unwrap()["registrationMode"],
        body["registrationMode"]
    );
}
