//! AWS Sandbox — what `egress: deny` has to build for the template to mean it.

use super::helpers::{
    custom_resource_registration, render_built_ins_template, try_render_built_ins,
};
use alien_cloudformation::CloudFormationTarget;
use alien_core::{
    import::data::AwsSandboxImportData,
    sandbox_build_role::{SandboxBuildRole, SANDBOX_BUILD_POLICY_NAME},
    sandbox_egress::{
        sandbox_egress_operator_policy, sandbox_egress_operator_trust_policy,
        SandboxEgressConnector, LOOPBACK_ONLY_CIDR, SANDBOX_EGRESS_POLICY_NAME,
    },
    Network, NetworkSettings, RemoteBindings, ResourceLifecycle, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings, Worker, WorkerCode,
};
use serde_json::Value;

/// A bundle key a runtime rebuild can be granted: the version segment moves, the prefix does not.
/// A Frozen sandbox is built once and needs no such shape, so it keeps the flat key its snapshots
/// were taken with.
const LIVE_BUNDLE: &str = "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip";

fn sandbox_fixture(egress: SandboxEgress) -> Sandbox {
    sandbox_fixture_with(egress, "s3://acme-artifacts/agents/bundle.zip")
}

fn sandbox_fixture_with(egress: SandboxEgress, image: &str) -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: image.to_string(),
        })
        .egress(egress)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

/// A sandbox and the network its egress connector attaches to, which the emitter requires.
fn sandbox_stack(name: &str, egress: SandboxEgress) -> (Stack, StackSettings) {
    sandbox_stack_with_lifecycle(name, egress, ResourceLifecycle::Frozen)
}

fn sandbox_stack_with_lifecycle(
    name: &str,
    egress: SandboxEgress,
    lifecycle: ResourceLifecycle,
) -> (Stack, StackSettings) {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new(name.to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            match lifecycle {
                ResourceLifecycle::Live => sandbox_fixture_with(egress, LIVE_BUNDLE),
                _ => sandbox_fixture(egress),
            },
            lifecycle,
        )
        .build();
    (stack, settings)
}

/// The `sandbox-image-build` policy statements of the emitted build role, parsed as IAM sees
/// them — asserting on the serialized document catches an expression that renders wrong, not
/// just a missing string.
fn build_role_statements(template: &alien_cloudformation::CfTemplate) -> Vec<serde_json::Value> {
    let role = serde_json::to_value(
        template
            .resources
            .get("AgentsBuildRole")
            .expect("the build role must render"),
    )
    .expect("serializes");
    role["Properties"]["Policies"][0]["PolicyDocument"]["Statement"]
        .as_array()
        .unwrap_or_else(|| panic!("the build policy statements must be a list: {role:#}"))
        .clone()
}

/// The `Fn::Sub` text of the statement granting `s3:GetObject`, which is the bundle grant.
fn bundle_grant(template: &alien_cloudformation::CfTemplate) -> String {
    let statements = build_role_statements(template);
    let grant = statements
        .iter()
        .find(|statement| statement["Action"] == serde_json::json!(["s3:GetObject"]))
        .unwrap_or_else(|| panic!("the build role must read its bundle: {statements:#?}"));
    grant["Resource"]["Fn::Sub"]
        .as_str()
        .unwrap_or_else(|| panic!("the grant must be partition-qualified through Sub: {grant:#}"))
        .to_string()
}

/// Whether a parsed IAM statement carries any `ecr:` action.
fn grants_ecr(statement: &serde_json::Value) -> bool {
    statement["Action"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action.as_str().is_some_and(|a| a.starts_with("ecr:")))
    })
}

/// Every resource type the rendered template declares.
fn resource_types(template: &alien_cloudformation::CfTemplate) -> Vec<String> {
    template
        .resources
        .values()
        .map(|resource| resource.resource_type.clone())
        .collect()
}

/// The `importData` a rendered registration carries for one resource id.
///
/// CloudFormation intrinsics stand in for values only the deployed stack knows, so each is
/// replaced by a placeholder string — the shape is what the importer contract is judged on, not
/// the resolved values.
fn registration_import_data(
    template: &alien_cloudformation::CfTemplate,
    resource_id: &str,
) -> serde_json::Value {
    fn resolve(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                if map
                    .keys()
                    .any(|key| key.starts_with("Fn::") || key == "Ref")
                {
                    return serde_json::Value::String("resolved-at-deploy-time".to_string());
                }
                serde_json::Value::Object(
                    map.iter().map(|(k, v)| (k.clone(), resolve(v))).collect(),
                )
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(resolve).collect())
            }
            other => other.clone(),
        }
    }

    resolve(&emitted_import_data(template, resource_id))
}

/// The `importData` a rendered registration carries for one resource id, intrinsics and all.
fn emitted_import_data(
    template: &alien_cloudformation::CfTemplate,
    resource_id: &str,
) -> serde_json::Value {
    fn find(value: &serde_json::Value, resource_id: &str) -> Option<serde_json::Value> {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("id").and_then(serde_json::Value::as_str) == Some(resource_id) {
                    if let Some(import_data) = map.get("importData") {
                        return Some(import_data.clone());
                    }
                }
                map.values().find_map(|nested| find(nested, resource_id))
            }
            serde_json::Value::Array(items) => {
                items.iter().find_map(|nested| find(nested, resource_id))
            }
            _ => None,
        }
    }

    let rendered = serde_json::to_value(&template.resources).expect("serializes");
    find(&rendered, resource_id)
        .unwrap_or_else(|| panic!("no registration importData for '{resource_id}'"))
}

/// The image must be gone — it can only be built once the customer's account is a principal
/// Alien's registry has opened to, not true during stack creation — and the build role must
/// stay, since `sandbox/provision` grants the controller `iam:PassRole` but no `iam:CreateRole`.
#[test]
fn a_live_sandbox_ships_its_build_role_but_not_its_image() {
    let (stack, settings) = sandbox_stack_with_lifecycle(
        "acme-sandbox-live",
        SandboxEgress::Deny,
        ResourceLifecycle::Live,
    );
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "live sandbox",
    );

    let types = resource_types(&template);
    assert!(
        !types.iter().any(|t| t == "AWS::Lambda::MicrovmImage"),
        "a Live sandbox must not bake its image into stack creation: {types:?}"
    );
    assert!(
        template.resources.contains_key("AgentsBuildRole"),
        "the build role the controller passes must still be installed: {types:?}"
    );
    assert!(
        template.resources.contains_key("AgentsEgressConnector"),
        "the connector the build and the session are passed must still be installed: {types:?}"
    );

    // The registration has to carry the two things the controller cannot derive, and must not
    // carry a GetAtt against the image resource this template does not create.
    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    assert!(
        rendered.contains("buildRoleArn"),
        "registration must name the build role: {rendered}"
    );
    assert!(
        rendered.contains("bundleUri"),
        "registration must name the bundle the controller builds from: {rendered}"
    );
    assert!(
        !rendered.contains("LatestActiveImageVersion"),
        "no attribute of an image that is not created may be read: {rendered}"
    );

    // Setup registration builds its expected set from `emits_setup_scaffolding` and refuses one
    // missing any of them (`alien-manager/src/routes/stack.rs`), which is why the emitter
    // returns a runtime import ref instead of nothing — dropping it fails every install.
    assert!(
        rendered.contains("\"agents\""),
        "the sandbox must still register under its own id: {rendered}"
    );

    // The registration is the last step of a customer's install, and the contract it is parsed
    // against lives in another crate. Asserting the rendered payload merely *mentions*
    // `buildRoleArn` would pass while the importer rejected the whole object — so parse it.
    let import_data = registration_import_data(&template, "agents");
    let parsed: AwsSandboxImportData =
        serde_json::from_value(import_data.clone()).unwrap_or_else(|error| {
            panic!("the importer must accept what the emitter renders: {error}\n{import_data:#}")
        });
    assert_eq!(parsed.image_arn, None, "there is no image to name yet");
    assert_eq!(parsed.image_version, None);
    assert!(
        parsed.build_role_arn.is_some(),
        "the controller is handed the role it may only pass: {import_data:#}"
    );
    assert!(
        parsed.bundle_uri.is_some(),
        "the controller is handed the bundle it builds from: {import_data:#}"
    );

    // No `privateBaseImage`, so the base is public and pulled anonymously.
    let statements = build_role_statements(&template);
    assert!(
        !statements.iter().any(grants_ecr),
        "a build with no private base must hold no ECR grant: {statements:#?}"
    );
}

