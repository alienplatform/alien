use super::*;
use crate::{block::resource_block, gating::GatedAddresses};

fn statement(sid: &str, effect: &str, actions: &[&str], resources: &[&str]) -> Expression {
    expr::object([
        ("Sid", Expression::String(sid.into())),
        ("Effect", Expression::String(effect.into())),
        (
            "Action",
            Expression::Array(
                actions
                    .iter()
                    .map(|a| Expression::String((*a).into()))
                    .collect(),
            ),
        ),
        (
            "Resource",
            Expression::Array(
                resources
                    .iter()
                    .map(|r| Expression::String((*r).into()))
                    .collect(),
            ),
        ),
    ])
}

fn grant(label: &str, role: &str, statements: Vec<Expression>, extra: Vec<Structure>) -> Block {
    let mut body = vec![
        attr("name", Expression::String(label.replace('_', "-"))),
        attr("role", expr::traversal(["aws_iam_role", role, "id"])),
        attr("policy", document(statements)),
    ];
    body.extend(extra);
    resource_block("aws_iam_role_policy", label, body)
}

fn role(label: &str) -> Block {
    resource_block(
        "aws_iam_role",
        label,
        [attr("name", Expression::String(label.into()))],
    )
}

fn fragments(blocks: Vec<(&str, Vec<Block>)>) -> IndexMap<String, TfFragment> {
    blocks
        .into_iter()
        .map(|(id, blocks)| {
            let mut fragment = TfFragment::default();
            fragment.resource_blocks = blocks;
            (id.to_string(), fragment)
        })
        .collect()
}

fn block<'a>(per_resource: &'a IndexMap<String, TfFragment>, address: &str) -> &'a Block {
    per_resource
        .values()
        .flat_map(|fragment| &fragment.resource_blocks)
        .find(|block| block_address_text(block).as_deref() == Some(address))
        .unwrap_or_else(|| panic!("{address} is in the module"))
}

fn depends_on(block: &Block) -> Vec<String> {
    match attribute(block, "depends_on") {
        Some(Expression::Array(dependencies)) => {
            dependencies.iter().map(ToString::to_string).collect()
        }
        _ => Vec::new(),
    }
}

/// Grants are merged along one axis at a time, so no action reaches a resource it was not
/// granted on, and Allow, Deny and differently conditioned statements stay apart.
#[test]
fn merging_never_widens_a_grant() {
    let conditioned = {
        let Expression::Object(mut object) =
            statement("Tagged", "Allow", &["s3:GetObject"], &["b"])
        else {
            unreachable!()
        };
        object.insert(
            ObjectKey::Identifier(hcl::Identifier::sanitized("Condition")),
            expr::object([(
                "Bool",
                expr::object([("aws:SecureTransport", Expression::String("true".into()))]),
            )]),
        );
        Expression::Object(object)
    };
    let compacted = compact(vec![
        statement("ReadOne", "Allow", &["s3:GetObject"], &["one"]),
        statement("ReadTwo", "Allow", &["s3:GetObject"], &["two"]),
        statement("WriteOne", "Allow", &["s3:PutObject"], &["one"]),
        statement("DenyOne", "Deny", &["s3:GetObject"], &["one"]),
        conditioned.clone(),
        // A copy of an earlier statement adds nothing.
        statement("ReadOneAgain", "Allow", &["s3:GetObject"], &["one"]),
    ]);
    assert_eq!(
        compacted,
        vec![
            statement("ReadOne", "Allow", &["s3:GetObject"], &["one", "two"]),
            statement("WriteOne", "Allow", &["s3:PutObject"], &["one"]),
            statement("DenyOne", "Deny", &["s3:GetObject"], &["one"]),
            conditioned,
        ]
    );
}

