//! Checks a custom binding's fields against the outputs schema its type's descriptor carries.
//!
//! The schemas come from `utoipa`, through the extension SDK: objects with `properties` and
//! `required`, `$ref`s into `$defs`, `oneOf` for an optional struct, `enum` for a unit enum, and
//! the numeric and string bounds Rust's types imply. The check reads exactly the keywords below.
//! A schema that uses any other keyword refuses the binding, naming the keyword, so a binding is
//! never accepted against a rule the check skipped.
//!
//! One rule is stricter than JSON Schema: an object field its schema doesn't name is refused
//! unless `additionalProperties` allows it, so a misspelled output is caught. A field one `allOf`
//! branch names, as `#[serde(flatten)]` writes, counts as named in every branch.
//!
//! The check is hand-written rather than the `jsonschema` crate's because alien-core builds for
//! wasm32-wasip1, and that crate pulls in reqwest, fancy-regex and idna.

use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};

/// Keywords that describe a schema without constraining a value.
const ANNOTATIONS: &[&str] = &[
    "title",
    "description",
    "default",
    "example",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "format",
    "contentMediaType",
    "contentEncoding",
    "discriminator",
    "$schema",
    "$id",
    "$comment",
    "$defs",
    "definitions",
    "nullable",
];

/// Keywords the check enforces.
const CHECKED: &[&str] = &[
    "$ref",
    "type",
    "enum",
    "const",
    "properties",
    "required",
    "additionalProperties",
    "propertyNames",
    "items",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "pattern",
    "oneOf",
    "anyOf",
    "allOf",
];

/// How many `$ref`s and alternatives one value may pass through: a schema that refers to itself
/// without consuming the value would otherwise never end.
const MAX_DEPTH: usize = 64;

/// What is wrong with `value` against `schema`, a descriptor's whole outputs schema. Empty when
/// it matches.
pub(crate) fn problems(schema: &Map<String, Value>, value: &Value) -> Vec<String> {
    let root = Value::Object(schema.clone());
    let mut checker = Checker {
        root: &root,
        problems: Vec::new(),
        unsupported: Vec::new(),
        named_by_all_of: HashMap::new(),
    };
    checker.check(&root, value, "", 0);
    let mut all = checker.unsupported;
    all.extend(checker.problems);
    all
}

struct Checker<'a> {
    root: &'a Value,
    /// Ways the value breaks the schema.
    problems: Vec<String>,
    /// Constructs the check can't read. Any one refuses the binding, whatever the value.
    unsupported: Vec<String>,
    /// The fields an `allOf` names, across its branches, for the object at each path.
    named_by_all_of: HashMap<String, BTreeSet<String>>,
}

