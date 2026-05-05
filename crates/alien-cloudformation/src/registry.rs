//! Per-`(ResourceType, Platform)` dispatch for CloudFormation emitters.
//!
//! `CfRegistry::built_in()` returns a registry pre-populated with every
//! built-in AWS emitter. Plugins layer on additional `(ResourceType, Platform)`
//! entries via `register(...)`.

use crate::emitter::CfEmitter;
use alien_core::{ErrorData, Platform, ResourceType, Result};
use alien_error::AlienError;
use std::collections::HashMap;

#[derive(Hash, Eq, PartialEq, Clone)]
struct Key {
    resource_type: ResourceType,
    platform: Platform,
}

/// Lookup table from `(ResourceType, Platform)` to a `CfEmitter`.
#[derive(Default)]
pub struct CfRegistry {
    emitters: HashMap<Key, Box<dyn CfEmitter>>,
}

impl CfRegistry {
    /// Empty registry. Useful for tests.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Built-in AWS emitters for every distribution-supported resource.
    pub fn built_in() -> Self {
        let mut registry = Self::default();
        crate::built_ins::register_aws(&mut registry);
        registry
    }

    /// Register a single `(resource_type, platform)` emitter. Last write wins
    /// so plugin authors can override built-ins if they need to.
    pub fn register<E>(
        &mut self,
        resource_type: impl Into<ResourceType>,
        platform: Platform,
        emitter: E,
    ) -> &mut Self
    where
        E: CfEmitter + 'static,
    {
        self.emitters.insert(
            Key {
                resource_type: resource_type.into(),
                platform,
            },
            Box::new(emitter),
        );
        self
    }

    /// Look up an emitter. The generator returns a typed error if a registered
    /// resource has no emitter \u2014 see [`Self::require`].
    pub fn emitter(
        &self,
        resource_type: &ResourceType,
        platform: Platform,
    ) -> Option<&dyn CfEmitter> {
        self.emitters
            .get(&Key {
                resource_type: resource_type.clone(),
                platform,
            })
            .map(|boxed| boxed.as_ref())
    }

    /// Look up an emitter and produce a typed `ImportRegistrationMissing`
    /// error if none is registered.
    pub fn require(
        &self,
        resource_type: &ResourceType,
        platform: Platform,
    ) -> Result<&dyn CfEmitter> {
        self.emitter(resource_type, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ImportRegistrationMissing {
                resource_type: resource_type.clone(),
                platform,
                registration_kind: "cloudformation emitter".to_string(),
            })
        })
    }
}
