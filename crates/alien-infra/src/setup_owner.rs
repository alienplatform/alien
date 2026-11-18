//! Ownership markers for what a direct setup creates.
//!
//! A marker names the deployment itself, not only the resource prefix and id: two deployments
//! can reuse a prefix in one subscription or project, and the second must never mistake the
//! first one's resource for its own (destroying either would then delete the shared resource).
//! Markers are compared exactly.

const DESCRIPTION_LEAD: &str = "Alien setup owner: ";

/// The owner value: `<prefix>/<resource id>@<deployment id>`.
pub(crate) fn owner_value(resource_prefix: &str, resource_id: &str, deployment_id: &str) -> String {
    format!("{resource_prefix}/{resource_id}@{deployment_id}")
}

/// The owner value as written into a resource description.
pub(crate) fn description_marker(owner: &str) -> String {
    format!("{DESCRIPTION_LEAD}{owner}.")
}

/// True when `description` ends in exactly this owner's marker. A different deployment's marker
/// (even one that contains this owner's text) or a description without one never matches.
pub(crate) fn description_names_owner(description: &str, owner: &str) -> bool {
    description
        .rsplit_once(DESCRIPTION_LEAD)
        .is_some_and(|(_, rest)| rest == format!("{owner}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_value_names_the_deployment() {
        assert_eq!(owner_value("p", "box", "dep_1"), "p/box@dep_1");
    }

    #[test]
    fn a_description_matches_only_its_exact_owner() {
        let ours = owner_value("p", "box", "dep_1");
        let text = format!("Signs things. {}", description_marker(&ours));
        assert!(description_names_owner(&text, &ours));
        assert!(description_names_owner(&description_marker(&ours), &ours));
        // Same prefix and id, another deployment.
        let other = owner_value("p", "box", "dep_2");
        assert!(!description_names_owner(&text, &other));
        // The older prefix/id marker, and a marker that merely contains ours.
        assert!(!description_names_owner("Alien setup owner: p/box.", &ours));
        let longer = description_marker(&owner_value("p", "box", "dep_10"));
        assert!(!description_names_owner(&longer, &owner_value("p", "box", "dep_1")));
        assert!(!description_names_owner("handmade", &ours));
    }
}
