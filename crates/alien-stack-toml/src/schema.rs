//! The generated JSON schemas of alien-core's types, as `schema/canonical.json` holds them.

use std::sync::OnceLock;

use serde_json::{Map, Value};

static CANONICAL: &str = include_str!("../schema/canonical.json");

/// A schema, with the name of the component it was reached through.
#[derive(Clone, Copy)]
pub struct Schema<'a> {
    pub value: &'a Value,
    /// Set when the schema is a named component, such as `ResourceRef`.
    pub name: Option<&'a str>,
}

fn components() -> &'static Map<String, Value> {
    static COMPONENTS: OnceLock<Map<String, Value>> = OnceLock::new();
    COMPONENTS.get_or_init(|| {
        let document: Value =
            serde_json::from_str(CANONICAL).expect("schema/canonical.json is valid JSON");
        match document {
            Value::Object(mut document) => match document.remove("components") {
                Some(Value::Object(components)) => components,
                _ => panic!("schema/canonical.json has no `components` object"),
            },
            _ => panic!("schema/canonical.json is not an object"),
        }
    })
}

/// The component schema with this name.
pub fn component(name: &str) -> Schema<'static> {
    let (name, value) = components()
        .get_key_value(name)
        .unwrap_or_else(|| panic!("schema/canonical.json has no component '{name}'"));
    Schema {
        value,
        name: Some(name),
    }
}

impl<'a> Schema<'a> {
    pub fn inline(value: &'a Value) -> Self {
        Schema { value, name: None }
    }

    /// Follows `$ref` (and a lone `allOf`) to the schema that holds the content.
    pub fn resolved(self) -> Schema<'a> {
        let mut schema = self;
        loop {
            if let Some(target) = schema.value.get("$ref").and_then(Value::as_str) {
                schema = component(target.rsplit('/').next().unwrap_or(target));
                continue;
            }
            match schema.value.get("allOf").and_then(Value::as_array) {
                Some(parts) if parts.len() == 1 => schema = Schema::inline(&parts[0]),
                _ => return schema,
            }
        }
    }

    pub fn properties(&self) -> Option<&'a Map<String, Value>> {
        self.value.get("properties").and_then(Value::as_object)
    }

    pub fn property(&self, name: &str) -> Option<Schema<'a>> {
        self.properties()?.get(name).map(Schema::inline)
    }

    pub fn required(&self) -> impl Iterator<Item = &'a str> {
        self.value
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }

    pub fn one_of(&self) -> Option<Vec<Schema<'a>>> {
        let branches = self.value.get("oneOf")?.as_array()?;
        Some(branches.iter().map(Schema::inline).collect())
    }

    pub fn items(&self) -> Option<Schema<'a>> {
        self.value.get("items").map(Schema::inline)
    }

    /// The schema of a map's values, when the object is a map.
    pub fn additional(&self) -> Option<Schema<'a>> {
        match self.value.get("additionalProperties") {
            Some(value @ Value::Object(_)) => Some(Schema::inline(value)),
            _ => None,
        }
    }

    pub fn enum_values(&self) -> Option<Vec<&'a str>> {
        let values = self.value.get("enum")?.as_array()?;
        Some(values.iter().filter_map(Value::as_str).collect())
    }

    /// The JSON types the schema accepts, without `null`.
    pub fn types(&self) -> Vec<&'a str> {
        match self.value.get("type") {
            Some(Value::String(kind)) => vec![kind.as_str()],
            Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
        .into_iter()
        .filter(|kind| *kind != "null")
        .collect()
    }

    pub fn is_null(&self) -> bool {
        self.value.get("type").and_then(Value::as_str) == Some("null")
    }

    /// The single value of an internally tagged enum branch's `type` property.
    pub fn tag(&self) -> Option<&'a str> {
        let values = self.property("type")?.enum_values()?;
        match values.as_slice() {
            [tag] => Some(tag),
            _ => None,
        }
    }
}

pub fn kebab_to_camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for c in key.chars() {
        match c {
            '-' => upper_next = true,
            c if upper_next => {
                out.extend(c.to_uppercase());
                upper_next = false;
            }
            c => out.push(c),
        }
    }
    out
}

pub fn camel_to_kebab(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_convert_between_kebab_and_camel_case() {
        assert_eq!(kebab_to_camel("stop-grace-period"), "stopGracePeriod");
        assert_eq!(kebab_to_camel("cpu"), "cpu");
        assert_eq!(camel_to_kebab("stopGracePeriodSeconds"), "stop-grace-period-seconds");
        assert_eq!(camel_to_kebab("x86_64"), "x86_64");
    }

    #[test]
    fn components_resolve_references() {
        let daemon = component("Daemon");
        let code = daemon.property("code").expect("daemon has code").resolved();
        assert_eq!(code.name, Some("DaemonCode"));
        assert!(code.one_of().is_some());
    }
}
