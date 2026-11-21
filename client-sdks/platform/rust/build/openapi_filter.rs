use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value};

// The full client merges more repeated schemas than the default one: it also merges schemas
// that differ only in documentation text, and shares strings with a pattern. The default client
// keeps its generated type names, which handwritten code imports. Set once per build, before
// any schema is read.
static MERGE_REPEATED_SCHEMAS: AtomicBool = AtomicBool::new(false);

const HTTP_METHODS: &[&str] = &[
    "delete", "get", "head", "options", "patch", "post", "put", "trace",
];

// Handwritten Alien consumers import the path-derived Progenitor types below.
// Keep these anonymous schema families inline so deduplication does not rename
// their public Rust types. Missing pointers fail generation instead of silently
// changing that compatibility boundary.
const CONSUMER_NAMED_ANONYMOUS_SCHEMA_POINTERS: &[&str] = &[
    "/components/schemas/ConfigureModelsRequest/properties/requirements/items",
    "/paths/~1v1~1projects/post/requestBody/content/application~1json/schema/properties/gitRepository",
];

// The server adds package types and resource statuses without a client release. These
// response fields decode as plain strings instead of closed enums. When the
// component is present, a moved field or a missing enum fails generation.
const OPEN_STRING_ENUM_SCHEMAS: &[(&str, &str)] = &[
    ("DeploymentResource", "/properties/status"),
    (
        "CapabilityMaterialization",
        "/properties/packages/items/properties/type",
    ),
    (
        "DeploymentLinkSetupResponse",
        "/properties/visiblePackageTypes/items",
    ),
];

/// Platform operations used by Alien's production Rust consumers.
///
/// Keep this list explicit: adding a Platform API call should require a review
/// of the compiler input it brings into the always-enabled CLI graph.
pub const REQUIRED_OPERATION_IDS: &[&str] = &[
    "approveAccessRequest",
    "cancelDeploymentVolumeRestore",
    "cancelMachinesMachineDrain",
    "configureProjectBuckets",
    "configureProjectDeployments",
    "configureProjectKeys",
    "configureProjectModels",
    "configureProjectRegistry",
    "configureProjectRemoteSandbox",
    "createAPIKey",
    "createAccessRequest",
    "createDeployment",
    "planDeploymentCompute",
    "prepareDeploymentStack",
    "createDeploymentGroup",
    "createDeploymentGroupToken",
    "createDeploymentToken",
    "createDeploymentVolumeRestore",
    "createMachinesJoinToken",
    "createProject",
    "createRelease",
    "createReleaseChannel",
    "createRemoteBindingsExternalAccess",
    "createSetupLink",
    "createWorkspace",
    "deleteDeployment",
    "deleteDeploymentGroup",
    "deleteManager",
    "deleteProject",
    "deleteReleaseChannel",
    "denyAccessRequest",
    "drainMachinesMachine",
    "ensureDeploymentGroupByExternalId",
    "ensureProjectSandboxBaseImageRepository",
    "generateManagerBindingToken",
    "generateManagerToken",
    "getAPIKey",
    "getAccessRequest",
    "getAccessRequestCoordinates",
    "getCommand",
    "getDeployment",
    "getDeploymentGroup",
    "getDeploymentGroupByExternalId",
    "getDeploymentInfo",
    "getDeploymentInputs",
    "getDeploymentUpdateOperation",
    "getManager",
    "getProject",
    // Portal reads add endpoint status/DNS records and redacted OAuth settings.
    // listDomains shares those endpoint types and adds domain ownership/usage.
    "getProjectDeploymentPortalDomain",
    "getProjectGcpOAuthProvider",
    "getProjectCapabilityOverview",
    "getProjectDeploymentLinkSetup",
    "getRelease",
    "getResourceDeploymentDetail",
    "invokeOperation",
    "listAPIKeys",
    "listDeploymentGroups",
    "listDeployments",
    "listDomains",
    "createDomain",
    "getDomain",
    "refreshDomain",
    "deleteDomain",
    "updateDeploymentDomains",
    "listEvents",
    "listDeploymentMachines",
    "listDeploymentVolumeRestores",
    "listMachinesJoinTokens",
    "listManagerEvents",
    "listManagers",
    "listMemberships",
    "listProjects",
    "listReleaseChannels",
    "listReleases",
    "moveDeployment",
    "pinDeploymentRelease",
    "promoteRelease",
    "redeployDeployment",
    "removeMachinesMachine",
    "requestDeploymentCheck",
    "resolve",
    "retryDeployment",
    "revokeAPIKey",
    "revokeAccessRequest",
    "revokeMachinesJoinToken",
    "rotateMachinesJoinToken",
    "setDeploymentGroupExternalId",
    "setDeploymentReleaseChannel",
    "updateAPIKey",
    "updateDeploymentCompute",
    "updateDeploymentEnvironmentVariables",
    "updateDeploymentGroup",
    "updateDeploymentInputs",
    // Only the OAuth mode/client credentials request is added for portal writes;
    // appearance uses JSON to preserve unknown server fields.
    "updateProjectGcpOAuthProvider",
    "verifyOperationCheck",
    "whoami",
];

