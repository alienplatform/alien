//! `generated/index.ts`: the extension, as `Stack.extensions()` takes it, and one builder per
//! custom resource type, as Alien's own builders are written.
//!
//! The builders validate with zod schemas written from each type's JSON Schemas. The schemas
//! come from `utoipa`, so the writer handles what it emits for Rust types: objects with named
//! properties, structs that flatten structs, maps, arrays, strings, numbers, booleans, string
//! enums, nullable values, unions and references to `$defs`. Anything else stops the build,
//! naming the type and the construct.

use alien_core::{ExtensionManifest, ResourceTypeDescriptor};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Methods every builder already has; a config field with one of these names can't get a setter.
const BUILDER_METHODS: [&str; 6] = [
    "build",
    "enabled",
    "displayName",
    "description",
    "resource",
    "constructor",
];

/// Names the module declares or uses besides the builders' own: its imports, its constants and
/// the globals it refers to. A builder that exported one of them would break the module.
const MODULE_NAMES: [&str; 10] = [
    "CustomResourceType",
    "ExtensionManifest",
    "Resource",
    "z",
    "extension",
    "modules",
    "globalThis",
    "Symbol",
    "Record",
    "Partial",
];

/// The module's source.
pub(crate) fn index_ts(manifest: &ExtensionManifest) -> Result<String, String> {
    let manifest_json = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("the extension manifest did not serialize: {error}"))?;
    let mut builders = String::new();
    let mut exported = HashMap::new();
    for (resource_type, descriptor) in &manifest.resource_types {
        builders.push_str(&builder(resource_type.as_ref(), descriptor, &mut exported)?);
    }
    let imports = if manifest.resource_types.is_empty() {
        "import type { ExtensionManifest } from \"@alienplatform/core\"".to_string()
    } else {
        "import {\n  CustomResourceType,\n  type ExtensionManifest,\n  type Resource,\n  z,\n} from \"@alienplatform/core\"".to_string()
    };
    Ok(format!(
        r#"// Written by `alien extensions build`. The next build replaces it.
{imports}

/** The {name} extension. List it in `Stack.extensions()`. */
const extension: ExtensionManifest = {manifest_json}

// The Alien CLI reads the extension's bundle from beside this module.
const modules = ((globalThis as Record<symbol, unknown>)[Symbol.for("{key}")] ??= []) as {{ name: string; url: string }}[]
modules.push({{ name: extension.name, url: import.meta.url }})

export default extension
{builders}"#,
        name = manifest.name,
        key = crate::config::EXTENSION_MODULES_KEY,
    ))
}

