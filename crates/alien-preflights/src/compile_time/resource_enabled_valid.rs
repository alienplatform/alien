//! Validates `resource.enabled(input)` before anything renders.
//!
//! Most rules here exist because breaking them makes the resource get created
//! anyway, which is the one outcome the feature must never produce. The setup
//! emitters render a frozen gate as `count = var.input_x ? 1 : 0` (Terraform)
//! or an `Fn::Equals` condition (CloudFormation), and the runner resolves a
//! live gate from the deployment's input values, so the input has to be a
//! deployer variable that exists on the target and always holds a real
//! boolean.
//!
//! Two rules cover what a gate cannot reach on its own: a `"*"`-scoped grant is
//! read straight off the profile and keeps its access after the resource is gone,
//! and a dependent of a gated resource looks up outputs that will not be there.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, StackInputKind, StackInputProvider};
use std::collections::HashMap;

/// Rejects `.enabled()` uses that could not actually keep the resource out.
pub struct ResourceEnabledValidCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ResourceEnabledValidCheck {
    fn description(&self) -> &'static str {
        "Resources enabled by a stack input must be gated on a deployer-supplied boolean"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack
            .resources()
            .any(|(_, entry)| entry.enabled_when.is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, entry) in stack.resources() {
            let Some(input_id) = entry.enabled_when.as_deref() else {
                continue;
            };

            let resource_type = entry.config.resource_type();

            // The type/id rules live in `alien_core::gateability` so the setup
            // generators enforce the same refusals for callers that render
            // without preflights. Only the reserved-vault refusal falls
            // through: `dependents_of_gated_resources` below reads a stack
            // where the vault's auto-wired links do not exist yet, and the
            // input rules further down still apply to it.
            if let Some(refusal) =
                alien_core::gate_refusal(resource_type.as_ref(), resource_id.as_str())
            {
                errors.push(format!(
                    "Resource '{resource_id}' of type '{resource_type}' is enabled by input \
                     '{input_id}', but {}",
                    refusal.reason()
                ));
                if refusal != alien_core::GateRefusal::ReservedSecretsVault {
                    continue;
                }
            }

            // `ServiceAccount::from_permission_profile` builds the runtime role from the
            // profile's "*" key alone. It never sees the resource list, so gating the
            // resource cannot take a wildcard grant back off the role.
            // Grant ids use the permission namespace, which is not always the
            // raw resource type; a raw-type prefix would let a '*' grant for a
            // remapped type slip past this net.
            let permission_set_prefix = format!(
                "{}/",
                crate::mutations::management_permission_profile::permission_resource_type(
                    resource_type.as_ref(),
                )
            );
            let named_profiles = stack
                .permissions
                .profiles
                .iter()
                .map(|(name, profile)| (name.as_str(), profile));
            // The management profile grants the same way and its role outlives
            // any single resource, so it is swept by the same rule.
            let management_profile = stack
                .management()
                .profile()
                .map(|profile| ("management", profile));
            for (profile_name, profile) in named_profiles.chain(management_profile) {
                let Some(wildcard_grants) = profile.0.get("*") else {
                    continue;
                };

                for grant in wildcard_grants {
                    if !grant.id().starts_with(&permission_set_prefix) {
                        continue;
                    }

                    errors.push(format!(
                        "Profile '{profile_name}' grants '{}' at the '*' scope while resource \
                         '{resource_id}' is enabled by input '{input_id}'. A '*' grant is read \
                         off the profile alone, so it stays on the runtime role after a deployer \
                         says no and leaves the access without the resource. Remove the '*' grant \
                         and .link() '{resource_id}' from the compute resource instead, which \
                         scopes the grant to that resource so it follows the gate",
                        grant.id()
                    ));
                }
            }

            let Some(input) = stack.inputs.iter().find(|input| input.id == input_id) else {
                errors.push(format!(
                    "Resource '{resource_id}' is enabled by input '{input_id}', which the stack \
                     does not declare"
                ));
                continue;
            };

            if input.kind != StackInputKind::Boolean {
                errors.push(format!(
                    "Resource '{resource_id}' is enabled by input '{input_id}', which is {:?}. \
                     Only a boolean can decide whether a resource exists",
                    input.kind
                ));
            }

            if !input.provided_by.contains(&StackInputProvider::Deployer) {
                errors.push(format!(
                    "Input '{input_id}' enables resource '{resource_id}' but is not \
                     deployer-provided, so it never reaches the setup template and the resource \
                     would be created whatever the deployer chose"
                ));
            }

            if !input.required && input.default.is_none() {
                errors.push(format!(
                    "Input '{input_id}' enables resource '{resource_id}', so it must be required \
                     or declare a default. An optional input with neither renders as null, and \
                     whether the resource exists would be undecided"
                ));
            }

            if let Some(platforms) = input.platforms.as_ref() {
                if !platforms.contains(&platform) {
                    errors.push(format!(
                        "Input '{input_id}' enables resource '{resource_id}' but is scoped to \
                         {platforms:?}, so it is absent from the {platform:?} setup template \
                         while the resource is still in it"
                    ));
                }
            }
        }

