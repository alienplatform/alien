//! AWS Sandbox — a Lambda MicroVM image and the role that builds it.
//!
//! `AWS::Lambda::MicrovmImage` requires all thirteen of its properties, so nothing here can be
//! left out by omission. The shapes below were read from the live CloudFormation registry, not
//! translated from the Terraform emitter.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        cf_from_json, default_network, private_subnet_ids_expr, required_logical_id,
        resource_config, service_trust_policy, subnet_refs, tags, vpc_id_expr,
        CONDITION_NETWORK_MODE_CREATE, PARAM_PRIVATE_SUBNET_IDS,
    },
    emitters::aws::service_account::permission_context,
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, BundleUri, ErrorData, NetworkSettings, RemoteBindings, Result, Sandbox,
    SandboxCode, SandboxEgress,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

/// The only architecture MicroVM images accept — the schema's enum has one member, so the agent
/// in the image has to be an aarch64 Linux binary.
const ARCHITECTURE: &str = "ARM_64";

/// Port the in-sandbox agent serves, both its own protocol and the lifecycle hooks.
const AGENT_PORT: i64 = 8971;

/// Unprivileged identity commands run as inside the sandbox, never the agent's own.
const EXEC_UID: &str = "60000";

/// The one destination the session's security group permits, which reaches nothing.
const LOOPBACK_ONLY_CIDR: &str = "127.0.0.1/32";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsSandboxEmitter;

