//! Reading a table of `alien.toml` as an alien-core type, driven by the type's JSON schema.
//!
//! There is no code per field. The walk knows the TOML spelling of a name (kebab-case, and
//! the unit left out of `...Seconds`), what a schema says about a value, and a few forms
//! that alien-core's types share:
//!
//! - a resource ID written where a `ResourceRef` is expected,
//! - a range written `{ default = 2, allowed = [1, 4] }` where a type has `min`, `max`
//!   and `default`,
//! - durations and sizes as strings,
//! - internally tagged enums picked by their `type`.
//!
//! Every value of the output remembers the span it came from, so an error reported later
//! (for example by serde) points at the author's text.

use std::collections::HashMap;
use std::ops::Range;

use indexmap::IndexMap;
use serde_json::{Map, Number, Value as Json};

use crate::node::{Entry, Node, Table, Value};
use crate::schema::{camel_to_kebab, kebab_to_camel, Schema};
use crate::suggest;
use crate::units::{UnitSuffix, UNIT_SUFFIXES};

/// A problem with the document and where it is.
#[derive(Debug, Clone)]
pub struct Failure {
    pub span: Range<usize>,
    pub message: String,
    depth: usize,
}

pub type Walk<T> = Result<T, Failure>;

/// Where each part of the output came from: the value, and for a table entry, its key.
#[derive(Debug, Clone, Default)]
pub struct SpanMap {
    values: HashMap<Vec<String>, Range<usize>>,
    keys: HashMap<Vec<String>, Range<usize>>,
}

impl SpanMap {
    /// The span of the part of the output at `path`, or of its closest ancestor that has one.
    ///
    /// `key` asks for the key of the entry rather than its value, which is where an unknown
    /// field is.
    pub fn locate(&self, path: &[String], key: bool) -> Option<Range<usize>> {
        if key {
            if let Some(span) = self.keys.get(path) {
                return Some(span.clone());
            }
        }
        (0..=path.len())
            .rev()
            .find_map(|len| self.values.get(&path[..len]))
            .cloned()
    }

    /// The span of the value at exactly `path`.
    pub fn exact(&self, path: &[String]) -> Option<Range<usize>> {
        self.values.get(path).cloned()
    }

    pub fn insert_value(&mut self, path: &[String], span: Range<usize>) {
        self.values.insert(path.to_vec(), span);
    }

    pub fn insert_key(&mut self, path: &[String], span: Range<usize>) {
        self.keys.insert(path.to_vec(), span);
    }
}

/// What the author may write at the top of a resource besides the schema's own fields.
#[derive(Debug, Default)]
pub struct Vocabulary {
    /// Fields of the schema the author writes another way (`code` is `image` or `build`).
    pub hidden: Vec<&'static str>,
    /// Keys that are not fields of the schema.
    pub extra: Vec<&'static str>,
}

/// The IDs the document declares, with their resource types.
pub type Declared = IndexMap<String, String>;

pub struct Walker<'a> {
    declared: &'a Declared,
    pub spans: SpanMap,
    path: Vec<String>,
}

impl<'a> Walker<'a> {
    pub fn new(declared: &'a Declared) -> Self {
        Walker {
            declared,
            spans: SpanMap::default(),
            path: Vec::new(),
        }
    }

