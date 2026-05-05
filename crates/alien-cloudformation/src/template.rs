//! CloudFormation template IR.
//!
//! These types live in this crate (not `alien-core`) because they're CF-shaped
//! \u2014 PascalCase serde, intrinsic-function-aware expressions \u2014 and only this
//! crate's generator + emitters use them. Keep `alien-core` agnostic of any
//! one distribution format.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Complete CloudFormation template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CfTemplate {
    #[serde(rename = "AWSTemplateFormatVersion")]
    pub aws_template_format_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transform: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub metadata: IndexMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, CfParameter>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub conditions: IndexMap<String, CfExpression>,
    #[serde(default)]
    pub resources: IndexMap<String, CfResource>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub outputs: IndexMap<String, CfOutput>,
}

/// CloudFormation parameter declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CfParameter {
    #[serde(rename = "Type")]
    pub parameter_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<CfExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_values: Option<Vec<CfExpression>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_echo: Option<bool>,
}

/// CloudFormation resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CfResource {
    #[serde(skip)]
    pub logical_id: String,
    #[serde(rename = "Type")]
    pub resource_type: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub properties: IndexMap<String, CfExpression>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub metadata: IndexMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_replace_policy: Option<String>,
}

impl CfResource {
    pub fn new(logical_id: String, resource_type: String) -> Self {
        Self {
            logical_id,
            resource_type,
            properties: IndexMap::new(),
            depends_on: vec![],
            condition: None,
            metadata: IndexMap::new(),
            deletion_policy: None,
            update_replace_policy: None,
        }
    }
}

/// CloudFormation output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CfOutput {
    pub value: CfExpression,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<IndexMap<String, CfExpression>>,
}

/// CloudFormation expression \u2014 literal, list, object, or intrinsic function.
///
/// Untagged enum so YAML/JSON serialization mirrors hand-written CFN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CfExpression {
    Null,
    Bool(bool),
    Integer(i64),
    Number(f64),
    String(String),
    List(Vec<CfExpression>),
    Object(IndexMap<String, CfExpression>),
}

impl CfExpression {
    pub fn list<I>(items: I) -> Self
    where
        I: IntoIterator<Item = CfExpression>,
    {
        Self::List(items.into_iter().collect())
    }

    pub fn object<I, K>(items: I) -> Self
    where
        I: IntoIterator<Item = (K, CfExpression)>,
        K: Into<String>,
    {
        Self::Object(
            items
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub fn ref_(logical_id: impl Into<String>) -> Self {
        Self::object([("Ref", Self::String(logical_id.into()))])
    }

    pub fn get_att(logical_id: impl Into<String>, attribute: impl Into<String>) -> Self {
        Self::object([(
            "Fn::GetAtt",
            Self::List(vec![
                Self::String(logical_id.into()),
                Self::String(attribute.into()),
            ]),
        )])
    }

    pub fn sub(template: impl Into<String>) -> Self {
        Self::object([("Fn::Sub", Self::String(template.into()))])
    }

    pub fn equals(left: CfExpression, right: CfExpression) -> Self {
        Self::object([("Fn::Equals", Self::List(vec![left, right]))])
    }

    pub fn not(condition: CfExpression) -> Self {
        Self::object([("Fn::Not", Self::List(vec![condition]))])
    }

    pub fn and<I>(conditions: I) -> Self
    where
        I: IntoIterator<Item = CfExpression>,
    {
        Self::object([("Fn::And", Self::List(conditions.into_iter().collect()))])
    }

    pub fn or<I>(conditions: I) -> Self
    where
        I: IntoIterator<Item = CfExpression>,
    {
        Self::object([("Fn::Or", Self::List(conditions.into_iter().collect()))])
    }

    pub fn if_(
        condition_name: impl Into<String>,
        when_true: CfExpression,
        when_false: CfExpression,
    ) -> Self {
        Self::object([(
            "Fn::If",
            Self::List(vec![
                Self::String(condition_name.into()),
                when_true,
                when_false,
            ]),
        )])
    }

    pub fn to_json_string(value: CfExpression) -> Self {
        Self::object([("Fn::ToJsonString", value)])
    }

    pub fn no_value() -> Self {
        Self::ref_("AWS::NoValue")
    }
}

impl From<&str> for CfExpression {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfExpression {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<bool> for CfExpression {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<u8> for CfExpression {
    fn from(value: u8) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<u32> for CfExpression {
    fn from(value: u32) -> Self {
        Self::Integer(i64::from(value))
    }
}