/// The zod schemas of one type's config and outputs, and its builder class. `exported` holds
/// the names earlier types' builders export, each with its type.
fn builder(
    resource_type: &str,
    descriptor: &ResourceTypeDescriptor,
    exported: &mut HashMap<String, String>,
) -> Result<String, String> {
    let config = &descriptor.schemas.config;
    let outputs = &descriptor.schemas.outputs;
    let class = class_name(config, resource_type, "");
    let outputs_name = class_name(outputs, resource_type, "Outputs");
    for name in [
        format!("{class}ConfigSchema"),
        format!("{class}Config"),
        class.clone(),
        format!("{outputs_name}Schema"),
        outputs_name.clone(),
    ] {
        if MODULE_NAMES.contains(&name.as_str()) {
            return Err(format!(
                "`{resource_type}` would export `{name}`, a name the generated module already uses; rename its Rust type"
            ));
        }
        if let Some(other) = exported.insert(name.clone(), resource_type.to_string()) {
            return Err(format!(
                "`{resource_type}` would export `{name}`, which `{other}` already exports; rename one of their Rust types"
            ));
        }
    }
    let defs = |schema: &Map<String, Value>| {
        schema
            .get("$defs")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let in_type = |what: &str, schema: &Map<String, Value>| {
        Writer {
            defs: defs(schema),
            depth: 0,
        }
        .zod(&Value::Object(schema.clone()))
        .map_err(|problem| format!("the {what} schema of `{resource_type}` {problem}"))
    };
    let config_zod = in_type("config", config)?;
    let outputs_zod = in_type("outputs", outputs)?;

    let config_fields = struct_fields(config, &defs(config), 0)
        .map_err(|problem| format!("the config schema of `{resource_type}` {problem}"))?;
    let mut setters = String::new();
    for (field, property) in sorted_properties(&config_fields) {
        if field == "id" || field == "type" {
            continue;
        }
        if BUILDER_METHODS.contains(&field.as_str()) {
            return Err(format!(
                "`{resource_type}` has a config field named `{field}`, which every builder already has as a method; rename the field"
            ));
        }
        if !is_identifier(field) {
            return Err(format!(
                "`{resource_type}` has a config field named `{field}`, which is not a TypeScript method name"
            ));
        }
        // The builder and the class it extends keep their own state in `_`-prefixed members,
        // such as `_config` and `_displayName`.
        if field.starts_with('_') {
            return Err(format!(
                "`{resource_type}` has a config field named `{field}`; a name starting with `_` is the builder's own; rename the field"
            ));
        }
        let doc = property
            .get("description")
            .and_then(Value::as_str)
            .map(|text| format!("  /** {} */\n", text.replace("*/", "*\\/")))
            .unwrap_or_default();
        setters.push_str(&format!(
            "\n{doc}  public {field}(value: {class}Config[\"{field}\"]): this {{\n    this._config.{field} = value\n    return this\n  }}\n"
        ));
    }
    // A `#[serde(flatten)]` map beside the named fields: its entries have no setter of their own,
    // so `set` writes one, refusing a key that would replace a named field.
    if !matches!(
        config_fields.get("additionalProperties"),
        None | Some(Value::Bool(false))
    ) {
        let mut named: Vec<&String> = sorted_properties(&config_fields)
            .into_iter()
            .map(|(field, _)| field)
            .collect();
        if named.iter().any(|field| field.as_str() == "set") {
            return Err(format!(
                "`{resource_type}` has a config field named `set` beside a flattened map, whose entries the builder sets with `set`; rename the field"
            ));
        }
        named.retain(|field| field.as_str() != "type");
        let mut reserved = vec!["type".to_string()];
        reserved.extend(named.into_iter().cloned());
        let reserved = serde_json::to_string(&reserved).unwrap_or_default();
        setters.push_str(&format!(
            "\n  /** Sets `key` among the settings the config flattens beside its named fields. */\n  public set(key: string, value: unknown): this {{\n    if ({reserved}.includes(key)) {{\n      throw new Error(`${{key}} is a named field of the config; set it with its own method`)\n    }}\n    ;(this._config as Record<string, unknown>)[key] = value\n    return this\n  }}\n"
        ));
    }
    let doc = config
        .get("description")
        .and_then(Value::as_str)
        .map(|text| format!("/** {} */\n", text.replace("*/", "*\\/")))
        .unwrap_or_default();
    let type_literal = serde_json::to_string(resource_type).unwrap_or_default();
    Ok(format!(
        r#"
/** The config of each `{resource_type}` resource. */
export const {class}ConfigSchema = {config_zod}
export type {class}Config = z.infer<typeof {class}ConfigSchema>

/** What the controller or importer of each `{resource_type}` resource reports. */
export const {outputs_name}Schema = {outputs_zod}
export type {outputs_name} = z.infer<typeof {outputs_name}Schema>

{doc}export class {class} extends CustomResourceType {{
  private _config: Partial<{class}Config> = {{}}

  constructor(id: string) {{
    super()
    this._config.id = id
  }}
{setters}
  /**
   * Validates the config and builds the resource.
   * @throws Error if the config is invalid.
   */
  public build(): Resource {{
    const config = {class}ConfigSchema.parse(this._config)
    return this.resource({{ type: {type_literal}, ...config }})
  }}
}}
"#
    ))
}

/// The schema's `title`, which the SDK sets to the Rust type's name, or one from the type name.
fn class_name(schema: &Map<String, Value>, resource_type: &str, suffix: &str) -> String {
    match schema.get("title").and_then(Value::as_str) {
        Some(title) if is_identifier(title) => title.to_string(),
        _ => {
            let name = resource_type.rsplit('/').next().unwrap_or(resource_type);
            let pascal: String = name
                .split(['-', '_'])
                .map(|part| {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                        .unwrap_or_default()
                })
                .collect();
            format!("{pascal}{suffix}")
        }
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Writes zod expressions, resolving `$defs` references inline.
struct Writer {
    defs: Map<String, Value>,
    depth: usize,
}

impl Writer {
    fn zod(&mut self, schema: &Value) -> Result<String, String> {
        // utoipa writes no recursive schemas for Rust types without boxing, and the writer inlines
        // references, so a deep chain means a type that refers to itself.
        if self.depth > 32 {
            return Err("refers to itself, which a builder can't express".to_string());
        }
        self.depth += 1;
        let written = self.write(schema);
        self.depth -= 1;
        written
    }

    fn write(&mut self, schema: &Value) -> Result<String, String> {
        let Value::Object(schema) = schema else {
            return match schema {
                Value::Bool(true) => Ok("z.unknown()".to_string()),
                _ => Err(format!("has `{schema}` where a schema belongs")),
            };
        };
        // A schema that constrains nothing, such as a `serde_json::Value` map value's `{}`, takes
        // any JSON value.
        const CONSTRAINTS: [&str; 10] = [
            "type",
            "$ref",
            "enum",
            "const",
            "oneOf",
            "anyOf",
            "allOf",
            "properties",
            "items",
            "additionalProperties",
        ];
        if !CONSTRAINTS
            .iter()
            .any(|keyword| schema.contains_key(*keyword))
        {
            return Ok("z.unknown()".to_string());
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let target = definition(&self.defs, reference)?.clone();
            let written = self.zod(&target)?;
            return nullable(schema, written);
        }
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            let written = if values.iter().all(Value::is_string) {
                format!("z.enum({})", Value::Array(values.clone()))
            } else {
                let literals: Vec<String> = values
                    .iter()
                    .map(|value| format!("z.literal({value})"))
                    .collect();
                union(literals)
            };
            return nullable(schema, written);
        }
        for keyword in ["oneOf", "anyOf"] {
            if let Some(options) = schema.get(keyword).and_then(Value::as_array) {
                let mut written = Vec::new();
                let mut nullable = false;
                for option in options {
                    if option.get("type").and_then(Value::as_str) == Some("null") {
                        nullable = true;
                    } else {
                        written.push(self.zod(option)?);
                    }
                }
                let written = union(written);
                return Ok(if nullable {
                    format!("{written}.nullable()")
                } else {
                    written
                });
            }
        }
        if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
            let written = match parts.as_slice() {
                [only] => self.zod(only)?,
                _ => self.object(&struct_fields(schema, &self.defs, 0)?)?,
            };
            return nullable(schema, written);
        }
        let types: Vec<&str> = match schema.get("type") {
            Some(Value::String(one)) => vec![one.as_str()],
            Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
            None if schema.contains_key("properties") => vec!["object"],
            _ => {
                return Err(format!(
                    "has a schema without a type: {}",
                    Value::Object(schema.clone())
                ))
            }
        };
        let nullable = types.contains(&"null");
        let types: Vec<&str> = types.into_iter().filter(|t| *t != "null").collect();
        let written: Vec<String> = types
            .iter()
            .map(|kind| self.typed(kind, schema))
            .collect::<Result<_, _>>()?;
        let written = match written.len() {
            0 => "z.null()".to_string(),
            _ => union(written),
        };
        Ok(if nullable && !types.is_empty() {
            format!("{written}.nullable()")
        } else {
            written
        })
    }

    fn typed(&mut self, kind: &str, schema: &Map<String, Value>) -> Result<String, String> {
        Ok(match kind {
            "string" => "z.string()".to_string(),
            "boolean" => "z.boolean()".to_string(),
            "number" | "integer" => {
                let mut written = String::from("z.number()");
                if kind == "integer" {
                    written.push_str(".int()");
                }
                if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
                    written.push_str(&format!(".min({minimum})"));
                }
                if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
                    written.push_str(&format!(".max({maximum})"));
                }
                written
            }
            // A tuple lists one schema per position.
            "array" if schema.get("prefixItems").is_some() => {
                let positions = schema
                    .get("prefixItems")
                    .and_then(Value::as_array)
                    .ok_or("has `prefixItems` that is not a list")?;
                let positions = positions
                    .iter()
                    .map(|position| self.zod(position))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("z.tuple([{}])", positions.join(", "))
            }
            "array" => {
                let items = schema.get("items").ok_or("has an array without `items`")?;
                format!("z.array({})", self.zod(items)?)
            }
            "object" => self.object(schema)?,
            other => {
                return Err(format!(
                    "has the type `{other}`, which a builder can't express"
                ))
            }
        })
    }

    fn object(&mut self, schema: &Map<String, Value>) -> Result<String, String> {
        let properties = schema.get("properties").and_then(Value::as_object);
        if properties.is_none() {
            return Ok(match schema.get("additionalProperties") {
                Some(value @ Value::Object(_)) => {
                    format!("z.record(z.string(), {})", self.zod(value)?)
                }
                _ => "z.record(z.string(), z.unknown())".to_string(),
            });
        }
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut fields = Vec::new();
        for (name, property) in sorted_properties(schema) {
            let mut written = self.zod(property)?;
            if !required.contains(&name.as_str()) {
                written.push_str(".optional()");
            }
            fields.push(format!(
                "{}: {written}",
                serde_json::to_string(name).unwrap_or_default()
            ));
        }
        let written = format!("z.object({{ {} }})", fields.join(", "));
        // A `#[serde(flatten)]` map beside named fields; a plain `z.object` would strip its keys.
        // The catchall is `z.unknown()`, not the map's value schema: a named field that doesn't fit
        // that schema would give a type no value satisfies. The extension checks the values.
        Ok(match schema.get("additionalProperties") {
            None | Some(Value::Bool(false)) => written,
            Some(_) => format!("{written}.catchall(z.unknown())"),
        })
    }
}