pub fn filter_openapi(document: &Value, required_operation_ids: &[&str]) -> Result<Value, String> {
    let root = document
        .as_object()
        .ok_or_else(|| "OpenAPI document must be a JSON object".to_string())?;
    let source_paths = root
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenAPI document is missing an object-valued `paths` field".to_string())?;

    let required: BTreeSet<&str> = required_operation_ids.iter().copied().collect();
    if required.len() != required_operation_ids.len() {
        return Err("required operation allowlist contains duplicate IDs".to_string());
    }

    let mut found = BTreeMap::<String, String>::new();
    let mut paths = Map::new();

    for (path, path_item) in source_paths {
        let path_object = path_item
            .as_object()
            .ok_or_else(|| format!("path item `{path}` must be a JSON object"))?;
        let mut retained = Map::new();
        let mut retained_operation = false;

        for (key, value) in path_object {
            if HTTP_METHODS.contains(&key.as_str()) {
                let operation_id = value
                    .get("operationId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{key} {path} is missing `operationId`"))?;
                if required.contains(operation_id) {
                    if let Some(previous) =
                        found.insert(operation_id.to_string(), format!("{key} {path}"))
                    {
                        return Err(format!(
                            "operationId `{operation_id}` is duplicated at {previous} and {key} {path}"
                        ));
                    }
                    retained.insert(key.clone(), value.clone());
                    retained_operation = true;
                }
            } else {
                retained.insert(key.clone(), value.clone());
            }
        }

        if retained_operation {
            paths.insert(path.clone(), Value::Object(retained));
        }
    }

    let missing: Vec<_> = required
        .iter()
        .filter(|operation_id| !found.contains_key(**operation_id))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "required operation IDs are missing from the OpenAPI document: {}",
            missing.join(", ")
        ));
    }

    let mut filtered = root.clone();
    filtered.insert("paths".to_string(), Value::Object(paths));
    filtered.remove("webhooks");
    filtered.remove("components");
    filtered.insert(
        "components".to_string(),
        reachable_components(document, &filtered)?,
    );
    preserve_empty_array_presence(&mut filtered)?;
    normalize_binding_unions(&mut filtered)?;
    open_string_enums(&mut filtered)?;
    deduplicate_anonymous_schemas(
        &mut filtered,
        required_operation_ids == REQUIRED_OPERATION_IDS,
    )?;
    canonicalize_nullable_enums(&mut filtered);
    normalize_optional_response_bodies(&mut filtered);
    allow_unknown_properties(&mut filtered);

    Ok(Value::Object(filtered))
}

fn deduplicate_anonymous_schemas(
    document: &mut Map<String, Value>,
    protect_consumer_names: bool,
) -> Result<BTreeMap<String, String>, String> {
    let mut occurrences = BTreeMap::<String, (usize, Value)>::new();
    let document_value = Value::Object(document.clone());
    let mut protected_shapes = BTreeSet::new();
    if protect_consumer_names {
        for pointer in CONSUMER_NAMED_ANONYMOUS_SCHEMA_POINTERS {
            let schema = document_value.pointer(pointer).ok_or_else(|| {
                format!("consumer-named anonymous schema is missing at `{pointer}`")
            })?;
            collect_shareable_schema_keys(schema, &mut protected_shapes)?;
        }
    }
    let schemas = document
        .get("components")
        .and_then(|components| components.get("schemas"))
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenAPI document is missing object-valued component schemas".to_string())?;
    if let Some(paths) = document.get("paths") {
        collect_openapi_schema_occurrences(paths, &mut occurrences);
    }
    for schema in schemas.values() {
        collect_schema_occurrences(schema, &mut occurrences, false);
    }
    let existing_names = schemas.keys().cloned().collect::<BTreeSet<_>>();
    let mut existing_shapes = BTreeMap::<String, Option<String>>::new();
    for (name, schema) in schemas {
        let key = canonical_schema_key(schema)?;
        existing_shapes
            .entry(key)
            .and_modify(|name| *name = None)
            .or_insert_with(|| Some(name.clone()));
    }

    let mut replacements = BTreeMap::<String, String>::new();
    let mut extracted = Vec::<(String, String, Value)>::new();
    let mut generated_names = BTreeMap::<String, String>::new();
    for (key, (count, schema)) in occurrences {
        if count < 2 || protected_shapes.contains(&key) {
            continue;
        }
        if let Some(Some(name)) = existing_shapes.get(&key) {
            replacements.insert(key, name.clone());
            continue;
        }

        let name = format!(
            "AlienSharedObject{:016x}",
            stable_schema_hash(key.as_bytes())
        );
        if existing_names.contains(&name) {
            return Err(format!(
                "generated component name `{name}` collides with an existing schema"
            ));
        }
        if let Some(previous_key) = generated_names.insert(name.clone(), key.clone()) {
            if previous_key != key {
                return Err(format!("generated component hash collision for `{name}`"));
            }
        }
        replacements.insert(key.clone(), name.clone());
        extracted.push((key, name, schema));
    }

    if let Some(paths) = document.get_mut("paths") {
        replace_openapi_schema_occurrences(paths, &replacements)?;
    }
    let schemas = document
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(Value::as_object_mut)
        .expect("component schemas were validated above");
    for schema in schemas.values_mut() {
        replace_schema_occurrences(schema, &replacements, false)?;
    }
    for (_, name, mut schema) in extracted {
        replace_schema_occurrences(&mut schema, &replacements, false)?;
        schemas.insert(name, schema);
    }

    Ok(replacements)
}

fn is_shareable_anonymous_schema(schema: &Value) -> bool {
    if schema.get("$ref").is_some() {
        return false;
    }
    match schema.get("type").and_then(Value::as_str) {
        // Inline, a nullable object is `Option<Type>` and an object without named properties is
        // a map, but shared they become a wrapper type around `Option<Type>` or the map, which
        // changes the field types of every use. The default client keeps sharing them, as it
        // always has.
        Some("object") => {
            !MERGE_REPEATED_SCHEMAS.load(Ordering::Relaxed)
                || (schema.get("nullable").is_none()
                    && schema
                        .get("properties")
                        .and_then(Value::as_object)
                        .is_some_and(|properties| !properties.is_empty()))
        }
        // A string with a pattern becomes its own validated Rust type at every use. Enums
        // are left alone: they are nullable in places, and merging them changes the argument
        // types of the generated builders.
        Some("string") => {
            MERGE_REPEATED_SCHEMAS.load(Ordering::Relaxed)
                && schema.get("pattern").is_some()
                && schema.get("enum").is_none()
                && schema.get("nullable").is_none()
        }
        _ => false,
    }
}

