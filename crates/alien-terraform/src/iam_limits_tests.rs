use super::*;
use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    emitters::aws::helpers::{
        iam_managed_policy_name_template, iam_role_name_template, jsonencode,
    },
    expr, TerraformOptions, TerraformTarget, TfRegistry,
};
use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Platform, SetupConfigurationProfile, Stack, StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;

const PREFIX: usize = 40;
const REGION: usize = 16;
const ACCOUNT: &str = "123456789012";

fn longest_prefix() -> String {
    "s".repeat(PREFIX)
}

fn longest_region() -> String {
    "r".repeat(REGION)
}

fn role(label: &str) -> Block {
    resource_block(
        "aws_iam_role",
        label,
        [attr("name", iam_role_name_template(label))],
    )
}

fn inline_policy(label: &str, role: &str, policy: Expression) -> Block {
    resource_block(
        "aws_iam_role_policy",
        label,
        [
            attr("name", Expression::String(label.into())),
            attr("role", expr::traversal(["aws_iam_role", role, "id"])),
            attr("policy", policy),
        ],
    )
}

/// `jsonencode` of a one-statement document allowing `s3:GetObject` on `resource`.
fn document(resource: Expression) -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".into())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Effect", Expression::String("Allow".into())),
                ("Action", Expression::String("s3:GetObject".into())),
                ("Resource", resource),
            ])]),
        ),
    ]))
}

/// IAM's count for the same document with `resource` already resolved.
fn resolved_chars(resource: &str) -> usize {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": resource}],
    })
    .to_string()
    .chars()
    .filter(|c| !c.is_whitespace())
    .count()
}

/// A policy whose document is exactly `chars` long.
fn sized_policy(chars: usize) -> Expression {
    let padding = chars - resolved_chars("");
    document(Expression::String("x".repeat(padding)))
}

fn usage(resources: Vec<Block>) -> Result<ModuleUsage> {
    RenderedModule::new(resources, IndexMap::new()).usage(DeploymentLengths::longest())
}

fn check(resources: Vec<Block>) -> Result<()> {
    RenderedModule::new(resources, IndexMap::new()).check()
}

