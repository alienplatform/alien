//! IAM size coverage of the AWS Terraform emitters and permission sets, on both AWS targets and
//! with data resources Frozen and Live.
//!
//! The render sizes every IAM policy at the longest value of each interpolation and refuses one
//! it cannot bound, so a set or emitter that interpolates a value the size check does not know
//! fails here instead of in a deployment. Each fixture is a Live worker granted every AWS set
//! of its families stack-wide, and, where this crate has an AWS emitter for the family's
//! resource type, that resource with every set the worker can be granted on it. Every rendered
//! module is `terraform validate`d.
//!
//! Two checks keep this complete:
//! - every AWS emitter in [`TfRegistry::built_in`] must render in some fixture;
//! - every permission set family with AWS permissions must have a fixture, be listed as covered
//!   by an emitter registered from outside this crate, or be listed as not grantable in a
//!   stack this crate renders.
//!
//! Families without a resource here (their type has no AWS emitter in this crate, or
//! preflights add the resource after profiles are checked) are covered only by their
//! stack-wide sets; their resource-bound grants are rendered by the emitter of that type.

use super::helpers::{assert_terraform_valid, prepare_for_target, render_with_registry};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, Ai, ArtifactRegistry, Build,
    ComputeCluster, Key, Kv, NetworkSettings, PermissionProfile, Platform, Queue,
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, ServiceAccount,
    Stack, StackSettings, Storage, Vault, Worker, WorkerCode, WorkerTrigger,
};
use alien_permissions::{
    get_permission_set, list_permission_set_ids, permission_set_covers_platform, BindingTarget,
    MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD,
};
use alien_terraform::{
    block::{attr, resource_block},
    emitters::aws::helpers::{
        aws_terraform_permission_context, emit_iam_role_policy, required_label,
        resource_prefix_template, service_assume_role_policy,
    },
    TerraformTarget, TfEmitter, TfFragment, TfRegistry,
};
use std::collections::BTreeSet;

/// The fixtures, by the permission set families they hold: one per resource type, granted
/// every set of its family, so each type is sized in the smallest stack that holds it.
const FIXTURES: &[&[&str]] = &[
    &["ai"],
    &["artifact-registry"],
    &["build"],
    &["key"],
    &["kv"],
    &["queue"],
    &["sandbox"],
    &["service-account"],
    &["storage"],
    &["vault"],
    &["worker"],
    // No resource: their resources have no AWS Terraform emitter in this crate, or are added by
    // preflights after the profiles are checked, so only their stack-wide sets are granted.
    &["container"],
    &["daemon"],
    &["email"],
    &["experimental"],
    &["kubernetes-cluster"],
    &["network"],
    &["observe"],
    &["remote-stack-management"],
];

/// Families whose AWS Terraform emitter is registered from outside this crate, by a composed
/// CLI. A stand-in registered emitter grants each of their AWS sets here
/// (`every_set_of_a_registered_emitters_family_renders`); the real emitter's own grants are
/// sized where it is tested.
const COVERED_BY_A_REGISTERED_EMITTER: &[&str] = &["compute-cluster", "postgres"];

/// Families whose every AWS set binds only to a resource of a type no AWS Terraform emitter in
/// this crate renders, so no stack rendered here can grant them.
const NOT_GRANTABLE_HERE: &[&str] = &["kubernetes-public-endpoint"];

/// Every permission set family that grants anything on AWS has a fixture here or is listed as
/// covered by a registered emitter, so a new family cannot go unsized.
#[test]
fn every_aws_permission_set_family_has_a_fixture() {
    let families: BTreeSet<String> = list_permission_set_ids()
        .into_iter()
        .filter(|id| permission_set_covers_platform(id, Platform::Aws))
        .filter(|id| ![SANDBOX_SETUP_ROLES_GUARD, MANAGEMENT_ROLE_GUARD].contains(id))
        .map(|id| {
            id.split('/')
                .next()
                .expect("a set ID has a family")
                .to_string()
        })
        .collect();
    let covered: BTreeSet<String> = FIXTURES
        .iter()
        .flat_map(|families| families.iter())
        .chain(COVERED_BY_A_REGISTERED_EMITTER)
        .chain(NOT_GRANTABLE_HERE)
        .map(|family| family.to_string())
        .collect();
    assert_eq!(
        families.difference(&covered).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "these AWS permission set families have no coverage fixture"
    );
    assert_eq!(
        covered.difference(&families).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "these fixtures name no AWS permission set family"
    );
}