fn collect_shareable_schema_keys(value: &Value, keys: &mut BTreeSet<String>) -> Result<(), String> {
    if is_shareable_anonymous_schema(value) {
        keys.insert(canonical_schema_key(value)?);
    }
    for_schema_children(value, |child| collect_shareable_schema_keys(child, keys))?;
    Ok(())
}

fn collect_openapi_schema_occurrences(
    value: &Value,
    occurrences: &mut BTreeMap<String, (usize, Value)>,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_openapi_schema_occurrences(value, occurrences);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key == "schema" {
                    collect_schema_occurrences(value, occurrences, true);
                } else if matches!(key.as_str(), "example" | "examples" | "default" | "enum")
                    || key.starts_with("x-")
                {
                    // These fields contain literal contract or extension data. A payload may
                    // itself contain a key named `schema`; it is not an OpenAPI Schema Object.
                    continue;
                } else {
                    collect_openapi_schema_occurrences(value, occurrences);
                }
            }
        }
        _ => {}
    }
}

fn collect_schema_occurrences(
    value: &Value,
    occurrences: &mut BTreeMap<String, (usize, Value)>,
    collect_current: bool,
) {
    if collect_current && is_shareable_anonymous_schema(value) {
        let schema = value.clone();
        let key = canonical_schema_key(&schema).expect("JSON schema serializes");
        occurrences
            .entry(key)
            .and_modify(|(count, _)| *count += 1)
            .or_insert((1, schema));
    }
    for_schema_children_in_position(value, |child, in_union| {
        collect_schema_occurrences(child, occurrences, may_share_in_position(in_union));
        Ok::<(), ()>(())
    })
    .expect("schema occurrence collection is infallible");
}

fn stable_schema_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn replace_openapi_schema_occurrences(
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
) -> Result<(), String> {
    match value {
        Value::Array(values) => {
            for value in values {
                replace_openapi_schema_occurrences(value, replacements)?;
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key == "schema" {
                    replace_schema_occurrences(value, replacements, true)?;
                } else if matches!(key.as_str(), "example" | "examples" | "default" | "enum")
                    || key.starts_with("x-")
                {
                    continue;
                } else {
                    replace_openapi_schema_occurrences(value, replacements)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn replace_schema_occurrences(
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
    replace_current: bool,
) -> Result<(), String> {
    if replace_current && is_shareable_anonymous_schema(value) {
        let key = canonical_schema_key(value)?;
        if let Some(name) = replacements.get(&key) {
            *value = serde_json::json!({
                "$ref": format!("#/components/schemas/{name}")
            });
            return Ok(());
        }
    }

    for_schema_children_in_position_mut(value, |child, in_union| {
        replace_schema_occurrences(child, replacements, may_share_in_position(in_union))
    })?;
    Ok(())
}

fn for_schema_children<E>(
    schema: &Value,
    mut visit: impl FnMut(&Value) -> Result<(), E>,
) -> Result<(), E> {
    for_schema_children_in_position(schema, |child, _| visit(child))
}

fn for_schema_children_mut<E>(
    schema: &mut Value,
    mut visit: impl FnMut(&mut Value) -> Result<(), E>,
) -> Result<(), E> {
    for_schema_children_in_position_mut(schema, |child, _| visit(child))
}

// A variant of `oneOf`/`anyOf`/`allOf` that becomes a reference changes the generated parent
// from an enum with inline variants to a struct of flattened optional parts, so the full client
// keeps variants inline (their own children are still shared). The default client keeps sharing
// them, as it always has.
fn may_share_in_position(in_union: bool) -> bool {
    !(in_union && MERGE_REPEATED_SCHEMAS.load(Ordering::Relaxed))
}

/// Visits every child schema with whether it is a variant of `allOf`, `anyOf` or `oneOf`.
fn for_schema_children_in_position<E>(
    schema: &Value,
    mut visit: impl FnMut(&Value, bool) -> Result<(), E>,
) -> Result<(), E> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    for key in ["properties", "patternProperties", "dependentSchemas"] {
        if let Some(children) = object.get(key).and_then(Value::as_object) {
            for child in children.values() {
                visit(child, false)?;
            }
        }
    }
    for key in SINGLE_CHILD_KEYS {
        if let Some(child) = object.get(*key).filter(|value| value.is_object()) {
            visit(child, false)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get(key).and_then(Value::as_array) {
            for child in children {
                visit(child, key != "prefixItems")?;
            }
        }
    }
    Ok(())
}

fn for_schema_children_in_position_mut<E>(
    schema: &mut Value,
    mut visit: impl FnMut(&mut Value, bool) -> Result<(), E>,
) -> Result<(), E> {
    let Some(object) = schema.as_object_mut() else {
        return Ok(());
    };
    for key in ["properties", "patternProperties", "dependentSchemas"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                visit(child, false)?;
            }
        }
    }
    for key in SINGLE_CHILD_KEYS {
        if let Some(child) = object.get_mut(*key).filter(|value| value.is_object()) {
            visit(child, false)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_array_mut) {
            for child in children {
                visit(child, key != "prefixItems")?;
            }
        }
    }
    Ok(())
}

const SINGLE_CHILD_KEYS: &[&str] = &[
    "items",
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "contains",
    "not",
    "if",
    "then",
    "else",
];

// Two schemas with the same structure but different documentation text are one Rust type.
// The API spec repeats shared object schemas inline with a per-use description, which
// otherwise generates one copy of the type, its builder and its conversions per use.
fn canonical_schema_key(schema: &Value) -> Result<String, String> {
    let result = if MERGE_REPEATED_SCHEMAS.load(Ordering::Relaxed) {
        serde_json::to_string(&without_documentation(schema))
    } else {
        serde_json::to_string(schema)
    };
    result.map_err(|error| format!("failed to serialize schema: {error}"))
}

fn without_documentation(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !matches!(key.as_str(), "description" | "title" | "example"))
                .map(|(key, value)| {
                    // `properties` maps field names to schemas: a field called
                    // `description` is a field, not documentation.
                    let value = if key == "properties" {
                        match value {
                            Value::Object(fields) => Value::Object(
                                fields
                                    .iter()
                                    .map(|(name, field)| {
                                        (name.clone(), without_documentation(field))
                                    })
                                    .collect(),
                            ),
                            other => other.clone(),
                        }
                    } else {
                        without_documentation(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_documentation).collect()),
        other => other.clone(),
    }
}

/// Precedes a schema position (`<component>#<path>`) at the end of a description in the two
/// throwaway copies `merge_repeated_schemas_with_positions` returns. The build script reads it
/// back from the generated docs to learn which merged type each removed type became.
pub const POSITION_MARKER: &str = "alien-position:";

/// Full client only: shares repeated schemas so the generated client stays small enough to
/// compile in under 8 GB. Run after `normalize_openapi`. Returns the merged document and two
/// copies the build script uses to keep the type names merging removes:
/// - `document` where every schema inside a replaced schema ends its description with the
///   position it has in the merged document (references followed);
/// - the merged document where every schema of a component ends its description with its
///   own position.
#[allow(dead_code)] // the tests include this file but only the build script merges
pub fn merge_repeated_schemas_with_positions(
    document: &Value,
) -> Result<(Value, Value, Value), String> {
    let mut root = document
        .as_object()
        .ok_or_else(|| "OpenAPI document must be a JSON object".to_string())?
        .clone();
    MERGE_REPEATED_SCHEMAS.store(true, Ordering::Relaxed);
    let replacements = deduplicate_anonymous_schemas(&mut root, false)?;
    let merged = Value::Object(root);
    let merged_schemas = merged
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .ok_or_else(|| "merged document has no component schemas".to_string())?;

    let mut unmerged = document.clone();
    let marker = SourceMarker {
        replacements: &replacements,
        merged_schemas,
    };
    if let Some(paths) = unmerged.get_mut("paths") {
        marker.mark_openapi(paths)?;
    }
    if let Some(schemas) = unmerged
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        for schema in schemas.values_mut() {
            marker.mark_occurrences(schema, false)?;
        }
    }

    let mut positioned = merged.clone();
    if let Some(schemas) = positioned
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        for (name, schema) in schemas.iter_mut() {
            mark_positions(schema, name, &mut Vec::new());
        }
    }
    Ok((merged, unmerged, positioned))
}

struct SourceMarker<'a> {
    replacements: &'a BTreeMap<String, String>,
    merged_schemas: &'a Map<String, Value>,
}

