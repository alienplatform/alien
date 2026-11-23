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
    /// External provider binding (generic endpoint-based). The tag must stay
    /// unique across every resource type's binding enum — a bare "external"
    /// collides with the external Postgres binding in the shared
    /// `ALIEN_*_BINDING` namespace.
    #[serde(rename = "external-ai")]
    External(ExternalAiBinding),
}

/// AWS Bedrock AI binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BedrockAiBinding {
    /// The AWS region where Bedrock is accessed (for example `us-east-1`). The gateway builds
    /// `https://bedrock-runtime.<region>.amazonaws.com` from it, so parsing refuses anything
    /// that is not a plain AWS region name.
    #[serde(deserialize_with = "deserialize_aws_region")]
    pub region: String,
}

impl BedrockAiBinding {
    /// Builds a binding after checking the region with [`validate_aws_region`].
    pub fn new(region: impl Into<String>) -> Result<Self, String> {
        let region = region.into();
        validate_aws_region(&region)?;
        Ok(Self { region })
    }
}

/// Accepts an AWS region name such as `us-east-1`, `eu-central-2`, `us-gov-west-1` or
/// `ap-southeast-4`: lowercase letter groups and one trailing number, joined by single
/// dashes, starting with a two-letter geography. No dots, slashes, spaces or uppercase, so a
/// region can never change the host of a URL built from it.
pub fn validate_aws_region(region: &str) -> Result<(), String> {
    let invalid = || {
        format!(
            "invalid AWS region {region:?}: expected a region name such as \"us-east-1\" \
             (lowercase letters, digits and single dashes only)"
        )
    };
    if region.len() > 30 {
        return Err(invalid());
    }
    let parts: Vec<&str> = region.split('-').collect();
    let [geography, middle @ .., number] = parts.as_slice() else {
        return Err(invalid());
    };
    let lowercase = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase());
    let valid = geography.len() == 2
        && lowercase(geography)
        && !middle.is_empty()
        && middle.iter().all(|m| lowercase(m))
        && !number.is_empty()
        && number.len() <= 2
        && number.bytes().all(|b| b.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn deserialize_aws_region<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let region = String::deserialize(deserializer)?;
    validate_aws_region(&region).map_err(serde::de::Error::custom)?;
    Ok(region)
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

/// What an installer supplies for an AI resource with an external binding.
///
/// Two inputs, told apart by whether the JSON carries a `service` tag (other than `external-ai`):
/// - `{ "provider", "apiKey" }` — BYO key (the original input; stored deployments and Helm values
///   carry it, so it parses exactly as before). `{ "service": "external-ai", "provider",
///   "apiKey" }` also parses, as it did before the cloud-identity form existed.
/// - `{ "service": "bedrock", "region" }` — the cloud's own AI service, called with the workload's
///   ambient identity (for example an EKS pod's IRSA role). No key, no Secret.
///
/// The cloud-identity form needs the pod to have its own AWS identity: on Kubernetes, an IRSA role
/// on the service account (or an EKS Pod Identity association) with `bedrock:InvokeModel` and
/// `bedrock:InvokeModelWithResponseStream`. Without one the gateway refuses to sign and never uses
/// the node's role. The `region` must be a plain AWS region name such as `us-east-1`.
///
/// Access is not checked at install time (the setup credentials cannot speak for the pod's role):
/// it is checked on the first call, and a model the role cannot use fails with
/// `provider_access_unavailable`.
///
/// Untagged, because the BYO-key input has no tag and must keep parsing unchanged. The two shapes
/// cannot overlap: the ambient one requires `service`, the key one requires `provider` and `apiKey`.
/// `Deserialize` is hand-written (see below) so a typo gets an error naming both accepted forms, and
/// so an ambient input carrying an unused key such as `apiKey` is refused instead of dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ExternalAi {
    /// A managed cloud AI service reached with the workload's own identity.
    Ambient(AmbientAiBinding),
    /// A provider reached with a key the installer supplies.
    ApiKey(ExternalAiBinding),
}

