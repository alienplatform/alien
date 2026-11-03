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
fn chunking_preserves_expanded_policies_and_rejects_unsplittable_statements() {
    for (name, resource_counts, expected_chunks) in [
        ("empty", vec![], Some(0)),
        ("single", vec![1], Some(1)),
        ("expanded", vec![17; 5], Some(2)),
        ("oversized first", vec![90, 1], None),
        ("oversized alone", vec![90], None),
        ("oversized last", vec![1, 90], None),
        (
            "oversized after flushed chunk",
            vec![17, 17, 17, 17, 90],
            None,
        ),
    ] {
        let statements: Vec<_> = resource_counts
            .into_iter()
            .enumerate()
            .map(|(index, resources)| statement(index, resources))
            .collect();
        let expected = resolve_policy(&policy_document_expr(statements.clone()).unwrap());
        if name == "expanded" {
            assert!(policy_document_json(&statements).unwrap().to_string().len() < 5_500);
            assert!(expected.to_string().len() > 6_144);
        }
        let result = chunk_iam_statements(statements.clone(), 5_500);
        let Some(expected_chunks) = expected_chunks else {
            let oversized = statements.iter().find(|s| s.resource.len() == 90).unwrap();
            assert!(
                policy_document_json(std::slice::from_ref(oversized))
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
            let error = result.expect_err(name).to_string();
            assert!(
                error.contains("too large for a managed policy"),
                "{name}: {error}"
            );
            assert!(error.contains(&oversized.sid), "{name}: {error}");
            continue;
        };
        let chunks = result.unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(chunks.len(), expected_chunks, "{name}");
        let mut actual = Vec::new();
        for chunk in chunks {
            let policy = resolve_policy(&policy_document_expr(chunk).unwrap());
            assert!(policy.to_string().len() <= 6_144, "{name}: {policy}");
            actual.extend(policy["Statement"].as_array().unwrap().iter().cloned());
        }
        assert_eq!(actual, *expected["Statement"].as_array().unwrap(), "{name}");
    }
}

#[test]
fn bounded_catalog_chunks_preserve_conditions_and_fit_the_attachment_budget() {
    let generator = AwsRuntimePermissionsGenerator::new();
    let context = aws_terraform_permission_context().with_aws_permissions_boundary_arn(
        "arn:aws:iam::${data.aws_caller_identity.current.account_id}:policy/${local.resource_prefix}-deployment-management-boundary",
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
    crate::iam_limits::RenderedModule::new(fragment.resource_blocks, indexmap::IndexMap::new())
        .check()
        .unwrap();
    // Merged and packed, the policies grant and deny exactly what the statements did.
    let parsed = |statements: &[serde_json::Value]| -> Vec<AwsIamStatement> {
        statements
            .iter()
            .map(|statement| serde_json::from_value(statement.clone()).unwrap())
            .collect()
    };
    assert_eq!(
        expanded(&parsed(&actual)),
        expanded(&parsed(expected["Statement"].as_array().unwrap()))
    );
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
                "arn:aws:iam::123456789012:policy/{}-deployment-management-boundary",
                "p".repeat(40)
            )
        );
    }
}

/// The statements of an automatic-setup management role (every provision set), as rendered for
/// a release whose grants need 19 policies with a split in statement order: with the setup
/// policy and the permissions boundary that is 21 attachments, over the default quota of 20.
fn automatic_management_statements() -> Vec<AwsIamStatement> {
    serde_json::from_str(include_str!(
        "testdata/automatic-management-statements.json"
    ))
    .unwrap()
}

/// JSON with object keys sorted, so equal conditions written in another key order compare equal.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<&String, String> = map
                .iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect();
            format!("{sorted:?}")
        }
        Value::Array(values) => format!("{:?}", values.iter().map(canonical).collect::<Vec<_>>()),
        value => value.to_string(),
    }
}

