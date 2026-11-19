//! Common utilities shared between alien-cli and alien-deploy-cli

pub mod airgap;
pub mod kept_keys;
pub mod network;
pub mod setup_fingerprint;
pub mod setup_item;
pub mod setup_templates;
pub mod tui;

pub use setup_item::SetupItem;
pub use tui::ErrorPrinter;

/// What a direct (CLI) setup run can do, sent with every setup request by `alien` and
/// `alien-deploy`. `create-keys`: it creates the stack's encryption keys (AWS KMS, Cloud KMS,
/// Key Vault) with the admin's own credentials. A platform that does not know the field refuses
/// a stack with keys instead.
pub const SETUP_CAPABILITIES: [&str; 1] = ["create-keys"];
