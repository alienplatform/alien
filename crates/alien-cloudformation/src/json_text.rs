//! Renders `Fn::ToJsonString` with core intrinsic functions.
//!
//! Generated templates do not use the `AWS::LanguageExtensions` transform. The
//! transform resolves every parameter reference to a literal when it processes a
//! template, and an update with `--use-previous-template` (the console's "Use
//! existing template") reuses that processed copy. A parameter-only update then
//! changes nothing, so every template parameter would silently stop working after
//! install. Without the transform, CloudFormation resolves each `Ref` against the
//! current parameter values on every update.
//!
//! Emitters still describe JSON text as `Fn::ToJsonString` over a value. Before
//! the template is returned, [`lower_json_strings`] rewrites each one into an
//! `Fn::Join` that CloudFormation evaluates to the same text the transform
//! produced:
//!
//! - literals serialize as JSON, and objects and lists keep their order;
//! - a `Ref` to a list parameter becomes a JSON array of strings, and any other
//!   `Ref`, `Fn::GetAtt`, `Fn::Sub`, `Fn::Join`, `Fn::Select` or `Fn::FindInMap`
//!   becomes a JSON string, numbers included (the transform renders a `Number`
//!   parameter as `"2"` too);
//! - `AWS::NoValue` becomes `null`, in objects and in lists;
//! - `Fn::If` picks between the two branches' JSON text.
//!
//! Resolved strings are inserted between quotes without escaping, which
//! CloudFormation cannot express. Every value these payloads reference is an
//! identifier, ARN, name, CIDR or enum choice; none contains a quote or a
//! backslash.

use crate::template::{CfExpression, CfParameter, CfTemplate};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use indexmap::IndexMap;

const TO_JSON_STRING: &str = "Fn::ToJsonString";

/// Intrinsic functions only the `AWS::LanguageExtensions` transform provides.
const TRANSFORM_ONLY_FUNCTIONS: [&str; 2] = ["Fn::Length", "Fn::ForEach"];

/// Intrinsic functions that always resolve to one string.
const STRING_FUNCTIONS: [&str; 7] = [
    "Fn::GetAtt",
    "Fn::Sub",
    "Fn::Join",
    "Fn::Select",
    "Fn::FindInMap",
    "Fn::ImportValue",
    "Fn::Base64",
];

/// Rewrites every `Fn::ToJsonString` in the template into core intrinsics and
/// refuses functions that need the language extensions transform.
pub(crate) fn lower_json_strings(template: &mut CfTemplate) -> Result<()> {
    let parameters = template.parameters.clone();
    let lowering = Lowering {
        parameters: &parameters,
    };
    for (logical_id, resource) in &mut template.resources {
        for value in resource.properties.values_mut() {
            lowering
                .expression(value)
                .map_err(|reason| refusal(&format!("resource '{logical_id}'"), reason))?;
        }
    }
    for (name, output) in &mut template.outputs {
        lowering
            .expression(&mut output.value)
            .map_err(|reason| refusal(&format!("output '{name}'"), reason))?;
    }
    for (name, condition) in &mut template.conditions {
        lowering
            .expression(condition)
            .map_err(|reason| refusal(&format!("condition '{name}'"), reason))?;
    }
    for (name, rule) in &mut template.rules {
        for assertion in &mut rule.assertions {
            lowering
                .expression(&mut assertion.assertion)
                .map_err(|reason| refusal(&format!("rule '{name}'"), reason))?;
        }
    }
    Ok(())
}

fn refusal(location: &str, reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: format!("render CloudFormation {location}"),
        reason,
    })
}

struct Lowering<'a> {
    parameters: &'a IndexMap<String, CfParameter>,
}

/// One piece of JSON text: fixed characters, or an intrinsic that resolves to
/// characters at deploy time.
enum Part {
    Text(String),
    Value(CfExpression),
}