/// `nullable: true`, OpenAPI 3.0's form of a nullable value.
fn nullable(schema: &Map<String, Value>, written: String) -> Result<String, String> {
    Ok(if schema.get("nullable") == Some(&Value::Bool(true)) {
        format!("{written}.nullable()")
    } else {
        written
    })
}

/// The schema in `defs` that `reference` names.
fn definition<'a>(defs: &'a Map<String, Value>, reference: &str) -> Result<&'a Value, String> {
    let name = reference
        .strip_prefix("#/$defs/")
        .or_else(|| reference.strip_prefix("#/components/schemas/"))
        .ok_or_else(|| format!("refers to `{reference}`, outside its `$defs`"))?;
    defs.get(name)
        .ok_or_else(|| format!("refers to `{name}`, which its `$defs` lacks"))
}

/// The object schema of a struct. utoipa writes a struct that flattens another as an `allOf` of
/// the structs, whose fields this merges into one object.
fn struct_fields(
    schema: &Map<String, Value>,
    defs: &Map<String, Value>,
    depth: usize,
) -> Result<Map<String, Value>, String> {
    if depth > 32 {
        return Err("refers to itself, which a builder can't express".to_string());
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return match definition(defs, reference)? {
            Value::Object(target) => struct_fields(target, defs, depth + 1),
            other => Err(format!("has `{other}` where a schema belongs")),
        };
    }
    let Some(parts) = schema.get("allOf").and_then(Value::as_array) else {
        return Ok(schema.clone());
    };
    let not_structs =
        || "combines schemas with `allOf` that are not all structs, which a builder can't express";
    let mut properties = Map::new();
    let mut required = Vec::new();
    let mut open = false;
    for part in parts {
        let part = struct_fields(part.as_object().ok_or_else(not_structs)?, defs, depth + 1)?;
        let Some(Value::Object(fields)) = part.get("properties") else {
            return Err(not_structs().to_string());
        };
        properties.extend(fields.clone());
        if let Some(Value::Array(names)) = part.get("required") {
            required.extend(names.iter().cloned());
        }
        open |= part
            .get("additionalProperties")
            .is_some_and(|values| *values != Value::Bool(false));
    }
    let mut merged = Map::new();
    merged.insert("type".to_string(), Value::from("object"));
    merged.insert("properties".to_string(), Value::Object(properties));
    merged.insert("required".to_string(), Value::Array(required));
    if open {
        merged.insert("additionalProperties".to_string(), Value::Bool(true));
    }
    Ok(merged)
}

