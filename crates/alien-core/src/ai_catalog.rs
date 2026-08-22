//! Curated, per-cloud model catalog for the AI gateway.
//!
//! Single source of truth for which public model ids each cloud exposes, the
//! upstream id the gateway forwards, and the wire protocol of the model's native
//! endpoint. Backs `getAvailableModels()` and the gateway's `/v1/models`, and the
//! Azure controller deploys the Azure entries as named deployments at provision
//! time (see `azure_deployments`).
//!
//! A model is includable only if its cloud serves it over a protocol the client
//! SDK already speaks (OpenAI Chat Completions or Anthropic Messages), so the
//! gateway forwards the request body untranslated.

use crate::Platform;
use serde::{Deserialize, Serialize};

/// A public API accepted from an application client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ClientApi {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

impl ClientApi {
    /// Public request protocols accepted for every text-generation model. The
    /// gateway translates to the model's provider-native protocol when needed.
    pub const ALL: [Self; 3] = [
        Self::OpenAiChatCompletions,
        Self::OpenAiResponses,
        Self::AnthropicMessages,
    ];
}

/// The provider API used for the upstream request. This is deliberately
/// separate from [`ClientApi`]: an adapter may expose one client API over a
/// different provider API, but only after that exact combination is qualified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ProviderApi {
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    OpenAi,
    /// Anthropic Messages (`/v1/messages`).
    Anthropic,
    /// OpenAI Responses on bedrock-mantle. The only API the GPT-5 family serves;
    /// the exact path is per-model, see `RESPONSES_UPSTREAM`.
    OpenAiResponses,
}

/// The one-time action, if any, a customer must take in the cloud provider before
/// the gateway can invoke a model. Static per (provider, cloud), surfaced in docs
/// and the example README. Distinct from the read-only availability observation
/// reported by the resource heartbeat for a particular deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// Enabled by default; nothing for the customer to do (quota still applies).
    OutOfBox,
    /// Needs a one-time customer action first; the string says what.
    RequiresOneTimeStep(&'static str),
}

/// One curated model: the public id an app requests, the cloud that serves it,
/// the upstream id the gateway forwards (for Azure this is the deployment name),
/// and the protocol of its native endpoint.
#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub public_id: &'static str,
    pub cloud: Platform,
    pub upstream_id: &'static str,
    /// Provider-native protocols. These choose the lowest-overhead upstream;
    /// applications may use any [`ClientApi`] through the gateway adapter.
    pub client_apis: &'static [ClientApi],
    pub provider_api: ProviderApi,
}

/// One model served directly by Anthropic. This is separate from `CatalogModel`:
/// direct Anthropic is a provider connection, not a deployable cloud platform.
#[derive(Debug, Clone, Copy)]
pub struct DirectAnthropicModel {
    pub public_id: &'static str,
    pub upstream_id: &'static str,
}

/// One OpenAI model qualified through the Gateway's direct-provider route.
///
/// OpenAI's account model listing also contains embeddings, image, audio,
/// moderation, realtime, and other APIs. Keep this list explicit so
/// `GET /v1/models` never claims that an observed provider model is callable
/// through Chat Completions or Responses when it is not.
#[derive(Debug, Clone, Copy)]
pub struct DirectOpenAiModel {
    pub public_id: &'static str,
    pub client_apis: &'static [ClientApi],
}

/// One Databricks-hosted model service qualified through Unity AI Gateway.
#[derive(Debug, Clone, Copy)]
pub struct DirectDatabricksModel {
    pub public_id: &'static str,
    pub upstream_id: &'static str,
    pub client_apis: &'static [ClientApi],
}

impl DirectAnthropicModel {
    pub fn display_name(&self) -> &'static str {
        resolve(self.public_id)
            .map(CatalogModel::display_name)
            .unwrap_or(self.public_id)
    }
}