/// The declared base is the one repository the build may pull, in whichever region the host
/// names or the deployment's own when it names `{region}`.
#[test]
fn a_declared_private_base_is_the_only_repository_a_live_build_pulls() {
    let (stack, settings) = sandbox_stack_with_lifecycle(
        "acme-sandbox-live-base",
        SandboxEgress::Allow,
        ResourceLifecycle::Live,
    );
    let mut stack = stack;
    let sandbox = stack
        .resources
        .get_mut("agents")
        .and_then(|entry| entry.config.downcast_mut::<Sandbox>())
        .expect("the sandbox is in the stack");
    sandbox.private_base_image =
        Some("123456789012.dkr.ecr.{region}.amazonaws.com/acme/agents-base:1.4".to_string());
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "live sandbox with a private base",
    );

    let statements = build_role_statements(&template);
    let ecr: Vec<_> = statements.iter().filter(|s| grants_ecr(s)).collect();
    assert_eq!(
        ecr,
        [
            &serde_json::json!({
                "Sid": "AuthorizeSandboxBaseImagePull",
                "Effect": "Allow",
                "Action": ["ecr:GetAuthorizationToken"],
                "Resource": "*"
            }),
            &serde_json::json!({
                "Sid": "PullSandboxBaseImage",
                "Effect": "Allow",
                "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                "Resource": {
                    "Fn::Sub":
                        "arn:${AWS::Partition}:ecr:${AWS::Region}:123456789012:repository/acme/agents-base"
                }
            }),
            &serde_json::json!({
                "Sid": "DenySameAccountImagePull",
                "Effect": "Deny",
                "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                "Resource": {
                    "Fn::Sub": "arn:${AWS::Partition}:ecr:*:${AWS::AccountId}:repository/*"
                }
            }),
        ],
        "the token call on `*`, the pull on exactly the declared repository, and no pull from \
         this account"
    );
}

/// A grant naming the one object the template was generated with denies the build the first
/// time the base image changes its key — exactly the update Live exists to avoid. The Frozen
/// case here is the mutation guard: the same code path must not widen its grant too.
#[test]
fn only_a_live_build_role_reads_the_bundle_prefix() {
    let (live, live_settings) = sandbox_stack_with_lifecycle(
        "acme-sandbox-live-grant",
        SandboxEgress::Deny,
        ResourceLifecycle::Live,
    );
    let (live_template, _yaml) = render_built_ins_template(
        &live,
        live_settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "live sandbox",
    );
    let (frozen, frozen_settings) = sandbox_stack("acme-sandbox-frozen-grant", SandboxEgress::Deny);
    let (frozen_template, _yaml) = render_built_ins_template(
        &frozen,
        frozen_settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "frozen sandbox",
    );

    assert_eq!(
        bundle_grant(&frozen_template),
        "arn:${AWS::Partition}:s3:::acme-artifacts/agents/bundle.zip",
        "a Frozen role reads the one object its template names, and nothing else"
    );
    assert_eq!(
        bundle_grant(&live_template),
        "arn:${AWS::Partition}:s3:::acme-artifacts/sandbox-bundle/*",
        "a Live role reads the prefix the moving key stays inside"
    );
}

/// The Frozen path is the one every installed stack is on.
#[test]
fn a_frozen_sandbox_still_bakes_its_image_into_the_setup_stack() {
    let (stack, settings) = sandbox_stack("acme-sandbox-frozen", SandboxEgress::Deny);
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "frozen sandbox",
    );

    let types = resource_types(&template);
    assert!(
        types.iter().any(|t| t == "AWS::Lambda::MicrovmImage"),
        "a Frozen sandbox is built by stack creation: {types:?}"
    );
    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    assert!(
        rendered.contains("LatestActiveImageVersion"),
        "registration reads the version off the image it created: {rendered}"
    );
    assert!(
        !rendered.contains("buildRoleArn"),
        "a Frozen sandbox hands the controller nothing to build with: {rendered}"
    );

    let import_data = registration_import_data(&template, "agents");
    let parsed: AwsSandboxImportData = serde_json::from_value(import_data.clone())
        .unwrap_or_else(|error| panic!("the importer must accept it: {error}\n{import_data:#}"));
    assert!(
        parsed.image_arn.is_some() && parsed.image_version.is_some(),
        "a setup-built sandbox registers the image it created: {import_data:#}"
    );
    assert_eq!(parsed.build_role_arn, None);

    // A setup-baked image pulls its public base anonymously; an ECR grant here would hand the
    // role running a customer-authored Dockerfile pull access it never needs.
    let statements = build_role_statements(&template);
    assert!(
        !statements.iter().any(grants_ecr),
        "a Frozen build role must carry no ECR action: {statements:#?}"
    );

    // See `build_role_trust_policy` for why the condition belongs on this statement.
    let role = serde_json::to_value(
        template
            .resources
            .get("AgentsBuildRole")
            .expect("the build role must render"),
    )
    .expect("serializes");
    assert_eq!(
        role["Properties"]["AssumeRolePolicyDocument"]["Statement"][0]["Condition"],
        serde_json::json!({ "StringEquals": { "aws:SourceAccount": { "Ref": "AWS::AccountId" } } }),
        "the trust policy must be conditioned on the stack's own account: {role:#}"
    );
}

/// Open + Live is the leanest emitted combination — no image (built at runtime) and no egress
/// apparatus (an open session starts without a connector) — and no other test renders it.
#[test]
fn an_open_live_sandbox_ships_only_the_build_role() {
    let stack = Stack::new("acme-sandbox-open-live".to_string())
        .add(
            sandbox_fixture_with(SandboxEgress::Allow, LIVE_BUNDLE),
            ResourceLifecycle::Live,
        )
        .build();
    // No network in the stack at all: an open sandbox must not need one.
    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "open live sandbox",
    );

    assert!(
        template.resources.contains_key("AgentsBuildRole"),
        "the build role the controller passes must still be installed"
    );
    let types = resource_types(&template);
    assert!(
        !types.iter().any(|t| t == "AWS::Lambda::MicrovmImage"),
        "a Live sandbox must not bake its image into stack creation: {types:?}"
    );
    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    for absent in [
        "EgressConnector",
        "EgressSecurityGroup",
        "EgressOperatorRole",
    ] {
        assert!(
            !rendered.contains(absent),
            "an open sandbox must not render {absent}:\n{rendered}"
        );
    }
    let description = template.description.as_deref().expect("a description");
    assert!(
        description.contains("built after the deployment registers"),
        "the one line every console shows must caveat the runtime build: {description}"
    );
}

