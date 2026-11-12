//! The step protocol, which runs a custom resource's controller outside Alien: over stdin and
//! stdout to a local process, or over HTTPS to the developer's server.
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
pub mod token;
mod transport;

#[cfg(not(target_arch = "wasm32"))]
pub use credentials::ForwardedCredentials;
pub use proxy::{
    deployment_config_for_extension, stack_settings_for_extension, CredentialSource,
    ExtensionProxyController, ProxyFactory, DELETE_TIMEOUT, HOST_GRACE, PROXY_CONTROLLER_TYPE,
    STEP_DEADLINE,
};
#[cfg(not(target_arch = "wasm32"))]
pub use transport::{remove_stale_private_copies, HttpTransport, StdioTransport};
pub use transport::{TokenSigner, Transport, TransportFailure, CONNECT_TIMEOUT};
