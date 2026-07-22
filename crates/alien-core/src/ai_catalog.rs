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

/// The upstream wire protocol a model speaks. The gateway forwards to the
/// matching native endpoint; the client SDK is responsible for speaking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    OpenAi,
    /// Anthropic Messages (`/v1/messages`).
    Anthropic,
}

/// One curated model: the public id an app requests, the cloud that serves it,
/// the upstream id the gateway forwards (for Azure this is the deployment name),
/// and the protocol of its native endpoint.
#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub public_id: &'static str,
    pub cloud: Platform,
    pub upstream_id: &'static str,
    pub protocol: Protocol,
}

static CATALOG: &[CatalogModel] = &[
    // AWS Bedrock over `/openai/v1` chat completions. The plain Bedrock model id,
    // not the `us.*` cross-region inference profile — that endpoint rejects it.
    // Invoke/Converse-only models (older Llama/Mistral-v0/Nova) can't be served here.
    CatalogModel { public_id: "gpt-oss-20b", cloud: Platform::Aws, upstream_id: "openai.gpt-oss-20b-1:0", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gpt-oss-120b", cloud: Platform::Aws, upstream_id: "openai.gpt-oss-120b-1:0", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gpt-oss-safeguard-20b", cloud: Platform::Aws, upstream_id: "openai.gpt-oss-safeguard-20b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gpt-oss-safeguard-120b", cloud: Platform::Aws, upstream_id: "openai.gpt-oss-safeguard-120b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "deepseek-v3.2", cloud: Platform::Aws, upstream_id: "deepseek.v3.2", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "qwen3-32b", cloud: Platform::Aws, upstream_id: "qwen.qwen3-32b-v1:0", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "qwen3-coder-30b", cloud: Platform::Aws, upstream_id: "qwen.qwen3-coder-30b-a3b-v1:0", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "qwen3-coder-next", cloud: Platform::Aws, upstream_id: "qwen.qwen3-coder-next", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "qwen3-next-80b", cloud: Platform::Aws, upstream_id: "qwen.qwen3-next-80b-a3b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "qwen3-vl-235b", cloud: Platform::Aws, upstream_id: "qwen.qwen3-vl-235b-a22b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "mistral-large-3", cloud: Platform::Aws, upstream_id: "mistral.mistral-large-3-675b-instruct", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "devstral-2", cloud: Platform::Aws, upstream_id: "mistral.devstral-2-123b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "magistral-small", cloud: Platform::Aws, upstream_id: "mistral.magistral-small-2509", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "ministral-3-14b", cloud: Platform::Aws, upstream_id: "mistral.ministral-3-14b-instruct", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "ministral-3-8b", cloud: Platform::Aws, upstream_id: "mistral.ministral-3-8b-instruct", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "ministral-3-3b", cloud: Platform::Aws, upstream_id: "mistral.ministral-3-3b-instruct", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "minimax-m2", cloud: Platform::Aws, upstream_id: "minimax.minimax-m2", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "minimax-m2.1", cloud: Platform::Aws, upstream_id: "minimax.minimax-m2.1", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "minimax-m2.5", cloud: Platform::Aws, upstream_id: "minimax.minimax-m2.5", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "kimi-k2.5", cloud: Platform::Aws, upstream_id: "moonshotai.kimi-k2.5", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "nemotron-nano-9b", cloud: Platform::Aws, upstream_id: "nvidia.nemotron-nano-9b-v2", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "nemotron-nano-12b", cloud: Platform::Aws, upstream_id: "nvidia.nemotron-nano-12b-v2", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "nemotron-nano-3-30b", cloud: Platform::Aws, upstream_id: "nvidia.nemotron-nano-3-30b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "nemotron-super-3-120b", cloud: Platform::Aws, upstream_id: "nvidia.nemotron-super-3-120b", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemma-3-4b", cloud: Platform::Aws, upstream_id: "google.gemma-3-4b-it", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemma-3-12b", cloud: Platform::Aws, upstream_id: "google.gemma-3-12b-it", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemma-3-27b", cloud: Platform::Aws, upstream_id: "google.gemma-3-27b-it", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "glm-4.7", cloud: Platform::Aws, upstream_id: "zai.glm-4.7", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "glm-4.7-flash", cloud: Platform::Aws, upstream_id: "zai.glm-4.7-flash", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "glm-5", cloud: Platform::Aws, upstream_id: "zai.glm-5", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "palmyra-vision-7b", cloud: Platform::Aws, upstream_id: "writer.palmyra-vision-7b", protocol: Protocol::OpenAi },
    // AWS Bedrock, Claude over classic InvokeModel (the Anthropic Messages body is
    // the InvokeModel body; the model travels in the URL). `upstream_id` is the plain
    // Bedrock model id; the gateway prepends the region's cross-region inference-profile
    // geo prefix (`us.`/`eu.`/`apac.`) at request time, since Claude is invocable only
    // through a profile. Dated ids (`…-<date>-v1:0`) are required where AWS has no short
    // alias. These need Claude model access granted on the deployment's account.
    CatalogModel { public_id: "claude-sonnet-5", cloud: Platform::Aws, upstream_id: "anthropic.claude-sonnet-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.8", cloud: Platform::Aws, upstream_id: "anthropic.claude-opus-4-8", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.7", cloud: Platform::Aws, upstream_id: "anthropic.claude-opus-4-7", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.6", cloud: Platform::Aws, upstream_id: "anthropic.claude-opus-4-6-v1", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.5", cloud: Platform::Aws, upstream_id: "anthropic.claude-opus-4-5-20251101-v1:0", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.1", cloud: Platform::Aws, upstream_id: "anthropic.claude-opus-4-1-20250805-v1:0", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.6", cloud: Platform::Aws, upstream_id: "anthropic.claude-sonnet-4-6", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.5", cloud: Platform::Aws, upstream_id: "anthropic.claude-sonnet-4-5-20250929-v1:0", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-haiku-4.5", cloud: Platform::Aws, upstream_id: "anthropic.claude-haiku-4-5-20251001-v1:0", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-fable-5", cloud: Platform::Aws, upstream_id: "anthropic.claude-fable-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-mythos-5", cloud: Platform::Aws, upstream_id: "anthropic.claude-mythos-5", protocol: Protocol::Anthropic },
    // GCP Vertex, Gemini. The OpenAI-compatible Vertex endpoint expects the `google/` prefix.
    // The 2.5 family serves in-region; the 3.x models serve on the `global` location.
    CatalogModel { public_id: "gemini-2.5-pro", cloud: Platform::Gcp, upstream_id: "google/gemini-2.5-pro", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemini-2.5-flash", cloud: Platform::Gcp, upstream_id: "google/gemini-2.5-flash", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemini-2.5-flash-lite", cloud: Platform::Gcp, upstream_id: "google/gemini-2.5-flash-lite", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemini-3.5-flash", cloud: Platform::Gcp, upstream_id: "google/gemini-3.5-flash", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gemini-3.1-flash-lite", cloud: Platform::Gcp, upstream_id: "google/gemini-3.1-flash-lite", protocol: Protocol::OpenAi },
    // GCP Vertex, Claude. The upstream id is the Vertex Model Garden id that travels
    // in the `:rawPredict` URL path (`publishers/anthropic/models/<id>`); models past
    // Sonnet 4.5 carry no date suffix, older ones keep an `@<date>` version. Needs
    // Claude model access granted on the deployment's project.
    CatalogModel { public_id: "claude-sonnet-5", cloud: Platform::Gcp, upstream_id: "claude-sonnet-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.8", cloud: Platform::Gcp, upstream_id: "claude-opus-4-8", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.7", cloud: Platform::Gcp, upstream_id: "claude-opus-4-7", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.6", cloud: Platform::Gcp, upstream_id: "claude-opus-4-6", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.5", cloud: Platform::Gcp, upstream_id: "claude-opus-4-5@20251101", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.6", cloud: Platform::Gcp, upstream_id: "claude-sonnet-4-6", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.5", cloud: Platform::Gcp, upstream_id: "claude-sonnet-4-5@20250929", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-haiku-4.5", cloud: Platform::Gcp, upstream_id: "claude-haiku-4-5@20251001", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-fable-5", cloud: Platform::Gcp, upstream_id: "claude-fable-5", protocol: Protocol::Anthropic },
    // Azure, OpenAI-protocol. The upstream id is the deployment name the controller
    // creates (see AZURE_DEPLOYMENTS); the app requests it by the same id. Azure serves
    // only what is deployed, so this list must stay in sync with AZURE_DEPLOYMENTS.
    CatalogModel { public_id: "gpt-4.1", cloud: Platform::Azure, upstream_id: "gpt-4.1", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "gpt-4o-mini", cloud: Platform::Azure, upstream_id: "gpt-4o-mini", protocol: Protocol::OpenAi },
    CatalogModel { public_id: "model-router", cloud: Platform::Azure, upstream_id: "model-router", protocol: Protocol::OpenAi },
    // Azure, Claude over the Foundry Anthropic endpoint. The upstream id is the
    // Foundry deployment name (defaults to the model id). Unlike the OpenAI list,
    // these are not in AZURE_DEPLOYMENTS: a first Claude deployment requires
    // accepting Azure Marketplace terms, a portal step the controller cannot
    // perform, so Claude deployments are created in the Foundry portal. Until
    // that portal step runs, /v1/models advertises these while Foundry answers
    // "deployment not found" — a deliberate tradeoff so the deployment-name
    // contract is discoverable; the upstream 404 passes through attributably.
    CatalogModel { public_id: "claude-sonnet-5", cloud: Platform::Azure, upstream_id: "claude-sonnet-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.8", cloud: Platform::Azure, upstream_id: "claude-opus-4-8", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.7", cloud: Platform::Azure, upstream_id: "claude-opus-4-7", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.6", cloud: Platform::Azure, upstream_id: "claude-opus-4-6", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-opus-4.5", cloud: Platform::Azure, upstream_id: "claude-opus-4-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.6", cloud: Platform::Azure, upstream_id: "claude-sonnet-4-6", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-sonnet-4.5", cloud: Platform::Azure, upstream_id: "claude-sonnet-4-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-haiku-4.5", cloud: Platform::Azure, upstream_id: "claude-haiku-4-5", protocol: Protocol::Anthropic },
    CatalogModel { public_id: "claude-fable-5", cloud: Platform::Azure, upstream_id: "claude-fable-5", protocol: Protocol::Anthropic },
];

/// Azure deployments to create at provision time: (deployment name, model name,
/// model version). The deployment name is the catalog `upstream_id`. The version
/// is validated against the target region's model catalog at deploy time.
static AZURE_DEPLOYMENTS: &[(&str, &str, &str)] = &[
    ("gpt-4.1", "gpt-4.1", "2025-04-14"),
    ("gpt-4o-mini", "gpt-4o-mini", "2024-07-18"),
    ("model-router", "model-router", "2025-11-18"),
];

/// AWS models servable over the bedrock-mantle OpenAI Responses API, mapped to the
/// id that endpoint expects (mantle drops the InvokeModel version suffix, and only
/// a subset of the chat catalog supports Responses at all — Claude is Messages-only
/// and e.g. Qwen rejects it). Kept explicit rather than derived: the two id schemes
/// differ per model family, not by a rule.
static RESPONSES_UPSTREAM: &[(&str, &str)] = &[
    ("gpt-oss-20b", "openai.gpt-oss-20b"),
    ("gpt-oss-120b", "openai.gpt-oss-120b"),
];

/// The bedrock-mantle Responses-API id for a public model id, or `None` when the
/// model is not servable over the Responses API.
pub fn responses_upstream_id(public_id: &str) -> Option<&'static str> {
    RESPONSES_UPSTREAM
        .iter()
        .find(|(public, _)| *public == public_id)
        .map(|(_, upstream)| *upstream)
}