impl CfEmitter for AwsSandboxEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let sandbox = resource_config::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let image_id = required_logical_id(ctx)?;
        let role_id = format!("{image_id}BuildRole");

        let artifact_uri = artifact_uri(sandbox)?;
        refuse_unsupported_egress(sandbox)?;
        let egress = egress_network(ctx, sandbox)?;
        // The size whose peak stays inside the declared ceilings. A MicroVM bursts to four times
        // its baseline with no way to opt out, so the baseline is a quarter of what was declared.
        let tier = sandbox.microvm_tier()?;
        let operator_role_id = format!("{image_id}EgressOperatorRole");
        let security_group_id = format!("{image_id}EgressSecurityGroup");
        let connector_id = format!("{image_id}EgressConnector");

        let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
        // Named rather than left to CloudFormation: the grant that passes this role is scoped by
        // name, and a generated `<stack>-<logical id>-<random>` would not match the pattern the
        // Terraform module produces. One name means one pattern for both.
        //
        // Unclamped on purpose. `SandboxBuildRoleNameCheck` refuses at plan time any id that could
        // reach IAM's 64-character ceiling under the widest permitted prefix, so this cannot
        // overflow — and clamping is what would hurt: Terraform's generic clamp replaces the tail
        // with a hash, which drops the `-build` the pass grant matches and denies the image build
        // at runtime instead of failing here.
        role.properties.insert(
            "RoleName".to_string(),
            CfExpression::sub(format!("${{AWS::StackName}}-{}-build", sandbox.id())),
        );
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            service_trust_policy(["lambda.amazonaws.com"]),
        );
        role.properties
            .insert("Policies".to_string(), build_policies(artifact_uri));
        role.properties.insert("Tags".to_string(), tags(ctx));

        // An open sandbox routes nothing through a VPC, so none of this exists for it: no
        // connector to attach, no group to deny with, and no role to manage interfaces.
        let mut egress_resources = Vec::new();
        if let Some((vpc_id, subnet_ids)) = egress {
            // Lambda assumes this to manage the connector's network interfaces. AWS documents the
            // permissions it must hold; the property being optional is not a promise that AWS
            // provisions an equivalent role.
            let mut operator_role =
                CfResource::new(operator_role_id.clone(), "AWS::IAM::Role".to_string());
            operator_role.properties.insert(
                "AssumeRolePolicyDocument".to_string(),
                service_trust_policy(["lambda.amazonaws.com"]),
            );
            operator_role
                .properties
                .insert("Policies".to_string(), operator_policies());
            operator_role
                .properties
                .insert("Tags".to_string(), tags(ctx));

            let mut security_group = CfResource::new(
                security_group_id.clone(),
                "AWS::EC2::SecurityGroup".to_string(),
            );
            security_group.properties.insert(
                "GroupDescription".to_string(),
                CfExpression::from(format!("Sandbox {} session egress", sandbox.id()).as_str()),
            );
            security_group
                .properties
                .insert("VpcId".to_string(), vpc_id);
            // EC2 adds an allow-all egress rule to any group whose template states none, so the deny
            // has to be written down. Loopback-only is AWS's documented way to say it. Widening this
            // rule is the one edit that turns `egress: deny` back into internet access.
            security_group.properties.insert(
                "SecurityGroupEgress".to_string(),
                CfExpression::list([CfExpression::object([
                    ("IpProtocol", CfExpression::from("-1")),
                    ("CidrIp", CfExpression::from(LOOPBACK_ONLY_CIDR)),
                    (
                        "Description",
                        CfExpression::from("Sandbox sessions reach nothing outbound"),
                    ),
                ])]),
            );
            security_group
                .properties
                .insert("Tags".to_string(), tags(ctx));

            let mut connector = CfResource::new(
                connector_id.clone(),
                "AWS::Lambda::NetworkConnector".to_string(),
            );
            connector.properties.insert(
                "Name".to_string(),
                CfExpression::sub(format!("${{AWS::StackName}}-{}", sandbox.id())),
            );
            connector.properties.insert(
                "OperatorRole".to_string(),
                CfExpression::get_att(&operator_role_id, "Arn"),
            );
            connector.properties.insert(
                "Configuration".to_string(),
                CfExpression::object([(
                    "VpcEgressConfiguration",
                    CfExpression::object([
                        (
                            "AssociatedComputeResourceTypes",
                            CfExpression::list([CfExpression::from("MicroVm")]),
                        ),
                        // Documented optional in both the CloudControl schema and the
                        // CloudFormation reference, and rejected when absent: "NetworkProtocol
                        // cannot be null or empty for VPC_EGRESS connector". IPv4 rather than
                        // DualStack because the security group that carries the deny matches IPv4
                        // CIDRs — a v6 path would be outside it.
                        ("NetworkProtocol", CfExpression::from("IPv4")),
                        ("SubnetIds", subnet_ids),
                        (
                            "SecurityGroupIds",
                            CfExpression::list([CfExpression::get_att(
                                &security_group_id,
                                "GroupId",
                            )]),
                        ),
                    ]),
                )]),
            );
            connector.properties.insert("Tags".to_string(), tags(ctx));
            egress_resources.extend([operator_role, security_group, connector]);
        }

        let mut image = CfResource::new(
            image_id.to_string(),
            "AWS::Lambda::MicrovmImage".to_string(),
        );
        let properties = &mut image.properties;

        properties.insert(
            "Name".to_string(),
            CfExpression::sub(format!("${{AWS::StackName}}-{}", sandbox.id())),
        );
        properties.insert(
            "Description".to_string(),
            CfExpression::from(format!("Sandbox {}", sandbox.id()).as_str()),
        );
        properties.insert(
            "BaseImageArn".to_string(),
            CfExpression::sub(
                "arn:${AWS::Partition}:lambda:${AWS::Region}:aws:microvm-image:al2023-1",
            ),
        );
        properties.insert("BaseImageVersion".to_string(), CfExpression::from("1"));
        properties.insert(
            "BuildRoleArn".to_string(),
            CfExpression::get_att(&role_id, "Arn"),
        );
        properties.insert(
            "CodeArtifact".to_string(),
            CfExpression::object([("Uri", code_artifact_uri(artifact_uri))]),
        );
        // The switch behind "control plane never sees sandbox contents", not an approximation.
        properties.insert(
            "Logging".to_string(),
            CfExpression::object([("Disabled", CfExpression::from(true))]),
        );
        // AWS's own connector, not the deny connector this template creates. The image build
        // runs through whatever the image names and has to reach a registry; naming the deny
        // connector here leaves it nowhere to go and the image never becomes ACTIVE. A session's
        // egress is the connector passed at `RunMicrovm`, which is the deny one.
        properties.insert(
            "EgressNetworkConnectors".to_string(),
            CfExpression::list([CfExpression::sub(
                "arn:${AWS::Partition}:lambda:${AWS::Region}:aws:network-connector:aws-network-connector:INTERNET_EGRESS",
            )]),
        );
        properties.insert(
            "CpuConfigurations".to_string(),
            CfExpression::list([CfExpression::object([(
                "Architecture",
                CfExpression::from(ARCHITECTURE),
            )])]),
        );
        properties.insert(
            "Resources".to_string(),
            CfExpression::list([CfExpression::object([(
                "MinimumMemoryInMiB",
                CfExpression::Integer(tier.baseline_memory_mib),
            )])]),
        );
        // The enum has exactly one member, `ALL`, which grants mount, netns and eBPF. There is
        // no subset to request, so the answer is none.
        properties.insert(
            "AdditionalOsCapabilities".to_string(),
            CfExpression::list([]),
        );
        properties.insert("Hooks".to_string(), hooks());
        properties.insert("EnvironmentVariables".to_string(), environment_variables());
        properties.insert("Tags".to_string(), tags(ctx));

        let mut resources = vec![role];
        resources.append(&mut egress_resources);
        resources.push(image);
        if let Some(policy) = remote_access_policy(ctx)? {
            resources.push(policy);
        }
        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let sandbox = resource_config::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let image_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("previewPorts", preview_ports(sandbox)),
            (
                "egressConnectorArns",
                egress_connector_arns(sandbox, image_id),
            ),
            (
                "allowEgress",
                CfExpression::from(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
            // Both `GetMicrovmImage` and `RunMicrovm` require the ARN — measured against the
            // live API, where a bare name is refused with "Malformed ARN - doesn't start with
            // 'arn:'". `Ref` was measured to return it too, but the attribute says
            // so outright and is what the Terraform module uses for this field — one intrinsic
            // for one field, so the two formats cannot drift apart on it.
            (
                "imageIdentifier",
                CfExpression::get_att(image_id, "ImageArn"),
            ),
            ("imageArn", CfExpression::get_att(image_id, "ImageArn")),
            // `RunMicrovm` has no tags, so image plus version is the whole of session identity.
            // A stale version enumerates the wrong set and orphans live sessions.
            (
                "imageVersion",
                CfExpression::get_att(image_id, "LatestActiveImageVersion"),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let sandbox = resource_config::<Sandbox>(ctx, Sandbox::RESOURCE_TYPE)?;
        let image_id = required_logical_id(ctx)?;
        let mut fields = vec![
            ("service".to_string(), CfExpression::from("sandbox-aws")),
            ("previewPorts".to_string(), preview_ports(sandbox)),
            (
                "egressConnectorArns".to_string(),
                egress_connector_arns(sandbox, image_id),
            ),
            (
                "allowEgress".to_string(),
                CfExpression::from(matches!(sandbox.egress, SandboxEgress::Allow)),
            ),
            (
                "imageArn".to_string(),
                CfExpression::get_att(image_id, "ImageArn"),
            ),
            (
                "imageVersion".to_string(),
                CfExpression::get_att(image_id, "LatestActiveImageVersion"),
            ),
            ("region".to_string(), CfExpression::ref_("AWS::Region")),
        ];
        if let Some(seconds) = sandbox.session.idle_suspend_seconds {
            fields.push((
                "idleSuspendSeconds".to_string(),
                CfExpression::Integer(i64::from(seconds)),
            ));
        }
        if let Some(seconds) = sandbox.session.max_lifetime_seconds {
            fields.push((
                "maxLifetimeSeconds".to_string(),
                CfExpression::Integer(i64::from(seconds)),
            ));
        }
        Ok(Some(CfExpression::object(fields)))
    }
}

/// Attaches this sandbox's remote grant to the stack's shared Remote Bindings identity.
fn remote_access_policy(ctx: &EmitContext<'_>) -> Result<Option<CfResource>> {
    let (Some(definition), Some(access_logical_id)) = (
        alien_core::remote_bindings::remote_binding_is_deliverable(ctx.resource)
            .then(|| alien_core::remote_bindings::remote_binding_for_entry(ctx.resource))
            .flatten(),
        ctx.stack.resources().find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        }),
    ) else {
        return Ok(None);
    };
    let permission_set = alien_permissions::get_permission_set(definition.permission_set)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "permission set '{}' named by the sandbox Remote Bindings definition is missing",
                    definition.permission_set
                ),
            })
        })?;

    // The bare resource id: the generator renders `${stackPrefix}` as `${AWS::StackName}`, which
    // is how this template already names the image.
    let context = permission_context().with_resource_name(ctx.resource_id.to_string());
    let document = AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: format!(
                "could not generate the remote sandbox IAM policy for '{}'",
                ctx.resource_id
            ),
        })?;
    let document = cf_from_json(serde_json::to_value(document).into_alien_error().context(
        ErrorData::TemplateSerializationFailed {
            format: "CloudFormation IAM policy".to_string(),
            reason: "Failed to serialize the remote sandbox IAM policy".to_string(),
        },
    )?)?;

    let mut policy = CfResource::new(
        format!("{}RemoteExecutePolicy", required_logical_id(ctx)?),
        "AWS::IAM::Policy".to_string(),
    );
    policy.properties.insert(
        "PolicyName".to_string(),
        CfExpression::sub(format!(
            "${{AWS::StackName}}-{}-sandbox-access",
            ctx.resource_id
        )),
    );
    policy.properties.insert(
        "Roles".to_string(),
        CfExpression::list([CfExpression::ref_(format!("{access_logical_id}Role"))]),
    );
    policy
        .properties
        .insert("PolicyDocument".to_string(), document);
    Ok(Some(policy))
}

