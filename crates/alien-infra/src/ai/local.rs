use std::time::Duration;

use tracing::info;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs, ExternalAiHeartbeatData,
    HeartbeatBackend, Platform, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// The env var the developer sets to bring their own provider key for local dev.
const API_KEY_ENV: &str = AiBinding::LOCAL_API_KEY_ENV;
/// The default provider when running locally; the SDK maps it to the provider's
/// OpenAI-compatible base URL (overridable via `ALIEN_AI_LOCAL_BASE_URL`).
const DEFAULT_PROVIDER: &str = AiBinding::LOCAL_DEFAULT_PROVIDER;

/// Local AI controller. There is no cloud LLM to provision and no ambient identity on
/// a developer's machine, so the developer *brings their own key* (`OPENAI_API_KEY`) and
/// the app calls the provider directly. `get_binding_params` syncs the key-less binding
/// coordinates; the key itself reaches a linked workload through the runtime-only channel
/// (`LocalBindingsProvider::resolve_runtime_only_binding_env`), so it never enters
/// persisted or control-plane-synced state.
#[controller]
pub struct LocalAiController {
    /// The BYO-key provider (e.g. "openai"). None until create_start runs.
    pub(crate) provider: Option<String>,
}

#[controller]
impl LocalAiController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;

        // Fail loud if the developer hasn't supplied a key — local AI has no ambient
        // identity to fall back on, so this is a required, actionable precondition.
        if std::env::var(API_KEY_ENV).map(|k| k.is_empty()).unwrap_or(true) {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: format!(
                    "local AI resource '{}' needs a provider API key: set {API_KEY_ENV} before `alien dev`",
                    config.id
                ),
            }));
        }

        self.provider = Some(DEFAULT_PROVIDER.to_string());
        info!(id=%config.id, provider=DEFAULT_PROVIDER, "Local AI (BYO-key) controller: no resource to create");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    // Loops as a heartbeat tick; there is no per-stack resource to poll.

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let provider = self.provider.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Provider not set in state".to_string(),
            })
        })?;
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Local,
            backend: HeartbeatBackend::Local,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::External(ExternalAiHeartbeatData {
                status: AiHeartbeatStatus::default(),
                provider,
            })),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Ai has no mutable fields -- update is a no-op that also recovers RefreshFailed.

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id=%config.id, "Local AI update (no-op -- no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    // No cloud resource is created; deletion is always a no-op.

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id=%config.id, "Local AI delete (no-op -- nothing was provisioned)");
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────

    terminal_state!(state = CreateFailed, status = ResourceStatus::ProvisionFailed);
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = RefreshFailed, status = ResourceStatus::RefreshFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let provider = self.provider.as_ref()?;
        Some(ResourceOutputs::new(AiOutputs {
            provider: provider.clone(),
            endpoint: None,
            account: None,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Synced channel (control-plane `remote_binding_params`): emit the provider only,
        // with the key stripped so a BYO-key never reaches synced/persisted state. The
        // gateway reads the key from the never-synced worker-env channel below.
        let provider = match &self.provider {
            Some(p) => p,
            None => return Ok(None),
        };
        let mut value = serde_json::to_value(AiBinding::external(provider.clone(), String::new()))
            .into_alien_error()
            .context(ErrorData::ResourceStateSerializationFailed {
                resource_id: "binding".to_string(),
                message: "Failed to serialize AI binding parameters".to_string(),
            })?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("apiKey");
        }
        Ok(Some(value))
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ResourceController;

    #[test]
    fn synced_binding_carries_the_provider_but_never_the_key() {
        // The control-plane-synced channel must expose the provider (so the resource's
        // shape is visible) while stripping the BYO key -- a secret never belongs in
        // synced or persisted state. The worker-env channel (resolve_binding_params)
        // is where the key travels, and it is never synced.
        let controller = LocalAiController {
            provider: Some("openai".to_string()),
            ..Default::default()
        };
        let value = controller
            .get_binding_params()
            .expect("binding params serialize")
            .expect("a provisioned controller emits a binding");
        assert_eq!(value.get("provider").and_then(|v| v.as_str()), Some("openai"));
        assert!(
            value.get("apiKey").is_none(),
            "the synced binding must never carry the provider key: {value}"
        );
    }
}
