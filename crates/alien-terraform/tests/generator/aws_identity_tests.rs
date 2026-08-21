//! AWS identity & network — service-account / management /
//! network (Create + ByoVpcAws + UseDefault).

use super::helpers::{
    assert_terraform_valid, assert_terraform_variable_plan_invalid_contains, gate_input, render,
    snapshot_module, try_render,
};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, ServiceAccount,
    Stack, StackSettings, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

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

    let security_group = rendered
        .split("resource \"aws_security_group\" \"agents_egress\"")
        .nth(1)
        .unwrap_or_else(|| panic!("the sandbox egress security group must render:\n{rendered}"))
        .split("\nresource \"")
        .next()
        .expect("the block runs to the next resource");
    assert_eq!(
        security_group.matches("egress {").count(),
        1,
        "exactly one egress rule, or the default allow-all survives:\n{security_group}"
    );
    assert!(
        security_group.contains("\"127.0.0.1/32\""),
        "the only permitted destination must be the one that reaches nothing:\n{security_group}"
    );
    assert!(
        !security_group.contains("0.0.0.0/0"),
        "a wide egress rule turns deny back into outbound access:\n{security_group}"
    );

    let connector = rendered
        .split("resource \"awscc_lambda_network_connector\" \"agents\"")
        .nth(1)
        .unwrap_or_else(|| panic!("the egress connector must render:\n{rendered}"))
        .split("\nresource \"")
        .next()
        .expect("the block runs to the next resource");
    assert!(
        connector.contains("aws_security_group.agents_egress.id"),
        "the connector must carry the group that denies:\n{connector}"
    );
    assert!(
        connector.contains("aws_subnet.default_network_private"),
        "the connector must place its ENIs in the network's private subnets:\n{connector}"
    );
    assert!(
        connector.contains("\"MicroVm\""),
        "the connector must be usable by MicroVMs:\n{connector}"
    );

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
