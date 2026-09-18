use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Map, Value};

const HTTP_METHODS: &[&str] = &[
    "delete", "get", "head", "options", "patch", "post", "put", "trace",
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
    "deleteReleaseChannel",
    "ensureDeploymentGroupByExternalId",
    "generateManagerBindingToken",
    "generateManagerToken",
    "getAPIKey",
    "getAccessRequest",
    "getAccessRequestCoordinates",
    "getDeployment",
    "getDeploymentGroup",
    "getDeploymentGroupByExternalId",
    "getDeploymentInfo",
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
    "listMachinesInventory",
    "listManagerEvents",
    "listManagers",
    "listMemberships",
    "listProjects",
    "listReleaseChannels",
    "listReleases",
    "pinDeploymentRelease",
    "promoteRelease",
    "resolve",
    "retryDeployment",
    "revokeAPIKey",
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
    canonicalize_nullable_enums(&mut filtered);

    Ok(Value::Object(filtered))
}

pub fn normalize_openapi(document: &Value) -> Result<Value, String> {
    let mut root = document
        .as_object()
        .ok_or_else(|| "OpenAPI document must be a JSON object".to_string())?
        .clone();
    canonicalize_nullable_enums(&mut root);
    Ok(Value::Object(root))
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
