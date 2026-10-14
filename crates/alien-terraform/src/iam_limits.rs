//! Definite IAM role quota overflows in the final emitted module.

use crate::emitter::TfFragment;
use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use hcl::{
    expr::{Expression, ObjectKey, TraversalOperator},
    structure::{Block, Structure},
};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Conditional resources and unresolved expressions are excluded from the
/// lower bound. Attachments count distinct literal ARNs and references to
/// unconditional policies created here. Those two sets may alias, so their
/// maximum is a conservative lower bound. Other dynamic identities are excluded.
pub(crate) fn validate_role_policy_limits(fragments: &IndexMap<String, TfFragment>) -> Result<()> {
    let blocks: Vec<_> = fragments
        .values()
        .flat_map(|f| &f.resource_blocks)
        .collect();
    let roles: HashSet<_> = blocks
        .iter()
        .filter(|b| resource_type(b) == Some("aws_iam_role") && !conditional(b))
        .filter_map(|b| b.labels.get(1).map(|label| label.as_str()))
        .collect();
    let policies: HashSet<_> = blocks
        .iter()
        .filter(|b| resource_type(b) == Some("aws_iam_policy") && !conditional(b))
        .filter_map(|b| b.labels.get(1).map(|label| label.as_str()))
        .collect();
    let mut sizes: IndexMap<String, usize> = IndexMap::new();
    let mut attachments: IndexMap<String, HashSet<PolicyIdentity>> = IndexMap::new();
    for block in blocks.iter().filter(|b| !conditional(b)) {
        match resource_type(block) {
            Some("aws_iam_role") => {
                let Some(label) = block.labels.get(1).map(|label| label.as_str()) else {
                    continue;
                };
                for nested in block
                    .body
                    .blocks()
                    .filter(|b| b.identifier.as_str() == "inline_policy")
                {
                    if let Some(policy) = attribute(nested, "policy") {
                        *sizes.entry(label.to_string()).or_default() += policy_size(policy)?;
                    }
                }
                if let Some(Expression::Array(arns)) = attribute(block, "managed_policy_arns") {
                    for arn in arns {
                        if let Some(identity) = policy_identity(arn, &policies) {
                            attachments
                                .entry(label.to_string())
                                .or_default()
                                .insert(identity);
                        }
                    }
                }
            }
            Some("aws_iam_role_policy" | "aws_iam_role_policy_attachment") => {
                let Some(role) = attribute(block, "role").and_then(role_label) else {
                    continue;
                };
                if !roles.contains(role) {
                    continue;
                }
                if resource_type(block) == Some("aws_iam_role_policy") {
                    if let Some(policy) = attribute(block, "policy") {
                        *sizes.entry(role.to_string()).or_default() += policy_size(policy)?;
                    }
                } else {
                    if let Some(identity) = attribute(block, "policy_arn")
                        .and_then(|value| policy_identity(value, &policies))
                    {
                        attachments
                            .entry(role.to_string())
                            .or_default()
                            .insert(identity);
                    }
                }
            }
            _ => {}
        }
    }
    let mut shortfalls = Vec::new();
    for (role, size) in sizes {
        if size > 10_240 {
            shortfalls.push(format!("IAM role '{role}' requires at least {size} non-whitespace inline-policy characters; AWS allows 10240 across all inline policies on one role. Reduce permissions or move grants to managed policies."));
        }
    }
    for (role, arns) in attachments {
        let literal_count = arns
            .iter()
            .filter(|id| matches!(id, PolicyIdentity::Arn(_)))
            .count();
        let count = literal_count.max(arns.len() - literal_count);
        if count > 20 {
            shortfalls.push(format!("IAM role '{role}' requires at least {count} managed-policy attachments; AWS allows at most 20 per role. Reduce the number of attached policies."));
        }
    }
    if shortfalls.is_empty() {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_terraform_module".into(),
            reason: shortfalls.join("\n"),
        }))
    }
}

