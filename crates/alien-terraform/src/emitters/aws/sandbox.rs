//! AWS Sandbox — a Lambda MicroVM image and the role that builds it.
//!
//! Emitted through `awscc`, not `hashicorp/aws`: the AWS provider has no MicroVM resource at
//! 6.57, so Cloud Control is the only Terraform path to this API. The generator adds the
//! provider requirement when it sees these resource types, the same way it does for `azapi`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, default_network, downcast,
        emit_iam_role_policy_for_target_with_label, iam_policy_name_sanitize, iam_role_block,
        iam_role_name_template, iam_role_policy_block, nested_block, private_subnet_ids_expr,
        required_label, resource_prefix_template, service_assume_role_policy, tags, vpc_id_expr,
    },
    expr,
};
use alien_core::{
    import::EmitContext, permissions::PermissionSetReference, ErrorData, NetworkSettings,
    RemoteBindings, Result, Sandbox, SandboxCode, SandboxEgress, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_error::AlienError;
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

/// Terraform resource type for a MicroVM image.
///
/// Cloud Control rather than `awscc`: the schema makes `AdditionalOsCapabilities` required, and
/// the sandbox asks for none of them. `awscc` drops an empty list before sending, so the create
/// comes back `Model validation failed (#: required key [AdditionalOsCapabilities] not found)`
/// and no image is ever built. Cloud Control sends the body as written.
pub const MICROVM_IMAGE_RESOURCE: &str = "aws_cloudcontrolapi_resource";

/// The Cloud Control type this resource stands for.
const MICROVM_IMAGE_TYPE_NAME: &str = "AWS::Lambda::MicrovmImage";

/// Terraform resource type for the barrier that holds creation back until a role is usable.
pub const PROPAGATION_BARRIER_RESOURCE: &str = "time_sleep";

/// Terraform resource type for the egress network connector a session's traffic runs through.
pub const NETWORK_CONNECTOR_RESOURCE: &str = "awscc_lambda_network_connector";

/// AWS's own connector, named by the image rather than the deny connector the module creates.
///
/// The image build runs through whatever the image names, and it has to reach a registry. Naming
/// the deny connector there leaves the build with nowhere to go and the image never becomes
/// ACTIVE. A session's egress is decided by the connector passed at `RunMicrovm`, which is the
/// deny connector, so the build reaching out does not widen what a session can reach.
fn internet_egress_connector_arn() -> Expression {
    expr::raw(
        "\"arn:${data.aws_partition.current.partition}:lambda:${data.aws_region.current.region}:aws:network-connector:aws-network-connector:INTERNET_EGRESS\"",
    )
}

/// The only architecture MicroVM images accept. The schema's enum has exactly this one member,
/// so the agent has to be an aarch64 Linux binary.
const ARCHITECTURE: &str = "ARM_64";

/// Port the in-sandbox agent serves, both its own protocol and the lifecycle hooks.
const AGENT_PORT: i64 = 8971;

/// Unprivileged identity commands run as inside the sandbox, never the agent's own.
const EXEC_UID: &str = "60000";

/// The one destination the session's security group permits, which reaches nothing.
pub const LOOPBACK_ONLY_CIDR: &str = "127.0.0.1/32";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsSandboxEmitter;

impl TfEmitter for AwsSandboxEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let artifact_uri = artifact_uri(sandbox)?;
        refuse_unsupported_egress(sandbox)?;
        // An open sandbox routes nothing through a VPC: no subnets, and none of the connector
        // apparatus below exists for it.
        let open = matches!(sandbox.egress, SandboxEgress::Allow);
        let subnet_ids = if open {
            None
        } else {
            Some(egress_subnet_ids(ctx, sandbox)?)
        };
        // The size whose peak stays inside the declared ceilings. A MicroVM bursts to four times
        // its baseline with no way to opt out, so the baseline is a quarter of what was declared.
        let tier = sandbox.microvm_tier()?;
        let egress_label = format!("{label}_egress");

        let build_role = iam_role_block(
            label,
            iam_role_name_template(&format!("{}-build", sandbox.id())),
            service_assume_role_policy(&["lambda.amazonaws.com"]),
            tags(ctx, "sandbox"),
        );

        let build_policy = iam_role_policy_block(
            label,
            label,
            "sandbox-image-build",
            // Statements are raw objects: `iam_role_policy_block` already jsonencodes the whole
            // document, and encoding them again renders each one as a JSON *string*, which IAM
            // rejects with MalformedPolicyDocument. `terraform validate` cannot see it — the HCL
            // and the string are both well-formed — so it only shows up at apply.
            vec![
                Expression::from_iter([
                    ("Effect", Expression::String("Allow".to_string())),
                    (
                        "Action",
                        Expression::from(vec![Expression::String("s3:GetObject".to_string())]),
                    ),
                    (
                        "Resource",
                        // A template for the same reason the operator policy's ARNs are: a plain
                        // string literal has its `${` escaped, so the partition would reach IAM as
                        // literal text and the grant would match nothing.
                        expr::template(artifact_object_arn(&artifact_uri)),
                    ),
                ]),
                Expression::from_iter([
                    ("Effect", Expression::String("Allow".to_string())),
                    (
                        "Action",
                        Expression::from(vec![
                            // CreateLogGroup as well as the writes: the build creates no group of
                            // its own, so without it the first build's logs go nowhere. The house
                            // build role grants all three.
                            Expression::String("logs:CreateLogGroup".to_string()),
                            Expression::String("logs:CreateLogStream".to_string()),
                            Expression::String("logs:PutLogEvents".to_string()),
                        ]),
                    ),
                    ("Resource", Expression::String("*".to_string())),
                ]),
            ],
        );

        // Lambda assumes this to manage the connector's ENIs in the customer's VPC. The API
        // documents the permissions it must hold; the field being optional is not a promise
        // that AWS provisions an equivalent role on its own.
        let operator_role = iam_role_block(
            &egress_label,
            iam_role_name_template(&format!("{}-egress", sandbox.id())),
            service_assume_role_policy(&["lambda.amazonaws.com"]),
            tags(ctx, "sandbox"),
        );

        let operator_policy = iam_role_policy_block(
            &egress_label,
            &egress_label,
            "sandbox-egress-connector",
            operator_statements(),
        );

        // Both roles are referenced by ARN, which Terraform can resolve the moment the role
        // exists — before its inline policy is attached, and before IAM has propagated either.
        // Lambda assumes the operator role to place the connector's interfaces, so without this
        // the first apply fails with "unable to assume the provided NetworkConnectorOperatorRole"
        // and succeeds on a retry, which is the worst shape for a customer's first install.
        let iam_propagation = resource_block(
            PROPAGATION_BARRIER_RESOURCE,
            &format!("{label}_iam_propagation"),
            [
                attr("create_duration", Expression::String("30s".to_string())),
                // Keyed on the roles' unique ids rather than their ARNs: the name is templated,
                // so a replaced role keeps the same ARN and the barrier would never re-run.
                attr(
                    "triggers",
                    if open {
                        Expression::from_iter([(
                            "build_role",
                            expr::traversal(["aws_iam_role", label, "unique_id"]),
                        )])
                    } else {
                        Expression::from_iter([
                            (
                                "build_role",
                                expr::traversal(["aws_iam_role", label, "unique_id"]),
                            ),
                            (
                                "operator_role",
                                expr::traversal(["aws_iam_role", &egress_label, "unique_id"]),
                            ),
                        ])
                    },
                ),
                attr(
                    "depends_on",
                    if open {
                        Expression::from(vec![expr::traversal(["aws_iam_role_policy", label])])
                    } else {
                        Expression::from(vec![
                            expr::traversal(["aws_iam_role_policy", label]),
                            expr::traversal(["aws_iam_role_policy", &egress_label]),
                        ])
                    },
                ),
            ],
        );

        let security_group = resource_block(
            "aws_security_group",
            &egress_label,
            [
                attr(
                    "name_prefix",
                    resource_prefix_template(&format!("{}-egress-", sandbox.id())),
                ),
                attr(
                    "description",
                    Expression::String(format!("Sandbox {} session egress", sandbox.id())),
                ),
                attr("vpc_id", vpc_id_expr(ctx)),
                // Loopback-only is AWS's documented way to say "no egress": EC2 attaches an
                // allow-all rule to a new group unless the template states one, so the deny has
                // to be written down rather than left out. Widening this rule is the one edit
                // that turns `egress: deny` back into internet access.
                nested_block(
                    "egress",
                    vec![
                        attr("from_port", Expression::Number(0.into())),
                        attr("to_port", Expression::Number(0.into())),
                        attr("protocol", Expression::String("-1".to_string())),
                        attr(
                            "cidr_blocks",
                            Expression::from(vec![Expression::String(
                                LOOPBACK_ONLY_CIDR.to_string(),
                            )]),
                        ),
                    ],
                ),
                attr("tags", tags(ctx, "sandbox")),
            ],
        );

        let barrier_dependency = Expression::from(vec![expr::traversal([
            PROPAGATION_BARRIER_RESOURCE,
            &format!("{label}_iam_propagation"),
        ])]);

        let connector = resource_block(
            NETWORK_CONNECTOR_RESOURCE,
            label,
            [
                attr("depends_on", barrier_dependency.clone()),
                attr("name", resource_prefix_template(sandbox.id())),
                attr(
                    "operator_role",
                    expr::traversal(["aws_iam_role", &egress_label, "arn"]),
                ),
                attr(
                    "configuration",
                    Expression::from_iter([(
                        "vpc_egress_configuration",
                        Expression::from_iter([
                            (
                                "associated_compute_resource_types",
                                Expression::from(vec![Expression::String("MicroVm".to_string())]),
                            ),
                            // Documented optional in both the CloudControl schema and the
                            // CloudFormation reference, and rejected when absent:
                            // "NetworkProtocol cannot be null or empty for VPC_EGRESS connector".
                            // IPv4 rather than DualStack because the security group that carries
                            // the deny matches IPv4 CIDRs — a v6 path would be outside it.
                            ("network_protocol", Expression::String("IPv4".to_string())),
                            (
                                "subnet_ids",
                                subnet_ids
                                    .unwrap_or_else(|| Expression::from(Vec::<Expression>::new())),
                            ),
                            (
                                "security_group_ids",
                                Expression::from(vec![expr::traversal([
                                    "aws_security_group",
                                    &egress_label,
                                    "id",
                                ])]),
                            ),
                        ]),
                    )]),
                ),
                attr("tags", tag_objects(ctx, true)),
            ],
        );

        // Cloud Control takes the whole body as one JSON document, so the property names here
        // are the schema's own rather than the provider's snake_case rewriting of them.
        let desired_state = Expression::from_iter([
            ("Name", resource_prefix_template(sandbox.id())),
            (
                "Description",
                Expression::String(format!("Sandbox {}", sandbox.id())),
            ),
            ("BaseImageArn", base_image_arn()),
            ("BaseImageVersion", Expression::String("1".to_string())),
            (
                "BuildRoleArn",
                expr::traversal(["aws_iam_role", label, "arn"]),
            ),
            (
                "CodeArtifact",
                Expression::from_iter([("Uri", Expression::String(artifact_uri.clone()))]),
            ),
            // Content-bearing logging off: the control plane must never see session contents,
            // and this is the switch rather than an approximation of it.
            (
                "Logging",
                Expression::from_iter([("Disabled", Expression::Bool(true))]),
            ),
            // The build's route out, not the session's. See `internet_egress_connector_arn`.
            (
                "EgressNetworkConnectors",
                Expression::from(vec![internet_egress_connector_arn()]),
            ),
            (
                "CpuConfigurations",
                Expression::from(vec![Expression::from_iter([(
                    "Architecture",
                    Expression::String(ARCHITECTURE.to_string()),
                )])]),
            ),
            (
                "Resources",
                Expression::from(vec![Expression::from_iter([(
                    "MinimumMemoryInMiB",
                    Expression::Number(tier.baseline_memory_mib.into()),
                )])]),
            ),
            // The schema's enum is exactly ["ALL"], which grants mount, netns and eBPF. There is
            // no subset to ask for, so the answer is none — and the key has to be present.
            (
                "AdditionalOsCapabilities",
                Expression::from(Vec::<Expression>::new()),
            ),
            ("Hooks", hooks()),
            ("EnvironmentVariables", environment_variables()),
            ("Tags", tag_objects(ctx, false)),
        ]);

        let image = resource_block(
            MICROVM_IMAGE_RESOURCE,
            label,
            [
                attr("depends_on", barrier_dependency),
                attr(
                    "type_name",
                    Expression::String(MICROVM_IMAGE_TYPE_NAME.to_string()),
                ),
                attr("desired_state", expr::jsonencode(desired_state)),
            ],
        );

        let mut fragment = TfFragment::empty()
            .with_resource(build_role)
            .with_resource(build_policy)
            .with_resource(iam_propagation)
            .with_resource(image);
        if !open {
            fragment = fragment
                .with_resource(operator_role)
                .with_resource(operator_policy)
                .with_resource(security_group)
                .with_resource(connector);
        }
        emit_remote_bindings_policy(ctx, &mut fragment)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("previewPorts", preview_ports(sandbox)),
            ("egressConnectorArns", egress_connector_arns(sandbox, label)),
            (
                "allowEgress",
                Expression::Bool(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
            // The ARN, not the name. Measured against the live API: `GetMicrovmImage` and
            // `RunMicrovm` both refuse a bare name — the latter with "Malformed ARN - doesn't
            // start with 'arn:'" — so a controller handed a name cannot read its own image or
            // start a session.
            ("imageIdentifier", image_property(label, "ImageArn")),
            ("imageArn", image_property(label, "ImageArn")),
            // The version the controller scopes sessions to. `RunMicrovm` has no tags, so a
            // stale version here enumerates the wrong set and orphans live sessions.
            (
                "imageVersion",
                image_property(label, "LatestActiveImageVersion"),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let sandbox = downcast::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fields = vec![
            ("service", Expression::String("sandbox-aws".to_string())),
            ("previewPorts", preview_ports(sandbox)),
            ("egressConnectorArns", egress_connector_arns(sandbox, label)),
            (
                "allowEgress",
                Expression::Bool(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
            ("imageArn", image_property(label, "ImageArn")),
            (
                "imageVersion",
                image_property(label, "LatestActiveImageVersion"),
            ),
            (
                "region",
                expr::traversal(["data", "aws_region", "current", "region"]),
            ),
        ];
        if let Some(seconds) = sandbox.session.idle_suspend_seconds {
            fields.push((
                "idleSuspendSeconds",
                Expression::Number(i64::from(seconds).into()),
            ));
        }
        if let Some(seconds) = sandbox.session.max_lifetime_seconds {
            fields.push((
                "maxLifetimeSeconds",
                Expression::Number(i64::from(seconds).into()),
            ));
        }
        Ok(Some(expr::object(fields)))
    }
}

/// Attaches this sandbox's remote grant to the stack's shared Remote Bindings identity.
///
/// The grant is authorized against the image name rather than its ARN: the permission set builds
/// the ARN itself from `${stackPrefix}-${resourceName}`, and the image is named from the same two
/// parts here.
fn emit_remote_bindings_policy(ctx: &EmitContext<'_>, fragment: &mut TfFragment) -> Result<()> {
    let (Some(definition), Some(access_label)) = (
        alien_core::remote_bindings::remote_binding_for_entry(ctx.resource),
        remote_bindings_label(ctx),
    ) else {
        return Ok(());
    };
    let permission = PermissionSetReference::from_name(definition.permission_set);
    let Some(permission_set) =
        permission.resolve(|name| alien_permissions::get_permission_set(name).cloned())
    else {
        return Ok(());
    };

    let context =
        aws_terraform_permission_context().with_resource_name(ctx.resource_id.to_string());
    emit_iam_role_policy_for_target_with_label(
        fragment,
        access_label,
        &permission_set,
        &format!("{access_label}_{}_remote_execute", ctx.resource_id),
        &format!(
            "access-{}-{}",
            ctx.resource_id,
            iam_policy_name_sanitize(&permission_set.id)
        ),
        &context,
        BindingTarget::Resource,
    )
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}

/// The ports a preview capability may be minted for.
///
/// Carried into both the import data and the binding because minting is where ingress is granted:
/// `CreateMicrovmAuthToken` will scope a token to any port it is asked for, so the declared list
/// has to reach the code that mints or the declaration bounds nothing.
/// One attribute of the created image.
///
/// Cloud Control returns the whole resource as a JSON string in `properties`, so an attribute is
/// read out of it rather than off the resource — there are no typed attributes to reach for.
fn image_property(label: &str, property: &str) -> Expression {
    expr::raw(format!(
        "jsondecode({MICROVM_IMAGE_RESOURCE}.{label}.properties)[\"{property}\"]"
    ))
}

fn preview_ports(sandbox: &Sandbox) -> Expression {
    Expression::from(
        sandbox
            .preview_ports
            .iter()
            .map(|port| Expression::Number(i64::from(*port).into()))
            .collect::<Vec<_>>(),
    )
}

/// The four tags every emitter writes, as `{key, value}` objects.
///
/// `snake` picks the spelling: the connector goes through `awscc`, which rewrites the schema's
/// names, while the image body is Cloud Control and carries them as the schema declares them.
fn tag_objects(ctx: &EmitContext<'_>, snake: bool) -> Expression {
    // The same keys every other emitter writes. Ownership and the setup-vs-runtime split key off
    // `managed-by`, so a private spelling here would leave the image invisible to both — and make
    // the two package formats disagree about the resource they each create.
    let pairs: Vec<(&str, Expression)> = vec![
        (
            ALIEN_MANAGED_BY_TAG_KEY,
            Expression::String("setup".to_string()),
        ),
        (ALIEN_STACK_TAG_KEY, expr::raw("local.resource_prefix")),
        (
            ALIEN_RESOURCE_TAG_KEY,
            Expression::String(ctx.resource_id.to_string()),
        ),
        ("resource-type", Expression::String("sandbox".to_string())),
    ];

    Expression::from(
        pairs
            .into_iter()
            .map(|(key, value)| {
                Expression::from_iter([
                    (
                        if snake { "key" } else { "Key" },
                        Expression::String(key.to_string()),
                    ),
                    (if snake { "value" } else { "Value" }, value),
                ])
            })
            .collect::<Vec<_>>(),
    )
}

/// The AWS-managed base image, which is region-scoped.
fn base_image_arn() -> Expression {
    Expression::from(hcl::TemplateExpr::QuotedString(
        "arn:${data.aws_partition.current.partition}:lambda:${data.aws_region.current.region}:aws:microvm-image:al2023-1".to_string(),
    ))
}

/// The one object the build reads, as an ARN.
///
/// The bundle URI is known when the module is emitted, so the build role is scoped to it rather
/// than to every object in the account. `s3://bucket/key` maps to `arn:<partition>:s3:::bucket/key`; a
/// URI without a key would be a bucket ARN, which `artifact_uri` has already refused.
fn artifact_object_arn(uri: &str) -> String {
    format!(
        "arn:${{data.aws_partition.current.partition}}:s3:::{}",
        uri.trim_start_matches("s3://")
    )
}

/// What Lambda may do while managing the connector's network interfaces.
///
/// Reproduces the role AWS documents as the prerequisite for creating a network connector, and
/// the contents of its `AWSLambdaNetworkConnectorOperatorPolicy`. Written out rather than
/// attached so the grant is visible in the module the customer reads and does not change under
/// them when AWS revises the managed policy.
fn operator_statements() -> Vec<Expression> {
    vec![
        Expression::from_iter([
            ("Sid", Expression::String("CreateENI".to_string())),
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::String("ec2:CreateNetworkInterface".to_string()),
            ),
            // Templates rather than plain strings: a string literal has its `${` escaped, so the
            // region and account would reach IAM as the literal text and the ARN is refused with
            // MalformedPolicyDocument — at apply, which is the only place it shows.
            (
                "Resource",
                Expression::from(
                    [
                        "arn:${data.aws_partition.current.partition}:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:network-interface/*",
                        "arn:${data.aws_partition.current.partition}:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:subnet/*",
                        "arn:${data.aws_partition.current.partition}:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:security-group/*",
                    ]
                    .map(expr::template)
                    .to_vec(),
                ),
            ),
        ]),
        Expression::from_iter([
            ("Sid", Expression::String("TagENI".to_string())),
            ("Effect", Expression::String("Allow".to_string())),
            ("Action", Expression::String("ec2:CreateTags".to_string())),
            (
                "Resource",
                expr::template(
                    "arn:${data.aws_partition.current.partition}:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:network-interface/*",
                ),
            ),
            (
                "Condition",
                Expression::from_iter([(
                    "StringEquals",
                    Expression::from_iter([(
                        "ec2:ManagedResourceOperator",
                        Expression::String("network-connectors.lambda.amazonaws.com".to_string()),
                    )]),
                )]),
            ),
        ]),
    ]
}

/// The private subnets the connector places its ENIs in.
///
/// A connector must name between one and sixteen subnets, and only a created or bring-your-own
/// VPC yields any. Refusing the other network modes here is what keeps the deny path honest: the
/// alternative is a connector expression that resolves to an empty list, and a session with no
/// connector reaches the public internet.
fn egress_subnet_ids(ctx: &EmitContext<'_>, sandbox: &Sandbox) -> Result<Expression> {
    let refuse = |reason: String| {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform emit sandbox '{}'", sandbox.id()),
            reason,
        }))
    };

    let Some((_label, network)) = default_network(ctx) else {
        return refuse(
            "an AWS sandbox routes session traffic through a VPC egress connector, and this \
             stack declares no network for it to attach to"
                .to_string(),
        );
    };

    match &network.settings {
        NetworkSettings::Create { .. } | NetworkSettings::ByoVpcAws { .. } => {
            Ok(private_subnet_ids_expr(ctx))
        }
        NetworkSettings::UseDefault => refuse(
            "an AWS sandbox routes session traffic through a VPC egress connector, which needs \
             private subnets; the account's default VPC has only public ones. Set the network \
             to create or byo-vpc-aws"
                .to_string(),
        ),
        _ => refuse(
            "an AWS sandbox routes session traffic through a VPC egress connector, and this \
             stack's network settings are for another cloud"
                .to_string(),
        ),
    }
}

