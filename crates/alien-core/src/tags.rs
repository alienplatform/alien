//! Common resource boundary tags used by cloud controllers and permission sets.

use std::collections::HashMap;

pub const ALIEN_STACK_TAG_KEY: &str = "deployment";
pub const ALIEN_RESOURCE_TAG_KEY: &str = "resource";
pub const ALIEN_MANAGED_BY_TAG_KEY: &str = "managed-by";
pub const ALIEN_MANAGED_BY_TAG_VALUE: &str = "runtime";
pub const DEFAULT_ALIEN_LABEL_DOMAIN: &str = "alien.dev";

pub fn standard_resource_tags(
    stack_prefix: impl Into<String>,
    resource_id: impl Into<String>,
) -> HashMap<String, String> {
    HashMap::from([
        (ALIEN_STACK_TAG_KEY.to_string(), stack_prefix.into()),
        (ALIEN_RESOURCE_TAG_KEY.to_string(), resource_id.into()),
        (
            ALIEN_MANAGED_BY_TAG_KEY.to_string(),
            ALIEN_MANAGED_BY_TAG_VALUE.to_string(),
        ),
    ])
}

pub fn branded_tag_key(domain: impl AsRef<str>, key: impl AsRef<str>) -> String {
    format!("{}/{}", domain.as_ref(), key.as_ref())
}

pub fn branded_standard_resource_tags(
    domain: impl AsRef<str>,
    stack_prefix: impl Into<String>,
    resource_id: impl Into<String>,
) -> HashMap<String, String> {
    let domain = domain.as_ref();
    standard_resource_tags(stack_prefix, resource_id)
        .into_iter()
        .map(|(key, value)| (branded_tag_key(domain, key), value))
        .collect()
}

pub fn default_branded_standard_resource_tags(
    stack_prefix: impl Into<String>,
    resource_id: impl Into<String>,
) -> HashMap<String, String> {
    branded_standard_resource_tags(DEFAULT_ALIEN_LABEL_DOMAIN, stack_prefix, resource_id)
}