#[derive(PartialEq, Eq, Hash)]
enum PolicyIdentity {
    Arn(String),
    Resource(String),
}

fn policy_identity(expression: &Expression, policies: &HashSet<&str>) -> Option<PolicyIdentity> {
    if let Expression::String(arn) = expression {
        return Some(PolicyIdentity::Arn(arn.clone()));
    }
    let Expression::Traversal(traversal) = expression else {
        return None;
    };
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    match traversal.operators.as_slice() {
        [TraversalOperator::GetAttr(label), TraversalOperator::GetAttr(attr)]
            if root.as_str() == "aws_iam_policy"
                && attr.as_str() == "arn"
                && policies.contains(label.as_str()) =>
        {
            Some(PolicyIdentity::Resource(label.to_string()))
        }
        _ => None,
    }
}

fn resource_type(block: &Block) -> Option<&str> {
    (block.identifier.as_str() == "resource")
        .then(|| block.labels.first().map(|label| label.as_str()))
        .flatten()
}

fn attribute<'a>(block: &'a Block, name: &str) -> Option<&'a Expression> {
    block.body.0.iter().find_map(|structure| match structure {
        Structure::Attribute(attr) if attr.key.as_str() == name => Some(&attr.expr),
        _ => None,
    })
}

fn conditional(block: &Block) -> bool {
    attribute(block, "count").is_some() || attribute(block, "for_each").is_some()
}

fn role_label(expression: &Expression) -> Option<&str> {
    let Expression::Traversal(traversal) = expression else {
        return None;
    };
    let Expression::Variable(root) = &traversal.expr else {
        return None;
    };
    if root.as_str() != "aws_iam_role" {
        return None;
    }
    match traversal.operators.as_slice() {
        [TraversalOperator::GetAttr(label), TraversalOperator::GetAttr(attr)]
            if matches!(attr.as_str(), "name" | "id") =>
        {
            Some(label.as_str())
        }
        _ => None,
    }
}

fn policy_size(expression: &Expression) -> Result<usize> {
    match expression {
        Expression::String(document) => Ok(document.chars().filter(|c| !c.is_whitespace()).count()),
        Expression::FuncCall(call)
            if call.name.to_string() == "jsonencode"
                && call.args.len() == 1
                && !call.expand_final =>
        {
            json_size_lower_bound(&call.args[0])
        }
        _ => Ok(0),
    }
}

fn string_size(value: &str) -> Result<usize> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".into(),
            reason: "Failed to measure IAM policy string".into(),
        })
        .map(|json| json.chars().filter(|c| !c.is_whitespace()).count())
}

