//! The broker's wire format (one JSON request, one JSON reply per connection) and the checks a
//! request passes before anything runs. Everything here is pure, so every allow and refuse case
//! is a unit test.

use serde::{Deserialize, Serialize};

use crate::image_permissions::{Declared, MountType};

/// Ports a loopback NFS proxy may listen on (the range the AWS mount helper's proxy uses).
pub const PROXY_PORTS: std::ops::RangeInclusive<u16> = 20049..=21049;

/// `MOUNT_ATTR_*` from `linux/mount.h`.
pub const MOUNT_ATTR_RDONLY: u64 = 0x1;
pub const MOUNT_ATTR_NOSUID: u64 = 0x2;
pub const MOUNT_ATTR_NODEV: u64 = 0x4;
pub const MOUNT_ATTR_NOEXEC: u64 = 0x8;
pub const MOUNT_ATTR_RELATIME: u64 = 0x0;
pub const MOUNT_ATTR_NOATIME: u64 = 0x10;
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x20;
pub const MOUNT_ATTR_NODIRATIME: u64 = 0x80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Mount {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        source: Option<String>,
        target: String,
        #[serde(default)]
        options: String,
    },
    Bind {
        from: String,
        target: String,
    },
    Umount {
        target: String,
        #[serde(default)]
        flags: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errno: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Reply {
    pub fn ok() -> Self {
        Self {
            ok: true,
            errno: None,
            reason: None,
        }
    }

    pub fn refused(errno: Errno, reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            errno: Some(errno.name().into()),
            reason: Some(reason.into()),
        }
    }
}

/// The errors a reply names. The shims map them to exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errno {
    Eperm,
    Einval,
    Enoent,
    Enotdir,
    Ebusy,
    Etimedout,
    Eio,
    Eloop,
    Exdev,
}

impl Errno {
    pub fn name(self) -> &'static str {
        match self {
            Self::Eperm => "EPERM",
            Self::Einval => "EINVAL",
            Self::Enoent => "ENOENT",
            Self::Enotdir => "ENOTDIR",
            Self::Ebusy => "EBUSY",
            Self::Etimedout => "ETIMEDOUT",
            Self::Eio => "EIO",
            Self::Eloop => "ELOOP",
            Self::Exdev => "EXDEV",
        }
    }

    pub fn from_raw(errno: i32) -> Self {
        match errno {
            libc::EPERM | libc::EACCES => Self::Eperm,
            libc::ENOENT => Self::Enoent,
            libc::ENOTDIR => Self::Enotdir,
            libc::EBUSY => Self::Ebusy,
            libc::ETIMEDOUT => Self::Etimedout,
            libc::ELOOP => Self::Eloop,
            libc::EXDEV => Self::Exdev,
            libc::EINVAL => Self::Einval,
            _ => Self::Eio,
        }
    }
}

/// A request that passed every check that does not need the file system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "plan", rename_all = "camelCase")]
pub enum Plan {
    Nfs {
        target: String,
        source: String,
        /// Passed to `fsconfig`, in order: (key, value) or a flag.
        params: Vec<(String, Option<String>)>,
        attributes: u64,
        port: u16,
    },
    Tmpfs {
        target: String,
        params: Vec<(String, Option<String>)>,
        attributes: u64,
    },
    Bind {
        from: String,
        target: String,
    },
    Umount {
        target: String,
        flags: i32,
    },
}

pub type Refusal = (Errno, String);

fn refuse<T>(reason: impl Into<String>) -> Result<T, Refusal> {
    Err((Errno::Eperm, reason.into()))
}

fn declared_as<'a>(
    declared: &'a Declared,
    target: &str,
    kind: Option<MountType>,
) -> Result<&'a crate::image_permissions::DeclaredMount, Refusal> {
    let mount = declared.mount(target).ok_or_else(|| {
        (
            Errno::Eperm,
            format!("'{target}' is not a declared mount path"),
        )
    })?;
    if let Some(kind) = kind {
        if mount.kind != kind {
            return refuse(format!(
                "'{target}' is declared for {:?}, not {kind:?}",
                mount.kind
            ));
        }
    }
    Ok(mount)
}