/// A Kubernetes target skips the sandbox emitter, so the stack description must not caveat a
/// runtime image build that never happens.
#[test]
fn a_kubernetes_target_description_makes_no_runtime_build_promise() {
    let (stack, settings) = sandbox_stack_with_lifecycle(
        "acme-sandbox-eks-live",
        SandboxEgress::Deny,
        ResourceLifecycle::Live,
    );
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Eks,
        "eks",
        "live sandbox on a kubernetes target",
    );
    let description = template.description.as_deref().expect("a description");
    assert!(
        !description.contains("built after the deployment registers"),
        "no sandbox is emitted on a Kubernetes target, so no build step may be described: \
         {description}"
    );
}

/// `egress: deny` has to be built, not assumed.
///
/// A MicroVM started with no egress connector reaches the public internet — verified against a
/// live account. The connector is what puts session traffic inside the VPC, and the security
/// group is what stops it there: EC2 adds an allow-all egress rule to any group whose template
/// states none, so the only rule present must be the one that reaches nothing.
#[test]
fn aws_sandbox_deny_builds_a_connector_that_permits_nothing_outbound() {
    let (stack, settings) = sandbox_stack("acme-sandbox-deny", SandboxEgress::Deny);
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox deny",
    );

    // The group's rules and the connector's configuration are compared whole by the parity
    // tests below.
    let connector = template
        .resources
        .get("AgentsEgressConnector")
        .expect("the egress connector must render");
    assert_eq!(connector.resource_type, "AWS::Lambda::NetworkConnector");

    let image = template
        .resources
        .get("Agents")
        .expect("the MicroVM image must render");
    let connectors = serde_json::to_string(
        image
            .properties
            .get("EgressNetworkConnectors")
            .expect("the image's connector list"),
    )
    .expect("serializes");
    // The switch that keeps session output out of the control plane's reach. It has no test of
    // its own anywhere else, so a one-sided edit re-enabling it would ship green.
    let logging = serde_json::to_string(
        image
            .properties
            .get("Logging")
            .expect("the image must state its logging"),
    )
    .expect("serializes");
    assert!(
        logging.contains("\"Disabled\":true"),
        "content-bearing logging must be off: {logging}"
    );

    // The build's route, not the session's. Naming the deny connector here would leave the image
    // build with nowhere to reach a registry, and it would never become ACTIVE.
    assert!(
        connectors.contains("INTERNET_EGRESS"),
        "the image must build through AWS's own connector: {connectors}"
    );
    assert!(
        !connectors.contains("AgentsEgressConnector"),
        "the deny connector belongs to the session, not the build: {connectors}"
    );

    // And the session's own connector still reaches the binding, which is what actually bounds
    // a running sandbox.
    let registration = serde_json::to_string(&template.resources).expect("serializes");
    assert!(
        registration.contains("AgentsEgressConnector"),
        "the deny connector must still be carried to the session"
    );
}