#[test]
fn sizes_each_interpolation_at_its_longest_value() {
    let prefix = longest_prefix();
    let region = longest_region();
    let module_resources = vec![
        role("execution"),
        resource_block(
            "aws_s3_bucket",
            "data",
            [attr(
                "bucket",
                expr::template("${local.resource_prefix}-data"),
            )],
        ),
        resource_block(
            "aws_ecr_repository",
            "images",
            [attr(
                "name",
                expr::template("${local.resource_prefix}-images"),
            )],
        ),
        resource_block(
            "aws_iam_policy",
            "boundary",
            [
                attr(
                    "name",
                    iam_managed_policy_name_template("deployment-management-boundary"),
                ),
                attr("policy", sized_policy(100)),
            ],
        ),
        resource_block("aws_kms_key", "key", [attr("count", expr::raw("1"))]),
    ];
    let locals = IndexMap::from([(
        "key_selected_key".to_string(),
        expr::raw("var.input_encrypt ? { arn = aws_kms_key.key[0].arn } : null"),
    )]);
    let kms_arn =
        format!("arn:aws:kms:{region}:{ACCOUNT}:key/00000000-0000-0000-0000-000000000000");
    for (resource, resolved) in [
        (
            expr::template("arn:aws:s3:::${local.resource_prefix}-*"),
            format!("arn:aws:s3:::{prefix}-*"),
        ),
        (
            expr::template(
                "arn:aws:ssm:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:parameter/${local.resource_prefix}-*",
            ),
            format!("arn:aws:ssm:{region}:{ACCOUNT}:parameter/{prefix}-*"),
        ),
        (
            expr::template("arn:aws:ecr:*:${var.managing_account_id}:repository/*"),
            "arn:aws:ecr:*:210987654321:repository/*".to_string(),
        ),
        (
            expr::template("${aws_s3_bucket.data.arn}/*"),
            format!("arn:aws:s3:::{prefix}-data/*"),
        ),
        (
            expr::raw("format(\"%s-*\", aws_ecr_repository.images.arn)"),
            format!("arn:aws:ecr:{region}:{ACCOUNT}:repository/{prefix}-images-*"),
        ),
        // The name fits IAM's 128 characters, so the hashed branch is never taken.
        (
            expr::traversal(["aws_iam_policy", "boundary", "arn"]),
            format!("arn:aws:iam::{ACCOUNT}:policy/{prefix}-deployment-management-boundary"),
        ),
        (
            expr::traversal(["local", "key_selected_key", "arn"]),
            kms_arn.clone(),
        ),
        (
            expr::template("${data.aws_partition.current.partition}"),
            "aws-us-gov".to_string(),
        ),
    ] {
        let resources = [
            module_resources.clone(),
            vec![inline_policy("grant", "execution", document(resource.clone()))],
        ]
        .concat();
        let module = RenderedModule::new(resources, locals.clone());
        let usage = module.usage(DeploymentLengths::longest()).unwrap();
        assert_eq!(
            usage.roles["execution"],
            RoleUsage {
                inline_policies: 1,
                inline_chars: resolved_chars(&resolved),
                attachments: 0,
            },
            "{resource}"
        );
        assert_eq!(usage.managed_policies["boundary"], 100);
    }

    // A replacement no longer than every match never lengthens the value, so it is sized as
    // the value: here the region becomes `*`.
    let resources = [
        module_resources,
        vec![inline_policy(
            "grant",
            "execution",
            document(expr::raw(
                "replace(aws_ecr_repository.images.arn, format(\":%s:\", data.aws_region.current.region), \":*:\")",
            )),
        )],
    ]
    .concat();
    assert_eq!(
        RenderedModule::new(resources, locals)
            .usage(DeploymentLengths::longest())
            .unwrap()
            .roles["execution"]
            .inline_chars,
        resolved_chars(&format!(
            "arn:aws:ecr:{region}:{ACCOUNT}:repository/{prefix}-images"
        ))
    );
}

#[test]
fn shortened_names_are_bounded_by_their_limit() {
    // 40 + 1 + 30 characters exceed IAM's 64-character role names, so the name is cut to 64.
    let suffix = "x".repeat(30);
    let resources = vec![
        role("execution"),
        resource_block(
            "aws_iam_role",
            "long",
            [attr("name", iam_role_name_template(&suffix))],
        ),
        inline_policy(
            "grant",
            "execution",
            document(expr::traversal(["aws_iam_role", "long", "arn"])),
        ),
    ];
    assert_eq!(
        usage(resources).unwrap().roles["execution"].inline_chars,
        resolved_chars(&format!("arn:aws:iam::{ACCOUNT}:role/{}", "n".repeat(64)))
    );
}

#[test]
fn an_interpolation_without_a_known_bound_fails_the_render() {
    for resource in [
        expr::template("arn:aws:s3:::${var.bucket_name}"),
        expr::template("${aws_lambda_function.handler.arn}"),
        expr::template("${aws_s3_bucket.absent.arn}"),
        expr::template("%{ if true }arn:aws:s3:::x%{ endif }"),
        expr::raw("[for name in var.names : name]"),
        expr::raw("join(\",\", var.names)"),
        expr::traversal(["local", "undefined"]),
    ] {
        let error = check(vec![
            role("execution"),
            inline_policy("grant", "execution", document(resource.clone())),
        ])
        .expect_err("an unbounded interpolation must not be sized as zero");
        assert_eq!(error.code, "TEMPLATE_SERIALIZATION_FAILED", "{resource}");
        assert!(
            format!("{error:?}").contains("Cannot bound the size of"),
            "{resource}: {error:?}"
        );
    }
}

