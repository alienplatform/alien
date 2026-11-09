//! Operations as a stack file declares them.
//!
//! The same rules as the TypeScript SDK's `toOperationsConfig`: custom plugins are pinned to
//! an exact `name@version`, and a declaration names at least one plugin.

use indexmap::IndexMap;

use super::invalid;
use crate::error::Result;
use crate::{CustomPluginOperationsConfig, OperationsConfig, PluginOperationsConfig};

/// A published custom plugin written as `name@version`.
pub fn custom_plugin(spec: &str) -> Result<CustomPluginOperationsConfig> {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            Ok(CustomPluginOperationsConfig {
                name: name.to_string(),
                version: version.to_string(),
                config: PluginOperationsConfig::default(),
            })
        }
        _ => Err(invalid(
            "operations.custom",
            format!("custom plugin '{spec}' must be written as 'name@version'"),
        )),
    }
}

/// The operations a stack declares, from its built-in and custom plugins.
pub fn operations_config(
    plugins: IndexMap<String, PluginOperationsConfig>,
    custom: Vec<CustomPluginOperationsConfig>,
) -> Result<OperationsConfig> {
    let config = OperationsConfig { plugins, custom };
    if config.is_empty() {
        return Err(invalid("operations", "needs at least one plugin"));
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorData, OperationApproval, OperationApprovalDecision, OperationSettingValue};
    use alien_error::AlienError;

    fn field_and_reason(error: AlienError<ErrorData>) -> (String, String) {
        match error.error.expect("error data") {
            ErrorData::StackDefinitionInvalid { field, reason } => (field, reason),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn a_custom_plugin_is_pinned_to_a_version() {
        let plugin = custom_plugin("mine@1.2.0").unwrap();
        assert_eq!(plugin.name, "mine");
        assert_eq!(plugin.version, "1.2.0");
        assert_eq!(plugin.config, PluginOperationsConfig::default());

        let scoped = custom_plugin("@acme/diagnostics@1.4.0").unwrap();
        assert_eq!(scoped.name, "@acme/diagnostics");
        assert_eq!(scoped.version, "1.4.0");

        for spec in ["mine", "@1.0.0", "mine@", ""] {
            let (field, reason) = field_and_reason(custom_plugin(spec).unwrap_err());
            assert_eq!(field, "operations.custom");
            assert!(reason.contains("name@version"), "{reason}");
        }
    }

    #[test]
    fn a_declaration_names_a_plugin_and_serializes_as_the_sdk_does() {
        let (field, reason) =
            field_and_reason(operations_config(IndexMap::new(), vec![]).unwrap_err());
        assert_eq!(field, "operations");
        assert!(reason.contains("at least one plugin"), "{reason}");

        let config = operations_config(
            IndexMap::from([(
                "kubernetes".to_string(),
                PluginOperationsConfig {
                    settings: IndexMap::from([(
                        "database".to_string(),
                        OperationSettingValue::Literal("app".to_string()),
                    )]),
                    approval: IndexMap::from([(
                        "*".to_string(),
                        OperationApproval::Decision(OperationApprovalDecision::Auto),
                    )]),
                },
            )]),
            vec![custom_plugin("mine@1.2.0").unwrap()],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::json!({
                "plugins": {
                    "kubernetes": {
                        "settings": { "database": "app" },
                        "approval": { "*": "auto" }
                    }
                },
                "custom": [{ "name": "mine", "version": "1.2.0" }]
            })
        );
    }
}
