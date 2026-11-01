use super::*;
use crate::{block::resource_block, gating::GatedAddresses};
use std::collections::BTreeSet;

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

/// Labels of the managed policies with label stem `base`, in the order they were emitted.
fn policies(per_resource: &IndexMap<String, TfFragment>, base: &str) -> Vec<String> {
    per_resource
        .values()
        .flat_map(|fragment| &fragment.resource_blocks)
        .filter(|block| block_type(block) == Some("aws_iam_policy"))
        .map(|block| block.labels[1].as_str().to_string())
        .filter(|label| {
            label
                .strip_prefix(&format!("{base}_"))
                .is_some_and(|hash| hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .collect()
}

fn consolidate(per_resource: &mut IndexMap<String, TfFragment>) -> GatedAddresses {
    let mut gated = GatedAddresses::default();
    consolidate_role_policies(per_resource, &IndexMap::new(), &mut gated).unwrap();
    gated
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
    consolidate(&mut per_resource);

    let [first, second] = policies(&per_resource, "app_setup").try_into().unwrap();
    assert_eq!(
        depends_on(block(&per_resource, "aws_s3_bucket.data")),
        [format!("aws_iam_role_policy_attachment.{first}_attachment")]
    );
    assert_eq!(
        depends_on(block(&per_resource, &format!("aws_iam_policy.{second}"))),
        ["aws_s3_bucket.data"]
    );
    let policy_of = |label: &str| {
        hcl::format::to_string(&block(&per_resource, &format!("aws_iam_policy.{label}")).body)
            .unwrap()
    };
    assert!(policy_of(&first).contains("s3:GetObject"));
    assert!(policy_of(&first).contains("sqs:SendMessage"));
    assert!(policy_of(&second).contains("s3:PutObject"));
    for stub in ["app_first", "app_second", "app_third"] {
        let stub = block(&per_resource, &format!("aws_iam_role_policy.{stub}"));
        assert_eq!(
            attribute(stub, "policy"),
            Some(&Expression::String(STUB_POLICY.into()))
        );
        assert_eq!(
            depends_on(stub),
            [
                format!("aws_iam_role_policy_attachment.{first}_attachment"),
                format!("aws_iam_role_policy_attachment.{second}_attachment")
            ]
        );
    }
    assert!(!DependencyGraph::new(&per_resource, &IndexMap::new()).has_cycle());
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
    let gated = consolidate(&mut per_resource);

    let [always] = policies(&per_resource, "app_setup").try_into().unwrap();
    let [jobs] = policies(&per_resource, "app_input_jobs_setup")
        .try_into()
        .unwrap();
    for address in [
        format!("aws_iam_policy.{jobs}"),
        format!("aws_iam_role_policy_attachment.{jobs}_attachment"),
        "terraform_data.app_input_jobs_setup_policies_barrier".to_string(),
        "aws_iam_role_policy.app_jobs".to_string(),
    ] {
        assert_eq!(
            attribute(block(&per_resource, &address), "count"),
            Some(&count),
            "{address}"
        );
    }
    let always = block(&per_resource, &format!("aws_iam_policy.{always}"));
    assert_eq!(attribute(always, "count"), None);
    assert!(!hcl::format::to_string(&always.body)
        .unwrap()
        .contains("sqs:SendMessage"));
    let mut rendered = gated.rendered_forms();
    rendered.sort();
    assert_eq!(
        rendered,
        [
            format!("aws_iam_policy.{jobs}"),
            format!("aws_iam_role_policy_attachment.{jobs}_attachment"),
            "terraform_data.app_input_jobs_setup_policies_barrier".to_string()
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

fn attribute_text(block: &Block, name: &str) -> String {
    attribute(block, name)
        .unwrap_or_else(|| panic!("{:?} sets {name}", block.labels))
        .to_string()
}

/// A role with one grant per letter, each too large to share a managed policy with another.
fn role_with_large_grants(letters: &[char]) -> IndexMap<String, TfFragment> {
    let mut blocks = vec![role("app")];
    for letter in letters {
        let resource = format!("arn:aws:s3:::{}", letter.to_string().repeat(3_000));
        blocks.push(grant(
            &format!("app_{letter}"),
            "app",
            vec![statement(
                &letter.to_uppercase().to_string(),
                "Allow",
                &[&format!("s3:Get{letter}")],
                &[&resource],
            )],
            vec![],
        ));
    }
    fragments(vec![("identity", blocks)])
}

/// A managed policy's address and name follow its document, so an unchanged document keeps
/// both when an earlier policy goes away, when one is inserted before it, or when grants are
/// reordered; the barrier changes only with the set of attachments.
#[test]
fn an_unchanged_policy_keeps_its_address_wherever_it_lands() {
    let render = |letters: &[char]| {
        let mut per_resource = role_with_large_grants(letters);
        consolidate(&mut per_resource);
        let labels = policies(&per_resource, "app_setup");
        assert_eq!(labels.len(), letters.len(), "one policy per grant");
        let names: Vec<String> = labels
            .iter()
            .map(|label| {
                attribute_text(
                    block(&per_resource, &format!("aws_iam_policy.{label}")),
                    "name",
                )
            })
            .collect();
        let barrier = attribute_text(
            block(&per_resource, "terraform_data.app_setup_policies_barrier"),
            "triggers_replace",
        );
        (labels, names, barrier)
    };
    let (both, both_names, both_barrier) = render(&['x', 'y']);
    let (only_y, only_y_names, _) = render(&['y']);
    let (inserted, inserted_names, _) = render(&['w', 'x', 'y']);
    let (reordered, reordered_names, reordered_barrier) = render(&['y', 'x']);

    assert_eq!(only_y, [both[1].clone()]);
    assert_eq!(only_y_names, [both_names[1].clone()]);
    assert_eq!(inserted[1..], both[..]);
    assert_eq!(inserted_names[1..], both_names[..]);
    assert!(!both.contains(&inserted[0]));
    assert_eq!(reordered, [both[1].clone(), both[0].clone()]);
    assert_eq!(
        reordered_names,
        [both_names[1].clone(), both_names[0].clone()]
    );
    assert_eq!(reordered_barrier, both_barrier);
    assert!(both_names[0].contains(&both[0]["app_setup_".len()..]));
}

/// Two managed policies of one role at one address cannot both exist, so the render fails.
/// Two grants of one role with the same document, kept apart (here by their gates), become two
/// managed policies. The first keeps the address its document gives it; the second's address
/// also follows its first former grant, so each stays put while its own grants do.
#[test]
fn identical_managed_policies_on_one_role_get_their_own_addresses() {
    let same = vec![statement("A", "Allow", &["s3:GetObject"], &["*"])];
    let on = grant(
        "app_on",
        "app",
        same.clone(),
        vec![attr("count", expr::raw("var.jobs ? 1 : 0"))],
    );
    let off = grant(
        "app_off",
        "app",
        same,
        vec![attr("count", expr::raw("var.jobs ? 0 : 1"))],
    );
    let mut alone = fragments(vec![("identity", vec![role("app"), on.clone()])]);
    consolidate(&mut alone);
    let [first] = policies(&alone, "app_jobs_setup").try_into().unwrap();

    let mut both = fragments(vec![("identity", vec![role("app"), on, off])]);
    consolidate(&mut both);
    let labels = policies(&both, "app_jobs_setup");
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert_eq!(labels[0], first);
    assert_ne!(labels[1], first);
    for (label, count) in labels.iter().zip(["var.jobs ? 1 : 0", "var.jobs ? 0 : 1"]) {
        let policy = block(&both, &format!("aws_iam_policy.{label}"));
        assert_eq!(attribute(policy, "count").unwrap().to_string(), count);
    }
}

/// A reference through a local reaches what the local names: a grant whose document names a
/// local over the bucket, and a grant the bucket waits for, are never folded.
#[test]
fn grants_that_reach_each_other_through_a_local_are_not_folded() {
    let mut per_resource = fragments(vec![(
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
                "app_second",
                "app",
                vec![statement(
                    "B",
                    "Allow",
                    &["s3:PutObject"],
                    &["${local.data_bucket.arn}"],
                )],
                vec![],
            ),
            resource_block(
                "aws_s3_bucket",
                "data",
                [attr(
                    "depends_on",
                    Expression::Array(vec![expr::traversal(["aws_iam_role_policy", "app_first"])]),
                )],
            ),
        ],
    )]);
    let locals = IndexMap::from([(
        "data_bucket".to_string(),
        expr::raw("{ arn = aws_s3_bucket.data.arn }"),
    )]);
    consolidate_role_policies(&mut per_resource, &locals, &mut GatedAddresses::default()).unwrap();
    let policies = policies(&per_resource, "app_setup");
    assert_eq!(policies.len(), 2, "{policies:?}");
    assert!(!DependencyGraph::new(&per_resource, &locals).has_cycle());
}

/// The (effect, action, resource, condition) pairs a list of statements grants or denies.
fn pairs(statements: &[Expression]) -> BTreeSet<(String, String, String, String)> {
    let key = |name: &str| ObjectKey::Identifier(hcl::Identifier::sanitized(name));
    let values = |value: Option<&Expression>| match value {
        Some(Expression::Array(values)) => values.iter().map(ToString::to_string).collect(),
        Some(value) => vec![value.to_string()],
        None => Vec::new(),
    };
    let mut pairs = BTreeSet::new();
    for statement in statements {
        let Expression::Object(object) = statement else {
            panic!("a statement is an object");
        };
        let effect = object[&key("Effect")].to_string();
        let condition = object
            .get(&key("Condition"))
            .map(ToString::to_string)
            .unwrap_or_default();
        for action in values(object.get(&key("Action"))) {
            for resource in values(object.get(&key("Resource"))) {
                pairs.insert((effect.clone(), action.clone(), resource, condition.clone()));
            }
        }
    }
    pairs
}

fn conditioned(statement: Expression) -> Expression {
    let Expression::Object(mut object) = statement else {
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
}

/// Many grants of the same actions on one bucket each merge into one statement listing every
/// bucket, too large for a managed policy. It is split by resource into statements that each
/// fit, granting exactly what the grants did, conditions included, with unique Sids.
#[test]
fn an_oversized_merged_statement_is_split_by_resource() {
    let mut blocks = vec![role("app")];
    let mut granted = Vec::new();
    for bucket in 0..150 {
        let bucket = format!("arn:aws:s3:::${{local.resource_prefix}}-bucket-{bucket}");
        let statement = conditioned(expr::object([
            ("Sid", Expression::String("ReadWrite".into())),
            ("Effect", Expression::String("Allow".into())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("s3:GetObject".into()),
                    Expression::String("s3:PutObject".into()),
                ]),
            ),
            (
                "Resource",
                Expression::Array(vec![
                    expr::template(bucket.clone()),
                    expr::template(format!("{bucket}/*")),
                ]),
            ),
        ]));
        granted.push(statement.clone());
        blocks.push(grant(
            &format!("app_bucket_{}", granted.len()),
            "app",
            vec![statement],
            vec![],
        ));
    }
    blocks.push(grant(
        "app_queue",
        "app",
        vec![statement("Send", "Allow", &["sqs:SendMessage"], &["*"])],
        vec![],
    ));
    granted.push(statement("Send", "Allow", &["sqs:SendMessage"], &["*"]));
    let mut per_resource = fragments(vec![("identity", blocks)]);
    consolidate(&mut per_resource);

    let labels = policies(&per_resource, "app_setup");
    assert!(labels.len() >= 3, "{labels:?}");
    let module = RenderedModule::new(
        per_resource
            .values()
            .flat_map(|fragment| fragment.resource_blocks.iter().cloned())
            .collect(),
        IndexMap::new(),
    );
    let usage = module.usage(DeploymentLengths::longest()).unwrap();
    let mut statements = Vec::new();
    for label in &labels {
        let chars = usage.managed_policies[label];
        assert!(chars <= MAX_MANAGED_POLICY_CHARS, "{label}: {chars}");
        let Some(Expression::FuncCall(call)) = attribute(
            block(&per_resource, &format!("aws_iam_policy.{label}")),
            "policy",
        ) else {
            panic!("{label} is jsonencode(...)");
        };
        let Some(Expression::Object(document)) = call.args.first() else {
            panic!("{label} encodes an object");
        };
        let Some(Expression::Array(listed)) = document.get(&ObjectKey::Identifier(
            hcl::Identifier::sanitized("Statement"),
        )) else {
            panic!("{label} lists statements");
        };
        statements.extend(listed.iter().cloned());
    }
    assert_eq!(pairs(&statements), pairs(&granted));
    let sids: Vec<String> = statements
        .iter()
        .map(|statement| match statement {
            Expression::Object(object) => {
                object[&ObjectKey::Identifier(hcl::Identifier::sanitized("Sid"))].to_string()
            }
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        sids.iter().collect::<HashSet<_>>().len(),
        sids.len(),
        "{sids:?}"
    );
    assert_eq!(sids[0], "\"ReadWrite\"");
    assert_eq!(sids[1], "\"ReadWrite2\"");
    assert_eq!(sids.last().unwrap(), "\"Send\"");
}

/// Splitting the list of a `NotResource` or `NotAction` statement would change what it means,
/// so one too large for a managed policy fails the render.
#[test]
fn an_oversized_negated_statement_fails() {
    let resources: Vec<String> = (0..150)
        .map(|bucket| format!("arn:aws:s3:::bucket-with-a-long-name-{bucket}"))
        .collect();
    let resources: Vec<&str> = resources.iter().map(String::as_str).collect();
    let rename = |statement: Expression, from: &str, to: &str| {
        let Expression::Object(object) = statement else {
            unreachable!()
        };
        Expression::Object(
            object
                .into_iter()
                .map(|(key, value)| match object_key(&key) {
                    Some(name) if name == from => {
                        (ObjectKey::Identifier(hcl::Identifier::sanitized(to)), value)
                    }
                    _ => (key, value),
                })
                .collect(),
        )
    };
    for negated in [
        rename(
            statement("Deny", "Deny", &["s3:GetObject"], &resources),
            "Resource",
            "NotResource",
        ),
        rename(
            statement("Deny", "Deny", &["s3:GetObject"], &resources),
            "Action",
            "NotAction",
        ),
    ] {
        let mut per_resource = fragments(vec![(
            "identity",
            vec![role("app"), grant("app_deny", "app", vec![negated], vec![])],
        )]);
        let error = consolidate_role_policies(
            &mut per_resource,
            &IndexMap::new(),
            &mut GatedAddresses::default(),
        )
        .expect_err("a negated statement is never split");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
        assert!(error.to_string().contains("cannot be split"), "{error}");
    }
}