/// The ports a preview capability may be minted for.
///
/// Carried into both the import data and the binding because minting is where ingress is granted:
/// `CreateMicrovmAuthToken` will scope a token to any port it is asked for, so the declared list
/// has to reach the code that mints or the declaration bounds nothing.
fn preview_ports(sandbox: &Sandbox) -> CfExpression {
    CfExpression::list(
        sandbox
            .preview_ports
            .iter()
            .map(|port| CfExpression::Integer(i64::from(*port))),
    )
}

/// What the build role may do: read the bundle, and write its own build logs.
///
/// Scoped to the one object it reads. The bundle URI is known when the template is generated, so
/// there is no reason for the role a customer installs to carry account-wide object read.
fn build_policies(artifact_uri: BundleUri<'_>) -> CfExpression {
    CfExpression::list([CfExpression::object([
        ("PolicyName", CfExpression::from("sandbox-image-build")),
        (
            "PolicyDocument",
            CfExpression::object([
                ("Version", CfExpression::from("2012-10-17")),
                (
                    "Statement",
                    CfExpression::list([
                        CfExpression::object([
                            ("Effect", CfExpression::from("Allow")),
                            (
                                "Action",
                                CfExpression::list([CfExpression::from("s3:GetObject")]),
                            ),
                            (
                                "Resource",
                                // Partition-qualified like every other ARN here: a hardcoded
                                // `aws` never matches in GovCloud or China, and the build fails
                                // on the bundle it was granted.
                                artifact_object_arn(artifact_uri),
                            ),
                        ]),
                        CfExpression::object([
                            ("Effect", CfExpression::from("Allow")),
                            (
                                "Action",
                                CfExpression::list([
                                    // CreateLogGroup as well as the writes: the build creates no
                                    // group of its own, so without it the first build's logs go
                                    // nowhere. The house build role grants all three.
                                    CfExpression::from("logs:CreateLogGroup"),
                                    CfExpression::from("logs:CreateLogStream"),
                                    CfExpression::from("logs:PutLogEvents"),
                                ]),
                            ),
                            ("Resource", CfExpression::from("*")),
                        ]),
                    ]),
                ),
            ]),
        ),
    ])])
}

