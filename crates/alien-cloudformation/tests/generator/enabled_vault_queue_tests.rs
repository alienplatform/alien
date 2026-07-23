//! `.enabled(input)` for `Queue` and `Vault` on CloudFormation.
//!
//! `enabled_tests.rs` covers the mechanics on `Kv`. `Vault` adds the shape `Kv`
//! never had: it owns no resource of its own — Parameter Store is a name prefix
//! — so the only things the emitter returns are the IAM policies granting
//! access to that prefix. Those must follow the gate too, or declining the vault
//! still hands out the permissions it was declined for.

use super::helpers::{
    custom_resource_registration, gate_input, render_built_ins, render_built_ins_template, resolve,
    Declined,
};
use alien_cloudformation::{
    to_yaml, CfExpression, CfTemplate, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack, StackBuilder,
    StackInputDefinition, StackSettings, Vault,
};
use std::collections::HashMap;

fn render_as(stack: &Stack, registration: RegistrationMode, description: &str) -> CfTemplate {
    render_built_ins_template(
        stack,
        StackSettings::default(),
        registration,
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
    .0
}

fn render(stack: &Stack, description: &str) -> CfTemplate {
    render_as(stack, custom_resource_registration(), description)
}

const QUEUE_CONDITION: &str = "InputQueueEnabledIsTrue";
const VAULT_CONDITION: &str = "InputVaultEnabledIsTrue";

/// A vault id a customer could actually gate. `secrets` is the reserved
/// deployment secrets vault, auto-wired to workers after validation, and the
/// preflight refuses to gate it.
const VAULT_ID: &str = "app-tokens";

/// The Parameter Store namespace the vault's grants are written against, exactly
/// as it appears inside the rendered `Fn::Sub`.
const VAULT_NAMESPACE: &str = "${AWS::StackName}-app-tokens-";

fn gate_inputs() -> Vec<StackInputDefinition> {
    vec![
        gate_input("queueEnabled", "queue", "Whether to create the queue."),
        gate_input("vaultEnabled", "vault", "Whether to create the vault."),
    ]
}

/// Without a permission profile the vault emitter returns no resources at all,
/// so a missing `Condition` on its IAM policies would never show up. The service
/// account also gives the "off" answer an ungated neighbour to leave behind.
fn base() -> StackBuilder {
    Stack::new("gated-stack".to_string())
        .inputs(gate_inputs())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource(VAULT_ID, ["vault/data-read"])
                .resource("jobs", ["queue/data-write"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn stack(gated: bool) -> Stack {
    let queue = Queue::new("jobs".to_string()).build();
    let vault = Vault::new(VAULT_ID.to_string()).build();
    let builder = base();
    if gated {
        builder
            .add_enabled_when(queue, ResourceLifecycle::Frozen, "queueEnabled")
            .add_enabled_when(vault, ResourceLifecycle::Frozen, "vaultEnabled")
            .build()
    } else {
        builder
            .add(queue, ResourceLifecycle::Frozen)
            .add(vault, ResourceLifecycle::Frozen)
            .build()
    }
}

fn registration_entry_ids(template: &CfTemplate, conditions: &HashMap<&str, bool>) -> Vec<String> {
    let payload = template
        .resources
        .get("DeploymentRegistration")
        .expect("registration custom resource")
        .properties
        .get("Resources")
        .expect("registration resource list")
        .clone();
    let CfExpression::List(entries) = resolve(&payload, conditions, Declined::Removed)
        .expect("payload survives condition resolution")
    else {
        panic!("registration payload should resolve to a list");
    };
    entries
        .iter()
        .map(|entry| {
            let CfExpression::Object(fields) = entry else {
                panic!("registration entry should be an object: {entry:?}");
            };
            match fields.get("id").expect("registration entry id") {
                CfExpression::String(id) => id.clone(),
                other => panic!("registration entry id should be a string: {other:?}"),
            }
        })
        .collect()
}

/// The `DeploymentResources` output, resolved and parsed the way the registering
/// consumer reads it.
///
/// Returns the raw text alongside the parsed value so a caller can assert on the
/// exact bytes CloudFormation would hand over, not just on what survived
/// `serde_json`.
fn resolved_outputs_payload(
    template: &CfTemplate,
    conditions: &HashMap<&str, bool>,
) -> (String, serde_json::Value) {
    let value = &template
        .outputs
        .get("DeploymentResources")
        .expect("outputs fallback should carry the resources payload")
        .value;
    let resolved = resolve(value, conditions, Declined::Removed)
        .expect("the resources output survives condition resolution");
    let CfExpression::String(text) = resolved else {
        panic!("the resources output should resolve to JSON text: {resolved:?}");
    };
    let parsed = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("resources output should be valid JSON: {error}\n{text}"));
    (text, parsed)
}

/// Extract ids from the Outputs payload, verifying that every entry is a
/// well-formed object.
fn outputs_entry_ids(template: &CfTemplate, conditions: &HashMap<&str, bool>) -> Vec<String> {
    let (text, parsed) = resolved_outputs_payload(template, conditions);
    let serde_json::Value::Array(entries) = parsed else {
        panic!("resources output should be a JSON array: {text}");
    };
    entries
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .unwrap_or_else(|| panic!("registration entry has no id: {entry}\n{text}"));
            id.as_str()
                .unwrap_or_else(|| panic!("registration entry id should be a string: {id}"))
                .to_string()
        })
        .collect()
}

/// Logical ids of every resource carrying `condition`.
fn resources_conditioned_on(template: &CfTemplate, condition: &str) -> Vec<String> {
    let mut ids = template
        .resources
        .iter()
        .filter(|(_, resource)| resource.condition.as_deref() == Some(condition))
        .map(|(logical_id, _)| logical_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[test]
fn a_gated_queue_is_created_only_when_the_deployer_says_yes() {
    let template = render(&stack(true), "gated queue and vault");

    let queue = template
        .resources
        .get("Jobs")
        .expect("gated queue should still be declared");
    assert_eq!(queue.resource_type, "AWS::SQS::Queue");
    assert_eq!(queue.condition.as_deref(), Some(QUEUE_CONDITION));
    assert_eq!(
        resources_conditioned_on(&template, QUEUE_CONDITION),
        vec![
            "Jobs".to_string(),
            "JobsExecutionSaRoleQueuePermission00".to_string(),
        ],
        "the queue owns the SQS queue and its resource-scoped grant, both gated"
    );
}

/// The vault is a name prefix, so its IAM policies are the only thing it
/// creates. A policy left ungated grants access to the declined vault's
/// namespace, which is the whole point of declining it.
///
/// Scoped to what the vault's own emitter produced: the negative check below
/// only reads standalone `AWS::IAM::Policy` resources by `PolicyName`, so a
/// grant emitted by some *other* emitter — an inline policy on the service
/// account's role, say — would pass straight through it. That blind spot is
/// what `declining_the_vault_withdraws_every_grant_over_its_namespace` covers.
#[test]
fn a_gated_vault_gates_the_iam_policies_that_are_its_only_resources() {
    let template = render(&stack(true), "gated queue and vault");

    let gated = resources_conditioned_on(&template, VAULT_CONDITION);
    assert!(
        !gated.is_empty(),
        "the fixture must actually render vault IAM, or this test proves nothing"
    );
    for logical_id in &gated {
        assert_eq!(
            template.resources[logical_id].resource_type, "AWS::IAM::Policy",
            "the vault creates nothing but IAM policies"
        );
    }
    assert!(
        !template
            .resources
            .values()
            .any(|resource| resource.resource_type == "AWS::IAM::Policy"
                && resource.condition.is_none()
                && resource
                    .properties
                    .get("PolicyName")
                    .map(|name| format!("{name:?}").contains(VAULT_ID))
                    .unwrap_or(false)),
        "no vault policy may survive the gate being off"
    );
}

/// Registration runs the typed importer over every entry it receives, so a
/// declined resource has to be absent rather than present-and-null.
#[test]
fn declined_resources_leave_no_registration_entry() {
    let template = render(&stack(true), "gated queue and vault");

    let all_on = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, true)]);
    assert_eq!(
        registration_entry_ids(&template, &all_on),
        vec![
            "execution-sa".to_string(),
            "jobs".to_string(),
            VAULT_ID.to_string()
        ],
        "everything registers when the deployer says yes"
    );

    let all_off = HashMap::from([(QUEUE_CONDITION, false), (VAULT_CONDITION, false)]);
    assert_eq!(
        registration_entry_ids(&template, &all_off),
        vec!["execution-sa".to_string()],
        "declined resources must be absent from the payload, not null"
    );

    let queue_only = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, false)]);
    assert_eq!(
        registration_entry_ids(&template, &queue_only),
        vec!["execution-sa".to_string(), "jobs".to_string()],
        "the two gates are independent"
    );
}

