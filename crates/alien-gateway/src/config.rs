//! Build the gateway's routing table from the bindings the runtime injects.
//!
//! Each `ai` resource the workload links is injected as `ALIEN_<NAME>_BINDING` JSON.
//! That env-var namespace is shared with every other resource type, so we parse each
//! value as an `AiBinding` and keep only the ones that match (a non-AI binding carries
//! no matching AI `service` tag, so it fails to parse). `resolve_route` then attaches
//! the cloud's ambient credential to produce a route the proxy can serve.

use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::provider::LazyEnvBindingsProvider;
use alien_bindings::BindingsProvider;
use alien_core::bindings::AiBinding;
use alien_core::{
    AzureCredentials, ClientConfig, GcpCredentials, Platform, ENV_ALIEN_DEPLOYMENT_TOKEN,
    ENV_ALIEN_DEPLOYMENT_TYPE, ENV_ALIEN_MANAGER_URL,
};
use alien_error::{AlienError, Context, IntoAlienError};

use crate::creds::{AmbientCred, AwsSigV4Cred, BearerTokenCred};
use crate::error::{ErrorData, Result};
use crate::{GatewayBinding, GatewayRoute, TunedRoute};

/// The shared workload credential resolver used for the mint-gated (runtime-less) path.
pub type Managed = Arc<LazyEnvBindingsProvider>;

/// Build the shared credential resolver once, only when the runtime-less mint gate is present
/// (`ALIEN_MANAGER_URL` + `ALIEN_DEPLOYMENT_TOKEN`). The gateway holds it for the process so
/// every request re-resolves through it and short-lived minted credentials refresh before
/// they expire. Absent (a projected-identity workload) → `None`, and routes fall back to the
/// self-refreshing native credential path.
pub fn managed_provider() -> Result<Option<Managed>> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let mint_gate = env.contains_key(ENV_ALIEN_MANAGER_URL) && env.contains_key(ENV_ALIEN_DEPLOYMENT_TOKEN);
    if !mint_gate {
        return Ok(None);
    }
    // Past the gate, `from_env_lazy` only fails on an unusable deployment type or malformed
    // binding JSON. Swallowing that would sign upstream calls with whatever ambient identity
    // the host happens to carry, instead of the workload's.
    let provider = BindingsProvider::from_env_lazy(env).context(ErrorData::Other {
        message: "the workload credential resolver could not be built".to_string(),
    })?;
    Ok(Some(Arc::new(provider)))
}

/// AAD audience for an Azure AI Services / Foundry account token.
const AZURE_AI_AUDIENCE: &str = "https://cognitiveservices.azure.com";

/// The workload's resolved cloud credentials, from the runtime-less credential resolver: the
/// native/projected identity when present, else Alien-minted short-lived credentials (the
/// resolver in `alien-bindings` mints against the manager when `ALIEN_MANAGER_URL` +
/// `ALIEN_DEPLOYMENT_TOKEN` are set). `None` when neither is available (e.g. a projected
/// instance-role container that carries no explicit config), so the caller falls back to the
/// SDK default chain / metadata server, which resolve the same projected identity.
async fn workload_client_config() -> Result<Option<ClientConfig>> {
    let env: HashMap<String, String> = std::env::vars().collect();
    // No deployment type: the gateway is running outside an Alien workload (local dev, the
    // standalone launcher), so there is no resolved identity and the caller falls back to the
    // SDK default chain / metadata server. Any other failure is real.
    if !env.contains_key(ENV_ALIEN_DEPLOYMENT_TYPE) {
        return Ok(None);
    }
    let lazy = BindingsProvider::from_env_lazy(env).context(ErrorData::Other {
        message: "the workload credential resolver could not be built".to_string(),
    })?;
    let provider = lazy.provider().await.context(ErrorData::AmbientCredentialUnavailable {
        message: "the workload credential resolver failed".to_string(),
    })?;
    Ok(Some(provider.client_config().clone()))
}

