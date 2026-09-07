//! Generate MCP tool schemas from a plugin's manifest.
//!
//! Every operation a plugin declares already carries the shape an MCP tool
//! definition needs — a name, a description, and a JSON Schema for its
//! params — so this is pure codegen from [`PluginManifest`], not a new
//! concept: an AI agent (or any MCP client) gets one tool per operation
//! without the plugin author writing a separate tool definition by hand.

use schemars::schema::{RootSchema, Schema, SchemaObject};
use serde::{Deserialize, Serialize};

use crate::manifest::{OperationManifest, PluginManifest};

/// One MCP tool definition: `{ name, description, inputSchema }`, the shape
/// the Model Context Protocol's `tools/list` response and most MCP client
/// SDKs expect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSchema {
    /// The tool name an MCP client calls, `<plugin>/<operation>` — matches
    /// the same reference form the CLI's `invoke --operation` flag takes.
    pub name: String,
    /// Human-readable description, from the operation's own `description`
    /// when declared, otherwise a generated fallback naming the plugin and
    /// risk tier so a tool is never presented with no explanation at all.
    pub description: String,
    /// JSON Schema for the operation's params. An operation with no
    /// declared `paramsSchema` gets an empty-object schema (no required
    /// properties, nothing else accepted) rather than an unconstrained
    /// schema — matching that operation taking no meaningful params, not
    /// "any params are allowed".
    pub input_schema: RootSchema,
}

/// Generate one [`McpToolSchema`] per operation the manifest declares.
pub fn generate_mcp_tools(manifest: &PluginManifest) -> Vec<McpToolSchema> {
    manifest
        .operations
        .iter()
        .map(|operation| generate_mcp_tool(manifest, operation))
        .collect()
}

fn generate_mcp_tool(manifest: &PluginManifest, operation: &OperationManifest) -> McpToolSchema {
    let tier = operation.effective_tier(manifest.tier);
    McpToolSchema {
        name: format!("{}/{}", manifest.name, operation.name),
        description: operation.description.clone().unwrap_or_else(|| {
            format!(
                "Run the '{}' operation on the '{}' plugin ({} tier).",
                operation.name,
                manifest.name,
                tier.as_str()
            )
        }),
        input_schema: operation
            .params_schema
            .clone()
            .unwrap_or_else(empty_object_schema),
    }
}

fn empty_object_schema() -> RootSchema {
    RootSchema {
        schema: SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::Object.into()),
            object: Some(Box::new(schemars::schema::ObjectValidation {
                additional_properties: Some(Box::new(Schema::Bool(false))),
                ..Default::default()
            })),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn manifest_json(operations: &str) -> String {
        format!(
            r#"{{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": {{ "amd64": "postgres-linux-amd64" }},
                "operations": [{operations}]
            }}"#
        )
    }

    #[test]
    fn generates_one_tool_per_operation() {
        let manifest = PluginManifest::parse_and_validate(
            manifest_json(r#"{"name": "health"}, {"name": "version"}"#).as_bytes(),
        )
        .expect("valid manifest");

        let tools = generate_mcp_tools(&manifest);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "postgres/health");
        assert_eq!(tools[1].name, "postgres/version");
    }

    #[test]
    fn falls_back_to_a_generated_description_when_none_declared() {
        let manifest = PluginManifest::parse_and_validate(
            manifest_json(r#"{"name": "vacuum", "tier": "mutating"}"#).as_bytes(),
        )
        .expect("valid manifest");

        let tools = generate_mcp_tools(&manifest);
        assert_eq!(tools.len(), 1);
        assert!(tools[0].description.contains("vacuum"));
        assert!(tools[0].description.contains("postgres"));
        assert!(tools[0].description.contains("mutating"));
    }

    #[test]
    fn uses_the_declared_description_when_present() {
        let manifest = PluginManifest::parse_and_validate(
            manifest_json(r#"{"name": "health", "description": "Check database connectivity."}"#)
                .as_bytes(),
        )
        .expect("valid manifest");

        let tools = generate_mcp_tools(&manifest);
        assert_eq!(tools[0].description, "Check database connectivity.");
    }

    #[test]
    fn an_operation_with_no_params_schema_gets_a_closed_empty_object_schema() {
        let manifest =
            PluginManifest::parse_and_validate(manifest_json(r#"{"name": "health"}"#).as_bytes())
                .expect("valid manifest");

        let tools = generate_mcp_tools(&manifest);
        let schema_json = serde_json::to_value(&tools[0].input_schema).expect("schema serializes");
        assert_eq!(schema_json["type"], "object");
        assert_eq!(schema_json["additionalProperties"], false);
    }
}