impl CatalogModel {
    /// The model's publisher, for grouping in a picker. Derived from the public id,
    /// so the same public id reports the same provider on every cloud.
    pub fn provider(&self) -> &'static str {
        let id = self.public_id;
        if id.starts_with("claude") {
            "anthropic"
        } else if id.starts_with("gpt") || id == "model-router" {
            "openai"
        } else if id.starts_with("gemini") || id.starts_with("gemma") {
            "google"
        } else if id.starts_with("qwen") {
            "qwen"
        } else if id.starts_with("deepseek") {
            "deepseek"
        } else if id.starts_with("mistral")
            || id.starts_with("devstral")
            || id.starts_with("magistral")
            || id.starts_with("ministral")
        {
            "mistral"
        } else if id.starts_with("minimax") {
            "minimax"
        } else if id.starts_with("kimi") {
            "moonshotai"
        } else if id.starts_with("nemotron") {
            "nvidia"
        } else if id.starts_with("glm") {
            "zai"
        } else if id.starts_with("palmyra") {
            "writer"
        } else {
            "unknown"
        }
    }

    /// A human label for a model picker. Curated per id rather than derived so the
    /// acronyms (GPT, OSS, GLM, VL) and versions read correctly.
    pub fn display_name(&self) -> &'static str {
        match self.public_id {
            "gpt-5.6-sol" => "GPT-5.6 Sol",
            "gpt-5.6-terra" => "GPT-5.6 Terra",
            "gpt-5.6-luna" => "GPT-5.6 Luna",
            "gpt-5.5" => "GPT-5.5",
            "gpt-5.4" => "GPT-5.4",
            "gpt-oss-20b" => "GPT-OSS 20B",
            "gpt-oss-120b" => "GPT-OSS 120B",
            "gpt-oss-safeguard-20b" => "GPT-OSS Safeguard 20B",
            "gpt-oss-safeguard-120b" => "GPT-OSS Safeguard 120B",
            "deepseek-v3.2" => "DeepSeek V3.2",
            "qwen3-32b" => "Qwen3 32B",
            "qwen3-coder-30b" => "Qwen3 Coder 30B",
            "qwen3-next-80b" => "Qwen3 Next 80B",
            "qwen3-vl-235b" => "Qwen3 VL 235B",
            "mistral-large-3" => "Mistral Large 3",
            "devstral-2" => "Devstral 2",
            "magistral-small" => "Magistral Small",
            "ministral-3-14b" => "Ministral 3 14B",
            "ministral-3-8b" => "Ministral 3 8B",
            "ministral-3-3b" => "Ministral 3 3B",
            "minimax-m2" => "MiniMax M2",
            "minimax-m2.1" => "MiniMax M2.1",
            "minimax-m2.5" => "MiniMax M2.5",
            "kimi-k2.5" => "Kimi K2.5",
            "nemotron-nano-9b" => "Nemotron Nano 9B",
            "nemotron-nano-12b" => "Nemotron Nano 12B",
            "nemotron-nano-3-30b" => "Nemotron Nano 3 30B",
            "nemotron-super-3-120b" => "Nemotron Super 3 120B",
            "gemma-3-4b" => "Gemma 3 4B",
            "gemma-3-12b" => "Gemma 3 12B",
            "gemma-3-27b" => "Gemma 3 27B",
            "glm-4.7" => "GLM 4.7",
            "glm-4.7-flash" => "GLM 4.7 Flash",
            "glm-5" => "GLM 5",
            "palmyra-vision-7b" => "Palmyra Vision 7B",
            "claude-opus-5" => "Claude Opus 5",
            "claude-sonnet-5" => "Claude Sonnet 5",
            "claude-opus-4.8" => "Claude Opus 4.8",
            "claude-opus-4.7" => "Claude Opus 4.7",
            "claude-opus-4.6" => "Claude Opus 4.6",
            "claude-opus-4.5" => "Claude Opus 4.5",
            "claude-sonnet-4.6" => "Claude Sonnet 4.6",
            "claude-sonnet-4.5" => "Claude Sonnet 4.5",
            "claude-haiku-4.5" => "Claude Haiku 4.5",
            "claude-fable-5" => "Claude Fable 5",
            "gemini-2.5-pro" => "Gemini 2.5 Pro",
            "gemini-2.5-flash" => "Gemini 2.5 Flash",
            "gemini-2.5-flash-lite" => "Gemini 2.5 Flash Lite",
            "gemini-3.5-flash" => "Gemini 3.5 Flash",
            "gemini-3.1-flash-lite" => "Gemini 3.1 Flash Lite",
            "gpt-4.1" => "GPT-4.1",
            "gpt-4o-mini" => "GPT-4o mini",
            "model-router" => "Model Router",
            other => other,
        }
    }

    /// The one-time enablement step for this model on its cloud, if any. Only Claude
    /// needs one today, and the step differs per cloud.
    pub fn activation(&self) -> Activation {
        if !self.public_id.starts_with("claude") {
            return Activation::OutOfBox;
        }
        match self.cloud {
            Platform::Aws => Activation::RequiresOneTimeStep(
                "Submit the one-time Anthropic use-case form in the Bedrock console.",
            ),
            Platform::Gcp => Activation::RequiresOneTimeStep(
                "Enable Claude in Vertex AI Model Garden and accept Anthropic's terms of service, one-time, in the Google Cloud console.",
            ),
            Platform::Azure => Activation::RequiresOneTimeStep(
                "Accept the Marketplace terms and create the Claude deployment in the Microsoft Foundry portal (one-time).",
            ),
            _ => Activation::OutOfBox,
        }
    }
}

