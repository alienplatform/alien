use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::{ResourceType, Storage};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// The fine-tuning method applied to the base model.
///
/// The gateway-side controllers map each variant onto the provider's native
/// technique: `Sft` is supervised fine-tuning (all three clouds), `Dpo` is
/// direct preference optimization (Bedrock/Foundry), and `Lora` requests a
/// parameter-efficient adapter where the provider exposes it. Providers that
/// only implement a subset reject unsupported methods at job-submit time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FinetuneMethod {
    /// Supervised fine-tuning on labelled prompt/response pairs (default).
    Sft,
    /// Direct preference optimization on chosen/rejected pairs.
    Dpo,
    /// Low-rank adaptation (parameter-efficient) where the provider exposes it.
    Lora,
}

impl Default for FinetuneMethod {
    fn default() -> Self {
        Self::Sft
    }
}

/// Declares that an [`Ai`] resource should fine-tune a base model in the
/// customer's cloud before serving it.
///
/// The training data lives in a customer-owned [`Storage`](crate::Storage)
/// bucket (S3 / GCS / Blob), referenced by `training_data`. At deploy time the
/// cloud controller submits the provider's tuning job (Bedrock
/// `CreateModelCustomizationJob`, Vertex tuning job, or Foundry
/// `fine_tuning.jobs`), polls it to completion via the heartbeat loop, and
/// records the resulting artifact so the gateway can route `served_model_id`
/// to it. Base-model inference is unaffected — an `Ai` without a `finetune`
/// spec behaves exactly as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinetuneSpec {
    /// Provider-native base-model identifier to tune (e.g. an Amazon Nova model
    /// id on Bedrock, a Gemini model on Vertex, or a gpt-4o family model on
    /// Foundry). Validated against the target cloud when the job is submitted.
    pub base_model: String,

    /// The storage resource holding the JSONL training dataset. The controller
    /// reads it from the customer bucket the storage resolves to; the data
    /// never leaves the customer's cloud.
    pub training_data: String,

    /// Object key of the training file within `training_data`.
    /// Defaults to `training.jsonl`.
    #[serde(default = "default_training_key", skip_serializing_if = "is_default_training_key")]
    pub training_key: String,

    /// The public model id apps use to invoke the tuned model through the
    /// gateway (the `model` field in an OpenAI-compatible request). Defaults to
    /// `<ai-id>-tuned`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_id: Option<String>,

    /// The fine-tuning method. Defaults to supervised fine-tuning.
    #[serde(default, skip_serializing_if = "is_default_method")]
    pub method: FinetuneMethod,
}

fn default_training_key() -> String {
    "training.jsonl".to_string()
}

fn is_default_training_key(key: &str) -> bool {
    key == "training.jsonl"
}

fn is_default_method(method: &FinetuneMethod) -> bool {
    *method == FinetuneMethod::default()
}

impl FinetuneSpec {
    /// The public model id the gateway serves the tuned model under, falling
    /// back to `<ai-id>-tuned` when the spec doesn't set one explicitly.
    pub fn served_model_id_or_default(&self, ai_id: &str) -> String {
        self.served_model_id
            .clone()
            .unwrap_or_else(|| format!("{ai_id}-tuned"))
    }
}

/// Represents an AI Gateway resource that provides a unified interface to
/// managed AI inference services across cloud providers.
///
/// BYO-key external providers (OpenAI/Anthropic) are NOT declared here. Like any
/// other BYO infrastructure (e.g. external Redis for `kv`), an external AI
/// provider is supplied at deploy time as an `ExternalBinding::Ai` in the
/// stack's external-bindings map; the executor then skips the cloud controller.
///
/// When `finetune` is set, the resource also tunes a base model in the
/// customer's cloud and serves the result through the same gateway (see
/// [`FinetuneSpec`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Ai {
    /// Identifier for the AI resource. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// Optional fine-tuning declaration. When present, the resource tunes
    /// `finetune.base_model` on the customer's cloud and serves the result
    /// alongside the base models. Absent for a pure inference gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finetune: Option<FinetuneSpec>,
}

impl Ai {
    /// The resource type identifier for AI Gateway
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("ai");

