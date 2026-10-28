use super::*;
use hcl::eval::{Context as EvalContext, Evaluate};
use serde_json::{json, Value};

fn statement(index: usize, resources: usize) -> AwsIamStatement {
    AwsIamStatement {
        sid: format!("Read{index}"),
        effect: "Allow".into(),
        action: vec!["s3:GetObject".into()],
        resource: (0..resources)
            .map(|resource| {
                format!("arn:aws:s3:::${{local.resource_prefix}}-bucket-{index}-{resource}/*")
            })
            .collect(),
        not_resource: Vec::new(),
        condition: None,
    }
}

fn resolve_policy(expression: &Expression) -> Value {
    let rendered =
        hcl::to_string(&hcl::Body::from(vec![attr("policy", expression.clone())])).unwrap();
    let body: hcl::Body = hcl::from_str(&rendered).unwrap();
    let Expression::FuncCall(call) = body.attributes().next().unwrap().expr() else {
        panic!("policy must use jsonencode");
    };
    assert_eq!(call.name.name.as_str(), "jsonencode");
    let mut context = EvalContext::new();
    for (name, value) in [
        ("local", json!({"resource_prefix": "p".repeat(40)})),
        (
            "data",
            json!({"aws_region": {"current": {"region": "ap-southeast-1234"}}, "aws_caller_identity": {"current": {"account_id": "123456789012"}}}),
        ),
        (
            "var",
            json!({"managing_account_id": "210987654321", "managing_role_arn": "arn:aws:iam::210987654321:role/manager"}),
        ),
    ] {
        context.declare_var(name, hcl::to_value(value).unwrap());
    }
    serde_json::to_value(call.args[0].evaluate(&context).unwrap()).unwrap()
}

#[test]
fn chunks_fit_after_prefix_expansion_and_preserve_every_statement() {
    let statements: Vec<_> = (0..5).map(|index| statement(index, 17)).collect();
    assert!(policy_document_json(&statements).unwrap().to_string().len() < 5_500);
    let expected = resolve_policy(&policy_document_expr(statements.clone()).unwrap());
    assert!(expected.to_string().len() > 6_144);
    let chunks = chunk_iam_statements(statements, 5_500).unwrap();
    assert!(chunks.len() > 1);
    let mut actual = Vec::new();
    for chunk in chunks {
        let policy = resolve_policy(&policy_document_expr(chunk).unwrap());
        assert!(policy.to_string().len() <= 6_144);
        actual.extend(policy["Statement"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(actual, *expected["Statement"].as_array().unwrap());
}

#[test]
fn rejects_oversized_statements_even_after_a_valid_chunk() {
    let oversized = statement(1, 90);
    assert!(
        policy_document_json(std::slice::from_ref(&oversized))
            .unwrap()
            .to_string()
            .len()
            < 5_500
    );
    assert!(
        resolve_policy(&policy_document_expr(vec![oversized.clone()]).unwrap())
            .to_string()
            .len()
            > 6_144
    );
    for statements in [vec![oversized.clone()], vec![statement(0, 1), oversized]] {
        let error = chunk_iam_statements(statements, 5_500).unwrap_err();
        assert!(error.to_string().contains("too large for a managed policy"));
    }
}

#[test]
fn bounded_catalog_chunks_preserve_conditions_and_fit_the_attachment_budget() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let context = aws_terraform_permission_context().with_aws_permissions_boundary_arn(
        "arn:aws:iam::${data.aws_caller_identity.current.account_id}:policy/${local.resource_prefix}-permissions-boundary",
    );
    let mut statements = Vec::new();
    for id in alien_permissions::list_permission_set_ids() {
        if !id.ends_with("/provision") || id.starts_with("remote-stack-management/") {
            continue;
        }
        let set = alien_permissions::get_permission_set(id).unwrap();
        if set.platforms.aws.is_some() {
            statements.extend(
                generator
                    .generate_policy(set, BindingTarget::Stack, &context)
                    .unwrap()
                    .statement,
            );
        }
    }
    ensure_unique_statement_sids(&mut statements);
    let expected = resolve_policy(&policy_document_expr(statements.clone()).unwrap());
    let mut fragment = TfFragment::default();
    fragment
        .resource_blocks
        .push(resource_block("aws_iam_role", "management", []));
    emit_iam_managed_policy_chunks(
        &mut fragment,
        "management",
        "policies",
        "management",
        statements,
    )
    .unwrap();
    let mut actual = Vec::new();
    let mut policy_count = 0;
    for block in &fragment.resource_blocks {
        if block.labels()[0].as_str() != "aws_iam_policy" {
            continue;
        }
        policy_count += 1;
        let policy = resolve_policy(
            block
                .body
                .attributes()
                .find(|attr| attr.key.as_str() == "policy")
                .unwrap()
                .expr(),
        );
        assert!(policy.to_string().len() <= 6_144);
        actual.extend(policy["Statement"].as_array().unwrap().iter().cloned());
    }
    assert!((2..=20).contains(&policy_count));
    crate::iam_limits::validate_role_policy_limits(&indexmap::IndexMap::from([(
        "management".to_string(),
        fragment,
    )]))
    .unwrap();
    assert_eq!(actual, *expected["Statement"].as_array().unwrap());
    let writes: Vec<_> = actual
        .iter()
        .filter(|statement| {
            statement["Action"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action == "iam:CreateRole")
        })
        .collect();
    assert!(!writes.is_empty());
    for statement in writes {
        assert_eq!(
            statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
            format!(
                "arn:aws:iam::123456789012:policy/{}-permissions-boundary",
                "p".repeat(40)
            )
        );
    }
}
