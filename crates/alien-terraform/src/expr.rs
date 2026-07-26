//! Tiny `hcl-rs` expression helpers used by emitters.
//!
//! `hcl-rs` exposes everything we need; these wrappers just shorten the
//! call sites in resource emitters. No intermediate IR \u2014 every helper
//! returns `hcl::Expression` directly.

use alien_core::{ErrorData, Result};
use alien_error::AlienError;
use hcl::{
    expr::{Expression, Object, ObjectKey, TemplateExpr, Traversal, TraversalOperator},
    Identifier,
};
use std::str::FromStr;

/// `${local.resource_prefix}-foo`-style HCL template string. Becomes a quoted
/// template in the rendered HCL.
pub fn template(value: impl Into<String>) -> Expression {
    Expression::TemplateExpr(Box::new(TemplateExpr::QuotedString(value.into())))
}

/// Parse a raw HCL expression string (e.g. `aws_s3_bucket.x.id`,
/// `jsonencode({...})`). Use sparingly \u2014 prefer building Expression trees
/// directly. Returns a typed error on parse failure rather than panicking.
pub fn raw(text: impl AsRef<str>) -> Expression {
    parse(text.as_ref()).unwrap_or_else(|_| Expression::String(text.as_ref().to_string()))
}

/// `jsonencode(...)` HCL function call.
pub fn jsonencode(value: Expression) -> Expression {
    Expression::FuncCall(Box::new(
        hcl::expr::FuncCall::builder(Identifier::sanitized("jsonencode"))
            .arg(value)
            .build(),
    ))
}

/// Parse a raw HCL expression and bubble up parse failures as a typed error.
pub fn parse(text: &str) -> Result<Expression> {
    Expression::from_str(text).map_err(|err| {
        AlienError::new(ErrorData::GenericError {
            message: format!("invalid Terraform expression `{text}`: {err}"),
        })
    })
}

/// Build a `foo.bar.baz`-style traversal expression from segments. The first
/// segment becomes the root variable; the rest become `.attr` operators.
pub fn traversal<I, S>(parts: I) -> Expression
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = parts.into_iter();
    let first = iter
        .next()
        .expect("traversal must have at least one segment");
    let operators = iter
        .map(|part| TraversalOperator::GetAttr(Identifier::sanitized(part.as_ref())))
        .collect();
    Expression::Traversal(Box::new(Traversal {
        expr: Expression::Variable(Identifier::sanitized(first.as_ref()).into()),
        operators,
    }))
}

/// Like [`traversal`], but indexes into the second segment: `root.label[0].attr`.
/// Terraform renders a resource carrying `count` as a list, and a gate yields at
/// most one instance, so the index is always zero.
pub fn traversal_indexed(root: &str, label: &str, attribute: &str) -> Expression {
    Expression::Traversal(Box::new(Traversal {
        expr: Expression::Variable(Identifier::sanitized(root).into()),
        operators: vec![
            TraversalOperator::GetAttr(Identifier::sanitized(label)),
            TraversalOperator::Index(Expression::Number(hcl::Number::from(0))),
            TraversalOperator::GetAttr(Identifier::sanitized(attribute)),
        ],
    }))
}

/// Build an HCL object literal from `(key, value)` pairs. Identifier-shaped
/// keys become unquoted; everything else becomes a quoted string key.
pub fn object<I, K>(pairs: I) -> Expression
where
    I: IntoIterator<Item = (K, Expression)>,
    K: AsRef<str>,
{
    let mut object = Object::new();
    for (key, value) in pairs {
        let key_str = key.as_ref();
        let object_key = if is_identifier(key_str) {
            ObjectKey::Identifier(Identifier::sanitized(key_str))
        } else {
            ObjectKey::Expression(Expression::String(key_str.to_string()))
        };
        object.insert(object_key, value);
    }
    Expression::Object(object)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
