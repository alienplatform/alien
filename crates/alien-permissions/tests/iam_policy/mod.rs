//! A small model of IAM policy evaluation for tests: an explicit Deny wins, otherwise any
//! matching Allow allows, otherwise the request is denied. It models only the condition
//! operators the permission sets use and panics on any other, so a test never passes on an
//! operator it does not understand.
#![allow(dead_code)]

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
    pub context: HashMap<String, String>,
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
        self.context.insert(key.into(), value.into());
        self
    }

    /// Adds the tags a resource already carries as `aws:ResourceTag/*` keys.
    pub fn resource_tags(mut self, tags: &[(&str, &str)]) -> Self {
        for (key, value) in tags {
            self.context
                .insert(format!("aws:ResourceTag/{key}"), value.to_string());
        }
        self
    }

    /// Adds the tags a request writes as `aws:RequestTag/*` keys.
    pub fn request_tags(mut self, tags: &[(&str, &str)]) -> Self {
        for (key, value) in tags {
            self.context
                .insert(format!("aws:RequestTag/{key}"), value.to_string());
        }
        self
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

fn conditions_hold(statement: &AwsIamStatement, request: &Request) -> bool {
    let Some(conditions) = &statement.condition else {
        return true;
    };
    conditions.iter().all(|(operator, entries)| {
        entries.iter().all(|(key, expected)| {
            let actual = request.context.get(key);
            match operator.as_str() {
                "StringEquals" | "Bool" => actual == Some(expected),
                // A negated operator holds when the key is absent.
                "StringNotEquals" => actual != Some(expected),
                "StringLike" => actual.is_some_and(|actual| wildcard_match(expected, actual)),
                // "true" holds when the key is absent from the request, "false" when present.
                "Null" => match expected.as_str() {
                    "true" => actual.is_none(),
                    "false" => actual.is_some(),
                    other => panic!("Null takes \"true\" or \"false\", not {other}"),
                },
                "ForAllValues:StringEquals" => {
                    panic!("the evaluator doesn't model {operator}; no request here sets tag keys")
                }
                other => panic!("the evaluator doesn't model condition operator {other}"),
            }
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
