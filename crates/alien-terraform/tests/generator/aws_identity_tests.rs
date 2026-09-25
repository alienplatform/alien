//! AWS identity & network — service-account / management /
//! network (Create + ByoVpcAws + UseDefault).

use super::helpers::{
    assert_terraform_valid, assert_terraform_variable_plan_invalid_contains, gate_input, render,
    snapshot_module, try_render,
};
use alien_core::{
    sandbox_build_role::{SandboxBuildRole, SANDBOX_BUILD_POLICY_NAME},
    sandbox_egress::{
        sandbox_egress_name, sandbox_egress_operator_policy, sandbox_egress_operator_trust_policy,
        SandboxEgressConnector, LOOPBACK_ONLY_CIDR, SANDBOX_EGRESS_POLICY_NAME,
    },
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, ServiceAccount,
    Stack, StackSettings, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;
use hcl::expr::{BinaryOperator, Operation};
use serde_json::Value;

#[test]
fn aws_service_account_with_permission_set() {
    let sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-read")
                .expect("storage/data-read permission set")
                .clone(),
        )
        .build();
    let stack = Stack::new("acme-iam".to_string())
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_service_account", &module);
    assert_terraform_valid(&module, "aws_service_account");
}

#[test]
fn aws_remote_stack_management_role() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["worker/management", "storage/heartbeat"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_remote_stack_management", &module);
    assert_terraform_valid(&module, "aws_remote_stack_management");
}

