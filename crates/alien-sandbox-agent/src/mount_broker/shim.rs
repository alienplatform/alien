//! The commands an image uses to reach the broker. They are the agent binary itself, chosen by
//! name: `/opt/alien/bin/mount.nfs4` and `/opt/alien/bin/mount.nfs` speak `mount.nfs`'s argv
//! (`<source> <dir> [-o options] [-f] [-n] [-v] [-s] [-r] [-w]`) and exit like it;
//! `/opt/alien/bin/alien-mount` takes `tmpfs <dir> -o <options>`, `bind <from> <dir>`,
//! `nfs <source> <dir> -o <options>` and `umount <dir> [--force] [--detach|-l]`.

use std::path::Path;

use super::request::{Reply, Request};

/// The names the agent answers to as a shim.
pub const NAMES: &[&str] = &["mount.nfs4", "mount.nfs", "alien-mount"];
/// Where the agent links them.
pub const DIRECTORY: &str = "/opt/alien/bin";

/// `mount.nfs` exit codes.
const EX_USAGE: i32 = 1;
const EX_FAIL: i32 = 32;

/// The shim to run for `argv0`, if it names one.
pub fn shim_name(argv0: &str) -> Option<&'static str> {
    let name = Path::new(argv0).file_name()?.to_str()?;
    NAMES.iter().copied().find(|n| *n == name)
}

/// Builds the request a `mount.nfs` argv asks for. `mount.nfs` itself adds `addr=` from the
/// source host, and `clientaddr=`; so does this.
pub fn nfs_request(kind: &str, args: &[String]) -> Result<Option<Request>, String> {
    let mut positional = Vec::new();
    let mut options: Vec<String> = Vec::new();
    let mut fake = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" => options.push(iter.next().ok_or("-o needs options")?.clone()),
            "-f" => fake = true,
            "-n" | "-v" | "-s" => {}
            "-r" => options.push("ro".into()),
            "-w" => options.push("rw".into()),
            other if other.starts_with("-o") => options.push(other[2..].to_string()),
            other if other.starts_with('-') => return Err(format!("unknown flag {other}")),
            other => positional.push(other.to_string()),
        }
    }
    let [source, target] = positional.as_slice() else {
        return Err("usage: mount.nfs4 <source> <dir> [-o options]".into());
    };
    let mut joined = options.join(",");
    let keys: Vec<&str> = joined
        .split(',')
        .map(|o| o.split('=').next().unwrap_or_default())
        .collect();
    let mut extra = Vec::new();
    if !keys.contains(&"addr") {
        if let Some((host, _)) = source.split_once(":/") {
            extra.push(format!("addr={host}"));
        }
    }
    if !keys.contains(&"clientaddr") {
        if let Some((host, _)) = source.split_once(":/") {
            extra.push(format!("clientaddr={host}"));
        }
    }
    for item in extra {
        if !joined.is_empty() {
            joined.push(',');
        }
        joined.push_str(&item);
    }
    if fake {
        return Ok(None);
    }
    Ok(Some(Request::Mount {
        kind: kind.into(),
        source: Some(source.clone()),
        target: target.trim_end_matches('/').to_string(),
        options: joined,
    }))
}

/// Builds the request an `alien-mount` argv asks for.
pub fn alien_mount_request(args: &[String]) -> Result<Request, String> {
    let usage = "usage: alien-mount tmpfs <dir> -o <options> | bind <from> <dir> | nfs <source> <dir> -o <options> | umount <dir> [--force] [--detach]";
    let (command, rest) = args.split_first().ok_or(usage)?;
    let take_options = |rest: &[String]| -> Result<(Vec<String>, String), String> {
        let mut positional = Vec::new();
        let mut options = Vec::new();
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            if arg == "-o" {
                options.push(iter.next().ok_or("-o needs options")?.clone());
            } else if arg.starts_with('-') {
                return Err(format!("unknown flag {arg}"));
            } else {
                positional.push(arg.clone());
            }
        }
        Ok((positional, options.join(",")))
    };
    match command.as_str() {
        "tmpfs" => {
            let (positional, options) = take_options(rest)?;
            let [target] = positional.as_slice() else {
                return Err(usage.into());
            };
            Ok(Request::Mount {
                kind: "tmpfs".into(),
                source: None,
                target: target.clone(),
                options,
            })
        }
        "nfs" => match nfs_request("nfs4", rest)? {
            Some(request) => Ok(request),
            None => Err("-f is not supported here".into()),
        },
        "bind" => {
            let [from, target] = rest else {
                return Err(usage.into());
            };
            Ok(Request::Bind {
                from: from.clone(),
                target: target.clone(),
            })
        }
        "umount" => {
            let mut target = None;
            let mut flags = Vec::new();
            for arg in rest {
                match arg.as_str() {
                    "--force" | "-f" => flags.push("force".to_string()),
                    "--detach" | "--lazy" | "-l" => flags.push("detach".to_string()),
                    other if other.starts_with('-') => return Err(format!("unknown flag {other}")),
                    other if target.is_none() => target = Some(other.to_string()),
                    _ => return Err(usage.into()),
                }
            }
            Ok(Request::Umount {
                target: target.ok_or(usage)?,
                flags,
            })
        }
        _ => Err(usage.into()),
    }
}

