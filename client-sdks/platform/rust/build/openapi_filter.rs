use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};

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

// The server adds package types without a client release. No Alien consumer matches on these
// two response fields, so they decode as plain strings instead of closed enums. When the
// component is present, a moved field or a missing enum fails generation.
const OPEN_STRING_ENUM_SCHEMAS: &[(&str, &str)] = &[
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
    "configureProjectBuckets",
    "configureProjectDeployments",
    "configureProjectKeys",
    "configureProjectModels",
    "configureProjectRegistry",
    "configureProjectRemoteSandbox",
    "createAPIKey",
    "createAccessRequest",
    "createDeployment",
    "createDeploymentGroup",
    "createDeploymentGroupToken",
    "createDeploymentToken",
    "createProject",
    "createRelease",
    "createReleaseChannel",
    "createRemoteBindingsExternalAccess",
    "createSetupLink",
    "createWorkspace",
    "deleteDeployment",
    "deleteManager",
    "deleteProject",
    "deleteReleaseChannel",
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
    "getDeploymentUpdateOperation",
    "getManager",
    "getProject",
    "getProjectCapabilityOverview",
    "getProjectDeploymentLinkSetup",
    "getRelease",
    "getResourceDeploymentDetail",
    "invokeOperation",
    "listAPIKeys",
    "listDeploymentGroups",
    "listDeployments",
    "listEvents",
    "listDeploymentMachines",
    "listManagerEvents",
    "listManagers",
    "listMemberships",
    "listProjects",
    "listReleaseChannels",
    "listReleases",
    "pinDeploymentRelease",
    "promoteRelease",
    "redeployDeployment",
    "resolve",
    "retryDeployment",
    "revokeAPIKey",
    "revokeAccessRequest",
    "setDeploymentGroupExternalId",
    "setDeploymentReleaseChannel",
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
    preserve_secret_presence(&mut filtered)?;
    normalize_binding_unions(&mut filtered)?;
    open_string_enums(&mut filtered)?;
    deduplicate_anonymous_schemas(
        &mut filtered,
        required_operation_ids == REQUIRED_OPERATION_IDS,
    )?;
    canonicalize_nullable_enums(&mut filtered);
    allow_unknown_properties(&mut filtered);

    Ok(Value::Object(filtered))
}

fn deduplicate_anonymous_schemas(
    document: &mut Map<String, Value>,
    protect_consumer_names: bool,
) -> Result<(), String> {
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

    Ok(())
}

fn is_shareable_anonymous_schema(schema: &Value) -> bool {
    schema.get("$ref").is_none() && schema.get("type").and_then(Value::as_str) == Some("object")
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
        let key = serde_json::to_string(&schema).expect("JSON schema serializes");
        occurrences
            .entry(key)
            .and_modify(|(count, _)| *count += 1)
            .or_insert((1, schema));
    }
    for_schema_children(value, |child| {
        collect_schema_occurrences(child, occurrences, true);
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

    for_schema_children_mut(value, |child| {
        replace_schema_occurrences(child, replacements, true)
    })?;
    Ok(())
}

fn for_schema_children<E>(
    schema: &Value,
    mut visit: impl FnMut(&Value) -> Result<(), E>,
) -> Result<(), E> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    for key in ["properties", "patternProperties", "dependentSchemas"] {
        if let Some(children) = object.get(key).and_then(Value::as_object) {
            for child in children.values() {
                visit(child)?;
            }
        }
    }
    for key in [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "propertyNames",
        "contains",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = object.get(key).filter(|value| value.is_object()) {
            visit(child)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get(key).and_then(Value::as_array) {
            for child in children {
                visit(child)?;
            }
        }
    }
    Ok(())
}

fn for_schema_children_mut<E>(
    schema: &mut Value,
    mut visit: impl FnMut(&mut Value) -> Result<(), E>,
) -> Result<(), E> {
    let Some(object) = schema.as_object_mut() else {
        return Ok(());
    };
    for key in ["properties", "patternProperties", "dependentSchemas"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                visit(child)?;
            }
        }
    }
    for key in [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "propertyNames",
        "contains",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = object.get_mut(key).filter(|value| value.is_object()) {
            visit(child)?;
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get_mut(key).and_then(Value::as_array_mut) {
            for child in children {
                visit(child)?;
            }
        }
    }
    Ok(())
}