impl<'a> Checker<'a> {
    fn fork(&self) -> Checker<'a> {
        Checker {
            root: self.root,
            problems: Vec::new(),
            unsupported: Vec::new(),
            named_by_all_of: self.named_by_all_of.clone(),
        }
    }

    fn unsupported(&mut self, path: &str, what: String) {
        self.unsupported.push(format!(
            "the outputs schema {what} at {}, which Alien can't check a binding against",
            describe(path)
        ));
    }

    fn problem(&mut self, path: &str, what: String) {
        self.problems.push(format!("{} {what}", describe(path)));
    }

    fn check(&mut self, schema: &Value, value: &Value, path: &str, depth: usize) {
        if depth > MAX_DEPTH {
            self.unsupported(path, "nests deeper than 64 references".to_string());
            return;
        }
        let schema = match schema {
            Value::Bool(true) => return,
            Value::Bool(false) => {
                self.problem(path, "is not allowed".to_string());
                return;
            }
            Value::Object(schema) => schema,
            _ => {
                self.unsupported(path, "has a schema that is not an object".to_string());
                return;
            }
        };
        for keyword in schema.keys() {
            if !CHECKED.contains(&keyword.as_str())
                && !ANNOTATIONS.contains(&keyword.as_str())
                && !keyword.starts_with("x-")
            {
                self.unsupported(path, format!("uses '{keyword}'"));
            }
        }
        if schema
            .get("properties")
            .is_some_and(|properties| !properties.is_object())
        {
            self.unsupported(path, "has 'properties' that is not an object".to_string());
        }
        if schema.get("required").is_some_and(|required| {
            !required
                .as_array()
                .is_some_and(|names| names.iter().all(Value::is_string))
        }) {
            self.unsupported(
                path,
                "has a 'required' that is not a list of names".to_string(),
            );
        }
        if !self.unsupported.is_empty() {
            return;
        }
        if value.is_null() && schema.get("nullable") == Some(&Value::Bool(true)) {
            return;
        }

        if let Some(reference) = schema.get("$ref") {
            match reference
                .as_str()
                .and_then(|reference| self.resolve(reference))
            {
                Some(target) => self.check(target, value, path, depth + 1),
                None => self.unsupported(path, format!("has the reference {reference}")),
            }
        }
        if let Some(kinds) = schema.get("type") {
            let kinds: Vec<&str> = match kinds {
                Value::String(kind) => vec![kind.as_str()],
                Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            if kinds.is_empty() {
                self.unsupported(path, format!("has the type {}", schema["type"]));
                return;
            }
            if !kinds.iter().any(|kind| has_type(kind, value)) {
                self.problem(
                    path,
                    format!(
                        "is {}, where the outputs schema says {}",
                        type_name(value),
                        kinds.join(" or ")
                    ),
                );
                return;
            }
        }
        if let Some(allowed) = schema.get("enum") {
            match allowed {
                Value::Array(allowed) if !allowed.contains(value) => {
                    self.problem(
                        path,
                        format!(
                            "is {value}, which is not one of {}",
                            Value::Array(allowed.clone())
                        ),
                    );
                }
                Value::Array(_) => {}
                _ => self.unsupported(path, "has an 'enum' that is not a list".to_string()),
            }
        }
        if let Some(constant) = schema.get("const") {
            if constant != value {
                self.problem(
                    path,
                    format!("is {value}, where the outputs schema says {constant}"),
                );
            }
        }
        if value.is_object() && schema.contains_key("allOf") {
            let names = property_names(self.root, schema, depth);
            self.named_by_all_of
                .entry(path.to_string())
                .or_default()
                .extend(names);
        }
        match value {
            Value::Object(object) => self.check_object(schema, object, path, depth),
            Value::Array(items) => self.check_array(schema, items, path, depth),
            Value::String(text) => self.check_string(schema, text, path),
            Value::Number(_) => self.check_number(schema, value, path),
            _ => {}
        }
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(branches) = schema.get(keyword) {
                self.check_branches(keyword, branches, value, path, depth);
            }
        }
    }

    fn check_branches(
        &mut self,
        keyword: &str,
        branches: &Value,
        value: &Value,
        path: &str,
        depth: usize,
    ) {
        let Value::Array(branches) = branches else {
            self.unsupported(path, format!("has a '{keyword}' that is not a list"));
            return;
        };
        let mut matched = 0;
        let mut first_problems = Vec::new();
        for branch in branches {
            let mut fork = self.fork();
            fork.check(branch, value, path, depth + 1);
            if !fork.unsupported.is_empty() {
                self.unsupported.extend(fork.unsupported);
                return;
            }
            if fork.problems.is_empty() {
                matched += 1;
            } else if keyword == "allOf" {
                self.problems.extend(fork.problems);
            } else if first_problems.is_empty() {
                first_problems = fork.problems;
            }
        }
        match keyword {
            "anyOf" if matched == 0 => self.problem(
                path,
                format!(
                    "matches none of its alternatives ({})",
                    first_problems.join("; ")
                ),
            ),
            "oneOf" if matched == 0 => self.problem(
                path,
                format!(
                    "matches none of its alternatives ({})",
                    first_problems.join("; ")
                ),
            ),
            "oneOf" if matched > 1 => self.problem(
                path,
                format!("matches {matched} of its alternatives, where exactly one must"),
            ),
            _ => {}
        }
    }

    fn check_object(
        &mut self,
        schema: &Map<String, Value>,
        object: &Map<String, Value>,
        path: &str,
        depth: usize,
    ) {
        // `check` refused a `properties` or `required` of another shape.
        let properties = schema.get("properties").and_then(Value::as_object);
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(name) {
                self.problem(path, format!("has no '{name}'"));
            }
        }
        let named_elsewhere = self.named_by_all_of.get(path).cloned().unwrap_or_default();
        for (name, field) in object {
            let field_path = join(path, name);
            if let Some(names) = schema.get("propertyNames") {
                self.check(names, &Value::String(name.clone()), &field_path, depth);
            }
            match (
                properties.and_then(|properties| properties.get(name)),
                schema.get("additionalProperties"),
            ) {
                (Some(property), _) => self.check(property, field, &field_path, depth),
                (None, Some(additional)) => self.check(additional, field, &field_path, depth),
                (None, None) if properties.is_some() && !named_elsewhere.contains(name) => {
                    self.problem(
                        &field_path,
                        "is not one of the outputs its schema names".to_string(),
                    );
                }
                (None, None) => {}
            }
        }
    }

    fn check_array(
        &mut self,
        schema: &Map<String, Value>,
        items: &[Value],
        path: &str,
        depth: usize,
    ) {
        if let Some(item) = schema.get("items") {
            if item.is_array() {
                self.unsupported(path, "has a list of 'items'".to_string());
                return;
            }
            for (index, value) in items.iter().enumerate() {
                self.check(
                    item,
                    value,
                    &format!(
                        "{}[{index}]",
                        if path.is_empty() { "outputs" } else { path }
                    ),
                    depth,
                );
            }
        }
        self.check_count(schema, "minItems", "maxItems", items.len(), "items", path);
    }

    fn check_string(&mut self, schema: &Map<String, Value>, text: &str, path: &str) {
        self.check_count(
            schema,
            "minLength",
            "maxLength",
            text.chars().count(),
            "characters",
            path,
        );
        if let Some(pattern) = schema.get("pattern") {
            match pattern.as_str().map(Regex::new) {
                Some(Ok(pattern)) if pattern.is_match(text) => {}
                Some(Ok(pattern)) => self.problem(
                    path,
                    format!("is {text:?}, which doesn't match {}", pattern.as_str()),
                ),
                _ => self.unsupported(path, format!("has the pattern {pattern}")),
            }
        }
    }

    fn check_number(&mut self, schema: &Map<String, Value>, value: &Value, path: &str) {
        let Some(number) = value.as_f64() else { return };
        for (keyword, holds) in [
            (
                "minimum",
                (|n: f64, bound: f64| n >= bound) as fn(f64, f64) -> bool,
            ),
            ("maximum", |n, bound| n <= bound),
            ("exclusiveMinimum", |n, bound| n > bound),
            ("exclusiveMaximum", |n, bound| n < bound),
        ] {
            match schema.get(keyword) {
                None => {}
                Some(bound) => match bound.as_f64() {
                    Some(bound) if holds(number, bound) => {}
                    Some(bound) => self.problem(
                        path,
                        format!("is {value}, where the outputs schema says {keyword} {bound}"),
                    ),
                    None => self.unsupported(path, format!("has the {keyword} {bound}")),
                },
            }
        }
    }

    fn check_count(
        &mut self,
        schema: &Map<String, Value>,
        min: &str,
        max: &str,
        count: usize,
        unit: &str,
        path: &str,
    ) {
        for (keyword, holds) in [
            (
                min,
                (|count: u64, bound: u64| count >= bound) as fn(u64, u64) -> bool,
            ),
            (max, |count, bound| count <= bound),
        ] {
            match schema.get(keyword) {
                None => {}
                Some(bound) => match bound.as_u64() {
                    Some(bound) if holds(count as u64, bound) => {}
                    Some(bound) => self.problem(
                        path,
                        format!(
                            "has {count} {unit}, where the outputs schema says {keyword} {bound}"
                        ),
                    ),
                    None => self.unsupported(path, format!("has the {keyword} {bound}")),
                },
            }
        }
    }

    /// The schema a local reference such as `#/$defs/Ttl` points at, in the outputs schema.
    fn resolve(&self, reference: &str) -> Option<&'a Value> {
        let pointer = reference.strip_prefix('#')?;
        self.root.pointer(pointer)
    }
}