/// The connector attaches where `sandbox_egress_network` says, which is the stack's first network.
/// A created network is the one whose subnets the template names by logical id, so the connector
/// names them exactly when the created network is the first one.
#[test]
fn a_deny_sandbox_attaches_to_the_first_of_two_networks() {
    let created = || NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let brought = || NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0brought".to_string(),
        public_subnet_ids: vec!["subnet-public-a".to_string()],
        private_subnet_ids: existing_subnets(),
        security_group_ids: vec!["sg-0network".to_string()],
    };
    for (first, second) in [(created(), brought()), (brought(), created())] {
        let stack = Stack::new("acme-sandbox-two-networks".to_string())
            .add(
                Network::new("first-net".to_string())
                    .settings(first.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Network::new("second-net".to_string())
                    .settings(second.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                sandbox_fixture_with(SandboxEgress::Deny, LIVE_BUNDLE),
                ResourceLifecycle::Live,
            )
            .build();
        let chosen =
            alien_core::sandbox_egress::sandbox_egress_network(&stack, &SandboxEgress::Deny)
                .expect("both networks are attachable")
                .expect("deny attaches to a network");
        assert_eq!(chosen.id, "first-net");

        let (template, _yaml) = render_built_ins_template(
            &stack,
            StackSettings {
                network: Some(first.clone()),
                ..StackSettings::default()
            },
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            &format!("deny sandbox on {first:?} then {second:?}"),
        );
        let subnets = serde_json::to_string(
            &emitted_properties(&template, "AgentsEgressConnector")["Configuration"]
                ["VpcEgressConfiguration"]["SubnetIds"],
        )
        .expect("serializes");
        let names_created_subnets =
            |network: &str| subnets.contains(&format!("{network}PrivateSubnet"));
        match first {
            NetworkSettings::Create { .. } => assert!(
                names_created_subnets("FirstNet") && !names_created_subnets("SecondNet"),
                "the first network is the created one: {subnets}"
            ),
            _ => assert!(
                !names_created_subnets("FirstNet") && !names_created_subnets("SecondNet"),
                "the first network is brought, so no created subnet is named: {subnets}"
            ),
        }
    }
}

/// Without a VPC there are no subnets, and a connector needs between one and sixteen.
///
/// Rendering one anyway would produce either a deploy-time failure the reader cannot act on or —
/// worse — a session with no connector, which is the case that reaches the internet.
#[test]
fn aws_sandbox_refuses_to_render_without_a_network_to_attach_to() {
    let stack = Stack::new("acme-sandbox-no-network".to_string())
        .add(
            sandbox_fixture(SandboxEgress::Deny),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox without a network",
    )
    .expect_err("a sandbox with no network must be refused at emit time");
    assert!(
        error.message.contains("declares no network"),
        "the refusal must name why: {}",
        error.message
    );
}

/// An open sandbox builds no connector, and needs no VPC to build one in.
///
/// `allow` is a session started with no egress connector, which leaves AWS's managed internet
/// path in place. Rendering the deny apparatus anyway would demand a VPC from a stack that never
/// routes through one, and would attach a connector that contradicts the declaration.
#[test]
fn aws_sandbox_allowing_egress_builds_no_connector() {
    let stack = Stack::new("acme-sandbox-open".to_string())
        .add(
            sandbox_fixture(SandboxEgress::Allow),
            ResourceLifecycle::Frozen,
        )
        .build();

    // No network in the stack at all: an open sandbox must not need one.
    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "open sandbox",
    );

    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    for absent in [
        "EgressConnector",
        "EgressSecurityGroup",
        "EgressOperatorRole",
    ] {
        assert!(
            !rendered.contains(absent),
            "an open sandbox must not render {absent}:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("MicrovmImage"),
        "the image is still the sandbox's durable parent:\n{rendered}"
    );
    assert!(
        rendered.contains("allowEgress"),
        "the binding must say the empty connector list means open, not stripped:\n{rendered}"
    );
}

/// An egress mode the artifact cannot deliver is refused, not dropped.
///
/// AWS offers no domain filter at the connector, so `allowDomains` has nothing to render into and
/// would otherwise be silently ignored while the customer believed it applied.
#[test]
fn aws_sandbox_refuses_an_egress_mode_it_cannot_deliver() {
    for mode in [SandboxEgress::AllowDomains {
        domains: vec!["example.com".to_string()],
    }] {
        let (stack, settings) = sandbox_stack("acme-sandbox-egress", mode.clone());
        let error = try_render_built_ins(
            &stack,
            settings,
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            "sandbox egress",
        )
        .expect_err(&format!("egress {mode:?} must be refused at emit time"));
        assert!(
            error.message.contains("VPC egress connector"),
            "the refusal must name why: {}",
            error.message
        );
    }
}

/// The withheld mode must not still be the parameter's default — see `add_network_parameters`
/// for why a stack can reach this mismatch. Asserted through cfn-lint, which is what
/// CloudFormation itself would say.
#[test]
fn a_restricted_sandbox_never_defaults_to_the_mode_it_withholds() {
    let settings = StackSettings {
        network: Some(NetworkSettings::UseDefault),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-sandbox-default-clash".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            sandbox_fixture(SandboxEgress::Deny),
            ResourceLifecycle::Frozen,
        )
        .build();

    let (template, yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "restricted sandbox with a default-network setting",
    );

    let network_mode = template
        .parameters
        .get("NetworkMode")
        .expect("the network mode parameter must render");
    let rendered = serde_json::to_string(network_mode).expect("parameter serializes");
    assert!(
        !rendered.contains("\"use-default\""),
        "the withheld mode must appear neither as a value nor as the default: {rendered}"
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("restricted sandbox default clamp");
}

/// A sandbox on a Kubernetes target is a pod bounded by the chart, not cloud infrastructure.
///
/// Emitter lookup keys off the cloud the cluster runs in, so without a skip an EKS install
/// provisions the MicroVM image, connector, security group and roles of a backend the Kubernetes
/// runtime never uses, and registers import data naming it. Terraform already refuses; one
/// declaration has to install the same thing in both formats.
#[test]
fn an_eks_install_provisions_none_of_the_microvm_backend() {
    let (stack, settings) = sandbox_stack("acme-sandbox-on-eks", SandboxEgress::Deny);

    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Eks,
        "eks",
        "sandbox on a kubernetes target",
    );

    for absent in ["AWS::Lambda::MicrovmImage", "AWS::Lambda::NetworkConnector"] {
        let rendered = serde_json::to_string(&template.resources).expect("resources serialize");
        assert!(
            !rendered.contains(absent),
            "an EKS install must not provision {absent}"
        );
    }

    // The mode is withheld only where a connector actually demands subnets, so an EKS installer
    // keeps an option that works. Offering it is not enough on its own: the condition and the
    // expressions that branch on it have to exist too, or the installer picks a documented answer
    // and gets a template that renders the BYO branch with empty parameters.
    let network_mode = template
        .parameters
        .get("NetworkMode")
        .expect("the network mode parameter must render");
    let rendered = serde_json::to_string(network_mode).expect("parameter serializes");
    assert!(
        rendered.contains("use-default"),
        "no sandbox is emitted here, so nothing forces named subnets: {rendered}"
    );
    assert!(
        template.conditions.contains_key("NetworkModeUseExisting"),
        "a template offering use-default has to keep the condition its branches read"
    );
    let settings = serde_json::to_string(&template.outputs).expect("outputs serialize")
        + &serde_json::to_string(&template.resources).expect("resources serialize");
    assert!(
        settings.contains("use-default"),
        "the mode is offered, so something has to render its branch: {}",
        &settings[..settings.len().min(400)]
    );
}

/// Complete rendered templates for the three sandbox shapes a customer can install.
///
/// The sandbox is the largest surface this adds — an IAM role, an egress connector and a
/// loopback-only deny rule — and until now no test read it as a whole artifact. Snapshots also
/// reach the two sites substring assertions cannot: the network expression a compute emitter
/// consumes, and the `default-network` registration payload. The Live worker is there to show a
/// Live workload contributes nothing to a setup surface — it does not exercise
/// `created_or_provided`, which on a Kubernetes target only a cluster resource reaches.
#[test]
fn the_sandbox_templates_render_whole() {
    let worker = || {
        Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
            })
            .permissions("execution".to_string())
            .build()
    };

    for (name, egress, target, setup_target) in [
        (
            "sandbox_restricted_aws",
            SandboxEgress::Deny,
            CloudFormationTarget::Aws,
            "aws",
        ),
        (
            "sandbox_open_aws",
            SandboxEgress::Allow,
            CloudFormationTarget::Aws,
            "aws",
        ),
        (
            "sandbox_restricted_eks",
            SandboxEgress::Deny,
            CloudFormationTarget::Eks,
            "eks",
        ),
    ] {
        let (stack, settings) = sandbox_stack(name, egress.clone());
        let stack = Stack::new(stack.id.clone())
            .add(
                Network::new("default-network".to_string())
                    .settings(settings.network.clone().expect("network"))
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(sandbox_fixture(egress), ResourceLifecycle::Frozen)
            .add(worker(), ResourceLifecycle::Live)
            .build();

        let (_template, yaml) = render_built_ins_template(
            &stack,
            settings,
            custom_resource_registration(),
            target,
            setup_target,
            name,
        );
        insta::assert_snapshot!(name, yaml);
    }
}

/// The grant a remote caller's credentials are bounded by.
///
/// The setup package is where the Remote Bindings identity gets its policies, so without this the
/// manager mints a session against a role that carries none. Nothing else pins the statement
/// `Sid`s, which follow the permission labels.
#[test]
fn aws_remote_sandbox_grants_the_access_identity_its_own_image_and_nothing_wider() {
    let stack = Stack::new("byo-sandbox".to_string())
        .add_with_remote_access(
            sandbox_fixture(SandboxEgress::Allow),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "remote sandbox",
    );
    insta::assert_snapshot!("remote_sandbox_grant_aws", yaml);

    let policy = template
        .resources
        .get("AgentsRemoteExecutePolicy")
        .expect("the remote grant must attach to the Remote Bindings role");
    assert_eq!(policy.resource_type, "AWS::IAM::Policy");
    let roles =
        serde_json::to_string(policy.properties.get("Roles").expect("Roles")).expect("serializes");
    assert!(
        roles.contains("AccessRole"),
        "the grant belongs to the shared Remote Bindings identity: {roles}"
    );

    let document = serde_json::to_string(
        policy
            .properties
            .get("PolicyDocument")
            .expect("PolicyDocument"),
    )
    .expect("serializes");
    for action in [
        "lambda:RunMicrovm",
        "lambda:SuspendMicrovm",
        "lambda:ResumeMicrovm",
        "lambda:TerminateMicrovm",
        "lambda:CreateMicrovmAuthToken",
        "lambda:GetMicrovm",
        // AWS attaches its own INTERNET_EGRESS and HTTP_INGRESS connectors to an open-egress
        // session and authorizes each as PassNetworkConnector, so without this nothing starts.
        "lambda:PassNetworkConnector",
    ] {
        assert!(document.contains(action), "{action} is missing: {document}");
    }
    assert!(
        document.contains("microvm-image:${AWS::StackName}-agents"),
        "the grant must name this sandbox's own image: {document}"
    );
    // AWS's own connectors sit under the literal account `aws`; a customer-declared one carries
    // the customer's account id, so this scope cannot name one.
    assert!(
        document.contains("aws:network-connector:aws-network-connector:*"),
        "the connector grant must be scoped to AWS-managed connectors: {document}"
    );
    for withheld in [
        "iam:PassRole",
        "lambda:CreateMicrovmShellAuthToken",
        "microvm-image:${AWS::StackName}-*",
    ] {
        assert!(
            !document.contains(withheld),
            "{withheld} must stay out of the remote grant: {document}"
        );
    }
}

