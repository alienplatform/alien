//! Every field of the editor schema is written by some fixture, so no field of alien.toml is
//! left without a differential test against `alien.ts`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use alien_stack_toml::EDITOR_SCHEMA;
use serde_json::Value;

fn name_of(target: &str) -> &str {
    target.rsplit('/').next().expect("a reference has a name")
}

struct Schema {
    document: Value,
}

impl Schema {
    fn reference(&self, name: &str) -> &Value {
        &self.document["definitions"][name]
    }

    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(target) => self.reference(name_of(target)),
            None => schema,
        }
    }

    /// Every field path the schema describes. `[]` stands for a list item, `*` for a key chosen by
    /// the author.
    fn paths(
        &self,
        schema: &Value,
        at: &str,
        through: &mut Vec<String>,
        out: &mut BTreeSet<String>,
    ) {
        if let Some(target) = schema.get("$ref").and_then(Value::as_str) {
            if through.iter().any(|seen| seen == target) {
                return;
            }
            through.push(target.to_string());
            self.paths(
                self.resolve(schema),
                &format!("#{}", name_of(target)),
                through,
                out,
            );
            through.pop();
            return;
        }
        for union in ["anyOf", "oneOf"] {
            for branch in schema
                .get(union)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.paths(branch, at, through, out);
            }
        }
        for (name, property) in schema
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let path = format!("{at}/{name}");
            out.insert(path.clone());
            self.paths(property, &path, through, out);
        }
        if let Some(values) = schema
            .get("additionalProperties")
            .filter(|values| values.is_object())
        {
            let path = format!("{at}/*");
            out.insert(path.clone());
            self.paths(values, &path, through, out);
        }
        if let Some(items) = schema.get("items") {
            let path = format!("{at}[]");
            out.insert(path.clone());
            self.paths(items, &path, through, out);
        }
    }

    /// The paths of `instance` that `schema` describes.
    fn covered(&self, schema: &Value, instance: &Value, at: &str, out: &mut BTreeSet<String>) {
        let at = &match schema.get("$ref").and_then(Value::as_str) {
            Some(target) => format!("#{}", name_of(target)),
            None => at.to_string(),
        };
        let schema = self.resolve(schema);
        for union in ["anyOf", "oneOf"] {
            for branch in schema
                .get(union)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.covered(branch, instance, at, out);
            }
        }
        match instance {
            Value::Object(fields) => {
                let properties = schema.get("properties").and_then(Value::as_object);
                for (key, value) in fields {
                    match properties.and_then(|properties| properties.get(key)) {
                        Some(property) => {
                            let path = format!("{at}/{key}");
                            out.insert(path.clone());
                            self.covered(property, value, &path, out);
                        }
                        None => {
                            if let Some(values) = schema
                                .get("additionalProperties")
                                .filter(|values| values.is_object())
                            {
                                let path = format!("{at}/*");
                                out.insert(path.clone());
                                self.covered(values, value, &path, out);
                            }
                        }
                    }
                }
            }
            Value::Array(items) => {
                if let Some(schema) = schema.get("items") {
                    let path = format!("{at}[]");
                    out.insert(path.clone());
                    for item in items {
                        self.covered(schema, item, &path, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Fields no fixture can write, and why.
const UNCOVERED: &[(&str, &str)] = &[];

#[test]
fn every_field_of_the_editor_schema_is_written_by_a_fixture() {
    let schema = Schema {
        document: serde_json::from_str(EDITOR_SCHEMA).expect("the editor schema is JSON"),
    };
    let mut described = BTreeSet::new();
    schema.paths(&schema.document, "", &mut Vec::new(), &mut described);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut covered = BTreeSet::new();
    for entry in std::fs::read_dir(&root).expect("fixtures directory") {
        let path = entry.expect("fixture entry").path().join("alien.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let document: Value =
            toml::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        schema.covered(&schema.document, &document, "", &mut covered);
    }

    let uncovered: Vec<&String> = described
        .iter()
        .filter(|path| !covered.contains(*path))
        .filter(|path| !UNCOVERED.iter().any(|(skipped, _)| skipped == path))
        .collect();
    assert!(
        uncovered.is_empty(),
        "{} fields are in the editor schema but in no fixture:\n{}",
        uncovered.len(),
        uncovered
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
