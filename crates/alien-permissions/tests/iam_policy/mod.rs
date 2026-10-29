//! A small model of IAM policy evaluation for tests: an explicit Deny wins, otherwise any
//! matching Allow allows, otherwise the request is denied. It models only the condition
//! operators the permission sets use and panics on any other, so a test never passes on an
//! operator it does not understand.
#![allow(dead_code)]

use alien_core::AwsConditionValue;
use alien_permissions::generators::AwsIamStatement;
use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    ExplicitlyDenied,
    NotAllowed,
}

pub struct Request {
    pub action: &'static str,
    pub resource: String,
    pub context: HashMap<String, AwsConditionValue>,
}

impl Request {
    pub fn new(action: &'static str, resource: impl Into<String>) -> Self {
        Self {
            action,
            resource: resource.into(),
            context: HashMap::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context
            .insert(key.into().to_ascii_lowercase(), value.into().into());
        self
    }

    /// Supplies a multivalued request key, including an explicitly empty dataset.
    pub fn with_values(mut self, key: &str, values: &[&str]) -> Self {
        self.context.insert(
            key.to_ascii_lowercase(),
            AwsConditionValue::List(values.iter().map(|value| (*value).to_owned()).collect()),
        );
        self
    }

    /// Adds the tags a resource already carries as `aws:ResourceTag/*` keys.
    pub fn resource_tags(mut self, tags: &[(&str, &str)]) -> Self {
        self.add_tags("aws:ResourceTag", tags);
        self
    }

    /// Adds request tags and their case-preserving `aws:TagKeys` values.
    pub fn request_tags(mut self, tags: &[(&str, &str)]) -> Self {
        self.add_tags("aws:RequestTag", tags);
        self.with_values(
            "aws:TagKeys",
            &tags.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        )
    }

    fn add_tags(&mut self, prefix: &str, tags: &[(&str, &str)]) {
        for (key, value) in tags {
            assert!(
                self.context
                    .insert(
                        format!("{prefix}/{key}").to_ascii_lowercase(),
                        (*value).into()
                    )
                    .is_none(),
                "the evaluator does not model duplicate tag keys, including case variants"
            );
        }
    }
}

/// IAM's ARN and action wildcard: `*` matches any run of characters, `?` one character.
pub fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut matches = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    matches[0][0] = true;
    for p in 1..=pattern.len() {
        if pattern[p - 1] == '*' {
            matches[p][0] = matches[p - 1][0];
        }
        for v in 1..=value.len() {
            matches[p][v] = match pattern[p - 1] {
                '*' => matches[p - 1][v] || matches[p][v - 1],
                '?' => matches[p - 1][v - 1],
                c => matches[p - 1][v - 1] && c == value[v - 1],
            };
        }
    }
    matches[pattern.len()][value.len()]
}

fn values(value: &AwsConditionValue) -> &[String] {
    match value {
        AwsConditionValue::String(value) => std::slice::from_ref(value),
        AwsConditionValue::List(values) => values,
    }
}

fn condition_holds(
    operator: &str,
    expected: &AwsConditionValue,
    actual: Option<&AwsConditionValue>,
) -> bool {
    let expected = values(expected);
    assert!(
        !expected.is_empty(),
        "empty policy condition values are not modeled"
    );
    let actual_values = actual.map(values).unwrap_or_default();
    let is_null = actual_values.iter().all(String::is_empty);
    if operator == "Null" {
        assert_eq!(expected.len(), 1, "Null requires one boolean value");
        return match expected[0].as_str() {
            "true" => is_null,
            "false" => !is_null,
            other => panic!("Null takes \"true\" or \"false\", not {other}"),
        };
    }
    let (qualifier, comparison) = operator.split_once(':').unwrap_or(("", operator));
    let negated = match comparison {
        "StringEquals" | "StringLike" | "Bool" => false,
        "StringNotEquals" | "StringNotLike" => true,
        other => panic!("the evaluator doesn't model condition operator {other}"),
    };
    assert!(
        qualifier.is_empty() || comparison != "Bool",
        "set-qualified Bool is not modeled"
    );
    let matches = |actual: &String| {
        let any = expected.iter().any(|expected| match comparison {
            "StringLike" | "StringNotLike" => wildcard_match(expected, actual),
            _ => expected == actual,
        });
        if negated {
            !any
        } else {
            any
        }
    };
    match qualifier {
        "ForAllValues" => is_null || actual_values.iter().all(matches),
        "ForAnyValue" => !is_null && actual_values.iter().any(matches),
        "" => {
            assert!(
                !matches!(actual, Some(AwsConditionValue::List(_))),
                "multivalued request keys require a set operator"
            );
            actual_values.first().map_or(negated, matches)
        }
        other => panic!("the evaluator doesn't model condition qualifier {other}"),
    }
}

fn conditions_hold(statement: &AwsIamStatement, request: &Request) -> bool {
    let Some(conditions) = &statement.condition else {
        return true;
    };
    conditions.iter().all(|(operator, entries)| {
        entries.iter().all(|(key, expected)| {
            condition_holds(
                operator,
                expected,
                request.context.get(&key.to_ascii_lowercase()),
            )
        })
    })
}

fn statement_matches(statement: &AwsIamStatement, request: &Request) -> bool {
    if !statement
        .action
        .iter()
        .any(|action| wildcard_match(&action.to_lowercase(), &request.action.to_lowercase()))
    {
        return false;
    }
    assert!(
        statement.resource.is_empty() != statement.not_resource.is_empty(),
        "a statement must specify Resource or NotResource exclusively ({})",
        statement.sid
    );
    let resource_matches = if statement.not_resource.is_empty() {
        statement
            .resource
            .iter()
            .any(|resource| wildcard_match(resource, &request.resource))
    } else {
        !statement
            .not_resource
            .iter()
            .any(|resource| wildcard_match(resource, &request.resource))
    };
    resource_matches && conditions_hold(statement, request)
}

pub fn evaluate(statements: &[AwsIamStatement], request: &Request) -> Decision {
    let matching: Vec<_> = statements
        .iter()
        .filter(|statement| statement_matches(statement, request))
        .collect();
    if matching.iter().any(|statement| statement.effect == "Deny") {
        Decision::ExplicitlyDenied
    } else if matching.iter().any(|statement| statement.effect == "Allow") {
        Decision::Allowed
    } else {
        Decision::NotAllowed
    }
}
