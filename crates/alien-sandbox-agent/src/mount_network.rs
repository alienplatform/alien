//! Keeps guest sockets away from the mount target and the proxy that fronts it.
//!
//! Every socket made by the image or by a command carries the guest mark (`0xa11f`, set when the
//! socket is created, see `gateway_egress::guest_network`). The helper that mounts, and the proxy
//! it starts, are forked before the agent joins the guest cgroup, so their sockets carry no mark.
//! These rules drop guest-marked traffic to the NFS port and to each proxy's loopback port, so the
//! only path to the file system is the mount the kernel makes on the helper's behalf.

use std::net::Ipv4Addr;

use nlink::netlink::{
    nftables::{
        Chain, ChainType, CmpOp, Expr, Family, Hook, MetaKey, Policy, Priority, Register, Rule,
    },
    Connection, Nftables,
};

/// The mark every guest socket carries.
pub const GUEST_MARK: u32 = 0xa11f;
/// The NFS port of the mount target.
pub const NFS_PORT: u16 = 2049;
const TABLE: &str = "alien_mounts";

/// Replaces the table: guest-marked sockets may reach neither port 2049 nor any of
/// `proxy_ports` on loopback. One transaction; until it commits the previous table stays.
pub async fn install(proxy_ports: &[u16]) -> nlink::netlink::Result<()> {
    let conn = Connection::<Nftables>::new()?;
    let exists = conn
        .list_tables()
        .await?
        .iter()
        .any(|table| table.family == Family::Ip && table.name == TABLE);
    let mut batch = conn.transaction();
    if exists {
        batch = batch.del_table(TABLE, Family::Ip);
    }
    let guest = || {
        Rule::new(TABLE, "output")
            .family(Family::Ip)
            .expr(Expr::Meta {
                dreg: Register::R0,
                key: MetaKey::Mark,
            })
            .expr(Expr::Cmp {
                sreg: Register::R0,
                op: CmpOp::Eq,
                data: GUEST_MARK.to_ne_bytes().to_vec(),
            })
    };
    batch = batch
        .add_table(TABLE, Family::Ip)
        .add_chain(
            Chain::new(TABLE, "output")?
                .family(Family::Ip)
                .chain_type(ChainType::Filter)
                .hook(Hook::Output)
                .priority(Priority::Filter)
                .policy(Policy::Accept),
        )
        .add_rule(guest().match_tcp_dport(NFS_PORT).drop());
    for port in proxy_ports {
        batch = batch.add_rule(
            guest()
                .match_daddr_v4(Ipv4Addr::LOCALHOST, 32)
                .match_tcp_dport(*port)
                .drop(),
        );
    }
    batch.commit(&conn).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    fn marked_connect(address: SocketAddr, mark: u32) -> std::io::Result<TcpStream> {
        let socket = socket2_like(address, mark)?;
        Ok(socket)
    }

    /// A connecting socket carrying `mark`, made the way a guest socket is.
    fn socket2_like(address: SocketAddr, mark: u32) -> std::io::Result<TcpStream> {
        // SAFETY: plain socket(2); the descriptor is wrapped right away.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fd was just created and is owned here.
        let stream = unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
        let stream: TcpStream = stream;
        if unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_MARK,
                (&mark as *const u32).cast(),
                size_of::<u32>() as _,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let timeout = Duration::from_millis(700);
        // connect_timeout needs the std socket; rebuild the call on the marked descriptor.
        let std::net::SocketAddr::V4(v4) = address else {
            return Err(std::io::ErrorKind::InvalidInput.into());
        };
        let mut raw: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        raw.sin_family = libc::AF_INET as _;
        raw.sin_port = v4.port().to_be();
        raw.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
        stream.set_nonblocking(true)?;
        // SAFETY: raw is a valid sockaddr_in for the length given.
        let rc = unsafe {
            libc::connect(
                stream.as_raw_fd(),
                (&raw as *const libc::sockaddr_in).cast(),
                size_of::<libc::sockaddr_in>() as _,
            )
        };
        if rc != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(std::io::Error::last_os_error());
        }
        let mut poll = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as i32) };
        if ready <= 0 {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let mut error: libc::c_int = 0;
        let mut length = size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: SO_ERROR into a c_int.
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut error as *mut libc::c_int).cast(),
                &mut length,
            )
        };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error));
        }
        Ok(stream)
    }

    /// Guest-marked sockets cannot reach the NFS port or a proxy's loopback port; unmarked ones
    /// (the helper and its proxy) and guest sockets to any other port still connect.
    /// Run with `unshare --user --map-root-user --net --mount -- <test binary> --ignored`.
    #[test]
    #[ignore = "needs nftables and CAP_NET_ADMIN, e.g. unshare --user --map-root-user --net"]
    fn guest_marked_sockets_cannot_reach_the_mount_target_or_the_proxy() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // Loopback of a fresh namespace must be up.
        assert!(std::process::Command::new("ip")
            .args(["link", "set", "lo", "up"])
            .status()
            .unwrap()
            .success());
        let nfs = TcpListener::bind(("127.0.0.1", NFS_PORT)).expect("a stand-in mount target");
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let to = |l: &TcpListener| l.local_addr().unwrap();

        // Before the rules: every mark connects, so the refusals below are the rules' doing.
        for listener in [&nfs, &proxy, &other] {
            marked_connect(to(listener), GUEST_MARK).expect("connects before the rules");
        }
        runtime.block_on(install(&[proxy_port])).expect("installs");

        let guest_nfs = marked_connect(to(&nfs), GUEST_MARK);
        let guest_proxy = marked_connect(to(&proxy), GUEST_MARK);
        assert!(guest_nfs.is_err(), "guest socket reached 2049");
        assert!(guest_proxy.is_err(), "guest socket reached the proxy port");
        marked_connect(to(&nfs), 0).expect("the helper's unmarked socket reaches 2049");
        marked_connect(to(&proxy), 0).expect("the unmarked socket reaches the proxy");
        marked_connect(to(&other), GUEST_MARK).expect("guest traffic elsewhere is untouched");

        // A second install replaces the table rather than stacking.
        runtime.block_on(install(&[])).expect("reinstalls");
        marked_connect(to(&proxy), GUEST_MARK).expect("the proxy port rule left with the table");
        assert!(marked_connect(to(&nfs), GUEST_MARK).is_err(), "2049 stays closed to guests");
    }
}