impl SourceMarker<'_> {
    // Walks the same positions as `replace_openapi_schema_occurrences`.
    fn mark_openapi(&self, value: &mut Value) -> Result<(), String> {
        match value {
            Value::Array(values) => {
                for value in values {
                    self.mark_openapi(value)?;
                }
            }
            Value::Object(object) => {
                for (key, value) in object {
                    if key == "schema" {
                        self.mark_occurrences(value, true)?;
                    } else if matches!(key.as_str(), "example" | "examples" | "default" | "enum")
                        || key.starts_with("x-")
                    {
                        continue;
                    } else {
                        self.mark_openapi(value)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // Walks the same positions as `replace_schema_occurrences`. A replaced schema and all it
    // contains get the position they have under the component that replaces it.
    fn mark_occurrences(&self, value: &mut Value, replace_current: bool) -> Result<(), String> {
        if replace_current && is_shareable_anonymous_schema(value) {
            let key = canonical_schema_key(value)?;
            if let Some(name) = self.replacements.get(&key) {
                return self.mark_replaced(value, name, &mut Vec::new());
            }
        }
        for_schema_children_in_position_mut(value, |child, in_union| {
            self.mark_occurrences(child, may_share_in_position(in_union))
        })
    }

    fn mark_replaced(
        &self,
        value: &mut Value,
        component: &str,
        path: &mut Vec<String>,
    ) -> Result<(), String> {
        if !is_positioned_schema(value) {
            return Ok(());
        }
        let (owner, owner_path) = resolve_position(self.merged_schemas, component, path)
            .ok_or_else(|| {
                format!(
                    "position `{}` of a replaced schema is missing from the merged document",
                    position_text(component, path)
                )
            })?;
        append_position(value, &position_text(&owner, &owner_path));
        for_schema_children_with_path_mut(value, |segments, child| {
            path.extend(segments.iter().cloned());
            let result = self.mark_replaced(child, component, path);
            path.truncate(path.len() - segments.len());
            result
        })
    }
}

// Every schema of a merged component, by its own position; references are not followed.
fn mark_positions(value: &mut Value, component: &str, path: &mut Vec<String>) {
    if !is_positioned_schema(value) {
        return;
    }
    append_position(value, &position_text(component, path));
    for_schema_children_with_path_mut(value, |segments, child| {
        path.extend(segments.iter().cloned());
        mark_positions(child, component, path);
        path.truncate(path.len() - segments.len());
        Ok::<(), ()>(())
    })
    .expect("marking positions is infallible");
}

// A reference keeps no description (its siblings are ignored and could change the generated
// type), and only objects are schemas.
fn is_positioned_schema(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| !object.contains_key("$ref"))
}

/// The component and path a position ends at in the merged document, following references.
fn resolve_position(
    schemas: &Map<String, Value>,
    component: &str,
    path: &[String],
) -> Option<(String, Vec<String>)> {
    let mut owner = component.to_string();
    let mut owner_path = Vec::new();
    let mut node = follow_references(schemas, schemas.get(&owner)?, &mut owner, &mut owner_path)?;
    for segment in path {
        node = match node {
            Value::Object(object) => object.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
        owner_path.push(segment.clone());
        // Also runs on a `properties` map, which is harmless: no property is called `$ref`.
        node = follow_references(schemas, node, &mut owner, &mut owner_path)?;
    }
    Some((owner, owner_path))
}

fn follow_references<'a>(
    schemas: &'a Map<String, Value>,
    mut node: &'a Value,
    owner: &mut String,
    owner_path: &mut Vec<String>,
) -> Option<&'a Value> {
    while let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        let name = reference.strip_prefix("#/components/schemas/")?;
        *owner = name.to_string();
        owner_path.clear();
        node = schemas.get(name)?;
    }
    Some(node)
}

fn position_text(component: &str, path: &[String]) -> String {
    let path: Vec<String> = path
        .iter()
        .map(|segment| segment.replace('~', "~0").replace('/', "~1"))
        .collect();
    format!("{component}#/{}", path.join("/"))
}

fn append_position(value: &mut Value, position: &str) {
    let object = value
        .as_object_mut()
        .expect("positioned schemas are objects");
    let description = match object.get("description").and_then(Value::as_str) {
        Some(text) => format!("{text}\n\n{POSITION_MARKER}{position}"),
        None => format!("{POSITION_MARKER}{position}"),
    };
    object.insert("description".to_string(), Value::String(description));
}

/// Visits every child schema with the path segments that lead to it from its parent.
fn for_schema_children_with_path_mut<E>(
    schema: &mut Value,
    mut visit: impl FnMut(&[String], &mut Value) -> Result<(), E>,
) -> Result<(), E> {
    let Some(object) = schema.as_object_mut() else {
        return Ok(());
    };
    for key in ["properties", "patternProperties", "dependentSchemas"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_object_mut) {
            for (name, child) in children.iter_mut() {
                visit(&[key.to_string(), name.clone()], child)?;
            }
        }
    }
    for key in SINGLE_CHILD_KEYS {
        if let Some(child) = object.get_mut(*key).filter(|value| value.is_object()) {
            visit(&[key.to_string()], child)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_array_mut) {
            for (index, child) in children.iter_mut().enumerate() {
                visit(&[key.to_string(), index.to_string()], child)?;
            }
        }
    }
    Ok(())
}

