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
//! CloudFormation cannot express. A parameter inserted this way must therefore
//! be unable to hold a character JSON would need escaped: it is a `Number`, it
//! has `AllowedValues` that need no escaping, or its `AllowedPattern` is one of
//! [`JSON_SAFE_PATTERNS`]. The pass refuses any other parameter, so a value
//! with a quote is rejected when the stack is created or updated instead of
//! producing JSON that registration cannot parse.

use crate::{
    dependencies::collect_references,
    template::{CfExpression, CfParameter, CfTemplate},
};
use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use indexmap::{IndexMap, IndexSet};

const TO_JSON_STRING: &str = "Fn::ToJsonString";

/// Characters a value can hold and still sit between JSON quotes unescaped:
/// anything but a double quote, a backslash or a control character. In a
/// `CommaDelimitedList`, CloudFormation applies the pattern to each entry.
macro_rules! json_safe_characters {
    () => {
        r#"[^"\\\x00-\x1F]"#
    };
}

/// `AllowedPattern` for a free-text parameter that is inserted into JSON text.
/// It accepts the empty string, so optional parameters keep their empty default.
pub(crate) const JSON_SAFE_TEXT_PATTERN: &str = concat!("^", json_safe_characters!(), "*$");

/// `ConstraintDescription` for [`JSON_SAFE_TEXT_PATTERN`].
pub(crate) const JSON_SAFE_TEXT_CONSTRAINT: &str =
    "must not contain double quotes, backslashes or control characters";

/// `AllowedPattern` for an optional ACM certificate ARN, in any partition
/// (`aws`, `aws-cn`, `aws-us-gov`, ...). The certificate ID excludes the same
/// characters as [`JSON_SAFE_TEXT_PATTERN`].
pub(crate) const CERTIFICATE_ARN_PATTERN: &str = concat!(
    "^$|^arn:aws[a-z-]*:acm:[a-z0-9-]+:[0-9]{12}:certificate/",
    json_safe_characters!(),
    "+$"
);