    /// Returns the AI resource's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Outputs generated by a successfully provisioned AI Gateway resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AiOutputs {
    /// The AI provider name (e.g., "bedrock", "vertex", "foundry", "external").
    pub provider: String,
    /// The provider endpoint URL, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The provider account or project identifier, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl ResourceOutputsDefinition for AiOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Ai::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<AiOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Ai {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // The training dataset lives in a customer Storage bucket that must be
        // provisioned before the tuning job can read it, so a finetune spec adds
        // that storage as a dependency. A pure inference gateway has none.
        match &self.finetune {
            Some(spec) => vec![ResourceRef::new(
                Storage::RESOURCE_TYPE,
                spec.training_data.clone(),
            )],
            None => Vec::new(),
        }
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_ai = new_config.as_any().downcast_ref::<Ai>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: Self::RESOURCE_TYPE,
                actual: new_config.get_resource_type(),
            })
        })?;

        if self.id != new_ai.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        // The tuned artifact is derived from the training data + base model, so
        // repointing them would silently serve a different model under the same
        // served id. Require a new resource id (hence a fresh tuning job) instead.
        if let (Some(old), Some(new)) = (&self.finetune, &new_ai.finetune) {
            if old.base_model != new.base_model || old.training_data != new.training_data {
                return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: "finetune 'baseModel' and 'trainingData' are immutable; \
                             create a new AI resource to retrain"
                        .to_string(),
                }));
            }
        }
        Ok(())
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
        other.as_any().downcast_ref::<Ai>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_builder() {
        let ai = Ai::new("llm".to_string()).build();
        assert_eq!(ai.id, "llm");
    }

    #[test]
    fn test_ai_resource_type() {
        assert_eq!(Ai::RESOURCE_TYPE.as_ref(), "ai");
    }

    #[test]
    fn test_ai_resource_definition() {
        let ai = Ai::new("test-ai".to_string()).build();
        assert_eq!(ai.get_resource_type(), Ai::RESOURCE_TYPE);
        assert_eq!(ResourceDefinition::id(&ai), "test-ai");
        assert!(ai.get_dependencies().is_empty());
    }

    #[test]
    fn test_ai_validate_update() {
        let original = Ai::new("test-ai".to_string()).build();
        let valid_update = Ai::new("test-ai".to_string()).build();
        let invalid_update = Ai::new("different-ai".to_string()).build();

        assert!(original.validate_update(&valid_update).is_ok());
        assert!(original.validate_update(&invalid_update).is_err());
    }

    #[test]
    fn test_ai_finetune_dependency() {
        let base = Ai::new("llm".to_string()).build();
        assert!(
            base.get_dependencies().is_empty(),
            "a pure inference gateway has no dependencies"
        );

        let tuned = Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: "training-set".to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build();
        assert_eq!(
            tuned.get_dependencies(),
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "training-set")],
            "a finetune spec depends on its training-data storage"
        );
    }

    #[test]
    fn test_ai_served_model_id_default() {
        let spec = FinetuneSpec {
            base_model: "b".to_string(),
            training_data: "d".to_string(),
            training_key: "training.jsonl".to_string(),
            served_model_id: None,
            method: FinetuneMethod::Sft,
        };
        assert_eq!(spec.served_model_id_or_default("llm"), "llm-tuned");

        let explicit = FinetuneSpec {
            served_model_id: Some("finance-model".to_string()),
            ..spec
        };
        assert_eq!(explicit.served_model_id_or_default("llm"), "finance-model");
    }

    #[test]
    fn test_ai_finetune_immutable_fields_rejected() {
        let original = Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: "set-a".to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build();

        // Same base + data, changed method: allowed.
        let ok = Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: "set-a".to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Dpo,
            })
            .build();
        assert!(original.validate_update(&ok).is_ok());

        // Changed training data: rejected.
        let repoint = Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: "set-b".to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build();
        let err = original
            .validate_update(&repoint)
            .expect_err("repointing training data must be rejected");
        assert_eq!(err.code, "INVALID_RESOURCE_UPDATE");
    }

    #[test]
    fn test_ai_finetune_roundtrip_camel_case() {
        let ai = Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: "training-set".to_string(),
                training_key: "data.jsonl".to_string(),
                served_model_id: Some("finance-model".to_string()),
                method: FinetuneMethod::Lora,
            })
            .build();

        let json = serde_json::to_value(&ai).unwrap();
        assert_eq!(json["finetune"]["baseModel"], "amazon.nova-lite-v1:0");
        assert_eq!(json["finetune"]["trainingData"], "training-set");
        assert_eq!(json["finetune"]["trainingKey"], "data.jsonl");
        assert_eq!(json["finetune"]["servedModelId"], "finance-model");
        assert_eq!(json["finetune"]["method"], "lora");

        let back: Ai = serde_json::from_value(json).unwrap();
        assert_eq!(ai, back);
    }

    #[test]
    fn test_ai_without_finetune_omits_field() {
        let ai = Ai::new("llm".to_string()).build();
        let json = serde_json::to_value(&ai).unwrap();
        assert!(
            json.get("finetune").is_none(),
            "finetune must be omitted when unset so the inference-only shape is unchanged"
        );
    }

    #[test]
    fn test_ai_outputs_serialization() {
        let outputs = AiOutputs {
            provider: "bedrock".to_string(),
            endpoint: Some("https://bedrock-runtime.us-east-1.amazonaws.com".to_string()),
            account: None,
        };

        let json = serde_json::to_string(&outputs).unwrap();
        let deserialized: AiOutputs = serde_json::from_str(&json).unwrap();
        assert_eq!(outputs, deserialized);
    }
}