/// The workload's GCP access token when the resolver produced a ready OAuth2 token (the minted /
/// explicit case). `None` for any other variant, so the caller falls back to the metadata server.
async fn gcp_access_token() -> Result<Option<String>> {
    let Some(config) = workload_client_config().await? else {
        return Ok(None);
    };
    Ok(config.gcp_config().and_then(|g| match &g.credentials {
        GcpCredentials::AccessToken { token } => Some(token.clone()),
        _ => None,
    }))
}

/// The workload's Azure access token when the resolver produced a ready bearer token; `None`
/// otherwise, so the caller falls back to Azure IMDS (the projected-identity case).
async fn azure_access_token() -> Result<Option<String>> {
    let Some(config) = workload_client_config().await? else {
        return Ok(None);
    };
    Ok(config.azure_config().and_then(|a| match &a.credentials {
        AzureCredentials::AccessToken { token } => Some(token.clone()),
        _ => None,
    }))
}

/// Parse each `ALIEN_<NAME>_BINDING` env var into a `GatewayBinding`, skipping non-AI
/// bindings and the External BYO-key variant.
pub fn bindings_from_env() -> Result<Vec<GatewayBinding>> {
    bindings_from_pairs(std::env::vars())
}

/// Like [`bindings_from_env`], but over an explicit env map instead of `std::env::vars`.
pub fn bindings_from_env_map(env: &HashMap<String, String>) -> Result<Vec<GatewayBinding>> {
    bindings_from_pairs(env.iter().map(|(k, v)| (k.clone(), v.clone())))
}

/// The `service` tags `AiBinding` deserializes. A binding carrying any other tag belongs to
/// another resource type sharing the `ALIEN_<NAME>_BINDING` namespace.
const AI_SERVICE_TAGS: [&str; 4] = ["bedrock", "vertex", "foundry", "external"];

#[derive(serde::Deserialize)]
struct ServiceTag {
    service: String,
}

/// The binding's canonical name — the same decode `alien-bindings` applies, so the gateway's
/// route keys match the resolver's binding-map keys and a caller can append the resource's
/// own id (`/<name>/v1`).
fn canonical_binding_name(env_key_name: &str) -> String {
    env_key_name.to_lowercase().replace('_', "-")
}

fn bindings_from_pairs(
    pairs: impl Iterator<Item = (String, String)>,
) -> Result<Vec<GatewayBinding>> {
    let mut bindings = Vec::new();
    for (key, value) in pairs {
        let Some(name) = key.strip_prefix("ALIEN_").and_then(|k| k.strip_suffix("_BINDING")) else {
            continue;
        };
        // Not an AI binding: another resource type in the shared namespace.
        let Ok(tag) = serde_json::from_str::<ServiceTag>(&value) else {
            continue;
        };
        if !AI_SERVICE_TAGS.contains(&tag.service.as_str()) {
            continue;
        }
        let name = canonical_binding_name(name);
        // Ours, but unparseable. Skipping would drop the route and leave the app calling a
        // gateway that serves nothing, so surface it here.
        let binding: AiBinding = serde_json::from_str(&value).into_alien_error().context(
            ErrorData::BindingConfigInvalid {
                binding: name.clone(),
                message: format!("the '{}' binding could not be parsed", tag.service),
            },
        )?;
        if let Some(binding) = gateway_binding(&name, binding) {
            bindings.push(binding);
        }
    }
    Ok(bindings)
}