/// The connectors a session starts with, which an open sandbox has none of.
///
/// Empty is not a missing value here: it is how `allow` is expressed on the wire, and
/// `allowEgress` travels beside it so a stripped `deny` cannot be mistaken for it.
fn egress_connector_arns(sandbox: &Sandbox, label: &str) -> Expression {
    match sandbox.egress {
        SandboxEgress::Allow => Expression::from(Vec::<Expression>::new()),
        _ => Expression::from(vec![expr::traversal([
            NETWORK_CONNECTOR_RESOURCE,
            label,
            "arn",
        ])]),
    }
}

/// Refuses an egress mode the emitted artifact cannot deliver.
///
/// `deny` is built from a connector whose security group carries no egress rule. Outbound
/// allowances are not: `allow` would depend on the network's NAT topology, and AWS has no
/// domain-filtering primitive at the connector, so `allowDomains` has nothing to render into.
/// Emitting a template that silently ignores a declared egress policy is worse than refusing it —
/// the customer would believe outbound access was configured.
fn refuse_unsupported_egress(sandbox: &Sandbox) -> Result<()> {
    let refuse = |mode: &str| {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform emit sandbox '{}'", sandbox.id()),
            reason: format!(
                "AWS sandboxes reach the network through a VPC egress connector, which this \
                 module builds to deny outbound traffic; egress '{mode}' has no connector \
                 configuration to render into. Declare egress: deny, or use a platform that \
                 supports it"
            ),
        }))
    };

    match &sandbox.egress {
        SandboxEgress::Deny | SandboxEgress::Allow => Ok(()),
        SandboxEgress::AllowDomains { .. } => refuse("allowDomains"),
    }
}

