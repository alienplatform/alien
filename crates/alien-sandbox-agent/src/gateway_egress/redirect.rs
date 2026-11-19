use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4},
    os::fd::AsRawFd,
};

use nlink::netlink::{
    nftables::{
        Chain, ChainType, CmpOp, CtDirection, Expr, Family, Hook, MetaKey, Policy, Priority,
        Register, Rule, Verdict,
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
    crate::netfilter::install(install_rules())
}

pub(super) async fn install_at_run() -> io::Result<()> {
    crate::netfilter::apply(install_rules()).await
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
        // NFS leaves the guest only through the image's own proxy: TCP 2049 to anything but
        // loopback is dropped unless the socket carries the image mark. This covers the kernel's
        // NFS client (whose sockets carry no mark), so a mount cannot bypass the loopback proxy
        // through a referral or a second address.
        let [meta, cmp] = image_mark(CmpOp::Neq);
        batch = batch
            .add_rule(
                filter()
                    .expr(meta.clone())
                    .expr(cmp.clone())
                    .match_daddr_v4(Ipv4Addr::new(127, 0, 0, 0), 8)
                    .match_l4proto(libc::IPPROTO_TCP as u8)
                    .match_tcp_dport(2049)
                    .accept(),
            )
            .add_rule(
                filter()
                    .expr(meta)
                    .expr(cmp)
                    .match_l4proto(libc::IPPROTO_TCP as u8)
                    .match_tcp_dport(2049)
                    .expr(Expr::Verdict(Verdict::Drop)),
            );
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