pub fn normalize_openapi(document: &Value) -> Result<Value, String> {
    let mut root = document
        .as_object()
        .ok_or_else(|| "OpenAPI document must be a JSON object".to_string())?
        .clone();
    preserve_empty_array_presence(&mut root)?;
    normalize_binding_unions(&mut root)?;
    open_string_enums(&mut root)?;
    canonicalize_nullable_enums(&mut root);
    normalize_optional_response_bodies(&mut root);
    allow_unknown_properties(&mut root);
    Ok(Value::Object(root))
}

// Progenitor requires one success type per operation. For an operation that can
// return JSON or no body, expose the HTTP response so callers can inspect its
// status before decoding. Keep every status and the original wire body intact.
fn normalize_optional_response_bodies(document: &mut Map<String, Value>) {
    let Some(paths) = document.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };
    for path in paths.values_mut().filter_map(Value::as_object_mut) {
        for method in HTTP_METHODS {
            let Some(responses) = path
                .get_mut(*method)
                .and_then(|operation| operation.get_mut("responses"))
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            let successes: Vec<_> = responses
                .iter()
                .filter(|(status, _)| status.starts_with('2'))
                .map(|(_, response)| response)
                .collect();
            let has_json = successes.iter().any(|response| {
                response
                    .get("content")
                    .and_then(Value::as_object)
                    .is_some_and(|content| {
                        content.keys().any(|kind| {
                            kind == "application/json" || kind.starts_with("application/json;")
                        })
                    })
            });
            let has_empty = successes.iter().any(|response| {
                response.get("$ref").is_none()
                    && response
                        .get("content")
                        .is_none_or(|content| content.as_object().is_some_and(Map::is_empty))
            });
            if has_json && has_empty {
                for (status, response) in responses {
                    if status.starts_with('2') {
                        response["content"] = serde_json::json!({"application/octet-stream": {}});
                    }
                }
            }
        }
    }
}

/// Optional string arrays where an empty list means something different from an
/// absent field. Typify gives such a field an empty-array default and leaves it
/// out when empty; a native Vec yields `Option<Vec<String>>` instead, so `[]`
/// reaches the wire, without changing the canonical API schema.
const PRESENCE_PRESERVING_STRING_ARRAYS: &[(&str, &str)] = &[
    // Stored secret IDs: empty means "none stored", absent means "unknown".
    ("DeploymentConfig", "storedSecretInputIds"),
    // Quota warning overrides: empty clears them, absent keeps them.
    ("UpdateDeploymentComputeRequest", "quotaWarningOverrides"),
];

fn preserve_empty_array_presence(document: &mut Map<String, Value>) -> Result<(), String> {
    for (schema_name, field_name) in PRESENCE_PRESERVING_STRING_ARRAYS {
        preserve_array_presence(document, schema_name, field_name)?;
    }
    Ok(())
}

