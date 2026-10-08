//! Operations a stack declares: which plugins its deployments load, their
//! settings, and which operations need approval.
//!
//! The shape is plugin-agnostic. Plugin manifests declare the settings they
//! accept, and whoever creates a release validates these values against them.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Operations declared by a stack, or by an Operator installed on its own.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperationsConfig {
    /// Built-in plugins, by plugin name.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub plugins: IndexMap<String, PluginOperationsConfig>,
    /// Published custom plugins, pinned to exact versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<CustomPluginOperationsConfig>,
}

impl OperationsConfig {
    /// Whether the config declares no plugin at all.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.custom.is_empty()
    }

    /// The same declaration with every setting value removed, safe to report.
    pub fn without_settings(&self) -> Self {
        let strip = |config: &PluginOperationsConfig| PluginOperationsConfig {
            settings: IndexMap::new(),
            approval: config.approval.clone(),
        };
        Self {
            plugins: self
                .plugins
                .iter()
                .map(|(name, config)| (name.clone(), strip(config)))
                .collect(),
            custom: self
                .custom
                .iter()
                .map(|plugin| CustomPluginOperationsConfig {
                    name: plugin.name.clone(),
                    version: plugin.version.clone(),
                    config: strip(&plugin.config),
                })
                .collect(),
        }
    }
}

/// Settings and approval rules for one plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PluginOperationsConfig {
    /// Values for the settings the plugin's manifest declares.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub settings: IndexMap<String, OperationSettingValue>,
    /// Approval rule per operation: an operation name, or `*` for all of them.
    /// Operations no rule matches need approval.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub approval: IndexMap<String, OperationApproval>,
}

/// A published custom plugin at an exact version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CustomPluginOperationsConfig {
    /// Plugin name as published.
    pub name: String,
    /// Exact published version.
    pub version: String,
    /// Settings and approval rules.
    #[serde(flatten)]
    pub config: PluginOperationsConfig,
}

/// Value of one plugin setting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum OperationSettingValue {
    /// A literal value.
    Literal(String),
    /// The value of a stack input.
    Input {
        /// Stack input id.
        input: String,
    },
    /// Stack resources the plugin may act on, such as buckets or queues.
    Resources {
        /// Resource ids in the same stack.
        resources: Vec<String>,
    },
    /// An environment variable of the Operator process. Only for an Operator
    /// installed on its own, so secrets stay in its environment.
    Env {
        /// Environment variable name.
        env: String,
    },
}

/// Approval rule for the operations a pattern matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum OperationApproval {
    /// Run without approval, or require it.
    Decision(OperationApprovalDecision),
    /// A decision plus the highest risk tier a wildcard access request may cover.
    Detailed {
        /// Run without approval, or require it.
        decision: OperationApprovalDecision,
        /// Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard
        /// access request for these operations may cover.
        #[serde(rename = "maxRisk", default, skip_serializing_if = "Option::is_none")]
        max_risk: Option<String>,
    },
}

impl OperationApproval {
    /// The decision this rule makes.
    pub fn decision(&self) -> OperationApprovalDecision {
        match self {
            Self::Decision(decision) | Self::Detailed { decision, .. } => *decision,
        }
    }
}

/// Whether matching operations run without approval.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum OperationApprovalDecision {
    /// Run without approval.
    Auto,
    /// Require approval.
    Manual,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_setting_and_approval_form() {
        let json = serde_json::json!({
            "plugins": {
                "db": {
                    "settings": {
                        "url": { "input": "dbUrl" },
                        "database": "app",
                        "password": { "env": "DB_PASSWORD" },
                        "buckets": { "resources": ["data"] }
                    },
                    "approval": { "*": "manual", "health": { "decision": "auto", "maxRisk": "read-only" } }
                }
            },
            "custom": [{ "name": "mine", "version": "1.2.0", "approval": { "*": "auto" } }]
        });
        let config: OperationsConfig = serde_json::from_value(json.clone()).unwrap();
        let db = &config.plugins["db"];
        assert_eq!(
            db.settings["database"],
            OperationSettingValue::Literal("app".into())
        );
        assert_eq!(
            db.settings["url"],
            OperationSettingValue::Input {
                input: "dbUrl".into()
            }
        );
        assert_eq!(
            db.approval["*"].decision(),
            OperationApprovalDecision::Manual
        );
        assert_eq!(
            db.approval["health"].decision(),
            OperationApprovalDecision::Auto
        );
        assert_eq!(
            config.custom[0].config.approval["*"].decision(),
            OperationApprovalDecision::Auto
        );
        assert_eq!(serde_json::to_value(&config).unwrap(), json);
    }
}