pub fn models_for(cloud: Platform) -> Vec<&'static CatalogModel> {
    CATALOG.iter().filter(|m| m.cloud == cloud).collect()
}

/// The catalog model for a public id, or `None` if it is not exposed.
///
/// First match: for an id serving on more than one cloud this is the AWS entry;
/// cloud-scoped callers use `lookup_for` via `resolve_for`.
pub fn lookup(public_id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|m| m.public_id == public_id)
}

fn lookup_for(public_id: &str, cloud: Platform) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|m| m.public_id == public_id && m.cloud == cloud)
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
        Some((stem, digits)) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            stem
        }
        _ => base,
    }
}

/// Strip a release-date suffix: `-20251001`.
fn strip_release_date(id: &str) -> &str {
    match id.rsplit_once('-') {
        Some((stem, date))
            if date.len() == 8 && date.starts_with("20") && date.bytes().all(|b| b.is_ascii_digit()) =>
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

    use super::*;

    #[test]
    fn resolve_accepts_anthropic_native_spellings() {
        // Claude Code /model forms: dashed minor version, with and without date.
        assert_eq!(resolve("claude-haiku-4-5").unwrap().public_id, "claude-haiku-4.5");
        assert_eq!(
            resolve("claude-sonnet-4-5-20250929").unwrap().public_id,
            "claude-sonnet-4.5"
        );
        // Full Bedrock upstream ids, with geo/vendor prefix and version suffix.
        assert_eq!(
            resolve("us.anthropic.claude-haiku-4-5-20251001-v1:0").unwrap().public_id,
            "claude-haiku-4.5"
        );
        assert_eq!(
            resolve("anthropic.claude-opus-4-6-v1").unwrap().public_id,
            "claude-opus-4.6"
        );
        // Whole versions are already catalog form.
        assert_eq!(resolve("claude-sonnet-5").unwrap().public_id, "claude-sonnet-5");
        // Exact public ids still win untouched.
        assert_eq!(resolve("claude-opus-4.8").unwrap().public_id, "claude-opus-4.8");
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
            .any(|m| m.public_id == "gpt-oss-20b" && m.protocol == Protocol::OpenAi));
        assert!(
            aws.iter().any(|m| m.protocol == Protocol::Anthropic),
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
        assert_eq!(gcp.protocol, Protocol::Anthropic);
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
        assert_eq!(m.protocol, Protocol::OpenAi);
        assert_eq!(m.upstream_id, "openai.gpt-oss-20b-1:0");

        let c = lookup("claude-opus-4.8").expect("claude known");
        assert_eq!(c.protocol, Protocol::Anthropic);

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
    fn protocol_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Protocol::OpenAi).unwrap(), "\"openai\"");
        assert_eq!(serde_json::to_string(&Protocol::Anthropic).unwrap(), "\"anthropic\"");
    }
}
