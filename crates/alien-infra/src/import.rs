//! Per-resource importer trait + dispatch registry.
//!
//! Importers turn a typed `ImportData` payload (from CFN Custom Resource,
//! TF provider, or Helm chart) into a runtime [`StackResourceState`]. They
//! live next to the controllers in `alien-infra` because they construct the
//! controller's typed `internal_state` struct \u2014 colocating them gives a
//! compile-time check that the importer and controller agree on the shape.
//!
//! `ImporterRegistry::built_in()` returns a registry pre-populated with every
//! built-in `(ResourceType, Platform)` importer. Plugins layer on additional
//! entries via `register(...)` against the same builder.

use alien_core::{
    import::ImportContext, ErrorData, Platform, ResourceType, Result, StackResourceState,
};
use alien_error::{AlienError, IntoAlienError};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

/// Generator-side typed payload \u2192 controller `StackResourceState` translator.
///
/// One impl per `(resource_type, cloud)` pair. Lives at
/// `alien-infra/src/<resource>/<cloud>_import.rs` next to the controller so
/// `internal_state` stays in lock-step with the controller's state struct.
pub trait ResourceImporter: Send + Sync {
    /// Typed payload this importer accepts. Wire JSON is deserialized to
    /// this type before [`Self::import`] runs, so no `serde_json::Value`
    /// leaks into the importer body.
    type ImportData: DeserializeOwned + Send + Sync;

    /// Build the typed [`StackResourceState`] for this resource. The importer
    /// owns the controller's `internal_state` shape; field-name mismatches
    /// fail at compile time.
    fn import(&self, data: Self::ImportData, ctx: &ImportContext<'_>)
        -> Result<StackResourceState>;
}

/// Dyn-safe wrapper around [`ResourceImporter`] that takes
/// `serde_json::Value` (the wire payload) and runs the typed importer
/// internally.
pub trait ErasedImporter: Send + Sync {
    fn import_json(
        &self,
        data: serde_json::Value,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState>;
}

impl<I> ErasedImporter for I
where
    I: ResourceImporter,
{
    fn import_json(
        &self,
        data: serde_json::Value,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let typed: I::ImportData = serde_json::from_value(data)
            .into_alien_error()
            .map_err(|err| {
                AlienError::new(ErrorData::JsonDeserializationFailed {
                    reason: format!(
                        "importer payload for resource '{}' platform '{}' failed to deserialize: {}",
                        ctx.resource_id, ctx.platform, err
                    ),
                })
            })?;
        self.import(typed, ctx)
    }
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct Key {
    resource_type: ResourceType,
    platform: Platform,
}

/// Lookup table from `(ResourceType, Platform)` to a [`ResourceImporter`].
///
/// `built_in()` returns the registry the manager + agent dispatch through;
/// plugins extend it via `register(...)`.
#[derive(Default)]
pub struct ImporterRegistry {
    importers: HashMap<Key, Box<dyn ErasedImporter>>,
}

impl ImporterRegistry {
    /// Empty registry. Useful for tests.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Built-in importers across every cloud the setup import path
    /// ships, **OSS subset only**.
    ///
    /// Compiled clouds depend on Cargo features:
    ///
    /// * `aws` — every AWS resource importer (storage, kv, vault, queue,
    ///   network, service-account, remote-stack-management, build,
    ///   artifact-registry, function).
    /// * `gcp` — same set plus `service_activation`.
    /// * `azure` — same set plus aux preflight-injected resources
    ///   (`azure_resource_group`, `azure_storage_account`,
    ///   `azure_container_apps_environment`, `azure_service_bus_namespace`).
    ///
    /// # Platform-only resources
    ///
    /// `container` and `container-cluster` are deliberately **not** registered
    /// here — their controllers live in `platform/crates/alien-managerx`
    /// (see ALIEN-120 for the planned `alien-platform-controllers` extraction).
    /// Platform-mode managers extend the registry on top:
    ///
    /// ```ignore
    /// let mut registry = alien_infra::ImporterRegistry::built_in();
    /// alien_platform_controllers::register_platform_importers(&mut registry);
    /// ```
    ///
    /// OSS-mode callers that encounter a `container` / `container-cluster`
    /// resource get a typed `ImportRegistrationMissing` error — explicit, not
    /// silent. That is the OSS / platform boundary.
    pub fn built_in() -> Self {
        let mut registry = Self::default();
        #[cfg(feature = "aws")]
        crate::aws_importers::register(&mut registry);
        #[cfg(feature = "gcp")]
        crate::gcp_importers::register(&mut registry);
        #[cfg(feature = "azure")]
        crate::azure_importers::register(&mut registry);
        registry
    }

    /// Register a single `(resource_type, platform)` importer. Last write
    /// wins so plugin authors can override built-ins if they need to.
    pub fn register<I>(
        &mut self,
        resource_type: impl Into<ResourceType>,
        platform: Platform,
        importer: I,
    ) -> &mut Self
    where
        I: ResourceImporter + 'static,
    {
        self.importers.insert(
            Key {
                resource_type: resource_type.into(),
                platform,
            },
            Box::new(importer),
        );
        self
    }

    /// Look up an importer.
    pub fn importer(
        &self,
        resource_type: &ResourceType,
        platform: Platform,
    ) -> Option<&dyn ErasedImporter> {
        self.importers
            .get(&Key {
                resource_type: resource_type.clone(),
                platform,
            })
            .map(|boxed| boxed.as_ref())
    }

    /// Run the registered importer for this `(resource_type, platform)` with
    /// the given untyped payload. Produces a typed
    /// `ImportRegistrationMissing` error if no importer is registered.
    pub fn run(
        &self,
        resource_type: &ResourceType,
        platform: Platform,
        data: serde_json::Value,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let importer = self.importer(resource_type, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ImportRegistrationMissing {
                resource_type: resource_type.clone(),
                platform,
                registration_kind: "importer".to_string(),
            })
        })?;
        importer.import_json(data, ctx)
    }
}
