//! The step protocol, which runs a custom resource's controller: as a WebAssembly module in the
//! process that steps the deployment, or over stdin and stdout to a local process.
//!
//! [`ExtensionProxyController`] is the controller the executor runs for a custom resource.
//! [`ProxyFactory`] registers it per type and platform in a deployment's registry.

#[cfg(not(target_arch = "wasm32"))]
mod credentials;
#[cfg(not(target_arch = "wasm32"))]
pub mod host;
pub mod protocol;
mod proxy;
#[cfg(any(feature = "test-utils", test))]
pub mod test_controller;
mod transport;
#[cfg(not(target_arch = "wasm32"))]
pub mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use credentials::{check_extension_identity, ExtensionIdentityCredentials, IdentityHome};
pub use proxy::{
    deployment_config_for_extension, has_extension_identity, stack_settings_for_extension,
    CredentialSource, ExtensionProxyController, ProxyFactory, DELETE_TIMEOUT, HOST_GRACE,
    PROXY_CONTROLLER_TYPE, STEP_DEADLINE,
};
#[cfg(not(target_arch = "wasm32"))]
pub use transport::StdioTransport;
pub use transport::{Transport, TransportFailure, CONNECT_TIMEOUT};
