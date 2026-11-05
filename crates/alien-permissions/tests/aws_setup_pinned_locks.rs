//! What the management identity can do to the values setup pins for a deployment: the machine
//! image its machines boot, the roles it hands to EC2, the anchor and the deployer's inputs.
//! Checked by evaluating concrete requests against the rendered policies the way IAM does: an
//! explicit Deny wins, otherwise any matching Allow allows, otherwise the request is denied.

mod iam_policy;

use alien_core::PermissionSet;
use alien_permissions::{
    generators::{
        aws_permissions_boundary_covers, aws_stack_permissions_boundary, AwsIamStatement,
        AwsRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext, MANAGEMENT_ROLE_GUARD,
};
use iam_policy::{evaluate, Decision, Request};

const ACCOUNT: &str = "123456789012";
const PUBLISHER: &str = "345678901234";
const PREFIX: &str = "app";
const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/app-deployment-management-boundary";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region("us-east-1")
        .with_aws_permissions_boundary_arn(BOUNDARY)
}

fn render(set: &PermissionSet, context: &PermissionContext) -> Vec<AwsIamStatement> {
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, BindingTarget::Stack, context)
        .unwrap_or_else(|error| panic!("{} should render: {error}", set.id))
        .statement
}

fn rendered(ids: &[&str], context: &PermissionContext) -> Vec<AwsIamStatement> {
    ids.iter()
        .flat_map(|id| render(get_permission_set(id).expect("registered set"), context))
        .collect()
}

/// A policy a developer's extended profile or the customer could attach to the same role.
fn allow_everything(actions: &[&str]) -> Vec<AwsIamStatement> {
    let set: PermissionSet = serde_json::from_value(serde_json::json!({
        "id": "test/allow-everything",
        "description": "Allows every listed action on every resource",
        "platforms": { "aws": [{
            "grant": { "actions": actions },
            "binding": { "stack": { "resources": ["*"] } }
        }] }
    }))
    .expect("permission set parses");
    render(&set, &context())
}

fn image(owner: &str) -> Request {
    Request::new(
        "ec2:RunInstances",
        "arn:aws:ec2:us-east-1::image/ami-0123456789abcdef0",
    )
    .with("ec2:Owner", owner)
}

#[test]
fn management_boots_only_the_publishers_images() {
    let context = context().with_machine_image_owner(PUBLISHER);
    for id in ["compute-cluster/management", "compute-cluster/provision"] {
        let compute = rendered(&[id], &context);
        assert_eq!(
            evaluate(&compute, &image(PUBLISHER)),
            Decision::Allowed,
            "{id}"
        );
        for other in ["amazon", "aws-marketplace", "137112412989", ACCOUNT] {
            assert_eq!(
                evaluate(&compute, &image(other)),
                Decision::NotAllowed,
                "{id}: image of {other}"
            );
        }

        // Another policy on the role may allow every image; the guard still refuses them.
        let mut guarded = compute.clone();
        guarded.extend(rendered(&[MANAGEMENT_ROLE_GUARD], &context));
        guarded.extend(allow_everything(&["ec2:RunInstances"]));
        assert_eq!(
            evaluate(&guarded, &image(PUBLISHER)),
            Decision::Allowed,
            "{id}"
        );
        for other in ["amazon", "137112412989", ACCOUNT] {
            assert_eq!(
                evaluate(&guarded, &image(other)),
                Decision::ExplicitlyDenied,
                "{id}: image of {other}"
            );
        }
    }
}

#[test]
fn without_a_publisher_the_guard_refuses_every_image() {
    let mut statements = rendered(&[MANAGEMENT_ROLE_GUARD], &context());
    statements.extend(allow_everything(&["ec2:RunInstances"]));
    for owner in [PUBLISHER, "amazon", ACCOUNT] {
        assert_eq!(
            evaluate(&statements, &image(owner)),
            Decision::ExplicitlyDenied,
            "image of {owner}"
        );
    }
    // The image Deny names images only: the rest of a launch is left to the Allows.
    assert_eq!(
        evaluate(
            &statements,
            &Request::new(
                "ec2:RunInstances",
                "arn:aws:ec2:us-east-1:123456789012:subnet/subnet-0123456789abcdef0"
            )
        ),
        Decision::Allowed
    );
}

#[test]
fn the_compute_sets_need_the_publisher() {
    for id in ["compute-cluster/management", "compute-cluster/provision"] {
        let error = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                get_permission_set(id).unwrap(),
                BindingTarget::Stack,
                &context(),
            )
            .expect_err("a compute set must not render without the publisher");
        assert_eq!(error.code, "VARIABLE_NOT_FOUND", "{id}");
        assert!(error.message.contains("machineImageOwner"), "{id}");
    }
}

#[test]
fn ec2_receives_only_the_compute_roles() {
    let context = context().with_machine_image_owner(PUBLISHER);
    for id in ["compute-cluster/management", "compute-cluster/provision"] {
        let statements = rendered(&[id], &context);
        let pass = |role: &str, service: &str| {
            evaluate(
                &statements,
                &Request::new(
                    "iam:PassRole",
                    format!("arn:aws:iam::{ACCOUNT}:role/{role}"),
                )
                .with("iam:PassedToService", service),
            )
        };
        for role in [
            "app-compute",
            "app-compute-instances",
            "app-compute-isolation-v1",
            "app-compute-role",
            "app-compute-isolation-v1-role",
        ] {
            assert_eq!(
                pass(role, "ec2.amazonaws.com"),
                Decision::Allowed,
                "{id}: {role}"
            );
            assert_eq!(
                pass(role, "lambda.amazonaws.com"),
                Decision::NotAllowed,
                "{id}: {role}"
            );
        }
        for role in [
            "app-management",
            "app-api-sa",
            "app-execution-sa",
            "app-agents-build",
            "app-volume-backups-compute",
            "app-access",
            "other-compute",
        ] {
            assert_eq!(
                pass(role, "ec2.amazonaws.com"),
                Decision::NotAllowed,
                "{id}: {role}"
            );
        }
    }
}