/// Checks a request against the declaration.
pub fn plan(declared: &Declared, request: &Request) -> Result<Plan, Refusal> {
    match request {
        Request::Mount {
            kind,
            source,
            target,
            options,
        } => match kind.as_str() {
            "nfs" | "nfs4" => {
                declared_as(declared, target, Some(MountType::Nfs))?;
                let source = source
                    .as_deref()
                    .ok_or_else(|| (Errno::Einval, "an nfs mount needs a source".to_string()))?;
                let nfs = nfs_options(source, options)?;
                Ok(Plan::Nfs {
                    target: target.clone(),
                    source: source.to_string(),
                    params: nfs.params,
                    attributes: nfs.attributes,
                    port: nfs.port,
                })
            }
            "tmpfs" => {
                let mount = declared_as(declared, target, Some(MountType::Tmpfs))?;
                if source
                    .as_deref()
                    .is_some_and(|s| s != "tmpfs" && s != "none")
                {
                    return refuse("a tmpfs takes no source");
                }
                let limit = mount.size_bytes.unwrap_or(0);
                let (params, attributes) = tmpfs_options(options, limit)?;
                Ok(Plan::Tmpfs {
                    target: target.clone(),
                    params,
                    attributes,
                })
            }
            other => refuse(format!(
                "type '{other}' cannot be mounted; only nfs and tmpfs"
            )),
        },
        Request::Bind { from, target } => {
            let bind = declared_as(declared, target, Some(MountType::Bind))?;
            let declared_from = bind.from.as_deref().unwrap_or_default();
            // The declared source or a directory below it; the helper then requires that the
            // source is on a mount the broker made.
            let below = from
                .strip_prefix(declared_from)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
            if !below || crate::image_permissions::check_clean(from).is_err() {
                return refuse(format!(
                    "'{target}' may only be bound from {declared_from} or below it"
                ));
            }
            Ok(Plan::Bind {
                from: from.clone(),
                target: target.clone(),
            })
        }
        Request::Umount { target, flags } => {
            declared_as(declared, target, None)?;
            let mut raw = 0;
            for flag in flags {
                raw |= match flag.as_str() {
                    "force" => libc::MNT_FORCE,
                    "detach" | "lazy" => libc::MNT_DETACH,
                    other => {
                        return Err((Errno::Einval, format!("unknown unmount flag '{other}'")))
                    }
                };
            }
            Ok(Plan::Umount {
                target: target.clone(),
                flags: raw,
            })
        }
    }
}

struct NfsOptions {
    params: Vec<(String, Option<String>)>,
    attributes: u64,
    port: u16,
}

/// Options `mount(8)` and `mount.nfs` handle in user space; the kernel never sees them.
fn user_space_only(key: &str) -> bool {
    matches!(
        key,
        "bg" | "fg"
            | "retry"
            | "_netdev"
            | "defaults"
            | "auto"
            | "noauto"
            | "nofail"
            | "user"
            | "nouser"
            | "users"
            | "comment"
    ) || key.starts_with("x-")
}

/// Mount attributes a request may set. `nosuid` and `nodev` are always set.
fn attribute(key: &str) -> Option<u64> {
    Some(match key {
        "ro" => MOUNT_ATTR_RDONLY,
        "rw" | "exec" | "nosuid" | "nodev" | "atime" => 0,
        "noexec" => MOUNT_ATTR_NOEXEC,
        "noatime" => MOUNT_ATTR_NOATIME,
        "nodiratime" => MOUNT_ATTR_NODIRATIME,
        "relatime" => MOUNT_ATTR_RELATIME,
        "strictatime" => MOUNT_ATTR_STRICTATIME,
        _ => return None,
    })
}

fn split(options: &str) -> impl Iterator<Item = (&str, Option<&str>)> {
    options
        .split(',')
        .filter(|item| !item.is_empty())
        .map(|item| match item.split_once('=') {
            Some((key, value)) => (key, Some(value)),
            None => (item, None),
        })
}

fn number(key: &str, value: Option<&str>, max: u64) -> Result<String, Refusal> {
    value
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v <= max)
        .map(|v| v.to_string())
        .ok_or_else(|| (Errno::Einval, format!("'{key}' needs a number up to {max}")))
}

