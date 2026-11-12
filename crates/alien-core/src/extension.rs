//! What a stack carries about each extension it uses.
//!
//! A stack lists its extensions in `Stack.extensions`, and each one describes the resource types
//! it defines. Every holder of a stack reads a custom type's descriptor from there, so one
//! deployment never sees the types of an extension another deployment uses.

use crate::{BinaryTarget, PermissionSet, Platform, ResourceLifecycle, ResourceType};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// An extension a stack uses, and the resource types it defines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    /// The extension's name, the first part of each type it defines, such as "acme".
    pub name: String,
    /// The extension's version.
    pub version: String,
    /// The sha256 digest of the extension's bundle. Every loader checks the bundle against it.
    pub digest: String,
    /// The resource types the extension defines, keyed by type name, such as "acme/dns-zone".
    #[cfg_attr(feature = "openapi", schema(value_type = HashMap<String, ResourceTypeDescriptor>))]
    pub resource_types: IndexMap<ResourceType, ResourceTypeDescriptor>,
}

impl ExtensionManifest {
    /// The extension that can define `resource_type`: the type's first part, as `acme` in
    /// `acme/dns-zone`. The name prefixes every type an extension adds.
    pub fn owner_of(resource_type: &ResourceType) -> Option<&str> {
        resource_type
            .as_ref()
            .split_once('/')
            .map(|(extension, _)| extension)
    }

    /// Whether this extension can define `resource_type`.
    pub fn owns(&self, resource_type: &ResourceType) -> bool {
        Self::owner_of(resource_type) == Some(self.name.as_str())
    }

    /// The id of the setup-owned identity the extension's controllers run as, such as
    /// "acme-ext". Short, since cloud identity names carry the deployment's prefix and GCP caps
    /// a service account's at 30 characters.
    pub fn identity_id(&self) -> String {
        format!("{}-ext", self.name)
    }
}

/// The name the customer sees for an extension's identity, in the setup template and the cloud
/// console, such as "Extension identity (acme)".
pub fn extension_identity_display_name(extension: &str) -> String {
    format!("Extension identity ({extension})")
}

/// The AWS role name setup gives an extension's identity: `<prefix>-<id>`, cut to IAM's 64
/// characters with a hash of the full name, as the Terraform template derives it.
pub fn extension_identity_aws_role_name(prefix: &str, id: &str) -> String {
    let full = format!("{prefix}-{id}");
    if full.chars().count() <= 64 {
        return full;
    }
    let stem: String = full.chars().take(55).collect();
    format!("{stem}-{}", &sha1_hex(&full)[..8])
}

/// The GCP service account id setup gives an extension's identity, as the Terraform template
/// derives every service account's: a readable stem and a hash of the full name, within GCP's 30
/// characters.
pub fn extension_identity_gcp_account_id(prefix: &str, id: &str) -> String {
    let full = format!("{prefix}-{}", terraform_label(id))
        .to_lowercase()
        .replace('_', "-");
    let stem: String = format!("a-{full}").chars().take(21).collect();
    format!("{}-{}", stem.trim_matches('-'), &sha1_hex(&full)[..8])
}

/// The Terraform label a resource id gets in a generated module.
fn terraform_label(id: &str) -> String {
    let label: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if label.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{label}")
    } else {
        label
    }
}

fn sha1_hex(value: &str) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(value.as_bytes()))
}

/// What Alien knows about a resource type an extension defines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeDescriptor {
    /// The lifecycles a resource of this type may have.
    pub lifecycles: Vec<ResourceLifecycle>,
    /// Whether deleting the resource can lose the customer's data. Removing a deployment keeps
    /// such a resource unless the deletion erases data.
    pub holds_data: bool,
    /// The setup templates and importers the extension provides for this type.
    pub setup: SetupSupport,
    /// The platforms the extension ships a controller for this type on.
    pub controller_platforms: Vec<Platform>,
    /// The permission sets the extension defines for this type. Their ids are
    /// `<extension>/<type>/<name>`.
    pub permission_sets: Vec<PermissionSet>,
    /// What a workload that links a resource of this type gets.
    pub link: LinkDescriptor,
    /// The JSON schemas of the type's config and outputs.
    pub schemas: ResourceTypeSchemas,
    /// The cloud services the type needs enabled, per platform: GCP APIs such as
    /// "dns.googleapis.com", Azure resource providers such as "Microsoft.Network". Alien enables
    /// them with the services its own resource types need.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = HashMap<String, Vec<String>>))]
    pub services: IndexMap<Platform, Vec<String>>,
    /// Whether the type needs the stack's network. Alien then creates the network on AWS, GCP
    /// and Azure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_network: bool,
}