/// A sandbox that routes egress through a connector is not remotely reachable: the remote grant
/// passes only AWS's own connectors, never one the customer declared. Preflight refuses such a
/// stack; the emitter agrees, so a stack that reaches here another way installs no usable grant.
#[test]
fn aws_remote_sandbox_with_restricted_egress_carries_no_grant() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("byo-sandbox-deny".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add_with_remote_access(
            sandbox_fixture(SandboxEgress::Deny),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    // Not `render_built_ins_template`: a bindings-only stack skips the standard conditions, which
    // the created network's own resources reference, so cfn-lint refuses the template this shape
    // produces. That is the pre-existing gap the refusal above closes, not the subject here.
    let template = try_render_built_ins(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "remote sandbox deny",
    )
    .expect("the template renders");

    let logical_ids = template.resources.keys().collect::<Vec<_>>();
    assert!(
        template.resources.contains_key("Agents")
            && template.resources.contains_key("AgentsEgressConnector"),
        "the sandbox and its deny connector must still render: {logical_ids:?}"
    );
    assert!(
        template.resources.contains_key("AccessRole"),
        "the Remote Bindings identity must still render: {logical_ids:?}"
    );
    assert!(
        !template.resources.contains_key("AgentsRemoteExecutePolicy"),
        "an egress-restricted sandbox must carry no remote execute grant: {logical_ids:?}"
    );
    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    assert!(
        !rendered.contains("lambda:CreateMicrovmAuthToken"),
        "no policy in the template may mint session credentials for this sandbox: {rendered}"
    );
}

/// The management identity must be able to report on a remotely-bound sandbox without gaining any
/// reach into its sessions — the caller drives those.
#[test]
fn aws_remote_sandbox_management_role_heartbeats_without_reaching_a_session() {
    // The grants this test is about, out of what the preflight mutation derives for this stack:
    // heartbeat so the identity can report on the sandbox, management because a frozen sandbox is
    // setup-owned.
    let stack = Stack::new("byo-sandbox".to_string())
        .management(alien_core::permissions::ManagementPermissions::extend(
            alien_core::PermissionProfile::new()
                .global(["sandbox/heartbeat", "sandbox/management"]),
        ))
        .add_with_remote_access(
            sandbox_fixture(SandboxEgress::Allow),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "remote sandbox management",
    );

    let management = template
        .resources
        .iter()
        .filter(|(name, resource)| {
            name.starts_with("ManagementRole") && resource.resource_type.contains("Policy")
        })
        .map(|(_, resource)| serde_json::to_string(&resource.properties).expect("serializes"))
        .collect::<String>();

    assert!(
        management.contains("lambda:GetMicrovmImage"),
        "the management identity must be able to read the sandbox it reports on: {management}"
    );
    // `PassNetworkConnector` is withheld because `sandbox/management` as a whole reaches a
    // session, not because the action itself does — split that statement out and this stops
    // holding while still passing.
    for reaches_a_session in [
        "lambda:RunMicrovm",
        "lambda:SuspendMicrovm",
        "lambda:ResumeMicrovm",
        "lambda:TerminateMicrovm",
        "lambda:CreateMicrovmAuthToken",
        "lambda:PassNetworkConnector",
    ] {
        assert!(
            !management.contains(reaches_a_session),
            "{reaches_a_session} reaches a session and belongs to the remote caller alone: {yaml}"
        );
    }
}

/// Other sets grant role writes on `role/<prefix>-*`. The guard refuses them on every role carrying
/// setup's sandbox tags, and both roles this template creates for a deny sandbox carry them —
/// the egress operator role under a name CloudFormation generates, which no name match could hit.
#[test]
fn the_management_role_may_not_rewrite_a_sandboxs_setup_roles() {
    let (mut stack, settings) = sandbox_stack("acme-guarded", SandboxEgress::Deny);
    stack.permissions.management = alien_core::permissions::ManagementPermissions::extend(
        alien_core::PermissionProfile::new().global([
            "sandbox/management",
            "artifact-registry/management",
            alien_permissions::SANDBOX_SETUP_ROLES_GUARD,
        ]),
    );
    stack.resources.insert(
        "management".to_string(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: vec![],
            remote_access: false,
            enabled_when: None,
        },
    );
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox setup roles guard",
    );

    let statements: Vec<Value> = template
        .resources
        .iter()
        .filter(|(name, resource)| {
            name.starts_with("ManagementRole") && resource.resource_type.contains("Policy")
        })
        .flat_map(|(_, resource)| {
            let properties = serde_json::to_value(&resource.properties).expect("serializes");
            properties["PolicyDocument"]["Statement"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let denies: Vec<&Value> = statements
        .iter()
        .filter(|statement| statement["Effect"] == "Deny")
        .collect();
    assert_eq!(denies.len(), 1, "{statements:#?}");
    assert_eq!(denies[0]["Resource"], serde_json::json!(["*"]));
    assert_eq!(
        denies[0]["Condition"],
        serde_json::json!({
            "StringEquals": {
                "aws:ResourceTag/managed-by": "setup",
                "aws:ResourceTag/resource-type": "sandbox"
            }
        })
    );
    assert!(
        !denies[0].to_string().contains("iam:PassRole"),
        "the build role must stay passable"
    );

    let sandbox_roles: Vec<(&String, Value)> = template
        .resources
        .iter()
        .filter(|(name, resource)| {
            name.starts_with("Agents") && resource.resource_type == "AWS::IAM::Role"
        })
        .map(|(name, resource)| {
            (
                name,
                serde_json::to_value(&resource.properties).expect("serializes"),
            )
        })
        .collect();
    assert_eq!(
        sandbox_roles.len(),
        2,
        "the build and egress operator roles"
    );
    for (name, properties) in sandbox_roles {
        let tags = properties["Tags"].as_array().expect("tags");
        for (key, value) in [("managed-by", "setup"), ("resource-type", "sandbox")] {
            assert!(
                tags.contains(&serde_json::json!({ "Key": key, "Value": value })),
                "{name} must carry {key}={value} for the guard to reach it: {tags:?}"
            );
        }
    }
}

/// Storage is a remote-binding type too, so the same prefix match stripped `storage/heartbeat`
/// from every bring-your-own-bucket deployment's management identity.
#[test]
fn aws_remote_storage_management_role_keeps_its_heartbeat() {
    let stack = Stack::new("byo-bucket".to_string())
        .management(alien_core::permissions::ManagementPermissions::extend(
            alien_core::PermissionProfile::new().global(["storage/heartbeat"]),
        ))
        .add_with_remote_access(
            alien_core::Storage::new("customer-data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "remote storage management",
    );

    let management = template
        .resources
        .iter()
        .filter(|(name, resource)| {
            name.starts_with("ManagementRole") && resource.resource_type.contains("Policy")
        })
        .map(|(_, resource)| serde_json::to_string(&resource.properties).expect("serializes"))
        .collect::<String>();

    assert!(
        management.contains("s3:"),
        "the management identity must keep its storage heartbeat grant: {yaml}"
    );
    assert!(
        !management.contains("s3:GetObject"),
        "heartbeat must not reach object contents: {management}"
    );
}

/// A Live sandbox is reachable remotely too: setup renders its build role either way
/// (`SetupEmission::Always`), so the grant the preflight publishes has somewhere to attach.
#[test]
fn aws_remote_sandbox_grants_a_live_sandbox_the_same_execute_set() {
    let stack = Stack::new("byo-sandbox".to_string())
        .add_with_remote_access(
            sandbox_fixture_with(SandboxEgress::Allow, LIVE_BUNDLE),
            ResourceLifecycle::Live,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "live remote sandbox",
    );
    // The snapshot is what holds the set closed. A `contains` assertion is monotone, so it
    // passes just as happily on a widened action list or a wildcarded image ARN.
    insta::assert_snapshot!("remote_sandbox_grant_live_aws", yaml);

    let policy = template
        .resources
        .get("AgentsRemoteExecutePolicy")
        .expect("a Live remote sandbox must still receive the remote grant");
    assert_eq!(policy.resource_type, "AWS::IAM::Policy");
    let roles =
        serde_json::to_string(policy.properties.get("Roles").expect("Roles")).expect("serializes");
    assert!(
        roles.contains("AccessRole"),
        "the grant belongs to the shared Remote Bindings identity: {roles}"
    );
    assert!(
        template.resources.contains_key("AgentsBuildRole"),
        "setup renders the build role for a Live sandbox too, which is what the grant attaches beside"
    );

    let document = serde_json::to_string(
        policy
            .properties
            .get("PolicyDocument")
            .expect("PolicyDocument"),
    )
    .expect("serializes");
    for action in [
        "lambda:RunMicrovm",
        "lambda:TerminateMicrovm",
        "lambda:SuspendMicrovm",
        "lambda:ResumeMicrovm",
        "lambda:GetMicrovm",
        "lambda:CreateMicrovmAuthToken",
        "lambda:PassNetworkConnector",
    ] {
        assert!(document.contains(action), "{action} is missing: {document}");
    }
    assert!(
        document.contains("microvm-image:${AWS::StackName}-agents"),
        "the grant must name this sandbox's own image: {document}"
    );
}

/// Values no emitter could get right by hardcoding a default.
const PARITY_PARTITION: &str = "aws-us-gov";
const PARITY_ACCOUNT: &str = "987654321098";
const PARITY_REGION: &str = "us-gov-east-1";

/// Every combination the build role's grant branches on: lifecycle, whether the bundle URI
/// carries the region token, and whether a private base image is declared and where its region
/// comes from.
const PARITY_CASES: [(ResourceLifecycle, &str, Option<&str>); 6] = [
    (
        ResourceLifecycle::Frozen,
        "s3://acme-artifacts/agents/bundle.zip",
        None,
    ),
    (
        ResourceLifecycle::Frozen,
        "s3://acme-artifacts-{region}/agents/bundle.zip",
        None,
    ),
    (ResourceLifecycle::Live, LIVE_BUNDLE, None),
    (
        ResourceLifecycle::Live,
        "s3://acme-artifacts-{region}/sandbox-bundle/f00dcafe/bundle.zip",
        None,
    ),
    (
        ResourceLifecycle::Live,
        LIVE_BUNDLE,
        Some("123456789012.dkr.ecr.eu-west-1.amazonaws.com/acme/agents-base:1.4"),
    ),
    (
        ResourceLifecycle::Live,
        LIVE_BUNDLE,
        Some("123456789012.dkr.ecr.{region}.amazonaws.com/acme/agents-base@sha256:f00d"),
    ),
];

/// Resolves the intrinsics the build role uses to the fixed parity values, and panics on any
/// other: a placeholder would let both sides compare equal without either being checked.
fn resolve_intrinsics(value: &serde_json::Value) -> serde_json::Value {
    let pseudo = |name: &str| match name {
        "AWS::Partition" => PARITY_PARTITION,
        "AWS::AccountId" => PARITY_ACCOUNT,
        "AWS::Region" => PARITY_REGION,
        other => panic!("the build role references {other}, which the parity test cannot resolve"),
    };
    match value {
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("Fn::Sub") => {
            let text = map["Fn::Sub"].as_str().unwrap_or_else(|| {
                panic!("only the string form of Fn::Sub is resolvable: {value}")
            });
            let resolved = ["AWS::Partition", "AWS::AccountId", "AWS::Region"]
                .into_iter()
                .fold(text.to_string(), |acc, name| {
                    acc.replace(&format!("${{{name}}}"), pseudo(name))
                });
            assert!(
                !resolved.contains("${"),
                "unresolved substitution left in {resolved}"
            );
            serde_json::Value::String(resolved)
        }
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("Ref") => {
            let name = map["Ref"].as_str().expect("Ref names a string");
            serde_json::Value::String(pseudo(name).to_string())
        }
        serde_json::Value::Object(map) => {
            if let Some(key) = map
                .keys()
                .find(|key| key.starts_with("Fn::") || *key == "Ref")
            {
                panic!("the parity test cannot resolve {key}: {value}");
            }
            serde_json::Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), resolve_intrinsics(v)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(resolve_intrinsics).collect())
        }
        other => other.clone(),
    }
}

/// A direct deploy creates the build role through the IAM API from `SandboxBuildRole`, so a
/// grant changed in the emitter alone would give the two install paths different roles.
#[test]
fn the_emitted_build_role_matches_the_shared_policy_builder() {
    for (lifecycle, bundle_uri, private_base_image) in PARITY_CASES {
        let stack = Stack::new("acme-sandbox-parity".to_string())
            .add(
                Sandbox {
                    private_base_image: private_base_image.map(str::to_string),
                    ..sandbox_fixture_with(SandboxEgress::Allow, bundle_uri)
                },
                lifecycle,
            )
            .build();
        let case =
            format!("{lifecycle:?} sandbox built from {bundle_uri} on base {private_base_image:?}");
        let (template, _yaml) = render_built_ins_template(
            &stack,
            StackSettings::default(),
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            &case,
        );
        let role = serde_json::to_value(
            template
                .resources
                .get("AgentsBuildRole")
                .expect("the build role must render"),
        )
        .expect("serializes");
        let properties = &role["Properties"];
        let policies = properties["Policies"]
            .as_array()
            .unwrap_or_else(|| panic!("{case}: Policies must be a list: {role:#}"));

        let expected = SandboxBuildRole::builder()
            .sandbox_id("agents")
            .partition(PARITY_PARTITION)
            .account_id(PARITY_ACCOUNT)
            .region(PARITY_REGION)
            .bundle_uri(bundle_uri)
            .runtime_built(lifecycle == ResourceLifecycle::Live)
            .maybe_private_base_image(private_base_image)
            .build();

        assert_eq!(policies.len(), 1, "{case}: one inline policy: {role:#}");
        assert_eq!(
            policies[0]["PolicyName"],
            serde_json::json!(SANDBOX_BUILD_POLICY_NAME),
            "{case}"
        );
        assert_eq!(
            resolve_intrinsics(&policies[0]["PolicyDocument"]),
            serde_json::to_value(expected.policy().expect("the builder accepts the fixture"))
                .expect("serializes"),
            "{case}: permission policy"
        );
        assert_eq!(
            resolve_intrinsics(&properties["AssumeRolePolicyDocument"]),
            serde_json::to_value(expected.trust_policy()).expect("serializes"),
            "{case}: trust policy"
        );
    }
}

const PARITY_PREFIX: &str = "acme-parity";
const PARITY_OPERATOR_ARN: &str = "arn:aws-us-gov:iam::987654321098:role/acme-parity-agents-egress";
const PARITY_SECURITY_GROUP: &str = "sg-0parity";

/// The `PrivateSubnetIds` parameter as an installer fills it in.
fn existing_subnets() -> Vec<String> {
    vec![
        "subnet-existing-a".to_string(),
        "subnet-existing-b".to_string(),
    ]
}

/// A network mode `egress: deny` accepts, the value of the template's `NetworkModeCreate`
/// condition, and the private subnets its connector must name once both are resolved.
struct EgressParityCase {
    network: NetworkSettings,
    network_mode_create: bool,
    subnets: Vec<String>,
}

fn egress_parity_cases() -> [EgressParityCase; 3] {
    let create = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    [
        EgressParityCase {
            network: create.clone(),
            network_mode_create: true,
            subnets: vec![
                "subnet-created-1".to_string(),
                "subnet-created-2".to_string(),
            ],
        },
        EgressParityCase {
            network: create,
            network_mode_create: false,
            subnets: existing_subnets(),
        },
        EgressParityCase {
            network: NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-0parity".to_string(),
                public_subnet_ids: vec!["subnet-public-a".to_string()],
                private_subnet_ids: existing_subnets(),
                security_group_ids: vec!["sg-0network".to_string()],
            },
            network_mode_create: false,
            subnets: existing_subnets(),
        },
    ]
}

fn deny_template(network: &NetworkSettings) -> alien_cloudformation::CfTemplate {
    let settings = StackSettings {
        network: Some(network.clone()),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-sandbox-egress-parity".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            sandbox_fixture_with(SandboxEgress::Deny, LIVE_BUNDLE),
            ResourceLifecycle::Live,
        )
        .build();
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        &format!("deny sandbox on {network:?}"),
    );
    template
}

