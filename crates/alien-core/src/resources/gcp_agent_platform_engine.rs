use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// A Gemini Agent Platform reasoning engine: the durable parent that sandbox
/// environment templates and sessions hang under. One per sandbox, provisioned
/// once and addressed by the server-assigned id its controller records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct GcpAgentPlatformEngine {
    /// Identifier for the engine resource within the stack.
    #[builder(start_fn)]
    pub id: String,
}

impl GcpAgentPlatformEngine {
    /// The resource type identifier for Agent Platform reasoning engines.
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("gcp_agent_platform_engine");

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The engine id for a sandbox: one engine per sandbox. Shared by the mutation that
    /// synthesizes the engine and the template controller that reads it back as a dependency, so
    /// the two cannot drift on the convention.
    pub fn id_for_sandbox(sandbox_id: &str) -> String {
        format!("{sandbox_id}-engine")
    }
}

impl ResourceDefinition for GcpAgentPlatformEngine {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    fn validate_update(&self, _new_config: &dyn ResourceDefinition) -> Result<()> {
        Err(AlienError::new(ErrorData::InvalidResourceUpdate {
            resource_id: self.id.clone(),
            reason: "reasoning engines cannot be updated once created".to_string(),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<GcpAgentPlatformEngine>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_engine_carries_its_id() {
        let engine = GcpAgentPlatformEngine::new("orders-engine".to_string()).build();
        assert_eq!(engine.id(), "orders-engine");
    }

    #[test]
    fn an_engine_refuses_any_update() {
        let engine = GcpAgentPlatformEngine::new("orders-engine".to_string()).build();
        let error = engine
            .validate_update(&engine)
            .expect_err("a reasoning engine is immutable once created");
        assert_eq!(error.code, "INVALID_RESOURCE_UPDATE");
    }
}
