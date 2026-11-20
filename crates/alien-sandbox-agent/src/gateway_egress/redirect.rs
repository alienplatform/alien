use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4},
    os::fd::AsRawFd,
};

use nlink::netlink::{
    nftables::{
        Chain, ChainName, ChainType, CmpOp, CtDirection, CtState, Expr, Family, Hook, MetaKey,
        Policy, Priority, Register, Rule, Verdict,
    },
    Connection, Nftables,
};
use tokio::net::{TcpSocket, TcpStream};

use super::guest_network::GUEST_MARK;

pub(super) const MARK: u32 = 0xa11e;

pub(super) fn socket(marked: bool) -> io::Result<TcpSocket> {
    let socket = TcpSocket::new_v4()?;
    if marked {
        mark(socket.as_raw_fd())?;
    }
    Ok(socket)
}

pub(super) fn original_destination(stream: &TcpStream) -> io::Result<SocketAddrV4> {
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::sockaddr_in>() as libc::socklen_t;
    // SAFETY: address and len are writable, correctly sized buffers for SO_ORIGINAL_DST.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_IP,
            80,
            (&mut address as *mut libc::sockaddr_in).cast(),
            &mut len,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(SocketAddrV4::new(
        Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()),
        u16::from_be(address.sin_port),
    ))
}

/// Filter sockets tagged by the guest cgroup across every UID. Provider restore
/// processes remain outside that cgroup and retain their DNS and metadata paths.
pub(super) fn install() -> io::Result<()> {
    crate::netfilter::install(install_rules())?;
    let ip6 = crate::netfilter::install(install_ip6_rules()).map_err(ip6_failed)?;
    if matches!(ip6, Ip6::Absent(_)) {
        crate::netfilter::install(block_ip6_ports_by_policy()).map_err(ip6_policy_failed)?;
    }
    report_ip6(ip6);
    Ok(())
}

pub(super) async fn install_at_run() -> io::Result<()> {
    crate::netfilter::apply(install_rules()).await?;
    let ip6 = crate::netfilter::apply(install_ip6_rules())
        .await
        .map_err(ip6_failed)?;
    if matches!(ip6, Ip6::Absent(_)) {
        crate::netfilter::apply(block_ip6_ports_by_policy())
            .await
            .map_err(ip6_policy_failed)?;
    }
    report_ip6(ip6);
    Ok(())
}

/// Priority of the IPv6 policy rules: right after the local table's rule (0), so addresses of the
/// VM itself (`::1` among them) still route locally, and before any lookup that could route out.
pub(crate) const IP6_POLICY_PRIORITY: u32 = 1;

/// The policy rules for a kernel without IPv6 netfilter: every IPv6 TCP or UDP packet to the NFS
/// port or a declared connection's port is refused by routing ("prohibit": the connect fails with
/// `EACCES`), whoever sends it, the kernel's NFS client included. Routing rules need no netfilter,
/// and the image cannot remove them (no `NET_ADMIN`). Loopback is untouched: the local table is
/// looked up first. A rule left by an earlier start is kept.
fn ip6_policy_rules() -> Vec<nlink::netlink::rule::RuleBuilder> {
    let mut rules = Vec::new();
    for port in drop_ports() {
        for protocol in [libc::IPPROTO_TCP, libc::IPPROTO_UDP] {
            rules.push(
                nlink::netlink::rule::RuleBuilder::v6()
                    .priority(IP6_POLICY_PRIORITY)
                    .ipproto(protocol as u8)
                    .dport_eq(port)
                    .prohibit(),
            );
        }
    }
    rules
}

