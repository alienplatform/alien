//! AI Gateway binding definitions for managed AI inference across cloud providers.

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents an AI Gateway binding for managed inference across cloud providers.
///
/// The managed variants (Bedrock/Vertex/Foundry) carry only identifiers and
/// endpoints; authentication uses the workload's ambient cloud identity. The
/// `External` (BYO-key) variant deliberately carries a vault-resolved API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum AiBinding {
    /// AWS Bedrock AI binding
    Bedrock(BedrockAiBinding),
    /// GCP Vertex AI binding
    Vertex(VertexAiBinding),
    /// Azure AI Foundry binding
    Foundry(FoundryAiBinding),
    /// External provider binding (generic endpoint-based)
    External(ExternalAiBinding),
}

/// AWS Bedrock AI binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BedrockAiBinding {
    /// The AWS region where Bedrock is accessed
    pub region: String,
}

/// GCP Vertex AI binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct VertexAiBinding {
    /// The GCP project ID
    pub project: String,
    /// The Vertex AI region (e.g., "us-central1")
    pub location: String,
}

/// Azure AI Foundry binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct FoundryAiBinding {
    /// The Foundry deployment endpoint URL
    pub endpoint: String,
    /// The Azure account or subscription identifier
    pub account: String,
}

/// External AI provider binding configuration (BYO-key).
///
/// The operator-supplied secret rides inside the binding via
/// `BindingValue<String>`, so it is a literal on cloud platforms and gains
/// Kubernetes SecretRef resolution for free (`extract_binding_secrets` walks
/// the binding JSON for `secretRef`).
// No derived `Debug` — an inline `api_key` would print cleartext; see the redacting impl below.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExternalAiBinding {
    /// The external AI provider name (e.g., "openai", "anthropic")
    pub provider: String,
    /// The provider API key. Resolved to plaintext in the worker environment
    /// (literal on cloud, Kubernetes Secret on K8s) so the SDK reads it directly.
    pub api_key: BindingValue<String>,
}

// Redacts the inline key and keeps every other field, mirroring the external
// Postgres binding's redacting impl.
impl std::fmt::Debug for ExternalAiBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalAiBinding")
            .field("provider", &self.provider)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl AiBinding {
    /// The env var a developer sets to bring their own provider key on the Local platform.
    /// Shared by the Local controller (provision-time check) and the local bindings
    /// resolver (runtime-only re-resolution), so the two never drift.
    pub const LOCAL_API_KEY_ENV: &'static str = "OPENAI_API_KEY";
    /// The BYO-key provider assumed on the Local platform.
    pub const LOCAL_DEFAULT_PROVIDER: &'static str = "openai";

    pub fn bedrock(region: impl Into<String>) -> Self {
        Self::Bedrock(BedrockAiBinding {
            region: region.into(),
        })
    }

    pub fn vertex(project: impl Into<String>, location: impl Into<String>) -> Self {
        Self::Vertex(VertexAiBinding {
            project: project.into(),
            location: location.into(),
        })
    }

    pub fn foundry(endpoint: impl Into<String>, account: impl Into<String>) -> Self {
        Self::Foundry(FoundryAiBinding {
            endpoint: endpoint.into(),
            account: account.into(),
        })
    }

    pub fn external(
        provider: impl Into<String>,
        api_key: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::External(ExternalAiBinding {
            provider: provider.into(),
            api_key: api_key.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bedrock_binding_roundtrip() {
        let binding = AiBinding::bedrock("us-east-1");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"bedrock""#));

        let deserialized: AiBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_vertex_binding_roundtrip() {
        let binding = AiBinding::vertex("my-project", "us-central1");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"vertex""#));

        let deserialized: AiBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_foundry_binding_roundtrip() {
        let binding = AiBinding::foundry("https://my-foundry.openai.azure.com", "my-subscription");

        let json = serde_json::to_value(&binding).unwrap();
        let json_str = json.to_string();
        assert!(json_str.contains(r#""service":"foundry""#));
        assert!(
            !json_str.contains("instantAccess"),
            "foundry binding must not serialize instant_access"
        );

        let deserialized: AiBinding = serde_json::from_value(json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_external_binding_roundtrip() {
        let binding = AiBinding::external("openai", "sk-test-key");

        // The injected env-var JSON must match exactly what the SDK's
        // `ai(name)` parser expects: service-tagged, camelCase, key inline.
        let json = serde_json::to_value(&binding).unwrap();
        let json_str = json.to_string();
        assert!(json_str.contains(r#""apiKey""#), "external binding must serialize apiKey in camelCase");
        assert_eq!(
            json,
            serde_json::json!({
                "service": "external",
                "provider": "openai",
                "apiKey": "sk-test-key",
            })
        );

        let deserialized: AiBinding =
            serde_json::from_value(json).expect("external binding should round-trip");
        assert_eq!(binding, deserialized);
    }
}
