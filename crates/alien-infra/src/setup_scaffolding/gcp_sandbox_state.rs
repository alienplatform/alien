//! What a GCP sandbox's recorded state says about the setup run. Reads state only, so it builds
//! on every platform feature set, unlike the Google calls in `gcp_sandbox`.

use alien_core::StackState;

/// Whether `role` is this stack's custom role for the `sandbox/templates` permission set: the
/// role id is `role_<namespace>_sandbox_templates`, with an optional `_<suffix>`. Exact, so another
/// permission set's role that merely mentions templates never counts.
pub(super) fn is_templates_role(role: &str, resource_prefix: &str) -> bool {
    let id = role.rsplit('/').next().unwrap_or(role);
    let base = format!(
        "role_{}_sandbox_templates",
        alien_permissions::generators::custom_role_namespace_for_prefix(resource_prefix)
    );
    id == base
        || id
            .strip_prefix(&base)
            .is_some_and(|rest| rest.starts_with('_'))
}

/// Whether the setup run recorded, in the sandbox's own controller state, that it gave management
/// the `sandbox/templates` grant on the engine. Read as JSON so this holds whether or not the
/// controller carries the record yet: a state without it reads as not granted, which asks for the
/// setup run that places it.
pub(super) fn templates_grant_recorded(stack_state: &StackState, sandbox_id: &str) -> bool {
    stack_state
        .resources
        .get(sandbox_id)
        .and_then(|state| state.internal_state.as_ref())
        .and_then(|internal| internal.get("engineSetup")?.get("engineGrants")?.as_array())
        .is_some_and(|grants| {
            grants.iter().any(|grant| {
                grant
                    .get(0)
                    .and_then(|role| role.as_str())
                    .is_some_and(|role| is_templates_role(role, &stack_state.resource_prefix))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "acme";

    #[test]
    fn only_this_stacks_templates_role_counts_as_the_template_grant() {
        let ns = PREFIX.replace('-', "_");
        assert!(is_templates_role(
            &format!("projects/p/roles/role_{ns}_sandbox_templates"),
            PREFIX
        ));
        assert!(is_templates_role(
            &format!("projects/p/roles/role_{ns}_sandbox_templates_ab12"),
            PREFIX
        ));
        for other in [
            format!("projects/p/roles/role_{ns}_sandbox_templates2"),
            format!("projects/p/roles/role_{ns}_sandbox_execute"),
            format!("projects/p/roles/role_other_sandbox_templates"),
            "projects/p/roles/templates_role".to_string(),
        ] {
            assert!(!is_templates_role(&other, PREFIX), "{other}");
        }
    }
}
