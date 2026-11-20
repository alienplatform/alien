//! What a sandbox's own image may do beyond the bounded root grant every root image gets.
//!
//! A root image never holds `SYS_ADMIN` and its sockets go through the deployment gateway. Some
//! images need two narrow exceptions to that: mounting a network file system or a sized tmpfs at
//! a declared path, and reaching a declared private service directly. Both are declared here, on
//! the sandbox, so setup shows them to the customer who approves the deployment, and nothing a
//! caller sends at runtime can widen them.
//!
//! The agent enforces the declaration: it performs only the declared mounts on the image's behalf
//! and exempts only the declared connections from the gateway redirect. Deployment narrows each
//! connection to the private addresses the resource it names publishes, and the sandbox's network
//! boundary still bounds everything.

use crate::error::{ErrorData, Result};
use crate::resource::ResourceRef;
use crate::Platform;
use alien_error::AlienError;
use serde::{Deserialize, Serialize};

/// A file system type an image may ask the agent to mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum SandboxImageMountType {
    /// An NFS v4.1 or v4.2 mount; on AWS through the image's local TLS proxy.
    Nfs,
    /// Memory-backed scratch space. Requires `size`.
    Tmpfs,
    /// Another declared mount, or a directory inside one, placed again at `path`. Requires
    /// `from`.
    Bind,
}

impl SandboxImageMountType {
    /// The name the declaration and the agent's contract use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nfs => "nfs",
            Self::Tmpfs => "tmpfs",
            Self::Bind => "bind",
        }
    }
}

/// One mount an image may ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxImageMount {
    /// File system type.
    #[serde(rename = "type")]
    pub mount_type: SandboxImageMountType,
    /// Absolute, normalized path the mount is placed at. It may not be or contain a system path
    /// (`/`, `/opt/alien`, `/proc`, `/sys`, `/dev`, `/run`, `/etc`, `/usr`, `/bin`, `/sbin`,
    /// `/boot`, `/lib*`).
    pub path: String,
    /// Size of a tmpfs, as a whole quantity such as `8Gi` or `512Mi`. Required for tmpfs and
    /// refused for other types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// Source of a bind mount: the path of a declared `tmpfs` mount. The
    /// bind's own path must be inside a declared `nfs` mount. Required for bind and refused for
    /// other types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// A private service the image's own sockets may reach without the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxImageConnection {
    /// The stack resource whose private network endpoints the connection reaches. Never a raw
    /// address: deployment resolves it from the resource.
    pub to: ResourceRef,
    /// TCP port. Never 53.
    pub port: u16,
}

/// What a sandbox's image may do beyond the default root image grant.
///
/// Connections bypass the deployment gateway and its connection log, and isolation between the
/// workloads inside the image stays the image's responsibility; setup says both to the customer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxImagePermissions {
    /// Mounts the image may ask the agent for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<SandboxImageMount>,
    /// Private services the image's sockets may reach directly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<SandboxImageConnection>,
}

/// Where a resource's private network endpoints are, in terms deployment can resolve to an
/// address range before the resource itself exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum PrivateEndpointPlacement {
    /// Inside the stack's own network. Deployment resolves it to the network's address range.
    StackNetwork,
}

/// A resource that serves at private addresses a sandbox image may connect to directly.
///
/// Implemented by resource types whose endpoints live inside the stack's network. An extension's
/// resource type declares it through its descriptor's `privateNetworkEndpoints`.
pub trait PrivateNetworkEndpoints {
    /// Where the endpoints are.
    fn private_endpoint_placement(&self) -> PrivateEndpointPlacement;
}

/// Paths a mount may neither be nor contain nor sit under. `/lib*` is matched by prefix on the
/// first segment, which covers `/lib`, `/lib32`, `/lib64` and `/libx32`.
const PROTECTED_PATHS: &[&str] = &[
    "/opt/alien",
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/boot",
];

/// Largest tmpfs a declaration may name: 1 TiB, far above any MicroVM's memory, so the value
/// fits the agent's contract and a typo of a unit is still refused.
const MAX_TMPFS_BYTES: u64 = 1 << 40;

