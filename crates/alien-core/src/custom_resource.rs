//! Resources of a type an extension defines, kept as JSON.
//!
//! Alien deserializes `Resource`, `ResourceOutputs` and `ResourceHeartbeatData` by their type
//! tag. A tag of the form `<extension>/<type>` that Alien does not define loads as one of the
//! types here, so a stack, its state and its heartbeats can carry an extension's resource and
//! serialize it back unchanged.

use crate::error::ErrorData;
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use serde_json::{Map, Value};
use std::any::Any;

/// Type prefixes no extension may use: `alien/` is Alien's own namespace, and `experimental/`
/// holds built-in types that are not stable yet, such as `experimental/aws-opensearch`.
pub const RESERVED_TYPE_PREFIXES: &[&str] = &["alien", "experimental"];

/// Whether a type tag Alien does not define names an extension's type: `<extension>/<type>`,
/// both parts non-empty, under a prefix that is not reserved. Any other unknown tag stays an
/// error, so a typo in a built-in type name is refused instead of loading as custom.
pub(crate) fn is_custom_resource_type(tag: &str) -> bool {
    match tag.split_once('/') {
        Some((extension, name)) => {
            !extension.is_empty()
                && !name.is_empty()
                && !RESERVED_TYPE_PREFIXES.contains(&extension)
        }
        None => false,
    }
}

/// Configuration of a resource whose type an extension defines.
///
/// `config` holds every field but `type`, including `id`, so the resource serializes back
/// exactly as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomResource {
    resource_type: ResourceType,
    id: String,
    config: Map<String, Value>,
}

impl CustomResource {
    /// Builds one from the JSON object `Resource` read, with its `type` tag removed.
    pub fn from_untagged(resource_type: &str, value: Value) -> Result<Self, String> {
        let Value::Object(config) = value else {
            return Err(format!(
                "resource of type '{resource_type}' must be a JSON object"
            ));
        };
        let id = config
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("resource of type '{resource_type}' has no string 'id'"))?
            .to_string();
        Ok(Self {
            resource_type: ResourceType::from(resource_type),
            id,
            config,
        })
    }

    /// The resource's fields, without `type`.
    pub fn config(&self) -> &Map<String, Value> {
        &self.config
    }
}

impl ResourceDefinition for CustomResource {
    fn get_resource_type(&self) -> ResourceType {
        self.resource_type.clone()
    }

    fn id(&self) -> &str {
        &self.id
    }

    /// Alien cannot read references out of a config it does not know. A custom resource's
    /// dependencies travel in `ResourceEntry.dependencies` instead.
    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    /// The type and id can't change, as for Alien's own types; which other fields may is the
    /// controller's to say when it starts the update.
    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> crate::error::Result<()> {
        if new_config.get_resource_type() != self.resource_type {
            return Err(AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: self.resource_type.clone(),
                actual: new_config.get_resource_type(),
            }));
        }
        if new_config.id() != self.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }
        Ok(())
    }

    /// Alien cannot tell whether a custom resource holds data, so it never deletes a
    /// half-created one to create it again.
    fn replace_after_failed_create_is_safe(&self) -> bool {
        false
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
        other.as_any().downcast_ref::<CustomResource>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<Value> {
        Ok(Value::Object(self.config.clone()))
    }
}

/// Outputs of a resource whose type an extension defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomResourceOutputs {
    resource_type: ResourceType,
    fields: Map<String, Value>,
}

impl CustomResourceOutputs {
    pub(crate) fn new(resource_type: ResourceType, fields: Map<String, Value>) -> Self {
        Self {
            resource_type,
            fields,
        }
    }

    /// Builds one from the JSON object `ResourceOutputs` read, with its `type` tag removed.
    pub fn from_untagged(resource_type: &str, value: Value) -> Result<Self, String> {
        let Value::Object(fields) = value else {
            return Err(format!(
                "outputs of type '{resource_type}' must be a JSON object"
            ));
        };
        Ok(Self {
            resource_type: ResourceType::from(resource_type),
            fields,
        })
    }

    /// The output fields, without `type`.
    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }
}