/// The same invariant on the Outputs fallback, which is the path `alien render`
/// picks whenever no notification lambda is configured.
///
/// It carries the payload as JSON text rather than as a list, and the intrinsic
/// that produces that text renders a declined entry as a literal `null` instead
/// of dropping it. Nothing about that is visible in the rendered template — it
/// only happens once CloudFormation resolves the conditions — so this asserts on
/// the resolved text.
#[test]
fn the_outputs_fallback_omits_declined_resources() {
    let template = render_as(
        &stack(true),
        RegistrationMode::OutputsFallback,
        "gated queue and vault, outputs fallback",
    );

    let all_on = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, true)]);
    assert_eq!(
        outputs_entry_ids(&template, &all_on),
        vec![
            "execution-sa".to_string(),
            "jobs".to_string(),
            VAULT_ID.to_string()
        ],
        "everything registers when the deployer says yes"
    );

    let all_off = HashMap::from([(QUEUE_CONDITION, false), (VAULT_CONDITION, false)]);
    let (text, _) = resolved_outputs_payload(&template, &all_off);
    assert!(
        !text.contains("null"),
        "a declined resource must leave nothing behind, but the payload still \
         carries a null: {text}"
    );
    assert_eq!(
        outputs_entry_ids(&template, &all_off),
        vec!["execution-sa".to_string()],
        "only the ungated resource registers"
    );

    let queue_only = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, false)]);
    assert_eq!(
        outputs_entry_ids(&template, &queue_only),
        vec!["execution-sa".to_string(), "jobs".to_string()],
        "the two gates are independent"
    );
}

