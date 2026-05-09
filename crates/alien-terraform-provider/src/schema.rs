//! Provider + resource schema declarations.
//!
//! These structs are the source-of-truth shape for the eventual tfplugin6
//! [`Schema`] message. They intentionally carry only what the protocol needs —
//! attribute name, type kind, required/optional/computed flags, sensitivity —
//! so a thin adapter can map them onto `tfplugin6::Schema` without dragging
//! the whole gRPC surface into this crate.
//!
//! When the gRPC adapter lands (ALIEN-92 follow-up), it will translate
//! [`Schema`] / [`Attribute`] into protocol bytes; until then the same
//! structures drive validation in [`crate::resource_alien_deployment`].

use serde::{Deserialize, Serialize};

/// Top-level provider schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub attributes: Vec<Attribute>,
}

/// One schema attribute. Mirrors the subset of `tfplugin6.Schema.Attribute`
/// that this provider actually uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    pub description: String,
    pub kind: AttributeKind,
    pub required: bool,
    pub optional: bool,
    pub computed: bool,
    pub sensitive: bool,
}

/// Attribute primitive kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttributeKind {
    String,
    Bool,
    /// Dynamic JSON object — used for `management_config`, `stack_settings`.
    Dynamic,
    /// Homogeneous list of `T`.
    List(Box<AttributeKind>),
    /// Object with named typed fields.
    Object(Vec<NamedKind>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedKind {
    pub name: String,
    pub kind: AttributeKind,
}

fn attr(
    name: &str,
    description: &str,
    kind: AttributeKind,
    required: bool,
    optional: bool,
    computed: bool,
    sensitive: bool,
) -> Attribute {
    Attribute {
        name: name.to_string(),
        description: description.to_string(),
        kind,
        required,
        optional,
        computed,
        sensitive,
    }
}

/// Provider schema. Empty for `alien` — every knob lives on the resource.
pub fn provider_schema() -> Schema {
    Schema { attributes: vec![] }
}

/// Resource schema for `alien_deployment`. Mirrors the
/// `terraform_data.alien_stack_import.input` shape produced by
/// [`alien_terraform::generator`] so a customer can pipe module outputs
/// straight into this resource:
///
/// ```hcl
/// module "stack" { source = "./alien-tf" }
///
/// resource "alien_deployment" "this" {
///   manager_url            = "https://manager.example.com"
///   deployment_group_token = var.alien_dg_token
///   name                   = "acme-prod"
///   stack_prefix           = module.stack.deployment_stack_prefix
///   platform               = module.stack.deployment_platform
///   region                 = module.stack.deployment_region
///   management_config      = jsondecode(module.stack.deployment_management_config)
///   stack_settings         = jsondecode(module.stack.deployment_stack_settings)
///   resources              = jsondecode(module.stack.deployment_resources)
/// }
/// ```
///
/// The `name` attribute is required and uniquely identifies the deployment
/// inside the deployment group — the manager returns 409 on collision, so
/// callers must pick a distinct name per logical deployment (typically one
/// `alien_deployment` resource per Terraform workspace).
pub fn resource_schema() -> Schema {
    Schema {
        attributes: vec![
            attr(
                "manager_url",
                "URL of the Alien Manager to register against. Defaults to the white-label baked-in URL when present.",
                AttributeKind::String,
                false,
                true,
                true,
                false,
            ),
            attr(
                "deployment_group_token",
                "Deployment-group bearer token. Authorizes the import; never logged.",
                AttributeKind::String,
                true,
                false,
                false,
                true,
            ),
            attr(
                "name",
                "Deployment name. Required and unique within the deployment group — the manager returns 409 on collision.",
                AttributeKind::String,
                true,
                false,
                false,
                false,
            ),
            attr(
                "stack_prefix",
                "Physical stack prefix used by the generated module. Pass `module.x.deployment_stack_prefix`.",
                AttributeKind::String,
                true,
                false,
                false,
                false,
            ),
            attr(
                "platform",
                "Cloud platform of the imported stack. One of: `aws`, `gcp`, `azure`, `kubernetes`.",
                AttributeKind::String,
                true,
                false,
                false,
                false,
            ),
            attr(
                "region",
                "Region (or location) reported by the distribution artifact.",
                AttributeKind::String,
                true,
                false,
                false,
                false,
            ),
            attr(
                "management_config",
                "Platform-derived management configuration (JSON object). Pass `jsondecode(module.x.deployment_management_config)`.",
                AttributeKind::Dynamic,
                true,
                false,
                false,
                false,
            ),
            attr(
                "stack_settings",
                "Stack settings supplied by the distribution artifact (JSON object). Pass `jsondecode(module.x.deployment_stack_settings)`.",
                AttributeKind::Dynamic,
                true,
                false,
                false,
                false,
            ),
            attr(
                "resources",
                "List of imported resources, each with `id`, `type`, and typed `import_data`.",
                AttributeKind::List(Box::new(AttributeKind::Object(vec![
                    NamedKind {
                        name: "id".to_string(),
                        kind: AttributeKind::String,
                    },
                    NamedKind {
                        name: "type".to_string(),
                        kind: AttributeKind::String,
                    },
                    NamedKind {
                        name: "import_data".to_string(),
                        kind: AttributeKind::Dynamic,
                    },
                ]))),
                true,
                false,
                false,
                false,
            ),
            attr(
                "deployment_id",
                "Manager-assigned deployment id. Computed after Create.",
                AttributeKind::String,
                false,
                false,
                true,
                false,
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_schema_has_no_attributes() {
        assert!(provider_schema().attributes.is_empty());
    }

    #[test]
    fn resource_schema_required_attributes_match_request_shape() {
        let schema = resource_schema();
        let required: Vec<&str> = schema
            .attributes
            .iter()
            .filter(|a| a.required)
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(
            required,
            vec![
                "deployment_group_token",
                "name",
                "stack_prefix",
                "platform",
                "region",
                "management_config",
                "stack_settings",
                "resources",
            ]
        );
    }

    #[test]
    fn deployment_group_token_is_sensitive() {
        let schema = resource_schema();
        let attr = schema
            .attributes
            .iter()
            .find(|a| a.name == "deployment_group_token")
            .expect("deployment_group_token attribute");
        assert!(
            attr.sensitive,
            "deployment_group_token MUST be sensitive — terraform plan / log output \
             must not echo bearer tokens."
        );
    }
}
