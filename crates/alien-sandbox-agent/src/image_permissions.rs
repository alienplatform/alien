//! What a sandbox image may do beyond the bounded root grant: mount declared file systems
//! through the agent's mount broker, reach declared private destinations without the gateway
//! redirect, and run declared steps around a pause.
//!
//! Carried as JSON in `ALIEN_SANDBOX_IMAGE_PERMISSIONS`, only on images that declare it, and
//! only together with `ALIEN_SANDBOX_IMAGE_COMMAND=image-permissions-v1` and
//! `ALIEN_SANDBOX_IMAGE_PERMISSIONS_V=1`. An agent that predates the declaration accepts only
//! `ALIEN_SANDBOX_IMAGE_COMMAND=true`, so it refuses to start rather than ignore it.
//!
//! The image never holds `SYS_ADMIN`. Mounts are made by the broker ([`crate::mount_broker`]),
//! which checks every request against this declaration.

use std::{net::Ipv4Addr, sync::OnceLock};

use alien_error::AlienError;
use serde::Deserialize;

use crate::error::{ErrorData, Result};

// One definition, shared with deployment, which writes these variables.
pub use alien_core::sandbox_image::{
    ENV_IMAGE_COMMAND, ENV_IMAGE_PERMISSIONS as ENV_PERMISSIONS,
    ENV_IMAGE_PERMISSIONS_VERSION as ENV_PERMISSIONS_VERSION,
};
/// Seconds a lifecycle step may run before it is killed (1-40, default 40). The step, then the
/// NFS detach (at most [`crate::mount_broker::DETACH_DEADLINE`]), fit a 60 s provider hook.
pub const ENV_LIFECYCLE_DEADLINE: &str = "ALIEN_SANDBOX_LIFECYCLE_DEADLINE_SECONDS";
pub const DEFAULT_LIFECYCLE_DEADLINE_SECONDS: u64 = 40;
pub const MAX_LIFECYCLE_DEADLINE_SECONDS: u64 = 40;