/// Declining every gated resource must still leave a payload the consumer can
/// read, rather than a malformed array or a stray delimiter.
#[test]
fn the_outputs_fallback_survives_every_resource_being_declined() {
    let template = render_as(
        &Stack::new("gated-stack".to_string())
            .inputs(gate_inputs())
            .add_enabled_when(
                Queue::new("jobs".to_string()).build(),
                ResourceLifecycle::Frozen,
                "queueEnabled",
            )
            .build(),
        RegistrationMode::OutputsFallback,
        "every resource gated, outputs fallback",
    );

    let all_off = HashMap::from([(QUEUE_CONDITION, false)]);
    let (text, parsed) = resolved_outputs_payload(&template, &all_off);
    assert_eq!(text, "[]", "an all-declined payload is an empty array");
    assert_eq!(parsed, serde_json::json!([]));
}

/// Ungated stacks gain no conditions: opt-in means no `.enabled(...)`, so no gating.
#[test]
fn an_ungated_stack_gains_no_conditions() {
    let template = render(&stack(false), "ungated queue and vault");

    assert!(
        template.conditions.is_empty(),
        "nothing is gated, so no condition belongs in the template: {:?}",
        template.conditions
    );
    assert!(
        template
            .resources
            .values()
            .all(|resource| resource.condition.is_none()),
        "no resource may carry a condition when nothing is gated"
    );
    assert_eq!(
        registration_entry_ids(&template, &HashMap::new()),
        vec![
            "execution-sa".to_string(),
            "jobs".to_string(),
            VAULT_ID.to_string()
        ],
    );
}

/// The gated template stays a template a security team can read end to end.
#[test]
fn the_gated_template_snapshot_stays_reviewable() {
    let yaml = render_built_ins(
        &stack(true),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "gated queue and vault",
    );
    insta::assert_snapshot!("enabled_gated_queue_and_vault", yaml);
}

