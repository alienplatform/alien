//! [`ManagementTrustOptions`]: a required `ManagementExternalId` and an
//! allow-list for `ManagingRoleArn`.
//!
//! Each test checks parameter values the way CloudFormation validates them
//! before it creates or updates anything:
//! - a missing value takes the default, and a parameter without one fails
//!   with "must have values";
//! - then `AllowedValues`, `MinLength`, `MaxLength` and `AllowedPattern`
//!   must all hold.
//!
//! A pre-parameter stack updated to the template has no value. A stack left
//! empty and updated with "use existing value" sends "".
//!
//! To check against AWS: create a stack from a template rendered with the
//! default options, then update it to one rendered with both options on.
//! - Without `ManagementExternalId`, and with a role ARN outside the list, the
//!   update must fail at parameter validation and leave the stack unchanged.
//! - With the issued value and a listed ARN, the change set must show only
//!   in-place `AssumeRolePolicyDocument` modifications.

use super::helpers::custom_resource_registration;
use alien_cloudformation::{
    generate_cloudformation_template, test_utils::cfn_lint, to_yaml, CfExpression, CfParameter,
    CfRegistry, CfTemplate, CloudFormationOptions, CloudFormationTarget, ManagementTrustOptions,
    RegistrationMode,
};
use alien_core::{
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, RemoteBindings, RemoteStackManagement, ResourceLifecycle, Result,
    Stack, StackSettings, Storage,
};

const EXTERNAL_ID: &str = "ManagementExternalId";
const ROLE_ARN: &str = "ManagingRoleArn";
const ISSUED: &str = "0123456789abcdef0123456789ABCDEF";
const CURRENT_ROLE: &str = "arn:aws:iam::111122223333:role/deployment-management";
const PREVIOUS_ROLE: &str = "arn:aws:iam::111122223333:role/ops.management";

