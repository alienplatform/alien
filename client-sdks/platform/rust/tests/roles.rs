use alien_platform_api::types::Role;
use serde_json::json;

#[test]
fn role_scopes_roundtrip_through_their_typed_variants() {
    for (wire, scope) in [
        ("workspace.member", "workspace"),
        ("project.developer", "project"),
        ("deployment.manager", "deployment"),
        ("deployment-group.deployer", "deployment-group"),
        ("manager.runtime", "manager"),
    ] {
        let role: Role = serde_json::from_value(json!(wire)).unwrap();
        let decoded_scope = match &role {
            Role::WorkspaceRole(_) => "workspace",
            Role::ProjectRole(_) => "project",
            Role::DeploymentRole(_) => "deployment",
            Role::DeploymentGroupRole(_) => "deployment-group",
            Role::ManagerRole(_) => "manager",
        };
        assert_eq!(decoded_scope, scope);
        assert_eq!(serde_json::to_value(role).unwrap(), json!(wire));
    }
    for invalid in [
        json!("workspace.unknown"),
        json!("unknown.runtime"),
        json!({}),
    ] {
        assert!(serde_json::from_value::<Role>(invalid).is_err());
    }
}