fn emitted_properties(
    template: &alien_cloudformation::CfTemplate,
    logical_id: &str,
) -> serde_json::Value {
    serde_json::to_value(
        template
            .resources
            .get(logical_id)
            .unwrap_or_else(|| panic!("{logical_id} must render")),
    )
    .expect("serializes")["Properties"]
        .clone()
}

/// Resolves every intrinsic the connector uses to the value it takes in a deployed stack. An
/// intrinsic outside these tables panics, so a new reference cannot compare equal unchecked.
fn resolve_connector(
    value: &serde_json::Value,
    network_mode_create: bool,
) -> Option<serde_json::Value> {
    let reference = |name: &str| -> Option<Value> {
        match name {
            "AWS::NoValue" => None,
            "AWS::StackName" => Some(Value::from(PARITY_PREFIX)),
            "DefaultNetworkPrivateSubnet1" => Some(Value::from("subnet-created-1")),
            "DefaultNetworkPrivateSubnet2" => Some(Value::from("subnet-created-2")),
            "PrivateSubnetIds" => Some(serde_json::json!(existing_subnets())),
            other => panic!("the parity test cannot resolve Ref {other}"),
        }
    };
    let condition = |name: &str| match name {
        "NetworkModeCreate" => network_mode_create,
        "NetworkCreateUseAz2" => true,
        "NetworkCreateUseAz3" => false,
        other => panic!("the parity test cannot evaluate condition {other}"),
    };
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("Ref") => {
            reference(map["Ref"].as_str().expect("Ref names a string"))
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::GetAtt") => {
            let target = map["Fn::GetAtt"]
                .as_array()
                .expect("GetAtt takes a pair")
                .iter()
                .map(|part| part.as_str().expect("GetAtt parts are strings"))
                .collect::<Vec<_>>();
            Some(Value::from(match target.as_slice() {
                ["AgentsEgressOperatorRole", "Arn"] => PARITY_OPERATOR_ARN,
                ["AgentsEgressSecurityGroup", "GroupId"] => PARITY_SECURITY_GROUP,
                other => panic!("the parity test cannot resolve GetAtt {other:?}"),
            }))
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::If") => {
            let branches = map["Fn::If"].as_array().expect("If takes three items");
            let name = branches[0].as_str().expect("If names a condition");
            resolve_connector(
                &branches[if condition(name) { 1 } else { 2 }],
                network_mode_create,
            )
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::Sub") => {
            let text = map["Fn::Sub"].as_str().expect("the string form of Sub");
            let resolved = text.replace("${AWS::StackName}", PARITY_PREFIX);
            assert!(!resolved.contains("${"), "unresolved Sub in {text}");
            Some(Value::from(resolved))
        }
        Value::Object(map) => {
            if let Some(key) = map.keys().find(|key| key.starts_with("Fn::")) {
                panic!("the parity test cannot resolve {key}: {value}");
            }
            Some(Value::Object(
                map.iter()
                    .filter_map(|(k, v)| {
                        resolve_connector(v, network_mode_create).map(|v| (k.clone(), v))
                    })
                    .collect(),
            ))
        }
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .filter_map(|item| resolve_connector(item, network_mode_create))
                .flat_map(|item| match item {
                    // A list parameter referenced inside a list stands for its members.
                    Value::Array(members) => members,
                    one => vec![one],
                })
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// Tags carry `insertionOrder: false` in the connector's schema, so their order is not state.
fn tags_sorted(mut properties: serde_json::Value) -> serde_json::Value {
    if let Some(tags) = properties["Tags"].as_array_mut() {
        tags.sort_by(|a, b| a["Key"].as_str().cmp(&b["Key"].as_str()));
    }
    properties
}

/// A direct deploy creates the operator role through the IAM API from the shared builder, so a
/// grant changed in the emitter alone would give the two install paths different roles.
#[test]
fn the_emitted_operator_role_matches_the_shared_builder() {
    for EgressParityCase { network, .. } in egress_parity_cases() {
        let template = deny_template(&network);
        let properties = emitted_properties(&template, "AgentsEgressOperatorRole");
        let policies = properties["Policies"]
            .as_array()
            .unwrap_or_else(|| panic!("Policies must be a list: {properties:#}"));

        assert_eq!(policies.len(), 1, "one inline policy: {properties:#}");
        assert_eq!(
            policies[0]["PolicyName"],
            serde_json::json!(SANDBOX_EGRESS_POLICY_NAME)
        );
        assert_eq!(
            resolve_intrinsics(&policies[0]["PolicyDocument"]),
            sandbox_egress_operator_policy(PARITY_PARTITION, PARITY_ACCOUNT, PARITY_REGION),
            "permission policy"
        );
        assert_eq!(
            resolve_intrinsics(&properties["AssumeRolePolicyDocument"]),
            sandbox_egress_operator_trust_policy(),
            "trust policy"
        );
    }
}

/// A direct deploy sends this builder's output to Cloud Control as the connector's desired state;
/// it must be the resource CloudFormation creates for the same sandbox.
#[test]
fn the_emitted_connector_matches_the_direct_desired_state() {
    for EgressParityCase {
        network,
        network_mode_create,
        subnets,
    } in egress_parity_cases()
    {
        let case = format!("connector on {network:?} with NetworkModeCreate={network_mode_create}");
        let template = deny_template(&network);
        let emitted = resolve_connector(
            &emitted_properties(&template, "AgentsEgressConnector"),
            network_mode_create,
        )
        .expect("the connector's properties resolve to a value");

        let direct = SandboxEgressConnector::builder()
            .resource_prefix(PARITY_PREFIX)
            .sandbox_id("agents")
            .operator_role_arn(PARITY_OPERATOR_ARN)
            .private_subnet_ids(&subnets)
            .security_group_id(PARITY_SECURITY_GROUP)
            .build()
            .desired_state();

        assert_eq!(tags_sorted(emitted), tags_sorted(direct), "{case}");
    }
}

/// The group is what enforces `egress: deny`, and a direct deploy creates it with exactly one
/// all-protocol rule to [`LOOPBACK_ONLY_CIDR`] and no ingress. The name is not compared: the
/// direct path finds its group again by `sandbox_egress_name`, CloudFormation by logical id, and
/// adding `GroupName` here would replace the group under every installed stack.
#[test]
fn the_emitted_deny_group_matches_the_direct_rule_set() {
    for EgressParityCase { network, .. } in egress_parity_cases() {
        let case = format!("deny group on {network:?}");
        let template = deny_template(&network);
        let properties = emitted_properties(&template, "AgentsEgressSecurityGroup");

        assert_eq!(properties.get("GroupName"), None, "{case}");
        assert_eq!(
            properties["GroupDescription"],
            serde_json::json!("Sandbox agents session egress"),
            "{case}"
        );
        assert_eq!(
            properties["SecurityGroupEgress"],
            serde_json::json!([{
                "IpProtocol": "-1",
                "CidrIp": LOOPBACK_ONLY_CIDR,
                "Description": "Sandbox sessions reach nothing outbound"
            }]),
            "{case}: one rule, all protocols, to the destination that reaches nothing"
        );
        assert_eq!(
            properties.get("SecurityGroupIngress"),
            None,
            "{case}: {properties:#}"
        );

        // A standalone rule resource widens the group as surely as an inline one.
        for (logical_id, resource) in &template.resources {
            if resource
                .resource_type
                .starts_with("AWS::EC2::SecurityGroup")
                && logical_id != "AgentsEgressSecurityGroup"
            {
                let rendered = serde_json::to_string(resource).expect("serializes");
                assert!(
                    !rendered.contains("AgentsEgressSecurityGroup"),
                    "{case}: {logical_id} adds a rule to the deny group: {rendered}"
                );
            }
        }
    }
}

const PARITY_CONNECTOR_ARN: &str =
    "arn:aws-us-gov:lambda:us-gov-east-1:987654321098:network-connector:nc-0parity";

/// Resolves the registration's intrinsics to a deployed stack's values. The build role resolves
/// from its own emitted `RoleName`, so a renamed role cannot compare equal to the direct side's
/// derivation by both sides reading one placeholder.
fn resolve_registration(
    template: &alien_cloudformation::CfTemplate,
    value: &serde_json::Value,
) -> serde_json::Value {
    let sub = |text: &str| {
        let resolved = text
            .replace("${AWS::StackName}", PARITY_PREFIX)
            .replace("${AWS::Partition}", PARITY_PARTITION)
            .replace("${AWS::AccountId}", PARITY_ACCOUNT)
            .replace("${AWS::Region}", PARITY_REGION);
        assert!(!resolved.contains("${"), "unresolved Sub in {text}");
        resolved
    };
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::Sub") => {
            Value::from(sub(map["Fn::Sub"]
                .as_str()
                .expect("the string form of Sub")))
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::GetAtt") => {
            let target: Vec<&str> = map["Fn::GetAtt"]
                .as_array()
                .expect("GetAtt takes a pair")
                .iter()
                .map(|part| part.as_str().expect("GetAtt parts are strings"))
                .collect();
            let resource = serde_json::to_value(
                template
                    .resources
                    .get(target[0])
                    .unwrap_or_else(|| panic!("GetAtt names {} which must render", target[0])),
            )
            .expect("serializes");
            match target.as_slice() {
                [_, "Arn"] if resource["Type"] == "AWS::IAM::Role" => {
                    assert_eq!(
                        resource["Properties"].get("Path"),
                        None,
                        "the pass grant is scoped to the root path"
                    );
                    let role_name = resource["Properties"]["RoleName"]["Fn::Sub"]
                        .as_str()
                        .expect("the build role is named through Sub");
                    Value::from(format!(
                        "arn:{PARITY_PARTITION}:iam::{PARITY_ACCOUNT}:role/{}",
                        sub(role_name)
                    ))
                }
                [_, "Arn"] if resource["Type"] == "AWS::Lambda::NetworkConnector" => {
                    Value::from(PARITY_CONNECTOR_ARN)
                }
                other => panic!("the parity test cannot resolve GetAtt {other:?}"),
            }
        }
        Value::Object(map) => {
            if let Some(key) = map
                .keys()
                .find(|key| key.starts_with("Fn::") || *key == "Ref")
            {
                panic!("the parity test cannot resolve {key}: {value}");
            }
            Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), resolve_registration(template, v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| resolve_registration(template, item))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A direct deploy registers a runtime-built sandbox from `AwsSandboxImportData::runtime_built`
/// instead of this template, so a field changed in the emitter alone would hand the controller a
/// different build role, bundle, egress, or preview set depending on how it was installed.
#[test]
fn the_emitted_registration_matches_the_direct_seed() {
    let network = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    for (egress, bundle_uri) in [
        (
            SandboxEgress::Allow,
            "s3://acme-artifacts-{region}/sandbox-bundle/f00dcafe/bundle.zip",
        ),
        (SandboxEgress::Deny, LIVE_BUNDLE),
        (
            SandboxEgress::Deny,
            "s3://acme-artifacts-{region}/sandbox-bundle/f00dcafe/bundle.zip",
        ),
    ] {
        let sandbox = Sandbox {
            preview_ports: vec![8080, 3000],
            ..sandbox_fixture_with(egress.clone(), bundle_uri)
        };
        let stack = Stack::new("acme-sandbox-registration-parity".to_string())
            .add(
                Network::new("default-network".to_string())
                    .settings(network.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(sandbox.clone(), ResourceLifecycle::Live)
            .build();
        let case = format!("{egress:?} sandbox built from {bundle_uri}");
        let (template, _yaml) = render_built_ins_template(
            &stack,
            StackSettings {
                network: Some(network.clone()),
                ..StackSettings::default()
            },
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            &case,
        );

        let emitted: AwsSandboxImportData = serde_json::from_value(resolve_registration(
            &template,
            &emitted_import_data(&template, "agents"),
        ))
        .unwrap_or_else(|error| panic!("{case}: the importer must accept it: {error}"));
        let direct = AwsSandboxImportData::runtime_built(
            &sandbox,
            alien_core::sandbox_build_role::sandbox_build_role_arn(
                PARITY_PARTITION,
                PARITY_ACCOUNT,
                PARITY_PREFIX,
                "agents",
            ),
            PARITY_REGION,
            Some(PARITY_CONNECTOR_ARN),
        )
        .unwrap_or_else(|error| panic!("{case}: the direct seed resolves: {error}"));

        assert_eq!(emitted, direct, "{case}");
    }
}
