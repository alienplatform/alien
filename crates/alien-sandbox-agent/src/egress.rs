//! Fail-closed startup policy. No image code or exec request runs before this completes.

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    net::{Ipv4Addr, ToSocketAddrs},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    process::{Command, Stdio},
};

use alien_core::SandboxEgress;
use alien_error::{AlienError, Context, IntoAlienError};

use crate::error::{ErrorData, Result};

fn failed(reason: impl ToString) -> ErrorData {
    ErrorData::OperationFailed {
        operation: "install sandbox egress policy".to_string(),
        reason: reason.to_string(),
    }
}

fn public(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 168 || b == 0))
        || (a == 198 && (b == 18 || b == 19)))
}

/// Resolves once at startup and pins the result in /etc/hosts. Commands get no DNS exception:
/// allowing arbitrary DNS queries would let them bypass a deny/allowlist with query names.
fn destinations(policy: &SandboxEgress) -> Result<BTreeSet<Ipv4Addr>> {
    let mut ips = BTreeSet::new();
    if let SandboxEgress::AllowDomains { domains } = policy {
        let mut hosts = String::new();
        for domain in domains {
            if domain.is_empty()
                || !domain
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
            {
                return Err(AlienError::new(failed(format!(
                    "invalid allowlisted hostname '{domain}'"
                ))));
            }
            let hostname = domain
                .strip_suffix('.')
                .unwrap_or(domain)
                .to_ascii_lowercase();
            let addresses: BTreeSet<_> = (hostname.as_str(), 0)
                .to_socket_addrs()
                .into_alien_error()
                .context(failed(format!("resolve {domain}")))?
                .filter_map(|addr| match addr.ip() {
                    std::net::IpAddr::V4(ip) if public(ip) => Some(ip),
                    _ => None,
                })
                .collect();
            if addresses.is_empty() {
                return Err(AlienError::new(failed(format!(
                    "{domain} has no public IPv4 address"
                ))));
            }
            for ip in &addresses {
                hosts.push_str(&format!("{ip} {hostname} {hostname}.\n"));
            }
            ips.extend(addresses);
        }
        pin_hosts(&hosts)
            .into_alien_error()
            .context(failed("pin allowlisted hostnames"))?;
    }
    Ok(ips)
}

/// AWS provides /etc/hosts as a read-only mount. Replace that mount during bootstrap,
/// before dropping SYS_ADMIN; commands can read it but cannot replace or edit it.
fn pin_hosts(hosts: &str) -> std::io::Result<()> {
    match fs::OpenOptions::new().append(true).open("/etc/hosts") {
        Ok(mut file) => return file.write_all(hosts.as_bytes()),
        Err(error) if error.raw_os_error() == Some(libc::EROFS) => {}
        Err(error) => return Err(error),
    }
    let mut content = fs::read_to_string("/etc/hosts")?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(hosts);
    let source = "/opt/alien/pinned-hosts";
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)?;
    file.write_all(content.as_bytes())?;
    file.set_permissions(fs::Permissions::from_mode(0o444))?;
    unsafe {
        if libc::mount(
            c"/opt/alien/pinned-hosts".as_ptr(),
            c"/etc/hosts".as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        if libc::mount(
            std::ptr::null(),
            c"/etc/hosts".as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
            std::ptr::null(),
        ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Atomically installs default-deny IPv4 egress for the declaration-owned command uid.
/// AWS's internal transport must initialize after image restore; filtering its root traffic
/// prevents the VM from serving requests. Native nftables uid matching is supported even
/// though this kernel lacks the iptables owner extension.
pub fn install(policy: &SandboxEgress, command_uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 || command_uid == 0 {
        return Err(AlienError::new(failed(
            "requires a root supervisor and a non-root command uid",
        )));
    }
    crate::privilege::block_command_ipv6();
    let ips = destinations(policy)?;
    // Keep other tables and AWS transport unchanged. add is idempotent; flush and replacement
    // are submitted in one atomic batch, so commands never see a partial policy on restart.
    let mut rules = format!("add table ip alien_egress\nflush table ip alien_egress\nadd chain ip alien_egress output {{ type filter hook output priority 0; policy accept; }}\nadd chain ip alien_egress commands\nadd rule ip alien_egress output meta skuid {command_uid} jump commands\n");
    let mut add =
        |rule: &str| rules.push_str(&format!("add rule ip alien_egress commands {rule}\n"));
    if !matches!(policy, SandboxEgress::Allow) {
        add("udp dport 53 drop");
        add("tcp dport 53 drop");
    }
    add("oifname \"lo\" accept");
    add("ct state established,related accept");
    if matches!(policy, SandboxEgress::Allow) {
        let resolv = fs::read_to_string("/etc/resolv.conf")
            .into_alien_error()
            .context(failed("read resolver configuration"))?;
        for line in resolv.lines() {
            let mut words = line.split_whitespace();
            if words.next() == Some("nameserver") {
                if let Some(ip) = words.next().and_then(|s| s.parse::<Ipv4Addr>().ok()) {
                    for protocol in ["udp", "tcp"] {
                        add(&format!("ip daddr {ip} {protocol} dport 53 accept"));
                    }
                }
            }
        }
    }
    match policy {
        SandboxEgress::Deny => {}
        SandboxEgress::AllowDomains { .. } => {
            for ip in ips {
                add(&format!("ip daddr {ip} accept"));
            }
        }
        SandboxEgress::Allow => {
            for cidr in [
                "0.0.0.0/8",
                "10.0.0.0/8",
                "100.64.0.0/10",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "192.0.0.0/24",
                "198.18.0.0/15",
                "224.0.0.0/3",
            ] {
                add(&format!("ip daddr {cidr} drop"));
            }
            add("accept");
        }
    }
    add("drop");
    let mut child = Command::new("/usr/sbin/nft")
        .args(["--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(failed("start nft (the image must include nftables)"))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(rules.as_bytes())
        .into_alien_error()
        .context(failed("write rules"))?;
    let output = child
        .wait_with_output()
        .into_alien_error()
        .context(failed("wait for nft"))?;
    if !output.status.success() {
        return Err(AlienError::new(failed(String::from_utf8_lossy(
            &output.stderr,
        ))));
    }
    Ok(())
}
