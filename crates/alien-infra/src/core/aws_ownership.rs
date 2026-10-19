//! Telling an AWS object this deployment created from one that only shares its name.
//!
//! Controllers derive object names from the deployment prefix and resource ID, so the same
//! account can already hold an object with that name that someone else made. Only the tags a
//! controller writes on what it creates show that an object is ours.

use std::collections::HashMap;

use alien_core::ALIEN_MANAGED_BY_TAG_KEY;

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
}