static CATALOG: &[CatalogModel] = &[
    // AWS Bedrock over `/openai/v1` chat completions. The plain Bedrock model id,
    // not the `us.*` cross-region inference profile — that endpoint rejects it.
    // Invoke/Converse-only models (older Llama/Mistral-v0/Nova) can't be served here.
    // The GPT-5 family is Responses-only: chat completions, converse and invoke are
    // all unavailable, so `upstream_id` here is the mantle id.
    CatalogModel {
        public_id: "gpt-5.6-sol",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-5.6-sol",
        client_apis: &[ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAiResponses,
    },
    CatalogModel {
        public_id: "gpt-5.6-terra",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-5.6-terra",
        client_apis: &[ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAiResponses,
    },
    CatalogModel {
        public_id: "gpt-5.6-luna",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-5.6-luna",
        client_apis: &[ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAiResponses,
    },
    CatalogModel {
        public_id: "gpt-5.5",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-5.5",
        client_apis: &[ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAiResponses,
    },
    CatalogModel {
        public_id: "gpt-5.4",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-5.4",
        client_apis: &[ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAiResponses,
    },
    CatalogModel {
        public_id: "gpt-oss-20b",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-oss-20b-1:0",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gpt-oss-120b",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-oss-120b-1:0",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gpt-oss-safeguard-20b",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-oss-safeguard-20b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gpt-oss-safeguard-120b",
        cloud: Platform::Aws,
        upstream_id: "openai.gpt-oss-safeguard-120b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "deepseek-v3.2",
        cloud: Platform::Aws,
        upstream_id: "deepseek.v3.2",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "qwen3-32b",
        cloud: Platform::Aws,
        upstream_id: "qwen.qwen3-32b-v1:0",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "qwen3-coder-30b",
        cloud: Platform::Aws,
        upstream_id: "qwen.qwen3-coder-30b-a3b-v1:0",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "qwen3-next-80b",
        cloud: Platform::Aws,
        upstream_id: "qwen.qwen3-next-80b-a3b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "qwen3-vl-235b",
        cloud: Platform::Aws,
        upstream_id: "qwen.qwen3-vl-235b-a22b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "mistral-large-3",
        cloud: Platform::Aws,
        upstream_id: "mistral.mistral-large-3-675b-instruct",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "devstral-2",
        cloud: Platform::Aws,
        upstream_id: "mistral.devstral-2-123b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "magistral-small",
        cloud: Platform::Aws,
        upstream_id: "mistral.magistral-small-2509",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "ministral-3-14b",
        cloud: Platform::Aws,
        upstream_id: "mistral.ministral-3-14b-instruct",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "ministral-3-8b",
        cloud: Platform::Aws,
        upstream_id: "mistral.ministral-3-8b-instruct",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "ministral-3-3b",
        cloud: Platform::Aws,
        upstream_id: "mistral.ministral-3-3b-instruct",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "minimax-m2",
        cloud: Platform::Aws,
        upstream_id: "minimax.minimax-m2",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "minimax-m2.1",
        cloud: Platform::Aws,
        upstream_id: "minimax.minimax-m2.1",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "minimax-m2.5",
        cloud: Platform::Aws,
        upstream_id: "minimax.minimax-m2.5",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "kimi-k2.5",
        cloud: Platform::Aws,
        upstream_id: "moonshotai.kimi-k2.5",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "nemotron-nano-9b",
        cloud: Platform::Aws,
        upstream_id: "nvidia.nemotron-nano-9b-v2",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "nemotron-nano-12b",
        cloud: Platform::Aws,
        upstream_id: "nvidia.nemotron-nano-12b-v2",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "nemotron-nano-3-30b",
        cloud: Platform::Aws,
        upstream_id: "nvidia.nemotron-nano-3-30b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "nemotron-super-3-120b",
        cloud: Platform::Aws,
        upstream_id: "nvidia.nemotron-super-3-120b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemma-3-4b",
        cloud: Platform::Aws,
        upstream_id: "google.gemma-3-4b-it",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemma-3-12b",
        cloud: Platform::Aws,
        upstream_id: "google.gemma-3-12b-it",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemma-3-27b",
        cloud: Platform::Aws,
        upstream_id: "google.gemma-3-27b-it",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "glm-4.7",
        cloud: Platform::Aws,
        upstream_id: "zai.glm-4.7",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "glm-4.7-flash",
        cloud: Platform::Aws,
        upstream_id: "zai.glm-4.7-flash",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "glm-5",
        cloud: Platform::Aws,
        upstream_id: "zai.glm-5",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "palmyra-vision-7b",
        cloud: Platform::Aws,
        upstream_id: "writer.palmyra-vision-7b",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    // AWS Bedrock, Claude over classic InvokeModel (the Anthropic Messages body is
    // the InvokeModel body; the model travels in the URL). `upstream_id` is the plain
    // Bedrock model id; the gateway prepends the region's cross-region inference-profile
    // geo prefix (`us.`/`eu.`/`apac.`) at request time, since Claude is invocable only
    // through a profile. Dated ids (`…-<date>-v1:0`) are required where AWS has no short
    // alias. These need Claude model access granted on the deployment's account.
    CatalogModel {
        public_id: "claude-opus-5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-opus-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-sonnet-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.8",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-opus-4-8",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.7",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-opus-4-7",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.6",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-opus-4-6-v1",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-opus-4-5-20251101-v1:0",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.6",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-sonnet-4-6",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-sonnet-4-5-20250929-v1:0",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-haiku-4.5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-haiku-4-5-20251001-v1:0",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-fable-5",
        cloud: Platform::Aws,
        upstream_id: "anthropic.claude-fable-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    // GCP Vertex, Gemini. The OpenAI-compatible Vertex endpoint expects the `google/` prefix.
    // The 2.5 family serves in-region; the 3.x models serve on the `global` location.
    CatalogModel {
        public_id: "gemini-2.5-pro",
        cloud: Platform::Gcp,
        upstream_id: "google/gemini-2.5-pro",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemini-2.5-flash",
        cloud: Platform::Gcp,
        upstream_id: "google/gemini-2.5-flash",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemini-2.5-flash-lite",
        cloud: Platform::Gcp,
        upstream_id: "google/gemini-2.5-flash-lite",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemini-3.5-flash",
        cloud: Platform::Gcp,
        upstream_id: "google/gemini-3.5-flash",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gemini-3.1-flash-lite",
        cloud: Platform::Gcp,
        upstream_id: "google/gemini-3.1-flash-lite",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    // GCP Vertex, Claude. The upstream id is the Vertex Model Garden id that travels
    // in the `:rawPredict` URL path (`publishers/anthropic/models/<id>`); models past
    // Sonnet 4.5 carry no date suffix, older ones keep an `@<date>` version. Needs
    // Claude model access granted on the deployment's project.
    CatalogModel {
        public_id: "claude-opus-5",
        cloud: Platform::Gcp,
        upstream_id: "claude-opus-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-5",
        cloud: Platform::Gcp,
        upstream_id: "claude-sonnet-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.8",
        cloud: Platform::Gcp,
        upstream_id: "claude-opus-4-8",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.7",
        cloud: Platform::Gcp,
        upstream_id: "claude-opus-4-7",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.6",
        cloud: Platform::Gcp,
        upstream_id: "claude-opus-4-6",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.5",
        cloud: Platform::Gcp,
        upstream_id: "claude-opus-4-5@20251101",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.6",
        cloud: Platform::Gcp,
        upstream_id: "claude-sonnet-4-6",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.5",
        cloud: Platform::Gcp,
        upstream_id: "claude-sonnet-4-5@20250929",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-haiku-4.5",
        cloud: Platform::Gcp,
        upstream_id: "claude-haiku-4-5@20251001",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-fable-5",
        cloud: Platform::Gcp,
        upstream_id: "claude-fable-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    // Azure, OpenAI-protocol. The upstream id is the deployment name the controller
    // creates (see AZURE_DEPLOYMENTS); the app requests it by the same id. Azure serves
    // only what is deployed, so this list must stay in sync with AZURE_DEPLOYMENTS.
    CatalogModel {
        public_id: "gpt-4.1",
        cloud: Platform::Azure,
        upstream_id: "gpt-4.1",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "gpt-4o-mini",
        cloud: Platform::Azure,
        upstream_id: "gpt-4o-mini",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    CatalogModel {
        public_id: "model-router",
        cloud: Platform::Azure,
        upstream_id: "model-router",
        client_apis: &[ClientApi::OpenAiChatCompletions],
        provider_api: ProviderApi::OpenAi,
    },
    // Azure, Claude over the Foundry Anthropic endpoint. The upstream id is the
    // Foundry deployment name (defaults to the model id). Unlike the OpenAI list,
    // these are not in AZURE_DEPLOYMENTS: a first Claude deployment requires
    // accepting Azure Marketplace terms, a portal step the controller cannot
    // perform, so Claude deployments are created in the Foundry portal. These stay
    // in the catalog as the deployment-name mapping. The resource heartbeat lists
    // actual deployments, so the gateway omits Claude until that deployment exists.
    CatalogModel {
        public_id: "claude-opus-5",
        cloud: Platform::Azure,
        upstream_id: "claude-opus-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-5",
        cloud: Platform::Azure,
        upstream_id: "claude-sonnet-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.8",
        cloud: Platform::Azure,
        upstream_id: "claude-opus-4-8",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.7",
        cloud: Platform::Azure,
        upstream_id: "claude-opus-4-7",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.6",
        cloud: Platform::Azure,
        upstream_id: "claude-opus-4-6",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-opus-4.5",
        cloud: Platform::Azure,
        upstream_id: "claude-opus-4-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.6",
        cloud: Platform::Azure,
        upstream_id: "claude-sonnet-4-6",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-sonnet-4.5",
        cloud: Platform::Azure,
        upstream_id: "claude-sonnet-4-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-haiku-4.5",
        cloud: Platform::Azure,
        upstream_id: "claude-haiku-4-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
    CatalogModel {
        public_id: "claude-fable-5",
        cloud: Platform::Azure,
        upstream_id: "claude-fable-5",
        client_apis: &[ClientApi::AnthropicMessages],
        provider_api: ProviderApi::Anthropic,
    },
];

/// Changes whenever public model ids or their supported client APIs change.
/// Heartbeat consumers use this to distinguish an old observation from a
/// current catalog without duplicating the catalog in durable state.
pub const AI_CATALOG_REVISION: &str = "2026-08-20.1";

/// Azure deployments to create at provision time: (deployment name, model name,
/// model version). The deployment name is the catalog `upstream_id`. The version
/// is validated against the target region's model catalog at deploy time.
static AZURE_DEPLOYMENTS: &[(&str, &str, &str)] = &[
    ("gpt-4.1", "gpt-4.1", "2025-04-14"),
    ("model-router", "model-router", "2025-11-18"),
];

/// Direct Anthropic aliases qualified for the native Messages API. Keep this
/// explicit: the three cloud providers use different upstream IDs and cannot be
/// used as an accidental source of direct-provider routing data.
static DIRECT_ANTHROPIC_MODELS: &[DirectAnthropicModel] = &[
    DirectAnthropicModel {
        public_id: "claude-opus-5",
        upstream_id: "claude-opus-5",
    },
    DirectAnthropicModel {
        public_id: "claude-sonnet-5",
        upstream_id: "claude-sonnet-5",
    },
    DirectAnthropicModel {
        public_id: "claude-opus-4.8",
        upstream_id: "claude-opus-4-8",
    },
    DirectAnthropicModel {
        public_id: "claude-opus-4.7",
        upstream_id: "claude-opus-4-7",
    },
    DirectAnthropicModel {
        public_id: "claude-opus-4.6",
        upstream_id: "claude-opus-4-6",
    },
    DirectAnthropicModel {
        public_id: "claude-opus-4.5",
        upstream_id: "claude-opus-4-5",
    },
    DirectAnthropicModel {
        public_id: "claude-sonnet-4.6",
        upstream_id: "claude-sonnet-4-6",
    },
    DirectAnthropicModel {
        public_id: "claude-sonnet-4.5",
        upstream_id: "claude-sonnet-4-5-20250929",
    },
    DirectAnthropicModel {
        public_id: "claude-haiku-4.5",
        upstream_id: "claude-haiku-4-5-20251001",
    },
    DirectAnthropicModel {
        public_id: "claude-fable-5",
        upstream_id: "claude-fable-5",
    },
];

/// Direct OpenAI models qualified end to end through the Gateway.
///
/// Add a model only after exercising every client API listed for it against a
/// real provider account. Provider discovery is then used as the account-level
/// availability filter; discovery alone is not sufficient to expose a model.
///
/// Every entry below was exercised live (2026-08-09) against both
/// `/v1/chat/completions` and `/v1/responses`. Note the GPT-5 family answers
/// both APIs here, unlike on bedrock-mantle where it is Responses-only.
static DIRECT_OPENAI_MODELS: &[DirectOpenAiModel] = &[
    DirectOpenAiModel {
        public_id: "gpt-5.6-sol",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-5.6-terra",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-5.6-luna",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-5.5",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-5.4",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-4.1",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-4.1-mini",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectOpenAiModel {
        public_id: "gpt-4o-mini",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
];

/// Generally available Databricks-hosted text-generation models that have an
/// exact Alien public-model ID, reviewed against Databricks' supported-models
/// catalog on 2026-08-10. Preview, deprecated, embedding, and image-generation
/// models are intentionally excluded. Credential verification is separate from
/// model access: Databricks can accept OAuth while a service is disabled by quota.
/// Source: <https://docs.databricks.com/aws/en/machine-learning/foundation-model-apis/supported-models>
static DIRECT_DATABRICKS_MODELS: &[DirectDatabricksModel] = &[
    DirectDatabricksModel {
        public_id: "gpt-5.6-sol",
        upstream_id: "databricks-gpt-5-6-sol",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-5.6-terra",
        upstream_id: "databricks-gpt-5-6-terra",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-5.6-luna",
        upstream_id: "databricks-gpt-5-6-luna",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-5.5",
        upstream_id: "databricks-gpt-5-5",
        client_apis: &[ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-5.4",
        upstream_id: "databricks-gpt-5-4",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "claude-haiku-4.5",
        upstream_id: "databricks-claude-haiku-4-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-opus-5",
        upstream_id: "system.ai.claude-opus-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-sonnet-5",
        upstream_id: "databricks-claude-sonnet-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-sonnet-4.6",
        upstream_id: "databricks-claude-sonnet-4-6",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-sonnet-4.5",
        upstream_id: "databricks-claude-sonnet-4-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-fable-5",
        upstream_id: "databricks-claude-fable-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-opus-4.8",
        upstream_id: "databricks-claude-opus-4-8",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-opus-4.7",
        upstream_id: "databricks-claude-opus-4-7",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-opus-4.6",
        upstream_id: "databricks-claude-opus-4-6",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "claude-opus-4.5",
        upstream_id: "databricks-claude-opus-4-5",
        client_apis: &[
            ClientApi::OpenAiChatCompletions,
            ClientApi::OpenAiResponses,
            ClientApi::AnthropicMessages,
        ],
    },
    DirectDatabricksModel {
        public_id: "gemini-3.5-flash",
        upstream_id: "databricks-gemini-3-5-flash",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gemini-3.1-flash-lite",
        upstream_id: "databricks-gemini-3-1-flash-lite",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-oss-120b",
        upstream_id: "databricks-gpt-oss-120b",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gpt-oss-20b",
        upstream_id: "databricks-gpt-oss-20b",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
    DirectDatabricksModel {
        public_id: "gemma-3-12b",
        upstream_id: "databricks-gemma-3-12b",
        client_apis: &[ClientApi::OpenAiChatCompletions, ClientApi::OpenAiResponses],
    },
];

/// Where a model sits on the bedrock-mantle Responses API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponsesTarget {
    /// The id mantle expects, which drops the InvokeModel version suffix.
    pub upstream_id: &'static str,
    /// Path under the mantle host. The GPT-5 family serves on `/openai/v1/responses`,
    /// the open-weight models on `/v1/responses`.
    pub path: &'static str,
}

/// AWS models servable over the bedrock-mantle OpenAI Responses API. Only a subset
/// of the chat catalog supports Responses at all — Claude is Messages-only and e.g.
/// Qwen rejects it. Kept explicit rather than derived: both the id scheme and the
/// path differ per model family, not by a rule.
static RESPONSES_UPSTREAM: &[(&str, ResponsesTarget)] = &[
    (
        "gpt-oss-20b",
        ResponsesTarget {
            upstream_id: "openai.gpt-oss-20b",
            path: "/v1/responses",
        },
    ),
    (
        "gpt-oss-120b",
        ResponsesTarget {
            upstream_id: "openai.gpt-oss-120b",
            path: "/v1/responses",
        },
    ),
    (
        "gpt-5.6-sol",
        ResponsesTarget {
            upstream_id: "openai.gpt-5.6-sol",
            path: "/openai/v1/responses",
        },
    ),
    (
        "gpt-5.6-terra",
        ResponsesTarget {
            upstream_id: "openai.gpt-5.6-terra",
            path: "/openai/v1/responses",
        },
    ),
    (
        "gpt-5.6-luna",
        ResponsesTarget {
            upstream_id: "openai.gpt-5.6-luna",
            path: "/openai/v1/responses",
        },
    ),
    (
        "gpt-5.5",
        ResponsesTarget {
            upstream_id: "openai.gpt-5.5",
            path: "/openai/v1/responses",
        },
    ),
    (
        "gpt-5.4",
        ResponsesTarget {
            upstream_id: "openai.gpt-5.4",
            path: "/openai/v1/responses",
        },
    ),
];

/// The bedrock-mantle Responses target for a public model id, or `None` when the
/// model is not servable over the Responses API.
pub fn responses_target(public_id: &str) -> Option<ResponsesTarget> {
    RESPONSES_UPSTREAM
        .iter()
        .find(|(public, _)| *public == public_id)
        .map(|(_, target)| *target)
}

pub fn models_for(cloud: Platform) -> Vec<&'static CatalogModel> {
    CATALOG.iter().filter(|m| m.cloud == cloud).collect()
}

pub fn direct_anthropic_models() -> Vec<&'static DirectAnthropicModel> {
    DIRECT_ANTHROPIC_MODELS.iter().collect()
}

pub fn resolve_direct_anthropic(public_id: &str) -> Option<&'static DirectAnthropicModel> {
    DIRECT_ANTHROPIC_MODELS
        .iter()
        .find(|model| model.public_id == public_id)
}

pub fn direct_openai_models() -> Vec<&'static DirectOpenAiModel> {
    DIRECT_OPENAI_MODELS.iter().collect()
}

pub fn resolve_direct_openai(public_id: &str) -> Option<&'static DirectOpenAiModel> {
    DIRECT_OPENAI_MODELS
        .iter()
        .find(|model| model.public_id == public_id)
}

pub fn direct_databricks_models() -> Vec<&'static DirectDatabricksModel> {
    DIRECT_DATABRICKS_MODELS.iter().collect()
}

pub fn resolve_direct_databricks(public_id: &str) -> Option<&'static DirectDatabricksModel> {
    DIRECT_DATABRICKS_MODELS
        .iter()
        .find(|model| model.public_id == public_id)
}

/// The catalog model for a public id, or `None` if it is not exposed.
///
/// First match: for an id serving on more than one cloud this is the AWS entry;
/// cloud-scoped callers use `lookup_for` via `resolve_for`.
pub fn lookup(public_id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|m| m.public_id == public_id)
}

fn lookup_for(public_id: &str, cloud: Platform) -> Option<&'static CatalogModel> {
    CATALOG
        .iter()
        .find(|m| m.public_id == public_id && m.cloud == cloud)
}

/// The catalog model for a client-sent model id on a specific cloud. A public id
/// can appear once per cloud (Claude serves on more than one), so resolution must
/// scope to the binding's cloud rather than filter a first-match lookup — the
/// first match is another cloud's entry whenever ids overlap.
pub fn resolve_for(model_id: &str, cloud: Platform) -> Option<&'static CatalogModel> {
    lookup_for(model_id, cloud).or_else(|| lookup_for(&canonical_public_id(model_id), cloud))
}

/// The catalog model for a client-sent model id, accepting the Anthropic-native
/// spellings agent CLIs actually send alongside the catalog's public ids.
///
/// Claude Code's `/model` emits ids like `claude-sonnet-4-5-20250929` or
/// `claude-haiku-4-5`, Bedrock-aware clients may carry the full upstream id
/// (`us.anthropic.claude-haiku-4-5-20251001-v1:0`), and Vertex clients the
/// `@date` form (`claude-sonnet-4-5@20250929`). Exact public ids win; otherwise
/// the id is canonicalized — vendor/geo prefix, InvokeModel `-vN[:M]` suffix,
/// and either release-date suffix drop off, and a dashed minor version becomes
/// the catalog's dotted form (`claude-haiku-4-5` → `claude-haiku-4.5`).
///
/// A public id can appear once per cloud, and this returns the first catalog
/// entry — for a multi-cloud id that is the AWS one. Callers routing by a
/// binding must use `resolve_for` with the binding's cloud.
pub fn resolve(model_id: &str) -> Option<&'static CatalogModel> {
    lookup(model_id).or_else(|| lookup(&canonical_public_id(model_id)))
}

fn canonical_public_id(model_id: &str) -> String {
    let mut id = model_id;
    if let Some(pos) = id.rfind("anthropic.") {
        id = &id[pos + "anthropic.".len()..];
    }
    // Vertex spells the release date as an `@` suffix rather than a dash.
    id = id.split_once('@').map_or(id, |(base, _)| base);
    id = strip_invoke_version(id);
    id = strip_release_date(id);
    dot_minor_version(id)
}

/// Strip an InvokeModel version suffix: `-v1:0` or `-v1`.
fn strip_invoke_version(id: &str) -> &str {
    let base = id.split_once(':').map_or(id, |(base, _)| base);
    match base.rsplit_once("-v") {
        Some((stem, digits))
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            stem
        }
        _ => base,
    }
}

/// Strip a release-date suffix: `-20251001`.
fn strip_release_date(id: &str) -> &str {
    match id.rsplit_once('-') {
        Some((stem, date))
            if date.len() == 8
                && date.starts_with("20")
                && date.bytes().all(|b| b.is_ascii_digit()) =>
        {
            stem
        }
        _ => id,
    }
}

/// Rewrite a trailing dashed minor version to the catalog's dotted form:
/// `claude-haiku-4-5` → `claude-haiku-4.5`. Whole versions (`claude-sonnet-5`)
/// are already in catalog form and pass through.
fn dot_minor_version(id: &str) -> String {
    let Some((stem, minor)) = id.rsplit_once('-') else {
        return id.to_string();
    };
    let Some((prefix, major)) = stem.rsplit_once('-') else {
        return id.to_string();
    };
    let both_numeric = !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|b| b.is_ascii_digit())
        && minor.bytes().all(|b| b.is_ascii_digit());
    if both_numeric {
        format!("{prefix}-{major}.{minor}")
    } else {
        id.to_string()
    }
}

/// The Azure predefined model deployments, as (deployment name, model name, version).
pub fn azure_deployments() -> Vec<(&'static str, &'static str, &'static str)> {
    AZURE_DEPLOYMENTS.to_vec()
}

#[cfg(test)]
mod tests {
    /// A public id may serve on more than one cloud (Claude does), but must appear at
    /// most once per cloud — a duplicate within a cloud would make `resolve_for`
    /// silently pick whichever entry comes first.
    #[test]
    fn public_ids_are_unique_per_cloud() {
        let mut seen = std::collections::HashSet::new();
        for model in super::CATALOG {
            assert!(
                seen.insert((model.cloud, model.public_id)),
                "public id '{}' appears more than once under {:?}",
                model.public_id,
                model.cloud
            );
        }
    }

    #[test]
    fn direct_anthropic_aliases_are_unique_and_round_trip() {
        let mut public_ids = std::collections::HashSet::new();
        let mut upstream_ids = std::collections::HashSet::new();
        for model in DIRECT_ANTHROPIC_MODELS {
            assert!(public_ids.insert(model.public_id));
            assert!(upstream_ids.insert(model.upstream_id));
            assert_eq!(
                resolve_direct_anthropic(model.public_id)
                    .expect("direct model must resolve")
                    .upstream_id,
                model.upstream_id
            );
            assert_ne!(model.display_name(), model.public_id);
        }
        assert!(resolve_direct_anthropic("claude-not-real").is_none());
    }

    #[test]
    fn direct_openai_models_are_unique_and_have_qualified_apis() {
        let mut public_ids = std::collections::HashSet::new();
        for model in DIRECT_OPENAI_MODELS {
            assert!(public_ids.insert(model.public_id));
            assert!(!model.client_apis.is_empty());
            assert_eq!(
                resolve_direct_openai(model.public_id)
                    .expect("direct model must resolve")
                    .client_apis,
                model.client_apis
            );
        }
        assert!(resolve_direct_openai("text-embedding-3-small").is_none());
        assert!(resolve_direct_openai("gpt-image-1").is_none());
    }

    #[test]
    fn direct_databricks_models_are_unique_and_resolve_to_provider_ids() {
        let mut public_ids = std::collections::HashSet::new();
        for model in DIRECT_DATABRICKS_MODELS {
            assert!(public_ids.insert(model.public_id));
            assert!(
                model.upstream_id.starts_with("databricks-")
                    || model.upstream_id.starts_with("system.ai.")
            );
            assert!(!model.client_apis.is_empty());
            assert_eq!(
                resolve_direct_databricks(model.public_id)
                    .expect("direct Databricks model must resolve")
                    .upstream_id,
                model.upstream_id
            );
        }
        assert!(resolve_direct_databricks("bge-large-en").is_none());
        assert!(resolve_direct_databricks("databricks-genie").is_none());
        assert_eq!(
            resolve_direct_databricks("claude-opus-5")
                .expect("Claude Opus 5 must resolve")
                .upstream_id,
            "system.ai.claude-opus-5"
        );
        assert_eq!(
            resolve_direct_databricks("gpt-5.5")
                .expect("GPT-5.5 must resolve")
                .client_apis,
            &[ClientApi::OpenAiResponses]
        );
    }

    use super::*;

    #[test]
    fn resolve_accepts_anthropic_native_spellings() {
        // Claude Code /model forms: dashed minor version, with and without date.
        assert_eq!(
            resolve("claude-haiku-4-5").unwrap().public_id,
            "claude-haiku-4.5"
        );
        assert_eq!(
            resolve("claude-sonnet-4-5-20250929").unwrap().public_id,
            "claude-sonnet-4.5"
        );
        // Full Bedrock upstream ids, with geo/vendor prefix and version suffix.
        assert_eq!(
            resolve("us.anthropic.claude-haiku-4-5-20251001-v1:0")
                .unwrap()
                .public_id,
            "claude-haiku-4.5"
        );
        assert_eq!(
            resolve("anthropic.claude-opus-4-6-v1").unwrap().public_id,
            "claude-opus-4.6"
        );
        // Whole versions are already catalog form.
        assert_eq!(
            resolve("claude-sonnet-5").unwrap().public_id,
            "claude-sonnet-5"
        );
        // Exact public ids still win untouched.
        assert_eq!(
            resolve("claude-opus-4.8").unwrap().public_id,
            "claude-opus-4.8"
        );
        assert_eq!(resolve("gpt-oss-20b").unwrap().public_id, "gpt-oss-20b");
        // Unknowns stay unknown — no fuzzy matching.
        assert!(resolve("claude-nonexistent-9-9").is_none());
        assert!(resolve("gpt-5").is_none());
    }

    #[test]
    fn aws_has_openai_and_anthropic_with_plain_ids() {
        let aws = models_for(Platform::Aws);
        assert!(!aws.is_empty());
        assert!(aws
            .iter()
            .any(|m| m.public_id == "gpt-oss-20b" && m.provider_api == ProviderApi::OpenAi));
        assert!(
            aws.iter().any(|m| m.provider_api == ProviderApi::Anthropic),
            "Claude must be included via the Anthropic protocol"
        );
        // The OpenAI endpoint rejects `us.*` cross-region profile ids.
        assert!(aws.iter().all(|m| !m.upstream_id.starts_with("us.")));
    }

    #[test]
    fn resolve_for_scopes_to_cloud() {
        // The same public id serves on more than one cloud with different upstream
        // ids, so resolution must scope to the binding's cloud.
        let aws = resolve_for("claude-opus-4.8", Platform::Aws).expect("aws claude");
        assert_eq!(aws.upstream_id, "anthropic.claude-opus-4-8");
        let gcp = resolve_for("claude-opus-4.8", Platform::Gcp).expect("gcp claude");
        assert_eq!(gcp.upstream_id, "claude-opus-4-8");
        assert_eq!(gcp.provider_api, ProviderApi::Anthropic);
        // Canonicalization applies per cloud: Claude Code's dashed release-date
        // spelling resolves to the Vertex `@date` id.
        let dated = resolve_for("claude-haiku-4-5-20251001", Platform::Gcp).expect("dated id");
        assert_eq!(dated.upstream_id, "claude-haiku-4-5@20251001");
        // A Vertex-native `@date` spelling resolves too — it is the very id the
        // GCP catalog stores upstream.
        let vertex = resolve_for("claude-sonnet-4-5@20250929", Platform::Gcp).expect("vertex id");
        assert_eq!(vertex.upstream_id, "claude-sonnet-4-5@20250929");
        // A model serving on one cloud does not resolve on another.
        assert!(resolve_for("gemini-2.5-pro", Platform::Aws).is_none());
        assert!(resolve_for("gpt-4.1", Platform::Gcp).is_none());
    }

    #[test]
    fn lookup_round_trips() {
        let m = lookup("gpt-oss-20b").expect("known model");
        assert_eq!(m.cloud, Platform::Aws);
        assert_eq!(m.provider_api, ProviderApi::OpenAi);
        assert_eq!(m.upstream_id, "openai.gpt-oss-20b-1:0");

        let c = lookup("claude-opus-4.8").expect("claude known");
        assert_eq!(c.provider_api, ProviderApi::Anthropic);

        assert!(lookup("nonexistent-model").is_none());
    }

    #[test]
    fn azure_deployments_map_to_catalog() {
        assert!(!azure_deployments().is_empty());
        for (deployment, _, _) in azure_deployments() {
            assert!(
                models_for(Platform::Azure)
                    .iter()
                    .any(|m| m.upstream_id == deployment),
                "azure deployment {deployment} must map to a catalog model"
            );
        }
    }

    #[test]
    fn api_kinds_have_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&ProviderApi::OpenAi).unwrap(),
            "\"openai\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderApi::Anthropic).unwrap(),
            "\"anthropic\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderApi::OpenAiResponses).unwrap(),
            "\"openairesponses\""
        );
        assert_eq!(
            serde_json::to_string(&ClientApi::OpenAiChatCompletions).unwrap(),
            "\"open-ai-chat-completions\""
        );
        assert_eq!(
            serde_json::to_string(&ClientApi::OpenAiResponses).unwrap(),
            "\"open-ai-responses\""
        );
        assert_eq!(
            serde_json::to_string(&ClientApi::AnthropicMessages).unwrap(),
            "\"anthropic-messages\""
        );
    }

    #[test]
    fn client_apis_are_explicit_and_non_empty() {
        for model in CATALOG {
            assert!(
                !model.client_apis.is_empty(),
                "'{}' has no supported client API",
                model.public_id
            );
        }

        let gpt_oss = resolve_for("gpt-oss-20b", Platform::Aws).unwrap();
        assert!(gpt_oss
            .client_apis
            .contains(&ClientApi::OpenAiChatCompletions));
        assert!(gpt_oss.client_apis.contains(&ClientApi::OpenAiResponses));
    }

    #[test]
    fn every_model_has_provider_display_name_and_activation() {
        for m in CATALOG {
            assert_ne!(
                m.provider(),
                "unknown",
                "no provider mapping for '{}'",
                m.public_id
            );
            assert_ne!(
                m.display_name(),
                m.public_id,
                "no curated display_name for '{}'",
                m.public_id
            );
            // Only Claude needs a one-time step; everything else is out of the box.
            let is_claude = m.public_id.starts_with("claude");
            match m.activation() {
                Activation::OutOfBox => {
                    assert!(
                        !is_claude,
                        "'{}' (Claude) must require a one-time step",
                        m.public_id
                    )
                }
                Activation::RequiresOneTimeStep(summary) => {
                    assert!(is_claude, "'{}' must be out of the box", m.public_id);
                    assert!(
                        !summary.is_empty(),
                        "'{}' step summary is empty",
                        m.public_id
                    );
                }
            }
        }
    }

    /// The gateway forwards, it does not translate, so a client picks its wire format
    /// from the model id alone. Break this and an OpenAI body reaches the Anthropic
    /// upstream, or the reverse, for a bare 400 no caller can act on.
    #[test]
    fn only_claude_ids_speak_the_anthropic_protocol() {
        for m in CATALOG {
            assert_eq!(
                m.provider_api == ProviderApi::Anthropic,
                m.public_id.starts_with("claude"),
                "'{}' is {:?} but its id says otherwise",
                m.public_id,
                m.provider_api
            );
        }
    }
}