/// Every set of `family` that grants anything on AWS and can be bound at `scope`.
fn aws_sets(family: &str, scope: BindingTarget) -> Vec<String> {
    list_permission_set_ids()
        .into_iter()
        .map(|id| id.to_string())
        .filter(|id| id.split('/').next() == Some(family))
        .filter(|id| permission_set_covers_platform(id, Platform::Aws))
        // Guards preflights add for the management and setup roles, not workload grants.
        .filter(|id| ![SANDBOX_SETUP_ROLES_GUARD, MANAGEMENT_ROLE_GUARD].contains(&id.as_str()))
        // Preflights allow impersonation only of a named service account.
        .filter(|id| !(scope == BindingTarget::Stack && id == "service-account/impersonate"))
        .filter(|id| {
            let set = get_permission_set(id).expect("a listed set");
            set.platforms
                .aws
                .iter()
                .flatten()
                .all(|permission| match scope {
                    BindingTarget::Stack => permission.binding.stack.is_some(),
                    BindingTarget::Resource => permission.binding.resource.is_some(),
                })
        })
        .collect()
}

/// `lifecycle` where the type's ownership policy allows it, Frozen otherwise.
fn allowed(
    resource_type: alien_core::ResourceType,
    lifecycle: ResourceLifecycle,
) -> ResourceLifecycle {
    if ownership_policy_for_resource_type(resource_type.as_ref()).allows_lifecycle(lifecycle) {
        lifecycle
    } else {
        ResourceLifecycle::Frozen
    }
}

fn network_settings() -> NetworkSettings {
    NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    }
}

/// One resource of each of `families`, data resources at `lifecycle`, and a Live worker granted
/// every set of those families, stack-wide and on each resource.
fn fixture(
    families: &[&str],
    target: TerraformTarget,
    lifecycle: ResourceLifecycle,
) -> (Stack, StackSettings) {
    let has = |family: &str| families.contains(&family);
    let mut execution = PermissionProfile::new().global(
        families
            .iter()
            .flat_map(|family| aws_sets(family, BindingTarget::Stack))
            .collect::<Vec<_>>(),
    );
    let mut stack = Stack::new("coverage".to_string());
    let mut api = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1".to_string(),
        })
        .permissions("execution".to_string());
    let mut granted_on_resources = BTreeSet::new();
    let mut grant = |resource: &str, family: &str| {
        let sets = aws_sets(family, BindingTarget::Resource);
        if !sets.is_empty() {
            granted_on_resources.insert(family.to_string());
        }
        execution = std::mem::take(&mut execution).resource(resource, sets);
    };
    if has("storage") {
        let files = Storage::new("files".to_string()).build();
        grant("files", "storage");
        api = api
            .link(&files)
            .trigger(WorkerTrigger::storage(&files, vec!["created".to_string()]));
        // Reachable from outside the deployment as well, through remote bindings.
        stack = stack.add_with_remote_access(files, allowed(Storage::RESOURCE_TYPE, lifecycle));
    }
    if has("kv") {
        let table = Kv::new("table".to_string()).build();
        grant("table", "kv");
        api = api.link(&table);
        // Reachable from outside the deployment as well, through remote bindings.
        stack = stack.add_with_remote_access(table, allowed(Kv::RESOURCE_TYPE, lifecycle));
    }
    if has("queue") {
        let jobs = Queue::new("jobs".to_string()).build();
        grant("jobs", "queue");
        api = api.link(&jobs).trigger(WorkerTrigger::queue(&jobs));
        // Reachable from outside the deployment as well, through remote bindings.
        stack = stack.add_with_remote_access(jobs, allowed(Queue::RESOURCE_TYPE, lifecycle));
    }
    if has("vault") {
        let passwords = Vault::new("passwords".to_string()).build();
        grant("passwords", "vault");
        api = api.link(&passwords);
        stack = stack.add(passwords, allowed(Vault::RESOURCE_TYPE, lifecycle));
    }
    if has("key") {
        let key = Key::new("key".to_string()).build();
        grant("key", "key");
        api = api.link(&key);
        stack = stack.add(key, allowed(Key::RESOURCE_TYPE, lifecycle));
    }
    if has("ai") {
        let model = Ai::new("model".to_string()).build();
        grant("model", "ai");
        api = api.link(&model);
        stack = stack.add(model, allowed(Ai::RESOURCE_TYPE, lifecycle));
    }
    if has("artifact-registry") {
        let images = ArtifactRegistry::new("images".to_string()).build();
        grant("images", "artifact-registry");
        api = api.link(&images);
        stack = stack.add(images, allowed(ArtifactRegistry::RESOURCE_TYPE, lifecycle));
    }
    if has("sandbox") {
        grant("agents", "sandbox");
        stack = stack.add(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "s3://coverage-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            // Only AWS itself provisions a sandbox at runtime.
            if target == TerraformTarget::Eks {
                ResourceLifecycle::Frozen
            } else {
                allowed(Sandbox::RESOURCE_TYPE, lifecycle)
            },
        );
    }
    if has("service-account") {
        grant("reader", "service-account");
        stack = stack.add(
            ServiceAccount::new("reader".to_string()).build(),
            allowed(ServiceAccount::RESOURCE_TYPE, lifecycle),
        );
    }
    if has("build") {
        grant("builder", "build");
        stack = stack.add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .build(),
            allowed(Build::RESOURCE_TYPE, lifecycle),
        );
    }
    if has("worker") {
        grant("api", "worker");
    }
    for family in families {
        assert!(
            !aws_sets(family, BindingTarget::Stack).is_empty()
                || granted_on_resources.contains(*family),
            "the {family} fixture grants none of its sets"
        );
    }
    let stack = stack
        .permission("execution", execution)
        .add(api.build(), ResourceLifecycle::Live)
        .build();
    let settings = StackSettings {
        network: Some(network_settings()),
        ..StackSettings::default()
    };
    (stack, settings)
}

