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
//! Three rules cover what a gate cannot reach on its own: a `"*"`-scoped grant
//! is read straight off the profile and keeps its access after the resource is
//! gone, a dependent of a gated resource looks up outputs that will not be
//! there, and a sibling whose name-prefix grants cover the gated resource's
//! secret namespace keeps that namespace reachable after a deployer says no.

use crate::error::Result;
use crate::mutations::secrets_vault::SECRETS_VAULT_ID;
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

            // `SecretsVaultMutation` links this vault to Live Workers and compute clusters
            // after every compile-time check has run, so `dependents_of_gated_resources`
            // below reads a stack where those links do not exist yet and lets the gate pass.
            if resource_id == SECRETS_VAULT_ID {
                errors.push(format!(
                    "Resource '{resource_id}' is enabled by input '{input_id}', but \
                     '{SECRETS_VAULT_ID}' is the deployment secrets vault. Workers and compute \
                     clusters are wired to it automatically after this check runs, so a deployer \
                     who says no would leave them resolving a binding for a vault that was never \
                     created. Its presence cannot be optional. Give a vault you want to gate a \
                     different id"
                ));
            }

            let resource_type = entry.config.resource_type();

            // Framework infrastructure Alien derives from the stack itself. A gate
            // here is never a customer choice, and ServiceAccountMutation inserts
            // profile-derived "{profile}-sa" entries unconditionally, which would
            // silently overwrite a gated entry before any render guard could fire.
            // The SDK does not offer .enabled() on these; this keeps hand-authored
            // stacks to the same rule.
            if matches!(
                resource_type.as_ref(),
                "build" | "artifact-registry" | "service-account" | "compute-cluster"
            ) {
                errors.push(format!(
                    "Resource '{resource_id}' of type '{resource_type}' is enabled by input \
                     '{input_id}', but Alien derives this resource from the stack itself, so \
                     it cannot be optional"
                ));
                continue;
            }

            // Code-carrying compute ships with every release; whether it runs
            // is a rollout question, not a data-plane opt-out, so gating it is
            // a different feature than this one. Live-only ownership is the
            // compute classification, read from the one table that defines it
            // so a new compute type cannot silently become gateable.
            if !alien_core::ownership_policy_for_resource_type(resource_type.as_ref())
                .allows_frozen()
            {
                errors.push(format!(
                    "Resource '{resource_id}' of type '{resource_type}' is enabled by input \
                     '{input_id}', but code-carrying compute ships with every release, so it \
                     cannot be optional"
                ));
                continue;
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

        errors.extend(dependents_of_gated_resources(stack));
        errors.extend(gated_resources_inside_a_sibling_namespace(stack));

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Rejects resources that would outlive a gated resource they read outputs from.
///
/// `StackState::get_resource_outputs` errors when a resource is absent, so an
/// ungated dependent of a gated resource fails at deploy time in the customer's
/// account. Catching it here fails when the manifest is written instead.
fn dependents_of_gated_resources(stack: &Stack) -> Vec<String> {
    let gates: HashMap<&str, &str> = stack
        .resources()
        .filter_map(|(id, entry)| Some((id.as_str(), entry.enabled_when.as_deref()?)))
        .collect();

    let mut errors = Vec::new();
    for (dependent_id, entry) in stack.resources() {
        // `ResourceEntry` documents the total as `config.get_dependencies()` plus its own
        // list, and each compute type folds its links and triggers into the former. That
        // canonical aggregation is used here rather than the per-type downcast in
        // `resource_link_permissions`, which only keeps the permission-bearing link types.
        let config_dependencies = entry.config.get_dependencies();

        for dependency in config_dependencies.iter().chain(&entry.dependencies) {
            let Some(dependency_gate) = gates.get(dependency.id()) else {
                continue;
            };
            let dependency_id = dependency.id();

            match entry.enabled_when.as_deref() {
                Some(gate) if gate == *dependency_gate => {}
                Some(gate) => errors.push(format!(
                    "Resource '{dependent_id}' depends on '{dependency_id}', but the two are \
                     gated on different inputs: '{gate}' and '{dependency_gate}'. Nothing makes a \
                     deployer answer both the same way, so '{dependent_id}' can be created while \
                     '{dependency_id}' is not. Gate both on '{dependency_gate}'"
                )),
                None => errors.push(format!(
                    "Resource '{dependent_id}' depends on '{dependency_id}', which is enabled by \
                     input '{dependency_gate}'. A deployer who says no would leave \
                     '{dependent_id}' looking up outputs of a resource that was never created. \
                     Gate '{dependent_id}' on '{dependency_gate}' too"
                )),
            }
        }
    }

    errors
}

/// Resource types whose data-plane grants cover a name prefix rather than an
/// exact resource: `vault/data-{read,write}` and `postgres/data-access` bind
/// secret access to every name under `{resource}-*`. AWS keeps the two in
/// different services, but GCP stores both kinds of secret in Secret Manager
/// under the same naming scheme, so the two types form one namespace family.
const NAME_PREFIX_GRANTED_TYPES: &[&str] = &["vault", "postgres"];

/// Rejects a gated resource whose secret namespace stays reachable through a
/// sibling's grants after the deployer declines it.
///
/// Ids may contain hyphens, so `app` and `app-config` are distinct resources
/// whose namespaces nest: a grant on `app` covers everything under `app-*`,
/// including all of `app-config`'s secrets. Declining `app-config` withdraws
/// its own grants but not the sibling's, so the declined namespace stays
/// readable and writable. The rule fires whether or not such a grant exists
/// yet: ids cannot be renamed once deployments exist, so a stack that ships
/// this pair is one `.link()` away from an overlap nobody can fix.
fn gated_resources_inside_a_sibling_namespace(stack: &Stack) -> Vec<String> {
    let mut errors = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        if !NAME_PREFIX_GRANTED_TYPES.contains(&entry.config.resource_type().as_ref()) {
            continue;
        }

        for (sibling_id, sibling) in stack.resources() {
            if !NAME_PREFIX_GRANTED_TYPES.contains(&sibling.config.resource_type().as_ref()) {
                continue;
            }
            if !resource_id.starts_with(&format!("{sibling_id}-")) {
                continue;
            }
            // Gated on the same input, the two exist or vanish together, so no
            // grant survives a namespace it covers.
            if sibling.enabled_when.as_deref() == Some(input_id) {
                continue;
            }

            errors.push(format!(
                "Resource '{resource_id}' is enabled by input '{input_id}', but its id extends \
                 '{sibling_id}', and {} data grants are name-prefix scoped: a grant on \
                 '{sibling_id}' covers every secret named '{sibling_id}-*', which contains all of \
                 '{resource_id}'s. A deployer who says no would still leave '{resource_id}'s \
                 namespace readable and writable through '{sibling_id}'. Gate '{sibling_id}' on \
                 '{input_id}' too, or rename one of them so neither id extends the other",
                sibling.config.resource_type()
            ));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::PermissionProfile, Kv, Storage, ResourceLifecycle, StackInputDefinition,
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

    /// Compute stays out: gating code-carrying resources is a rollout
    /// question, not a data-plane opt-out.
    #[tokio::test]
    async fn rejects_a_gated_compute_resource() {
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

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("cannot be optional"), "{errors:?}");
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
        assert!(errors[0].contains("required or declare a default"), "{errors:?}");
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
        let errors = errors_for(stack_with_bucket_depending_on_gated_store(Some("buildEnabled"))).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("gated on different inputs"), "{errors:?}");
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

    /// A boolean deployer input named `id`, for stacks that gate on more than one.
    fn named_input(id: &str) -> StackInputDefinition {
        let mut input = boolean_input();
        input.id = id.to_string();
        input
    }

    /// Vault grants cover `{id}-*`, so `app`'s grant reads all of `app-config`'s
    /// secrets and declining `app-config` withdraws nothing.
    #[tokio::test]
    async fn rejects_a_gated_vault_inside_an_ungated_siblings_namespace() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![named_input("configEnabled")])
            .add(
                Vault::new("app".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Vault::new("app-config".to_string()).build(),
                ResourceLifecycle::Frozen,
                "configEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("its id extends 'app'"), "{errors:?}");
        assert!(errors[0].contains("rename one of them"), "{errors:?}");
    }

    /// Two gates mean two independent answers, and only one of them removes the
    /// covering grant.
    #[tokio::test]
    async fn rejects_sibling_namespaces_gated_on_different_inputs() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![named_input("appEnabled"), named_input("configEnabled")])
            .add_enabled_when(
                Vault::new("app".to_string()).build(),
                ResourceLifecycle::Frozen,
                "appEnabled",
            )
            .add_enabled_when(
                Vault::new("app-config".to_string()).build(),
                ResourceLifecycle::Frozen,
                "configEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("'app-config'"), "{errors:?}");
        assert!(errors[0].contains("Gate 'app' on 'configEnabled'"), "{errors:?}");
    }

    /// One answer creates or removes both, so the covering grant never outlives
    /// the namespace it covers.
    #[tokio::test]
    async fn accepts_sibling_namespaces_gated_on_the_same_input() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![named_input("featureEnabled")])
            .add_enabled_when(
                Vault::new("app".to_string()).build(),
                ResourceLifecycle::Frozen,
                "featureEnabled",
            )
            .add_enabled_when(
                Vault::new("app-config".to_string()).build(),
                ResourceLifecycle::Frozen,
                "featureEnabled",
            )
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// `app2` is not under `app-*`: the wildcard requires the hyphen, so only a
    /// hyphen extension nests namespaces.
    #[tokio::test]
    async fn accepts_sibling_ids_that_do_not_extend_each_other() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![named_input("appEnabled")])
            .add(
                Vault::new("app2".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Vault::new("app".to_string()).build(),
                ResourceLifecycle::Frozen,
                "appEnabled",
            )
            .build();

        assert!(errors_for(stack).await.is_empty());
    }

    /// GCP stores postgres connection secrets and vault secrets in Secret
    /// Manager under the same naming scheme, so the family crosses the two
    /// types: a postgres named `db` covers a vault named `db-tokens`.
    #[tokio::test]
    async fn rejects_a_gated_vault_inside_a_postgres_namespace() {
        let stack = Stack::new("test-stack".to_string())
            .inputs(vec![named_input("tokensEnabled")])
            .add(
                alien_core::Postgres::new("db".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Vault::new("db-tokens".to_string()).build(),
                ResourceLifecycle::Frozen,
                "tokensEnabled",
            )
            .build();

        let errors = errors_for(stack).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("postgres data grants"), "{errors:?}");
    }

    #[tokio::test]
    async fn ungated_stacks_skip_the_check_entirely() {
        let stack = Stack::new("test-stack".to_string())
            .add(Kv::new("store".to_string()).build(), ResourceLifecycle::Frozen)
            .build();
        assert!(!ResourceEnabledValidCheck.should_run(&stack, Platform::Aws));
    }
}
