use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::traits::{Kv, Storage};
use alien_bindings::BindingsProviderApi;
use alien_commands::server::{CommandDispatcher, CommandRegistry};
use alien_core::Platform;

/// Resources alien-manager needs for its own operation.
///
/// Contains the KV store, storage, dispatcher, and registry used by the
/// command server, plus bindings providers for cross-account registry access.
pub struct ServerBindings {
    /// General-purpose KV store for manager operational data.
    ///
    /// Used by:
    /// - Command server: command state, leases, params (`cmd:*`, `target:*`)
    pub kv: Arc<dyn Kv>,
    /// Storage for large command payloads.
    pub command_storage: Arc<dyn Storage>,
    /// Dispatcher for push-model command delivery (Lambda/PubSub/ServiceBus).
    pub command_dispatcher: Arc<dyn CommandDispatcher>,
    /// Registry for command metadata (source of truth).
    pub command_registry: Arc<dyn CommandRegistry>,
    /// Optional artifact registry for cross-account image access.
    pub artifact_registry: Option<Arc<dyn Storage>>,
    /// Primary bindings provider (fallback for single-cloud / private manager mode).
    pub bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    /// Per-target-platform bindings providers. Each cloud has its own artifact
    /// registry (ECR/GAR/ACR), so `reconcile_registry_access()` looks up the
    /// provider matching the deployment's platform. Falls back to
    /// `bindings_provider` if the target platform is not in this map.
    pub target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    /// Routing table mapping repo path prefixes to upstream registries.
    /// Built at startup and immutable afterwards.
    pub registry_routing_table: Arc<crate::routes::registry_proxy::RegistryRoutingTable>,
}
