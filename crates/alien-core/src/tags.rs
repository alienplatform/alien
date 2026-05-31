//! Common resource boundary tags used by cloud controllers and permission sets.

use std::collections::HashMap;

pub const ALIEN_STACK_TAG_KEY: &str = "deployment";
pub const ALIEN_RESOURCE_TAG_KEY: &str = "resource";
pub const ALIEN_MANAGED_BY_TAG_KEY: &str = "managed-by";
pub const ALIEN_MANAGED_BY_TAG_VALUE: &str = "deployment";

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
