use std::{
    io::{self, Write},
    net::{Ipv4Addr, SocketAddrV4},
    os::fd::AsRawFd,
    process::{Command, Stdio},
};

use tokio::net::{TcpSocket, TcpStream};

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

/// All guest TCP, including root-owned processes, is redirected. Only explicitly marked agent
/// sockets and replies on the agent's control listener escape the redirect; no UID-wide exemption.
pub(super) fn install(control_port: u16) -> io::Result<()> {
    let rules = format!(
        r#"add table ip alien_gateway
flush table ip alien_gateway
add chain ip alien_gateway redirect {{ type nat hook output priority -100; policy accept; }}
add rule ip alien_gateway redirect meta mark {MARK} return
add rule ip alien_gateway redirect ct direction reply tcp sport {{ {control_port},15001,15002,15053 }} return
add rule ip alien_gateway redirect ip daddr 127.0.0.1 tcp dport {{ {control_port},15001,15002,15053 }} return
add rule ip alien_gateway redirect ip daddr 127.0.0.1 udp dport 15053 return
add rule ip alien_gateway redirect ct direction reply udp sport 15053 return
add rule ip alien_gateway redirect udp dport 53 redirect to :15053
add rule ip alien_gateway redirect tcp dport 53 redirect to :15053
add rule ip alien_gateway redirect meta l4proto tcp redirect to :15001
add chain ip alien_gateway filter {{ type filter hook output priority 0; policy drop; }}
add rule ip alien_gateway filter meta mark {MARK} accept
add rule ip alien_gateway filter ct direction reply tcp sport {{ {control_port},15001,15002,15053 }} accept
add rule ip alien_gateway filter oifname "lo" accept
add table ip6 alien_gateway
flush table ip6 alien_gateway
add chain ip6 alien_gateway output {{ type filter hook output priority 0; policy drop; }}
"#
    );
    let mut child = Command::new("/usr/sbin/nft")
        .args(["--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("missing nft stdin"))?
        .write_all(rules.as_bytes())?;
    let result = child.wait_with_output()?;
    if !result.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    Ok(())
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