#[test]
fn a_policy_must_name_a_role_of_the_module() {
    for role_expression in [
        expr::raw("var.role_name"),
        expr::traversal(["aws_iam_role", "absent", "id"]),
    ] {
        let policy = resource_block(
            "aws_iam_role_policy",
            "grant",
            [
                attr("role", role_expression),
                attr("policy", sized_policy(100)),
            ],
        );
        let error = check(vec![role("execution"), policy]).expect_err("unknown role");
        assert_eq!(error.code, "TEMPLATE_SERIALIZATION_FAILED");
    }
}

#[test]
fn inline_policies_of_a_role_are_summed_up_to_the_quota() {
    let mut embedding = role("execution");
    embedding.body.0.push(nested(block(
        "inline_policy",
        [attr("policy", sized_policy(5_000))],
    )));
    check(vec![
        embedding.clone(),
        inline_policy("external", "execution", sized_policy(5_240)),
    ])
    .expect("exactly 10,240 characters fit");

    let error = check(vec![
        embedding,
        inline_policy("external", "execution", sized_policy(5_241)),
    ])
    .expect_err("10,241 characters exceed the quota although neither policy does");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    let message = error.to_string();
    assert!(message.contains("IAM role 'execution' can need 10241 non-whitespace characters across its 2 inline policies"), "{message}");
}

#[test]
fn conditional_roles_and_policies_count_as_present() {
    let mut gated_role = role("execution");
    gated_role
        .body
        .0
        .push(attr("count", expr::raw("var.enabled ? 1 : 0")));
    let mut gated_policy = inline_policy("first", "execution", sized_policy(6_000));
    gated_policy
        .body
        .0
        .push(attr("for_each", expr::raw("var.grants")));
    let indexed = resource_block(
        "aws_iam_role_policy",
        "second",
        [
            attr(
                "role",
                expr::traversal_indexed("aws_iam_role", "execution", "id"),
            ),
            attr("policy", sized_policy(6_000)),
        ],
    );
    let usage = usage(vec![gated_role, gated_policy, indexed]).unwrap();
    assert_eq!(
        usage.roles["execution"],
        RoleUsage {
            inline_policies: 2,
            inline_chars: 12_000,
            attachments: 0,
        }
    );
}

#[test]
fn managed_policies_must_fit_the_managed_policy_quota() {
    let managed = |chars| {
        resource_block(
            "aws_iam_policy",
            "grants",
            [
                attr("name", Expression::String("grants".into())),
                attr("policy", sized_policy(chars)),
            ],
        )
    };
    check(vec![managed(6_144)]).expect("exactly 6,144 characters fit");
    let error = check(vec![managed(6_145)]).expect_err("6,145 characters exceed the quota");
    assert!(error
        .to_string()
        .contains("IAM managed policy 'grants' can need 6145 non-whitespace characters"));
}

#[test]
fn attachments_count_distinct_policies_up_to_the_quota() {
    let attachment = |label: &str, arn: Expression| {
        resource_block(
            "aws_iam_role_policy_attachment",
            label,
            [
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", "execution", "name"]),
                ),
                attr("policy_arn", arn),
            ],
        )
    };
    let mut resources = vec![role("execution")];
    for index in 0..10 {
        resources.push(resource_block(
            "aws_iam_policy",
            &format!("chunk_{index}"),
            [
                attr("name", Expression::String(format!("chunk-{index}"))),
                attr("policy", sized_policy(100)),
            ],
        ));
        resources.push(attachment(
            &format!("chunk_{index}"),
            expr::traversal(["aws_iam_policy", &format!("chunk_{index}"), "arn"]),
        ));
        resources.push(attachment(
            &format!("aws_{index}"),
            Expression::String(format!("arn:aws:iam::aws:policy/Managed{index}")),
        ));
    }
    // The same policies attached again, by reference and by literal ARN, are one attachment.
    resources.push(attachment(
        "again",
        expr::traversal_indexed("aws_iam_policy", "chunk_0", "arn"),
    ));
    let mut embedding = role("execution");
    embedding.body.0.push(attr(
        "managed_policy_arns",
        Expression::Array(vec![Expression::String(
            "arn:aws:iam::aws:policy/Managed0".into(),
        )]),
    ));
    resources[0] = embedding;
    assert_eq!(
        usage(resources.clone()).unwrap().roles["execution"].attachments,
        20
    );
    check(resources.clone()).expect("twenty attachments fit");

    // An attachment this check cannot identify may be a different policy, so it counts.
    resources.push(attachment("external", expr::raw("var.extra_policy_arn")));
    let error = check(resources).expect_err("a twenty-first attachment exceeds the quota");
    assert!(error
        .to_string()
        .contains("IAM role 'execution' requires 21 managed-policy attachments"));
}

