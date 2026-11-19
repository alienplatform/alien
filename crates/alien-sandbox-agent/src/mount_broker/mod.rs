//! The mount broker: the only process that mounts on behalf of an image that declared
//! `mounts` in its image permissions ([`crate::image_permissions`]).
//!
//! The image never holds `SYS_ADMIN`. The broker is forked at boot while the agent is still
//! single-threaded and still holds its full grant, before `restrict_supervisor`. It keeps only the
//! capabilities it needs, opens no network sockets (a seccomp filter refuses every family but
//! `AF_UNIX`), and runs one loop on one thread:
//!
//! * `/run/alien/mount.sock` (root-only, in the image's mount namespace): one JSON request per
//!   connection from a root process in the image cgroup, checked against the declaration
//!   ([`request::plan`]), then carried out by a helper forked for that request alone. The helper
//!   joins the image's mount namespace with `setns`, resolves paths with
//!   `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS)`, mounts with the new mount API onto
//!   the opened directory, and checks by mount ID that the declared path now shows that mount.
//!   Every request has a deadline; a helper that misses it is killed and the image gets
//!   `ETIMEDOUT`. Only mounts the broker made (by mount ID) may be unmounted.
//! * a join socket the agent's image-spawn path uses between `fork` and `exec`: the broker moves
//!   the sender (identified by the kernel, `SCM_CREDENTIALS`) into the image cgroup. The guest's
//!   cgroup mount is read-only and the agent no longer holds `SYS_ADMIN`, so the broker, which
//!   keeps its own writable view, does the move.
//! * a control socket the agent uses to hand over the listening socket and the image's mount
//!   namespace once they exist.

#[cfg(target_os = "linux")]
mod ops;
pub mod request;
pub mod shim;

#[cfg(target_os = "linux")]
mod process;

#[cfg(all(test, target_os = "linux"))]
mod integration_tests;

#[cfg(target_os = "linux")]
pub use process::*;

/// Where the image reaches the broker.
pub const SOCKET_PATH: &str = "/run/alien/mount.sock";