fn canonical_schema_key(schema: &Value) -> Result<String, String> {
    serde_json::to_string(schema).map_err(|error| format!("failed to serialize schema: {error}"))
}

pub fn normalize_openapi(document: &Value) -> Result<Value, String> {
    let mut root = document
        .as_object()
        .ok_or_else(|| "OpenAPI document must be a JSON object".to_string())?
        .clone();
    preserve_secret_presence(&mut root)?;
    normalize_binding_unions(&mut root)?;
    open_string_enums(&mut root)?;
    canonicalize_nullable_enums(&mut root);
    allow_unknown_properties(&mut root);
    Ok(Value::Object(root))
}

// A native Vec avoids typify's intrinsic empty-array default for this optional
// field, yielding Option<Vec<String>> without changing the canonical API schema.
fn preserve_secret_presence(document: &mut Map<String, Value>) -> Result<(), String> {
    let Some(config) = document
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(|schemas| schemas.get_mut("DeploymentConfig"))
    else {
        return Ok(());
    };
    let required = config
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|fields| fields.iter().any(|field| field == "storedSecretInputIds"));
    let Some(field) = config.pointer_mut("/properties/storedSecretInputIds") else {
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
        return Err("DeploymentConfig.storedSecretInputIds must remain an optional unconstrained nonnullable string array".to_string());
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
        normalize_nullable_string_enums(schema);
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
    if schema.get("type").and_then(Value::as_str) == Some("object")
        && schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| required.contains(&Value::String(field.to_string())))
    {
        if let Some(tag) = schema
            .get("properties")
            .and_then(|properties| properties.get(field))
        {
            if tag.get("type").and_then(Value::as_str) == Some("string")
                && tag.get("nullable") != Some(&Value::Bool(true))
                && tag.get("$ref").is_none()
            {
                if let Some(values) = tag.get("enum").and_then(Value::as_array) {
                    let tags: Option<BTreeSet<String>> = values
                        .iter()
                        .map(|value| value.as_str().map(str::to_string))
                        .collect();
                    if let Some(tags) = tags.filter(|tags| !tags.is_empty()) {
                        return Some(tags);
                    }
                }
            }
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
                    fields.iter().any(|field| {
                        let mut seen = BTreeSet::new();
                        branches.iter().all(|branch| {
                            required_string_tags(branch, field)
                                .is_some_and(|tags| tags.into_iter().all(|tag| seen.insert(tag)))
                        })
                    })
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

// A closed string enum and null are disjoint. Progenitor needs oneOf here
// to generate a scalar enum instead of a flattened struct that rejects strings.
fn normalize_nullable_string_enums(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        let disjoint =
            object
                .get("anyOf")
                .and_then(Value::as_array)
                .is_some_and(|branches| {
                    branches.len() == 2 && branches.iter().any(|branch| {
                branch == &serde_json::json!({"type": "string", "nullable": true, "enum": [null]})
            }) && branches.iter().any(|branch| {
                branch.get("$ref").is_none()
                    && branch.get("type").and_then(Value::as_str) == Some("string")
                    && branch.get("nullable") != Some(&Value::Bool(true))
                    && branch.get("enum").and_then(Value::as_array).is_some_and(|values| {
                        !values.is_empty() && values.iter().all(Value::is_string)
                    })
            })
                });
        if disjoint && !object.contains_key("oneOf") {
            let branches = object.remove("anyOf").expect("union was checked");
            object.insert("oneOf".to_string(), branches);
        }
    }
    for_schema_children_mut(schema, |child| {
        normalize_nullable_string_enums(child);
        Ok::<(), ()>(())
    })
    .expect("nullable enum normalization is infallible");
}