/// Map a parsed `AiBinding` to a `GatewayBinding`, or `None` for variants the gateway
/// does not serve. The binding name is the canonical path segment (lowercased, as the
/// env-var key encodes it).
fn gateway_binding(name: &str, binding: AiBinding) -> Option<GatewayBinding> {
    // A managed binding may carry a tuned model the gateway serves alongside the
    // static catalog. Read it before consuming the binding into its variant.
    let tuned = binding.tuned_model().map(|t| TunedRoute {
        served_id: t.served_id.clone(),
        upstream_id: t.upstream_id.clone(),
    });
    match binding {
        AiBinding::Bedrock(b) => Some(GatewayBinding {
            name: name.to_string(),
            cloud: Platform::Aws,
            region: Some(b.region),
            project: None,
            azure_endpoint: None,
            tuned,
        }),
        AiBinding::Vertex(b) => Some(GatewayBinding {
            name: name.to_string(),
            cloud: Platform::Gcp,
            region: Some(b.location),
            project: Some(b.project),
            azure_endpoint: None,
            tuned,
        }),
        AiBinding::Foundry(b) => Some(GatewayBinding {
            name: name.to_string(),
            cloud: Platform::Azure,
            region: None,
            project: None,
            azure_endpoint: Some(b.endpoint),
            tuned,
        }),
        // External is a BYO-key provider, not an ambient-managed cloud — not served here.
        AiBinding::External(_) => None,
    }
}