#[test]
fn aws_remote_stack_management_skips_live_provision_sets() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("job", ["worker/provision", "worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("job".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/job:1.2.3".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut rendered = String::new();
    for (_, contents) in module.iter() {
        rendered.push_str(contents);
        rendered.push('\n');
    }

    assert!(rendered.contains("lambda:InvokeFunction"));
    assert!(!rendered.contains("lambda:CreateFunction"));
    assert_terraform_valid(
        &module,
        "aws_remote_stack_management_skips_live_provision_sets",
    );
}

/// A management grant naming a gated resource must follow that resource's
/// gate. The shared managed policy is unconditional, so a grant left there
/// would outlive the very resource it names — the management role would keep
/// invoke rights on a function the deployer declined.
#[test]
fn aws_management_grants_for_a_gated_resource_carry_its_gate() {
    let stack = Stack::new("acme-mgmt".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable the job worker",
            "Whether to run the job worker.",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("jobs", ["worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Worker::new("jobs".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/jobs:1.2.3"
                        .to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
            "jobsEnabled",
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut main = String::new();
    for (_, contents) in module.iter() {
        main.push_str(contents);
        main.push('\n');
    }

    // The grant exists, and it is behind the worker's gate.
    let gated_policy = main
        .split("resource \"")
        .find(|block| block.starts_with("aws_iam_role_policy\" \"management_input_jobs_enabled\""))
        .unwrap_or_else(|| panic!("the gated management policy should render:\n{main}"));
    // HCL re-pads `=` to align with sibling attributes, so match the value.
    assert!(
        gated_policy.contains("var.input_jobs_enabled ? 1 : 0"),
        "the gated management policy must carry the worker's gate:\n{gated_policy}"
    );
    assert!(
        gated_policy.contains("lambda:InvokeFunction"),
        "the dispatch grant belongs in the gated policy:\n{gated_policy}"
    );

    // And it is not also sitting in the unconditional managed policy.
    for block in main.split("resource \"") {
        if block.starts_with("aws_iam_policy\" \"management_managed_policy") {
            assert!(
                !block.contains("lambda:InvokeFunction"),
                "a declinable resource's grant must not ride the unconditional \
                 managed policy:\n{block}"
            );
        }
    }

    assert_terraform_valid(&module, "aws_management_grants_for_a_gated_resource");
}

/// Two resources gated on one input land in one policy, each contributing the
/// same permission set and therefore the same statement id. IAM rejects a
/// policy document that repeats one, so the ids must be made unique — the
/// shared managed-policy path already does this, and the gated path has to
/// agree.
#[test]
fn management_grants_sharing_a_gate_get_unique_statement_ids() {
    let worker = |id: &str| {
        Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: format!("123456789012.dkr.ecr.us-east-1.amazonaws.com/app/{id}:1.2.3"),
            })
            .permissions("execution".to_string())
            .build()
    };
    let stack = Stack::new("acme-mgmt".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable the job workers",
            "Whether to run the job workers.",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("jobs-a", ["worker/dispatch-command"])
                .resource("jobs-b", ["worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(worker("jobs-a"), ResourceLifecycle::Live, "jobsEnabled")
        .add_enabled_when(worker("jobs-b"), ResourceLifecycle::Live, "jobsEnabled")
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut rendered = String::new();
    for (_, contents) in module.iter() {
        rendered.push_str(contents);
        rendered.push('\n');
    }

    let gated_policy = rendered
        .split("resource \"")
        .find(|block| block.starts_with("aws_iam_role_policy\" \"management_input_jobs_enabled\""))
        .unwrap_or_else(|| panic!("the gated management policy should render:\n{rendered}"));

    let sids: Vec<&str> = gated_policy
        .match_indices("Sid = \"")
        .map(|(i, _)| {
            let rest = &gated_policy[i + 7..];
            &rest[..rest.find('"').unwrap_or(0)]
        })
        .collect();
    assert_eq!(
        sids.len(),
        2,
        "both workers contribute a statement: {sids:?}"
    );
    assert_ne!(
        sids[0], sids[1],
        "IAM rejects a policy document with duplicate statement ids: {sids:?}"
    );

    assert_terraform_valid(&module, "management_grants_sharing_a_gate");
}

#[test]
fn aws_network_create_two_az() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-net".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, settings);
    snapshot_module("aws_network_create_two_az", &module);
    assert_terraform_valid(&module, "aws_network_create_two_az");
}

#[test]
fn aws_network_byo_vpc_emits_no_resources() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec!["subnet-private-a".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-byo".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, settings);
    snapshot_module("aws_network_byo_vpc", &module);
    assert_terraform_valid(&module, "aws_network_byo_vpc");
}

/// The sandbox build role's policy must render as an IAM document, not as strings.
///
/// `iam_role_policy_block` jsonencodes the whole document, so a statement that is itself
/// jsonencoded lands in `Statement` as a JSON string. `terraform validate` accepts that — the HCL
/// and the string are both well-formed — and IAM rejects it at apply with MalformedPolicyDocument.
/// Rendering and reading the statements back is the only place that shows up.
#[test]
fn aws_sandbox_build_policy_is_a_well_formed_scoped_document() {
    let (stack, settings) = sandbox_stack("acme-sandbox", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Aws, settings);

    let rendered: String = module.iter().map(|(_, contents)| contents).collect();
    let policy = rendered
        .lines()
        .find(|line| line.contains("Statement") && line.contains("s3:GetObject"))
        .unwrap_or_else(|| panic!("the build policy must render:\n{rendered}"));

    assert!(
        !policy.contains("Statement = [jsonencode"),
        "statements must be objects, not encoded strings: {policy}"
    );
    assert!(
        policy.contains(":s3:::acme-artifacts/agents/bundle.zip"),
        "the build role reads one object and must be scoped to it: {policy}"
    );
    // `$${` would be an escaped literal, so the partition would reach IAM as text and the grant
    // would match nothing — visible only as AccessDenied during the image build.
    assert!(
        policy.contains("arn:${data.aws_partition.current.partition}:s3:::")
            && !policy.contains("$${"),
        "the partition must interpolate rather than render as literal text: {policy}"
    );
    assert!(
        !policy.contains(r#""s3:GetObject"], "Resource" = "*""#)
            && !policy.contains(r#"s3:GetObject"] Resource = "*""#),
        "account-wide object read must not be emitted: {policy}"
    );
    // A setup-baked image pulls its public base anonymously; an ECR grant here would hand the
    // role running a customer-authored Dockerfile pull access it never needs.
    assert!(
        !rendered.contains("ecr:"),
        "a Frozen build role must carry no ECR action:\n{rendered}"
    );
}

/// A module that declares `awscc` must configure it, or the customer cannot plan.
///
/// awscc has no ambient default, so a `required_providers` entry with no `provider` block makes
/// `terraform plan` refuse the module — and `terraform validate` passes, because it does not
/// evaluate provider configuration. The region must match the `aws` provider's, or the image is
/// built somewhere the binding will not look for it.
#[test]
fn aws_sandbox_module_configures_the_awscc_provider() {
    let (stack, settings) = sandbox_stack("acme-sandbox-provider", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Aws, settings);
    let rendered: String = module.iter().map(|(_, contents)| contents).collect();

    assert!(
        rendered.contains("provider \"awscc\""),
        "awscc is declared, so it must be configured:\n{rendered}"
    );
    let block = rendered
        .split("provider \"awscc\"")
        .nth(1)
        .expect("the block just asserted");
    assert!(
        block
            .lines()
            .take(4)
            .any(|line| line.contains("region") && line.contains("var.aws_region")),
        "awscc must take the same region as the aws provider: {block}"
    );
}

/// An open sandbox builds no connector, and needs no VPC to build one in.
///
/// `allow` is a session started with no egress connector, which leaves AWS's managed internet
/// path in place. Rendering the deny apparatus anyway would demand a VPC from a stack that never
/// routes through one.
#[test]
fn aws_sandbox_allowing_egress_builds_no_connector() {
    // Without a `Network` in the stack: rendering the deny apparatus would demand one, so a
    // fixture that supplies a VPC anyway could not tell the two outcomes apart.
    let (stack, settings) =
        sandbox_stack_without_a_network("acme-sandbox-open", SandboxEgress::Allow);
    let module = render(&stack, TerraformTarget::Aws, settings);
    let rendered: String = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // Named for the sandbox rather than by type: the stack's own network renders a security
    // group of its own, and matching on the type alone would pass or fail for the wrong reason.
    for absent in ["awscc_lambda_network_connector", "agents_egress"] {
        assert!(
            !rendered.contains(absent),
            "an open sandbox must not render {absent}:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("AWS::Lambda::MicrovmImage"),
        "the image is still the sandbox's durable parent:\n{rendered}"
    );
    assert!(
        rendered.contains("allowEgress"),
        "the empty connector list must be readable as open, not stripped:\n{rendered}"
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
        let error = try_render(&stack, TerraformTarget::Aws, settings)
            .expect_err(&format!("egress {mode:?} must be refused at emit time"));
        assert!(
            error.to_string().contains("VPC egress connector"),
            "the refusal must name why: {error}"
        );
    }
}

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
/// A stack carrying only the sandbox — no `Network`, which is what "needs no VPC" means.
fn sandbox_stack_without_a_network(name: &str, egress: SandboxEgress) -> (Stack, StackSettings) {
    let stack = Stack::new(name.to_string())
        .add(sandbox_fixture(egress), ResourceLifecycle::Frozen)
        .build();
    (stack, StackSettings::default())
}

fn sandbox_stack(name: &str, egress: SandboxEgress) -> (Stack, StackSettings) {
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
        .add(sandbox_fixture(egress), ResourceLifecycle::Frozen)
        .build();
    (stack, settings)
}

/// The same stack with the sandbox declared Live, so its image is built at runtime instead of
/// `terraform apply`.
fn live_sandbox_stack(name: &str, egress: SandboxEgress) -> (Stack, StackSettings) {
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
            sandbox_fixture_with(egress, LIVE_BUNDLE),
            ResourceLifecycle::Live,
        )
        .build();
    (stack, settings)
}

/// The image must be gone — it can only be built once the customer's account is a principal
/// Alien's registry has opened to, not true during `terraform apply` — and the build role must
/// remain, since `sandbox/provision` grants `iam:PassRole` but no `iam:CreateRole`.
///
/// Validated with real `terraform validate`, not text matching: the failure this guards against
/// is a plan-time one — reading `LatestActiveImageVersion` off a resource the module no longer
/// declares.
#[test]
fn a_live_sandbox_module_keeps_the_build_role_and_drops_the_image() {
    let (mut stack, settings) = live_sandbox_stack("acme-sandbox-live", SandboxEgress::Deny);
    stack
        .resources
        .get_mut("agents")
        .and_then(|entry| entry.config.downcast_mut::<Sandbox>())
        .expect("the sandbox is in the stack")
        .private_base_image =
        Some("123456789012.dkr.ecr.{region}.amazonaws.com/acme/agents-base:1.4".to_string());
    let module = render(&stack, TerraformTarget::Aws, settings);
    assert_terraform_valid(&module, "live sandbox module");

    let rendered: String = module.iter().map(|(_, contents)| contents).collect();
    assert!(
        !rendered.contains("LatestActiveImageVersion"),
        "no attribute of an image the module does not declare may be read:\n{rendered}"
    );
    assert!(
        rendered.contains("resource \"aws_iam_role\" \"agents\""),
        "the build role the controller passes must still be installed:\n{rendered}"
    );
    assert!(
        rendered.contains("buildRoleArn") && rendered.contains("bundleUri"),
        "registration must hand the controller the role and the bundle:\n{rendered}"
    );
    assert!(
        rendered.contains("awscc_lambda_network_connector"),
        "the connector the build and the session are passed must still be installed:\n{rendered}"
    );

    let statements = build_policy_statements(&rendered);

    // The grant a runtime rebuild needs, and the assertion that keeps this module and the
    // CloudFormation one from installing different roles for one declaration: both derive the
    // prefix through `alien_core::stable_bundle_key_prefix`, and both must render it.
    let bundle = statements
        .iter()
        .find(|statement| statement["Sid"] == "ReadSandboxBundlePrefix")
        .unwrap_or_else(|| panic!("a Live build role must read its bundle: {statements:#?}"));
    assert_eq!(
        bundle["Resource"],
        "arn:${data.aws_partition.current.partition}:s3:::acme-artifacts/sandbox-bundle/*",
        "a Live role reads the prefix the moving key stays inside, not the one object"
    );

    // The declared base is the one repository the build may pull, in the deployment's region
    // because its host names `{region}`; the token call has no resource type, so it is `*`.
    let ecr: Vec<_> = statements
        .iter()
        .filter(|statement| statement["Sid"] != "ReadSandboxBundlePrefix")
        .collect();
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
                "Resource": "arn:${data.aws_partition.current.partition}:ecr:\
                             ${data.aws_region.current.region}:123456789012:repository/acme/agents-base"
            }),
            &serde_json::json!({
                "Sid": "DenySameAccountImagePull",
                "Effect": "Deny",
                "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                "Resource": "arn:${data.aws_partition.current.partition}:ecr:*:\
                             ${data.aws_caller_identity.current.account_id}:repository/*"
            }),
        ],
        "the token call on `*`, the pull on exactly the declared repository, and no pull from \
         this account"
    );
}

/// The `sandbox-image-build` policy's statements, parsed as IAM will read them.
///
/// The renderer prints a `jsonencode` argument in HCL object syntax — JSON up to the key
/// separators (quoted statement keys, bare top-level ones) — so rewriting those parses the
/// document structurally instead of pinning the formatter's exact output. Drift in the syntax
/// fails the parse loudly rather than passing on a stale literal.
fn build_policy_statements(rendered: &str) -> Vec<serde_json::Value> {
    let line = rendered
        .lines()
        .find(|line| line.contains("Statement") && line.contains("s3:GetObject"))
        .unwrap_or_else(|| panic!("the build policy must render:\n{rendered}"));
    let start = line
        .find("jsonencode(")
        .unwrap_or_else(|| panic!("the policy must be a jsonencode call: {line}"))
        + "jsonencode(".len();
    let end = line
        .rfind(')')
        .unwrap_or_else(|| panic!("the jsonencode call must close: {line}"));
    let json_text = line[start..end]
        .replace("\" = ", "\": ")
        .replace("Version = ", "\"Version\": ")
        .replace("Statement = ", "\"Statement\": ");
    let document: serde_json::Value = serde_json::from_str(&json_text)
        .unwrap_or_else(|error| panic!("the policy document must parse: {error}\n{line}"));
    document["Statement"]
        .as_array()
        .unwrap_or_else(|| panic!("the policy statements must be a list: {document:#}"))
        .clone()
}

/// Open + Live drops every consumer of the propagation barrier: no image is baked at apply, and
/// no connector exists whose operator role must propagate. Keeping the `time_sleep` would ship a
/// customer a thirty-second wait that gates nothing.
///
/// Validated with real `terraform validate` because this is the one emitted combination no other
/// test renders.
#[test]
fn an_open_live_sandbox_module_keeps_only_the_build_role_and_no_barrier() {
    let (stack, settings) = live_sandbox_stack("acme-sandbox-open-live", SandboxEgress::Allow);
    let module = render(&stack, TerraformTarget::Aws, settings);
    assert_terraform_valid(&module, "open live sandbox module");

    let rendered: String = module.iter().map(|(_, contents)| contents).collect();
    assert!(
        !rendered.contains("resource \"time_sleep\""),
        "nothing here waits on IAM propagation, so no barrier may render:\n{rendered}"
    );
    for absent in [
        "AWS::Lambda::MicrovmImage",
        "awscc_lambda_network_connector",
    ] {
        assert!(
            !rendered.contains(absent),
            "an open Live sandbox must not render {absent}:\n{rendered}"
        );
    }
    assert!(
        rendered.contains("resource \"aws_iam_role\" \"agents\""),
        "the build role the controller passes must still be installed:\n{rendered}"
    );
    assert!(
        rendered.contains("A completed apply installs the sandbox's build role but not"),
        "the README must caveat the runtime build without naming an absent connector:\n{rendered}"
    );
}

/// A Kubernetes target skips the sandbox emitter, so its README must not caveat a runtime image
/// build that never happens.
#[test]
fn a_kubernetes_target_readme_makes_no_runtime_build_promise() {
    let (stack, settings) = live_sandbox_stack("acme-sandbox-eks-live", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Eks, settings);
    let rendered: String = module.iter().map(|(_, contents)| contents).collect();
    assert!(
        !rendered.contains("built after the deployment registers"),
        "no sandbox is emitted on a Kubernetes target, so no build step may be described:\n\
         {rendered}"
    );
}

/// `egress: deny` has to be built, not assumed.
///
/// A MicroVM started with no egress connector reaches the public internet — verified against a
/// live account. The connector is what puts session traffic inside the VPC, and the security
/// group is what stops it there: EC2 attaches an allow-all egress rule to any group whose
/// template states none, so the only rule present must be the one that reaches nothing.
/// `terraform validate` cannot see any of this.
/// Emitter lookup keys off the cloud a cluster runs in, so an EKS target resolves the AWS
/// sandbox emitter. A Kubernetes sandbox is a pod the chart bounds with a NetworkPolicy, and it
/// creates no cloud resource — provisioning a MicroVM image and its connector for one would be a
/// second backend nobody uses, billed and permissioned.
#[test]
fn a_kubernetes_target_emits_no_microvm_infrastructure_for_a_sandbox() {
    let (stack, settings) = sandbox_stack("acme-sandbox-eks", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Eks, settings);
    let rendered: String = module.iter().map(|(_, contents)| contents).collect();

    for absent in [
        "aws_cloudcontrolapi_resource",
        "awscc_lambda_network_connector",
        "AWS::Lambda::MicrovmImage",
    ] {
        assert!(
            !rendered.contains(absent),
            "an EKS module must not carry the AWS sandbox backend, found {absent}:\n{rendered}"
        );
    }
}

#[test]
fn aws_sandbox_deny_builds_a_connector_that_permits_nothing_outbound() {
    let (stack, settings) = sandbox_stack("acme-sandbox-deny", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Aws, settings);
    let rendered: String = module.iter().map(|(_, contents)| contents).collect();

    // The group's rules and the connector's configuration are compared whole by the parity
    // tests below.

    // Scoped to the image block rather than the whole module: the binding also names the
    // connector, so a module-wide search passes even when the image has lost its own entry.
    let image = rendered
        .split("resource \"aws_cloudcontrolapi_resource\"")
        .nth(1)
        .expect("the image block must render");
    let image = image.split("\nresource ").next().expect("block ends");
    // The switch that keeps session output out of the control plane's reach.
    assert!(
        image.contains("\"Disabled\" = true") || image.contains("Disabled = true"),
        "content-bearing logging must be off:\n{image}"
    );

    // Cloud Control rather than awscc: the schema requires AdditionalOsCapabilities and the
    // sandbox asks for none, and awscc drops an empty list before sending it.
    assert!(
        image.contains("AWS::Lambda::MicrovmImage") && image.contains("AdditionalOsCapabilities"),
        "the image must go through Cloud Control with the required empty list intact:\n{image}"
    );
    assert!(
        image.contains("INTERNET_EGRESS"),
        "the image must build through AWS's own connector:\n{image}"
    );
    assert!(
        !image.contains("awscc_lambda_network_connector.agents.arn"),
        "the deny connector belongs to the session, not the build:\n{image}"
    );
    // The roles are referenced by ARN, which resolves before the inline policy is attached and
    // before IAM has propagated it. Without a barrier the first apply fails and a retry works.
    let barrier = rendered
        .split("resource \"time_sleep\"")
        .nth(1)
        .expect("the sandbox must emit an IAM propagation barrier");
    let barrier = barrier.split("\nresource ").next().expect("block ends");
    assert!(
        barrier.contains("aws_iam_role_policy.agents")
            && barrier.contains("aws_iam_role_policy.agents_egress"),
        "the barrier must wait for both inline policies:\n{barrier}"
    );
    assert!(
        barrier.contains("unique_id"),
        "the barrier must re-run when a role is replaced, and a templated name keeps its ARN:\n{barrier}"
    );

    let connector_block = rendered
        .split("resource \"awscc_lambda_network_connector\"")
        .nth(1)
        .expect("connector renders");
    let connector_block = connector_block.split("\nresource ").next().expect("ends");
    assert!(
        connector_block.contains("time_sleep.agents_iam_propagation"),
        "the connector must wait for the barrier:\n{connector_block}"
    );

    assert!(
        rendered.contains("awscc_lambda_network_connector.agents.arn"),
        "the binding must still carry the connector a session runs through:\n{rendered}"
    );
}

/// The connector attaches where `sandbox_egress_network` says, which is the stack's first network,
/// whichever of the two is created and whichever is brought.
#[test]
fn a_deny_sandbox_attaches_to_the_first_of_two_networks() {
    let created = || NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let brought = || NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0brought".to_string(),
        public_subnet_ids: vec!["subnet-public-a".to_string()],
        private_subnet_ids: vec!["subnet-private-a".to_string()],
        security_group_ids: vec!["sg-0network".to_string()],
    };
    for (first, second, expected) in [
        (created(), brought(), "aws_subnet.first_net_private[*].id"),
        (brought(), created(), "var.first_net_private_subnet_ids"),
    ] {
        let stack = Stack::new("acme-sandbox-two-networks".to_string())
            .add(
                Network::new("first-net".to_string())
                    .settings(first.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Network::new("second-net".to_string())
                    .settings(second)
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

        let module = render(
            &stack,
            TerraformTarget::Aws,
            StackSettings {
                network: Some(first),
                ..StackSettings::default()
            },
        );
        let sandbox_file: hcl::Body =
            hcl::parse(module.get("agents.tf").expect("agents.tf renders")).expect("parses");
        let connector = resource_blocks(&sandbox_file, "awscc_lambda_network_connector")
            .next()
            .expect("the connector renders");
        let configuration = block_attribute(connector, "configuration")
            .expr()
            .to_string();
        assert!(
            configuration.contains(expected) && !configuration.contains("second_net"),
            "expected the subnets to be {expected}: {configuration}"
        );
    }
}

/// Without a VPC there are no subnets, and a connector needs between one and sixteen.
///
/// Rendering one anyway would produce either an apply-time failure the reader cannot act on or —
/// worse — a session with no connector, which is the mode that silently reaches the internet.
#[test]
fn aws_sandbox_refuses_to_render_without_a_network_to_attach_to() {
    let stack = Stack::new("acme-sandbox-no-network".to_string())
        .add(
            sandbox_fixture(SandboxEgress::Deny),
            ResourceLifecycle::Frozen,
        )
        .build();
    let error = try_render(&stack, TerraformTarget::Aws, StackSettings::default())
        .expect_err("a sandbox with no network must be refused at emit time");
    assert!(
        error.to_string().contains("declares no network"),
        "the refusal must name why: {error}"
    );
}

/// The withheld network mode has to be refused where the installer can still act on it — see
/// `network_mode_variable_block` for why. Asserted against real `terraform plan` diagnostics
/// rather than by matching rendered text, because what matters is that Terraform itself rejects it.
#[test]
fn a_restricted_sandbox_makes_terraform_refuse_the_default_network_at_plan_time() {
    let (stack, settings) = sandbox_stack("acme-sandbox-denied", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Aws, settings);

    assert_terraform_valid(&module, "restricted sandbox module");
    snapshot_module("aws_sandbox_restricted", &module);
    assert_terraform_variable_plan_invalid_contains(
        &module,
        "restricted sandbox with network_mode=use-default",
        // `name` and `token` have no defaults, so a plan missing them aborts before any
        // validation is evaluated — the assertion would pass on the wrong diagnostic.
        &[
            ("name", "example"),
            ("token", "example-token"),
            ("network_mode", "use-default"),
        ],
        "must name subnets",
    );
}

/// The counterpart: an open sandbox routes through no connector, so the mode stays available.
#[test]
fn an_open_sandbox_leaves_the_default_network_selectable() {
    let (stack, settings) = sandbox_stack("acme-sandbox-open-modes", SandboxEgress::Allow);
    let module = render(&stack, TerraformTarget::Aws, settings);

    assert_terraform_valid(&module, "open sandbox module");
    let variables = module.get("variables.tf").expect("variables.tf renders");
    assert!(
        !variables.contains("must name subnets"),
        "an open sandbox must not restrict the network mode:\n{variables}"
    );
}

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
    (
        ResourceLifecycle::Live,
        "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
        None,
    ),
    (
        ResourceLifecycle::Live,
        "s3://acme-artifacts-{region}/sandbox-bundle/f00dcafe/bundle.zip",
        None,
    ),
    (
        ResourceLifecycle::Live,
        "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
        Some("123456789012.dkr.ecr.eu-west-1.amazonaws.com/acme/agents-base:1.4"),
    ),
    (
        ResourceLifecycle::Live,
        "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
        Some("123456789012.dkr.ecr.{region}.amazonaws.com/acme/agents-base@sha256:f00d"),
    ),
];

/// Other sets grant role writes on `role/<prefix>-*`. One guard refuses them on the management role
/// by its name, which the prefix cap keeps short of a hash; the other on every role carrying
/// setup's sandbox tags, which both roles the module creates for a deny sandbox carry.
#[test]
fn the_management_role_may_not_rewrite_a_sandboxs_setup_roles() {
    let (mut stack, settings) = sandbox_stack("acme-guarded", SandboxEgress::Deny);
    stack.permissions.management =
        ManagementPermissions::extend(PermissionProfile::new().global([
            "sandbox/management",
            "artifact-registry/management",
            alien_permissions::SANDBOX_SETUP_ROLES_GUARD,
            alien_permissions::MANAGEMENT_ROLE_GUARD,
        ]));
    stack.resources.insert(
        "management".to_string(),
        alien_core::ResourceEntry {
            config: alien_core::Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: vec![],
            remote_access: false,
            enabled_when: None,
        },
    );
    let module = render(&stack, TerraformTarget::Aws, settings);

    let mut denies = Vec::new();
    let mut sandbox_role_tags = Vec::new();
    for (file, contents) in module.iter() {
        if !file.ends_with(".tf") {
            continue;
        }
        let body: hcl::Body =
            hcl::parse(contents).unwrap_or_else(|error| panic!("{file} parses: {error}"));
        for kind in ["aws_iam_policy", "aws_iam_role_policy"] {
            for block in resource_blocks(&body, kind) {
                if block.labels()[1].as_str().starts_with("management") {
                    collect_denies(jsonencoded(block_attribute(block, "policy")), &mut denies);
                }
            }
        }
        if file == "agents.tf" {
            for role in resource_blocks(&body, "aws_iam_role") {
                sandbox_role_tags.push((
                    role.labels()[1].as_str().to_string(),
                    block_attribute(role, "tags").expr().to_string(),
                ));
            }
        }
    }
    let own_role = serde_json::json!([format!(
        "arn:aws:iam::{PARITY_ACCOUNT}:role/{PARITY_PREFIX}-management"
    )]);
    let (own_role_denies, denies): (Vec<_>, Vec<_>) = denies
        .into_iter()
        .partition(|deny| deny["Resource"] == own_role);
    assert_eq!(
        own_role_denies.len(),
        1,
        "the management role may not rewrite itself"
    );
    assert_eq!(denies.len(), 1, "{denies:#?}");
    // Typed, so the snapshot keeps field order whether or not a workspace build turns on
    // serde_json's `preserve_order`, which reorders the raw `Value`'s keys.
    let guards: Vec<alien_permissions::generators::AwsIamStatement> =
        [&own_role_denies[0], &denies[0]]
            .into_iter()
            .map(|deny| serde_json::from_value(deny.clone()).expect("a deny is an IAM statement"))
            .collect();
    insta::assert_snapshot!(
        "aws_management_role_guards",
        serde_json::to_string_pretty(&guards).expect("serializes")
    );
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

    assert_eq!(
        sandbox_role_tags.len(),
        2,
        "the build and egress operator roles: {sandbox_role_tags:?}"
    );
    for (role, tags) in sandbox_role_tags {
        let tags: hcl::Expression = hcl::from_str(&format!("x = {tags}"))
            .map(|body: hcl::Body| body.attributes().next().unwrap().expr().clone())
            .unwrap_or_else(|error| panic!("{role} tags parse: {error}"));
        let hcl::Expression::Object(object) = tags else {
            panic!("{role} tags must be an object");
        };
        for (key, value) in [("managed-by", "setup"), ("resource-type", "sandbox")] {
            assert!(
                object.iter().any(|(k, v)| {
                    let k = match k {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        hcl::ObjectKey::Expression(expression) => {
                            expression.to_string().trim_matches('"').to_string()
                        }
                        other => format!("{other:?}"),
                    };
                    k == key && v == &hcl::Expression::String(value.to_string())
                }),
                "{role} must carry {key}={value} for the guard to reach it: {object:?}"
            );
        }
    }
    assert_terraform_valid(
        &module,
        "management_role_may_not_rewrite_sandbox_setup_roles",
    );
}

/// Every `Effect = "Deny"` statement under `expression`, evaluated.
fn collect_denies(expression: &hcl::Expression, denies: &mut Vec<serde_json::Value>) {
    match expression {
        hcl::Expression::Array(items) => {
            for item in items {
                collect_denies(item, denies);
            }
        }
        hcl::Expression::Object(object) => {
            let field = |name: &str| {
                object.iter().find_map(|(key, value)| {
                    let key = match key {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        hcl::ObjectKey::Expression(hcl::Expression::String(text)) => text.clone(),
                        _ => return None,
                    };
                    (key == name).then_some(value)
                })
            };
            if field("Effect") == Some(&hcl::Expression::String("Deny".to_string())) {
                denies.push(evaluate_policy_expression(expression));
            } else if let Some(statements) = field("Statement") {
                collect_denies(statements, denies);
            }
        }
        _ => {}
    }
}

/// Evaluates the `jsonencode` argument an IAM document is written as, resolving the data sources
/// the build role reads to the fixed parity values and panicking on anything else: a placeholder
/// would let both sides compare equal without either being checked.
fn evaluate_policy_expression(expression: &hcl::Expression) -> serde_json::Value {
    let resolve_template = |text: &str| {
        let resolved = text
            .replace("${local.resource_prefix}", PARITY_PREFIX)
            .replace("${data.aws_partition.current.partition}", PARITY_PARTITION)
            .replace("${data.aws_region.current.region}", PARITY_REGION)
            .replace(
                "${data.aws_caller_identity.current.account_id}",
                PARITY_ACCOUNT,
            );
        assert!(
            !resolved.contains("${"),
            "unresolved interpolation left in {resolved}"
        );
        serde_json::Value::String(resolved)
    };
    match expression {
        hcl::Expression::String(text) => resolve_template(text),
        hcl::Expression::TemplateExpr(template) => match template.as_ref() {
            hcl::TemplateExpr::QuotedString(text) => resolve_template(text),
            heredoc => panic!("the parity test cannot evaluate a heredoc: {heredoc:?}"),
        },
        hcl::Expression::Array(items) => {
            serde_json::Value::Array(items.iter().map(evaluate_policy_expression).collect())
        }
        hcl::Expression::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        hcl::ObjectKey::Expression(hcl::Expression::String(text)) => text.clone(),
                        other => panic!("the parity test cannot evaluate the key {other:?}"),
                    };
                    (key, evaluate_policy_expression(value))
                })
                .collect(),
        ),
        hcl::Expression::Traversal(_)
            if expression.to_string() == "data.aws_caller_identity.current.account_id" =>
        {
            serde_json::Value::String(PARITY_ACCOUNT.to_string())
        }
        other => panic!("the parity test cannot evaluate {other}"),
    }
}

/// The argument of the `jsonencode(...)` call an IAM document attribute holds.
fn jsonencoded(attribute: &hcl::Attribute) -> &hcl::Expression {
    match attribute.expr() {
        hcl::Expression::FuncCall(call) if call.name.name.as_str() == "jsonencode" => {
            assert_eq!(call.args.len(), 1, "jsonencode takes one argument");
            &call.args[0]
        }
        other => panic!("{} must be a jsonencode call: {other}", attribute.key()),
    }
}

/// The `resource "<kind>" "<label>"` blocks of a rendered file.
fn resource_blocks<'a>(
    body: &'a hcl::Body,
    kind: &'a str,
) -> impl Iterator<Item = &'a hcl::Block> + 'a {
    body.blocks().filter(move |block| {
        block.identifier() == "resource"
            && block.labels().first().map(|label| label.as_str()) == Some(kind)
    })
}

fn block_attribute<'a>(block: &'a hcl::Block, key: &str) -> &'a hcl::Attribute {
    block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == key)
        .unwrap_or_else(|| panic!("{:?} has no {key}", block.labels()))
}

/// A direct deploy creates the build role through the IAM API from `SandboxBuildRole`, so a
/// grant changed in the module alone would give the two install paths different roles.
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
        let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
        let sandbox_file: hcl::Body =
            hcl::parse(module.get("agents.tf").expect("agents.tf renders"))
                .unwrap_or_else(|error| panic!("{case}: agents.tf parses: {error}"));

        let policies: Vec<_> = resource_blocks(&sandbox_file, "aws_iam_role_policy")
            .filter(|block| {
                block_attribute(block, "name").expr()
                    == &hcl::Expression::String(SANDBOX_BUILD_POLICY_NAME.to_string())
            })
            .collect();
        assert_eq!(policies.len(), 1, "{case}: one build policy");
        let role_label = policies[0].labels()[1].as_str();
        let role = resource_blocks(&sandbox_file, "aws_iam_role")
            .find(|block| block.labels()[1].as_str() == role_label)
            .unwrap_or_else(|| panic!("{case}: the build role {role_label} renders"));

        let expected = SandboxBuildRole::builder()
            .sandbox_id("agents")
            .partition(PARITY_PARTITION)
            .account_id(PARITY_ACCOUNT)
            .region(PARITY_REGION)
            .bundle_uri(bundle_uri)
            .runtime_built(lifecycle == ResourceLifecycle::Live)
            .maybe_private_base_image(private_base_image)
            .build();

        assert_eq!(
            evaluate_policy_expression(jsonencoded(block_attribute(policies[0], "policy"))),
            serde_json::to_value(expected.policy().expect("the builder accepts the fixture"))
                .expect("serializes"),
            "{case}: permission policy"
        );
        assert_eq!(
            evaluate_policy_expression(jsonencoded(block_attribute(role, "assume_role_policy"))),
            serde_json::to_value(expected.trust_policy()).expect("serializes"),
            "{case}: trust policy"
        );
    }
}

/// A direct deploy creates the operator role through the IAM API from the shared builder, so a
/// grant changed in the module alone would give the two install paths different roles.
#[test]
fn the_emitted_operator_role_matches_the_shared_builder() {
    let (stack, settings) = sandbox_stack("acme-sandbox-egress-parity", SandboxEgress::Deny);
    let module = render(&stack, TerraformTarget::Aws, settings);
    let sandbox_file: hcl::Body = hcl::parse(module.get("agents.tf").expect("agents.tf renders"))
        .unwrap_or_else(|error| panic!("agents.tf parses: {error}"));

    let policies: Vec<_> = resource_blocks(&sandbox_file, "aws_iam_role_policy")
        .filter(|block| {
            block_attribute(block, "name").expr()
                == &hcl::Expression::String(SANDBOX_EGRESS_POLICY_NAME.to_string())
        })
        .collect();
    assert_eq!(policies.len(), 1, "one operator policy");
    let role_label = policies[0].labels()[1].as_str();
    let role = resource_blocks(&sandbox_file, "aws_iam_role")
        .find(|block| block.labels()[1].as_str() == role_label)
        .unwrap_or_else(|| panic!("the operator role {role_label} renders"));

    assert_eq!(
        evaluate_policy_expression(jsonencoded(block_attribute(policies[0], "policy"))),
        sandbox_egress_operator_policy(PARITY_PARTITION, PARITY_ACCOUNT, PARITY_REGION),
        "permission policy"
    );
    assert_eq!(
        evaluate_policy_expression(jsonencoded(block_attribute(role, "assume_role_policy"))),
        sandbox_egress_operator_trust_policy(),
        "trust policy"
    );
    assert_eq!(
        evaluate_name(block_attribute(role, "name").expr()),
        Value::from(sandbox_egress_name(PARITY_PREFIX, "agents")),
        "the direct path finds the role by this name"
    );
}

const PARITY_PREFIX: &str = "acme-parity";
const PARITY_CONNECTOR_ARN: &str =
    "arn:aws-us-gov:lambda:us-gov-east-1:987654321098:network-connector:nc-0parity";

/// Evaluates a name template with `local.resource_prefix` bound to the parity prefix. Any other
/// form panics rather than guessing.
fn evaluate_name(expression: &hcl::Expression) -> serde_json::Value {
    match expression {
        hcl::Expression::TemplateExpr(template) => match template.as_ref() {
            hcl::TemplateExpr::QuotedString(text) => {
                let resolved = text.replace("${local.resource_prefix}", PARITY_PREFIX);
                assert!(
                    !resolved.contains("${"),
                    "unresolved interpolation left in {resolved}"
                );
                serde_json::Value::from(resolved)
            }
            heredoc => panic!("the parity test cannot evaluate a heredoc: {heredoc:?}"),
        },
        other => panic!("the parity test cannot evaluate the name {other}"),
    }
}

/// Evaluates a sandbox's registration `importData`. The build role resolves from its own `name`,
/// so the direct side's derivation is compared against what the module would create.
fn evaluate_registration(sandbox_file: &hcl::Body, expression: &hcl::Expression) -> Value {
    match expression {
        hcl::Expression::Bool(flag) => Value::from(*flag),
        hcl::Expression::Number(number) => {
            Value::from(number.as_u64().expect("a port is unsigned"))
        }
        hcl::Expression::String(_) | hcl::Expression::TemplateExpr(_) => {
            evaluate_policy_expression(expression)
        }
        hcl::Expression::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| evaluate_registration(sandbox_file, item))
                .collect(),
        ),
        hcl::Expression::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        other => panic!("the parity test cannot evaluate the key {other:?}"),
                    };
                    (key, evaluate_registration(sandbox_file, value))
                })
                .collect(),
        ),
        hcl::Expression::Traversal(_) => {
            let path = expression.to_string();
            let parts: Vec<&str> = path.split('.').collect();
            match parts.as_slice() {
                ["aws_iam_role", label, "arn"] => {
                    let role = resource_blocks(sandbox_file, "aws_iam_role")
                        .find(|block| block.labels()[1].as_str() == *label)
                        .unwrap_or_else(|| panic!("{path} names a role that must render"));
                    assert!(
                        role.body().attributes().all(|a| a.key() != "path"),
                        "the pass grant is scoped to the root path"
                    );
                    let name = evaluate_name(block_attribute(role, "name").expr());
                    Value::from(format!(
                        "arn:{PARITY_PARTITION}:iam::{PARITY_ACCOUNT}:role/{}",
                        name.as_str().expect("a role name")
                    ))
                }
                ["awscc_lambda_network_connector", label, "arn"] => {
                    resource_blocks(sandbox_file, "awscc_lambda_network_connector")
                        .find(|block| block.labels()[1].as_str() == *label)
                        .unwrap_or_else(|| panic!("{path} names a connector that must render"));
                    Value::from(PARITY_CONNECTOR_ARN)
                }
                _ => panic!("the parity test cannot resolve {path}"),
            }
        }
        other => panic!("the parity test cannot evaluate {other}"),
    }
}

/// A direct deploy registers a runtime-built sandbox from `AwsSandboxImportData::runtime_built`
/// instead of this module, so a field changed in the module alone would hand the controller a
/// different build role, bundle, egress, or preview set depending on how it was installed.
#[test]
fn the_emitted_registration_matches_the_direct_seed() {
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
        let (mut stack, settings) = live_sandbox_stack("acme-sandbox-parity", egress.clone());
        let sandbox = Sandbox {
            preview_ports: vec![8080, 3000],
            ..sandbox_fixture_with(egress.clone(), bundle_uri)
        };
        stack
            .resources
            .get_mut("agents")
            .expect("the sandbox is in the stack")
            .config = alien_core::Resource::new(sandbox.clone());
        let case = format!("{egress:?} sandbox built from {bundle_uri}");
        let module = render(&stack, TerraformTarget::Aws, settings);
        let sandbox_file: hcl::Body =
            hcl::parse(module.get("agents.tf").expect("agents.tf renders"))
                .unwrap_or_else(|error| panic!("{case}: agents.tf parses: {error}"));
        let locals: hcl::Body = hcl::parse(module.get("locals.tf").expect("locals.tf renders"))
            .unwrap_or_else(|error| panic!("{case}: locals.tf parses: {error}"));
        let registered = locals
            .blocks()
            .flat_map(|block| block.body().attributes())
            .find(|attribute| attribute.key() == "deployment_resources")
            .unwrap_or_else(|| panic!("{case}: the registration list renders"));
        let hcl::Expression::Array(entries) = registered.expr() else {
            panic!("{case}: the registration list is a list");
        };
        let import_data = entries
            .iter()
            .find_map(|entry| {
                let hcl::Expression::Object(object) = entry else {
                    return None;
                };
                let field = |name: &str| {
                    object.iter().find_map(|(key, value)| {
                        matches!(key, hcl::ObjectKey::Identifier(id) if id.as_str() == name)
                            .then_some(value)
                    })
                };
                (field("id") == Some(&hcl::Expression::String("agents".to_string())))
                    .then(|| field("importData").expect("a registration carries importData"))
            })
            .unwrap_or_else(|| panic!("{case}: the sandbox registers"));

        let emitted: alien_core::import::data::AwsSandboxImportData =
            serde_json::from_value(evaluate_registration(&sandbox_file, import_data))
                .unwrap_or_else(|error| panic!("{case}: the importer must accept it: {error}"));
        let direct = alien_core::import::data::AwsSandboxImportData::runtime_built(
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

/// The ARN of the operator role the parity module creates; its name is pinned against
/// `sandbox_egress_name` by `the_emitted_operator_role_matches_the_shared_builder`.
const PARITY_OPERATOR_ARN: &str = "arn:aws-us-gov:iam::987654321098:role/acme-parity-agents-egress";
const PARITY_SECURITY_GROUP: &str = "sg-0parity";
const CREATED_SUBNETS: [&str; 2] = ["subnet-created-1", "subnet-created-2"];
const EXISTING_SUBNETS: [&str; 2] = ["subnet-existing-a", "subnet-existing-b"];

/// A network `egress: deny` accepts, the `network_mode` an installer picks, and the private
/// subnets the connector must name once both are bound.
fn egress_parity_cases() -> [(NetworkSettings, &'static str, [&'static str; 2]); 3] {
    let create = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    [
        (create.clone(), "create-new", CREATED_SUBNETS),
        (create, "use-existing", EXISTING_SUBNETS),
        (
            NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-0parity".to_string(),
                public_subnet_ids: vec!["subnet-public-a".to_string()],
                private_subnet_ids: EXISTING_SUBNETS.map(String::from).to_vec(),
                security_group_ids: vec!["sg-0network".to_string()],
            },
            "use-existing",
            EXISTING_SUBNETS,
        ),
    ]
}

fn deny_module(network: &NetworkSettings) -> alien_terraform::ModuleFiles {
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
    render(&stack, TerraformTarget::Aws, settings)
}

fn sandbox_body(module: &alien_terraform::ModuleFiles) -> hcl::Body {
    hcl::parse(module.get("agents.tf").expect("agents.tf renders"))
        .unwrap_or_else(|error| panic!("agents.tf parses: {error}"))
}

fn only_resource<'a>(body: &'a hcl::Body, kind: &'a str, label: &str) -> &'a hcl::Block {
    let blocks: Vec<_> = resource_blocks(body, kind)
        .filter(|block| block.labels()[1].as_str() == label)
        .collect();
    assert_eq!(blocks.len(), 1, "one {kind}.{label}");
    blocks[0]
}

/// `awscc` spells the schema's property names in snake_case; Cloud Control takes them as declared.
fn schema_name(snake: &str) -> String {
    snake
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// Evaluates a connector attribute with the module's references bound to what a deployed module
/// holds for `network_mode`. Any reference outside this table panics, so a new one cannot compare
/// equal unchecked.
fn evaluate_connector(expression: &hcl::Expression, network_mode: &str) -> Value {
    match expression {
        hcl::Expression::String(text) => Value::from(text.clone()),
        hcl::Expression::TemplateExpr(_) => evaluate_name(expression),
        hcl::Expression::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| evaluate_connector(item, network_mode))
                .collect(),
        ),
        hcl::Expression::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        hcl::ObjectKey::Expression(hcl::Expression::String(text)) => text.clone(),
                        other => panic!("the parity test cannot evaluate the key {other:?}"),
                    };
                    (schema_name(&key), evaluate_connector(value, network_mode))
                })
                .collect(),
        ),
        hcl::Expression::Conditional(conditional) => {
            match evaluate_connector(&conditional.cond_expr, network_mode) {
                Value::Bool(true) => evaluate_connector(&conditional.true_expr, network_mode),
                Value::Bool(false) => evaluate_connector(&conditional.false_expr, network_mode),
                other => panic!("a condition evaluates to a bool, not {other}"),
            }
        }
        hcl::Expression::Operation(operation) => match operation.as_ref() {
            Operation::Binary(op) if op.operator == BinaryOperator::Eq => Value::from(
                evaluate_connector(&op.lhs_expr, network_mode)
                    == evaluate_connector(&op.rhs_expr, network_mode),
            ),
            other => panic!("the parity test cannot evaluate {other:?}"),
        },
        hcl::Expression::Traversal(_) => match expression.to_string().as_str() {
            "local.resource_prefix" => Value::from(PARITY_PREFIX),
            "var.network_mode" => Value::from(network_mode),
            "aws_iam_role.agents_egress.arn" => Value::from(PARITY_OPERATOR_ARN),
            "aws_security_group.agents_egress.id" => Value::from(PARITY_SECURITY_GROUP),
            "aws_subnet.default_network_private[*].id" => serde_json::json!(CREATED_SUBNETS),
            "var.private_subnet_ids" | "var.default_network_private_subnet_ids" => {
                serde_json::json!(EXISTING_SUBNETS)
            }
            other => panic!("the parity test cannot resolve {other}"),
        },
        other => panic!("the parity test cannot evaluate {other}"),
    }
}

/// A direct deploy sends this builder's output to Cloud Control as the connector's desired state;
/// it must be the resource the module creates for the same sandbox.
#[test]
fn the_emitted_connector_matches_the_direct_desired_state() {
    for (network, network_mode, subnets) in egress_parity_cases() {
        let case = format!("connector on {network:?} with network_mode={network_mode}");
        let sandbox_file = sandbox_body(&deny_module(&network));
        let connector = only_resource(&sandbox_file, "awscc_lambda_network_connector", "agents");

        let emitted = serde_json::Value::Object(
            connector
                .body()
                .attributes()
                .filter(|attribute| attribute.key() != "depends_on")
                .map(|attribute| {
                    (
                        schema_name(attribute.key()),
                        evaluate_connector(attribute.expr(), network_mode),
                    )
                })
                .collect(),
        );
        let subnets = subnets.map(String::from).to_vec();
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

/// Tags carry `insertionOrder: false` in the connector's schema, so their order is not state.
fn tags_sorted(mut properties: serde_json::Value) -> serde_json::Value {
    if let Some(tags) = properties["Tags"].as_array_mut() {
        tags.sort_by(|a, b| a["Key"].as_str().cmp(&b["Key"].as_str()));
    }
    properties
}

/// The group is what enforces `egress: deny`, and a direct deploy creates it named
/// `sandbox_egress_name` with exactly one all-protocol rule to [`LOOPBACK_ONLY_CIDR`] and no
/// ingress. The module keeps `name_prefix`, which a rename to `name` would replace the group over.
#[test]
fn the_emitted_deny_group_matches_the_direct_rule_set() {
    for (network, _, _) in egress_parity_cases() {
        let case = format!("deny group on {network:?}");
        let module = deny_module(&network);
        let sandbox_file = sandbox_body(&module);
        let group = only_resource(&sandbox_file, "aws_security_group", "agents_egress");
        let attributes: Vec<&str> = group.body().attributes().map(|a| a.key()).collect();

        assert_eq!(
            evaluate_name(block_attribute(group, "name_prefix").expr()),
            serde_json::json!(format!("{}-", sandbox_egress_name(PARITY_PREFIX, "agents"))),
            "{case}"
        );
        assert!(
            !attributes.contains(&"name"),
            "{case}: name_prefix alone names the group"
        );
        assert_eq!(
            block_attribute(group, "description").expr(),
            &hcl::Expression::String("Sandbox agents session egress".to_string()),
            "{case}"
        );
        assert!(
            !attributes.contains(&"egress") && !attributes.contains(&"ingress"),
            "{case}: rules are written as blocks, where they can be counted: {attributes:?}"
        );
        assert_eq!(
            group
                .body()
                .blocks()
                .filter(|block| block.identifier() == "ingress")
                .count(),
            0,
            "{case}: no ingress"
        );

        let egress: Vec<_> = group
            .body()
            .blocks()
            .filter(|block| block.identifier() == "egress")
            .collect();
        assert_eq!(
            egress.len(),
            1,
            "{case}: one rule, or the default allow-all survives"
        );
        let rule: Vec<(&str, &hcl::Expression)> = egress[0]
            .body()
            .attributes()
            .map(|attribute| (attribute.key(), attribute.expr()))
            .collect();
        assert_eq!(
            rule,
            vec![
                ("from_port", &hcl::Expression::Number(0.into())),
                ("to_port", &hcl::Expression::Number(0.into())),
                ("protocol", &hcl::Expression::String("-1".to_string())),
                (
                    "cidr_blocks",
                    &hcl::Expression::Array(vec![hcl::Expression::String(
                        LOOPBACK_ONLY_CIDR.to_string()
                    )])
                ),
            ],
            "{case}: all protocols to the destination that reaches nothing, and no other target"
        );

        // A standalone rule resource widens the group as surely as an inline one.
        for (file, contents) in module.iter().filter(|(file, _)| file.ends_with(".tf")) {
            let body: hcl::Body = hcl::parse(contents)
                .unwrap_or_else(|error| panic!("{case}: {file} parses: {error}"));
            for kind in [
                "aws_security_group_rule",
                "aws_vpc_security_group_egress_rule",
                "aws_vpc_security_group_ingress_rule",
            ] {
                for block in resource_blocks(&body, kind) {
                    let rendered = hcl::to_string(block).expect("the block renders");
                    assert!(
                        !rendered.contains("aws_security_group.agents_egress"),
                        "{case}: {file} adds a rule to the deny group:\n{rendered}"
                    );
                }
            }
        }
    }
}