fn preserve_array_presence(
    document: &mut Map<String, Value>,
    schema_name: &str,
    field_name: &str,
) -> Result<(), String> {
    let Some(schema) = document
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(|schemas| schemas.get_mut(schema_name))
    else {
        return Ok(());
    };
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|fields| fields.iter().any(|field| field == field_name));
    let Some(field) = schema.pointer_mut(&format!("/properties/{field_name}")) else {
        return Ok(());
    };
    let valid = !required
        && field.get("type").and_then(Value::as_str) == Some("array")
        && field.get("items") == Some(&serde_json::json!({"type": "string"}))
        && field
            .get("nullable")
            .is_none_or(|nullable| nullable == &Value::Bool(false))
        && field.as_object().is_some_and(|field| {
            field
                .keys()
                .all(|key| matches!(key.as_str(), "type" | "items" | "description" | "nullable"))
        });
    if !valid {
        return Err(format!(
            "{schema_name}.{field_name} must remain an optional unconstrained nonnullable string array"
        ));
    }
    field.as_object_mut().unwrap().insert("x-rust-type".to_string(), serde_json::json!({
        "crate": "std", "version": "*", "path": "std::vec::Vec", "parameters": [{"type": "string"}]
    }));
    Ok(())
}

fn open_string_enums(document: &mut Map<String, Value>) -> Result<(), String> {
    for (component, pointer) in OPEN_STRING_ENUM_SCHEMAS {
        let Some(schema) = document
            .get_mut("components")
            .and_then(|components| components.get_mut("schemas"))
            .and_then(|schemas| schemas.get_mut(*component))
        else {
            continue;
        };
        let opened = schema
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .and_then(|schema| schema.remove("enum"));
        if opened.is_none() {
            return Err(format!(
                "open string schema `{component}{pointer}` is missing or has no enum"
            ));
        }
    }
    Ok(())
}

// typify turns `additionalProperties: false` into `deny_unknown_fields`, which fails a whole
// response when the server adds an optional field. Serialization is unaffected, so request
// bodies stay exactly as built; the server still validates them.
fn allow_unknown_properties(document: &mut Map<String, Value>) {
    if let Some(paths) = document.get_mut("paths") {
        allow_unknown_properties_in_openapi(paths);
    }
    if let Some(components) = document
        .get_mut("components")
        .and_then(Value::as_object_mut)
    {
        for (section, value) in components {
            match (section.as_str(), value.as_object_mut()) {
                ("schemas", Some(schemas)) => {
                    for schema in schemas.values_mut() {
                        allow_unknown_properties_in_schema(schema);
                    }
                }
                _ => allow_unknown_properties_in_openapi(value),
            }
        }
    }
}

fn allow_unknown_properties_in_openapi(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                allow_unknown_properties_in_openapi(value);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key == "schema" {
                    allow_unknown_properties_in_schema(value);
                } else if matches!(key.as_str(), "example" | "examples" | "enum")
                    || key.starts_with("x-")
                {
                    continue;
                } else {
                    allow_unknown_properties_in_openapi(value);
                }
            }
        }
        _ => {}
    }
}

fn allow_unknown_properties_in_schema(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        if object.get("additionalProperties") == Some(&Value::Bool(false)) {
            object.remove("additionalProperties");
        }
    }
    for_schema_children_mut(schema, |child| {
        allow_unknown_properties_in_schema(child);
        Ok::<(), ()>(())
    })
    .expect("opening object schemas is infallible");
}

fn canonicalize_nullable_enums(document: &mut Map<String, Value>) {
    let Some(schemas) = document
        .get("components")
        .and_then(|components| components.get("schemas"))
        .and_then(Value::as_object)
    else {
        return;
    };

    let mut enum_components = BTreeMap::<String, Option<String>>::new();
    for (name, schema) in schemas {
        let Some(values) = string_enum_values(schema, false) else {
            continue;
        };
        let key = serde_json::to_string(&values).expect("string enum values serialize");
        enum_components
            .entry(key)
            .and_modify(|name| *name = None)
            .or_insert_with(|| Some(name.clone()));
    }

    if let Some(paths) = document.get_mut("paths") {
        replace_nullable_enum_duplicates(paths, &enum_components, true);
    }
    if let Some(schemas) = document
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(Value::as_object_mut)
    {
        for schema in schemas.values_mut() {
            replace_nullable_enum_duplicates(schema, &enum_components, false);
        }
    }
}

fn replace_nullable_enum_duplicates(
    value: &mut Value,
    enum_components: &BTreeMap<String, Option<String>>,
    replace_current: bool,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                replace_nullable_enum_duplicates(value, enum_components, true);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                replace_nullable_enum_duplicates(value, enum_components, true);
            }

            if !replace_current || object.contains_key("$ref") {
                return;
            }
            let Some(values) = string_enum_values(&Value::Object(object.clone()), true) else {
                return;
            };
            let key = serde_json::to_string(&values).expect("string enum values serialize");
            let Some(Some(component_name)) = enum_components.get(&key) else {
                return;
            };

            object.remove("type");
            object.remove("enum");
            object.insert(
                "allOf".to_string(),
                Value::Array(vec![serde_json::json!({
                    "$ref": format!("#/components/schemas/{component_name}")
                })]),
            );
        }
        _ => {}
    }
}