/// Patterns that admit no character JSON text would need escaped.
const JSON_SAFE_PATTERNS: [&str; 2] = [JSON_SAFE_TEXT_PATTERN, CERTIFICATE_ARN_PATTERN];

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
                self.require_json_safe(&intrinsic)?;
                parts.push(Part::Text("[\"".to_string()));
                parts.push(Part::Value(CfExpression::join("\",\"", intrinsic)));
                parts.push(Part::Text("\"]".to_string()));
            }
            "Ref" => {
                self.require_json_safe(&intrinsic)?;
                string_value(intrinsic, parts)
            }
            function if STRING_FUNCTIONS.contains(&function) => {
                self.require_json_safe(&intrinsic)?;
                string_value(intrinsic, parts)
            }
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

    /// Refuses a value that reads a parameter able to hold a character JSON text
    /// would need escaped.
    fn require_json_safe(&self, value: &CfExpression) -> std::result::Result<(), String> {
        // Fn::Sub variables can shadow parameters; only the values they read
        // must satisfy the parameter constraints. Plain text is not a reference.
        let mut references = IndexSet::new();
        collect_references(value, &mut references);
        match self.parameters.iter().find(|(name, parameter)| {
            references.contains(*name) && !is_json_safe(parameter)
        }) {
            Some((name, _)) => Err(format!(
                "parameter '{name}' is inserted into JSON text without escaping, so it needs AllowedValues, a Number type, or an AllowedPattern that excludes double quotes, backslashes and control characters"
            )),
            None => Ok(()),
        }
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

fn is_json_safe(parameter: &CfParameter) -> bool {
    let number = matches!(parameter.parameter_type.as_str(), "Number" | "List<Number>");
    let safe_values = parameter.allowed_values.as_ref().is_some_and(|values| {
        values.iter().all(|value| match value {
            CfExpression::String(text) => quoted(text) == format!("\"{text}\""),
            CfExpression::Bool(_) | CfExpression::Integer(_) | CfExpression::Number(_) => true,
            CfExpression::Null | CfExpression::List(_) | CfExpression::Object(_) => false,
        })
    });
    let safe_pattern = parameter
        .allowed_pattern
        .as_deref()
        .is_some_and(|pattern| JSON_SAFE_PATTERNS.contains(&pattern));
    number || safe_values || safe_pattern
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
                Some((function, arguments)) if function == "Fn::Sub" => {
                    let (text, variables) = match arguments {
                        CfExpression::String(text) => (text, None),
                        CfExpression::List(arguments) => match &arguments[..] {
                            [CfExpression::String(text), CfExpression::Object(variables)] => {
                                (text, Some(variables))
                            }
                            _ => panic!("substitution arguments"),
                        },
                        _ => panic!("substitution arguments"),
                    };
                    let variables: HashMap<_, _> = variables
                        .into_iter()
                        .flat_map(|v| v.iter())
                        .map(|(name, value)| (name.as_str(), evaluate(value, deploy).unwrap()))
                        .collect();
                    let pattern = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
                    Value::String(
                        pattern
                            .replace_all(text, |captures: &regex::Captures<'_>| {
                                let name = &captures[1];
                                if let Some(literal) = name.strip_prefix('!') {
                                    format!("${{{literal}}}")
                                } else if let Some(value) = variables.get(name) {
                                    value.as_str().unwrap().to_string()
                                } else {
                                    deploy.strings[name].to_string()
                                }
                            })
                            .into_owned(),
                    )
                }
                Some((function, CfExpression::List(arguments))) if function == "Fn::Split" => {
                    let delimiter = evaluate(&arguments[0], deploy).unwrap();
                    let text = evaluate(&arguments[1], deploy).unwrap();
                    text.as_str()
                        .unwrap()
                        .split(delimiter.as_str().unwrap())
                        .map(Value::from)
                        .collect()
                }
                Some((function, CfExpression::List(arguments))) if function == "Fn::Select" => {
                    let CfExpression::Integer(index) = arguments[0] else {
                        panic!("select index")
                    };
                    evaluate(&arguments[1], deploy).unwrap()[index as usize].clone()
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

    fn parameter(parameter_type: &str, allowed_pattern: Option<&str>) -> CfParameter {
        CfParameter {
            parameter_type: parameter_type.to_string(),
            description: None,
            default: None,
            allowed_values: None,
            allowed_pattern: allowed_pattern.map(str::to_string),
            constraint_description: None,
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
            .insert("Machines".to_string(), parameter("Number", None));
        for name in ["Ids", "Ids2"] {
            template.parameters.insert(
                name.to_string(),
                parameter("CommaDelimitedList", Some(JSON_SAFE_TEXT_PATTERN)),
            );
        }
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

    /// A parameter that could hold a quote must not reach JSON text: the payload
    /// would stop parsing at registration instead of the value being refused when
    /// the stack is created or updated.
    #[test]
    fn unconstrained_parameters_are_refused_in_json_text() {
        for (parameter_type, value) in [
            ("String", CfExpression::ref_("Free")),
            ("CommaDelimitedList", CfExpression::ref_("Free")),
            (
                "String",
                CfExpression::sub("arn:aws:iam::${AWS::AccountId}:role/${Free}"),
            ),
        ] {
            let mut template = CfTemplate::default();
            template
                .parameters
                .insert("Free".to_string(), parameter(parameter_type, None));
            template.outputs.insert(
                "Json".to_string(),
                CfOutput {
                    value: CfExpression::to_json_string(CfExpression::object([("v", value)])),
                    description: None,
                    export: None,
                },
            );
            let error = lower_json_strings(&mut template)
                .expect_err("an unconstrained parameter must not be inserted unescaped");
            assert!(error.to_string().contains("'Free'"), "{error}");
        }

        let mut template = CfTemplate::default();
        let mut choice = parameter("String", None);
        choice.allowed_values = Some(vec![CfExpression::from("a\"b")]);
        template.parameters.insert("Choice".to_string(), choice);
        template.outputs.insert(
            "Json".to_string(),
            CfOutput {
                value: CfExpression::to_json_string(CfExpression::ref_("Choice")),
                description: None,
                export: None,
            },
        );
        lower_json_strings(&mut template)
            .expect_err("an allowed value that needs escaping is not safe");
    }

    #[test]
    fn boundary_document_json_uses_the_substitution_map_on_parameter_updates() {
        let context = alien_permissions::PermissionContext::new()
            .with_stack_prefix("${AWS::StackName}")
            .with_aws_region("${AWS::Region}")
            .with_aws_account_id("${AWS::AccountId}")
            .with_managing_account_id("${managingAccountId}");
        let document =
            alien_permissions::generators::aws_permissions_boundary(&context, &[]).unwrap();
        let rendered = alien_permissions::generators::AwsCloudFormationPermissionsGenerator::new()
            .render_document(&document)
            .unwrap();
        let expression: CfExpression = serde_json::from_value(rendered).unwrap();
        let mut template = CfTemplate {
            aws_template_format_version: "2010-09-09".into(),
            ..CfTemplate::default()
        };
        template
            .parameters
            .insert("ManagingAccountId".into(), parameter("String", None));
        template.parameters.insert(
            "ManagingRoleArn".into(),
            parameter("String", Some(JSON_SAFE_TEXT_PATTERN)),
        );
        template.outputs.insert(
            "Boundary".into(),
            CfOutput {
                value: CfExpression::to_json_string(expression.clone()),
                description: None,
                export: None,
            },
        );
        // The same-named parameter is legal in a plain output; the local substitution
        // variable must not read it when serializing the boundary into JSON.
        template.outputs.insert(
            "AccountInput".into(),
            CfOutput {
                value: CfExpression::ref_("ManagingAccountId"),
                description: None,
                export: None,
            },
        );
        let mut policy = crate::template::CfResource::new(
            "BoundaryPolicy".into(),
            "AWS::IAM::ManagedPolicy".into(),
        );
        policy
            .properties
            .insert("PolicyDocument".into(), expression);
        template.resources.insert(policy.logical_id.clone(), policy);
        lower_json_strings(&mut template).unwrap();
        crate::test_utils::cfn_lint(&crate::to_yaml(&template).unwrap())
            .assert_ok("boundary JSON with a shadowed parameter");
        for (role, account) in [
            ("arn:aws:iam::210987654321:role/manager", "210987654321"),
            (
                "arn:aws:iam::109876543210:role/other-manager",
                "109876543210",
            ),
        ] {
            let deploy = Deploy {
                strings: HashMap::from([
                    ("ManagingRoleArn", role),
                    ("ManagingAccountId", "unsafe\"input"),
                    ("AWS::Partition", "aws"),
                    ("AWS::AccountId", "123456789012"),
                    ("AWS::Region", "ap-southeast-7"),
                    ("AWS::StackName", "example"),
                ]),
                lists: HashMap::new(),
                conditions: HashMap::new(),
            };
            let text = evaluate(&template.outputs["Boundary"].value, &deploy).unwrap();
            let actual: serde_json::Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
            let expected = alien_permissions::generators::aws_permissions_boundary(
                &alien_permissions::PermissionContext::new()
                    .with_stack_prefix("example")
                    .with_aws_region("ap-southeast-7")
                    .with_aws_account_id("123456789012")
                    .with_managing_account_id(account),
                &[],
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn substitution_maps_do_not_hide_unsafe_parameter_reads() {
        for value in [
            CfExpression::sub_with("${Local}", [("Local", CfExpression::ref_("Free"))]),
            CfExpression::sub_with("${Free}", [("Free", CfExpression::ref_("Free"))]),
            CfExpression::sub_with("${Free}/${Other}", [("Other", CfExpression::from("safe"))]),
            CfExpression::sub_with("${Local}", [("Local", CfExpression::sub("${Free}"))]),
            CfExpression::sub_with(
                "${Local}",
                [(
                    "Local",
                    CfExpression::if_(
                        "Enabled",
                        CfExpression::from("safe"),
                        CfExpression::ref_("Free"),
                    ),
                )],
            ),
        ] {
            let mut template = CfTemplate::default();
            template
                .parameters
                .insert("Free".into(), parameter("String", None));
            template.outputs.insert(
                "Json".into(),
                CfOutput {
                    value: CfExpression::to_json_string(value),
                    description: None,
                    export: None,
                },
            );
            let error = lower_json_strings(&mut template).unwrap_err();
            assert!(error.to_string().contains("'Free'"), "{error}");
        }
    }

    #[test]
    fn literal_and_escaped_parameter_names_remain_literal_in_json() {
        for (value, expected) in [
            (
                CfExpression::join("", CfExpression::list([CfExpression::from("Free")])),
                "Free",
            ),
            (CfExpression::sub("${!Free}"), "${Free}"),
            (
                CfExpression::sub_with("${Free}", [("Free", CfExpression::from("${Free}"))]),
                "${Free}",
            ),
            (
                CfExpression::sub_with(
                    "${Free.Arn}",
                    [("Free.Arn", CfExpression::from("literal"))],
                ),
                "literal",
            ),
        ] {
            let mut template = CfTemplate::default();
            template
                .parameters
                .insert("Free".into(), parameter("String", None));
            template.outputs.insert(
                "Json".into(),
                CfOutput {
                    value: CfExpression::to_json_string(value),
                    description: None,
                    export: None,
                },
            );
            lower_json_strings(&mut template).unwrap();
            let deploy = Deploy {
                strings: HashMap::new(),
                lists: HashMap::new(),
                conditions: HashMap::new(),
            };
            let text = evaluate(&template.outputs["Json"].value, &deploy).unwrap();
            let actual: serde_json::Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
            assert_eq!(actual, expected);
        }
    }

    /// The patterns accept every legitimate value of the parameters they guard,
    /// and nothing JSON text would need escaped. CloudFormation evaluates
    /// `AllowedPattern` as a Java regular expression; these patterns use only
    /// anchors, alternation and character classes, which Rust's `regex` reads
    /// the same way. The live proof also checked them against CloudFormation.
    #[test]
    fn json_safe_patterns_accept_real_values_and_reject_escapes() {
        let text = regex::Regex::new(JSON_SAFE_TEXT_PATTERN).expect("text pattern compiles");
        let certificate =
            regex::Regex::new(CERTIFICATE_ARN_PATTERN).expect("certificate pattern compiles");
        let long_domain = format!("{}.Example.COM", vec!["a".repeat(63); 3].join("."));
        for value in [
            "",
            "vpc-0123456789abcdef0",
            "subnet-0123456789abcdef0",
            "sg-0123456789abcdef0",
            "10.0.0.0/16",
            "127.0.0.1/32",
            "512Mi",
            "123456789012",
            "arn:aws:iam::123456789012:role/Alien-Management",
            "arn:aws-cn:iam::123456789012:role/path/to/role",
            "arn:aws-us-gov:iam::123456789012:role/role+name=,.@_-",
            "API.Example.COM",
            "*.example.com",
            long_domain.as_str(),
        ] {
            assert!(text.is_match(value), "text pattern must accept {value:?}");
        }
        for value in [
            "",
            "arn:aws:acm:us-east-1:123456789012:certificate/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
            "arn:aws-cn:acm:cn-north-1:123456789012:certificate/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
            "arn:aws-us-gov:acm:us-gov-west-1:123456789012:certificate/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        ] {
            assert!(
                certificate.is_match(value),
                "certificate pattern must accept {value:?}"
            );
        }
        for value in ["a\"b", "a\\b", "a\nb", "a\tb", "\u{0}"] {
            assert!(!text.is_match(value), "text pattern must reject {value:?}");
            let arn = format!("arn:aws:acm:us-east-1:123456789012:certificate/{value}");
            assert!(
                !certificate.is_match(&arn),
                "certificate pattern must reject {arn:?}"
            );
        }
        assert!(!certificate.is_match("arn:aws:iam::123456789012:role/not-a-certificate"));
    }
}