#[tokio::test]
async fn every_aws_emitter_renders_every_permission_set_it_supports() {
    let registry = TfRegistry::built_in();
    let mut rendered_types = BTreeSet::new();
    let mut modules = Vec::new();
    for families in FIXTURES {
        for target in [TerraformTarget::Aws, TerraformTarget::Eks] {
            for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
                let case = format!("{families:?} on {target:?} with {lifecycle:?} data resources");
                let (stack, settings) = fixture(families, target, lifecycle);
                let stack = prepare_for_target(stack, target).await;
                rendered_types.extend(
                    stack
                        .resources()
                        .map(|(_, entry)| entry.config.resource_type().to_string()),
                );
                let module = render_with_registry(&stack, target, settings, &registry)
                    .unwrap_or_else(|error| panic!("{case}: the module renders: {error:?}"));
                modules.push((case, module));
            }
        }
    }
    let missing: Vec<&str> = registry
        .registered_keys()
        .filter(|(_, platform)| *platform == Platform::Aws)
        .map(|(resource_type, _)| resource_type)
        .filter(|resource_type| !rendered_types.contains(*resource_type))
        .collect();
    assert!(
        missing.is_empty(),
        "these AWS emitters have no resource in the coverage fixture; add one: {missing:?}"
    );
    std::thread::scope(|scope| {
        for (case, module) in &modules {
            scope.spawn(move || assert_terraform_valid(module, case));
        }
    });
}

/// Stands in for an emitter registered from outside this crate: a role of its own granted, at
/// stack scope, every AWS set of the families such emitters render.
struct RegisteredFamilyGrants;

impl TfEmitter for RegisteredFamilyGrants {
    fn emit(&self, ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        let role = format!("{}_registered", required_label(ctx)?);
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            &role,
            [
                attr("name", resource_prefix_template("registered")),
                attr(
                    "assume_role_policy",
                    service_assume_role_policy(&["ec2.amazonaws.com"]),
                ),
            ],
        ));
        let sets = COVERED_BY_A_REGISTERED_EMITTER
            .iter()
            .flat_map(|family| aws_sets(family, BindingTarget::Stack));
        for (index, id) in sets.enumerate() {
            let set = get_permission_set(&id).expect("a listed set");
            emit_iam_role_policy(
                &mut fragment,
                &role,
                set,
                index,
                &aws_terraform_permission_context(),
            )?;
        }
        Ok(fragment)
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Object(hcl::Object::new()))
    }
}

/// Every AWS set of a family whose emitter is registered from outside this crate renders, is
/// sized, and validates, granted by a registered emitter to its own role.
#[tokio::test]
async fn every_set_of_a_registered_emitters_family_renders() {
    let mut registry = TfRegistry::built_in();
    registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        RegisteredFamilyGrants,
    );
    let granted: usize = COVERED_BY_A_REGISTERED_EMITTER
        .iter()
        .map(|family| aws_sets(family, BindingTarget::Stack).len())
        .sum();
    assert!(granted > 0, "the stand-in grants nothing");
    let stack = Stack::new("registered".to_string())
        .add(
            ComputeCluster::new("compute".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let stack = prepare_for_target(stack, TerraformTarget::Aws).await;
    let module = render_with_registry(
        &stack,
        TerraformTarget::Aws,
        StackSettings {
            network: Some(network_settings()),
            ..StackSettings::default()
        },
        &registry,
    )
    .unwrap_or_else(|error| panic!("the module renders: {error:?}"));
    let stubs = module
        .iter()
        .filter(|(name, _)| name.ends_with(".tf"))
        .map(|(_, contents)| {
            contents
                .matches("resource \"aws_iam_role_policy\" \"compute_registered_")
                .count()
        })
        .sum::<usize>();
    assert_eq!(stubs, granted, "every set is granted once");
    assert_terraform_valid(&module, "registered emitter families");
}