fn string_enum_values(schema: &Value, require_null: bool) -> Option<Vec<String>> {
    let object = schema.as_object()?;
    if object.get("type")?.as_str()? != "string"
        || object.get("nullable") == Some(&Value::Bool(false))
    {
        return None;
    }
    let values = object.get("enum")?.as_array()?;
    if require_null && (!object.get("nullable")?.as_bool()? || !values.contains(&Value::Null)) {
        return None;
    }

    values
        .iter()
        .filter(|value| !value.is_null())
        .map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn reachable_components(source: &Value, filtered: &Map<String, Value>) -> Result<Value, String> {
    let source_components = source
        .get("components")
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenAPI document is missing object-valued `components`".to_string())?;
    let mut components = Map::new();
    let mut queued = BTreeSet::new();
    let mut queue = VecDeque::new();

    collect_component_references(&Value::Object(filtered.clone()), &mut queued, &mut queue);

    // Security requirement objects name schemes instead of using `$ref`.
    // They are tiny and retaining them avoids changing authentication metadata.
    if let Some(security_schemes) = source_components.get("securitySchemes") {
        components.insert("securitySchemes".to_string(), security_schemes.clone());
        collect_component_references(security_schemes, &mut queued, &mut queue);
    }

    while let Some((section, name)) = queue.pop_front() {
        let component = source_components
            .get(&section)
            .and_then(Value::as_object)
            .and_then(|values| values.get(&name))
            .ok_or_else(|| {
                format!("unresolved local component reference `#/components/{section}/{name}`")
            })?;

        components
            .entry(section.clone())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("new component sections are objects")
            .insert(name, component.clone());
        collect_component_references(component, &mut queued, &mut queue);
    }

    Ok(Value::Object(components))
}

fn collect_component_references(
    value: &Value,
    queued: &mut BTreeSet<(String, String)>,
    queue: &mut VecDeque<(String, String)>,
) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_component_references(value, queued, queue);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_component_references(value, queued, queue);
            }
        }
        Value::String(value) => {
            if let Some(reference) = parse_component_reference(value) {
                if queued.insert(reference.clone()) {
                    queue.push_back(reference);
                }
            }
        }
        _ => {}
    }
}

fn parse_component_reference(value: &str) -> Option<(String, String)> {
    let mut segments = value.strip_prefix("#/components/")?.split('/');
    let section = decode_json_pointer_segment(segments.next()?);
    let name = decode_json_pointer_segment(segments.next()?);
    Some((section, name))
}

fn decode_json_pointer_segment(segment: &str) -> String {
    segment.replace("~1", "/").replace("~0", "~")
}

// Progenitor represents anyOf as flattened optional fields. A branch containing
// unrestricted JSON overlaps every other alternative and cannot roundtrip that
// representation. Remove only demonstrably redundant JSON alternatives; keep
// sibling constraints and all concrete binding fields unchanged.
fn normalize_binding_unions(document: &mut Map<String, Value>) -> Result<(), String> {
    if let Some(paths) = document.get_mut("paths") {
        normalize_tagged_unions_in_openapi(paths);
    }
    if let Some(components) = document
        .get_mut("components")
        .and_then(Value::as_object_mut)
    {
        for (section, value) in components {
            if section != "schemas" {
                normalize_tagged_unions_in_openapi(value);
            }
        }
    }
    let Some(schemas) = document
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    for schema in schemas.values_mut() {
        normalize_nullable_strings(schema);
    }
    if schemas.get("JsonValue") == Some(&serde_json::json!({})) {
        for schema in schemas.values_mut() {
            simplify_unrestricted_json_unions(schema);
        }
    }
    for schema in schemas.values_mut() {
        normalize_disjoint_tagged_unions(schema);
    }
    Ok(())
}

fn simplify_unrestricted_json_unions(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        let unrestricted = object
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches.iter().any(|branch| {
                    branch == &serde_json::json!({"$ref": "#/components/schemas/JsonValue"})
                        || branch
                            == &serde_json::json!({
                                "allOf": [{"$ref": "#/components/schemas/JsonValue"}],
                                "nullable": true
                            })
                })
            });
        if unrestricted {
            object.remove("anyOf");
        }
    }
    for_schema_children_mut(schema, |child| {
        simplify_unrestricted_json_unions(child);
        Ok::<(), ()>(())
    })
    .expect("JSON union simplification is infallible");
}

// Required finite string tags prove that at most one branch can match. An
// allOf member is sufficient: every matching value must satisfy that member.
// References are deliberately left unresolved rather than guessing their tags.
fn required_string_tags(schema: &Value, field: &str) -> Option<BTreeSet<String>> {
    if schema.get("nullable") == Some(&Value::Bool(true)) || schema.get("$ref").is_some() {
        return None;
    }
    if let Some(values) = required_string_tag(schema, field)
        .and_then(|tag| tag.get("enum"))
        .and_then(Value::as_array)
    {
        let tags: Option<BTreeSet<String>> = values
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect();
        if let Some(tags) = tags.filter(|tags| !tags.is_empty()) {
            return Some(tags);
        }
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        if let Some(tags) = branches
            .iter()
            .find_map(|branch| required_string_tags(branch, field))
        {
            return Some(tags);
        }
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
            let mut tags = BTreeSet::new();
            for branch in branches {
                tags.extend(required_string_tags(branch, field)?);
            }
            return (!tags.is_empty()).then_some(tags);
        }
    }
    None
}

fn required_string_tag<'a>(schema: &'a Value, field: &str) -> Option<&'a Value> {
    let required = schema
        .get("required")
        .and_then(Value::as_array)?
        .contains(&Value::String(field.to_string()));
    let tag = schema.get("properties")?.get(field)?;
    (schema.get("type").and_then(Value::as_str) == Some("object")
        && required
        && tag.get("type").and_then(Value::as_str) == Some("string")
        && tag.get("nullable") != Some(&Value::Bool(true))
        && tag.get("$ref").is_none())
    .then_some(tag)
}