/// The fields `schema` names for an object: its own `properties` and those of its `allOf`
/// branches, through `$ref`s. Constructs it can't read add none; the check refuses them anyway.
fn property_names(root: &Value, schema: &Map<String, Value>, depth: usize) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if depth > MAX_DEPTH {
        return names;
    }
    if let Some(Value::Object(properties)) = schema.get("properties") {
        names.extend(properties.keys().cloned());
    }
    if let Some(Value::Object(target)) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| root.pointer(reference.strip_prefix('#')?))
    {
        names.extend(property_names(root, target, depth + 1));
    }
    for branch in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Value::Object(branch) = branch {
            names.extend(property_names(root, branch, depth + 1));
        }
    }
    names
}

/// The outputs a type's outputs schema names, which a custom binding of the type carries as
/// fields: its root `properties` and those of its `allOf` branches, through `$ref`s, as
/// `#[serde(flatten)]` writes them.
pub fn output_names(schema: &Map<String, Value>) -> BTreeSet<String> {
    property_names(&Value::Object(schema.clone()), schema, 0)
}

/// Why [`flattens_alternatives`] refuses an outputs schema.
pub const FLATTENED_OPTION: &str = "flatten an Option or an enum, whose fields depend on the value, which Alien can't check a binding against; flatten a struct and make its fields optional instead";

