//! The importer trait: turns a typed `ImportData` payload into a runtime
//! [`StackResourceState`].
//!
//! The trait lives here rather than next to the importers in `alien-infra`
//! so that code built for `wasm32-wasip1`, which cannot depend on
//! `alien-infra` and its cloud clients, can implement it.

use crate::{import::ImportContext, Result, StackResourceState};
use serde::de::DeserializeOwned;

/// Generator-side typed payload → controller `StackResourceState` translator.
///
/// One impl per `(resource_type, cloud)` pair. Alien's own impls live at
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

    /// Merge a fresh setup import into an existing runtime state.
    ///
    /// Most imported resources are wholly setup-owned, so replacement is the
    /// safe default. Importers for resources whose lifecycle crosses the
    /// setup/runtime boundary must override this and preserve controller-owned
    /// progress while applying only the setup-owned facts from `imported`.
    fn merge_reimport(
        &self,
        _existing: StackResourceState,
        imported: StackResourceState,
        _ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        Ok(imported)
    }
}