/// Lifecycle hooks, served by the agent on its own port.
///
/// `Run` and `Resume` are enabled because every MicroVM from one image shares the state resident
/// at capture, including the CSPRNG seed, so the reseed has to happen after each start.
///
/// `Ready` is not optional: AWS rejects an image that enables any MicroVM hook without it, and
/// it is what defers the snapshot until the agent is actually serving.
fn hooks() -> CfExpression {
    CfExpression::object([
        ("Port", CfExpression::Integer(AGENT_PORT)),
        (
            "MicrovmImageHooks",
            CfExpression::object([
                ("Ready", CfExpression::from("ENABLED")),
                ("ReadyTimeoutInSeconds", CfExpression::Integer(120)),
            ]),
        ),
        (
            "MicrovmHooks",
            CfExpression::object([
                ("Run", CfExpression::from("ENABLED")),
                ("RunTimeoutInSeconds", CfExpression::Integer(30)),
                ("Resume", CfExpression::from("ENABLED")),
                ("ResumeTimeoutInSeconds", CfExpression::Integer(30)),
            ]),
        ),
    ])
}

/// The agent's configuration contract.
///
/// `transport` authorization on AWS: the proxy validates a token scoped to one MicroVM before a
/// request arrives, and one MicroVM is one session.
fn environment_variables() -> CfExpression {
    let pairs = [
        ("ALIEN_SANDBOX_ROOT", "/sandbox".to_string()),
        ("ALIEN_SANDBOX_PORT", AGENT_PORT.to_string()),
        ("ALIEN_SANDBOX_AUTHORIZATION", "transport".to_string()),
        ("ALIEN_SANDBOX_EXEC_UID", EXEC_UID.to_string()),
        ("ALIEN_SANDBOX_EXEC_GID", EXEC_UID.to_string()),
    ];

    CfExpression::list(pairs.into_iter().map(|(key, value)| {
        CfExpression::object([
            ("Key", CfExpression::from(key)),
            ("Value", CfExpression::from(value.as_str())),
        ])
    }))
}

