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

/// Installs an IPv4 OUTPUT default-deny policy atomically through iptables-restore.
/// Replies to inbound agent/preview connections are allowed; a listening port never grants
/// outbound access. Restricted policies disable DNS after startup hostname pinning.
pub fn install(policy: &SandboxEgress) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(AlienError::new(failed("supervisor must start as root")));
    }
    crate::privilege::block_command_ipv6();
    let ips = destinations(policy)?;
    let mut rules =
        String::from("*filter\n:INPUT ACCEPT [0:0]\n:FORWARD DROP [0:0]\n:OUTPUT DROP [0:0]\n");
    if !matches!(policy, SandboxEgress::Allow) {
        rules
            .push_str("-A OUTPUT -p udp --dport 53 -j DROP\n-A OUTPUT -p tcp --dport 53 -j DROP\n");
    }
    rules.push_str("-A OUTPUT -o lo -j ACCEPT\n-A OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT\n");
    // Restricted policies need no DNS after startup pinning. The guest kernel lacks the
    // iptables owner match, so never create a resolver exception shared with commands.
    if matches!(policy, SandboxEgress::Allow) {
        let resolv = fs::read_to_string("/etc/resolv.conf")
            .into_alien_error()
            .context(failed("read resolver configuration"))?;
        for line in resolv.lines() {
            let mut words = line.split_whitespace();
            if words.next() == Some("nameserver") {
                if let Some(ip) = words.next().and_then(|s| s.parse::<Ipv4Addr>().ok()) {
                    for protocol in ["udp", "tcp"] {
                        rules.push_str(&format!(
                            "-A OUTPUT -d {ip} -p {protocol} --dport 53 -j ACCEPT\n"
                        ));
                    }
                }
            }
        }
    }
    match policy {
        SandboxEgress::Deny => {}
        SandboxEgress::AllowDomains { .. } => {
            for ip in ips {
                rules.push_str(&format!("-A OUTPUT -d {ip} -j ACCEPT\n"));
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
                rules.push_str(&format!("-A OUTPUT -d {cidr} -j DROP\n"));
            }
            rules.push_str("-A OUTPUT -j ACCEPT\n");
        }
    }
    rules.push_str("COMMIT\n");
    let mut child = Command::new("/usr/sbin/iptables-nft-restore")
        .arg("--wait")
        .arg("5")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(failed(
            "start iptables-nft-restore (the image must include iptables)",
        ))?;
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
        .context(failed("wait for iptables-nft-restore"))?;
    if !output.status.success() {
        return Err(AlienError::new(failed(String::from_utf8_lossy(
            &output.stderr,
        ))));
    }
    Ok(())
}