fn nfs_options(source: &str, options: &str) -> Result<NfsOptions, Refusal> {
    let (host, export) = source
        .split_once(":/")
        .map(|(host, rest)| (host, format!("/{rest}")))
        .ok_or_else(|| {
            (
                Errno::Einval,
                format!("source '{source}' is not host:/path"),
            )
        })?;
    if host != "127.0.0.1" {
        return refuse(format!(
            "source '{source}' must be the loopback proxy at 127.0.0.1"
        ));
    }
    if export.split('/').any(|c| c == "..") || export.contains(',') {
        return Err((
            Errno::Einval,
            format!("source '{source}' has an unclean path"),
        ));
    }
    let mut params: Vec<(String, Option<String>)> = Vec::new();
    let mut attributes = MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV;
    let (mut vers, mut addr, mut client, mut port) = (None, false, false, None);
    for (key, value) in split(options) {
        if user_space_only(key) {
            continue;
        }
        if let Some(bits) = attribute(key) {
            if value.is_some() {
                return Err((Errno::Einval, format!("'{key}' takes no value")));
            }
            attributes |= bits;
            continue;
        }
        let value_of = |allowed: &[&str]| -> Result<Option<String>, Refusal> {
            match value {
                Some(v) if allowed.contains(&v) => Ok(Some(v.to_string())),
                _ => refuse(format!("'{key}' must be one of {}", allowed.join(", "))),
            }
        };
        let param = match key {
            "vers" | "nfsvers" => {
                let v = value_of(&["4.1", "4.2"])?;
                if vers.replace(v.clone()).is_some_and(|old| old != v) {
                    return refuse("conflicting NFS versions");
                }
                Some(("vers", v))
            }
            "addr" => {
                value_of(&["127.0.0.1"])?;
                addr = true;
                Some(("addr", value.map(str::to_string)))
            }
            "clientaddr" => {
                client = true;
                Some(("clientaddr", value_of(&["127.0.0.1"])?))
            }
            "proto" => Some(("proto", value_of(&["tcp"])?)),
            "port" => {
                let p = value
                    .and_then(|v| v.parse::<u16>().ok())
                    .filter(|p| PROXY_PORTS.contains(p))
                    .ok_or_else(|| {
                        (
                            Errno::Eperm,
                            format!(
                                "port must be a loopback proxy port ({}-{})",
                                PROXY_PORTS.start(),
                                PROXY_PORTS.end()
                            ),
                        )
                    })?;
                port = Some(p);
                Some(("port", Some(p.to_string())))
            }
            "rsize" | "wsize" => Some((key, Some(number(key, value, 1 << 20)?))),
            "timeo" | "acregmin" | "acregmax" | "acdirmin" | "acdirmax" | "actimeo" => {
                Some((key, Some(number(key, value, 1 << 20)?)))
            }
            "retrans" => Some((key, Some(number(key, value, 100)?))),
            "nconnect" => Some((key, Some(number(key, value, 16)?))),
            "sec" => Some(("sec", value_of(&["sys"])?)),
            "lookupcache" => Some((key, value_of(&["all", "none", "pos", "positive"])?)),
            "local_lock" => Some((key, value_of(&["none", "all", "flock", "posix"])?)),
            "hard" | "soft" | "softerr" | "noresvport" | "resvport" | "ac" | "noac" | "cto"
            | "nocto" | "sharecache" | "nosharecache" | "intr" | "nointr" | "rdirplus"
            | "nordirplus" | "lock" | "nolock" | "nofsc" | "tcp" => {
                if value.is_some() {
                    return Err((Errno::Einval, format!("'{key}' takes no value")));
                }
                if key == "tcp" {
                    Some(("proto", Some("tcp".to_string())))
                } else {
                    Some((key, None))
                }
            }
            "suid" | "dev" => {
                return refuse(format!("'{key}' is not allowed; nosuid,nodev are forced"))
            }
            "mountaddr" | "mountport" | "mounthost" | "mountproto" | "mountvers" | "fsc"
            | "xprtsec" | "udp" | "rdma" | "minorversion" | "v2" | "v3" | "v4" => {
                return refuse(format!("'{key}' is not allowed"))
            }
            other => return refuse(format!("option '{other}' is not allowed")),
        };
        if let Some((key, value)) = param {
            params.push((key.to_string(), value));
        }
    }
    if vers.is_none() {
        return refuse("vers=4.1 or vers=4.2 is required");
    }
    if !addr {
        return refuse("addr=127.0.0.1 is required");
    }
    if !client {
        return refuse("clientaddr=127.0.0.1 is required");
    }
    let port = port.ok_or_else(|| {
        (
            Errno::Eperm,
            "port= (the loopback proxy) is required".to_string(),
        )
    })?;
    Ok(NfsOptions {
        params,
        attributes,
        port,
    })
}

/// A size as tmpfs reads it (`8g`, `512m`, bytes), without `%`.
pub fn parse_size(value: &str) -> Option<u64> {
    let (digits, shift) = match value.char_indices().last()? {
        (i, 'k' | 'K') => (&value[..i], 10),
        (i, 'm' | 'M') => (&value[..i], 20),
        (i, 'g' | 'G') => (&value[..i], 30),
        (i, 't' | 'T') => (&value[..i], 40),
        _ => (value, 0),
    };
    digits.parse::<u64>().ok()?.checked_mul(1u64 << shift)
}

