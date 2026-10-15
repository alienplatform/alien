#![cfg(feature = "full-api")]

use alien_core::{DeploymentConfig, StackSettings};

#[test]
fn execution_config_keeps_saved_quota_warning_overrides() {
    for overrides in [vec!["L-1216C47A".to_string()], Vec::new()] {
        let settings = StackSettings {
            quota_warning_overrides: overrides.clone(),
            ..Default::default()
        };
        let source: DeploymentConfig = serde_json::from_value(serde_json::json!({
            "stackSettings": settings,
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" }
        }))
        .unwrap();
        // Execution claims use this generated config model. A stale schema
        // silently drops settings even when the deployment read retains them.
        let wire: alien_platform_api::types::DeploymentConfig =
            serde_json::from_value(serde_json::to_value(&source).unwrap()).unwrap();
        let restored: DeploymentConfig =
            serde_json::from_value(serde_json::to_value(wire).unwrap()).unwrap();
        assert_eq!(restored.stack_settings.quota_warning_overrides, overrides);
    }
}
