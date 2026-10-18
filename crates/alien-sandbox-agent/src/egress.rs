//! Fail-closed startup policy. No image code or exec request runs before this completes.

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    net::{Ipv4Addr, ToSocketAddrs},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

use alien_error::{AlienError, Context, IntoAlienError};

use nlink::netlink::{
    nftables::{
        Chain, ChainType, CmpOp, CtState, Expr, Family, Hook, MetaKey, Policy, Priority, Register,
        Rule,
    },
    Connection, Nftables,
};

use crate::error::{ErrorData, Result};

/// The released object-form policies keep their direct, UID-filtered supervisor path.
#[derive(serde::Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum LegacyEgress {
    Deny,
    Allow,
    AllowDomains { domains: Vec<String> },
}

/// New string policies are handled by the gateway; object policies retain released behavior.
pub fn legacy_from_env() -> Result<Option<LegacyEgress>> {
    let Ok(encoded) = std::env::var("ALIEN_SANDBOX_EGRESS") else {
        return Ok(None);
    };
    if matches!(encoded.as_str(), "none" | "internet")
        || serde_json::from_str::<String>(&encoded)
            .is_ok_and(|value| matches!(value.as_str(), "none" | "internet"))
    {
        return Ok(None);
    }
    serde_json::from_str(&encoded)
        .map(Some)
        .into_alien_error()
        .context(failed("parse legacy startup policy"))
}

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
fn destinations(policy: &LegacyEgress) -> Result<BTreeSet<Ipv4Addr>> {
    let mut ips = BTreeSet::new();
    if let LegacyEgress::AllowDomains { domains } = policy {
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

/// Preserves the released supervisor policy: only the command UID is filtered. Agent and
/// platform transport traffic keep their existing route; legacy policies never use the gateway.
pub fn install(policy: &LegacyEgress, command_uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 || command_uid == 0 {
        return Err(AlienError::new(failed(
            "requires a root supervisor and a non-root command uid",
        )));
    }
    crate::privilege::block_command_ipv6();
    let ips = destinations(policy)?;
    let mut resolvers = Vec::new();
    if matches!(policy, LegacyEgress::Allow) {
        let resolv = fs::read_to_string("/etc/resolv.conf")
            .into_alien_error()
            .context(failed("read resolver configuration"))?;
        for line in resolv.lines() {
            let mut words = line.split_whitespace();
            if words.next() == Some("nameserver") {
                if let Some(ip) = words.next().and_then(|s| s.parse::<Ipv4Addr>().ok()) {
                    resolvers.push(ip);
                }
            }
        }
    }
    crate::netfilter::install(async {
        let conn = Connection::<Nftables>::new()?;
        let mut batch = conn.transaction();
        if conn
            .list_tables()
            .await?
            .iter()
            .any(|t| t.name == "alien_egress" && t.family == Family::Ip)
        {
            batch = batch.del_table("alien_egress", Family::Ip);
        }
        let command = || Rule::new("alien_egress", "commands").family(Family::Ip);
        batch = batch
            .add_table("alien_egress", Family::Ip)
            .add_chain(
                Chain::new("alien_egress", "output")?
                    .family(Family::Ip)
                    .chain_type(ChainType::Filter)
                    .hook(Hook::Output)
                    .priority(Priority::Filter)
                    .policy(Policy::Accept),
            )
            .add_chain(Chain::new("alien_egress", "commands")?.family(Family::Ip))
            .add_rule(
                Rule::new("alien_egress", "output")
                    .family(Family::Ip)
                    .expr(Expr::Meta {
                        dreg: Register::R0,
                        key: MetaKey::SkUid,
                    })
                    .expr(Expr::Cmp {
                        sreg: Register::R0,
                        op: CmpOp::Eq,
                        data: command_uid.to_ne_bytes().to_vec(),
                    })
                    .try_jump("commands")?,
            );
        if !matches!(policy, LegacyEgress::Allow) {
            batch = batch
                .add_rule(command().match_udp_dport(53).drop())
                .add_rule(command().match_tcp_dport(53).drop());
        }
        batch = batch.add_rule(command().match_oif("lo").accept()).add_rule(
            command()
                .match_ct_state(CtState::ESTABLISHED | CtState::RELATED)
                .accept(),
        );
        for ip in resolvers {
            batch = batch
                .add_rule(
                    command()
                        .match_daddr_v4(ip, 32)
                        .match_udp_dport(53)
                        .accept(),
                )
                .add_rule(
                    command()
                        .match_daddr_v4(ip, 32)
                        .match_tcp_dport(53)
                        .accept(),
                );
        }
        match policy {
            LegacyEgress::Deny => {}
            LegacyEgress::AllowDomains { .. } => {
                for ip in ips {
                    batch = batch.add_rule(command().match_daddr_v4(ip, 32).accept());
                }
            }
            LegacyEgress::Allow => {
                for (address, prefix) in [
                    ([0, 0, 0, 0], 8),
                    ([10, 0, 0, 0], 8),
                    ([100, 64, 0, 0], 10),
                    ([127, 0, 0, 0], 8),
                    ([169, 254, 0, 0], 16),
                    ([172, 16, 0, 0], 12),
                    ([192, 168, 0, 0], 16),
                    ([192, 0, 0, 0], 24),
                    ([198, 18, 0, 0], 15),
                    ([224, 0, 0, 0], 3),
                ] {
                    batch = batch.add_rule(
                        command()
                            .match_daddr_v4(Ipv4Addr::from(address), prefix)
                            .drop(),
                    );
                }
                batch = batch.add_rule(command().accept());
            }
        }
        batch.add_rule(command().drop()).commit(&conn).await
    })
    .into_alien_error()
    .context(failed("install legacy command egress rules"))
}
