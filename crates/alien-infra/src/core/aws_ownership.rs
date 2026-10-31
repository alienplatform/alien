//! Telling an AWS object this deployment created from one that only shares its name.
//!
//! Controllers derive object names from the deployment prefix and resource ID, so the same
//! account can already hold an object with that name that someone else made. Only the tags a
//! controller writes on what it creates show that an object is ours.

use std::collections::HashMap;

use alien_core::{
    setup_resource_tags, standard_resource_tags, InitialSetupAuthority, ResourceLifecycle,
    ALIEN_MANAGED_BY_TAG_KEY,
};

/// The `managed-by` value older versions wrote on the objects they created.
const LEGACY_MANAGED_BY_TAG_VALUE: &str = "deployment";

/// Whether `tags` carry every tag in `ours`, the tags this deployment writes for the resource
/// (`standard_resource_tags`). `managed-by=deployment`, written by versions before the
/// `runtime` value, is accepted too.
pub(crate) fn is_tagged_as_ours(
    tags: &HashMap<String, String>,
    ours: &HashMap<String, String>,
) -> bool {
    ours.iter().all(|(key, value)| {
        let found = tags.get(key).map(String::as_str);
        found == Some(value.as_str())
            || (key == ALIEN_MANAGED_BY_TAG_KEY && found == Some(LEGACY_MANAGED_BY_TAG_VALUE))
    })
}

/// Whether a retained data resource is this deployment's, judged by the tags of whoever
/// created it. A setup template (CloudFormation or Terraform, imported as `ImportedHandoff`)
/// writes the setup tags; controllers write the runtime tags, both for runtime-owned (Live)
/// resources and for setup-owned (Frozen) ones they create in a direct setup. Choosing by
/// creator, never accepting either set, keeps retention away from a same-named object that
/// another deployment or tool made.
pub(crate) fn is_retained_resource_ours(
    tags: &HashMap<String, String>,
    prefix: &str,
    resource_id: &str,
    resource_type: &str,
    lifecycle: ResourceLifecycle,
    setup_authority: InitialSetupAuthority,
) -> bool {
    match (lifecycle, setup_authority) {
        (ResourceLifecycle::Frozen, InitialSetupAuthority::ImportedHandoff) => {
            setup_resource_tags(prefix, resource_id, resource_type)
                .iter()
                .all(|(key, value)| tags.get(key) == Some(value))
        }
        (ResourceLifecycle::Frozen, InitialSetupAuthority::DirectSetup)
        | (ResourceLifecycle::Live, _) => {
            is_tagged_as_ours(tags, &standard_resource_tags(prefix, resource_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::standard_resource_tags;

    #[test]
    fn requires_every_tag_of_the_resource() {
        let ours = standard_resource_tags("prefix", "data");

        assert!(is_tagged_as_ours(&ours, &ours));

        let mut extra = ours.clone();
        extra.insert("team".to_string(), "x".to_string());
        assert!(is_tagged_as_ours(&extra, &ours), "extra tags are fine");

        let mut legacy = ours.clone();
        legacy.insert("managed-by".to_string(), "deployment".to_string());
        assert!(is_tagged_as_ours(&legacy, &ours));

        let other_resource = standard_resource_tags("prefix", "other");
        assert!(!is_tagged_as_ours(&other_resource, &ours));
        let other_deployment = standard_resource_tags("other-prefix", "data");
        assert!(!is_tagged_as_ours(&other_deployment, &ours));
        let mut setup = ours.clone();
        setup.insert("managed-by".to_string(), "setup".to_string());
        assert!(!is_tagged_as_ours(&setup, &ours));
        assert!(!is_tagged_as_ours(&HashMap::new(), &ours));
    }

    /// The tags each owner writes, as the CloudFormation/Terraform packages and controllers do.
    fn written(owner: &str, prefix: &str, id: &str) -> HashMap<String, String> {
        let mut tags = match owner {
            "setup" => setup_resource_tags(prefix, id, "storage")
                .into_iter()
                .collect(),
            "runtime" => standard_resource_tags(prefix, id),
            "legacy-runtime" => {
                let mut tags = standard_resource_tags(prefix, id);
                tags.insert("managed-by".into(), "deployment".into());
                tags
            }
            _ => HashMap::new(),
        };
        tags.insert("team".into(), "unrelated".into());
        tags
    }

    /// Retained resources are judged by their creator's tags only: template-setup resources by
    /// the setup tags, controller-created ones (Live, or Frozen in a direct setup) by the
    /// runtime tags. Another deployment's or resource's tags, and the other creator's tags of
    /// this deployment, are all refused.
    #[test]
    fn retained_resources_must_carry_their_creators_tags() {
        use InitialSetupAuthority::{DirectSetup, ImportedHandoff};
        use ResourceLifecycle::{Frozen, Live};
        for (owner, prefix, id, lifecycle, authority, expected) in [
            ("setup", "prefix", "data", Frozen, ImportedHandoff, true),
            ("runtime", "prefix", "data", Frozen, DirectSetup, true),
            (
                "legacy-runtime",
                "prefix",
                "data",
                Frozen,
                DirectSetup,
                true,
            ),
            ("runtime", "prefix", "data", Live, ImportedHandoff, true),
            ("runtime", "prefix", "data", Live, DirectSetup, true),
            (
                "legacy-runtime",
                "prefix",
                "data",
                Live,
                ImportedHandoff,
                true,
            ),
            ("runtime", "prefix", "data", Frozen, ImportedHandoff, false),
            (
                "legacy-runtime",
                "prefix",
                "data",
                Frozen,
                ImportedHandoff,
                false,
            ),
            ("setup", "prefix", "data", Frozen, DirectSetup, false),
            ("setup", "prefix", "data", Live, ImportedHandoff, false),
            (
                "setup",
                "other-prefix",
                "data",
                Frozen,
                ImportedHandoff,
                false,
            ),
            (
                "runtime",
                "other-prefix",
                "data",
                Frozen,
                DirectSetup,
                false,
            ),
            (
                "runtime",
                "other-prefix",
                "data",
                Live,
                ImportedHandoff,
                false,
            ),
            ("setup", "prefix", "other", Frozen, ImportedHandoff, false),
            ("none", "prefix", "data", Frozen, ImportedHandoff, false),
            ("none", "prefix", "data", Frozen, DirectSetup, false),
            ("none", "prefix", "data", Live, ImportedHandoff, false),
        ] {
            assert_eq!(
                is_retained_resource_ours(
                    &written(owner, prefix, id),
                    "prefix",
                    "data",
                    "storage",
                    lifecycle,
                    authority,
                ),
                expected,
                "{owner} tags of {prefix}/{id} judged as {lifecycle:?} after {authority:?}"
            );
        }
        let mut wrong_type = written("setup", "prefix", "data");
        wrong_type.insert("resource-type".into(), "kv".into());
        assert!(!is_retained_resource_ours(
            &wrong_type,
            "prefix",
            "data",
            "storage",
            Frozen,
            ImportedHandoff
        ));
    }
}