impl ResourceOutputsDefinition for CustomResourceOutputs {
    fn get_resource_type(&self) -> ResourceType {
        self.resource_type.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<CustomResourceOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<Value> {
        Ok(Value::Object(self.fields.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Resource, ResourceOutputs, Stack, Worker, WorkerCode};
    use serde_json::json;

    /// A custom resource keeps its type and id across an update; other fields are the
    /// controller's to judge.
    #[test]
    fn an_update_keeps_the_type_and_id() {
        let zone = |resource_type: &str, id: &str, domain: &str| {
            CustomResource::from_untagged(
                resource_type,
                serde_json::json!({ "id": id, "domain": domain }),
            )
            .unwrap()
        };
        let current = zone("acme/dns-zone", "zone", "a.example");
        current
            .validate_update(&zone("acme/dns-zone", "zone", "b.example"))
            .unwrap();
        assert!(current
            .validate_update(&zone("acme/dns-zone", "other", "a.example"))
            .is_err());
        assert!(current
            .validate_update(&zone("acme/record", "zone", "a.example"))
            .is_err());
    }

    /// A stack with a worker that depends on an extension's DNS zone, as alien.ts writes it.
    fn stack_json() -> Value {
        let api = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "api:1".to_string(),
            })
            .permissions("api".to_string())
            .build();
        json!({
            "id": "acme",
            "resources": {
                "zone": {
                    "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
                    "lifecycle": "live",
                    "dependencies": []
                },
                "api": {
                    "config": serde_json::to_value(Resource::new(api)).unwrap(),
                    "lifecycle": "live",
                    "dependencies": [{ "type": "acme/dns-zone", "id": "zone" }]
                }
            }
        })
    }

    #[test]
    fn only_an_unreserved_extension_prefix_names_a_custom_type() {
        for (tag, custom) in [
            ("acme/dns-zone", true),
            ("worker", false),
            ("not-a-resource", false),
            ("alien/dns-zone", false),
            ("experimental/dns-zone", false),
            ("/dns-zone", false),
            ("acme/", false),
        ] {
            assert_eq!(is_custom_resource_type(tag), custom, "{tag}");
        }
    }

    /// The tags serde lists in its `unknown variant` refusal: every tag the type defines.
    fn listed_tags(refusal: impl std::fmt::Display) -> Vec<String> {
        let message = refusal.to_string();
        let tags: Vec<String> = message
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|tag| *tag != "not-a-resource")
            .map(str::to_string)
            .collect();
        assert!(tags.len() > 10, "could not read the tags out of: {message}");
        tags
    }

    // The heartbeat deserializer applies the custom rule before the built-in tags, and platform
    // tells a custom heartbeat apart by its slash; both hold only while no built-in tag is custom.
    #[test]
    fn no_built_in_tag_names_a_custom_type() {
        let resources = serde_json::from_value::<Resource>(json!({ "type": "not-a-resource" }))
            .expect_err("an unknown tag is refused");
        let outputs =
            serde_json::from_value::<ResourceOutputs>(json!({ "type": "not-a-resource" }))
                .expect_err("an unknown tag is refused");
        let heartbeats = serde_json::from_value::<crate::ResourceHeartbeatData>(json!({
            "resourceType": "not-a-resource",
            "data": {}
        }))
        .expect_err("an unknown tag is refused");

        for tag in listed_tags(resources)
            .into_iter()
            .chain(listed_tags(outputs))
            .chain(listed_tags(heartbeats))
        {
            assert!(!is_custom_resource_type(&tag), "{tag}");
        }
    }

    #[test]
    fn custom_resource_round_trips_through_stack_json() {
        let stack: Stack = serde_json::from_value(stack_json()).expect("the stack loads");
        let zone = &stack.resources["zone"].config;
        let custom = zone
            .downcast_ref::<CustomResource>()
            .expect("the extension's type is a CustomResource");
        assert_eq!(custom.id(), "zone");
        assert_eq!(zone.resource_type().as_ref(), "acme/dns-zone");
        assert_eq!(custom.config()["domain"], "internal.acme.dev");

        let again = serde_json::to_value(&stack).expect("the stack serializes");
        assert_eq!(
            again["resources"]["zone"]["config"],
            stack_json()["resources"]["zone"]["config"]
        );
        let reloaded: Stack = serde_json::from_value(again).expect("it loads again");
        assert_eq!(reloaded, stack);
    }

