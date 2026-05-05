//! Per-`(ResourceType, Platform)` dispatch for Helm emitters.

use crate::emitter::HelmEmitter;
use alien_core::{ErrorData, Platform, ResourceType, Result};
use alien_error::AlienError;
use std::collections::HashMap;

#[derive(Hash, Eq, PartialEq, Clone)]
struct Key {
    resource_type: ResourceType,
    platform: Platform,
}

/// Lookup table from `(ResourceType, Platform)` to a `HelmEmitter`.
#[derive(Default)]
pub struct HelmRegistry {
    emitters: HashMap<Key, Box<dyn HelmEmitter>>,
}

impl HelmRegistry {
    /// Empty registry. Useful for tests.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Built-in K8s emitters across every resource the distribution
    /// rebuild ships.
    pub fn built_in() -> Self {
        let mut registry = Self::default();
        crate::emitters::register_built_ins(&mut registry);
        registry
    }

    /// Register a single `(resource_type, platform)` emitter. Last write
    /// wins so plugin authors can override built-ins if they need to.
    pub fn register<E>(
        &mut self,
        resource_type: impl Into<ResourceType>,
        platform: Platform,
        emitter: E,
    ) -> &mut Self
    where
        E: HelmEmitter + 'static,
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

    /// Look up an emitter.
    pub fn emitter(
        &self,
        resource_type: &ResourceType,
        platform: Platform,
    ) -> Option<&dyn HelmEmitter> {
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
    ) -> Result<&dyn HelmEmitter> {
        self.emitter(resource_type, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ImportRegistrationMissing {
                resource_type: resource_type.clone(),
                platform,
                registration_kind: "helm emitter".to_string(),
            })
        })
    }
}