#[test]
fn management_cannot_write_what_setup_pins() {
    let mut statements = rendered(&[MANAGEMENT_ROLE_GUARD], &context());
    statements.extend(allow_everything(&["ssm:*"]));
    let writes = [
        "ssm:PutParameter",
        "ssm:DeleteParameter",
        "ssm:DeleteParameters",
        "ssm:LabelParameterVersion",
        "ssm:AddTagsToResource",
        "ssm:RemoveTagsFromResource",
    ];
    // Any stack's anchor and inputs, in any region: a sibling deployment's too.
    for parameter in [
        "arn:aws:ssm:us-east-1:123456789012:parameter/_release-trust/app",
        "arn:aws:ssm:eu-west-1:123456789012:parameter/_release-trust/other",
        "arn:aws:ssm:us-east-1:123456789012:parameter/_deployer-inputs/app",
        "arn:aws:ssm:us-east-1:123456789012:parameter/_deployer-inputs/other",
    ] {
        for action in writes {
            assert_eq!(
                evaluate(&statements, &Request::new(action, parameter)),
                Decision::ExplicitlyDenied,
                "{action} on {parameter}"
            );
        }
    }
    // The guard leaves the stack's own parameters to the vault sets.
    assert_eq!(
        evaluate(
            &statements,
            &Request::new(
                "ssm:PutParameter",
                "arn:aws:ssm:us-east-1:123456789012:parameter/app-secrets-token"
            )
        ),
        Decision::Allowed
    );
}

#[test]
fn a_protected_deployment_without_a_boundary_is_refused() {
    let unbounded = PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region("us-east-1")
        .with_machine_image_owner(PUBLISHER);
    for id in ["compute-cluster/management", "compute-cluster/provision"] {
        let set = get_permission_set(id).unwrap();

        // Unprotected setups without a boundary keep the legacy prefix-wide pass.
        let legacy = render(set, &unbounded);
        assert_eq!(
            evaluate(
                &legacy,
                &Request::new("iam:PassRole", "arn:aws:iam::123456789012:role/app-api-sa")
            ),
            Decision::Allowed,
            "{id}"
        );

        let error = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                set,
                BindingTarget::Stack,
                &unbounded.clone().with_release_trust_protected(true),
            )
            .expect_err("a protected deployment never receives legacy grants");
        assert_eq!(error.code, "PROTECTED_DEPLOYMENT_WITHOUT_BOUNDARY", "{id}");

        // With the boundary recorded, a protected deployment renders the bounded grants.
        let bounded = render(
            set,
            &context()
                .with_machine_image_owner(PUBLISHER)
                .with_release_trust_protected(true),
        );
        assert_eq!(
            evaluate(
                &bounded,
                &Request::new("iam:PassRole", "arn:aws:iam::123456789012:role/app-api-sa")
                    .with("iam:PassedToService", "ec2.amazonaws.com")
            ),
            Decision::NotAllowed,
            "{id}"
        );
    }
}

/// Roles the management identity writes are capped by the boundary. A role it writes may read
/// this deployment's anchor and inputs, as the machine role does, and nothing more there: no
/// write, no retagging, no other deployment's values.
#[test]
fn roles_management_writes_can_only_read_what_setup_pins() {
    let stack = alien_core::Stack::new("app".to_string()).build();
    let context = context().with_managing_account_id("210987654321");
    let boundary = aws_stack_permissions_boundary(&stack, &context, |_, _| Ok(context.clone()))
        .expect("the boundary renders");
    let covers = |action: &str, resource: &str| {
        aws_permissions_boundary_covers(
            &boundary,
            &serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [{ "Effect": "Allow", "Action": action, "Resource": resource }]
            }),
        )
        .expect("the document compares")
    };
    let anchor = "arn:aws:ssm:us-east-1:123456789012:parameter/_release-trust/app";
    let input = "arn:aws:ssm:us-east-1:123456789012:parameter/_deployer-inputs/app";
    for parameter in [anchor, input] {
        assert!(covers("ssm:GetParameter", parameter), "{parameter}");
        for action in [
            "ssm:PutParameter",
            "ssm:DeleteParameter",
            "ssm:LabelParameterVersion",
            "ssm:AddTagsToResource",
            "ssm:RemoveTagsFromResource",
        ] {
            assert!(!covers(action, parameter), "{action} on {parameter}");
        }
    }
    for parameter in [
        "arn:aws:ssm:us-east-1:123456789012:parameter/_release-trust/app-prod",
        "arn:aws:ssm:us-east-1:123456789012:parameter/_deployer-inputs/app-prod",
    ] {
        assert!(!covers("ssm:GetParameter", parameter), "{parameter}");
    }
    // The stack's own parameters stay inside the scoped Allow.
    assert!(covers(
        "ssm:PutParameter",
        "arn:aws:ssm:us-east-1:123456789012:parameter/app-secrets-token"
    ));
}