/// Paths no declared mount may be at, under or above: the agent's own files, the system
/// directories and the broker's socket.
const RESERVED: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/boot",
    "/opt/alien",
    "/run/alien",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MountType {
    Nfs,
    Tmpfs,
    /// A bind from a declared nfs or tmpfs path (or below it) onto this path.
    Bind,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredMount {
    #[serde(rename = "type")]
    pub kind: MountType,
    pub path: String,
    /// The largest tmpfs the image may ask for. Required for tmpfs, refused for nfs.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// For a bind: the declared nfs or tmpfs path it binds from.
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredConnection {
    /// One private IPv4 host (`10.0.1.5` or `10.0.1.5/32`).
    pub cidr: String,
    /// TCP port.
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImagePermissions {
    #[serde(default)]
    pub mounts: Vec<DeclaredMount>,
    #[serde(default)]
    pub connections: Vec<DeclaredConnection>,
    #[serde(default)]
    pub on_pause: Option<Vec<String>>,
    #[serde(default)]
    pub on_resume: Option<Vec<String>>,
}

/// What the agent applies.
#[derive(Debug, Clone)]
pub struct Declared {
    pub permissions: ImagePermissions,
    /// (network, prefix, port), TCP only.
    pub destinations: Vec<(Ipv4Addr, u8, u16)>,
    pub lifecycle_deadline: std::time::Duration,
}

impl Declared {
    pub fn mount(&self, path: &str) -> Option<&DeclaredMount> {
        self.permissions.mounts.iter().find(|m| m.path == path)
    }
}

static DECLARED: OnceLock<Option<Declared>> = OnceLock::new();

/// Reads the declaration from the environment once. `Ok(None)` when the image declares none,
/// in which case nothing about the agent changes.
pub fn load_from_env() -> Result<Option<&'static Declared>> {
    let get = |name: &str| match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(invalid(name, "is not valid UTF-8")),
    };
    // The platform sets these for every sandbox (empty when the image has no permissions). A
    // value that is only the image's own (its OCI config `Env`, which the platform's would
    // override) grants nothing.
    let image_env = image_config_env()?;
    let platform =
        |name: &str| -> Result<Option<String>> { Ok(from_platform(name, get(name)?, &image_env)) };
    let parsed = from_values(
        get(ENV_IMAGE_COMMAND)?.as_deref(),
        platform(ENV_PERMISSIONS_VERSION)?.as_deref(),
        platform(ENV_PERMISSIONS)?.as_deref(),
        platform(ENV_LIFECYCLE_DEADLINE)?.as_deref(),
    )?;
    Ok(DECLARED.get_or_init(|| parsed).as_ref())
}

/// `value` unless it is empty or the image's own (`image_env`, from its OCI config).
fn from_platform(
    name: &str,
    value: Option<String>,
    image_env: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    value.filter(|value| {
        !value.is_empty() && image_env.get(name).map(String::as_str) != Some(value.as_str())
    })
}

/// The image's own environment from its saved OCI config; empty without one.
pub(crate) fn image_config_env() -> Result<std::collections::BTreeMap<String, String>> {
    match std::fs::read(alien_core::sandbox_image::IMAGE_COMMAND_PATH) {
        Ok(bytes) => {
            serde_json::from_slice::<alien_core::sandbox_image::SandboxImageCommand>(&bytes)
                .map(|command| command.env)
                .map_err(|error| {
                    invalid(
                        alien_core::sandbox_image::IMAGE_COMMAND_PATH,
                        &error.to_string(),
                    )
                })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(error) => Err(invalid(
            alien_core::sandbox_image::IMAGE_COMMAND_PATH,
            &error.to_string(),
        )),
    }
}

/// The declaration, once loaded.
pub fn declared() -> Option<&'static Declared> {
    DECLARED.get().and_then(Option::as_ref)
}

/// The marker rules, then the declaration itself.
pub fn from_values(
    image_command: Option<&str>,
    version: Option<&str>,
    permissions: Option<&str>,
    deadline: Option<&str>,
) -> Result<Option<Declared>> {
    let marked =
        image_command == Some(alien_core::sandbox_image::IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS);
    if !marked {
        if permissions.is_some() || version.is_some() {
            return Err(invalid(
                ENV_PERMISSIONS,
                &format!(
                    "is set without {ENV_IMAGE_COMMAND}={}",
                    alien_core::sandbox_image::IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS
                ),
            ));
        }
        return Ok(None);
    }
    if version != Some("1") {
        return Err(invalid(
            ENV_PERMISSIONS_VERSION,
            "must be 1; this agent understands only version 1 of the declaration",
        ));
    }
    let permissions = permissions.ok_or_else(|| invalid(ENV_PERMISSIONS, "is required"))?;
    let mut declared = parse(permissions)?;
    if let Some(deadline) = deadline {
        let seconds: u64 = deadline
            .parse()
            .ok()
            .filter(|s| (1..=MAX_LIFECYCLE_DEADLINE_SECONDS).contains(s))
            .ok_or_else(|| invalid(ENV_LIFECYCLE_DEADLINE, "must be 1 to 40 seconds"))?;
        declared.lifecycle_deadline = std::time::Duration::from_secs(seconds);
    }
    Ok(Some(declared))
}

pub fn parse(value: &str) -> Result<Declared> {
    let permissions: ImagePermissions = serde_json::from_str(value)
        .map_err(|error| invalid(ENV_PERMISSIONS, &error.to_string()))?;
    let destinations =
        validate(&permissions).map_err(|reason| invalid(ENV_PERMISSIONS, &reason))?;
    Ok(Declared {
        permissions,
        destinations,
        lifecycle_deadline: std::time::Duration::from_secs(DEFAULT_LIFECYCLE_DEADLINE_SECONDS),
    })
}

fn validate(
    permissions: &ImagePermissions,
) -> std::result::Result<Vec<(Ipv4Addr, u8, u16)>, String> {
    let mut seen = std::collections::BTreeSet::new();
    for mount in &permissions.mounts {
        check_path(&mount.path)?;
        if !seen.insert(mount.path.as_str()) {
            return Err(format!("'{}' is declared twice", mount.path));
        }
        match (mount.kind, mount.size_bytes) {
            (MountType::Tmpfs, None | Some(0)) => {
                return Err(format!("the tmpfs at '{}' needs sizeBytes", mount.path))
            }
            (MountType::Nfs | MountType::Bind, Some(_)) => {
                return Err(format!("the mount at '{}' takes no sizeBytes", mount.path))
            }
            _ => {}
        }
        match (mount.kind, &mount.from) {
            (MountType::Bind, None) => {
                return Err(format!("the bind at '{}' needs from", mount.path))
            }
            (MountType::Bind, Some(from)) => {
                let source = permissions.mounts.iter().find(|m| &m.path == from);
                if !source.is_some_and(|m| m.kind != MountType::Bind) {
                    return Err(format!(
                        "the bind at '{}' must come from a declared nfs or tmpfs path",
                        mount.path
                    ));
                }
            }
            (_, Some(_)) => return Err(format!("only a bind takes from ('{}')", mount.path)),
            _ => {}
        }
    }
    let mut destinations = Vec::new();
    for connection in &permissions.connections {
        if connection.port == 0 || connection.port == 53 {
            return Err(format!(
                "port {} cannot be a direct connection",
                connection.port
            ));
        }
        let (network, prefix) = parse_private_cidr(&connection.cidr)?;
        destinations.push((network, prefix, connection.port));
    }
    for (name, command) in [
        ("onPause", &permissions.on_pause),
        ("onResume", &permissions.on_resume),
    ] {
        if let Some(command) = command {
            if command.first().is_none_or(|program| program.is_empty()) {
                return Err(format!("{name} needs a program"));
            }
            if command.iter().any(|argument| argument.contains('\0')) {
                return Err(format!("{name} has an argument with a NUL byte"));
            }
        }
    }
    Ok(destinations)
}

/// A plain absolute path, not `/`, and not at, under or above a reserved path.
pub fn check_path(path: &str) -> std::result::Result<(), String> {
    check_clean(path)?;
    check_not_reserved(path)
}

/// A plain absolute path other than `/`: no empty, `.` or `..` component.
pub fn check_clean(path: &str) -> std::result::Result<(), String> {
    if !path.starts_with('/')
        || path.len() == 1
        || path.len() > 1024
        || path
            .split('/')
            .skip(1)
            .any(|c| c.is_empty() || c == "." || c == "..")
        || path.contains('\0')
    {
        return Err(format!("'{path}' is not a plain absolute path"));
    }
    Ok(())
}

fn check_not_reserved(path: &str) -> std::result::Result<(), String> {
    let within =
        |inner: &str, outer: &str| inner == outer || inner.starts_with(&format!("{outer}/"));
    if RESERVED
        .iter()
        .any(|reserved| within(path, reserved) || within(reserved, path))
    {
        return Err(format!("'{path}' is at, under or above a system path"));
    }
    Ok(())
}

/// One private (RFC 1918) IPv4 host: `10.0.1.5` or `10.0.1.5/32`. A connection names the
/// resolved address of the resource it reaches, never a network.
pub fn parse_private_cidr(cidr: &str) -> std::result::Result<(Ipv4Addr, u8), String> {
    let (address, prefix) = match cidr.split_once('/') {
        Some((address, prefix)) => (
            address,
            prefix
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= 32)
                .ok_or_else(|| format!("'{cidr}' has an invalid prefix"))?,
        ),
        None => (cidr, 32),
    };
    let address: Ipv4Addr = address
        .parse()
        .map_err(|_| format!("'{cidr}' is not an IPv4 address or network"))?;
    if prefix != 32 {
        return Err(format!("'{cidr}' is a network; name one host (/32)"));
    }
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    if u32::from(address) & !mask != 0 {
        return Err(format!("'{cidr}' has host bits set"));
    }
    if !address.is_private() {
        return Err(format!("'{cidr}' is not a private address"));
    }
    Ok((address, prefix))
}