impl Lowering<'_> {
    fn expression(&self, expression: &mut CfExpression) -> std::result::Result<(), String> {
        match expression {
            CfExpression::Object(fields) => {
                if let Some(function) = fields
                    .keys()
                    .find(|key| TRANSFORM_ONLY_FUNCTIONS.contains(&key.as_str()))
                {
                    return Err(format!(
                        "{function} needs the AWS::LanguageExtensions transform, which would freeze parameter values on updates that reuse the stack's template"
                    ));
                }
                if fields.len() == 1 {
                    if let Some(value) = fields.get(TO_JSON_STRING) {
                        let mut parts = Vec::new();
                        self.json(value, &mut parts)?;
                        *expression = concat(parts);
                        return Ok(());
                    }
                }
                for value in fields.values_mut() {
                    self.expression(value)?;
                }
                Ok(())
            }
            CfExpression::List(items) => {
                for item in items {
                    self.expression(item)?;
                }
                Ok(())
            }
            CfExpression::Null
            | CfExpression::Bool(_)
            | CfExpression::Integer(_)
            | CfExpression::Number(_)
            | CfExpression::String(_) => Ok(()),
        }
    }

    /// Appends the JSON text of `value` to `parts`.
    fn json(&self, value: &CfExpression, parts: &mut Vec<Part>) -> std::result::Result<(), String> {
        match value {
            CfExpression::Null => parts.push(Part::Text("null".to_string())),
            CfExpression::Bool(value) => parts.push(Part::Text(value.to_string())),
            CfExpression::Integer(value) => parts.push(Part::Text(value.to_string())),
            CfExpression::Number(value) => parts.push(Part::Text(
                serde_json::to_string(value).map_err(|error| error.to_string())?,
            )),
            CfExpression::String(value) => parts.push(Part::Text(quoted(value))),
            CfExpression::List(items) => {
                parts.push(Part::Text("[".to_string()));
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        parts.push(Part::Text(",".to_string()));
                    }
                    self.json(item, parts)?;
                }
                parts.push(Part::Text("]".to_string()));
            }
            CfExpression::Object(fields) if is_intrinsic(fields) => {
                self.intrinsic_json(fields, parts)?
            }
            CfExpression::Object(fields) => {
                parts.push(Part::Text("{".to_string()));
                for (index, (key, item)) in fields.iter().enumerate() {
                    let separator = if index > 0 { "," } else { "" };
                    parts.push(Part::Text(format!("{separator}{}:", quoted(key))));
                    self.json(item, parts)?;
                }
                parts.push(Part::Text("}".to_string()));
            }
        }
        Ok(())
    }

    fn intrinsic_json(
        &self,
        fields: &IndexMap<String, CfExpression>,
        parts: &mut Vec<Part>,
    ) -> std::result::Result<(), String> {
        let (function, arguments) = fields.iter().next().expect("intrinsic has one key");
        let intrinsic = CfExpression::Object(fields.clone());
        match function.as_str() {
            "Ref" if intrinsic == CfExpression::no_value() => {
                parts.push(Part::Text("null".to_string()));
            }
            "Ref" if self.is_list_parameter(arguments) => {
                parts.push(Part::Text("[\"".to_string()));
                parts.push(Part::Value(CfExpression::join("\",\"", intrinsic)));
                parts.push(Part::Text("\"]".to_string()));
            }
            "Ref" => string_value(intrinsic, parts),
            function if STRING_FUNCTIONS.contains(&function) => string_value(intrinsic, parts),
            "Fn::If" => {
                let CfExpression::List(branches) = arguments else {
                    return Err(format!("malformed Fn::If: {arguments:?}"));
                };
                let [condition, when_true, when_false] = &branches[..] else {
                    return Err(format!("malformed Fn::If: {arguments:?}"));
                };
                let branch = |value: &CfExpression| {
                    let mut branch_parts = Vec::new();
                    self.json(value, &mut branch_parts).map(|()| concat(branch_parts))
                };
                parts.push(Part::Value(CfExpression::object([(
                    "Fn::If",
                    CfExpression::list([condition.clone(), branch(when_true)?, branch(when_false)?]),
                )])));
            }
            function => {
                return Err(format!(
                    "{function} cannot be rendered as JSON text without the AWS::LanguageExtensions transform"
                ))
            }
        }
        Ok(())
    }

    fn is_list_parameter(&self, name: &CfExpression) -> bool {
        let CfExpression::String(name) = name else {
            return false;
        };
        self.parameters.get(name).is_some_and(|parameter| {
            parameter.parameter_type == "CommaDelimitedList"
                || parameter.parameter_type.starts_with("List<")
        })
    }
}