/// Why [`flattens_a_map_beside_fields`] refuses an outputs schema.
pub const FLATTENED_MAP: &str = "flatten a map beside a struct's fields, which Alien can't check a binding against; put the map under a named field instead of flattening it";

/// Whether an outputs schema flattens an `Option` or an enum anywhere: in the outputs or in any
/// type they use. utoipa writes `#[serde(flatten)] Option<S>` as an `allOf` branch with a
/// `oneOf` of null and `S`, and a flattened enum as one with a `oneOf` of its variants. Which
/// fields an object of it carries depends on the value, so the check can't tell a misspelled
/// field from one of them.
pub fn flattens_alternatives(schema: &Map<String, Value>) -> bool {
    let root = Value::Object(schema.clone());
    any_all_of(&root, &root, &|branches| {
        branches
            .iter()
            .any(|branch| branch.contains_key("oneOf") || branch.contains_key("anyOf"))
    })
}

/// Whether an outputs schema flattens a map beside a struct's fields anywhere: utoipa writes it
/// as an `allOf` where one branch allows any other field and another names its own. The check
/// reads each branch alone, so the struct's branch would refuse every key of the map. A struct
/// that itself flattens a struct names its fields through its own `allOf`, not `properties`.
pub fn flattens_a_map_beside_fields(schema: &Map<String, Value>) -> bool {
    let root = Value::Object(schema.clone());
    any_all_of(&root, &root, &|branches| {
        let open = |branch: &Map<String, Value>| {
            branch
                .get("additionalProperties")
                .is_some_and(|additional| additional != &Value::Bool(false))
        };
        branches.iter().any(|branch| open(branch))
            && branches
                .iter()
                .any(|branch| !open(branch) && !property_names(&root, branch, 0).is_empty())
    })
}

/// A rule on the branches of one `allOf`.
type AllOfTest<'a> = dyn Fn(&[&Map<String, Value>]) -> bool + 'a;

/// Whether `test` holds for the branches, each resolved through local `$ref`s, of any `allOf` in
/// the schema `node` or any schema in it, including the types under `$defs`. Walking the document
/// instead of following `$ref`s ends the walk on a type that refers to itself.
fn any_all_of(root: &Value, node: &Value, test: &AllOfTest<'_>) -> bool {
    let Value::Object(schema) = node else {
        return false;
    };
    if let Some(Value::Array(branches)) = schema.get("allOf") {
        let branches: Vec<_> = branches
            .iter()
            .filter_map(Value::as_object)
            .map(|branch| resolved(root, branch, 0))
            .collect();
        if test(&branches) {
            return true;
        }
    }
    schema
        .iter()
        .any(|(keyword, value)| match keyword.as_str() {
            // Maps from a name to a schema: a field or a type may be named like a keyword.
            "properties" | "$defs" | "definitions" => value
                .as_object()
                .into_iter()
                .flat_map(Map::values)
                .any(|schema| any_all_of(root, schema, test)),
            // A value is data, not a schema.
            "enum" | "const" | "default" | "example" | "examples" => false,
            _ => match value {
                Value::Array(schemas) => {
                    schemas.iter().any(|schema| any_all_of(root, schema, test))
                }
                schema => any_all_of(root, schema, test),
            },
        })
}

/// The schema `schema` refers to through local `$ref`s, or `schema` itself.
fn resolved<'a>(
    root: &'a Value,
    schema: &'a Map<String, Value>,
    depth: usize,
) -> &'a Map<String, Value> {
    match schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| root.pointer(reference.strip_prefix('#')?))
        .and_then(Value::as_object)
    {
        Some(target) if depth <= MAX_DEPTH => resolved(root, target, depth + 1),
        _ => schema,
    }
}

/// Whether `value` has the JSON Schema type `kind`. An integer is any number without a fraction,
/// so `1.0` is one, as JSON Schema counts it.
fn has_type(kind: &str, value: &Value) -> bool {
    match kind {
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        "number" => value.is_number(),
        other => type_name(value) == other,
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

fn describe(path: &str) -> String {
    if path.is_empty() {
        "the binding".to_string()
    } else {
        format!("'{path}'")
    }
}
