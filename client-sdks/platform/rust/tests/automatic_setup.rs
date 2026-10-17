use alien_platform_api::types::NewDeploymentRequest;
use serde_json::json;

#[test]
fn platform_deployment_request_preserves_explicit_setup_management() {
    for mode in ["automatic", "manual"] {
        let request = json!({
            "name": "production",
            "platform": "aws",
            "project": "demo",
            "setupMethod": "cli",
            "stackSettings": { "setupManagement": mode }
        });
        let typed: NewDeploymentRequest = serde_json::from_value(request).unwrap();
        let outgoing = serde_json::to_value(typed).unwrap();
        assert_eq!(outgoing["stackSettings"]["setupManagement"], mode);
        // Verify the server's core configuration sees the same requested mode.
        let settings: alien_core::StackSettings =
            serde_json::from_value(outgoing["stackSettings"].clone()).unwrap();
        assert_eq!(
            settings.setup_management == alien_core::SetupManagement::Automatic,
            mode == "automatic"
        );
    }
}