fn union(mut written: Vec<String>) -> String {
    match written.len() {
        1 => written.remove(0),
        _ => format!("z.union([{}])", written.join(", ")),
    }
}

/// `schema`'s properties by name. serde_json keeps a map's order only with its `preserve_order`
/// feature, which a workspace build turns on and a single crate's may not, so the generated code
/// sorts them to come out the same in every build.
fn sorted_properties(schema: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut properties: Vec<_> = schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));
    properties
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_config_field_named_like_the_builder_s_own_state() {
        let error = builder(
            "acme/zone",
            &descriptor(
                json!({ "title": "Zone", "type": "object", "required": ["id"], "properties": { "id": { "type": "string" }, "_config": { "type": "string" } } }),
                json!({ "title": "ZoneOutputs", "type": "object", "properties": {} }),
            ),
            &mut HashMap::new(),
        )
        .unwrap_err();
        assert!(error.contains("`_config`"), "{error}");
    }
    use alien_core::ResourceType;
    use serde_json::json;

    fn zod(schema: Value) -> Result<String, String> {
        Writer {
            defs: schema
                .get("$defs")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            depth: 0,
        }
        .zod(&schema)
    }

    /// A map of `serde_json::Value`, alone or under a field, takes any JSON value.
    #[test]
    fn a_map_of_any_json_value_takes_unknown_values() {
        assert_eq!(
            zod(json!({ "type": "object", "additionalProperties": {} })).unwrap(),
            "z.record(z.string(), z.unknown())"
        );
        assert_eq!(
            zod(json!({
                "type": "object",
                "required": ["settings"],
                "properties": {
                    "settings": { "type": "object", "additionalProperties": {}, "description": "Any JSON" }
                }
            }))
            .unwrap(),
            r#"z.object({ "settings": z.record(z.string(), z.unknown()) })"#
        );
    }

    /// A tuple, which utoipa writes with one schema per position in `prefixItems`, is a tuple.
    #[test]
    fn a_tuple_is_written_as_a_tuple() {
        assert_eq!(
            zod(json!({
                "type": "array",
                "items": false,
                "prefixItems": [
                    { "type": "string" },
                    { "type": "integer", "format": "int32", "minimum": 0 }
                ]
            }))
            .unwrap(),
            "z.tuple([z.string(), z.number().int().min(0)])"
        );
    }

    #[test]
    fn writes_what_utoipa_emits_for_rust_types() {
        assert_eq!(
            zod(json!({
                "type": "object",
                "required": ["id", "monthlyLimitUsd", "tier"],
                "properties": {
                    "id": { "type": "string" },
                    "monthlyLimitUsd": { "type": "integer", "format": "int32", "minimum": 0 },
                    "note": { "type": ["string", "null"] },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "object", "additionalProperties": { "type": "string" } },
                    "tier": { "$ref": "#/$defs/Tier" }
                },
                "$defs": { "Tier": { "type": "string", "enum": ["small", "large"] } }
            }))
            .unwrap(),
            r#"z.object({ "id": z.string(), "labels": z.record(z.string(), z.string()).optional(), "monthlyLimitUsd": z.number().int().min(0), "note": z.string().nullable().optional(), "tags": z.array(z.string()).optional(), "tier": z.enum(["small","large"]) })"#
        );
    }

    #[test]
    fn writes_unions_and_nullable_references() {
        assert_eq!(
            zod(json!({ "oneOf": [{ "type": "null" }, { "$ref": "#/$defs/A" }], "$defs": { "A": { "type": "boolean" } } }))
                .unwrap(),
            "z.boolean().nullable()"
        );
        assert_eq!(
            zod(json!({ "anyOf": [{ "type": "string" }, { "type": "number" }] })).unwrap(),
            "z.union([z.string(), z.number()])"
        );
    }

    #[test]
    fn refuses_what_a_builder_cant_express() {
        assert!(zod(json!({ "$ref": "#/$defs/Missing" }))
            .unwrap_err()
            .contains("`Missing`"));
        assert!(
            zod(json!({ "allOf": [{ "type": "string" }, { "type": "number" }] }))
                .unwrap_err()
                .contains("allOf")
        );
        assert!(zod(json!({ "type": "file" }))
            .unwrap_err()
            .contains("`file`"));
        let looping = json!({ "$ref": "#/$defs/A", "$defs": { "A": { "$ref": "#/$defs/A" } } });
        assert!(zod(looping).unwrap_err().contains("refers to itself"));
    }

    fn descriptor(config: Value, outputs: Value) -> ResourceTypeDescriptor {
        serde_json::from_value(json!({
            "lifecycles": ["frozen", "live"],
            "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": [],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": config, "outputs": outputs }
        }))
        .unwrap()
    }

    #[test]
    fn writes_a_builder_named_after_the_rust_type() {
        let written = builder(
            "acme/dns-zone",
            &descriptor(
                json!({
                    "title": "DnsZone",
                    "description": "A DNS zone the deployment owns.",
                    "type": "object",
                    "required": ["id", "domain"],
                    "properties": {
                        "id": { "type": "string" },
                        "domain": { "type": "string", "description": "The zone's domain." }
                    }
                }),
                json!({ "title": "DnsZoneOutputs", "type": "object", "required": ["zoneId"], "properties": { "zoneId": { "type": "string" } } }),
            ),
            &mut HashMap::new(),
        )
        .unwrap();
        for expected in [
            r#"export const DnsZoneConfigSchema = z.object({ "domain": z.string(), "id": z.string() })"#,
            r#"export const DnsZoneOutputsSchema = z.object({ "zoneId": z.string() })"#,
            "/** A DNS zone the deployment owns. */\nexport class DnsZone extends CustomResourceType {",
            "  /** The zone's domain. */\n  public domain(value: DnsZoneConfig[\"domain\"]): this {\n    this._config.domain = value",
            r#"return this.resource({ type: "acme/dns-zone", ...config })"#,
        ] {
            assert!(written.contains(expected), "{expected}\n{written}");
        }
        assert!(!written.contains("public id("), "{written}");
    }

    /// utoipa writes a struct with a `#[serde(flatten)]` struct as an `allOf` of the two; the
    /// outputs schema here is the SDK's for such a type.
    #[test]
    fn writes_a_struct_that_flattens_another_as_one_object() {
        let location = json!({ "ZoneLocation": { "type": "object", "required": ["region", "accountId"], "properties": { "accountId": { "type": "string" }, "region": { "type": "string" } } } });
        let written = builder(
            "probe/flat-zone",
            &descriptor(
                json!({
                    "allOf": [
                        { "$ref": "#/$defs/ZoneLocation" },
                        { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }
                    ],
                    "title": "FlatZone",
                    "$defs": location
                }),
                json!({
                    "allOf": [
                        { "$ref": "#/$defs/ZoneLocation" },
                        { "type": "object", "required": ["zoneId"], "properties": { "zoneId": { "type": "string" } } }
                    ],
                    "title": "FlatZoneOutputs",
                    "$defs": location
                }),
            ),
            &mut HashMap::new(),
        )
        .unwrap();
        for expected in [
            r#"export const FlatZoneConfigSchema = z.object({ "accountId": z.string(), "id": z.string(), "region": z.string() })"#,
            r#"export const FlatZoneOutputsSchema = z.object({ "accountId": z.string(), "region": z.string(), "zoneId": z.string() })"#,
            "  public region(value: FlatZoneConfig[\"region\"]): this {\n    this._config.region = value",
        ] {
            assert!(written.contains(expected), "{expected}\n{written}");
        }
    }

    /// The schemas are the SDK's for `#[serde(flatten)]` maps beside named fields: of booleans,
    /// of any JSON value, under a config field, and beside a flattened struct. The booleans
    /// don't fit `id`, so the catchall can't be the map's value schema.
    #[test]
    fn keeps_a_map_flattened_beside_named_fields() {
        let named = json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } }, "additionalProperties": { "type": "boolean" } });
        assert_eq!(
            zod(named.clone()).unwrap(),
            r#"z.object({ "id": z.string() }).catchall(z.unknown())"#
        );
        assert_eq!(
            zod(json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } }, "additionalProperties": {} })).unwrap(),
            r#"z.object({ "id": z.string() }).catchall(z.unknown())"#
        );
        assert_eq!(
            zod(json!({ "type": "object", "required": ["settings"], "properties": { "settings": { "$ref": "#/$defs/Named" } }, "$defs": { "Named": named } })).unwrap(),
            r#"z.object({ "settings": z.object({ "id": z.string() }).catchall(z.unknown()) })"#
        );
        let written = builder(
            "acme/both",
            &descriptor(
                json!({
                    "allOf": [
                        { "$ref": "#/$defs/Inner" },
                        { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } }, "additionalProperties": { "type": "integer", "format": "int64" } }
                    ],
                    "title": "Both",
                    "$defs": { "Inner": { "type": "object", "required": ["region"], "properties": { "region": { "type": "string" } } } }
                }),
                json!({ "type": "object", "properties": {} }),
            ),
            &mut HashMap::new(),
        )
        .unwrap();
        let expected = r#"export const BothConfigSchema = z.object({ "id": z.string(), "region": z.string() }).catchall(z.unknown())"#;
        assert!(written.contains(expected), "{expected}\n{written}");
        // The map's entries have no setter of their own, so `set` writes them, and refuses a key
        // that names a field.
        assert!(
            written.contains("public set(key: string, value: unknown): this")
                && written.contains(r#"["type","id","region"].includes(key)"#),
            "{written}"
        );
        let closed = json!({ "type": "object", "properties": {}, "additionalProperties": false });
        let beside_closed = json!({ "allOf": [closed.clone(), named.clone()] });
        assert_eq!(
            struct_fields(beside_closed.as_object().unwrap(), &Map::new(), 0).unwrap()
                ["additionalProperties"],
            json!(true)
        );
        let only_closed = json!({ "allOf": [closed, { "type": "object", "properties": {} }] });
        assert!(
            !struct_fields(only_closed.as_object().unwrap(), &Map::new(), 0)
                .unwrap()
                .contains_key("additionalProperties")
        );
    }

    /// A config without a flattened map gets no `set`, and one whose field is named `set` beside a
    /// map is refused, since the two would share the name.
    #[test]
    fn set_comes_only_with_a_flattened_map() {
        let closed = builder(
            "acme/thing",
            &descriptor(
                json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }),
                json!({ "type": "object", "properties": {} }),
            ),
            &mut HashMap::new(),
        )
        .unwrap();
        assert!(!closed.contains("public set("), "{closed}");
        let error = builder(
            "acme/thing",
            &descriptor(
                json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" }, "set": { "type": "string" } }, "additionalProperties": {} }),
                json!({ "type": "object", "properties": {} }),
            ),
            &mut HashMap::new(),
        )
        .unwrap_err();
        assert!(error.contains("named `set`"), "{error}");
    }

    #[test]
    fn refuses_a_field_named_like_a_builder_method() {
        let error = builder(
            "acme/thing",
            &descriptor(
                json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" }, "enabled": { "type": "boolean" } } }),
                json!({ "type": "object", "properties": {} }),
            ),
            &mut HashMap::new(),
        )
        .unwrap_err();
        assert!(error.contains("`enabled`"), "{error}");
    }

    fn manifest(types: Vec<(&str, ResourceTypeDescriptor)>) -> ExtensionManifest {
        ExtensionManifest {
            name: "acme".to_string(),
            version: "0.1.0".to_string(),
            digest: "sha256:4ff9".to_string(),
            resource_types: types
                .into_iter()
                .map(|(name, descriptor)| (ResourceType::from(name), descriptor))
                .collect(),
            egress: Vec::new(),
        }
    }

    #[test]
    fn refuses_a_name_already_taken_in_the_module() {
        let titled = |config: &str, outputs: &str| {
            descriptor(
                json!({ "title": config, "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }),
                json!({ "title": outputs, "type": "object", "properties": {} }),
            )
        };
        assert_eq!(
            index_ts(&manifest(vec![(
                "acme/resource",
                titled("Resource", "ResourceOutputs")
            )]))
            .unwrap_err(),
            "`acme/resource` would export `Resource`, a name the generated module already uses; rename its Rust type"
        );
        assert_eq!(
            index_ts(&manifest(vec![
                ("acme/zone", titled("Zone", "ZoneOutputs")),
                ("acme/other-zone", titled("Zone", "OtherZoneOutputs")),
            ]))
            .unwrap_err(),
            "`acme/other-zone` would export `ZoneConfigSchema`, which `acme/zone` already exports; rename one of their Rust types"
        );
    }
}