impl SandboxImagePermissions {
    /// Refuses a declaration that is malformed on its own: bad paths, sizes, duplicate entries
    /// and forbidden ports. Whether each connection's target exists and has private endpoints is
    /// a stack question, see [`crate::sandbox_image_permissions::validate_connections`].
    pub fn validate(&self, resource_id: &str) -> Result<()> {
        let invalid = |field: &str, value: &str, reason: &str| {
            AlienError::new(ErrorData::SandboxLimitInvalid {
                resource_id: resource_id.to_string(),
                field: field.to_string(),
                value: value.to_string(),
                reason: reason.to_string(),
            })
        };
        if self.mounts.is_empty() && self.connections.is_empty() {
            return Err(invalid(
                "imagePermissions",
                "{}",
                "declare at least one mount or connection, or leave imagePermissions out",
            ));
        }
        let mut paths = std::collections::BTreeSet::new();
        for mount in &self.mounts {
            if let Err(reason) = check_mount_path(&mount.path) {
                return Err(invalid(
                    "imagePermissions.mounts.path",
                    &mount.path,
                    &reason,
                ));
            }
            if !paths.insert(mount.path.as_str()) {
                return Err(invalid(
                    "imagePermissions.mounts.path",
                    &mount.path,
                    "declared more than once",
                ));
            }
            let refuse = |field: &str, value: &str, reason: &str| {
                Err(invalid(
                    &format!("imagePermissions.mounts.{field}"),
                    value,
                    reason,
                ))
            };
            match (mount.mount_type, &mount.size, &mount.from) {
                (SandboxImageMountType::Tmpfs, None, _) => {
                    return refuse(
                        "size",
                        &mount.path,
                        "a tmpfs mount needs a size, such as \"8Gi\"",
                    )
                }
                (SandboxImageMountType::Tmpfs, Some(size), None) => {
                    tmpfs_size_bytes(size)
                        .map_err(|reason| invalid("imagePermissions.mounts.size", size, &reason))?;
                }
                (SandboxImageMountType::Nfs | SandboxImageMountType::Bind, Some(size), _) => {
                    return refuse("size", size, "only a tmpfs mount takes a size")
                }
                (SandboxImageMountType::Bind, None, None) => {
                    return refuse(
                        "from",
                        &mount.path,
                        "a bind mount needs `from`, the declared mount it places again",
                    )
                }
                (SandboxImageMountType::Bind, None, Some(from)) => {
                    if let Err(reason) = check_mount_path(from) {
                        return refuse("from", from, &reason);
                    }
                    // Only a tmpfs the agent itself mounted, exactly at its path (the agent takes
                    // nothing else), so a bind can never expose a path of the image's own file
                    // system (or one shared file system at a second place) somewhere the
                    // declaration did not name.
                    let source = self.mounts.iter().find(|source| {
                        source.mount_type == SandboxImageMountType::Tmpfs && from == &source.path
                    });
                    if source.is_none() {
                        return refuse("from", from, "must be the path of a declared tmpfs mount");
                    }
                    // Only into a declared nfs mount: the bind exists to place local scratch
                    // inside the shared file system, not to cover any other path of the image.
                    let target = self.mounts.iter().any(|target| {
                        target.mount_type == SandboxImageMountType::Nfs
                            && is_under(&mount.path, &target.path)
                    });
                    if !target {
                        return refuse(
                            "path",
                            &mount.path,
                            "a bind must be placed inside a declared nfs mount",
                        );
                    }
                    if is_same_or_under(&mount.path, from) {
                        return refuse(
                            "from",
                            from,
                            "a bind cannot be placed inside its own source",
                        );
                    }
                }
                (SandboxImageMountType::Nfs | SandboxImageMountType::Tmpfs, _, Some(from)) => {
                    return refuse("from", from, "only a bind mount takes `from`")
                }
                (SandboxImageMountType::Nfs, None, None) => {}
            }
        }
        let mut connections = std::collections::HashSet::new();
        for connection in &self.connections {
            let value = format!("{}:{}", connection.to.id, connection.port);
            if connection.port == 0 {
                return Err(invalid(
                    "imagePermissions.connections.port",
                    &value,
                    "port 0 names no service",
                ));
            }
            if connection.port == 53 {
                return Err(invalid(
                    "imagePermissions.connections.port",
                    &value,
                    "DNS always goes through the agent's resolver; port 53 cannot be a direct connection",
                ));
            }
            if !connections.insert((&connection.to, connection.port)) {
                return Err(invalid(
                    "imagePermissions.connections",
                    &value,
                    "declared more than once",
                ));
            }
        }
        Ok(())
    }
}

