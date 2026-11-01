//! Every registered AWS emitter renders with every permission set that applies to its type, on
//! both AWS targets and with data resources Frozen and Live.
//!
//! The render sizes every IAM policy at the longest value of each interpolation and refuses one
//! it cannot bound, so a set or emitter that interpolates a value the size check does not know
//! fails here instead of in a deployment. The walk over [`TfRegistry::registered_keys`] keeps
//! this complete: an AWS emitter registered without a resource in the fixture fails the test.

use super::helpers::{assert_terraform_valid, prepare_for_target, render_with_registry};
use alien_core::{
    ownership_policy_for_resource_type, Ai, ArtifactRegistry, Build, Key, Kv, NetworkSettings,
    PermissionProfile, Platform, Queue, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings, Storage, Vault, Worker, WorkerCode,
    WorkerTrigger,
};
use alien_permissions::{
    get_permission_set, list_permission_set_ids, permission_set_covers_platform, BindingTarget,
    MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD,
};
use alien_terraform::{TerraformTarget, TfRegistry};
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
    &["storage"],
    &["vault"],
    &["worker"],
];

/// Every set of `family` that grants anything on AWS and can be bound at `scope`.
fn aws_sets(family: &str, scope: BindingTarget) -> Vec<String> {
    list_permission_set_ids()
        .into_iter()
        .map(|id| id.to_string())
        .filter(|id| id.split('/').next() == Some(family))
        .filter(|id| permission_set_covers_platform(id, Platform::Aws))
        // Guards preflights add for the management and setup roles, not workload grants.
        .filter(|id| ![SANDBOX_SETUP_ROLES_GUARD, MANAGEMENT_ROLE_GUARD].contains(&id.as_str()))
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
    let mut grant = |resource: &str, family: &str| {
        execution = std::mem::take(&mut execution)
            .resource(resource, aws_sets(family, BindingTarget::Resource));
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