/// Attach the binding's cloud ambient credential, producing a route the proxy serves.
pub async fn resolve_route(binding: GatewayBinding, managed: Option<&Managed>) -> Result<GatewayRoute> {
    // The gateway authorizes upstream calls with the workload's own identity. Both paths
    // handle either shape the resolver can select: minted short-lived credentials, which each
    // request re-resolves so they refresh before expiry, or the workload's projected identity,
    // which the SDK default chain / metadata server resolves and self-refreshes. An explicitly
    // supplied token is used as-is — it carries no refresh clock.
    let cred = match binding.cloud {
        Platform::Aws => {
            let region = binding.region.clone().ok_or_else(|| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding: binding.name.clone(),
                    message: "an AWS binding needs a region".to_string(),
                })
            })?;
            match managed {
                Some(p) => AmbientCred::Aws(AwsSigV4Cred::managed(region, p.clone())),
                None => match workload_client_config().await? {
                    Some(config) if config.aws_config().is_some() => {
                        AmbientCred::Aws(AwsSigV4Cred::from_client_config(region, &config).await?)
                    }
                    _ => AmbientCred::Aws(AwsSigV4Cred::new(region).await?),
                },
            }
        }
        Platform::Gcp => match managed {
            Some(p) => AmbientCred::Bearer(BearerTokenCred::managed_gcp(p.clone())),
            None => match gcp_access_token().await? {
                Some(token) => AmbientCred::Bearer(BearerTokenCred::static_token(token)),
                None => AmbientCred::Bearer(BearerTokenCred::gcp()),
            },
        },
        Platform::Azure => match managed {
            Some(p) => AmbientCred::Bearer(BearerTokenCred::managed_azure(p.clone(), AZURE_AI_AUDIENCE)),
            None => match azure_access_token().await? {
                Some(token) => AmbientCred::Bearer(BearerTokenCred::static_token(token)),
                None => AmbientCred::Bearer(BearerTokenCred::azure(AZURE_AI_AUDIENCE)),
            },
        },
        other => {
            return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                binding: binding.name.clone(),
                message: format!("the AI gateway does not serve {other:?} bindings"),
            }))
        }
    };

    Ok(GatewayRoute {
        name: binding.name,
        cloud: binding.cloud,
        region: binding.region,
        project: binding.project,
        azure_endpoint: binding.azure_endpoint,
        cred,
        upstream_base_override: None,
        tuned: binding.tuned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_managed_binding_to_its_cloud() {
        let aws = gateway_binding("llm", AiBinding::bedrock("us-east-2")).unwrap();
        assert_eq!(aws.cloud, Platform::Aws);
        assert_eq!(aws.region.as_deref(), Some("us-east-2"));

        let gcp = gateway_binding("llm", AiBinding::vertex("proj", "us-central1")).unwrap();
        assert_eq!(gcp.cloud, Platform::Gcp);
        assert_eq!(gcp.region.as_deref(), Some("us-central1"));
        assert_eq!(gcp.project.as_deref(), Some("proj"));

        let azure =
            gateway_binding("llm", AiBinding::foundry("https://x.openai.azure.com/", "acct")).unwrap();
        assert_eq!(azure.cloud, Platform::Azure);
        assert_eq!(azure.azure_endpoint.as_deref(), Some("https://x.openai.azure.com/"));
    }

    #[test]
    fn skips_external_byo_key_binding() {
        assert!(gateway_binding("llm", AiBinding::external("openai", "sk-x")).is_none());
    }

    #[test]
    fn parses_an_ai_binding_from_the_env_var() {
        temp_env::with_var(
            "ALIEN_LLM_BINDING",
            Some(r#"{"service":"foundry","endpoint":"https://x.openai.azure.com/","account":"x"}"#),
            || {
                let binding = bindings_from_env()
                    .expect("the env holds a valid AI binding")
                    .into_iter()
                    .find(|b| b.name == "llm")
                    .expect("the llm binding should be parsed from the env");
                assert_eq!(binding.cloud, Platform::Azure);
                assert_eq!(binding.azure_endpoint.as_deref(), Some("https://x.openai.azure.com/"));
            },
        );
    }

    /// The route key is the resource's own id, so a caller appends `/<name>/v1` without
    /// knowing how the env var encoded it.
    #[test]
    fn the_route_key_is_the_canonical_binding_name() {
        temp_env::with_var(
            "ALIEN_MY_LLM_BINDING",
            Some(r#"{"service":"bedrock","region":"us-east-2"}"#),
            || {
                let bindings = bindings_from_env().expect("the env holds a valid AI binding");
                assert_eq!(bindings.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["my-llm"]);
            },
        );
    }

    #[test]
    fn ignores_non_ai_bindings_in_the_shared_namespace() {
        temp_env::with_var(
            "ALIEN_DB_BINDING",
            Some(r#"{"connectionUrl":"postgres://localhost/db"}"#),
            || {
                let bindings = bindings_from_env().expect("a non-AI binding is not an error");
                assert!(
                    !bindings.iter().any(|b| b.name == "db"),
                    "a non-AI binding must not be picked up by the gateway"
                );
            },
        );
    }

    /// Another resource type's binding carries its own `service` tag; that is not ours.
    #[test]
    fn ignores_a_non_ai_service_tag() {
        temp_env::with_var(
            "ALIEN_DB_BINDING",
            Some(r#"{"service":"aurora","clusterEndpoint":"x","port":5432}"#),
            || {
                let bindings = bindings_from_env().expect("a non-AI service tag is not an error");
                assert!(bindings.is_empty(), "a postgres binding must not be picked up");
            },
        );
    }

    /// Skipping a malformed AI binding would drop its route and leave the app calling a
    /// gateway that serves nothing, so it must fail loudly instead.
    #[test]
    fn a_malformed_ai_binding_is_an_error() {
        temp_env::with_var("ALIEN_LLM_BINDING", Some(r#"{"service":"bedrock"}"#), || {
            let err = bindings_from_env().expect_err("a bedrock binding with no region must fail");
            assert_eq!(err.code, "GATEWAY_BINDING_CONFIG_INVALID");
        });
    }

    /// `AI_SERVICE_TAGS` is hand-maintained, so a new `AiBinding` variant added without
    /// updating it would be silently dropped as "not an AI binding". Pin the two together:
    /// every variant's serialized `service` tag must be listed, and nothing extra.
    #[test]
    fn ai_service_tags_match_the_binding_variants() {
        let tag_of = |b: &AiBinding| {
            serde_json::to_value(b).unwrap()["service"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let mut from_variants = [
            tag_of(&AiBinding::bedrock("us-east-1")),
            tag_of(&AiBinding::vertex("p", "us-central1")),
            tag_of(&AiBinding::foundry("https://x", "a")),
            tag_of(&AiBinding::external("openai", "sk-x")),
        ];
        from_variants.sort_unstable();
        let mut declared = AI_SERVICE_TAGS.map(str::to_string);
        declared.sort_unstable();
        assert_eq!(
            from_variants.as_slice(),
            declared.as_slice(),
            "AI_SERVICE_TAGS drifted from AiBinding's variants; update the array"
        );
    }
}