fn managed_stack() -> Stack {
    Stack::new("trust".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn eks_stack() -> Stack {
    Stack::new("kubernetes".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn try_render(
    stack: &Stack,
    target: CloudFormationTarget,
    registration: RegistrationMode,
    management_trust: ManagementTrustOptions,
) -> Result<CfTemplate> {
    let registry = CfRegistry::built_in();
    generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registry,
            target,
            stack_settings: StackSettings::default(),
            setup_target: match target {
                CloudFormationTarget::Eks => "kubernetes",
                _ => "aws",
            }
            .to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration,
            description: Some("management trust".to_string()),
            management_trust,
        },
    )
}

/// Render and require `cfn-lint` to pass, which also checks that no default
/// contradicts its parameter's pattern.
fn render(
    stack: &Stack,
    target: CloudFormationTarget,
    registration: RegistrationMode,
    management_trust: ManagementTrustOptions,
) -> CfTemplate {
    let template =
        try_render(stack, target, registration, management_trust).expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    cfn_lint(&yaml).assert_ok("management trust");
    template
}

fn hardened() -> ManagementTrustOptions {
    ManagementTrustOptions {
        require_external_id: true,
        allowed_managing_role_arns: vec![CURRENT_ROLE.to_string(), PREVIOUS_ROLE.to_string()],
    }
}

/// Whether CloudFormation accepts `value` (`None` = not supplied) for the
/// parameter at stack create or update.
fn accepts(parameter: &CfParameter, value: Option<&str>) -> bool {
    let value = match (value, &parameter.default) {
        (Some(value), _) => value.to_string(),
        (None, Some(CfExpression::String(default))) => default.clone(),
        (None, Some(other)) => panic!("unexpected default {other:?}"),
        (None, None) => return false,
    };
    let length = value.chars().count() as u32;
    parameter
        .allowed_values
        .as_ref()
        .is_none_or(|values| values.contains(&CfExpression::from(value.clone())))
        && parameter.min_length.is_none_or(|min| length >= min)
        && parameter.max_length.is_none_or(|max| length <= max)
        && parameter.allowed_pattern.as_deref().is_none_or(|pattern| {
            // CloudFormation requires the whole value to match.
            regex::Regex::new(&format!("^(?:{pattern})$"))
                .expect("AllowedPattern compiles")
                .is_match(&value)
        })
}

#[test]
fn a_required_external_id_refuses_every_install_or_update_without_the_value() {
    for (stack, target) in [
        (managed_stack(), CloudFormationTarget::Aws),
        (eks_stack(), CloudFormationTarget::Eks),
    ] {
        let open = render(
            &stack,
            target,
            custom_resource_registration(),
            ManagementTrustOptions::default(),
        );
        let required = render(
            &stack,
            target,
            custom_resource_registration(),
            ManagementTrustOptions {
                require_external_id: true,
                allowed_managing_role_arns: Vec::new(),
            },
        );
        let open_parameter = &open.parameters[EXTERNAL_ID];
        let required_parameter = &required.parameters[EXTERNAL_ID];

        // Default options: a stack from before the parameter, or one left
        // empty, updates without a value and keeps its old trust.
        assert!(accepts(open_parameter, None), "{target:?}");
        assert!(accepts(open_parameter, Some("")), "{target:?}");

        for (value, case) in [
            (None, "a stack from before the parameter"),
            (
                Some(""),
                "a stack left empty, updated with its existing value",
            ),
            (Some(&ISSUED[..31]), "a value one character short"),
            (
                Some(" 0123456789abcdef0123456789ABCDE"),
                "a value with a space",
            ),
        ] {
            assert!(
                !accepts(required_parameter, value),
                "{target:?}: {case} must be refused at parameter validation"
            );
        }
        assert!(accepts(required_parameter, Some(ISSUED)), "{target:?}");
        assert_eq!(required_parameter.no_echo, Some(true));

        // Only the parameter narrows. Roles, trust policies and the
        // registration resource stay as they were, so a stack that already
        // has the value updates without a single resource change.
        assert_eq!(open.resources, required.resources, "{target:?}");
        assert_eq!(open.conditions, required.conditions, "{target:?}");
    }
}

/// Registration modes that put `ManagingRoleArn` in a custom resource
/// property and in JSON text in the stack outputs.
fn registrations() -> [RegistrationMode; 2] {
    [
        custom_resource_registration(),
        RegistrationMode::Both {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
            callback_url: None,
        },
    ]
}

#[test]
fn the_managing_role_must_be_one_of_the_listed_roles() {
    for (stack, target) in [
        (managed_stack(), CloudFormationTarget::Aws),
        (eks_stack(), CloudFormationTarget::Eks),
    ] {
        for registration in registrations() {
            let open = render(
                &stack,
                target,
                registration.clone(),
                ManagementTrustOptions::default(),
            );
            let restricted = render(&stack, target, registration.clone(), hardened());
            let parameter = &restricted.parameters[ROLE_ARN];
            let case = format!("{target:?} {registration:?}");

            assert!(accepts(parameter, Some(CURRENT_ROLE)), "{case}");
            assert!(
                accepts(parameter, Some(PREVIOUS_ROLE)),
                "{case}: stacks installed against the previous role must still update"
            );
            for candidate in [
                None,
                Some(""),
                Some("arn:aws:iam::444455556666:role/deployment-management"),
                Some("arn:aws:iam::111122223333:role/deployment-management-2"),
                Some("arn:aws:iam::111122223333:role/xdeployment-management"),
                Some("arn:aws:iam::111122223333:role/path/deployment-management"),
                Some("arn:aws:iam::111122223333:user/deployment-management"),
                Some("arn:aws:iam::111122223333:role/opsXmanagement"),
            ] {
                assert!(
                    !accepts(parameter, candidate),
                    "{case}: {candidate:?} must be refused"
                );
            }
            assert!(
                parameter
                    .constraint_description
                    .as_deref()
                    .is_some_and(|text| {
                        text.contains(CURRENT_ROLE) && text.contains(PREVIOUS_ROLE)
                    }),
                "the refusal names the roles to use: {:?}",
                parameter.constraint_description
            );

            // Without the list any role ARN is still accepted.
            assert!(accepts(
                &open.parameters[ROLE_ARN],
                Some("arn:aws:iam::444455556666:role/deployment-management")
            ));
            assert_eq!(open.resources, restricted.resources, "{case}");
            assert_eq!(open.outputs, restricted.outputs, "{case}");
        }
    }
}

#[test]
fn a_listed_value_that_is_not_an_iam_role_arn_fails_the_render() {
    for arn in [
        "",
        "*",
        "arn:aws:iam::1111:role/deployment-management",
        "arn:aws:iam::111122223333:user/deployment-management",
        "arn:aws:s3:::deployment-management",
        "arn:aws:iam::111122223333:role/",
        "arn:aws:iam::111122223333:role/deployment-management|.*",
    ] {
        let error = try_render(
            &managed_stack(),
            CloudFormationTarget::Aws,
            custom_resource_registration(),
            ManagementTrustOptions {
                require_external_id: false,
                allowed_managing_role_arns: vec![CURRENT_ROLE.to_string(), arn.to_string()],
            },
        )
        .expect_err("an invalid role ARN must fail the render");
        assert_eq!(error.code, "MANAGING_ROLE_ARN_INVALID", "{arn:?}");
    }
}