    #[test]
    fn built_in_types_still_load_as_themselves() {
        let stack: Stack = serde_json::from_value(stack_json()).expect("the stack loads");
        let api = &stack.resources["api"].config;
        assert!(api.downcast_ref::<Worker>().is_some());
        assert!(api.downcast_ref::<CustomResource>().is_none());
    }

    #[test]
    fn custom_resource_without_id_is_refused() {
        let err =
            serde_json::from_value::<Resource>(json!({ "type": "acme/dns-zone", "domain": "x" }))
                .expect_err("no id");
        assert!(err.to_string().contains("no string 'id'"), "{err}");
    }

    #[test]
    fn custom_resource_is_never_replaced_after_a_failed_create() {
        let resource: Resource =
            serde_json::from_value(json!({ "type": "acme/dns-zone", "id": "zone" })).unwrap();
        assert!(!resource.replace_after_failed_create_is_safe());
    }

    #[test]
    fn custom_outputs_round_trip() {
        let tagged = json!({ "type": "acme/dns-zone", "zoneId": "Z123" });
        let outputs: ResourceOutputs =
            serde_json::from_value(tagged.clone()).expect("outputs load");
        let custom = outputs
            .downcast_ref::<CustomResourceOutputs>()
            .expect("CustomResourceOutputs");
        assert_eq!(custom.fields()["zoneId"], "Z123");
        assert_eq!(serde_json::to_value(&outputs).unwrap(), tagged);
    }

    #[test]
    fn unknown_type_outside_an_extension_namespace_is_refused() {
        for tag in ["not-a-resource", "alien/dns-zone", "experimental/dns-zone"] {
            let resource = serde_json::from_value::<Resource>(json!({ "type": tag, "id": "x" }))
                .expect_err(tag);
            assert!(
                resource.to_string().contains("unknown variant"),
                "{resource}"
            );
            let outputs =
                serde_json::from_value::<ResourceOutputs>(json!({ "type": tag })).expect_err(tag);
            assert!(outputs.to_string().contains("unknown variant"), "{outputs}");
        }
    }

    /// A release records no minimum Alien version for its extensions: a binary that has none
    /// of the stack's extensions loaded still reads and writes back a state with their
    /// resources unchanged, since a custom resource's config, controller state and outputs are
    /// kept as JSON.
    #[test]
    fn a_stack_state_with_a_custom_resource_round_trips_without_its_extension() {
        let state = json!({
            "platform": "aws",
            "resourcePrefix": "k1234567",
            "resources": {
                "zone": {
                    "type": "acme/dns-zone",
                    "_internal": {
                        "type": "acme/dns-zone",
                        "_controllerStateVersion": 1,
                        "state": "Ready",
                        "zoneId": "Z123",
                        "nameServers": ["ns-1.example.net"]
                    },
                    "status": "running",
                    "outputs": { "type": "acme/dns-zone", "zoneId": "Z123" },
                    "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
                    "previousConfig": { "type": "acme/dns-zone", "id": "zone", "domain": "old.acme.dev" },
                    "lifecycle": "live",
                    "lastFailedState": { "type": "acme/dns-zone", "state": "Creating" }
                }
            }
        });
        let loaded: crate::StackState =
            serde_json::from_value(state.clone()).expect("the state loads with no extension");
        assert_eq!(serde_json::to_value(&loaded).unwrap(), state);
    }

    #[test]
    fn built_in_type_with_a_malformed_body_is_refused() {
        serde_json::from_value::<Resource>(json!({ "type": "worker", "id": "api" }))
            .expect_err("a worker needs its code");
        serde_json::from_value::<ResourceOutputs>(json!({ "type": "storage" }))
            .expect_err("storage outputs need their fields");
    }
}