impl<'de> Deserialize<'de> for ExternalAi {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        const FORMS: &str = "expected either {\"service\": \"bedrock\", \"region\": \"<region>\"} \
            (the pod's cloud identity, no key) or {\"provider\": \"<name>\", \"apiKey\": \"<key>\"} (your own key)";
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(D::Error::custom(format!(
                "invalid external AI binding: {FORMS}"
            )));
        };
        // `external-ai` is the tag the BYO-key binding carries once it reaches a workload, and
        // earlier releases accepted that form here (the tag was ignored), so keep it on the key
        // path. Any other `service` is the cloud-identity form.
        let service = object.get("service").and_then(|v| v.as_str());
        if object.contains_key("service") && service != Some("external-ai") {
            if let Some(extra) = object.keys().find(|k| *k != "service" && *k != "region") {
                return Err(D::Error::custom(format!(
                    "invalid external AI binding: field `{extra}` is not allowed with `service` \
                     (it uses the workload's cloud identity and takes only `region`); {FORMS}"
                )));
            }
            serde_json::from_value(value)
                .map(Self::Ambient)
                .map_err(|e| D::Error::custom(format!("invalid external AI binding: {e}; {FORMS}")))
        } else {
            serde_json::from_value(value)
                .map(Self::ApiKey)
                .map_err(|e| D::Error::custom(format!("invalid external AI binding: {e}; {FORMS}")))
        }
    }
}

/// A managed cloud AI service the workload reaches with its ambient identity. The installer owns
/// the identity and its permissions; Alien grants nothing for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum AmbientAiBinding {
    /// AWS Bedrock, signed with the workload's AWS credential chain.
    Bedrock(BedrockAiBinding),
}

impl From<ExternalAiBinding> for ExternalAi {
    fn from(binding: ExternalAiBinding) -> Self {
        Self::ApiKey(binding)
    }
}

impl ExternalAi {
    /// The binding the workload SDK parses from `ALIEN_<NAME>_BINDING`.
    pub fn to_ai_binding(&self) -> AiBinding {
        match self {
            Self::Ambient(AmbientAiBinding::Bedrock(b)) => AiBinding::Bedrock(b.clone()),
            Self::ApiKey(b) => AiBinding::External(b.clone()),
        }
    }
}

impl AiBinding {
    /// The env var a developer sets to bring their own provider key on the Local platform.
    /// Shared by the Local controller (provision-time check) and the local bindings
    /// resolver (runtime-only re-resolution), so the two never drift.
    pub const LOCAL_API_KEY_ENV: &'static str = "OPENAI_API_KEY";
    /// The BYO-key provider assumed on the Local platform.
    pub const LOCAL_DEFAULT_PROVIDER: &'static str = "openai";
    /// When set on the `alien dev` process, local AI runs on Bedrock in this region with the
    /// developer's own AWS credentials (the default chain's, with the developer's full identity;
    /// not refreshed, so restart `alien dev` after an SSO session expires) instead of a
    /// bring-your-own provider key.
    pub const LOCAL_BEDROCK_REGION_ENV: &'static str = "ALIEN_AI_LOCAL_BEDROCK_REGION";
    /// The provider name the local controller reports when it serves Bedrock.
    pub const LOCAL_BEDROCK_PROVIDER: &'static str = "bedrock";

    /// The region named by [`Self::LOCAL_BEDROCK_REGION_ENV`], validated; `None` when unset.
    pub fn local_bedrock_region() -> Result<Option<String>, String> {
        match std::env::var(Self::LOCAL_BEDROCK_REGION_ENV) {
            Ok(region) if !region.is_empty() => BedrockAiBinding::new(region)
                .map(|binding| Some(binding.region))
                .map_err(|error| format!("{}: {error}", Self::LOCAL_BEDROCK_REGION_ENV)),
            _ => Ok(None),
        }
    }

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