/// Resolves the S3 bundle the MicroVM image is built from.
///
/// A MicroVM image is built from a zip containing a Dockerfile, not from a container image
/// What Lambda may do while managing the connector's network interfaces.
///
/// Reproduces the role AWS documents as the prerequisite for creating a network connector, and
/// the contents of its `AWSLambdaNetworkConnectorOperatorPolicy`. Written out rather than
/// attached so the grant is visible in the template the customer reads and does not change under
/// them when AWS revises the managed policy.
fn operator_policies() -> CfExpression {
    CfExpression::list([CfExpression::object([
        ("PolicyName", CfExpression::from("sandbox-egress-connector")),
        (
            "PolicyDocument",
            CfExpression::object([
                ("Version", CfExpression::from("2012-10-17")),
                (
                    "Statement",
                    CfExpression::list([
                        CfExpression::object([
                            ("Sid", CfExpression::from("CreateENI")),
                            ("Effect", CfExpression::from("Allow")),
                            (
                                "Action",
                                CfExpression::from("ec2:CreateNetworkInterface"),
                            ),
                            (
                                "Resource",
                                CfExpression::list([
                                    CfExpression::sub("arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:network-interface/*"),
                                    CfExpression::sub("arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:subnet/*"),
                                    CfExpression::sub("arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:security-group/*"),
                                ]),
                            ),
                        ]),
                        CfExpression::object([
                            ("Sid", CfExpression::from("TagENI")),
                            ("Effect", CfExpression::from("Allow")),
                            ("Action", CfExpression::from("ec2:CreateTags")),
                            (
                                "Resource",
                                CfExpression::sub("arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:network-interface/*"),
                            ),
                            (
                                "Condition",
                                CfExpression::object([(
                                    "StringEquals",
                                    CfExpression::object([(
                                        "ec2:ManagedResourceOperator",
                                        CfExpression::from(
                                            "network-connectors.lambda.amazonaws.com",
                                        ),
                                    )]),
                                )]),
                            ),
                        ]),
                    ]),
                ),
            ]),
        ),
    ])])
}