    /// Reads `node` as `schema`. The output's spans are recorded under `base`.
    pub fn walk_at(
        &mut self,
        base: &[String],
        node: &Node,
        schema: Schema<'_>,
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<Json> {
        self.path = base.to_vec();
        self.walk(node, schema, vocabulary)
    }

    fn fail<T>(&self, span: Range<usize>, message: impl Into<String>) -> Walk<T> {
        Err(Failure {
            span,
            message: message.into(),
            depth: self.path.len(),
        })
    }

    fn at<T>(&mut self, segment: &str, f: impl FnOnce(&mut Self) -> Walk<T>) -> Walk<T> {
        self.path.push(segment.to_string());
        let result = f(self);
        self.path.pop();
        result
    }

    fn walk(
        &mut self,
        node: &Node,
        schema: Schema<'_>,
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<Json> {
        self.spans.insert_value(&self.path, node.span.clone());
        let schema = schema.resolved();
        if schema.name == Some("ResourceRef") {
            return self.resource_ref(node);
        }
        if let Some(branches) = schema.one_of() {
            return self.one_of(node, &branches, vocabulary);
        }
        if schema.properties().is_some() {
            return self.object(node, schema, vocabulary);
        }
        if let Some(values) = schema.enum_values() {
            return self.enumeration(node, &values);
        }
        let types = schema.types();
        if types.contains(&"object") {
            return match schema.additional() {
                Some(values) => self.map(node, values),
                None => self.generic(node),
            };
        }
        if types.contains(&"array") {
            return self.array(node, schema);
        }
        if types.is_empty() {
            return self.generic(node);
        }
        self.scalar(node, &types)
    }

    /// A value no schema describes, kept as it is written.
    fn generic(&self, node: &Node) -> Walk<Json> {
        match &node.value {
            Value::String(text) => Ok(Json::String(text.clone())),
            Value::Boolean(flag) => Ok(Json::Bool(*flag)),
            Value::Integer(number) => Ok(Json::Number(Number::from(*number))),
            Value::Float(number) => match float_number(*number) {
                Some(number) => Ok(Json::Number(number)),
                None => self.fail(node.span.clone(), "number is out of range"),
            },
            Value::Datetime => self.fail(node.span.clone(), "dates are not supported here"),
            Value::Array(items) => items
                .iter()
                .map(|item| self.generic(item))
                .collect::<Walk<Vec<_>>>()
                .map(Json::Array),
            Value::Table(table) => {
                let mut out = Map::new();
                for entry in &table.entries {
                    out.insert(entry.key.clone(), self.generic(&entry.value)?);
                }
                Ok(Json::Object(out))
            }
        }
    }

    fn resource_ref(&mut self, node: &Node) -> Walk<Json> {
        let id = match &node.value {
            Value::String(id) => id.as_str(),
            Value::Table(table) => match table.get("id").and_then(Node::as_str) {
                Some(id) if table.entries.len() == 1 => id,
                _ => {
                    return self.fail(
                        node.span.clone(),
                        "write the ID of a resource, such as \"data\"",
                    )
                }
            },
            _ => {
                return self.fail(
                    node.span.clone(),
                    format!("expected the ID of a resource, found {}", node.kind()),
                )
            }
        };
        match self.declared.get(id) {
            Some(resource_type) => Ok(serde_json::json!({ "id": id, "type": resource_type })),
            None => self.fail(
                node.span.clone(),
                unknown_resource(id, self.declared.keys().map(String::as_str)),
            ),
        }
    }

    fn one_of(
        &mut self,
        node: &Node,
        branches: &[Schema<'_>],
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<Json> {
        let candidates: Vec<Schema<'_>> = branches
            .iter()
            .copied()
            .filter(|branch| !branch.is_null())
            .collect();
        let tags: Vec<&str> = candidates.iter().filter_map(Schema::tag).collect();
        if !candidates.is_empty() && tags.len() == candidates.len() {
            return self.tagged(node, &candidates, &tags, vocabulary);
        }

        let compatible: Vec<Schema<'_>> = candidates
            .iter()
            .copied()
            .filter(|candidate| accepts(node, *candidate))
            .collect();
        match compatible.as_slice() {
            [] => {
                let mut expected: Vec<&str> = Vec::new();
                for word in candidates.iter().flat_map(|c| describe(*c)) {
                    if !expected.contains(&word) {
                        expected.push(word);
                    }
                }
                self.fail(
                    node.span.clone(),
                    format!("expected {}, found {}", expected.join(" or "), node.kind()),
                )
            }
            [only] => self.walk(node, *only, vocabulary),
            several => {
                let mut deepest: Option<Failure> = None;
                for candidate in several {
                    let spans = self.spans.clone();
                    match self.walk(node, *candidate, vocabulary) {
                        Ok(value) => return Ok(value),
                        Err(failure) => {
                            self.spans = spans;
                            if deepest
                                .as_ref()
                                .is_none_or(|best| failure.depth > best.depth)
                            {
                                deepest = Some(failure);
                            }
                        }
                    }
                }
                Err(deepest.expect("at least two candidates were tried"))
            }
        }
    }

    fn tagged(
        &mut self,
        node: &Node,
        candidates: &[Schema<'_>],
        tags: &[&str],
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<Json> {
        let Value::Table(table) = &node.value else {
            return self.fail(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            );
        };
        let names: Vec<String> = tags.iter().map(|tag| camel_to_kebab(tag)).collect();
        let list = names.join(", ");
        let Some(tag) = table.get("type") else {
            return self.fail(
                node.span.clone(),
                format!("missing `type`; expected one of: {list}"),
            );
        };
        let Some(text) = tag.as_str() else {
            return self.fail(
                tag.span.clone(),
                format!("`type` must be a string, one of: {list}"),
            );
        };
        match tags
            .iter()
            .position(|candidate| *candidate == text || camel_to_kebab(candidate) == text)
        {
            Some(index) => self.walk(node, candidates[index], vocabulary),
            None => self.fail(
                tag.span.clone(),
                format!(
                    "unknown type '{text}'{}; expected one of: {list}",
                    suggest::hint(text, names.iter().map(String::as_str))
                ),
            ),
        }
    }

    fn enumeration(&mut self, node: &Node, values: &[&str]) -> Walk<Json> {
        let kebab: Vec<String> = values.iter().map(|value| camel_to_kebab(value)).collect();
        let Some(text) = node.as_str() else {
            return self.fail(
                node.span.clone(),
                format!(
                    "expected one of: {}; found {}",
                    kebab.join(", "),
                    node.kind()
                ),
            );
        };
        match values
            .iter()
            .position(|value| *value == text || camel_to_kebab(value) == text)
        {
            Some(index) => Ok(Json::String(values[index].to_string())),
            None => self.fail(
                node.span.clone(),
                format!(
                    "unknown value '{text}'{}; expected one of: {}",
                    suggest::hint(text, kebab.iter().map(String::as_str)),
                    kebab.join(", ")
                ),
            ),
        }
    }

    fn array(&mut self, node: &Node, schema: Schema<'_>) -> Walk<Json> {
        let Value::Array(items) = &node.value else {
            return self.fail(
                node.span.clone(),
                format!("expected a list, found {}", node.kind()),
            );
        };
        let mut out = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let value = self.at(&index.to_string(), |walker| match schema.items() {
                Some(items) => walker.walk(item, items, None),
                None => walker.generic(item),
            })?;
            out.push(value);
        }
        Ok(Json::Array(out))
    }

    fn map(&mut self, node: &Node, values: Schema<'_>) -> Walk<Json> {
        let Value::Table(table) = &node.value else {
            return self.fail(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            );
        };
        let mut out = Map::new();
        for entry in &table.entries {
            let value = self.at(&entry.key, |walker| {
                walker
                    .spans
                    .insert_key(&walker.path, entry.key_span.clone());
                walker.walk(&entry.value, values, None)
            })?;
            out.insert(entry.key.clone(), value);
        }
        Ok(Json::Object(out))
    }

    fn scalar(&mut self, node: &Node, types: &[&str]) -> Walk<Json> {
        let expected = || {
            let words: Vec<&str> = types
                .iter()
                .map(|kind| match *kind {
                    "string" => "a string",
                    "integer" => "a whole number",
                    "number" => "a number",
                    "boolean" => "true or false",
                    other => other,
                })
                .collect();
            words.join(" or ")
        };
        match &node.value {
            Value::String(text) if types.contains(&"string") => Ok(Json::String(text.clone())),
            Value::Boolean(flag) if types.contains(&"boolean") => Ok(Json::Bool(*flag)),
            Value::Integer(number) if types.contains(&"integer") || types.contains(&"number") => {
                Ok(Json::Number(Number::from(*number)))
            }
            Value::Float(number) if types.contains(&"number") => match float_number(*number) {
                Some(number) => Ok(Json::Number(number)),
                None => self.fail(node.span.clone(), "number is out of range"),
            },
            Value::Float(_) if types.contains(&"integer") => {
                self.fail(node.span.clone(), "expected a whole number")
            }
            _ => self.fail(
                node.span.clone(),
                format!("expected {}, found {}", expected(), node.kind()),
            ),
        }
    }

    fn object(
        &mut self,
        node: &Node,
        schema: Schema<'_>,
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<Json> {
        let Value::Table(table) = &node.value else {
            return self.fail(
                node.span.clone(),
                format!("expected a table, found {}", node.kind()),
            );
        };
        let properties = schema.properties().expect("an object has properties");
        if properties.len() == 3
            && ["min", "max", "default"]
                .iter()
                .all(|name| properties.contains_key(*name))
        {
            return self.range(node, table, schema);
        }

        let mut out = Map::new();
        for entry in &table.entries {
            let (property, unit) = self.property_of(entry, schema, vocabulary)?;
            let property_schema = schema.property(&property).expect("property was just found");
            let value = self.at(&property, |walker| {
                walker
                    .spans
                    .insert_key(&walker.path, entry.key_span.clone());
                match unit {
                    Some(unit) => {
                        walker
                            .spans
                            .insert_value(&walker.path, entry.value.span.clone());
                        walker.unit_value(&entry.value, unit)
                    }
                    None => walker.walk(&entry.value, property_schema, None),
                }
            })?;
            out.insert(property, value);
        }

        for required in schema.required() {
            if out.contains_key(required) {
                continue;
            }
            let field = schema.property(required).map(Schema::resolved);
            if let Some(field) = field {
                if field.types().contains(&"array") && field.properties().is_none() {
                    out.insert(required.to_string(), Json::Array(Vec::new()));
                } else if field.types().contains(&"object")
                    && field.properties().is_none()
                    && field.additional().is_some()
                {
                    out.insert(required.to_string(), Json::Object(Map::new()));
                }
            }
        }
        Ok(Json::Object(out))
    }

    /// The schema property a key of the table stands for, and the unit the key left out.
    fn property_of(
        &self,
        entry: &Entry,
        schema: Schema<'_>,
        vocabulary: Option<&Vocabulary>,
    ) -> Walk<(String, Option<&'static UnitSuffix>)> {
        let properties = schema.properties().expect("an object has properties");
        let key = entry.key.as_str();
        let camel = kebab_to_camel(key);

        // The alien-core spelling of a name, which sugar and the editor schema hide.
        let written_canonically = camel_to_kebab(key) != key && properties.contains_key(key);
        if written_canonically || (camel != key && properties.contains_key(&camel)) {
            let property = if written_canonically {
                key
            } else {
                camel.as_str()
            };
            if let Some((stem, _)) = split_unit(property) {
                return self.fail(
                    entry.key_span.clone(),
                    format!(
                        "write `{}`: the unit goes in the value, such as \"30s\" or \"512Mi\"",
                        camel_to_kebab(stem)
                    ),
                );
            }
            if written_canonically {
                return self.fail(
                    entry.key_span.clone(),
                    format!("keys are kebab-case: write `{}`", camel_to_kebab(key)),
                );
            }
        }
        if properties.contains_key(&camel) {
            return Ok((camel, None));
        }
        if let Some(unit) = UNIT_SUFFIXES
            .iter()
            .find(|unit| properties.contains_key(&format!("{camel}{}", unit.suffix)))
        {
            return Ok((format!("{camel}{}", unit.suffix), Some(unit)));
        }

        let allowed = allowed_keys(properties.keys().map(String::as_str), vocabulary);
        self.fail(
            entry.key_span.clone(),
            format!(
                "unknown key `{key}`{}; expected one of: {}",
                suggest::hint(key, allowed.iter().map(String::as_str)),
                allowed.join(", ")
            ),
        )
    }

    fn unit_value(&mut self, node: &Node, unit: &UnitSuffix) -> Walk<Json> {
        match &node.value {
            Value::Integer(number) => Ok(Json::Number(Number::from(*number))),
            Value::String(text) => match unit.parse_text(text) {
                Ok(number) => Ok(Json::Number(number)),
                Err(reason) => self.fail(node.span.clone(), reason),
            },
            _ => self.fail(
                node.span.clone(),
                format!("expected {}, found {}", unit.expects(), node.kind()),
            ),
        }
    }

    /// `{ default = 2, allowed = [1, 4] }` for a type with `min`, `max` and `default`.
    fn range(&mut self, node: &Node, table: &Table, schema: Schema<'_>) -> Walk<Json> {
        for entry in &table.entries {
            match entry.key.as_str() {
                "default" | "allowed" => {}
                "min" | "max" => {
                    return self.fail(
                        entry.key_span.clone(),
                        "write the range as `allowed = [min, max]` next to `default`",
                    )
                }
                other => {
                    return self.fail(
                        entry.key_span.clone(),
                        format!(
                            "unknown key `{other}`{}; a range is `{{ default = <n>, allowed = [<min>, <max>] }}`",
                            suggest::hint(other, ["default", "allowed"])
                        ),
                    )
                }
            }
        }
        let Some(default) = table.entry("default") else {
            return self.fail(
                node.span.clone(),
                "a range needs `default`, the value used when none is chosen",
            );
        };
        let Some(allowed) = table.entry("allowed") else {
            return self.fail(
                node.span.clone(),
                "a range needs `allowed = [min, max]`, the values that may be chosen",
            );
        };
        let bounds = match allowed.value.as_array() {
            Some([min, max]) => [min, max],
            _ => {
                return self.fail(
                    allowed.value.span.clone(),
                    "`allowed` is a list of two values: [min, max]",
                )
            }
        };
        let mut out = Map::new();
        for (name, value, key_span) in [
            ("min", bounds[0], allowed.key_span.clone()),
            ("max", bounds[1], allowed.key_span.clone()),
            ("default", &default.value, default.key_span.clone()),
        ] {
            let field = schema
                .property(name)
                .expect("a range has min, max and default");
            let walked = self.at(name, |walker| {
                walker.spans.insert_key(&walker.path, key_span);
                walker.walk(value, field, None)
            })?;
            out.insert(name.to_string(), walked);
        }
        Ok(Json::Object(out))
    }
}

/// The key the author writes for each property of a schema: unit left out, `id` and `type`
/// and what the vocabulary hides removed, what it adds put on.
pub fn allowed_keys<'a>(
    properties: impl Iterator<Item = &'a str>,
    vocabulary: Option<&Vocabulary>,
) -> Vec<String> {
    let mut keys: Vec<String> = properties
        .filter(|property| !matches!(*property, "id" | "type"))
        .map(|property| match split_unit(property) {
            Some((stem, _)) => camel_to_kebab(stem),
            None => camel_to_kebab(property),
        })
        .filter(|key| {
            vocabulary.is_none_or(|vocabulary| !vocabulary.hidden.contains(&key.as_str()))
        })
        .collect();
    if let Some(vocabulary) = vocabulary {
        keys.extend(vocabulary.extra.iter().map(|key| key.to_string()));
    }
    keys.sort();
    keys.dedup();
    keys
}

/// How the author writes a property of the schema: kebab-case, the unit left out.
pub fn author_key(property: &str) -> String {
    match split_unit(property) {
        Some((stem, _)) => camel_to_kebab(stem),
        None => camel_to_kebab(property),
    }
}

/// A property name that ends in a unit, as the name without it and the unit.
pub fn split_unit(property: &str) -> Option<(&str, &'static UnitSuffix)> {
    UNIT_SUFFIXES.iter().find_map(|unit| {
        let stem = property.strip_suffix(unit.suffix)?;
        stem.chars()
            .last()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            .then_some((stem, unit))
    })
}

fn float_number(number: f64) -> Option<Number> {
    if number.is_finite() && number.fract() == 0.0 && number.abs() < 9e15 {
        return Some(Number::from(number as i64));
    }
    Number::from_f64(number)
}

/// What a value is, in the words of "expected ..., found ...".
fn describe(schema: Schema<'_>) -> Vec<&'static str> {
    let schema = schema.resolved();
    if let Some(branches) = schema.one_of() {
        return branches.into_iter().flat_map(describe).collect();
    }
    if schema.properties().is_some() || schema.types().contains(&"object") {
        return vec!["a table"];
    }
    if schema.enum_values().is_some() {
        return vec!["a string"];
    }
    schema
        .types()
        .into_iter()
        .map(|kind| match kind {
            "string" => "a string",
            "integer" => "a whole number",
            "number" => "a number",
            "boolean" => "true or false",
            "array" => "a list",
            _ => "a value",
        })
        .collect()
}

/// Whether the kind of `node` is one `schema` accepts, so a failure of another branch of a
/// union is not reported for it.
fn accepts(node: &Node, schema: Schema<'_>) -> bool {
    let schema = schema.resolved();
    if let Some(branches) = schema.one_of() {
        return branches.into_iter().any(|branch| accepts(node, branch));
    }
    if schema.name == Some("ResourceRef") {
        // A resource ID is written as a string where a reference is expected.
        return matches!(node.value, Value::String(_) | Value::Table(_));
    }
    if schema.properties().is_some() {
        return matches!(node.value, Value::Table(_));
    }
    if schema.enum_values().is_some() {
        return matches!(node.value, Value::String(_));
    }
    let types = schema.types();
    if types.is_empty() {
        return true;
    }
    types.iter().any(|kind| {
        matches!(
            (*kind, &node.value),
            ("string", Value::String(_))
                | ("integer", Value::Integer(_))
                | ("number", Value::Integer(_) | Value::Float(_))
                | ("boolean", Value::Boolean(_))
                | ("array", Value::Array(_))
                | ("object", Value::Table(_))
        )
    })
}

pub fn unknown_resource<'a>(id: &str, declared: impl Iterator<Item = &'a str> + Clone) -> String {
    let list: Vec<&str> = declared.clone().collect();
    if list.is_empty() {
        return format!("unknown resource '{id}'; no resources are declared");
    }
    format!(
        "unknown resource '{id}'{}; declared resources: {}",
        suggest::hint(id, declared),
        list.join(", ")
    )
}
