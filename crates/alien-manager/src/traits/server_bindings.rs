use std::sync::Arc;

use alien_bindings::traits::{Kv, Storage};
use alien_bindings::BindingsProviderApi;
use alien_commands::server::{CommandDispatcher, CommandRegistry};

/// Resources alien-manager needs for its own operation.
///
/// Contains the KV store, storage, dispatcher, and registry used by the
/// command server, plus an optional artifact registry and bindings provider.
pub struct ServerBindings {
    /// KV store for command operational data (state, leases, params).
    pub command_kv: Arc<dyn Kv>,
    /// Storage for large command payloads.
    pub command_storage: Arc<dyn Storage>,
    /// Dispatcher for push-model command delivery (Lambda/PubSub/ServiceBus).
    pub command_dispatcher: Arc<dyn CommandDispatcher>,
    /// Registry for command metadata (source of truth).
    pub command_registry: Arc<dyn CommandRegistry>,
    /// Optional artifact registry for cross-account image access.
    pub artifact_registry: Option<Arc<dyn Storage>>,
    /// Optional bindings provider for accessing cloud resources.
    pub bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
}
