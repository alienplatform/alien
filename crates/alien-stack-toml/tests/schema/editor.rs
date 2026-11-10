//! `schema/alien.toml.json`: the schema editors check an `alien.toml` against.
//!
//! It is the canonical schema, written the way an author writes it: kebab-case keys, units
//! left out of names, resource IDs as strings, and the shorthand of `src/sugar.rs` spelled
//! out. Unknown keys are rejected, as the resolver rejects them.

use std::collections::BTreeSet;

use alien_core::stack_authoring::{AuthorableType, AUTHORABLE_TYPES};
use alien_stack_toml::authoring::{author_key, split_unit, vocabulary};
use serde_json::{json, Map, Value};

use crate::canonical;

const SCHEMA_URL: &str = "https://alien.dev/schemas/alien.toml.json";

/// IDs the resolver accepts for resources, pools, inputs and profiles.
const ID_PATTERN: &str = "^[A-Za-z0-9_-]{1,64}$";

const NUMBER_OR_TEXT: &str = "a number, or a string with a unit such as \"512Mi\"";

struct Editor {
    components: Map<String, Value>,
}

fn definition(name: &str) -> Value {
    json!({ "$ref": format!("#/definitions/{name}") })
}

fn kebab(property: &str) -> String {
    author_key(property)
}

/// How an enum value is written: camelCase values are kebab-case, others (`GET`, `Allow`) as
/// they are.
fn enum_value(value: &str) -> String {
    if value.starts_with(|c: char| c.is_ascii_lowercase()) {
        kebab(value)
    } else {
        value.to_string()
    }
}

fn string_or_number() -> Value {
    json!({ "anyOf": [{ "type": "number" }, { "type": "string" }] })
}

/// Fields of one object, as the editor schema wants them.
fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn map_of(values: Value) -> Value {
    json!({ "type": "object", "additionalProperties": values })
}

fn list_of(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn one_of(branches: Vec<Value>) -> Value {
    json!({ "anyOf": branches })
}

fn describe(mut schema: Value, text: &str) -> Value {
    schema
        .as_object_mut()
        .expect("a schema is an object")
        .insert("description".into(), json!(text));
    schema
}

fn properties_mut(schema: &mut Value) -> &mut Map<String, Value> {
    schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("the schema has properties")
}

fn drop_required(schema: &mut Value, names: &[&str]) {
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.retain(|name| !names.iter().any(|dropped| name.as_str() == Some(dropped)));
    }
}

