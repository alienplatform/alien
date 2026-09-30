//! The registry proxy checks OCI names, references, upload sessions and blob mount sources before
//! it forwards a request upstream. Requests go over a raw socket so no client rewrites them first.

#[path = "registry_proxy_validation/compatibility.rs"]
mod compatibility;
#[path = "registry_proxy_validation/harness.rs"]
mod harness;
#[path = "registry_proxy_validation/mounts.rs"]
mod mounts;
#[path = "registry_proxy_validation/names.rs"]
mod names;
#[path = "registry_proxy_validation/path_encoding.rs"]
mod path_encoding;