/// The VPC and private subnets the connector attaches to.
///
/// A connector must name between one and sixteen subnets, and only a created or bring-your-own
/// VPC yields any. Refusing the other network modes here is what keeps the deny path honest: the
/// alternative is a template with no subnets, and a session with no connector reaches the public
/// internet.
fn egress_network(
    ctx: &EmitContext<'_>,
    sandbox: &Sandbox,
) -> Result<Option<(CfExpression, CfExpression)>> {
    let refuse = |reason: String| {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("cloudformation emit sandbox '{}'", sandbox.id()),
            reason,
        }))
    };

    if matches!(sandbox.egress, SandboxEgress::Allow) {
        // Nothing to attach: a MicroVM started with no connector keeps AWS's managed internet
        // path, which is what `allow` asks for, and needs no VPC to do it.
        return Ok(None);
    }

    let Some((network_id, network)) = default_network(ctx) else {
        return refuse(
            "an AWS sandbox routes session traffic through a VPC egress connector, and this \
             stack declares no network for it to attach to"
                .to_string(),
        );
    };

    match &network.settings {
        // Deliberately not `private_subnet_ids_expr`: its use-default branch resolves to
        // `AWS::NoValue`, and a connector with no subnets is a required property missing at
        // deploy — cfn-lint rejects it, and worse, it is the case where a session would run with
        // no connector at all. Falling through to the use-existing parameter instead means
        // use-default fails when CloudFormation creates the connector rather than silently.
        NetworkSettings::Create { .. } => Ok(Some((
            CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                CfExpression::ref_(format!("{network_id}Vpc")),
                CfExpression::ref_("VpcId"),
            ),
            CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                subnet_refs(network_id, "PrivateSubnet"),
                CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
            ),
        ))),
        NetworkSettings::ByoVpcAws { .. } => {
            Ok(Some((vpc_id_expr(ctx), private_subnet_ids_expr(ctx))))
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
/// Empty is not a missing value here: it is how `allow` is expressed on the wire, and the
/// binding carries `allowEgress` alongside so the two cannot be confused.
fn egress_connector_arns(sandbox: &Sandbox, image_id: &str) -> CfExpression {
    match sandbox.egress {
        SandboxEgress::Allow => CfExpression::list([]),
        _ => CfExpression::list([CfExpression::get_att(
            format!("{image_id}EgressConnector"),
            "Arn",
        )]),
    }
}

/// Refuses an egress mode the emitted template cannot deliver.
///
/// `deny` is built from a connector whose security group permits nothing outbound. Outbound
/// allowances are not: AWS has no domain-filtering primitive at the connector, so `allowDomains`
/// has nothing to render into. `allow` is accepted and emits no connector at all — a MicroVM
/// without one reaches the internet.
/// A template that silently ignores a declared egress policy is worse than one that refuses it.
fn refuse_unsupported_egress(sandbox: &Sandbox) -> Result<()> {
    let refuse = |mode: &str| {
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("cloudformation emit sandbox '{}'", sandbox.id()),
            reason: format!(
                "AWS sandboxes reach the network through a VPC egress connector, which this \
                 template builds to deny outbound traffic; egress '{mode}' has no connector \
                 configuration to render into. Declare egress: deny for a connector that reaches \
                 nothing, or egress: allow for no connector at all"
            ),
        }))
    };

    match &sandbox.egress {
        SandboxEgress::Deny | SandboxEgress::Allow => Ok(()),
        SandboxEgress::AllowDomains { .. } => refuse("allowDomains"),
    }
}

/// reference, and Alien has no build step producing one yet. Requiring an `s3://` URI fails at
/// plan time with something a reader can act on, rather than at the end of a ~160s image build.
fn artifact_uri(sandbox: &Sandbox) -> Result<BundleUri<'_>> {
    let unsupported = |reason: String| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("cloudformation emit sandbox '{}'", sandbox.id()),
            reason,
        })
    };

    match &sandbox.code {
        // A bucket with no key would scope the build role to the whole bucket rather than the
        // one object, so it is refused here rather than silently widening the grant.
        SandboxCode::Image { image }
            if image.starts_with("s3://") && image.trim_start_matches("s3://").contains('/') =>
        {
            alien_core::parse_bundle_uri(image).map_err(unsupported)
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
            "AWS builds a MicroVM image from a prepared S3 bundle; CloudFormation templates \
             cannot build one from source"
                .to_string(),
        )),
    }
}

/// The `CodeArtifact.Uri`, with the region resolved where the vendor asked for one.
///
/// A URI carrying no token stays a plain string rather than becoming a `Sub` with nothing to
/// substitute: every bundle configured today has none, and routing them through an expression
/// would rewrite every existing customer's template for no behaviour change.
fn code_artifact_uri(uri: BundleUri<'_>) -> CfExpression {
    match uri {
        BundleUri::Literal(uri) => CfExpression::from(uri),
        BundleUri::Regional { before, after } => {
            CfExpression::sub(format!("{before}${{AWS::Region}}{after}"))
        }
    }
}