impl Editor {
    fn component(&self, name: &str) -> &Value {
        self.components
            .get(name)
            .unwrap_or_else(|| panic!("no component '{name}'"))
    }

    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        let mut schema = schema;
        loop {
            if let Some(target) = schema.get("$ref").and_then(Value::as_str) {
                schema = self.component(target.rsplit('/').next().unwrap_or(target));
                continue;
            }
            match schema.get("allOf").and_then(Value::as_array) {
                Some(parts) if parts.len() == 1 => schema = &parts[0],
                _ => return schema,
            }
        }
    }

    /// Whether the resolver supplies an empty list or map when the author leaves it out.
    fn defaults_to_empty(&self, schema: &Value) -> bool {
        let schema = self.resolve(schema);
        if schema.get("properties").is_some() {
            return false;
        }
        let types: Vec<&str> = match schema.get("type") {
            Some(Value::String(kind)) => vec![kind],
            Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        types.contains(&"array")
            || (types.contains(&"object")
                && schema
                    .get("additionalProperties")
                    .is_some_and(Value::is_object))
    }

    fn property(&self, name: &str, schema: &Value) -> (String, Value) {
        let schema = self.transform(schema);
        match split_unit(name) {
            Some((stem, unit)) => {
                let description = schema.get("description").cloned();
                let mut unit_schema = one_of(vec![
                    json!({ "type": "integer", "minimum": 0 }),
                    json!({ "type": "string", "pattern": unit.pattern() }),
                ]);
                if let Some(description) = description {
                    unit_schema["description"] = description;
                }
                (author_key(stem), unit_schema)
            }
            None => (kebab(name), schema),
        }
    }

    /// The canonical schema `value`, in the author's spelling.
    fn transform(&self, value: &Value) -> Value {
        let Value::Object(source) = value else {
            return value.clone();
        };
        let mut out = Map::new();
        for (key, value) in source {
            match key.as_str() {
                "$ref" => {
                    let target = value.as_str().expect("$ref is a string");
                    out.insert(
                        key.clone(),
                        json!(format!(
                            "#/definitions/{}",
                            target.rsplit('/').next().unwrap_or(target)
                        )),
                    );
                }
                "properties" => {
                    let properties = value.as_object().expect("properties is an object");
                    out.insert(
                        key.clone(),
                        Value::Object(
                            properties
                                .iter()
                                .map(|(name, schema)| self.property(name, schema))
                                .collect(),
                        ),
                    );
                }
                "required" => {
                    let names: Vec<Value> = value
                        .as_array()
                        .expect("required is a list")
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|name| {
                            let field = source["properties"].get(*name);
                            !field.is_some_and(|field| self.defaults_to_empty(field))
                        })
                        .map(|name| match split_unit(name) {
                            Some((stem, _)) => json!(author_key(stem)),
                            None => json!(kebab(name)),
                        })
                        .collect();
                    out.insert(key.clone(), Value::Array(names));
                }
                "oneOf" | "anyOf" => {
                    let branches: Vec<Value> = value
                        .as_array()
                        .expect("a union is a list")
                        .iter()
                        .filter(|branch| branch.get("type").and_then(Value::as_str) != Some("null"))
                        .map(|branch| self.transform(branch))
                        .collect();
                    out.insert("anyOf".into(), Value::Array(branches));
                }
                "allOf" => {
                    let parts: Vec<Value> = value
                        .as_array()
                        .expect("allOf is a list")
                        .iter()
                        .map(|part| self.transform(part))
                        .collect();
                    out.insert(key.clone(), Value::Array(parts));
                }
                "enum" => {
                    let values: Vec<Value> = value
                        .as_array()
                        .expect("enum is a list")
                        .iter()
                        .map(|value| match value.as_str() {
                            Some(text) => json!(enum_value(text)),
                            None => value.clone(),
                        })
                        .collect();
                    out.insert(key.clone(), Value::Array(values));
                }
                "type" => match value {
                    Value::Array(kinds) => {
                        let kinds: Vec<&Value> =
                            kinds.iter().filter(|kind| *kind != "null").collect();
                        out.insert(
                            key.clone(),
                            match kinds.as_slice() {
                                [only] => (*only).clone(),
                                _ => json!(kinds),
                            },
                        );
                    }
                    other => {
                        out.insert(key.clone(), other.clone());
                    }
                },
                "items" | "additionalProperties" => {
                    out.insert(key.clone(), self.transform(value));
                }
                "description" | "title" | "pattern" | "minimum" | "maximum" | "minItems"
                | "maxItems" | "minLength" | "maxLength" => {
                    out.insert(key.clone(), value.clone());
                }
                "default" if !value.is_object() && !value.is_array() => {
                    out.insert(key.clone(), value.clone());
                }
                _ => {}
            }
        }

        if let Some(Value::Array(branches)) = out.get("anyOf") {
            if branches.len() == 1 {
                let Some(Value::Array(mut branches)) = out.remove("anyOf") else {
                    unreachable!("checked above");
                };
                for (key, value) in branches.remove(0).as_object().cloned().unwrap_or_default() {
                    out.entry(key).or_insert(value);
                }
            }
        }
        if let Some(Value::Object(properties)) = out.get("properties") {
            if is_range(properties) {
                return range(properties, out.get("description").cloned());
            }
            out.entry("additionalProperties").or_insert(json!(false));
            out.entry("type").or_insert(json!("object"));
        }
        Value::Object(out)
    }

    fn definitions(&self) -> Map<String, Value> {
        let mut definitions: Map<String, Value> = canonical::reachable(&self.components)
            .into_iter()
            .map(|name| {
                let schema = self.transform(self.component(&name));
                (name, schema)
            })
            .collect();

        definitions.insert(
            "ResourceRef".into(),
            json!({
                "type": "string",
                "description": "The ID of a resource declared in this file.",
            }),
        );
        self.patch_definitions(&mut definitions);
        definitions
    }

    /// Where the author's shorthand changes what a type of alien-core takes.
    fn patch_definitions(&self, definitions: &mut Map<String, Value>) {
        let mut patch = |name: &str, change: &dyn Fn(&mut Value)| {
            change(
                definitions
                    .get_mut(name)
                    .unwrap_or_else(|| panic!("no definition '{name}'")),
            );
        };

        patch("StorageNotification", &|schema| {
            drop_required(schema, &["events"])
        });
        patch("ContainerAutoscaling", &|schema| {
            drop_required(schema, &["desired"])
        });
        patch("VolumeBackups", &|schema| {
            drop_required(schema, &["enabled", "interval", "retention"])
        });
        patch("PersistentStorage", &|schema| {
            drop_required(schema, &["mount-path"]);
            let properties = properties_mut(schema);
            let backups = properties.remove("backups").expect("storage has backups");
            properties.insert(
                "backups".into(),
                describe(
                    one_of(vec![json!({ "type": "boolean" }), backups]),
                    "Volume backups: true for the defaults (every 24 hours, kept 7 days), or their settings.",
                ),
            );
        });
        patch("WorkerTrigger", &|schema| {
            let branches = schema["anyOf"]
                .as_array()
                .expect("triggers are a union")
                .clone();
            let rewritten: Vec<Value> = branches
                .into_iter()
                .map(|mut branch| {
                    let tag = branch["properties"]["type"]["enum"][0]
                        .as_str()
                        .expect("a trigger has a tag")
                        .to_string();
                    let properties = properties_mut(&mut branch);
                    properties.remove("type");
                    let key = if tag == "schedule" {
                        let cron = properties.remove("cron").expect("a schedule has a cron");
                        properties.insert("schedule".into(), cron);
                        "schedule"
                    } else {
                        tag.as_str()
                    };
                    let key = key.to_string();
                    branch["required"] = json!([key]);
                    branch
                })
                .collect();
            *schema = one_of(rewritten);
        });
        patch("AuthoredInput", &|schema| {
            let properties = properties_mut(schema);
            let take = |properties: &mut Map<String, Value>, name: &str| {
                properties
                    .remove(name)
                    .unwrap_or_else(|| panic!("an input has {name}"))
            };
            let provided = take(properties, "provided-by");
            properties.insert(
                "provided-by".into(),
                one_of(vec![definition("StackInputProvider"), provided]),
            );
            let when = take(properties, "when");
            properties.insert(
                "when".into(),
                describe(
                    one_of(vec![
                        json!({ "type": "string" }),
                        when,
                    ]),
                    "Ask only when this boolean input is true: the ID of the input, or { input = \"<id>\", equals = false }.",
                ),
            );
            let generate = take(properties, "generate");
            properties.insert(
                "generate".into(),
                one_of(vec![json!({ "type": "integer", "minimum": 1 }), generate]),
            );
            let env = take(properties, "env");
            let mapping = env["items"].clone();
            properties.insert(
                "env".into(),
                describe(
                    one_of(vec![
                        json!({ "type": "string" }),
                        mapping.clone(),
                        list_of(one_of(vec![json!({ "type": "string" }), mapping])),
                    ]),
                    "Environment variables the input's value is set as: a name, a table, or a list of them.",
                ),
            );
        });
    }

    /// The table of one resource type.
    fn resource(&self, ty: &AuthorableType) -> Value {
        let kind = ty.table;
        let mut schema = self.transform(self.component(ty.schema));
        let vocabulary = vocabulary(kind);

        let properties = properties_mut(&mut schema);
        for key in ["id", "type"]
            .into_iter()
            .chain(vocabulary.hidden.iter().copied())
        {
            properties.remove(key);
        }
        let hidden: Vec<&str> = ["id", "type"]
            .into_iter()
            .chain(vocabulary.hidden.iter().copied())
            .collect();
        drop_required(&mut schema, &hidden);

        if kind == "sandbox" {
            let properties = properties_mut(&mut schema);
            let session = properties
                .remove("lifecycle")
                .expect("a sandbox has a lifecycle");
            properties.insert("session".into(), session);
            drop_required(&mut schema, &["lifecycle"]);
        }
        let properties = properties_mut(&mut schema);
        properties.insert(
            "lifecycle".into(),
            describe(
                json!({ "enum": ["live", "frozen"] }),
                "`live` resources are updated by releases; `frozen` ones are set up once and left alone.",
            ),
        );
        // Alien derives some resources from the stack itself, so they cannot be optional.
        let resource_type = if kind == "build-environment" {
            "build"
        } else {
            kind
        };
        if alien_core::gate_refusal(resource_type, "").is_none() {
            properties.insert(
                "enabled-when".into(),
                describe(
                    json!({ "type": "string" }),
                    "Create the resource only when this boolean `[input.<id>]` is true.",
                ),
            );
        }
        properties.insert("display-name".into(), json!({ "type": "string" }));
        properties.insert("description".into(), json!({ "type": "string" }));
        properties.insert("remote-access".into(), json!({ "type": "boolean" }));

        if ["container", "daemon", "worker", "sandbox"].contains(&kind) {
            self.code(&mut schema);
        }
        if ["container", "daemon", "worker", "build-environment"].contains(&kind) {
            self.workload(&mut schema);
        }
        match kind {
            "container" => self.container(&mut schema),
            "daemon" => self.daemon(&mut schema),
            "worker" => self.worker(&mut schema),
            "email" => self.email(&mut schema),
            _ => {}
        }
        schema
    }

    /// `image` or `build` in place of `code`.
    fn code(&self, schema: &mut Value) {
        let toolchains: Vec<Value> = self.component("ToolchainConfig")["oneOf"]
            .as_array()
            .expect("toolchains are a union")
            .iter()
            .map(|branch| {
                let mut branch = self.transform(branch);
                let tag = branch["properties"]["type"]["enum"][0].clone();
                let properties = properties_mut(&mut branch);
                properties.remove("type");
                properties.insert("toolchain".into(), json!({ "enum": [tag] }));
                properties.insert(
                    "src".into(),
                    describe(
                        json!({ "type": "string", "default": "." }),
                        "The directory to build, relative to alien.toml.",
                    ),
                );
                let mut required = branch["required"].as_array().cloned().unwrap_or_default();
                required.retain(|name| name != "type");
                required.push(json!("toolchain"));
                branch["required"] = Value::Array(required);
                branch
            })
            .collect();
        let properties = properties_mut(schema);
        properties.insert(
            "image".into(),
            describe(
                json!({ "type": "string" }),
                "A prebuilt image to run, instead of `build`.",
            ),
        );
        properties.insert(
            "build".into(),
            describe(
                one_of(toolchains),
                "Build from source: { toolchain = \"rust\", binary-name = \"<binary>\", src = \".\" }.",
            ),
        );
        schema["oneOf"] = json!([{ "required": ["image"] }, { "required": ["build"] }]);
    }

    fn workload(&self, schema: &mut Value) {
        let properties = properties_mut(schema);
        properties.insert(
            "env".into(),
            describe(
                map_of(one_of(vec![
                    json!({ "type": "string" }),
                    json!({ "type": "number" }),
                    json!({ "type": "boolean" }),
                    object(
                        json!({
                            "input": { "type": "string", "description": "The ID of a stack input." },
                            "type": { "enum": ["plain", "secret"] },
                        }),
                        &["input"],
                    ),
                ])),
                "Environment variables: a literal, or { input = \"<id>\" } to take the value of a stack input.",
            ),
        );
        properties.insert(
            "env-file".into(),
            describe(
                one_of(vec![
                    json!({ "type": "string" }),
                    list_of(json!({ "type": "string" })),
                ]),
                "A KEY=value file, or a list of them, relative to alien.toml.",
            ),
        );
        properties.insert(
            "permissions".into(),
            describe(
                one_of(vec![
                    json!({ "type": "string" }),
                    definition("PermissionProfile"),
                ]),
                "A permission profile name, or the grants in place, which make a profile named after the resource.",
            ),
        );
        drop_required(schema, &["permissions"]);
    }

    fn container(&self, schema: &mut Value) {
        let spec = |field: &str| {
            let value = string_or_number();
            describe(
                one_of(vec![
                    json!({ "type": "number" }),
                    json!({ "type": "string" }),
                    object(
                        json!({
                            "default": value.clone(),
                            "allowed": { "type": "array", "items": value, "minItems": 2, "maxItems": 2 },
                        }),
                        &["default", "allowed"],
                    ),
                ]),
                &format!("{field} reserved per instance, or a range the deployer picks from: {{ default = 1, allowed = [0.5, 2] }}."),
            )
        };
        let properties = properties_mut(schema);
        properties.insert("cpu".into(), spec("CPU cores"));
        properties.insert("memory".into(), spec("Memory"));
        properties.insert("pool".into(), json!({ "type": "string" }));
        properties.insert(
            "public-endpoints".into(),
            describe(
                map_of(self.endpoint("PublicEndpoint")),
                "Public HTTP endpoints by name, such as api = { port = 8080 }.",
            ),
        );
        let ports = properties.remove("ports").expect("a container has ports");
        properties.insert(
            "ports".into(),
            describe(
                list_of(one_of(vec![
                    json!({ "type": "integer" }),
                    ports["items"].clone(),
                ])),
                "Ports the container listens on: numbers, or tables.",
            ),
        );
        let tunnel = properties
            .remove("tunnel")
            .expect("a container has a tunnel");
        properties.insert(
            "tunnel".into(),
            one_of(vec![json!({ "type": "integer" }), tunnel]),
        );
        let storage = properties
            .remove("persistent-storage")
            .expect("a container has persistent storage");
        properties.insert(
            "persistent-storage".into(),
            one_of(vec![
                describe(
                    json!({ "type": "string" }),
                    "The size of the volume, such as \"10Gi\".",
                ),
                storage,
            ]),
        );
        drop_required(schema, &["cpu", "memory"]);
    }

    fn daemon(&self, schema: &mut Value) {
        let properties = properties_mut(schema);
        for field in ["cpu", "memory"] {
            properties.insert(
                field.into(),
                describe(
                    string_or_number(),
                    &format!("{field} reserved per instance: {NUMBER_OR_TEXT}."),
                ),
            );
        }
        properties.insert("pool".into(), json!({ "type": "string" }));
        let mut endpoint = self.endpoint("PublicEndpoint");
        properties_mut(&mut endpoint).insert(
            "protocol".into(),
            describe(
                json!({ "enum": ["http"] }),
                "Daemon endpoints serve HTTP only.",
            ),
        );
        properties.insert(
            "public-endpoints".into(),
            describe(
                map_of(endpoint),
                "Public HTTP endpoints by name, such as api = { port = 8080 }.",
            ),
        );
        properties.insert(
            "host-access".into(),
            describe(
                json!({ "type": "boolean" }),
                "Run privileged, in the host's PID namespace and network, with the host's `/` at `/host`, as uid 0.",
            ),
        );
        drop_required(schema, &["cpu", "memory"]);
    }

    fn worker(&self, schema: &mut Value) {
        let properties = properties_mut(schema);
        properties.insert(
            "public-endpoints".into(),
            describe(
                one_of(vec![
                    list_of(json!({ "type": "string" })),
                    map_of(self.endpoint("WorkerPublicEndpoint")),
                ]),
                "Public HTTP endpoints: a list of names, or a table by name.",
            ),
        );
    }

    fn email(&self, schema: &mut Value) {
        let properties = properties_mut(schema);
        for key in ["inbound", "events"] {
            let table = properties
                .remove(key)
                .unwrap_or_else(|| panic!("email has {key}"));
            properties.insert(
                key.into(),
                one_of(vec![
                    describe(json!({ "type": "string" }), "The ID of the resource."),
                    table,
                ]),
            );
        }
    }

    /// An endpoint table, whose name is the key it sits under.
    fn endpoint(&self, name: &str) -> Value {
        let mut endpoint = self.transform(self.component(name));
        properties_mut(&mut endpoint).remove("name");
        drop_required(&mut endpoint, &["name", "protocol"]);
        endpoint
    }

    fn root(&self) -> Value {
        let mut properties = Map::new();
        properties.insert(
            "name".into(),
            describe(
                json!({ "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }),
                "The stack's name: letters, digits, hyphens and underscores.",
            ),
        );
        properties.insert(
            "platforms".into(),
            describe(
                list_of(definition("Platform")),
                "Platforms the stack deploys to. Omit for all.",
            ),
        );
        for key in [
            "dynamic-container-repositories",
            "dynamic-container-image-resources",
        ] {
            properties.insert(key.into(), list_of(json!({ "type": "string" })));
        }
        properties.insert(
            "input".into(),
            describe(
                id_map(definition("AuthoredInput")),
                "Values the deployer or developer provides, keyed by ID.",
            ),
        );

        let mut cluster = self.transform(self.component("ComputeCluster"));
        let cluster_properties = properties_mut(&mut cluster);
        for key in [
            "capacity-groups",
            "dynamic-container-pool",
            "failure-domain-spread",
            "selected-failure-domains",
        ] {
            cluster_properties.remove(key);
        }
        for key in ["lifecycle", "display-name", "description"] {
            cluster_properties.insert(
                key.into(),
                if key == "lifecycle" {
                    json!({ "enum": ["live", "frozen"] })
                } else {
                    json!({ "type": "string" })
                },
            );
        }
        drop_required(&mut cluster, &["id", "capacity-groups"]);
        properties.insert(
            "cluster".into(),
            describe(
                cluster,
                "The compute cluster that holds the pools. Its ID defaults to `compute`.",
            ),
        );

        let mut pool = self.transform(self.component("AuthoredPool"));
        properties_mut(&mut pool).insert(
            "dynamic-containers".into(),
            describe(
                json!({ "type": "boolean" }),
                "Containers created after installation run on this pool. One pool may set it.",
            ),
        );
        properties.insert(
            "pool".into(),
            describe(
                id_map(pool),
                "Groups of machines containers and daemons run on, keyed by name.",
            ),
        );
        properties.insert(
            "permissions".into(),
            describe(
                id_map(definition("PermissionProfile")),
                "Permission profiles, keyed by name.",
            ),
        );
        properties.insert(
            "management-permissions".into(),
            definition("ManagementPermissions"),
        );
        properties.insert("operations".into(), self.operations());

        let mut experimental = Map::new();
        for ty in AUTHORABLE_TYPES {
            match ty.table.strip_prefix("experimental.") {
                Some(name) => {
                    experimental.insert(name.to_string(), id_map(definition(ty.schema)));
                }
                None => {
                    properties.insert(ty.table.to_string(), id_map(definition(ty.schema)));
                }
            }
        }
        properties.insert(
            "experimental".into(),
            object(Value::Object(experimental), &[]),
        );

        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "$id": SCHEMA_URL,
            "title": "alien.toml",
            "description": "An Alien stack written in TOML.",
            "type": "object",
            "properties": properties,
            "required": ["name"],
            "additionalProperties": false,
        })
    }

    fn operations(&self) -> Value {
        let plugin = || {
            json!({
                "type": "object",
                "properties": {
                    "approval": map_of(definition("OperationApproval")),
                },
                "additionalProperties": definition("OperationSettingValue"),
            })
        };
        let mut custom = plugin();
        custom["properties"]["name"] = json!({ "type": "string" });
        custom["properties"]["version"] = json!({ "type": "string" });
        custom["required"] = json!(["name", "version"]);
        describe(
            json!({
                "type": "object",
                "properties": {
                    "custom": list_of(one_of(vec![
                        json!({ "type": "string", "pattern": "^.+@[^@]+$" }),
                        custom,
                    ])),
                },
                "additionalProperties": plugin(),
            }),
            "Operations plugins, keyed by name; custom ones are \"name@version\" or tables.",
        )
    }
}