/// Resolves the S3 bundle the MicroVM image is built from.
///
/// A MicroVM image is built from a zip containing a Dockerfile, not from a container image
/// reference, and a Terraform module has nowhere to build one — the same reason the Worker
/// emitter refuses source. Requiring an `s3://` URI fails at plan time with something a reader
/// can act on, rather than at the end of a ~160s image build.
fn artifact_uri(sandbox: &Sandbox) -> Result<String> {
    let unsupported = |reason: String| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("terraform emit sandbox '{}'", sandbox.id()),
            reason,
        })
    };

    match &sandbox.code {
        // A bucket with no key would scope the build role to the whole bucket rather than the
        // one object, so it is refused here rather than silently widening the grant.
        SandboxCode::Image { image }
            if image.starts_with("s3://") && image.trim_start_matches("s3://").contains('/') =>
        {
            Ok(image.clone())
        }
        SandboxCode::Image { image } if image.starts_with("s3://") => Err(unsupported(format!(
            "code.image '{image}' names a bucket with no object key; give the full path to the \
             bundle, for example s3://bucket/sandbox.zip"
        ))),
        SandboxCode::Image { image } => Err(unsupported(format!(
            "AWS builds a MicroVM image from an S3 bundle containing a Dockerfile, so code.image \
             must be an s3:// URI, not the container reference '{image}'"
        ))),
        SandboxCode::Source { .. } => Err(unsupported(
            "AWS builds a MicroVM image from a prepared S3 bundle; Terraform modules cannot build \
             one from source"
                .to_string(),
        )),
    }
}