/// The stems of the files Alien's Terraform module writes besides one per resource: an
/// extension's Terraform modifier writes `<extension>.tf` beside them. `override` is there too,
/// since Terraform merges `override.tf` into the module, and `main`, the file a module's own
/// configuration conventionally sits in. alien-terraform checks its generator against the list.
pub const TERRAFORM_MODULE_FILE_STEMS: [&str; 12] = [
    "versions",
    "variables",
    "providers",
    "resource_prefix",
    "locals",
    "iam_propagation",
    "registration",
    "helm",
    "outputs",
    "retention-inventory",
    "override",
    "main",
];

/// The longest extension name a setup can name the extension's identity with. The
/// CloudFormation role is `${AWS::StackName}-<name>-ext`, which isn't cut to fit, and a
/// generated stack's prefix is at most 35 characters: 35 + 1 + 24 + 4 is IAM's 64, as for a
/// permission profile's `-sa` role on EKS. The EKS CloudFormation template takes at most 20:
/// it also names the service account of the pod the extension's controller runs in,
/// `${AWS::StackName}-<name>-ext-sa`, which it can't cut to a DNS label.
pub const MAX_EXTENSION_NAME_LENGTH: usize = 24;

/// Whether `name` is a lowercase letter followed by lowercase letters, digits and dashes: the
/// form of an extension's name and of the last segment of its permission sets' ids. Such a name
/// is a single path segment, a Terraform file stem with no `_override` suffix, and a valid part
/// of every cloud's identity names.
pub fn is_extension_name_segment(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What makes `name` unusable as an extension's name, or `None` when it can be one. The name
/// prefixes each type the extension defines, names its identity in every setup and its file in
/// Alien's Terraform module. `alien extensions build`, the SDK's `export!` and `alien release`
/// all apply this rule.
pub fn extension_name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        Some("is empty".to_string())
    } else if !is_extension_name_segment(name) {
        Some(
            "must start with a lowercase letter and hold only lowercase letters, digits and `-`"
                .to_string(),
        )
    } else if name.chars().count() > MAX_EXTENSION_NAME_LENGTH {
        Some(format!(
            "is longer than {MAX_EXTENSION_NAME_LENGTH} characters, which the name of its identity's AWS role can't fit"
        ))
    } else if crate::RESERVED_TYPE_PREFIXES.contains(&name) {
        Some("is reserved for Alien's own resource types".to_string())
    } else if TERRAFORM_MODULE_FILE_STEMS.contains(&name) {
        Some(format!(
            "names the file `{name}.tf` in Alien's Terraform module, beside which a Terraform modifier writes `<extension>.tf`"
        ))
    } else {
        None
    }
}

/// Whether `service` names a cloud service Alien can enable on `platform`: a GCP API,
/// `<name>.googleapis.com`, or an Azure resource provider, `Microsoft.<Namespace>`. Azure
/// compares provider names without case, so the prefix may be in any case.
pub fn is_cloud_service_name(platform: Platform, service: &str) -> bool {
    match platform {
        Platform::Gcp => service.strip_suffix(".googleapis.com").is_some_and(|name| {
            name.starts_with(|c: char| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }),
        Platform::Azure => service.split_once('.').is_some_and(|(prefix, namespace)| {
            prefix.eq_ignore_ascii_case("Microsoft")
                && namespace.starts_with(|c: char| c.is_ascii_alphabetic())
                && namespace.chars().all(|c| c.is_ascii_alphanumeric())
        }),
        _ => false,
    }
}

/// The platforms each setup path supports for one resource type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupSupport {
    /// Platforms with a CloudFormation emitter.
    pub cloudformation: Vec<Platform>,
    /// Platforms with a Terraform emitter.
    pub terraform: Vec<Platform>,
    /// Platforms with a Helm emitter.
    pub helm: Vec<Platform>,
    /// Platforms with an importer, which reads a resource a setup template created.
    pub importers: Vec<Platform>,
}

/// A setup template format Alien generates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupFormat {
    CloudFormation,
    Terraform,
    Helm,
}

impl std::fmt::Display for SetupFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SetupFormat::CloudFormation => "CloudFormation",
            SetupFormat::Terraform => "Terraform",
            SetupFormat::Helm => "Helm",
        })
    }
}

impl SetupSupport {
    /// The platforms the type has an emitter for in `format`.
    pub fn platforms(&self, format: SetupFormat) -> &[Platform] {
        match format {
            SetupFormat::CloudFormation => &self.cloudformation,
            SetupFormat::Terraform => &self.terraform,
            SetupFormat::Helm => &self.helm,
        }
    }
}

/// What a workload that links a resource of this type gets, besides environment variables.
///
/// The extension computes a link's environment variables from the resource's outputs. The
/// permission sets are fixed per type, because setup grants them before the resource has outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkDescriptor {
    /// Ids of the permission sets the workload gets on the linked resource.
    pub permission_sets: Vec<String>,
}