pub(super) async fn block_ip6_ports_by_policy() -> nlink::netlink::Result<()> {
    let conn = Connection::<nlink::netlink::Route>::new()?;
    for rule in ip6_policy_rules() {
        match conn.add_rule(rule).await {
            Ok(()) => {}
            Err(error) if error.is_already_exists() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Names the IPv6 policy step, which keeps the image from starting, and records it.
fn ip6_policy_failed(error: io::Error) -> io::Error {
    let error = io::Error::other(format!(
        "the IPv6 NFS-port policy rules (no IPv6 nftables) were refused: {error}"
    ));
    crate::boot_report::set("ip6 nfs ports", format!("failed: {error}"));
    crate::boot_report::record_ipv6();
    error
}

/// The outcome of the IPv6 half of the NFS-port drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ip6 {
    /// No image permissions: nothing to install.
    NotDeclared,
    Installed,
    /// The kernel refused the family itself when the table was added (this errno): it has no
    /// IPv6 netfilter, so there is nothing to install rules into.
    Absent(i32),
}

/// Set once the kernel refused the IPv6 family: the counters then skip it.
static IP6_ABSENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn report_ip6(ip6: Ip6) {
    let state = match ip6 {
        Ip6::NotDeclared => return,
        Ip6::Installed => "installed".to_string(),
        Ip6::Absent(errno) => {
            IP6_ABSENT.store(true, std::sync::atomic::Ordering::Relaxed);
            let error = io::Error::from_raw_os_error(errno);
            tracing::warn!(
                %error,
                "the kernel has no IPv6 nftables (table ip6 alien_gateway refused): continuing with the IPv4 NFS-port drop only"
            );
            let ports = drop_ports()
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            crate::boot_report::set(
                "ip6 nfs ports",
                format!(
                    "blocked by policy rule (priority {IP6_POLICY_PRIORITY}, tcp and udp, ports {ports})"
                ),
            );
            format!("absent ({}: {error})", errno_name(errno))
        }
    };
    crate::boot_report::set("ip6 nftables", state);
    crate::boot_report::record_ipv6();
}

fn errno_name(errno: i32) -> &'static str {
    match errno {
        libc::EOPNOTSUPP => "EOPNOTSUPP",
        libc::EAFNOSUPPORT => "EAFNOSUPPORT",
        libc::ENOENT => "ENOENT",
        _ => "errno",
    }
}

/// The errors with which a kernel without IPv6 netfilter refuses `add table ip6`. Any other error,
/// or any error after the table exists, keeps the agent from going on.
fn family_refused(error: &nlink::netlink::Error) -> Option<i32> {
    error
        .errno()
        .filter(|errno| [libc::EOPNOTSUPP, libc::EAFNOSUPPORT, libc::ENOENT].contains(errno))
}

/// Names the IPv6 step, which keeps the image from starting, and records it.
fn ip6_failed(error: io::Error) -> io::Error {
    let error = io::Error::other(format!(
        "the IPv6 NFS-port drop (table ip6 alien_gateway) was refused: {error}"
    ));
    crate::boot_report::set("ip6 nftables", format!("failed: {error}"));
    crate::boot_report::record_ipv6();
    error
}

/// One line with every counter of the NFS-port drop: per family, the jumps into the chain (per
/// port, in order), then the loopback return and the drop.
pub(super) async fn nfs_port_counters() -> io::Result<String> {
    let conn = Connection::<Nftables>::new().map_err(io::Error::other)?;
    let mut line = Vec::new();
    for (family, name) in [(Family::Ip, "ip"), (Family::Ip6, "ip6")] {
        if matches!(family, Family::Ip6) && IP6_ABSENT.load(std::sync::atomic::Ordering::Relaxed) {
            line.push("ip6 absent".to_string());
            continue;
        }
        let rules = conn
            .list_rules("alien_gateway", family)
            .await
            .map_err(io::Error::other)?;
        let mut jumps = Vec::new();
        let mut own = Vec::new();
        for rule in &rules {
            let Some((packets, _)) = rule.counter() else {
                continue;
            };
            if rule.chain == NFS_PORTS {
                own.push(packets);
            } else {
                jumps.push(packets);
            }
        }
        let [returned, dropped] = own[..] else {
            return Err(io::Error::other(format!(
                "{name}: expected two counted rules in {NFS_PORTS}, found {}",
                own.len()
            )));
        };
        line.push(format!(
            "{name} jumps={jumps:?} loopback_return={returned} dropped={dropped}"
        ));
    }
    Ok(line.join(" "))
}

fn image_mark(op: CmpOp) -> [Expr; 2] {
    [
        Expr::Meta {
            dreg: Register::R0,
            key: MetaKey::Mark,
        },
        Expr::Cmp {
            sreg: Register::R0,
            op,
            data: crate::mount_broker::IMAGE_MARK.to_ne_bytes().to_vec(),
        },
    ]
}

/// One rule per declared connection: TCP from the image's sockets (its own mark) to that network
/// and port takes `verdict` (skip the redirect; pass the filter). Empty without a declaration.
fn image_exemptions(rule: impl Fn() -> Rule, verdict: Expr) -> Vec<Rule> {
    crate::image_permissions::declared()
        .map(|declared| declared.destinations.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|(address, prefix, port)| {
            let [meta, cmp] = image_mark(CmpOp::Eq);
            rule()
                .expr(meta)
                .expr(cmp)
                .match_daddr_v4(address, prefix)
                .match_l4proto(libc::IPPROTO_TCP as u8)
                .match_tcp_dport(port)
                .expr(verdict.clone())
        })
        .collect()
}

const NFS_PORTS: &str = "nfs_ports";

/// The NFS port and every declared connection's port, once each.
fn drop_ports() -> Vec<u16> {
    let mut ports: Vec<u16> = crate::image_permissions::declared()
        .map(|declared| {
            declared
                .destinations
                .iter()
                .map(|(_, _, port)| *port)
                .collect()
        })
        .unwrap_or_default();
    ports.push(2049);
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// With image permissions, a new TCP connection that leaves the VM (anything not routed through
/// `lo`) to the NFS port or a declared connection's port is dropped unless its socket carries the
/// image mark, in both IPv4 and IPv6. This covers the kernel's NFS client, whose sockets carry no
/// mark, so a mount cannot bypass the image's loopback proxy through a referral or a second
/// address. Other traffic from outside the guest cgroup (the provider's DNS forwarder, NTP, the
/// endpoint proxy) is untouched.
fn add_nfs_port_drop(
    batch: nlink::netlink::nftables::Transaction,
    table: &str,
    chain: &str,
    family: Family,
) -> nlink::netlink::Result<nlink::netlink::nftables::Transaction> {
    let rule = || Rule::new(table, NFS_PORTS).family(family);
    let mut batch = batch
        .add_chain(Chain::new(table, NFS_PORTS)?.family(family))
        .add_rule(
            rule()
                .match_oif("lo")
                .counter()
                .expr(Expr::Verdict(Verdict::Return)),
        )
        .add_rule(rule().counter().drop());
    for port in drop_ports() {
        let [meta, cmp] = image_mark(CmpOp::Neq);
        batch = batch.add_rule(
            Rule::new(table, chain)
                .family(family)
                .match_ct_state(CtState::NEW)
                .expr(meta)
                .expr(cmp)
                .match_l4proto(libc::IPPROTO_TCP as u8)
                .match_tcp_dport(port)
                .counter()
                .jump(ChainName::new(NFS_PORTS)?),
        );
    }
    Ok(batch)
}

/// With image permissions, the IPv6 half of the NFS-port drop, in a transaction of its own after
/// the IPv4 rules so that a kernel refusing the IPv6 family is named. Nothing without them.
async fn install_ip6_rules() -> nlink::netlink::Result<Ip6> {
    if crate::image_permissions::declared().is_none() {
        return Ok(Ip6::NotDeclared);
    }
    let conn = Connection::<Nftables>::new()?;
    let tables = conn.list_tables().await?;
    let mut batch = conn.transaction();
    if tables
        .iter()
        .any(|table| table.family == Family::Ip6 && table.name == "alien_gateway")
    {
        batch = batch.del_table("alien_gateway", Family::Ip6);
    } else {
        // The table alone first: a refusal here is the kernel refusing the family, told apart
        // from a refused rule. The full transaction below replaces it.
        if let Err(error) = conn
            .transaction()
            .add_table("alien_gateway", Family::Ip6)
            .commit(&conn)
            .await
        {
            return match family_refused(&error) {
                Some(errno) => Ok(Ip6::Absent(errno)),
                None => Err(error),
            };
        }
        batch = batch.del_table("alien_gateway", Family::Ip6);
    }
    batch = batch.add_table("alien_gateway", Family::Ip6).add_chain(
        Chain::new("alien_gateway", "filter")?
            .family(Family::Ip6)
            .chain_type(ChainType::Filter)
            .hook(Hook::Output)
            .priority(Priority::Filter)
            .policy(Policy::Accept),
    );
    add_nfs_port_drop(batch, "alien_gateway", "filter", Family::Ip6)?
        .commit(&conn)
        .await?;
    Ok(Ip6::Installed)
}

async fn install_rules() -> nlink::netlink::Result<()> {
    let image = crate::image_permissions::declared().is_some();
    let conn = Connection::<Nftables>::new()?;
    let tables = conn.list_tables().await?;
    let mut batch = conn.transaction();
    // Replace only our tables, in one transaction. Until commit, an existing policy remains
    // effective. Any refused operation aborts the whole batch and prevents agent startup.
    if tables
        .iter()
        .any(|table| table.family == Family::Ip && table.name == "alien_gateway")
    {
        batch = batch.del_table("alien_gateway", Family::Ip);
    }
    batch = batch.add_table("alien_gateway", Family::Ip);
    let nat = || Rule::new("alien_gateway", "guest_tcp").family(Family::Ip);
    let filter = || Rule::new("alien_gateway", "filter").family(Family::Ip);
    let returning = Expr::Verdict(Verdict::Return);
    let system = |rule: Rule| {
        let rule = rule
            .expr(Expr::Meta {
                dreg: Register::R0,
                key: MetaKey::Mark,
            })
            .expr(Expr::Cmp {
                sreg: Register::R0,
                op: CmpOp::Neq,
                data: GUEST_MARK.to_ne_bytes().to_vec(),
            });
        // The image's sockets are guest traffic too, apart from their declared connections.
        if image {
            let [_, cmp] = image_mark(CmpOp::Neq);
            rule.expr(cmp)
        } else {
            rule
        }
    };
    batch = batch
        .add_chain(
            Chain::new("alien_gateway", "guest_tcp")?
                .family(Family::Ip)
                .chain_type(ChainType::Nat)
                .hook(Hook::Output)
                .priority(Priority::DstNat)
                .policy(Policy::Accept),
        )
        .add_rule(system(nat()).expr(returning.clone()));
    for rule in image_exemptions(nat, returning.clone()) {
        batch = batch.add_rule(rule);
    }
    batch = batch
        .add_rule(
            nat()
                .match_ct_direction(CtDirection::Reply)
                .match_l4proto(libc::IPPROTO_TCP as u8)
                .expr(returning.clone()),
        )
        .add_rule(
            nat()
                .match_ct_direction(CtDirection::Reply)
                .match_udp_sport(15053)
                .expr(returning.clone()),
        )
        // Some VM kernels expose DNAT but not the separate redirect expression.
        .add_rule(
            nat()
                .match_udp_dport(53)
                .dnat(Ipv4Addr::LOCALHOST, Some(15053)),
        )
        .add_rule(
            nat()
                .match_tcp_dport(53)
                .dnat(Ipv4Addr::LOCALHOST, Some(15053)),
        )
        .add_rule(nat().match_daddr_v4(Ipv4Addr::LOCALHOST, 8).expr(returning))
        .add_rule(
            nat()
                .match_l4proto(libc::IPPROTO_TCP as u8)
                .dnat(Ipv4Addr::LOCALHOST, Some(15001)),
        )
        .add_chain(
            Chain::new("alien_gateway", "filter")?
                .family(Family::Ip)
                .chain_type(ChainType::Filter)
                .hook(Hook::Output)
                .priority(Priority::Filter)
                .policy(Policy::Drop),
        );
    if image {
        batch = add_nfs_port_drop(batch, "alien_gateway", "filter", Family::Ip)?;
    }
    batch = batch.add_rule(system(filter()).accept());
    for rule in image_exemptions(filter, Expr::Verdict(Verdict::Accept)) {
        batch = batch.add_rule(rule);
    }
    batch
        // External inbound sockets must not become an uninspected outbound channel.
        // Preview traffic reaches command listeners through the agent's loopback proxy.
        .add_rule(filter().match_daddr_v4(Ipv4Addr::LOCALHOST, 32).accept())
        .add_rule(filter().match_oif("lo").accept())
        .commit(&conn)
        .await
}

pub(super) fn mark(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fd is live; MARK is a correctly sized socket option.
    if unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_MARK,
            (&MARK as *const u32).cast(),
            size_of::<u32>() as _,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::family_refused;
    use nlink::netlink::Error;

    #[test]
    fn only_a_refused_family_lets_the_ip6_table_be_absent() {
        // As a Lambda guest kernel answers `add table ip6`: "Not supported (errno 95)".
        for errno in [libc::EOPNOTSUPP, libc::EAFNOSUPPORT, libc::ENOENT] {
            assert_eq!(family_refused(&Error::from_errno(errno)), Some(errno));
            assert_eq!(family_refused(&Error::from_errno(-errno)), Some(errno));
        }
        for errno in [libc::EPERM, libc::EINVAL, libc::ENOMEM, libc::EBUSY] {
            assert_eq!(family_refused(&Error::from_errno(errno)), None);
        }
    }
}
