//! What Alien knows about each resource type an extension declares: the descriptor the stack
//! carries, built from the type's `#[alien_resource]` and what `register` added for it.

use crate::resource::ResourceDeclarations;
use alien_core::{
    LinkDescriptor, PermissionSet, ResourceType, ResourceTypeDescriptor, ResourceTypeSchemas,
    SetupSupport,
};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The JSON Schema of `S`, from its `utoipa::ToSchema`. The schemas it refers to are under
/// `$defs`, so the schema stands alone. Its `title` is the Rust type's name, which
/// `alien extensions build` names the TypeScript builder after.
pub(crate) fn schema<S: utoipa::ToSchema>() -> Map<String, Value> {
    let mut referenced = Vec::new();
    S::schemas(&mut referenced);
    let mut schema = to_json(&S::schema());
    schema
        .entry("title")
        .or_insert_with(|| Value::String(S::name().into_owned()));
    let defs: Map<String, Value> = referenced
        .into_iter()
        .map(|(name, schema)| (name, Value::Object(to_json(&schema))))
        .collect();
    if !defs.is_empty() {
        schema.insert("$defs".into(), Value::Object(defs));
    }
    let mut schema = Value::Object(schema);
    point_refs_at_defs(&mut schema);
    match schema {
        Value::Object(schema) => schema,
        _ => Map::new(),
    }
}

fn to_json(schema: &impl serde::Serialize) -> Map<String, Value> {
    match serde_json::to_value(schema) {
        Ok(Value::Object(schema)) => schema,
        _ => Map::new(),
    }
}

/// utoipa writes OpenAPI's `#/components/schemas/<name>`; standalone, they live in `$defs`.
fn point_refs_at_defs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                if let Some(name) = reference.strip_prefix("#/components/schemas/") {
                    *reference = format!("#/$defs/{name}");
                }
            }
            object.values_mut().for_each(point_refs_at_defs);
        }
        Value::Array(items) => items.iter_mut().for_each(point_refs_at_defs),
        _ => {}
    }
}

/// A value of `T` built from its schema: empty strings, zeros, `false`, empty lists, `null` where
/// the schema allows it, and the first of an enum's values.
pub(crate) fn placeholder<T: DeserializeOwned + utoipa::ToSchema>() -> Result<T, String> {
    let schema = Value::Object(schema::<T>());
    let defs = schema.get("$defs").cloned().unwrap_or(Value::Null);
    let value = placeholder_of(&schema, &defs, 0);
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn placeholder_of(schema: &Value, defs: &Value, depth: usize) -> Value {
    // A type that contains itself has no finite placeholder; the cut leaves `null`.
    if depth > 32 {
        return Value::Null;
    }
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        return placeholder_of(&defs[name], defs, depth + 1);
    }
    for combined in ["oneOf", "anyOf", "allOf"] {
        if let Some(first) = schema.get(combined).and_then(|options| options.get(0)) {
            return placeholder_of(first, defs, depth + 1);
        }
    }
    if let Some(first) = schema.get("enum").and_then(|values| values.get(0)) {
        return first.clone();
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(name)) => vec![name.as_str()],
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if types.contains(&"null") {
        return Value::Null;
    }
    match types.first().copied() {
        Some("string") => Value::String(String::new()),
        Some("integer") | Some("number") => Value::from(0),
        Some("boolean") => Value::Bool(false),
        Some("array") => Value::Array(Vec::new()),
        Some("object") | None => Value::Object(
            schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| {
                    properties
                        .iter()
                        .map(|(name, property)| {
                            (name.clone(), placeholder_of(property, defs, depth + 1))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some(_) => Value::Null,
    }
}

/// Each declared type's descriptor, or every problem that keeps one from being built.
/// `controller_platforms` is left empty: the controller binary, not the hooks, knows them.
pub(crate) fn descriptors(
    extension: &crate::Extension,
) -> Result<IndexMap<ResourceType, ResourceTypeDescriptor>, Vec<String>> {
    let mut problems = Vec::new();
    let mut descriptors = IndexMap::new();
    for (resource_type, declarations) in &extension.declarations {
        let permission_sets: Vec<PermissionSet> = extension
            .permission_sets
            .iter()
            .filter(|(of, _)| of == resource_type)
            .filter_map(|(_, jsonc)| match json5::from_str::<PermissionSet>(jsonc) {
                Ok(set) => Some(set),
                Err(error) => {
                    problems.push(format!(
                        "a permission set of `{resource_type}` is not a permission set: {error}"
                    ));
                    None
                }
            })
            .collect();
        let prefix = format!("{resource_type}/");
        for set in &permission_sets {
            // The name becomes the set's file in the bundle, `permissions/<type>/<name>.json`.
            if !set
                .id
                .strip_prefix(&prefix)
                .is_some_and(alien_core::is_extension_name_segment)
            {
                problems.push(format!(
                    "the permission set `{}` of `{resource_type}` must be named `{prefix}<name>`, with a name of lowercase letters, digits and `-` that starts with a letter",
                    set.id
                ));
            }
        }
        let link_permission_sets = declarations
            .link_permission_sets
            .clone()
            .unwrap_or_default();
        for id in &link_permission_sets {
            if !permission_sets.iter().any(|set| &set.id == id) {
                problems.push(format!(
                    "`{resource_type}`'s link grants `{id}`, which is not one of the type's own permission sets"
                ));
            }
        }
        let mut descriptor = descriptor_of(declarations, permission_sets, link_permission_sets);
        for (flattens, why) in [
            (
                alien_core::binding_schema::flattens_alternatives
                    as fn(&serde_json::Map<String, serde_json::Value>) -> bool,
                alien_core::binding_schema::FLATTENED_OPTION,
            ),
            (
                alien_core::binding_schema::flattens_a_map_beside_fields,
                alien_core::binding_schema::FLATTENED_MAP,
            ),
        ] {
            if flattens(&descriptor.schemas.outputs) {
                problems.push(format!("`{resource_type}`'s outputs {why}"));
            }
        }
        extension.describe(resource_type, &mut descriptor);
        descriptors.insert(resource_type.clone(), descriptor);
    }
    if problems.is_empty() {
        Ok(descriptors)
    } else {
        Err(problems)
    }
}

fn descriptor_of(
    declarations: &ResourceDeclarations,
    permission_sets: Vec<PermissionSet>,
    link_permission_sets: Vec<String>,
) -> ResourceTypeDescriptor {
    ResourceTypeDescriptor {
        lifecycles: declarations.lifecycles.clone(),
        holds_data: declarations.holds_data,
        setup: SetupSupport {
            cloudformation: declarations.setup_cloudformation.clone(),
            terraform: declarations.setup_terraform.clone(),
            helm: declarations.setup_helm.clone(),
            importers: declarations.setup_importers.clone(),
        },
        controller_platforms: Vec::new(),
        permission_sets,
        link: LinkDescriptor {
            permission_sets: link_permission_sets,
        },
        schemas: ResourceTypeSchemas {
            config: declarations.config_schema.clone(),
            outputs: declarations.outputs_schema.clone(),
        },
        services: IndexMap::new(),
        requires_network: false,
    }
}