#[test]
fn statement_ids_stay_unique() {
    let compacted = compact(vec![
        statement("Read", "Allow", &["s3:GetObject"], &["one"]),
        statement("Read", "Allow", &["s3:PutObject"], &["two"]),
        statement("Read", "Deny", &["s3:DeleteObject"], &["three"]),
    ]);
    let sids: Vec<_> = compacted
        .iter()
        .map(|statement| match statement {
            Expression::Object(object) => object
                .get(&ObjectKey::Identifier(hcl::Identifier::sanitized("Sid")))
                .unwrap()
                .to_string(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(sids, ["\"Read\"", "\"Read2\"", "\"Read3\""]);
}

/// Folding two grants whose resources reach each other would make the managed policy wait for
/// itself, so they go to separate managed policies; independent grants share one.
#[test]
fn grants_that_reach_each_other_are_not_folded() {
    let mut per_resource = fragments(vec![
        (
            "identity",
            vec![
                role("app"),
                grant(
                    "app_first",
                    "app",
                    vec![statement("A", "Allow", &["s3:GetObject"], &["*"])],
                    vec![],
                ),
                grant(
                    "app_third",
                    "app",
                    vec![statement("C", "Allow", &["sqs:SendMessage"], &["*"])],
                    vec![],
                ),
            ],
        ),
        (
            "bucket",
            vec![
                // The bucket waits for one grant, and another grant waits for the bucket.
                resource_block(
                    "aws_s3_bucket",
                    "data",
                    [attr(
                        "depends_on",
                        Expression::Array(vec![expr::traversal([
                            "aws_iam_role_policy",
                            "app_first",
                        ])]),
                    )],
                ),
                grant(
                    "app_second",
                    "app",
                    vec![statement("B", "Allow", &["s3:PutObject"], &["*"])],
                    vec![attr(
                        "depends_on",
                        Expression::Array(vec![expr::traversal(["aws_s3_bucket", "data"])]),
                    )],
                ),
            ],
        ),
    ]);
    let mut gated = GatedAddresses::default();
    consolidate_role_policies(&mut per_resource, &IndexMap::new(), &mut gated).unwrap();

    assert_eq!(
        depends_on(block(&per_resource, "aws_s3_bucket.data")),
        ["aws_iam_role_policy_attachment.app_setup_0_attachment"]
    );
    assert_eq!(
        depends_on(block(&per_resource, "aws_iam_policy.app_setup_1")),
        ["aws_s3_bucket.data"]
    );
    let policy_of =
        |label: &str| hcl::format::to_string(&block(&per_resource, label).body).unwrap();
    assert!(policy_of("aws_iam_policy.app_setup_0").contains("s3:GetObject"));
    assert!(policy_of("aws_iam_policy.app_setup_0").contains("sqs:SendMessage"));
    assert!(policy_of("aws_iam_policy.app_setup_1").contains("s3:PutObject"));
    for stub in ["app_first", "app_second", "app_third"] {
        let stub = block(&per_resource, &format!("aws_iam_role_policy.{stub}"));
        assert_eq!(
            attribute(stub, "policy"),
            Some(&Expression::String(STUB_POLICY.into()))
        );
        assert_eq!(
            depends_on(stub),
            [
                "aws_iam_role_policy_attachment.app_setup_0_attachment",
                "aws_iam_role_policy_attachment.app_setup_1_attachment"
            ]
        );
    }
    assert!(!DependencyGraph::new(&per_resource).has_cycle());
}

/// A grant behind a gate moves into managed policies behind the same gate.
#[test]
fn a_gated_grant_keeps_its_gate() {
    let count = expr::raw("var.input_jobs ? 1 : 0");
    let mut per_resource = fragments(vec![(
        "identity",
        vec![
            role("app"),
            grant(
                "app_always",
                "app",
                vec![statement("A", "Allow", &["s3:GetObject"], &["*"])],
                vec![],
            ),
            grant(
                "app_jobs",
                "app",
                vec![statement("J", "Allow", &["sqs:SendMessage"], &["*"])],
                vec![attr("count", count.clone())],
            ),
        ],
    )]);
    let mut gated = GatedAddresses::default();
    consolidate_role_policies(&mut per_resource, &IndexMap::new(), &mut gated).unwrap();

    for address in [
        "aws_iam_policy.app_input_jobs_setup_0",
        "aws_iam_role_policy_attachment.app_input_jobs_setup_0_attachment",
        "terraform_data.app_input_jobs_setup_policies_barrier",
        "aws_iam_role_policy.app_jobs",
    ] {
        assert_eq!(
            attribute(block(&per_resource, address), "count"),
            Some(&count),
            "{address}"
        );
    }
    assert_eq!(
        attribute(block(&per_resource, "aws_iam_policy.app_setup_0"), "count"),
        None
    );
    assert!(
        !hcl::format::to_string(&block(&per_resource, "aws_iam_policy.app_setup_0").body)
            .unwrap()
            .contains("sqs:SendMessage")
    );
    let mut rendered = gated.rendered_forms();
    rendered.sort();
    assert_eq!(
        rendered,
        [
            "aws_iam_policy.app_input_jobs_setup_0",
            "aws_iam_role_policy_attachment.app_input_jobs_setup_0_attachment",
            "terraform_data.app_input_jobs_setup_policies_barrier"
        ]
    );
}

#[test]
fn reading_a_former_grant_other_than_by_depends_on_fails() {
    let mut per_resource = fragments(vec![(
        "identity",
        vec![
            role("app"),
            grant(
                "app_grant",
                "app",
                vec![statement("A", "Allow", &["s3:GetObject"], &["*"])],
                vec![],
            ),
            resource_block(
                "terraform_data",
                "reader",
                [attr(
                    "input",
                    expr::traversal(["aws_iam_role_policy", "app_grant", "policy"]),
                )],
            ),
        ],
    )]);
    let error = consolidate_role_policies(
        &mut per_resource,
        &IndexMap::new(),
        &mut GatedAddresses::default(),
    )
    .expect_err("the reader would see the stub");
    assert!(
        error
            .to_string()
            .contains("terraform_data.reader reads aws_iam_role_policy.app_grant"),
        "{error}"
    );
}

#[test]
fn a_statement_too_large_for_a_managed_policy_fails() {
    let resource = format!("arn:aws:s3:::{}", "x".repeat(MAX_MANAGED_POLICY_CHARS));
    let mut per_resource = fragments(vec![(
        "identity",
        vec![
            role("app"),
            grant(
                "app_grant",
                "app",
                vec![statement("A", "Allow", &["s3:GetObject"], &[&resource])],
                vec![],
            ),
        ],
    )]);
    let error = consolidate_role_policies(
        &mut per_resource,
        &IndexMap::new(),
        &mut GatedAddresses::default(),
    )
    .expect_err("no managed policy holds it");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
}
