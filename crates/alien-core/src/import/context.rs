use crate::{ManagementConfig, Platform, ResourceEntry, Stack, StackSettings};
use indexmap::IndexMap;

/// Context passed to generator-side import emitters.
#[derive(Debug, Clone, Copy)]
pub struct EmitContext<'a> {
    /// The source stack being rendered.
    pub stack: &'a Stack,
    /// The stack resource entry currently being emitted.
    pub resource: &'a ResourceEntry,
    /// Stable resource id for the current entry.
    pub resource_id: &'a str,
    /// Target platform for this emission pass.
    pub platform: Platform,
    /// User-selected stack settings for the distribution artifact.
    pub stack_settings: &'a StackSettings,
    /// Stable names precomputed by the outer generator.
    pub names: &'a IndexMap<String, String>,
}

impl<'a> EmitContext<'a> {
    /// Returns the precomputed format-specific name for a resource id.
    pub fn name_for(&self, resource_id: &str) -> Option<&'a str> {
        self.names.get(resource_id).map(String::as_str)
    }
}

/// Context passed to manager- or agent-side importers.
#[derive(Debug, Clone, Copy)]
pub struct ImportContext<'a> {
    /// Resource id currently being imported.
    pub resource_id: &'a str,
    /// Target platform for this imported resource.
    pub platform: Platform,
    /// Region or location reported by the distribution artifact.
    pub region: &'a str,
    /// Stack settings supplied by the distribution artifact.
    pub stack_settings: &'a StackSettings,
    /// Platform-derived management configuration.
    pub management_config: &'a ManagementConfig,
    /// Original resource entry from the active stack.
    pub resource: &'a ResourceEntry,
}