fn invalid(setting: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigInvalid {
        setting: setting.into(),
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: Option<&str> = Some(alien_core::sandbox_image::IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS);
    const CONTRACT: &str = r#"{"mounts":[{"type":"nfs","path":"/workspace"},{"type":"tmpfs","path":"/var/lib/app/scratch","sizeBytes":8589934592},{"type":"bind","from":"/var/lib/app/scratch","path":"/workspace/tmp"}],"connections":[{"cidr":"10.0.1.5/32","port":2049}],"onPause":["/usr/local/bin/app","pause"],"onResume":["/usr/local/bin/app","resume"]}"#;

    /// What deployment writes for a declared sandbox is what this agent accepts: the
    /// environment comes from the same alien-core function the emitters and the controller use.
    #[test]
    fn the_environment_deployment_writes_is_accepted() {
        use alien_core::{
            ResourceRef, ResourceType, Sandbox, SandboxCode, SandboxEgress, SandboxImageConnection,
            SandboxImageMount, SandboxImageMountType, SandboxImagePermissions,
            SandboxLifecyclePolicy,
        };
        let mut sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "s3://bundles/agents.zip".to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                on_pause: Some(vec!["/usr/local/bin/app".into(), "pause".into()]),
                on_resume: Some(vec!["/usr/local/bin/app".into(), "resume".into()]),
                ..Default::default()
            })
            .build();
        // `alien build` records the image's user; deployment needs it to write the contract.
        sandbox.image_user = Some(0);
        sandbox.image_permissions = Some(SandboxImagePermissions {
            mounts: vec![
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Nfs,
                    path: "/workspace".into(),
                    size: None,
                    from: None,
                },
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Tmpfs,
                    path: "/var/lib/app/scratch".into(),
                    size: Some("8Gi".into()),
                    from: None,
                },
                SandboxImageMount {
                    mount_type: SandboxImageMountType::Bind,
                    path: "/workspace/tmp".into(),
                    size: None,
                    from: Some("/var/lib/app/scratch".into()),
                },
            ],
            connections: vec![SandboxImageConnection {
                to: ResourceRef::new(ResourceType::from("acme/files"), "workspace-files"),
                port: 2049,
            }],
        });
        // The target published the address deployment narrows the connection to.
        let published = serde_json::json!({ "privateIpv4Addresses": ["10.0.1.5"] });
        let environment = alien_core::sandbox_image_permissions::image_permissions_environment(
            &sandbox,
            |connection| {
                alien_core::sandbox_image_permissions::published_connection_addresses(
                    &sandbox,
                    connection,
                    Some(&published),
                )
            },
        )
        .unwrap();
        let get = |name: &str| environment.get(name).map(String::as_str);
        let declared = from_values(
            get(ENV_IMAGE_COMMAND),
            get(ENV_PERMISSIONS_VERSION),
            get(ENV_PERMISSIONS),
            None,
        )
        .unwrap()
        .expect("declared");
        let example = parse(CONTRACT).unwrap();
        assert_eq!(declared.permissions, example.permissions);
        assert_eq!(declared.destinations, example.destinations);
    }

    #[test]
    fn the_contract_example_is_accepted() {
        let declared = from_values(V1, Some("1"), Some(CONTRACT), None)
            .unwrap()
            .unwrap();
        assert_eq!(declared.permissions.mounts.len(), 3);
        assert_eq!(
            declared.destinations,
            vec![("10.0.1.5".parse().unwrap(), 32, 2049)]
        );
        assert_eq!(declared.lifecycle_deadline.as_secs(), 40);
        assert_eq!(
            declared.mount("/var/lib/app/scratch").unwrap().size_bytes,
            Some(8 << 30)
        );
        let short = from_values(V1, Some("1"), Some(CONTRACT), Some("5"))
            .unwrap()
            .unwrap();
        assert_eq!(short.lifecycle_deadline.as_secs(), 5);
        let longest = from_values(V1, Some("1"), Some(CONTRACT), Some("40"))
            .unwrap()
            .unwrap();
        assert_eq!(longest.lifecycle_deadline.as_secs(), 40);
        assert!(from_values(V1, Some("1"), Some(CONTRACT), Some("41")).is_err());
        assert!(from_values(V1, Some("1"), Some(CONTRACT), Some("61")).is_err());
    }

    #[test]
    fn only_the_platform_grants_permissions() {
        let image_env: std::collections::BTreeMap<String, String> =
            [(ENV_PERMISSIONS.to_string(), CONTRACT.to_string())].into();
        let set = |value: &str| Some(value.to_string());
        // The image's own config value grants nothing, and neither does the platform's empty one.
        assert_eq!(
            from_platform(ENV_PERMISSIONS, set(CONTRACT), &image_env),
            None
        );
        assert_eq!(from_platform(ENV_PERMISSIONS, set(""), &image_env), None);
        assert_eq!(from_platform(ENV_PERMISSIONS, None, &image_env), None);
        // A value the platform set (different from the image's) stands.
        let platform = r#"{"mounts":[{"type":"nfs","path":"/workspace"}]}"#;
        assert_eq!(
            from_platform(ENV_PERMISSIONS, set(platform), &image_env).as_deref(),
            Some(platform)
        );
        assert_eq!(
            from_platform(ENV_PERMISSIONS, set(CONTRACT), &Default::default()).as_deref(),
            Some(CONTRACT)
        );
    }

    #[test]
    fn absent_means_nothing_changes_and_every_partial_marker_refuses() {
        assert!(from_values(None, None, None, None).unwrap().is_none());
        assert!(from_values(Some("true"), None, None, None)
            .unwrap()
            .is_none());
        // A declaration the marker does not announce, or a marker without its version or body.
        assert!(from_values(Some("true"), Some("1"), Some(CONTRACT), None).is_err());
        assert!(from_values(None, None, Some(CONTRACT), None).is_err());
        assert!(from_values(V1, None, Some(CONTRACT), None).is_err());
        assert!(from_values(V1, Some("2"), Some(CONTRACT), None).is_err());
        assert!(from_values(V1, Some("1"), None, None).is_err());
    }

    #[test]
    fn invalid_declarations_are_refused() {
        for body in [
            r#"{"mounts":[{"type":"nfs","path":"/opt"}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"/opt/alien/x"}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"/"}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"/a/../b"}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"relative"}]}"#,
            r#"{"mounts":[{"type":"tmpfs","path":"/t"}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"/n","sizeBytes":1}]}"#,
            r#"{"mounts":[{"type":"nfs","path":"/n"},{"type":"tmpfs","path":"/n","sizeBytes":1}]}"#,
            r#"{"mounts":[{"type":"ext4","path":"/n"}]}"#,
            r#"{"connections":[{"cidr":"8.8.8.0/24","port":2049}]}"#,
            r#"{"connections":[{"cidr":"10.0.0.0/8","port":2049}]}"#,
            r#"{"connections":[{"cidr":"10.0.1.1/24","port":2049}]}"#,
            r#"{"connections":[{"cidr":"10.0.1.5/32","port":53}]}"#,
            r#"{"connections":[{"cidr":"10.0.1.0/24","port":2049}]}"#,
            r#"{"connections":[{"cidr":"10.0.1.4/31","port":2049}]}"#,
            r#"{"connections":[{"cidr":"127.0.0.1","port":2049}]}"#,
            r#"{"onPause":[]}"#,
            r#"{"mounts":[{"type":"bind","path":"/b"}]}"#,
            r#"{"mounts":[{"type":"bind","from":"/etc","path":"/b"}]}"#,
            r#"{"mounts":[{"type":"bind","from":"/c","path":"/b"},{"type":"bind","from":"/b","path":"/c"}]}"#,
            r#"{"mounts":[{"type":"nfs","from":"/b","path":"/n"}]}"#,
            r#"{"unknown":true}"#,
        ] {
            assert!(parse(body).is_err(), "{body}");
        }
    }
}