#[test]
fn generated_managed_policy_chunks_obey_the_quotas() {
    for (count, fits) in [(20, true), (21, false)] {
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(role("execution"));
        crate::emitters::aws::helpers::emit_iam_managed_policy_chunks(
            &mut fragment,
            "execution",
            "management",
            "q5-management",
            (0..count)
                .map(|index| alien_permissions::generators::AwsIamStatement {
                    sid: format!("Grant{index}"),
                    effect: "Allow".into(),
                    action: vec!["s3:GetObject".into()],
                    resource: vec![format!(
                        "arn:aws:s3:::${{local.resource_prefix}}-managed/{}",
                        "x".repeat(4500)
                    )],
                    not_resource: Vec::new(),
                    condition: None,
                })
                .collect(),
        )
        .unwrap();
        let module = RenderedModule::new(fragment.resource_blocks, IndexMap::new());
        let usage = module.usage(DeploymentLengths::longest()).unwrap();
        assert_eq!(usage.roles["execution"].attachments, count);
        assert_eq!(usage.managed_policies.len(), count);
        assert!(usage
            .managed_policies
            .values()
            .all(|chars| *chars <= IAM_MANAGED_POLICY_CHARS));
        assert_eq!(module.check().is_ok(), fits, "{count} chunks");
    }
}

/// The stack of the AWS container end-to-end app whose execution role IAM rejected: Frozen
/// storage, build (profile `build-execution`), artifact registry, vault, kv and queue, and two
/// Live containers using profile `execution`, which holds every data set, the build and
/// artifact-registry sets and worker execution stack-wide. Evaluated from its `alien.ts` as the
/// e2e harness does.
fn e2e_container_stack() -> Stack {
    serde_json::from_str(include_str!(
        "../tests/fixtures/aws-container-e2e-stack.json"
    ))
    .expect("the fixture is a stack")
}

/// Setup creates no compute cluster in Terraform: compute-cluster emitters come from composed
/// CLIs. This one emits nothing, so the rest of the module renders as `alien render` would.
struct NoComputeCluster;

impl crate::TfEmitter for NoComputeCluster {
    fn emit(&self, _ctx: &alien_core::import::EmitContext<'_>) -> Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, _ctx: &alien_core::import::EmitContext<'_>) -> Result<Expression> {
        Ok(expr::object(Vec::<(&str, Expression)>::new()))
    }
}

fn container_registry() -> TfRegistry {
    let mut registry = TfRegistry::built_in();
    registry.register(
        alien_core::ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        NoComputeCluster,
    );
    registry
}

/// Prepares the stack as `alien render` does: template preflights, then mutations against a
/// configuration without a deployment.
async fn prepare_for_render(stack: Stack) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("template mutations apply")
}

fn options(registry: &TfRegistry) -> TerraformOptions<'_> {
    TerraformOptions {
        registry,
        display_name: None,
        stack_settings: StackSettings::default(),
        registration: None,
        helm_install: None,
        supported_aws_regions: Vec::new(),
    }
}