/// Runs the shim `name` with `args` (without argv0) and returns the exit code.
pub fn main(name: &str, args: &[String]) -> i32 {
    let request = match name {
        "alien-mount" => alien_mount_request(args).map(Some),
        "mount.nfs" => nfs_request("nfs", args),
        _ => nfs_request("nfs4", args),
    };
    let request = match request {
        Ok(Some(request)) => request,
        Ok(None) => return 0,
        Err(message) => {
            eprintln!("{name}: {message}");
            return EX_USAGE;
        }
    };
    let socket = std::env::var_os("ALIEN_MOUNT_SOCKET")
        .filter(|_| cfg!(debug_assertions))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| super::SOCKET_PATH.into());
    match call(&socket, &request) {
        Ok(Reply {
            ok: true, mount_id, ..
        }) => {
            // `alien-mount` names the mount it made or removed; the mount helpers stay quiet,
            // as mount(8) expects.
            if let (Some(id), "alien-mount") = (mount_id, name) {
                println!("mount_id={id}");
            }
            0
        }
        Ok(Reply { errno, reason, .. }) => {
            eprintln!(
                "{name}: {}: {}",
                errno.unwrap_or_else(|| "EIO".into()),
                reason.unwrap_or_default()
            );
            EX_FAIL
        }
        Err(error) => {
            eprintln!(
                "{name}: cannot reach the mount broker at {}: {error}",
                socket.display()
            );
            EX_FAIL
        }
    }
}

#[cfg(target_os = "linux")]
fn call(socket: &Path, request: &Request) -> std::io::Result<Reply> {
    // Longer than the broker's own deadline, so the broker's answer arrives first.
    super::call(
        socket,
        request,
        super::REQUEST_DEADLINE + std::time::Duration::from_secs(10),
    )
}

#[cfg(not(target_os = "linux"))]
fn call(_: &Path, _: &Request) -> std::io::Result<Reply> {
    Err(std::io::Error::other("the mount broker runs only on Linux"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn mount_nfs_argv_becomes_a_request_with_addr_and_clientaddr() {
        let request = nfs_request(
            "nfs4",
            &args("127.0.0.1:/ /workspace/ -o nfsvers=4.2,port=20123,hard -n"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            request,
            Request::Mount {
                kind: "nfs4".into(),
                source: Some("127.0.0.1:/".into()),
                target: "/workspace".into(),
                options: "nfsvers=4.2,port=20123,hard,addr=127.0.0.1,clientaddr=127.0.0.1".into(),
            }
        );
        assert!(nfs_request("nfs4", &args("127.0.0.1:/ /w -f"))
            .unwrap()
            .is_none());
        assert!(nfs_request("nfs4", &args("127.0.0.1:/")).is_err());
        assert!(nfs_request("nfs4", &args("127.0.0.1:/ /w --bogus")).is_err());
    }

    #[test]
    fn alien_mount_argv() {
        assert!(matches!(
            alien_mount_request(&args("tmpfs /workspace/tmp -o size=8g,mode=1777")).unwrap(),
            Request::Mount { kind, .. } if kind == "tmpfs"
        ));
        assert_eq!(
            alien_mount_request(&args("umount /workspace --force -l")).unwrap(),
            Request::Umount {
                target: "/workspace".into(),
                flags: vec!["force".into(), "detach".into()],
            }
        );
        assert!(alien_mount_request(&args("bind /a")).is_err());
        assert!(alien_mount_request(&args("remount /a")).is_err());
        assert_eq!(shim_name("/opt/alien/bin/mount.nfs4"), Some("mount.nfs4"));
        assert_eq!(shim_name("/usr/local/bin/alien-sandbox-agent"), None);
    }
}