/// Lifecycle hooks, served by the agent on its own port.
///
/// `Run` and `Resume` are enabled because every MicroVM from one image shares the state resident
/// at capture — including the CSPRNG seed — so the reseed has to happen after each start.
///
/// `Ready` is not optional: AWS rejects an image that enables any MicroVM hook without it, and
/// it is what defers the snapshot until the agent is actually serving.
fn hooks() -> Expression {
    Expression::from_iter([
        ("Port", Expression::Number(AGENT_PORT.into())),
        (
            "MicrovmImageHooks",
            Expression::from_iter([
                ("Ready", Expression::String("ENABLED".to_string())),
                ("ReadyTimeoutInSeconds", Expression::Number(120.into())),
            ]),
        ),
        (
            "MicrovmHooks",
            Expression::from_iter([
                ("Run", Expression::String("ENABLED".to_string())),
                ("RunTimeoutInSeconds", Expression::Number(30.into())),
                ("Resume", Expression::String("ENABLED".to_string())),
                ("ResumeTimeoutInSeconds", Expression::Number(30.into())),
            ]),
        ),
    ])
}

/// The agent's configuration contract.
///
/// `ALIEN_SANDBOX_AUTHORIZATION` is `transport` on AWS: the proxy validates a token scoped to one
/// MicroVM before a request arrives, and one MicroVM is one session.
fn environment_variables() -> Expression {
    let pairs = [
        ("ALIEN_SANDBOX_ROOT", "/sandbox".to_string()),
        ("ALIEN_SANDBOX_PORT", AGENT_PORT.to_string()),
        ("ALIEN_SANDBOX_AUTHORIZATION", "transport".to_string()),
        ("ALIEN_SANDBOX_EXEC_UID", EXEC_UID.to_string()),
        ("ALIEN_SANDBOX_EXEC_GID", EXEC_UID.to_string()),
    ];

    Expression::from(
        pairs
            .into_iter()
            .map(|(key, value)| {
                Expression::from_iter([
                    ("Key", Expression::String(key.to_string())),
                    ("Value", Expression::String(value)),
                ])
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema's architecture enum has exactly one member, and the agent binary in the image
    /// has to match it. A change here without a matching build target is a ~160s image build
    /// that fails at the end.
    #[test]
    fn the_architecture_is_the_only_one_microvm_images_accept() {
        assert_eq!(ARCHITECTURE, "ARM_64");
    }

    /// A string literal has its `${` escaped, so an ARN written as one reaches IAM carrying the
    /// literal text instead of the account and region. IAM refuses the document, and only at
    /// apply: both the HCL and the JSON are well-formed until AWS reads the ARN.
    #[test]
    fn the_connector_policy_arns_interpolate_rather_than_escape() {
        for statement in operator_statements() {
            let rendered = statement.to_string();
            assert!(
                !rendered.contains("$${"),
                "an escaped interpolation reaches IAM as literal text: {rendered}"
            );
        }
    }

    /// `desired_state` is checked against the CloudFormation Resource Schema, which names every
    /// property in PascalCase and rejects anything else — but only once an apply reaches AWS, so
    /// a casing slip lands in a customer's install rather than in a test.
    #[test]
    fn the_image_properties_are_named_the_way_the_resource_schema_names_them() {
        let hooks = hooks().to_string();
        for property in [
            "Port",
            "MicrovmImageHooks",
            "Ready",
            "ReadyTimeoutInSeconds",
            "MicrovmHooks",
            "Run",
            "RunTimeoutInSeconds",
            "Resume",
            "ResumeTimeoutInSeconds",
        ] {
            assert!(
                hooks.contains(property),
                "hooks must carry {property}: {hooks}"
            );
        }
        // No property in this object is snake_case, so an underscore is the drift itself.
        assert!(
            !hooks.contains('_'),
            "a snake_case hook property is refused: {hooks}"
        );

        let variables = environment_variables().to_string();
        assert!(
            variables.contains("Key") && variables.contains("Value"),
            "{variables}"
        );
        // No key or value in the pairs contains these words, so a match is a lower-cased property.
        assert!(
            !variables.contains("key") && !variables.contains("value"),
            "an environment variable pair is Key/Value: {variables}"
        );
    }
}