/// The execution role's inline policies, sized at real deployment values, equal what IAM
/// counts for the same module (evaluated by Terraform with that prefix and region).
///
/// Before ALIEN-2147 the role held 19 policies: 10,804 characters at a 39-character prefix in
/// us-east-2, which IAM rejects, and the render was refused. The storage and vault grants the
/// stack-level policy already holds are no longer repeated per resource, which leaves 14.
#[tokio::test]
async fn sizes_the_e2e_execution_role_as_iam_counts_it() {
    let stack = prepare_for_render(e2e_container_stack()).await;
    let registry = container_registry();
    let module = crate::generator::render_terraform_module(
        &stack,
        TerraformTarget::Aws,
        options(&registry),
        false,
        false,
        SetupConfigurationProfile::Advanced,
    )
    .expect("module renders");
    let rendered = RenderedModule::parse(&module).unwrap();
    let us_east_2 = "us-east-2".len();
    for (prefix, region, chars) in [
        (9, us_east_2, 7_937),
        (39, us_east_2, 8_897),
        (40, us_east_2, 8_929),
        (PREFIX, REGION, 9_034),
    ] {
        let usage = rendered
            .usage(DeploymentLengths {
                prefix,
                region,
                ..DeploymentLengths::longest()
            })
            .unwrap();
        assert_eq!(
            usage.roles["execution_sa"],
            RoleUsage {
                inline_policies: 14,
                inline_chars: chars,
                attachments: 0,
            },
            "{prefix}-character prefix, {region}-character region"
        );
    }
    crate::generate_terraform_module(&stack, TerraformTarget::Aws, options(&registry))
        .expect("the module fits every deployment it accepts");
}

/// The vault's `"*"` sets in the e2e stack are all held by the execution role's stack-level
/// policy: the data sets by a wider prefix, the deployer-secrets protection as the same
/// document. Only a set the vault names itself is written for it.
#[tokio::test]
async fn a_vault_writes_no_copy_of_the_stack_level_policy() {
    let mut stack = prepare_for_render(e2e_container_stack()).await;
    let names = crate::naming::resource_labels(&stack).unwrap();
    let settings = StackSettings::default();
    let selected = |stack: &Stack| {
        let resource = &stack.resources["test-vault"];
        let ctx = alien_core::import::EmitContext {
            stack,
            resource,
            resource_id: "test-vault",
            platform: Platform::Aws,
            targets_kubernetes: false,
            stack_settings: &settings,
            names: &names,
        };
        let profile = &stack.permissions.profiles["execution"];
        // As the vault emitter selects them: sets named for the vault, then `"*"` ones.
        let mut seen = HashSet::new();
        let refs: Vec<_> = profile
            .0
            .get("test-vault")
            .into_iter()
            .flatten()
            .chain(profile.0["*"].iter())
            .filter(|reference| reference.id().starts_with("vault/"))
            .filter(|reference| seen.insert(reference.id().to_string()))
            .cloned()
            .collect();
        let context = crate::emitters::aws::helpers::stack_permission_context(&ctx)
            .with_resource_name("${local.resource_prefix}-test-vault".to_string());
        let ids = |refs: Vec<alien_core::PermissionSetReference>| {
            refs.iter().map(|r| r.id().to_string()).collect::<Vec<_>>()
        };
        (
            ids(crate::emitters::aws::helpers::without_stack_policy_grants(
                &ctx,
                "execution",
                profile,
                refs.clone(),
                &context,
            )
            .unwrap()),
            ids(crate::emitters::aws::helpers::without_stack_policy_copies(
                &ctx,
                "execution",
                profile,
                refs,
                &context,
            )
            .unwrap()),
        )
    };

    // Containment alone proves the Allows covered; the Deny is kept until it is compared.
    assert_eq!(
        selected(&stack),
        (
            vec!["vault/protect-unmapped-deployer-secrets".to_string()],
            Vec::<String>::new()
        )
    );

    stack
        .permissions
        .profiles
        .get_mut("execution")
        .unwrap()
        .0
        .insert(
            "test-vault".into(),
            vec![alien_core::PermissionSetReference::from_name(
                "vault/data-read",
            )],
        );
    assert_eq!(
        selected(&stack).1,
        vec!["vault/data-read".to_string()],
        "a set the vault names itself is still written"
    );
}