    pub fn external(provider: impl Into<String>, api_key: impl Into<BindingValue<String>>) -> Self {
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
        assert!(
            json_str.contains(r#""apiKey""#),
            "external binding must serialize apiKey in camelCase"
        );
        assert_eq!(
            json,
            serde_json::json!({
                "service": "external-ai",
                "provider": "openai",
                "apiKey": "sk-test-key",
            })
        );

        let deserialized: AiBinding =
            serde_json::from_value(json).expect("external binding should round-trip");
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn external_ai_with_the_external_ai_service_tag_still_parses_as_a_key() {
        let parsed: ExternalAi = serde_json::from_value(serde_json::json!({
            "service": "external-ai", "provider": "openai", "apiKey": "sk-x"
        }))
        .expect("the tagged BYO-key input parsed before and must keep parsing");
        assert!(matches!(&parsed, ExternalAi::ApiKey(b) if b.provider == "openai"));
        // Same through the external binding wrapper, and a tagged input missing its key is
        // still refused with the clear error.
        let binding: crate::ExternalBinding = serde_json::from_value(serde_json::json!({
            "type": "ai", "service": "external-ai", "provider": "openai", "apiKey": "sk-x"
        }))
        .unwrap();
        assert_eq!(
            binding.to_env_binding_value().unwrap(),
            serde_json::json!({"service": "external-ai", "provider": "openai", "apiKey": "sk-x"})
        );
        let err = serde_json::from_value::<ExternalAi>(
            serde_json::json!({"service": "external-ai", "provider": "openai"}),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("invalid external AI binding"), "{err}");
    }

    #[test]
    fn bedrock_region_must_be_a_plain_aws_region() {
        for ok in [
            "us-east-1",
            "eu-central-2",
            "ap-southeast-4",
            "us-gov-west-1",
            "us-iso-east-1",
            "cn-north-1",
        ] {
            let binding: BedrockAiBinding =
                serde_json::from_value(serde_json::json!({ "region": ok }))
                    .unwrap_or_else(|e| panic!("{ok} must parse: {e}"));
            assert_eq!(binding.region, ok);
            assert!(BedrockAiBinding::new(ok).is_ok());
        }
        for bad in [
            "x.evil.com/",
            "us-east-1.evil.com",
            "",
            "US-EAST-1",
            "us east 1",
            "us-east-1/",
            "us-east-1@evil.com",
            "us-east",
            "useast1",
            "us--east-1",
            "-us-east-1",
            "us-east-1-",
            "us-east-1\n",
            "us-east-123",
        ] {
            let err =
                serde_json::from_value::<BedrockAiBinding>(serde_json::json!({ "region": bad }))
                    .expect_err(&format!("{bad:?} must be refused"))
                    .to_string();
            assert!(
                err.contains("invalid AWS region"),
                "unclear error for {bad:?}: {err}"
            );
            assert!(BedrockAiBinding::new(bad).is_err());
            // Also refused through the tagged forms that carry a Bedrock binding.
            assert!(serde_json::from_value::<AiBinding>(
                serde_json::json!({ "service": "bedrock", "region": bad })
            )
            .is_err());
            assert!(serde_json::from_value::<ExternalAi>(
                serde_json::json!({ "service": "bedrock", "region": bad })
            )
            .is_err());
        }
    }

    #[test]
    fn test_external_ai_rejects_typos_and_unused_keys_with_clear_errors() {
        for bad in [
            serde_json::json!({"service": "bedrok", "region": "us-east-1"}),
            serde_json::json!({"service": "bedrock"}),
            serde_json::json!({"service": "bedrock", "region": "us-east-1", "apiKey": "sk-x"}),
            serde_json::json!({"provider": "openai"}),
            serde_json::json!("bedrock"),
        ] {
            let err = serde_json::from_value::<ExternalAi>(bad.clone())
                .expect_err(&format!("{bad} must be refused"))
                .to_string();
            assert!(
                err.contains("invalid external AI binding") && err.contains("\"apiKey\""),
                "unclear error for {bad}: {err}"
            );
            assert!(!err.contains("sk-x"), "error must not echo the key: {err}");
        }
        let err = serde_json::from_value::<ExternalAi>(
            serde_json::json!({"service": "bedrock", "region": "us-east-1", "apiKey": "sk-x"}),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("`apiKey` is not allowed"), "{err}");
    }
}