fn is_intrinsic(fields: &IndexMap<String, CfExpression>) -> bool {
    fields.len() == 1
        && fields
            .keys()
            .next()
            .is_some_and(|key| key == "Ref" || key.starts_with("Fn::"))
}

fn string_value(value: CfExpression, parts: &mut Vec<Part>) {
    parts.push(Part::Text("\"".to_string()));
    parts.push(Part::Value(value));
    parts.push(Part::Text("\"".to_string()));
}

fn quoted(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

/// Joins parts into one string expression, merging adjacent text.
fn concat(parts: Vec<Part>) -> CfExpression {
    let mut items: Vec<CfExpression> = Vec::new();
    let mut text = String::new();
    for part in parts {
        match part {
            Part::Text(value) => text.push_str(&value),
            Part::Value(value) => {
                if !text.is_empty() {
                    items.push(CfExpression::String(std::mem::take(&mut text)));
                }
                items.push(value);
            }
        }
    }
    if items.is_empty() {
        return CfExpression::String(text);
    }
    if !text.is_empty() {
        items.push(CfExpression::String(text));
    }
    CfExpression::join("", CfExpression::List(items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::CfOutput;
    use std::collections::HashMap;

    /// Deploy-time values for the evaluator below.
    struct Deploy {
        strings: HashMap<&'static str, &'static str>,
        lists: HashMap<&'static str, Vec<&'static str>>,
        conditions: HashMap<&'static str, bool>,
    }

    /// Evaluates the core intrinsics the lowering emits, the way CloudFormation does:
    /// `AWS::NoValue` drops a `Fn::Join` element, and a list parameter is a list.
    fn evaluate(expression: &CfExpression, deploy: &Deploy) -> Option<serde_json::Value> {
        use serde_json::Value;
        if *expression == CfExpression::no_value() {
            return None;
        }
        Some(match expression {
            CfExpression::String(text) => Value::String(text.clone()),
            CfExpression::Object(fields) => match fields.iter().next() {
                Some((function, CfExpression::String(name))) if function == "Ref" => {
                    match deploy.lists.get(name.as_str()) {
                        Some(items) => items.iter().map(|item| Value::from(*item)).collect(),
                        None => Value::from(deploy.strings[name.as_str()]),
                    }
                }
                Some((function, CfExpression::List(arguments))) if function == "Fn::If" => {
                    let CfExpression::String(condition) = &arguments[0] else {
                        panic!("condition name")
                    };
                    let taken = if deploy.conditions[condition.as_str()] {
                        1
                    } else {
                        2
                    };
                    return evaluate(&arguments[taken], deploy);
                }
                Some((function, CfExpression::List(arguments))) if function == "Fn::Join" => {
                    let CfExpression::String(delimiter) = &arguments[0] else {
                        panic!("delimiter")
                    };
                    let items = match evaluate(&arguments[1], deploy) {
                        Some(Value::Array(items)) => items,
                        other => panic!("Fn::Join takes a list: {other:?}"),
                    };
                    let items: Vec<String> = items
                        .into_iter()
                        .map(|item| item.as_str().expect("string element").to_string())
                        .collect();
                    Value::String(items.join(delimiter))
                }
                _ => panic!("unexpected expression {expression:?}"),
            },
            CfExpression::List(items) => items
                .iter()
                .filter_map(|item| evaluate(item, deploy))
                .collect(),
            other => panic!("unexpected literal {other:?}"),
        })
    }

    fn parameter(parameter_type: &str) -> CfParameter {
        CfParameter {
            parameter_type: parameter_type.to_string(),
            description: None,
            default: None,
            allowed_values: None,
            allowed_pattern: None,
            min_length: None,
            max_length: None,
            min_value: None,
            max_value: None,
            no_echo: None,
        }
    }

    fn lowered(value: CfExpression) -> CfTemplate {
        let mut template = CfTemplate::default();
        template
            .parameters
            .insert("Machines".to_string(), parameter("Number"));
        template
            .parameters
            .insert("Ids".to_string(), parameter("CommaDelimitedList"));
        template
            .parameters
            .insert("Ids2".to_string(), parameter("CommaDelimitedList"));
        template.outputs.insert(
            "Json".to_string(),
            CfOutput {
                value: CfExpression::to_json_string(value),
                description: None,
                export: None,
            },
        );
        lower_json_strings(&mut template).expect("lowering should succeed");
        template
    }

    /// The expected text is what `Fn::ToJsonString` produced for the same value in a
    /// real stack (AWS::LanguageExtensions, eu-west-1, Machines=2, Ids empty,
    /// Ids2=a,b, Never false).
    #[test]
    fn lowered_json_text_matches_the_transform_output() {
        let never = |when_true: CfExpression| {
            CfExpression::if_("Never", when_true, CfExpression::no_value())
        };
        let value = CfExpression::object([
            ("i", CfExpression::Integer(1)),
            ("f", CfExpression::Number(1.5)),
            ("b", CfExpression::Bool(true)),
            ("s", CfExpression::from("a\"b\\c")),
            ("num", CfExpression::ref_("Machines")),
            ("ids", CfExpression::ref_("Ids")),
            ("ids2", CfExpression::ref_("Ids2")),
            ("nv", CfExpression::no_value()),
            ("ifnv", never(CfExpression::from("x"))),
            (
                "lst",
                CfExpression::list([
                    CfExpression::Integer(1),
                    CfExpression::no_value(),
                    never(CfExpression::from("x")),
                    CfExpression::from("z"),
                ]),
            ),
            ("empty", CfExpression::object::<_, String>([])),
            ("emptyl", CfExpression::list([])),
            ("region", CfExpression::ref_("AWS::Region")),
        ]);
        let template = lowered(value);
        let deploy = Deploy {
            strings: HashMap::from([("Machines", "2"), ("AWS::Region", "eu-west-1")]),
            lists: HashMap::from([("Ids", vec![""]), ("Ids2", vec!["a", "b"])]),
            conditions: HashMap::from([("Never", false)]),
        };

        let text = evaluate(&template.outputs["Json"].value, &deploy).expect("output resolves");
        assert_eq!(
            text,
            serde_json::Value::String(
                r#"{"i":1,"f":1.5,"b":true,"s":"a\"b\\c","num":"2","ids":[""],"ids2":["a","b"],"nv":null,"ifnv":null,"lst":[1,null,null,"z"],"empty":{},"emptyl":[],"region":"eu-west-1"}"#
                    .to_string()
            )
        );
        let rendered = serde_json::to_string(&template).expect("template serializes");
        assert!(!rendered.contains(TO_JSON_STRING), "{rendered}");

        let taken = Deploy {
            conditions: HashMap::from([("Never", true)]),
            ..deploy
        };
        let text = evaluate(&template.outputs["Json"].value, &taken).expect("output resolves");
        let parsed: serde_json::Value =
            serde_json::from_str(text.as_str().expect("text")).expect("valid JSON");
        assert_eq!(parsed["ifnv"], "x");
        assert_eq!(parsed["lst"], serde_json::json!([1, null, "x", "z"]));
    }

    #[test]
    fn transform_only_functions_are_refused() {
        let mut template = CfTemplate::default();
        template.conditions.insert(
            "ThreeSubnets".to_string(),
            CfExpression::equals(
                CfExpression::object([("Fn::Length", CfExpression::ref_("Subnets"))]),
                CfExpression::Integer(3),
            ),
        );
        let error = lower_json_strings(&mut template).expect_err("Fn::Length needs the transform");
        assert!(error.to_string().contains("Fn::Length"), "{error}");

        let mut template = CfTemplate::default();
        template.outputs.insert(
            "Zones".to_string(),
            CfOutput {
                value: CfExpression::to_json_string(CfExpression::object([(
                    "Fn::GetAZs",
                    CfExpression::from(""),
                )])),
                description: None,
                export: None,
            },
        );
        let error = lower_json_strings(&mut template).expect_err("a list function has no text");
        assert!(error.to_string().contains("Fn::GetAZs"), "{error}");
    }
}
