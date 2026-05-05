//! Plugin extension regression — registering a custom emitter on top of
//! `HelmRegistry::built_in()` adds a new `infrastructure.<id>` entry
//! without touching the rest of the chart.

use alien_core::{
    import::EmitContext, Platform, ResourceDefinition, ResourceLifecycle, ResourceRef,
    ResourceType, Result, Stack, StackSettings,
};
use alien_helm::{
    generate_helm_chart, HelmEmitter, HelmFragment, HelmOptions, HelmRegistry, InfrastructureValue,
};
use indexmap::indexmap;
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCache {
    id: String,
}

impl PluginCache {
    const RESOURCE_TYPE: ResourceType = ResourceType::from_static("plugin-cache");
}

impl ResourceDefinition for PluginCache {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn get_dependencies(&self) -> Vec<ResourceRef> {
        vec![]
    }
    fn validate_update(&self, _new_config: &dyn ResourceDefinition) -> Result<()> {
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }
    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[derive(Debug, Default)]
struct PluginCacheEmitter;

impl HelmEmitter for PluginCacheEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(HelmFragment::default().with_infrastructure(InfrastructureValue {
            id: ctx.resource_id.to_string(),
            binding_type: "kv".to_string(),
            service: "memcached".to_string(),
            fields: indexmap! {
                "endpoint".to_string() => format!("memcached.{}.svc.cluster.local", ctx.resource_id),
            },
        }))
    }
}

#[test]
fn plugin_emitter_extends_infrastructure_yaml() {
    let mut registry = HelmRegistry::built_in();
    registry.register(
        PluginCache::RESOURCE_TYPE,
        Platform::Kubernetes,
        PluginCacheEmitter,
    );

    let stack = Stack::new("plugin-chart".to_string())
        .add(
            PluginCache {
                id: "session-cache".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .build();

    let chart = generate_helm_chart(
        &stack,
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: "plugin-chart".to_string(),
        },
    )
    .expect("chart should render");

    let onprem = chart.files.get("examples/onprem.yaml").expect("onprem");
    assert!(
        onprem.contains("session-cache:"),
        "plugin emitter should add infrastructure entry: {onprem}"
    );
    assert!(
        onprem.contains("service: memcached"),
        "plugin emitter should set service: {onprem}"
    );
}
