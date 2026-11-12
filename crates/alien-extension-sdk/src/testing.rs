//! Runs a hooks crate's hooks natively and in the module Alien loads, and checks they agree.
//!
//! A hook runs in the module sandboxed, with no files, network or environment, a clock at 0 and
//! a seeded random source. A hook that passes its tests natively can still fail or answer
//! differently there, so a test runs the same call both ways.

use crate::Extension;
use alien_core::hooks::{
    HookError, HookKind, HooksManifest, ImportInput, ImportOutput, LinkOutput,
};
use alien_core::Platform;
use alien_hooks_runtime::Runtime;
pub use alien_hooks_runtime::{build_module, HookModule, Limits, HOOKS_PROFILE};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Loads a hooks module in Alien's hooks runtime, as Alien loads a released one.
pub fn load_module(wasm: &[u8]) -> Result<HookModule, HookError> {
    Runtime::new()
        .and_then(|runtime| runtime.load(wasm, &hex::encode(Sha256::digest(wasm))))
        .map_err(HookError::from)
}

/// A hooks crate's extension, natively and as its module in Alien's hooks runtime.
pub struct HooksUnderTest {
    native: Extension,
    module: HookModule,
}

impl HooksUnderTest {
    /// Loads `wasm`, the module built from the crate whose `export!` gave `native`, and checks
    /// both declare the same hooks.
    pub fn new(native: Extension, wasm: &[u8]) -> Result<Self, HookError> {
        let module = load_module(wasm)?;
        let manifest = native.manifest()?;
        if manifest != module.manifest {
            return Err(format!(
                "the module declares other hooks than the crate does natively:\nnative: {manifest:?}\nmodule: {:?}",
                module.manifest
            )
            .into());
        }
        Ok(Self { native, module })
    }

    /// The hooks both declare.
    pub fn manifest(&self) -> &HooksManifest {
        &self.module.manifest
    }

    /// The module, as Alien's hooks runtime loaded it.
    pub fn module(&self) -> &HookModule {
        &self.module
    }

    /// Runs `hook` on `input` natively and in the module, with the limits Alien gives its kind,
    /// and returns the output when both give the same bytes. A hook that fails both ways returns
    /// the native error.
    pub fn call(&self, hook: &str, input: &[u8]) -> Result<Vec<u8>, Mismatch> {
        let kind = self
            .module
            .manifest
            .hooks
            .iter()
            .find(|entry| entry.name == hook)
            .map(|entry| entry.kind)
            .ok_or_else(|| Mismatch::NoSuchHook(hook.to_string()))?;
        let native = self.native.call(hook, input);
        let module = self
            .module
            .call(hook, input, &Limits::for_kind(kind))
            .map_err(module_error);
        match (native, module) {
            (Ok(native), Ok(module)) if native == module => Ok(native),
            (Err(native), Err(module)) => Err(Mismatch::BothFailed { native, module }),
            (native, module) => Err(Mismatch::Differ { native, module }),
        }
    }
}

/// The error a hook gave in the module, with whether the hook said another try may pass.
fn module_error(error: alien_error::AlienError<alien_hooks_runtime::ErrorData>) -> HookError {
    HookError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

/// Why [`HooksUnderTest::call`] has no single output.
#[derive(Debug)]
pub enum Mismatch {
    /// Neither side declares the hook.
    NoSuchHook(String),
    /// The hook failed natively and in the module.
    BothFailed {
        native: HookError,
        module: HookError,
    },
    /// The two sides gave different outputs, or one failed and the other didn't.
    Differ {
        native: Result<Vec<u8>, HookError>,
        module: Result<Vec<u8>, HookError>,
    },
}

/// Runs each fixture through its type's importer and the importer's outputs through the type's
/// link, and fails when a link grants other permission sets than the type's descriptor lists.
/// The descriptor's list comes from running the link on placeholder outputs, so a link whose
/// sets depend on the outputs fails here. `call` runs one hook, as [`HookModule::call`] does.
pub fn check_link_permission_sets(
    manifest: &HooksManifest,
    call: impl Fn(&str, &[u8]) -> Result<Vec<u8>, String>,
    fixtures: &[ImportInput],
) -> Result<(), String> {
    for fixture in fixtures {
        let resource_type = fixture.resource["config"]["type"]
            .as_str()
            .ok_or("a fixture's resource has no type")?;
        let descriptor = manifest
            .resource_types
            .get(&alien_core::ResourceType::from(resource_type))
            .ok_or_else(|| format!("the extension declares no type `{resource_type}`"))?;
        let routed = |kind: HookKind, platform: Option<Platform>| {
            manifest.hooks.iter().find(|entry| {
                entry.kind == kind
                    && entry.routing.get("resourceType").and_then(Value::as_str)
                        == Some(resource_type)
                    && platform.is_none_or(|platform| {
                        entry.routing.get("platform") == Some(&serde_json::json!(platform))
                    })
            })
        };
        let Some(link) = routed(HookKind::Link, None) else {
            continue;
        };
        let importer = routed(HookKind::Importer, Some(fixture.platform))
            .ok_or_else(|| format!("`{resource_type}` has no importer for {}", fixture.platform))?;
        let input = serde_json::to_vec(fixture).map_err(|e| e.to_string())?;
        let imported: ImportOutput = serde_json::from_slice(&call(&importer.name, &input)?)
            .map_err(|e| format!("the importer's output did not read: {e}"))?;
        let outputs = imported
            .outputs
            .ok_or_else(|| format!("`{resource_type}`'s importer returned no outputs"))?;
        let outputs = serde_json::to_vec(&outputs).map_err(|e| e.to_string())?;
        let granted: LinkOutput = serde_json::from_slice(&call(&link.name, &outputs)?)
            .map_err(|e| format!("the link's output did not read: {e}"))?;
        let mut granted = granted.permission_sets;
        let mut declared = descriptor.link.permission_sets.clone();
        granted.sort();
        declared.sort();
        if granted != declared {
            return Err(format!(
                "`{resource_type}`'s link grants {granted:?} on the outputs of a {} fixture, but {declared:?} on placeholder outputs; a link's permission sets can't depend on the outputs",
                fixture.platform
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_error_keeps_the_hook_s_retryable_flag() {
        for retryable in [true, false] {
            let mut error =
                alien_error::AlienError::new(alien_hooks_runtime::ErrorData::HookFailedWithCode {
                    extension: "acme".to_string(),
                    hook: "cloudformationModifier/0".to_string(),
                    code: "ZONE_LIMIT".to_string(),
                    message: "too many zones".to_string(),
                });
            error.retryable = retryable;
            assert_eq!(module_error(error).retryable, retryable);
        }
    }
}
