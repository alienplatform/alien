//! What a Terraform setup package creates for a sandbox, against what a direct setup creates.
//!
//! The Terraform emitters and the direct-setup controllers are two implementations of one
//! setup. An item only the emitter produces is a hole in direct setup that nothing else would
//! notice: the install works until the feature that needs the item is used. Each emitter item is
//! listed here with its status; the emitters' tests fail on an item that is not listed, and on a
//! listed item the emitter no longer produces, so a new emitter-only piece cannot land unseen.
//!
//! `Pending` is a known gap that is being closed. When it lands, change it to `Creates` and add
//! the controller test that proves the same grant, with the same role, scope and member.

/// How direct setup treats one item the Terraform package creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSetup {
    /// Direct setup creates it, with the same role, scope and member as the package.
    Creates,
    /// Direct setup does not create it yet. The text names the piece of work that will.
    Pending(&'static str),
    /// Only the package creates it. The text names the build refusal that covers it.
    PackageOnly(&'static str),
}

/// Items the Google Cloud sandbox emitter produces, keyed by what each one is for.
pub const GCP_SANDBOX_ITEMS: &[(&str, DirectSetup)] = &[
    ("engine-grant:access", DirectSetup::Creates),
    ("engine-grant:templates", DirectSetup::Creates),
    ("engine-grant:preview", DirectSetup::Creates),
    ("preview-signer-account", DirectSetup::Creates),
    ("preview-sign-role", DirectSetup::Creates),
    ("preview-sign-binding", DirectSetup::Creates),
];

/// Items the Azure sandbox emitter produces, keyed by what each one is for.
pub const AZURE_SANDBOX_ITEMS: &[(&str, DirectSetup)] = &[
    ("sandbox-group", DirectSetup::Creates),
    (
        "vnet-connection",
        DirectSetup::PackageOnly(
            "internet egress needs the package's dedicated sandbox subnets; a direct setup run \
             refuses it and names the package",
        ),
    ),
    ("role-assignment:access", DirectSetup::Creates),
    ("role-assignment:images", DirectSetup::Creates),
    ("role-assignment:workload", DirectSetup::Creates),
    (
        "storage-store",
        DirectSetup::PackageOnly(
            "persistent storage needs the store account and container the package creates; a \
             direct setup run refuses it and names the package",
        ),
    ),
    (
        "role-assignment:storage-store",
        DirectSetup::PackageOnly(
            "the store's grants (Blob Delegator on the account, Blob Data Contributor on its \
             container) come with the package's store; a direct setup run refuses persistent \
             storage",
        ),
    ),
];