/// The JSON schemas of a resource type's config and outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTypeSchemas {
    /// The schema of the resource's config.
    pub config: serde_json::Map<String, serde_json::Value>,
    /// The schema of the resource's outputs.
    pub outputs: serde_json::Map<String, serde_json::Value>,
}

/// The file in an extension bundle's core zip that holds its [`ExtensionBundleManifest`].
pub const EXTENSION_BUNDLE_MANIFEST: &str = "manifest.json";

/// The file in an extension bundle's core zip that holds its hooks module.
pub const EXTENSION_HOOKS_MODULE: &str = "hooks.wasm";

/// What an extension bundle's core zip says about the extension: what a loader checks before it
/// runs anything. The zip's own digest is not in it, since a file can't hold its own digest; the
/// stack pins the zip by [`ExtensionManifest::digest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionBundleManifest {
    /// The extension's name, as `export!` declares it.
    pub name: String,
    /// The extension's version, from its hooks crate.
    pub version: String,
    /// The protocol versions the extension speaks.
    pub protocol: ExtensionProtocols,
    /// The HTTPS server that runs the extension's controllers in a push deployment.
    pub controller_url: Option<String>,
    /// The sha256 of the hooks module, hex-encoded.
    pub hooks_sha256: String,
    /// The controller binary per target. Every target is listed when the extension has a
    /// controller, and one the build did not produce is `null`. Empty without a controller.
    pub controllers: IndexMap<BinaryTarget, Option<ControllerBinary>>,
    /// The resource types the extension defines, keyed by type name.
    pub resource_types: IndexMap<ResourceType, ResourceTypeDescriptor>,
}

/// The protocol versions an extension speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionProtocols {
    /// The hooks module's ABI version.
    pub hooks: u32,
    /// The step protocol version its controller speaks, or `null` without a controller.
    pub controller: Option<u32>,
}

/// One controller binary in an extension bundle, stored beside the core zip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerBinary {
    /// The binary's sha256, hex-encoded.
    pub sha256: String,
    /// The binary's size in bytes.
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names an extension can't take, each with the rule that refuses it, and names it can.
    #[test]
    fn an_extension_name_is_a_short_lowercase_segment_alien_does_not_use() {
        for (name, problem) in [
            ("alien", "reserved"),
            ("experimental", "reserved"),
            ("", "empty"),
            ("a/b", "lowercase letters"),
            ("my_ext", "lowercase letters"),
            ("foo_override", "lowercase letters"),
            ("override", "override.tf"),
            ("..", "lowercase letters"),
            ("a..b", "lowercase letters"),
            ("a\\b", "lowercase letters"),
            ("Acme", "lowercase letters"),
            ("a b", "lowercase letters"),
            ("outputs", "outputs.tf"),
            ("main", "main.tf"),
            ("-acme", "lowercase letters"),
            ("aaaaaaaaaaaaaaaaaaaaaaaaa", "longer than 24 characters"),
            ("ééééééééééééé", "lowercase letters"),
            ("retention-inventory", "retention-inventory.tf"),
        ] {
            let found = extension_name_problem(name).unwrap_or_default();
            assert!(found.contains(problem), "{name:?}: {found:?}");
        }
        for name in [
            "acme",
            "a",
            "aaaaaaaaaaaaaaaaaaaaaaaa",
            "dns-zone-2",
            "backups",
        ] {
            assert_eq!(extension_name_problem(name), None, "{name:?}");
        }
    }

    #[test]
    fn cloud_service_names_are_gcp_apis_or_azure_providers() {
        for (platform, service) in [
            (Platform::Gcp, "dns.googleapis.com"),
            (Platform::Gcp, "storage-component.googleapis.com"),
            (Platform::Azure, "Microsoft.Network"),
            (Platform::Azure, "microsoft.dbforpostgresql"),
        ] {
            assert!(is_cloud_service_name(platform, service), "{service}");
        }
        for (platform, service) in [
            (Platform::Gcp, "cloud.quotas"),
            (Platform::Gcp, "dns_api.googleapis.com"),
            (Platform::Gcp, "a.b.googleapis.com"),
            (Platform::Gcp, "Dns.googleapis.com"),
            (Platform::Gcp, ".googleapis.com"),
            (Platform::Azure, "App"),
            (Platform::Azure, "Microsoft."),
            (Platform::Azure, "Microsoft.Network.Dns"),
            (Platform::Azure, "Contoso.Network"),
            (Platform::Aws, "route53.amazonaws.com"),
            (Platform::Gcp, "Microsoft.Network"),
        ] {
            assert!(!is_cloud_service_name(platform, service), "{service}");
        }
    }
}