/// A table keyed by an ID.
fn id_map(values: Value) -> Value {
    json!({
        "type": "object",
        "propertyNames": { "pattern": ID_PATTERN },
        "additionalProperties": values,
    })
}

fn is_range(properties: &Map<String, Value>) -> bool {
    properties.len() == 3
        && ["min", "max", "default"]
            .iter()
            .all(|name| properties.contains_key(*name))
}

/// `{ default = 2, allowed = [1, 4] }`.
fn range(properties: &Map<String, Value>, description: Option<Value>) -> Value {
    let value = properties["default"].clone();
    let mut schema = object(
        json!({
            "default": value.clone(),
            "allowed": { "type": "array", "items": value, "minItems": 2, "maxItems": 2 },
        }),
        &["default", "allowed"],
    );
    if let Some(description) = description {
        schema["description"] = description;
    }
    schema
}

fn references(schema: &Value, found: &mut BTreeSet<String>) {
    match schema {
        Value::Object(object) => {
            for (key, value) in object {
                match (key.as_str(), value) {
                    ("$ref", Value::String(target)) => {
                        found.insert(target.rsplit('/').next().unwrap_or(target).to_string());
                    }
                    _ => references(value, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| references(item, found)),
        _ => {}
    }
}

pub fn generate() -> String {
    let editor = Editor {
        components: canonical::components(),
    };
    let mut definitions = editor.definitions();
    for ty in AUTHORABLE_TYPES {
        definitions.insert(ty.schema.to_string(), editor.resource(ty));
    }
    let root = editor.root();

    let mut keep = BTreeSet::new();
    let mut pending = BTreeSet::new();
    references(&root, &mut pending);
    while let Some(name) = pending.pop_first() {
        if keep.insert(name.clone()) {
            references(&definitions[&name], &mut pending);
        }
    }
    definitions.retain(|name, _| keep.contains(name));

    let mut document = root;
    document["definitions"] = Value::Object(definitions);
    let mut text =
        serde_json::to_string_pretty(&canonical::sorted(document)).expect("the schema serializes");
    text.push('\n');
    // The doc comments the descriptions come from use dashes the repository keeps out of
    // committed files.
    text.replace(" \u{2014} ", " - ")
        .replace(['\u{2014}', '\u{2013}'], "-")
}

#[test]
fn editor_schema_is_up_to_date() {
    crate::check_generated("alien.toml.json", &generate());
}