/// Checks that `path` is absolute, normalized and clear of every system path.
fn check_mount_path(path: &str) -> std::result::Result<(), String> {
    if !path.starts_with('/') {
        return Err("must be an absolute path".into());
    }
    if path == "/" {
        return Err("cannot mount over the root file system".into());
    }
    if path.len() > 1024 {
        return Err("longer than 1024 bytes".into());
    }
    if !path
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
    {
        return Err("may contain only letters, digits, '/', '-', '_' and '.'".into());
    }
    let segments: Vec<&str> = path[1..].split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(
            "must be normalized: no trailing '/', no '//', and no '.' or '..' segments".into(),
        );
    }
    if segments[0].starts_with("lib") {
        return Err(format!("overlaps the system path /{}", segments[0]));
    }
    for protected in PROTECTED_PATHS {
        if is_same_or_under(path, protected) || is_same_or_under(protected, path) {
            return Err(format!("overlaps the system path {protected}"));
        }
    }
    Ok(())
}

/// Whether `path` is `ancestor` or below it, compared by whole segments.
fn is_same_or_under(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether `path` is strictly below `ancestor`, compared by whole segments.
fn is_under(path: &str, ancestor: &str) -> bool {
    path != ancestor && is_same_or_under(path, ancestor)
}

/// A tmpfs size in bytes, from a whole quantity with an optional unit suffix.
pub fn tmpfs_size_bytes(size: &str) -> std::result::Result<u64, String> {
    let digits_end = size
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(size.len());
    let (number, suffix) = size.split_at(digits_end);
    let unit: u64 = match suffix {
        "" => 1,
        "k" => 1_000,
        "M" => 1_000_000,
        "G" => 1_000_000_000,
        "T" => 1_000_000_000_000,
        "Ki" => 1 << 10,
        "Mi" => 1 << 20,
        "Gi" => 1 << 30,
        "Ti" => 1 << 40,
        _ => {
            return Err(
                "must be a whole number with an optional unit (k, M, G, T, Ki, Mi, Gi, Ti)".into(),
            )
        }
    };
    let number: u64 = number
        .parse()
        .map_err(|_| "must start with a whole number, such as \"8Gi\"".to_string())?;
    let bytes = number
        .checked_mul(unit)
        .filter(|bytes| *bytes <= MAX_TMPFS_BYTES)
        .ok_or_else(|| "must be at most 1Ti".to_string())?;
    if bytes < 1 << 20 {
        return Err("must be at least 1Mi".into());
    }
    Ok(bytes)
}

/// Refuses an argv for a lifecycle step that the agent could not run unambiguously.
pub(crate) fn validate_lifecycle_argv(
    resource_id: &str,
    field: &str,
    argv: &[String],
) -> Result<()> {
    let invalid = |value: &str, reason: &str| {
        AlienError::new(ErrorData::SandboxLimitInvalid {
            resource_id: resource_id.to_string(),
            field: field.to_string(),
            value: value.to_string(),
            reason: reason.to_string(),
        })
    };
    let Some(program) = argv.first() else {
        return Err(invalid("[]", "needs a program to run"));
    };
    if !program.starts_with('/') {
        return Err(invalid(
            program,
            "the program must be an absolute path inside the image; there is no PATH lookup",
        ));
    }
    if argv
        .iter()
        .any(|argument| argument.contains('\0') || argument.len() > 4096)
    {
        return Err(invalid(
            program,
            "arguments may not contain NUL or exceed 4096 bytes",
        ));
    }
    if argv.len() > 64 {
        return Err(invalid(program, "at most 64 arguments"));
    }
    Ok(())
}

/// The only platform image permissions and lifecycle steps run on in this release.
pub(crate) fn require_image_permissions_platform(
    capability: &str,
    platform: Platform,
) -> Result<()> {
    if platform == Platform::Aws {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::SandboxCapabilityUnsupported {
        capability: format!("{capability} (available on AWS Lambda MicroVM sandboxes only)"),
        platform: platform.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceType;

    fn mount(
        mount_type: SandboxImageMountType,
        path: &str,
        size: Option<&str>,
    ) -> SandboxImageMount {
        SandboxImageMount {
            mount_type,
            path: path.into(),
            size: size.map(Into::into),
            from: None,
        }
    }

    fn connection(id: &str, port: u16) -> SandboxImageConnection {
        SandboxImageConnection {
            to: ResourceRef::new(ResourceType::from("acme-files"), id),
            port,
        }
    }

    fn permissions(
        mounts: Vec<SandboxImageMount>,
        connections: Vec<SandboxImageConnection>,
    ) -> SandboxImagePermissions {
        SandboxImagePermissions {
            mounts,
            connections,
        }
    }

    #[test]
    fn the_designed_declaration_is_accepted() {
        permissions(
            vec![
                mount(SandboxImageMountType::Nfs, "/workspace", None),
                mount(SandboxImageMountType::Tmpfs, "/workspace/tmp", Some("8Gi")),
            ],
            vec![connection("workspace-files", 2049)],
        )
        .validate("sandboxes")
        .unwrap();
    }

    #[test]
    fn system_and_unnormalized_paths_are_refused() {
        for path in [
            "/",
            "workspace",
            "/workspace/",
            "/workspace//tmp",
            "/workspace/./tmp",
            "/workspace/../etc",
            "/opt/alien",
            "/opt/alien/bin",
            "/opt",
            "/proc",
            "/proc/sys",
            "/sys",
            "/dev/shm",
            "/run",
            "/run/alien",
            "/etc",
            "/usr/local",
            "/bin",
            "/sbin",
            "/lib",
            "/lib64",
            "/libx32/foo",
            "/boot",
            "/boot/efi",
            "/work space",
            "/work,space",
        ] {
            let error = permissions(vec![mount(SandboxImageMountType::Nfs, path, None)], vec![])
                .validate("sandboxes")
                .expect_err(path);
            assert!(error.to_string().contains("mounts.path"), "{path}: {error}");
        }
        // Siblings that only share a prefix with a system path are fine.
        for path in [
            "/procedures",
            "/opt/alienware",
            "/devices",
            "/running",
            "/opt/data",
        ] {
            permissions(vec![mount(SandboxImageMountType::Nfs, path, None)], vec![])
                .validate("sandboxes")
                .unwrap_or_else(|error| panic!("{path}: {error}"));
        }
    }

    #[test]
    fn tmpfs_needs_a_size_and_nfs_takes_none() {
        assert!(permissions(
            vec![mount(SandboxImageMountType::Tmpfs, "/scratch", None)],
            vec![]
        )
        .validate("s")
        .is_err());
        assert!(permissions(
            vec![mount(SandboxImageMountType::Nfs, "/scratch", Some("1Gi"))],
            vec![]
        )
        .validate("s")
        .is_err());
        for size in ["8", "1.5Gi", "8gb", "2Ti", "", "Gi", "512Ki"] {
            assert!(
                permissions(
                    vec![mount(SandboxImageMountType::Tmpfs, "/scratch", Some(size))],
                    vec![]
                )
                .validate("s")
                .is_err(),
                "{size}"
            );
        }
        assert_eq!(tmpfs_size_bytes("8Gi").unwrap(), 8_589_934_592);
        assert_eq!(tmpfs_size_bytes("512Mi").unwrap(), 536_870_912);
        assert_eq!(tmpfs_size_bytes("2G").unwrap(), 2_000_000_000);
    }

    fn bind(from: &str, path: &str) -> SandboxImageMount {
        SandboxImageMount {
            mount_type: SandboxImageMountType::Bind,
            path: path.into(),
            size: None,
            from: Some(from.into()),
        }
    }

    /// The traced layout: a tmpfs outside the NFS mount, bound into it after each NFS mount.
    #[test]
    fn a_bind_places_a_declared_mount_again_and_nothing_else() {
        let tmpfs = || {
            mount(
                SandboxImageMountType::Tmpfs,
                "/var/lib/app/scratch",
                Some("8Gi"),
            )
        };
        let nfs = || mount(SandboxImageMountType::Nfs, "/workspace", None);
        permissions(
            vec![
                nfs(),
                tmpfs(),
                bind("/var/lib/app/scratch", "/workspace/tmp"),
            ],
            vec![],
        )
        .validate("s")
        .unwrap();

        for (mounts, why) in [
            (
                vec![
                    nfs(),
                    tmpfs(),
                    bind("/var/lib/app/scratch/a", "/workspace/a"),
                ],
                "source inside a tmpfs rather than the tmpfs itself (the agent takes only a \
                 declared mount path)",
            ),
            (
                vec![nfs(), bind("/var/lib/app/scratch", "/workspace/tmp")],
                "undeclared source",
            ),
            (vec![nfs(), bind("/etc", "/workspace/etc")], "system source"),
            (
                vec![tmpfs(), bind("/var/lib/app/scratch", "/data")],
                "target outside any nfs mount",
            ),
            (
                vec![nfs(), tmpfs(), bind("/var/lib/app/scratch", "/workspace")],
                "target is the nfs mount itself",
            ),
            (
                vec![
                    nfs(),
                    mount(SandboxImageMountType::Nfs, "/shared", None),
                    bind("/shared", "/workspace/shared"),
                ],
                "nfs source",
            ),
            (
                vec![tmpfs(), bind("/var/lib/app", "/data")],
                "parent of a mount",
            ),
            (
                vec![
                    tmpfs(),
                    bind("/var/lib/app/scratch", "/var/lib/app/scratch/x"),
                ],
                "inside itself",
            ),
            (
                vec![
                    nfs(),
                    tmpfs(),
                    bind("/var/lib/app/scratch", "/workspace/a"),
                    bind("/workspace/a", "/workspace/b"),
                ],
                "from a bind",
            ),
            (
                vec![SandboxImageMount {
                    mount_type: SandboxImageMountType::Bind,
                    path: "/x".into(),
                    size: None,
                    from: None,
                }],
                "no from",
            ),
            (
                vec![SandboxImageMount {
                    from: Some("/workspace".into()),
                    ..nfs()
                }],
                "from on nfs",
            ),
        ] {
            assert!(permissions(mounts, vec![]).validate("s").is_err(), "{why}");
        }
    }

    #[test]
    fn port_53_zero_and_duplicates_are_refused() {
        for connections in [
            vec![connection("files", 53)],
            vec![connection("files", 0)],
            vec![connection("files", 2049), connection("files", 2049)],
        ] {
            assert!(permissions(vec![], connections).validate("s").is_err());
        }
        assert!(permissions(
            vec![
                mount(SandboxImageMountType::Nfs, "/a", None),
                mount(SandboxImageMountType::Nfs, "/a", None)
            ],
            vec![]
        )
        .validate("s")
        .is_err());
        assert!(permissions(vec![], vec![]).validate("s").is_err());
    }

    #[test]
    fn unknown_fields_are_refused_so_an_older_reader_cannot_drop_them() {
        for json in [
            r#"{"mounts":[{"type":"nfs","path":"/w","readOnly":true}]}"#,
            r#"{"mounts":[],"capabilities":["SYS_ADMIN"]}"#,
            r#"{"connections":[{"to":{"type":"x","id":"y"},"port":2049,"cidr":"0.0.0.0/0"}]}"#,
            r#"{"mounts":[{"type":"overlay","path":"/w"}]}"#,
        ] {
            serde_json::from_str::<SandboxImagePermissions>(json).expect_err(json);
        }
    }

    #[test]
    fn lifecycle_argv_must_name_an_absolute_program() {
        validate_lifecycle_argv("s", "onPause", &["/usr/bin/app".into(), "pause".into()]).unwrap();
        assert!(validate_lifecycle_argv("s", "onPause", &[]).is_err());
        assert!(validate_lifecycle_argv("s", "onPause", &["app".into()]).is_err());
        assert!(validate_lifecycle_argv("s", "onPause", &["/a".into(), "b\0".into()]).is_err());
    }

    fn declared_sandbox() -> crate::Sandbox {
        let mut sandbox = crate::Sandbox::new("sandboxes".to_string())
            .code(crate::SandboxCode::Image {
                image: "s3://bundles/sandboxes.zip".to_string(),
                bundled_from: None,
            })
            .egress(crate::SandboxEgress::Internet)
            .lifecycle(crate::SandboxLifecyclePolicy::default())
            .build();
        sandbox.image_permissions = Some(permissions(
            vec![mount(SandboxImageMountType::Nfs, "/workspace", None)],
            vec![connection("workspace-files", 2049)],
        ));
        sandbox
    }

    #[test]
    fn only_aws_accepts_image_permissions_and_the_refusal_names_the_platform() {
        declared_sandbox()
            .validate_for_platform(Platform::Aws)
            .unwrap();
        for platform in [
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let error = declared_sandbox()
                .validate_for_platform(platform)
                .expect_err("refused off AWS");
            assert!(error.to_string().contains("imagePermissions"), "{error}");
            assert!(error.to_string().contains(&platform.to_string()), "{error}");
        }
        let mut steps_only = declared_sandbox();
        steps_only.image_permissions = None;
        steps_only.lifecycle.on_resume = Some(vec!["/bin/true".into()]);
        steps_only.validate_for_platform(Platform::Aws).unwrap();
        let error = steps_only.validate_for_platform(Platform::Gcp).unwrap_err();
        assert!(error.to_string().contains("onResume"), "{error}");

        let mut pause_only = declared_sandbox();
        pause_only.image_permissions = None;
        pause_only.lifecycle.on_pause = Some(vec!["/bin/sync".into()]);
        pause_only.validate_for_platform(Platform::Aws).unwrap();
        for platform in [Platform::Azure, Platform::Kubernetes, Platform::Local] {
            let error = pause_only
                .validate_for_platform(platform)
                .expect_err("onPause refused off AWS");
            assert!(error.to_string().contains("onPause"), "{error}");
            assert!(error.to_string().contains(&platform.to_string()), "{error}");
        }
    }

    /// The agent serves image permissions and lifecycle steps only on the guest network
    /// (egress `internet` or `none`) and only to an image whose command starts as root, so
    /// planning refuses anything else instead of an image that never starts.
    #[test]
    fn image_permissions_need_guest_network_egress_and_a_root_image() {
        let steps_only = || {
            let mut sandbox = declared_sandbox();
            sandbox.image_permissions = None;
            sandbox.lifecycle.on_pause = Some(vec!["/bin/sync".into()]);
            sandbox
        };
        for declared in [declared_sandbox, steps_only] {
            for egress in [crate::SandboxEgress::Internet, crate::SandboxEgress::None] {
                let mut sandbox = declared();
                sandbox.egress = egress;
                sandbox.validate_for_platform(Platform::Aws).unwrap();
                sandbox.image_user = Some(0);
                sandbox.validate_for_platform(Platform::Aws).unwrap();
            }
            for egress in [
                crate::SandboxEgress::Allow,
                crate::SandboxEgress::AllowDomains {
                    domains: vec!["example.com".into()],
                },
            ] {
                let mut sandbox = declared();
                sandbox.egress = egress;
                let error = sandbox
                    .validate_for_platform(Platform::Aws)
                    .expect_err("egress without the guest network");
                assert!(error.to_string().contains("egress"), "{error}");
            }
            let mut sandbox = declared();
            sandbox.image_user = Some(1000);
            let error = sandbox
                .validate_for_platform(Platform::Aws)
                .expect_err("a non-root image");
            assert!(error.to_string().contains("root"), "{error}");
        }
    }

    #[test]
    fn a_sandbox_without_image_permissions_serializes_as_before() {
        let mut sandbox = declared_sandbox();
        sandbox.image_permissions = None;
        let json = serde_json::to_value(&sandbox).unwrap();
        assert!(json.get("imagePermissions").is_none());
        assert_eq!(json["lifecycle"], serde_json::json!({}));

        let declared = serde_json::to_value(declared_sandbox()).unwrap();
        assert_eq!(
            declared["imagePermissions"],
            serde_json::json!({
                "mounts": [{"type": "nfs", "path": "/workspace"}],
                "connections": [{"to": {"type": "acme-files", "id": "workspace-files"}, "port": 2049}]
            })
        );
        let read: crate::Sandbox = serde_json::from_value(declared).unwrap();
        assert_eq!(read, declared_sandbox());
    }
}