/// Every (effect, action, resource, condition) a set of statements grants or denies.
fn expanded(statements: &[AwsIamStatement]) -> std::collections::BTreeSet<String> {
    statements
        .iter()
        .flat_map(|statement| {
            let resources = if statement.not_resource.is_empty() {
                statement
                    .resource
                    .iter()
                    .map(|r| format!("Resource {r}"))
                    .collect::<Vec<_>>()
            } else {
                vec![format!("NotResource {:?}", statement.not_resource)]
            };
            let condition = canonical(&serde_json::to_value(&statement.condition).unwrap());
            statement.action.iter().flat_map(move |action| {
                let condition = condition.clone();
                let effect = statement.effect.clone();
                resources
                    .clone()
                    .into_iter()
                    .map(move |resource| format!("{effect} {action} {resource} {condition}"))
            })
        })
        .collect()
}

#[test]
fn automatic_management_statements_fit_the_attachment_quota() {
    let statements = automatic_management_statements();
    let chunks = chunk_iam_statements(statements.clone(), 5_500).unwrap();
    // The setup policy and the permissions boundary are attached next to these. Keep two
    // attachments of headroom below AWS's default quota of 20 per role.
    assert!(chunks.len() + 2 <= 18, "{} policies", chunks.len());

    // Merged and packed, the policies grant and deny exactly what the statements did.
    for chunk in &chunks {
        assert!(policy_document_size(chunk).unwrap() <= 5_500);
    }
    assert_eq!(expanded(&chunks.concat()), expanded(&statements));
    // No Sid is used twice.
    let all = chunks.concat();
    let sids: std::collections::BTreeSet<&str> = all.iter().map(|s| s.sid.as_str()).collect();
    assert_eq!(sids.len(), all.len());
}

#[test]
fn only_statements_that_differ_in_their_actions_are_merged() {
    let mut read = statement(1, 1);
    read.action = vec!["s3:GetObject".into()];
    let mut write = read.clone();
    write.sid = "Write".into();
    write.action = vec!["s3:PutObject".into(), "s3:GetObject".into()];
    let mut other_resource = read.clone();
    other_resource.sid = "OtherResource".into();
    other_resource.resource = vec!["arn:aws:s3:::other/*".into()];
    let mut denied = read.clone();
    denied.sid = "Denied".into();
    denied.effect = "Deny".into();
    let mut excluded = read.clone();
    excluded.sid = "Excluded".into();
    excluded.resource = Vec::new();
    excluded.not_resource = vec!["arn:aws:s3:::other/*".into()];
    let mut excluded_too = excluded.clone();
    excluded_too.sid = "ExcludedToo".into();
    let input = vec![read, write, other_resource, denied, excluded, excluded_too];

    let merged = merge_iam_statements(input.clone(), 5_500).unwrap();
    let sids: Vec<&str> = merged.iter().map(|s| s.sid.as_str()).collect();
    assert_eq!(
        sids,
        [
            "Read1",
            "OtherResource",
            "Denied",
            "Excluded",
            "ExcludedToo"
        ]
    );
    assert_eq!(merged[0].action, ["s3:GetObject", "s3:PutObject"]);
    assert_eq!(expanded(&merged), expanded(&input));
}

#[test]
fn packing_the_same_statements_twice_gives_the_same_policies() {
    let first = chunk_iam_statements(automatic_management_statements(), 5_500).unwrap();
    let second = chunk_iam_statements(automatic_management_statements(), 5_500).unwrap();
    assert_eq!(first, second);
}

/// Statements a user split to keep each one within a policy stay split when merging them would
/// not fit one policy.
#[test]
fn a_merge_that_would_not_fit_one_policy_keeps_the_statements_apart() {
    let long = |sid: &str, prefix: &str| {
        let mut statement = statement(1, 1);
        statement.sid = sid.into();
        statement.action = (0..160)
            .map(|n| format!("s3:{prefix}Action{n:03}"))
            .collect();
        statement
    };
    let input = vec![long("First", "Get"), long("Second", "Put")];
    assert!(policy_document_size(&input[..1]).unwrap() <= 5_500);
    let chunks = chunk_iam_statements(input.clone(), 5_500).unwrap();
    let all = chunks.concat();
    assert_eq!(all.len(), 2);
    assert_eq!(expanded(&all), expanded(&input));
}
