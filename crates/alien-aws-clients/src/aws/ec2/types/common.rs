use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Common Types
// ---------------------------------------------------------------------------

/// A filter to apply when describing resources.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Filter {
    /// The name of the filter.
    pub name: String,
    /// The filter values.
    pub values: Vec<String>,
}

/// A tag to apply to a resource.
/// Note: EC2 XML responses use lowercase `<key>` and `<value>` tags.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct Tag {
    /// The key of the tag.
    pub key: String,
    /// The value of the tag.
    pub value: String,
}

/// Tag specification for creating resources with tags.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TagSpecification {
    /// The type of resource to tag.
    pub resource_type: String,
    /// The tags to apply.
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Tag>,
}
