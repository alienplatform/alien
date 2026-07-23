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

/// A tuned model the gateway can route to, produced by a completed
/// fine-tuning job. Maps the public `served_id` an app requests to the
/// provider-native artifact the gateway forwards to (a Bedrock custom-model
/// ARN, a Vertex tuned endpoint id, or a Foundry deployment name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct TunedModel {
    /// The public model id apps send in the `model` field.
    pub served_id: String,
    /// The provider-native upstream artifact (custom-model ARN / tuned endpoint
    /// id / deployment name) the gateway forwards to.
    pub upstream_id: String,
}

/// AWS Bedrock AI binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BedrockAiBinding {
    /// The AWS region where Bedrock is accessed
    pub region: String,
    /// A tuned model served alongside the base catalog, if the resource
    /// declared a completed fine-tuning job. Absent for a pure inference gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuned_model: Option<TunedModel>,
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
    /// A tuned model served alongside the base catalog, if the resource
    /// declared a completed fine-tuning job. Absent for a pure inference gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuned_model: Option<TunedModel>,
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
    /// A tuned model served alongside the base catalog, if the resource
    /// declared a completed fine-tuning job. Absent for a pure inference gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuned_model: Option<TunedModel>,
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
            tuned_model: None,
        })
    }

    pub fn vertex(project: impl Into<String>, location: impl Into<String>) -> Self {
        Self::Vertex(VertexAiBinding {
            project: project.into(),
            location: location.into(),
            tuned_model: None,
        })
    }

    pub fn foundry(endpoint: impl Into<String>, account: impl Into<String>) -> Self {
        Self::Foundry(FoundryAiBinding {
            endpoint: endpoint.into(),
            account: account.into(),
            tuned_model: None,
        })
    }

    /// Attach a tuned model to a managed binding, so the gateway routes
    /// `served_id` to `upstream_id` alongside the base catalog. A no-op on the
    /// `External` (BYO-key) variant, which the gateway does not serve.
    pub fn with_tuned_model(self, served_id: impl Into<String>, upstream_id: impl Into<String>) -> Self {
        let tuned = TunedModel {
            served_id: served_id.into(),
            upstream_id: upstream_id.into(),
        };
        match self {
            Self::Bedrock(b) => Self::Bedrock(BedrockAiBinding {
                tuned_model: Some(tuned),
                ..b
            }),
            Self::Vertex(b) => Self::Vertex(VertexAiBinding {
                tuned_model: Some(tuned),
                ..b
            }),
            Self::Foundry(b) => Self::Foundry(FoundryAiBinding {
                tuned_model: Some(tuned),
                ..b
            }),
            Self::External(b) => Self::External(b),
        }
    }

    /// The tuned model attached to this binding, if any.
    pub fn tuned_model(&self) -> Option<&TunedModel> {
        match self {
            Self::Bedrock(b) => b.tuned_model.as_ref(),
            Self::Vertex(b) => b.tuned_model.as_ref(),
            Self::Foundry(b) => b.tuned_model.as_ref(),
            Self::External(_) => None,
        }
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
    fn test_bedrock_binding_without_tuned_model_omits_field() {
        let binding = AiBinding::bedrock("us-east-1");
        let json = serde_json::to_value(&binding).unwrap();
        assert!(
            json.get("tunedModel").is_none(),
            "an untuned binding must omit tunedModel so the inference-only wire shape is unchanged"
        );
    }

    #[test]
    fn test_bedrock_binding_with_tuned_model_roundtrip() {
        let binding = AiBinding::bedrock("us-east-1")
            .with_tuned_model("finance-model", "arn:aws:bedrock:us-east-1:123:custom-model/abc");

        let json = serde_json::to_value(&binding).unwrap();
        assert_eq!(json["service"], "bedrock");
        assert_eq!(json["tunedModel"]["servedId"], "finance-model");
        assert_eq!(
            json["tunedModel"]["upstreamId"],
            "arn:aws:bedrock:us-east-1:123:custom-model/abc"
        );

        let back: AiBinding = serde_json::from_value(json).unwrap();
        assert_eq!(binding, back);
        assert_eq!(back.tuned_model().unwrap().served_id, "finance-model");
    }

    #[test]
    fn test_external_binding_ignores_tuned_model() {
        // The gateway does not serve BYO-key providers, so a tuned model is a no-op there.
        let binding = AiBinding::external("openai", "sk-x").with_tuned_model("x", "y");
        assert!(binding.tuned_model().is_none());
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
