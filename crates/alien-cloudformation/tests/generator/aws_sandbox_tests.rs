//! AWS Sandbox — what `egress: deny` has to build for the template to mean it.

use super::helpers::{
    custom_resource_registration, render_built_ins_template, try_render_built_ins,
};
use alien_cloudformation::CloudFormationTarget;
use alien_core::{
    import::data::AwsSandboxImportData, Network, NetworkSettings, RemoteBindings,
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, Stack,
    StackSettings, Worker, WorkerCode,
};

fn sandbox_fixture(egress: SandboxEgress) -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
        })
        .egress(egress)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
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
        .add(sandbox_fixture(egress), lifecycle)
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

    fn find(value: &serde_json::Value, resource_id: &str) -> Option<serde_json::Value> {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("id").and_then(serde_json::Value::as_str) == Some(resource_id) {
                    if let Some(import_data) = map.get("importData") {
                        return Some(resolve(import_data));
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
    // carry a GetAtt against the image resource this template no longer creates.
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

    // The runtime build's base image comes from a private registry, and the identity itself
    // needs all three actions — a repository policy on the registry side is not enough.
    let statements = build_role_statements(&template);
    let ecr = statements
        .iter()
        .find(|statement| grants_ecr(statement) && statement["Effect"] == "Allow")
        .unwrap_or_else(|| panic!("a Live build role must authenticate to ECR: {statements:#?}"));
    assert_eq!(
        ecr["Sid"], "PullSandboxBaseImage",
        "the statement a security reviewer reads must say what it is for"
    );
    assert_eq!(
        ecr["Action"],
        serde_json::json!([
            "ecr:GetAuthorizationToken",
            "ecr:BatchGetImage",
            "ecr:GetDownloadUrlForLayer"
        ]),
        "exactly the token call and the two pull actions, nothing wider"
    );
    assert_eq!(
        ecr["Resource"],
        serde_json::json!("*"),
        "GetAuthorizationToken is only accepted against `*`"
    );

    // Same-account pulls are authorized by identity policy alone, so without this Deny the
    // Allow above makes a customer-authored Dockerfile a reader of every private repository
    // in the customer's own account.
    let deny = statements
        .iter()
        .find(|statement| statement["Effect"] == "Deny")
        .unwrap_or_else(|| {
            panic!("same-account pulls must be denied on a Live build role: {statements:#?}")
        });
    assert_eq!(deny["Sid"], "DenySameAccountImagePull");
    assert_eq!(
        deny["Action"],
        serde_json::json!(["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"]),
        "the deny covers exactly the two pull actions — never the token call, which the \
         cross-account login needs"
    );
    assert_eq!(
        deny["Resource"],
        serde_json::json!({
            "Fn::Sub": "arn:${AWS::Partition}:ecr:*:${AWS::AccountId}:repository/*"
        }),
        "the deny must name this account's repositories through pseudo parameters, not literals"
    );
}

/// The Frozen path is the one every installed stack is on, and it does not move.
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
}

/// Open + Live is the leanest emitted combination — no image (built at runtime) and no egress
/// apparatus (an open session starts without a connector) — and no other test renders it.
#[test]
fn an_open_live_sandbox_ships_only_the_build_role() {
    let stack = Stack::new("acme-sandbox-open-live".to_string())
        .add(
            sandbox_fixture(SandboxEgress::Allow),
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

    let security_group = template
        .resources
        .get("AgentsEgressSecurityGroup")
        .expect("the sandbox egress security group must render");
    let egress = serde_json::to_string(
        security_group
            .properties
            .get("SecurityGroupEgress")
            .expect("an egress rule, or EC2's allow-all default survives"),
    )
    .expect("serializes");
    assert!(
        egress.contains("127.0.0.1/32"),
        "the only permitted destination must be the one that reaches nothing: {egress}"
    );
    assert!(
        !egress.contains("0.0.0.0/0"),
        "a wide egress rule turns deny back into outbound access: {egress}"
    );

    let connector = template
        .resources
        .get("AgentsEgressConnector")
        .expect("the egress connector must render");
    assert_eq!(connector.resource_type, "AWS::Lambda::NetworkConnector");
    let configuration = serde_json::to_string(
        connector
            .properties
            .get("Configuration")
            .expect("connector configuration"),
    )
    .expect("serializes");
    assert!(
        configuration.contains("AgentsEgressSecurityGroup"),
        "the connector must carry the group that denies: {configuration}"
    );
    assert!(
        configuration.contains("DefaultNetworkPrivateSubnet1"),
        "the connector must place its interfaces in the network's private subnets: \
         {configuration}"
    );
    assert!(
        configuration.contains("MicroVm"),
        "the connector must be usable by MicroVMs: {configuration}"
    );

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
/// manager mints a session against a role that carries none.
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
    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "remote sandbox",
    );

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
    // The profile the preflight mutation derives for this stack: heartbeat so the identity can
    // report on the sandbox, management because a frozen sandbox is setup-owned.
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