fn tmpfs_options(
    options: &str,
    limit: u64,
) -> Result<(Vec<(String, Option<String>)>, u64), Refusal> {
    let mut params = Vec::new();
    let mut attributes = MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV;
    let mut size = None;
    for (key, value) in split(options) {
        if user_space_only(key) {
            continue;
        }
        if let Some(bits) = attribute(key) {
            if value.is_some() {
                return Err((Errno::Einval, format!("'{key}' takes no value")));
            }
            attributes |= bits;
            continue;
        }
        match key {
            "size" => {
                let bytes = value
                    .and_then(parse_size)
                    .filter(|b| *b > 0)
                    .ok_or_else(|| (Errno::Einval, "size needs a byte count".to_string()))?;
                if bytes > limit {
                    return refuse(format!("size {bytes} is above the declared {limit} bytes"));
                }
                size = Some(bytes);
                params.push(("size".to_string(), Some(bytes.to_string())));
            }
            "mode" => {
                let mode = value
                    .and_then(|v| u32::from_str_radix(v, 8).ok())
                    .filter(|m| *m <= 0o7777)
                    .ok_or_else(|| (Errno::Einval, "mode needs an octal mode".to_string()))?;
                params.push(("mode".to_string(), Some(format!("{mode:o}"))));
            }
            "uid" | "gid" => {
                params.push((key.to_string(), Some(number(key, value, u32::MAX as u64)?)))
            }
            "nr_inodes" => {
                let n = value
                    .and_then(parse_size)
                    .ok_or_else(|| (Errno::Einval, "nr_inodes needs a number".to_string()))?;
                params.push((key.to_string(), Some(n.to_string())));
            }
            "suid" | "dev" => {
                return refuse(format!("'{key}' is not allowed; nosuid,nodev are forced"))
            }
            other => return refuse(format!("option '{other}' is not allowed for tmpfs")),
        }
    }
    if size.is_none() {
        return refuse("a tmpfs needs size=");
    }
    Ok((params, attributes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared() -> Declared {
        crate::image_permissions::parse(
            r#"{"mounts":[{"type":"nfs","path":"/workspace"},{"type":"tmpfs","path":"/workspace/tmp","sizeBytes":8589934592},{"type":"nfs","path":"/other"},{"type":"tmpfs","path":"/var/lib/vendor/workspace-tmp","sizeBytes":8589934592},{"type":"bind","from":"/var/lib/vendor/workspace-tmp","path":"/workspace/bound"}]}"#,
        )
        .unwrap()
    }

    fn nfs(options: &str) -> Request {
        Request::Mount {
            kind: "nfs4".into(),
            source: Some("127.0.0.1:/".into()),
            target: "/workspace".into(),
            options: options.into(),
        }
    }

    const GOOD: &str = "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2,noresvport,_netdev";

    #[test]
    fn the_contract_requests_parse_and_pass() {
        for text in [
            r#"{"op":"mount","type":"nfs","source":"127.0.0.1:/","target":"/workspace","options":"vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123"}"#,
            r#"{"op":"mount","type":"tmpfs","target":"/workspace/tmp","options":"size=8g,mode=1777"}"#,
            r#"{"op":"bind","from":"/var/lib/vendor/workspace-tmp","target":"/workspace/bound"}"#,
            r#"{"op":"bind","from":"/var/lib/vendor/workspace-tmp/sub","target":"/workspace/bound"}"#,
            r#"{"op":"mount","type":"nfs4","source":"127.0.0.1:/","target":"/workspace","options":"actimeo=30,nfsvers=4.2,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2,noresvport,port=20514,addr=127.0.0.1,clientaddr=127.0.0.1"}"#,
            r#"{"op":"umount","target":"/workspace","flags":["force","detach"]}"#,
        ] {
            let request: Request = serde_json::from_str(text).unwrap();
            plan(&declared(), &request).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        }
    }

    #[test]
    fn nfs_forces_nosuid_nodev_and_keeps_the_allowed_options() {
        let Plan::Nfs {
            params,
            attributes,
            port,
            ..
        } = plan(&declared(), &nfs(&format!("{GOOD},ro"))).unwrap()
        else {
            panic!()
        };
        assert_eq!(port, 20123);
        assert_eq!(
            attributes,
            MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_RDONLY
        );
        assert!(params.contains(&("vers".into(), Some("4.2".into()))));
        assert!(params.contains(&("hard".into(), None)));
        assert!(!params.iter().any(|(k, _)| k == "_netdev" || k == "ro"));
    }

    #[test]
    fn nfs_refusals() {
        let cases: &[(&str, &str)] = &[
            ("vers=3,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123", "v3"),
            ("vers=4.0,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123", "4.0"),
            (
                "nfsvers=4.1,vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123",
                "conflict",
            ),
            ("addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123", "no version"),
            ("vers=4.2,clientaddr=127.0.0.1,proto=tcp,port=20123", "no addr"),
            ("vers=4.2,addr=10.0.1.5,proto=tcp,port=20123", "addr spoof"),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=udp,port=20123", "udp"),
            ("vers=4.2,addr=127.0.0.1,udp,port=20123", "udp flag"),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=rdma,port=20123", "rdma"),
            ("vers=4.2,addr=127.0.0.1,port=20123", "no clientaddr"),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp", "no port"),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=2049",
                "port outside proxy range",
            ),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=8971", "agent port"),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,mountaddr=10.0.0.1",
                "mountaddr",
            ),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,mountport=2049",
                "mountport",
            ),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,fsc", "fsc"),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,xprtsec=tls",
                "xprtsec",
            ),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,sec=krb5",
                "krb5",
            ),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,sec=krb5p",
                "krb5p",
            ),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,suid", "suid"),
            ("vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,dev", "dev"),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,clientaddr=10.0.0.9",
                "clientaddr",
            ),
            (
                "vers=4.2,addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp,port=20123,whatever",
                "unknown",
            ),
        ];
        for (options, why) in cases {
            assert!(
                plan(&declared(), &nfs(options)).is_err(),
                "{why}: {options}"
            );
        }
        let mut wrong_source = nfs(GOOD);
        if let Request::Mount { source, .. } = &mut wrong_source {
            *source = Some("10.0.1.5:/".into());
        }
        assert!(plan(&declared(), &wrong_source).is_err(), "remote source");
    }

    #[test]
    fn wrong_type_or_path_is_refused() {
        let mut request = nfs(GOOD);
        if let Request::Mount { target, .. } = &mut request {
            *target = "/workspace/tmp".into();
        }
        assert!(
            plan(&declared(), &request).is_err(),
            "nfs on the tmpfs path"
        );
        for (kind, target) in [
            ("tmpfs", "/workspace"),
            ("tmpfs", "/elsewhere"),
            ("ext4", "/workspace"),
            ("proc", "/workspace"),
        ] {
            let request = Request::Mount {
                kind: kind.into(),
                source: None,
                target: target.into(),
                options: "size=1m".into(),
            };
            assert!(plan(&declared(), &request).is_err(), "{kind} at {target}");
        }
        let bind = Request::Bind {
            from: "/etc".into(),
            target: "/workspace".into(),
        };
        assert!(plan(&declared(), &bind).is_err(), "bind from undeclared");
        for (from, target) in [
            ("/workspace", "/workspace/bound"),
            ("/var/lib/vendor/workspace-tmpX", "/workspace/bound"),
            ("/var/lib/vendor/workspace-tmp/../x", "/workspace/bound"),
            ("/var/lib/vendor/workspace-tmp", "/other"),
        ] {
            let bind = Request::Bind {
                from: from.into(),
                target: target.into(),
            };
            assert!(plan(&declared(), &bind).is_err(), "bind {from} -> {target}");
        }
        let umount = Request::Umount {
            target: "/opt/alien".into(),
            flags: vec![],
        };
        assert!(plan(&declared(), &umount).is_err(), "umount undeclared");
        assert!(
            serde_json::from_str::<Request>(r#"{"op":"remount","target":"/workspace"}"#).is_err()
        );
    }

    #[test]
    fn tmpfs_needs_a_size_within_the_declaration() {
        let tmpfs = |options: &str| Request::Mount {
            kind: "tmpfs".into(),
            source: Some("tmpfs".into()),
            target: "/workspace/tmp".into(),
            options: options.into(),
        };
        assert!(plan(&declared(), &tmpfs("size=8g,mode=1777")).is_ok());
        assert!(plan(&declared(), &tmpfs("size=8589934592")).is_ok());
        for bad in [
            "mode=1777",
            "size=9g",
            "size=50%",
            "size=1g,suid",
            "size=1g,huge=always",
            "size=0",
        ] {
            assert!(plan(&declared(), &tmpfs(bad)).is_err(), "{bad}");
        }
    }
}