// An open tag such as an extension's resource type (`acme/dns-zone`) has a
// pattern instead of an enum. It cannot match a sibling whose finite tags the
// pattern rejects.
fn required_pattern_tag(schema: &Value, field: &str) -> Option<regress::Regex> {
    if schema.get("nullable") == Some(&Value::Bool(true)) || schema.get("$ref").is_some() {
        return None;
    }
    let tag = required_string_tag(schema, field)?;
    if tag.get("enum").is_some() {
        return None;
    }
    regress::Regex::new(tag.get("pattern")?.as_str()?).ok()
}

fn candidate_tag_fields(schema: &Value, fields: &mut BTreeSet<String>) {
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        fields.extend(
            required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
            for branch in branches {
                candidate_tag_fields(branch, fields);
            }
        }
    }
}

// OpenAPI containers use `schema` to introduce Schema Objects. Literal payloads
// and extensions are not schemas, even if they contain the same keywords.
fn normalize_tagged_unions_in_openapi(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_tagged_unions_in_openapi(value);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if key == "schema" {
                    normalize_disjoint_tagged_unions(value);
                } else if matches!(key.as_str(), "example" | "examples" | "default" | "enum")
                    || key.starts_with("x-")
                {
                    continue;
                } else {
                    normalize_tagged_unions_in_openapi(value);
                }
            }
        }
        _ => {}
    }
}

// Serde encodes an externally tagged enum's unit variants as strings and its data
// variants as one-key objects, for example `"success"` and `{"failed": {...}}`. When
// every branch is one of these and no tag repeats, at most one branch matches: a
// string never matches an object, and two object branches are disjoint only if
// neither admits the other's key, so they must forbid additional properties. The
// flattened struct Progenitor generates for anyOf cannot decode the string branches.
fn externally_tagged(branches: &[Value]) -> bool {
    let objects = branches
        .iter()
        .filter(|branch| branch.get("type").and_then(Value::as_str) == Some("object"))
        .count();
    let mut seen = BTreeSet::new();
    branches.iter().all(|branch| {
        external_tags(branch, objects > 1)
            .is_some_and(|tags| tags.into_iter().all(|tag| seen.insert(tag)))
    })
}

fn external_tags(schema: &Value, require_closed_objects: bool) -> Option<Vec<String>> {
    let object = schema.as_object()?;
    if object.get("nullable") == Some(&Value::Bool(true))
        || ["$ref", "allOf", "anyOf", "oneOf"]
            .iter()
            .any(|key| object.contains_key(*key))
    {
        return None;
    }
    match object.get("type").and_then(Value::as_str)? {
        "string" => object
            .get("enum")?
            .as_array()?
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .filter(|tags| !tags.is_empty()),
        "object" => {
            let closed = object.get("additionalProperties") == Some(&Value::Bool(false));
            if !closed && (require_closed_objects || object.contains_key("additionalProperties")) {
                return None;
            }
            let properties = object.get("properties")?.as_object()?;
            let required = object.get("required")?.as_array()?;
            let (tag, _) = properties.iter().next()?;
            (properties.len() == 1 && required.len() == 1 && required[0] == *tag)
                .then(|| vec![tag.clone()])
        }
        _ => None,
    }
}

fn normalize_disjoint_tagged_unions(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        if !object.contains_key("oneOf") && !object.contains_key("$ref") {
            let disjoint = object
                .get("anyOf")
                .and_then(Value::as_array)
                .is_some_and(|branches| {
                    let Some(first) = branches.first() else {
                        return false;
                    };
                    let mut fields = BTreeSet::new();
                    candidate_tag_fields(first, &mut fields);
                    let tagged_by_field = fields.iter().any(|field| {
                        let mut seen = BTreeSet::new();
                        let mut patterns = Vec::new();
                        // Two patterns may overlap, so at most one branch has an open tag.
                        branches
                            .iter()
                            .all(|branch| match required_string_tags(branch, field) {
                                Some(tags) => tags.into_iter().all(|tag| seen.insert(tag)),
                                None => required_pattern_tag(branch, field)
                                    .map(|pattern| patterns.push(pattern))
                                    .is_some(),
                            })
                            && patterns.len() <= 1
                            && patterns
                                .iter()
                                .all(|pattern| seen.iter().all(|tag| pattern.find(tag).is_none()))
                    });
                    tagged_by_field || externally_tagged(branches)
                });
            if disjoint {
                let branches = object.remove("anyOf").expect("union was checked");
                object.insert("oneOf".to_string(), branches);
            }
        }
    }
    for_schema_children_mut(schema, |child| {
        normalize_disjoint_tagged_unions(child);
        Ok::<(), ()>(())
    })
    .expect("tagged union normalization is infallible");
}

// A non-nullable string (open, or a closed enum) and null are disjoint. Progenitor
// needs oneOf here to generate a scalar type instead of a flattened struct that
// rejects strings.
fn normalize_nullable_strings(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        let disjoint = object
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches.len() == 2 && branches.iter().any(|branch| {
                    branch
                        == &serde_json::json!({"type": "string", "nullable": true, "enum": [null]})
                }) && branches.iter().any(|branch| {
                    branch.get("$ref").is_none()
                        && branch.get("type").and_then(Value::as_str) == Some("string")
                        && branch.get("nullable") != Some(&Value::Bool(true))
                        && branch.get("enum").is_none_or(|values| {
                            values.as_array().is_some_and(|values| {
                                !values.is_empty() && values.iter().all(Value::is_string)
                            })
                        })
                })
            });
        if disjoint && !object.contains_key("oneOf") {
            let branches = object.remove("anyOf").expect("union was checked");
            object.insert("oneOf".to_string(), branches);
        }
    }
    for_schema_children_mut(schema, |child| {
        normalize_nullable_strings(child);
        Ok::<(), ()>(())
    })
    .expect("nullable string normalization is infallible");
}