/// Count JSON structure and literals, never serialized HCL syntax. Dynamic
/// strings, functions and comprehensions contribute zero rather than guessing
/// their resolved size. Static object keys cannot disappear during jsonencode.
fn json_size_lower_bound(expression: &Expression) -> Result<usize> {
    match expression {
        Expression::String(value) => string_size(value),
        Expression::Null => Ok(4),
        Expression::Bool(value) => Ok(if *value { 4 } else { 5 }),
        Expression::Number(_) => Ok(1),
        Expression::Array(values) => {
            let mut size = 2 + values.len().saturating_sub(1);
            for value in values {
                size += json_size_lower_bound(value)?;
            }
            Ok(size)
        }
        Expression::Object(values) => {
            let mut size = 2 + values.len().saturating_sub(1);
            let mut keys = HashSet::new();
            for (key, value) in values {
                let key = match key {
                    ObjectKey::Identifier(key) => key.as_str(),
                    ObjectKey::Expression(Expression::String(key)) => key.as_str(),
                    _ => return Ok(0),
                };
                if !keys.insert(key) {
                    return Ok(0);
                }
                size += string_size(key)? + 1 + json_size_lower_bound(value)?;
            }
            Ok(size)
        }
        _ => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::{json_size_lower_bound, validate_role_policy_limits};
    use crate::{
        block::{attr, block, nested, resource_block},
        expr, generate_terraform_module, TerraformOptions, TerraformTarget, TfFragment, TfRegistry,
    };
    use alien_core::{PermissionSet, ResourceLifecycle, ServiceAccount, Stack, StackSettings};
    use alien_permissions::generators::AwsIamStatement;
    use hcl::expr::Expression;
    use indexmap::IndexMap;

    fn role() -> hcl::Block {
        resource_block("aws_iam_role", "execution", [])
    }

    fn policy(label: &str, size: usize) -> hcl::Block {
        resource_block(
            "aws_iam_role_policy",
            label,
            [
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", "execution", "name"]),
                ),
                attr("policy", Expression::String("x".repeat(size))),
            ],
        )
    }

    fn fragments(blocks: Vec<hcl::Block>) -> IndexMap<String, TfFragment> {
        let mut fragment = TfFragment::default();
        fragment.resource_blocks = blocks;
        IndexMap::from([("test".into(), fragment)])
    }

    #[test]
    fn aggregates_embedded_and_external_policies_and_accepts_exact_boundary() {
        let mut role = role();
        role.body.0.push(nested(block(
            "inline_policy",
            [attr("policy", Expression::String("x".repeat(5000)))],
        )));
        let good = fragments(vec![role.clone(), policy("external", 5240)]);
        validate_role_policy_limits(&good).unwrap();
        let error = validate_role_policy_limits(&fragments(vec![role, policy("external", 5241)]))
            .expect_err("aggregate exceeds quota although neither document does");
        assert!(error.to_string().contains("execution"));
        assert!(error.to_string().contains("10241"));
    }

    #[test]
    fn excludes_conditional_policies_and_roles() {
        let mut conditional_policy = policy("optional", 20000);
        conditional_policy
            .body
            .0
            .push(attr("count", expr::raw("var.enabled ? 1 : 0")));
        validate_role_policy_limits(&fragments(vec![role(), conditional_policy])).unwrap();
        let mut conditional_role = role();
        conditional_role
            .body
            .0
            .push(attr("for_each", expr::raw("var.roles")));
        validate_role_policy_limits(&fragments(vec![
            conditional_role,
            policy("external", 20000),
        ]))
        .unwrap();
    }

    #[test]
    fn attachment_absolute_maximum_and_multiple_shortfalls() {
        let mut blocks = vec![role(), policy("external", 10241)];
        for index in 0..20 {
            blocks.push(resource_block(
                "aws_iam_role_policy_attachment",
                &format!("grant{index}"),
                [
                    attr("role", expr::traversal(["aws_iam_role", "execution", "id"])),
                    attr(
                        "policy_arn",
                        Expression::String(format!("arn:aws:iam::aws:policy/grant{index}")),
                    ),
                ],
            ));
        }
        let error = validate_role_policy_limits(&fragments(blocks.clone())).unwrap_err();
        assert!(!error.to_string().contains("managed-policy attachments"));
        blocks.push(resource_block(
            "aws_iam_role_policy_attachment",
            "grant20",
            [
                attr("role", expr::traversal(["aws_iam_role", "execution", "id"])),
                attr(
                    "policy_arn",
                    Expression::String("arn:aws:iam::aws:policy/grant20".into()),
                ),
            ],
        ));
        let error = validate_role_policy_limits(&fragments(blocks)).unwrap_err();
        assert!(error.to_string().contains("10241"));
        assert!(error.to_string().contains("21 managed-policy attachments"));
    }

    #[test]
    fn generated_managed_policy_chunks_obey_attachment_limit() {
        for count in [20, 21] {
            let mut fragment = TfFragment::default();
            fragment.resource_blocks.push(role());
            crate::emitters::aws::helpers::emit_iam_managed_policy_chunks(
                &mut fragment,
                "execution",
                "management",
                "q5-management",
                (0..count)
                    .map(|index| AwsIamStatement {
                        sid: format!("Grant{index}"),
                        effect: "Allow".into(),
                        action: vec!["s3:GetObject".into()],
                        resource: vec![format!("arn:aws:s3:::q5-managed/{}", "x".repeat(4500))],
                        not_resource: Vec::new(),
                        condition: None,
                    })
                    .collect(),
            )
            .unwrap();
            assert_eq!(fragment.resource_blocks.len(), 1 + count * 2);
            // A duplicate reference and an unresolved external expression must
            // not inflate the number of known distinct attachments.
            for (label, arn) in [
                (
                    "duplicate",
                    expr::traversal(["aws_iam_policy", "management_0", "arn"]),
                ),
                ("unknown", expr::raw("var.policy_arn")),
                (
                    "missing",
                    expr::traversal(["aws_iam_policy", "absent", "arn"]),
                ),
            ] {
                fragment.resource_blocks.push(resource_block(
                    "aws_iam_role_policy_attachment",
                    label,
                    [
                        attr(
                            "role",
                            expr::traversal(["aws_iam_role", "execution", "name"]),
                        ),
                        attr("policy_arn", arn),
                    ],
                ));
            }
            let result =
                validate_role_policy_limits(&IndexMap::from([("generated".into(), fragment)]));
            if count == 20 {
                result.expect("twenty generated policies fit");
            } else {
                let error = result.expect_err("twenty-one chunks must fail before rendering");
                assert!(error.to_string().contains("21 managed-policy attachments"));
            }
        }
    }

    #[test]
    fn measures_json_literals_and_never_counts_expression_syntax() {
        let document = expr::object([
            ("Version", Expression::String("2012-10-17".into())),
            (
                "Statement",
                Expression::Array(vec![expr::object([
                    ("Effect", Expression::String("Allow".into())),
                    (
                        "Resource",
                        Expression::String("arn:aws:s3:::q5-quote/\"key\"".into()),
                    ),
                ])]),
            ),
        ]);
        let resolved = serde_json::json!({"Version":"2012-10-17", "Statement":[{"Effect":"Allow", "Resource":"arn:aws:s3:::q5-quote/\"key\""}]});
        assert_eq!(
            json_size_lower_bound(&document).unwrap(),
            resolved.to_string().chars().count()
        );
        let dynamic = expr::object([("Resource", expr::template("${var.long_expression_name}"))]);
        assert!(json_size_lower_bound(&dynamic).unwrap() <= "{\"Resource\":\"\"}".len());
        assert_eq!(
            json_size_lower_bound(&expr::raw("[for grant in var.grants : grant]")).unwrap(),
            0
        );
    }

    #[test]
    fn built_in_generation_rejects_oversized_service_account_and_renders_control() {
        for (count, should_pass) in [(10, true), (400, false)] {
            let resources: Vec<_> = (0..count)
                .map(|index| format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}"))
                .collect();
            let set: PermissionSet = serde_json::from_value(serde_json::json!({
                "id": "q5-policy-read", "description": "Read explicit test objects",
                "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]}, "binding": {"stack": {"resources": resources}}}]}
            })).unwrap();
            let mut account = ServiceAccount::new("execution-sa".into()).build();
            account.stack_permission_sets.push(set);
            let stack = Stack::new("q5-policy-control".into())
                .add(account, ResourceLifecycle::Frozen)
                .build();
            let registry = TfRegistry::built_in();
            let result = generate_terraform_module(
                &stack,
                TerraformTarget::Aws,
                TerraformOptions {
                    registry: &registry,
                    display_name: None,
                    stack_settings: StackSettings::default(),
                    registration: None,
                    helm_install: None,
                    supported_aws_regions: Vec::new(),
                },
            );
            if should_pass {
                result.expect("small policy must render");
            } else {
                let error =
                    result.expect_err("oversized policy must fail before files are returned");
                assert!(error.to_string().contains("execution_sa"));
                assert!(error.to_string().contains("10240"));
            }
        }
    }
}