// ------------------------------------------- the grant, whoever emitted it

/// The vault grant scoped to the resource, which is the shape `.link()` authors.
/// A `"*"`-scoped grant is deliberately excluded: it lands on the role through
/// `stack_permission_sets` and is stack-wide by design, which is why
/// `ResourceEnabledValidCheck` rejects one for a gated resource type outright.
fn resource_scoped_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(gate_inputs())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource(VAULT_ID, ["vault/data-read"])
                .resource("jobs", ["queue/data-write"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
            "queueEnabled",
        )
        .add_enabled_when(
            Vault::new(VAULT_ID.to_string()).build(),
            ResourceLifecycle::Frozen,
            "vaultEnabled",
        )
        .build()
}

/// Logical ids of every IAM resource that still grants over `namespace` once
/// CloudFormation has resolved `conditions` — a resource whose `Condition` is
/// false is gone, and `Fn::If` inside a surviving one picks its branch.
///
/// Deliberately blind to *which* emitter produced the grant, and to how it was
/// gated. Anything under `AWS::IAM::` is inspected whole, so an inline policy on
/// the service account's role counts exactly as much as the vault's own
/// standalone policy.
fn iam_grants_over(
    template: &CfTemplate,
    namespace: &str,
    conditions: &HashMap<&str, bool>,
) -> Vec<String> {
    let mut ids = template
        .resources
        .iter()
        .filter(|(_, resource)| match resource.condition.as_deref() {
            Some(condition) => *conditions
                .get(condition)
                .unwrap_or_else(|| panic!("no answer supplied for condition '{condition}'")),
            None => true,
        })
        .filter(|(_, resource)| resource.resource_type.starts_with("AWS::IAM::"))
        .filter(|(_, resource)| {
            resource
                .properties
                .iter()
                .filter_map(|(_, value)| resolve(value, conditions, Declined::Removed))
                .any(|value| format!("{value:?}").contains(namespace))
        })
        .map(|(logical_id, _)| logical_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// The property, not the mechanism: a deployer who declines the vault must be
/// left with no IAM at all over its namespace.
///
/// A "the vault's own policy carries the Condition" check misses this: two
/// emitters render the same grant and dedupe by body identity, so gating one
/// breaks the dedupe and leaves an ungated twin. This resolves the template with
/// the gate off and asks what access is still standing.
#[test]
fn declining_the_vault_withdraws_every_grant_over_its_namespace() {
    let template = render(&resource_scoped_stack(), "resource-scoped gated vault");

    let vault_on = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, true)]);
    assert!(
        !iam_grants_over(&template, VAULT_NAMESPACE, &vault_on).is_empty(),
        "the fixture must actually render a grant over `{VAULT_NAMESPACE}` when the \
         deployer says yes, or the assertion below proves nothing"
    );

    let vault_off = HashMap::from([(QUEUE_CONDITION, true), (VAULT_CONDITION, false)]);
    let leaked = iam_grants_over(&template, VAULT_NAMESPACE, &vault_off);
    assert!(
        leaked.is_empty(),
        "these IAM resources still grant over `{VAULT_NAMESPACE}` after the vault was \
         declined, so saying no would not withdraw the access: {leaked:?}\n{}",
        to_yaml(&template).expect("template should serialize")
    );
}

/// The service account's role is the one thing that outlives every gate, so it is
/// where an ungated grant would have to hide. It carries only what the profile
/// puts at the `"*"` scope, and this stack puts nothing there.
#[test]
fn the_service_account_role_carries_no_resource_scoped_grant() {
    let template = render(&resource_scoped_stack(), "resource-scoped gated vault");

    let role = template
        .resources
        .get("ExecutionSaRole")
        .expect("the service account role should be declared");
    assert_eq!(role.resource_type, "AWS::IAM::Role");
    assert!(
        role.condition.is_none(),
        "the role is ungated, which is why anything it holds outlives the vault's gate"
    );
    assert!(
        !role.properties.contains_key("Policies"),
        "a resource-scoped grant belongs to the resource's own emitter, which gates it; \
         an inline policy here would survive the vault being declined: {:?}",
        role.properties.get("Policies")
    );
}