/// The bundle object's ARN. Partition-qualified like every other ARN here — a hardcoded `aws`
/// never matches in GovCloud — and region-qualified for the same reason one region out.
fn artifact_object_arn(uri: BundleUri<'_>) -> CfExpression {
    let path = match uri {
        BundleUri::Literal(uri) => uri.trim_start_matches("s3://").to_string(),
        BundleUri::Regional { before, after } => format!(
            "{}${{AWS::Region}}{after}",
            before.trim_start_matches("s3://")
        ),
    };
    CfExpression::sub(format!("arn:${{AWS::Partition}}:s3:::{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGIONAL: &str = "s3://acme-artifacts-{region}/agents/bundle.zip";
    const LITERAL: &str = "s3://acme-artifacts-us-east-2/agents/bundle.zip";

    fn parsed(uri: &str) -> BundleUri<'_> {
        alien_core::parse_bundle_uri(uri).expect("the fixture parses")
    }

    /// The Sub string out of an intrinsic, so a test can compare what will actually be rendered.
    fn sub_text(expression: &CfExpression) -> &str {
        let CfExpression::Object(map) = expression else {
            panic!("expected an intrinsic, got {expression:?}");
        };
        let CfExpression::String(text) = &map["Fn::Sub"] else {
            panic!("expected Fn::Sub to carry a string");
        };
        text
    }

    /// A URI carrying no token must stay a plain string. Routing it through `Sub` would rewrite
    /// every template already in the field for no rendered change.
    #[test]
    fn a_uri_without_a_token_emits_the_plain_string_it_does_today() {
        assert_eq!(
            code_artifact_uri(parsed(LITERAL)),
            CfExpression::from(LITERAL),
            "a token-less URI must not become an intrinsic"
        );
    }

    /// Both consumers must name the same object. Asserting only that each mentions the region
    /// would pass while the two pointed at different buckets — which fails at build time as an
    /// access denial that reads like an IAM bug rather than a template bug.
    #[test]
    fn the_uri_and_the_build_grant_name_the_same_object() {
        let uri = code_artifact_uri(parsed(REGIONAL));
        let arn = artifact_object_arn(parsed(REGIONAL));

        let uri_text = sub_text(&uri);
        assert_eq!(
            sub_text(&arn),
            format!(
                "arn:${{AWS::Partition}}:s3:::{}",
                uri_text
                    .strip_prefix("s3://")
                    .expect("the URI stays an s3 URI")
            ),
            "the grant must name exactly the object the image is built from"
        );
        assert!(
            uri_text.contains("${AWS::Region}") && !uri_text.contains("{region}"),
            "the token must be consumed, not emitted verbatim: {uri_text}"
        );
    }

    /// The refusal has to land at plan time. A brace reaching S3 verbatim dies ~160s into the
    /// image build, which is the failure `artifact_uri` exists to move forward.
    #[test]
    fn a_token_this_build_cannot_resolve_is_refused_before_emitting() {
        let sandbox = Sandbox::new("sbx".to_string())
            .code(SandboxCode::Image {
                image: "s3://acme-artifacts-{regio}/bundle.zip".to_string(),
            })
            .limits(alien_core::SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(SandboxEgress::Allow)
            .session(alien_core::SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();

        let error = artifact_uri(&sandbox).expect_err("an unknown token must be refused");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    }

    /// The two emitters must agree: a sandbox declared once cannot mean different architectures
    /// or run as different uids depending on which package format the customer installed.
    #[test]
    fn the_agent_contract_matches_the_terraform_emitter() {
        assert_eq!(ARCHITECTURE, "ARM_64");
        assert_eq!(AGENT_PORT, 8971);
        assert_eq!(EXEC_UID, "60000");
        // Both formats deny by permitting one destination that reaches nothing. They agree by
        // asserting the same literal rather than by sharing a constant, which would make this
        // crate depend on the other package format.
        assert_eq!(LOOPBACK_ONLY_CIDR, "127.0.0.1/32");
    }
}