        let (dependent_errors, warnings) = dependents_of_gated_resources(stack);
        errors.extend(dependent_errors);

        // Severity is per finding: a scrubbable link only warns, but every other rule here
        // still fails, including the `"*"`-grant refusal above, which is a security rule.
        match (errors.is_empty(), warnings.is_empty()) {
            (true, true) => Ok(CheckResult::success()),
            (true, false) => Ok(CheckResult::with_warnings(warnings)),
            (false, _) => Ok(CheckResult::failed_with_warnings(errors, warnings)),
        }
    }
}

/// Sorts dependents of gated resources into refusals and warnings.
///
/// A pure link survives the gate being declined: `remove_declined` drops it along with the
/// resource, so the dependent keeps its own lifecycle and simply starts without that
/// `ALIEN_<ID>_BINDING`. Every other edge still refuses, because nothing removes it — a
/// trigger's wiring lives on the source resource, an ordering edge is not a binding, and a
/// resource type that does not report its links cannot have them scrubbed.
fn dependents_of_gated_resources(stack: &Stack) -> (Vec<String>, Vec<String>) {
    let gates: HashMap<&str, &str> = stack
        .resources()
        .filter_map(|(id, entry)| Some((id.as_str(), entry.enabled_when.as_deref()?)))
        .collect();

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for (dependent_id, entry) in stack.resources() {
        // `ResourceEntry` documents the total as `config.get_dependencies()` plus its own
        // list, and each compute type folds its links and triggers into the former.
        let config_dependencies = entry.config.get_dependencies();
        let links = alien_core::links_of(&entry.config);
        let mut seen: Vec<&str> = Vec::new();

        for dependency in config_dependencies.iter().chain(&entry.dependencies) {
            let dependency_id = dependency.id();
            let Some(dependency_gate) = gates.get(dependency_id) else {
                continue;
            };
            if seen.contains(&dependency_id) {
                continue;
            }
            seen.push(dependency_id);

            // Sharing the gate is already correct: the two rise and fall together.
            if entry.enabled_when.as_deref() == Some(*dependency_gate) {
                continue;
            }

            // Counting rather than testing membership: a resource that is both linked and
            // triggered appears twice, and the trigger is the half the scrub cannot remove.
            let link_count = links.iter().filter(|l| l.id() == dependency_id).count();
            let total = config_dependencies
                .iter()
                .chain(&entry.dependencies)
                .filter(|d| d.id() == dependency_id)
                .count();

            if link_count > 0 && total == link_count {
                warnings.push(format!(
                    "Resource '{dependent_id}' links '{dependency_id}', which is enabled by input \
                     '{dependency_gate}'. A deployer who says no drops the link too, so \
                     '{dependent_id}' starts without ALIEN_{}_BINDING. Make sure it runs without it",
                    dependency_id.to_uppercase().replace('-', "_")
                ));
                continue;
            }

            match entry.enabled_when.as_deref() {
                Some(gate) => errors.push(format!(
                    "Resource '{dependent_id}' depends on '{dependency_id}', but the two are \
                     gated on different inputs: '{gate}' and '{dependency_gate}'. Nothing makes a \
                     deployer answer both the same way, so '{dependent_id}' can be created while \
                     '{dependency_id}' is not. Gate both on '{dependency_gate}'"
                )),
                None => errors.push(format!(
                    "Resource '{dependent_id}' depends on '{dependency_id}', which is enabled by \
                     input '{dependency_gate}'. The dependency is not a plain link, so declining \
                     it would leave '{dependent_id}' pointing at a resource that was never \
                     created. Gate '{dependent_id}' on '{dependency_gate}' too"
                )),
            }
        }
    }

    (errors, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutations::secrets_vault::SECRETS_VAULT_ID;
    use alien_core::{
        permissions::PermissionProfile, Kv, ResourceLifecycle, StackInputDefinition, Storage,
        Vault, Worker, WorkerCode,
    };

    fn boolean_input() -> StackInputDefinition {
        StackInputDefinition::deployer_boolean(
            "storeEnabled",
            "Enable the store",
            "Whether to create the key-value store.",
            Some(true),
        )
    }

    fn stack_with(input: StackInputDefinition) -> Stack {
        Stack::new("test-stack".to_string())
            .inputs(vec![input])
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build()
    }

    async fn errors_for(stack: Stack) -> Vec<String> {
        ResourceEnabledValidCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("check should run")
            .errors
    }

    async fn result_for(stack: Stack) -> CheckResult {
        ResourceEnabledValidCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("check should run")
    }

    /// An ungated worker linking a gated store, which is the shape the scrub exists for.
    fn stack_with_worker_linking_gated_store(worker_gate: Option<&str>) -> Stack {
        let store = Kv::new("store".to_string()).build();
        let worker = Worker::new("api".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .link(&store)
            .build();

        let mut worker_input = boolean_input();
        worker_input.id = "workerEnabled".to_string();
        let builder = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input(), worker_input])
            .add_enabled_when(store, ResourceLifecycle::Live, "storeEnabled");

        match worker_gate {
            Some(gate) => builder.add_enabled_when(worker, ResourceLifecycle::Live, gate),
            None => builder.add(worker, ResourceLifecycle::Live),
        }
        .build()
    }

    /// The link is dropped with the resource, so the worker outliving it is legitimate —
    /// but the author still has to handle the binding being absent.
    #[tokio::test]
    async fn warns_rather_than_rejects_an_ungated_worker_linking_a_gated_store() {
        let result = result_for(stack_with_worker_linking_gated_store(None)).await;

        assert!(result.success, "should not block: {:?}", result.errors);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
        assert!(
            result.warnings[0].contains("Resource 'api' links 'store'"),
            "{:?}",
            result.warnings
        );
        assert!(
            result.warnings[0].contains("ALIEN_STORE_BINDING"),
            "{:?}",
            result.warnings
        );
    }

    /// Two independent answers still produce a worker that can outlive its store, but the
    /// link is scrubbed either way, so this is the same legitimate shape.
    #[tokio::test]
    async fn warns_for_a_linking_worker_gated_on_a_different_input() {
        let result = result_for(stack_with_worker_linking_gated_store(Some("workerEnabled"))).await;

        assert!(result.success, "should not block: {:?}", result.errors);
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    }

    /// Sharing the gate means they rise and fall together, so there is nothing to say.
    #[tokio::test]
    async fn stays_silent_when_the_linking_worker_shares_the_gate() {
        let result = result_for(stack_with_worker_linking_gated_store(Some("storeEnabled"))).await;

        assert!(result.success, "{:?}", result.errors);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    /// Build is not a compute kind but owns author-declared links that produce the same
    /// bindings, so it gets the same treatment. Before the links capability existed this
    /// was refused outright.
    #[tokio::test]
    async fn warns_for_a_build_linking_a_gated_store() {
        let store = Kv::new("store".to_string()).build();
        let builder = alien_core::Build::new("packager".to_string())
            .permissions("build".to_string())
            .link(&store)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(store, ResourceLifecycle::Frozen, "storeEnabled")
            .add(builder, ResourceLifecycle::Frozen)
            .build();

        let result = result_for(stack).await;

        assert!(result.success, "should not block: {:?}", result.errors);
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
        assert!(
            result.warnings[0].contains("Resource 'packager' links 'store'"),
            "{:?}",
            result.warnings
        );
    }

    /// The scrub only removes links. A queue reached by both a link and a trigger keeps the
    /// trigger wiring on the source resource, so this must stay a refusal.
    #[tokio::test]
    async fn rejects_a_queue_that_is_both_linked_and_triggered() {
        let queue = alien_core::Queue::new("jobs".to_string()).build();
        let queue_ref = alien_core::ResourceRef {
            resource_type: alien_core::Queue::RESOURCE_TYPE.clone(),
            id: "jobs".to_string(),
        };
        let worker = Worker::new("consumer".to_string())
            .permissions("consumer".to_string())
            .code(WorkerCode::Image {
                image: "example.com/consumer:latest".to_string(),
            })
            .link(&queue)
            .trigger(alien_core::WorkerTrigger::Queue { queue: queue_ref })
            .build();

        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(queue, ResourceLifecycle::Live, "storeEnabled")
            .add(worker, ResourceLifecycle::Live)
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("not a plain link"), "{errors:?}");
    }

    #[tokio::test]
    async fn rejects_a_framework_derived_resource() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(
                alien_core::ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("cannot be optional"), "{errors:?}");
    }

    #[tokio::test]
    async fn accepts_a_setup_created_resource_on_a_deployer_boolean() {
        let stack = stack_with(boolean_input());
        assert!(errors_for(stack).await.is_empty());
    }

    /// The runner follows a live gate by input value, so a live data resource
    /// is as gateable as a frozen one.
    #[tokio::test]
    async fn accepts_a_gated_live_data_resource() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Live,
                "storeEnabled",
            )
            .build();
        assert!(errors_for(stack).await.is_empty());
    }

    /// Compute gates as an existence choice: declining a live workload rides
    /// the same removal path as deleting it from a release, so the gate
    /// passes the same rules as any live data resource.
    #[tokio::test]
    async fn accepts_a_gated_compute_resource() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(
                Worker::new("proxy".to_string())
                    .permissions("proxy".to_string())
                    .code(WorkerCode::Image {
                        image: "example.com/proxy:latest".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
                "storeEnabled",
            )
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// Pausing the sole consumer is the point of gating compute: the worker
    /// is the dependent of the ungated queue, so nothing dangles — producers
    /// keep enqueuing and the queue's retention policy governs the backlog
    /// until the deployer accepts the worker again.
    #[tokio::test]
    async fn accepts_a_gated_worker_consuming_an_ungated_queue() {
        let queue = alien_core::Queue::new("jobs".to_string()).build();
        let worker = Worker::new("consumer".to_string())
            .permissions("consumer".to_string())
            .code(WorkerCode::Image {
                image: "example.com/consumer:latest".to_string(),
            })
            .trigger(alien_core::WorkerTrigger::Queue {
                queue: alien_core::ResourceRef {
                    resource_type: alien_core::Queue::RESOURCE_TYPE.clone(),
                    id: "jobs".to_string(),
                },
            })
            .build();

        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add(queue, ResourceLifecycle::Frozen)
            .add_enabled_when(worker, ResourceLifecycle::Live, "storeEnabled")
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// The reverse stays closed: an ungated worker consuming a gated queue
    /// would resolve a binding for a queue that may never exist.
    #[tokio::test]
    async fn rejects_an_ungated_worker_consuming_a_gated_queue() {
        let queue = alien_core::Queue::new("jobs".to_string()).build();
        let worker = Worker::new("consumer".to_string())
            .permissions("consumer".to_string())
            .code(WorkerCode::Image {
                image: "example.com/consumer:latest".to_string(),
            })
            .trigger(alien_core::WorkerTrigger::Queue {
                queue: alien_core::ResourceRef {
                    resource_type: alien_core::Queue::RESOURCE_TYPE.clone(),
                    id: "jobs".to_string(),
                },
            })
            .build();

        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .add_enabled_when(queue, ResourceLifecycle::Frozen, "storeEnabled")
            .add(worker, ResourceLifecycle::Live)
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("depends on 'jobs'"),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn rejects_an_undeclared_input() {
        let mut input = boolean_input();
        input.id = "somethingElse".to_string();
        let errors = errors_for(stack_with(input)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("does not declare"), "{errors:?}");
    }

    #[tokio::test]
    async fn rejects_a_non_boolean_input() {
        let mut input = boolean_input();
        input.kind = StackInputKind::String;
        input.default = None;
        input.required = true;
        let errors = errors_for(stack_with(input)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("Only a boolean"), "{errors:?}");
    }

    /// A developer-only input never reaches the template, so the resource would
    /// be created no matter what the deployer picked.
    #[tokio::test]
    async fn rejects_an_input_the_deployer_cannot_supply() {
        let mut input = boolean_input();
        input.provided_by = vec![StackInputProvider::Developer];
        let errors = errors_for(stack_with(input)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("not deployer-provided"), "{errors:?}");
    }

    /// Terraform renders an optional input with no default as null, and
    /// `var.x ? 1 : 0` on null fails at apply.
    #[tokio::test]
    async fn rejects_an_optional_input_with_no_default() {
        let mut input = boolean_input();
        input.required = false;
        input.default = None;
        let errors = errors_for(stack_with(input)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("required or declare a default"),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn rejects_an_input_scoped_away_from_the_target_platform() {
        let mut input = boolean_input();
        input.platforms = Some(vec![Platform::Gcp]);
        let errors = errors_for(stack_with(input)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("scoped to"), "{errors:?}");
    }

    /// The runtime role is built from the profile's "*" key alone, so this grant
    /// outlives the resource it was meant for.
    #[tokio::test]
    async fn rejects_a_wildcard_grant_for_the_gated_resource_type() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .permission(
                "execution",
                PermissionProfile::new().global(["kv/data-write"]),
            )
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("at the '*' scope"), "{errors:?}");
        assert!(errors[0].contains(".link() 'store'"), "{errors:?}");
    }

    /// The same grant scoped to the resource is what `.link()` authors, and it
    /// disappears with the resource.
    #[tokio::test]
    async fn accepts_a_resource_scoped_grant_for_the_gated_resource_type() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .permission(
                "execution",
                PermissionProfile::new().resource("store", ["kv/data-write"]),
            )
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// A wildcard grant for an unrelated resource type is untouched by this gate.
    #[tokio::test]
    async fn accepts_a_wildcard_grant_for_another_resource_type() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .permission(
                "execution",
                PermissionProfile::new().global(["storage/data-write"]),
            )
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// The management role is granted the same way and outlives any single
    /// resource, so a '*'-scoped grant there evades nothing.
    #[tokio::test]
    async fn rejects_a_wildcard_management_grant_for_the_gated_resource_type() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input()])
            .management(alien_core::ManagementPermissions::extend(
                PermissionProfile::new().global(["kv/data-write"]),
            ))
            .add_enabled_when(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
                "storeEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("'management'"), "{errors:?}");
        assert!(errors[0].contains("at the '*' scope"), "{errors:?}");
    }

    /// Builds a stack whose bucket depends on a gated store through an explicit
    /// entry dependency, with the bucket's own gate supplied by the caller. Both
    /// are plain data resources, so only the dependency rule is under test.
    fn stack_with_bucket_depending_on_gated_store(bucket_gate: Option<&str>) -> Stack {
        let store = Kv::new("store".to_string()).build();
        let bucket = Storage::new("packager".to_string()).build();
        let store_ref = alien_core::ResourceRef::new("kv".into(), "store");

        let mut bucket_input = boolean_input();
        bucket_input.id = "buildEnabled".to_string();
        let builder = Stack::new("test-stack".to_string())
            .inputs(vec![boolean_input(), bucket_input])
            .add_enabled_when(store, ResourceLifecycle::Frozen, "storeEnabled");

        let mut stack = match bucket_gate {
            Some(gate) => builder.add_enabled_when(bucket, ResourceLifecycle::Frozen, gate),
            None => builder.add(bucket, ResourceLifecycle::Frozen),
        }
        .build();
        stack
            .resources
            .get_mut("packager")
            .expect("bucket entry")
            .dependencies
            .push(store_ref);
        stack
    }

    /// `StackState::get_resource_outputs` errors on a missing resource, so this
    /// build breaks at deploy time for every deployer who says no.
    #[tokio::test]
    async fn rejects_an_ungated_dependent_of_a_gated_resource() {
        let errors = errors_for(stack_with_bucket_depending_on_gated_store(None)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("Resource 'packager' depends on 'store'"),
            "{errors:?}"
        );
        assert!(
            errors[0].contains("Gate 'packager' on 'storeEnabled'"),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn accepts_a_dependent_gated_on_the_same_input() {
        let stack = stack_with_bucket_depending_on_gated_store(Some("storeEnabled"));
        assert!(errors_for(stack).await.is_empty());
    }

    /// Two inputs mean two independent answers, and only one of them creates the store.
    #[tokio::test]
    async fn rejects_a_dependent_gated_on_a_different_input() {
        let errors = errors_for(stack_with_bucket_depending_on_gated_store(Some(
            "buildEnabled",
        )))
        .await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("gated on different inputs"),
            "{errors:?}"
        );
        assert!(
            errors[0].contains("'buildEnabled' and 'storeEnabled'"),
            "{errors:?}"
        );
    }

    /// Builds a stack whose only gated resource is a vault with the given id.
    fn stack_with_gated_vault(vault_id: &str) -> Stack {
        let mut input = boolean_input();
        input.id = "vaultEnabled".to_string();

        Stack::new("test-stack".to_string())
            .inputs(vec![input])
            .add_enabled_when(
                Vault::new(vault_id.to_string()).build(),
                ResourceLifecycle::Frozen,
                "vaultEnabled",
            )
            .build()
    }

    /// `SecretsVaultMutation` links this vault to Workers and compute clusters after
    /// every compile-time check has run. The dependents rule reads the pre-mutation
    /// stack, so nothing else here can catch a gate on it.
    #[tokio::test]
    async fn rejects_a_gated_deployment_secrets_vault() {
        let errors = errors_for(stack_with_gated_vault(SECRETS_VAULT_ID)).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("deployment secrets vault"), "{errors:?}");
        assert!(errors[0].contains("cannot be optional"), "{errors:?}");
    }

    /// Only the reserved id is wired up behind the check's back; any other vault
    /// gates like a normal setup-created resource.
    #[tokio::test]
    async fn accepts_a_gated_vault_with_another_id() {
        assert!(errors_for(stack_with_gated_vault("app-tokens"))
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn ungated_stacks_skip_the_check_entirely() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                Kv::new("store".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        assert!(!ResourceEnabledValidCheck.should_run(&stack, Platform::Aws));
    }
}
