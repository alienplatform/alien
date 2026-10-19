//! Path checks use both supervisor sockets and a real, irreversibly dropped command process.
use super::{failed, redirect, ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    os::fd::AsRawFd,
    time::Duration,
};

const DNS_QUESTION: &[u8] =
    b"\x00\x2a\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x06egress\x07invalid\x00\x00\x01\x00\x01";
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) async fn supervisor_udp(privileged: bool) -> Result<()> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .into_alien_error()
        .context(failed("bind UDP path check"))?;
    if privileged {
        redirect::mark(socket.as_raw_fd())
            .into_alien_error()
            .context(failed("mark UDP path check"))?;
    }
    let mut reply = [0; 512];
    let reply = tokio::time::timeout(PROBE_TIMEOUT, async {
        socket.connect("8.8.8.8:53").await?;
        socket.send(DNS_QUESTION).await?;
        socket.recv(&mut reply).await
    })
    .await;
    if matches!(reply, Ok(Ok(n)) if n > 0) {
        return Err(AlienError::new(ErrorData::EgressPathUnexpected));
    }
    Ok(())
}

pub(super) async fn command(identity: crate::exec::ExecIdentity) -> Result<()> {
    let executable = std::env::current_exe()
        .into_alien_error()
        .context(failed("locate path check executable"))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--egress-path-check")
        .env_clear()
        .kill_on_drop(true);
    // SAFETY: only the existing async-signal-safe identity drop runs between fork and exec.
    unsafe {
        command.pre_exec(move || crate::privilege::drop_to(identity));
    }
    let status = tokio::time::timeout(Duration::from_secs(5), command.status())
        .await
        .into_alien_error()
        .context(failed("command path check timeout"))?
        .into_alien_error()
        .context(failed("run command path check"))?;
    if !status.success() {
        return Err(AlienError::new(ErrorData::EgressPathUnexpected));
    }
    Ok(())
}

/// Internal child mode. It receives no token or credentials and has the command's exact identity.
pub fn command_path_is_closed() -> bool {
    for address in [
        SocketAddr::from(([169, 254, 169, 254], 80)),
        SocketAddr::from(([1, 1, 1, 1], 443)),
    ] {
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, PROBE_TIMEOUT) {
            if stream.set_read_timeout(Some(PROBE_TIMEOUT)).is_err()
                || stream.set_write_timeout(Some(PROBE_TIMEOUT)).is_err()
            {
                return false;
            }
            // A transparent proxy can accept TCP and then deny it. Only an actual HTTP reply
            // demonstrates a usable bypass; metadata error responses count as reachable too.
            if stream
                .write_all(b"GET / HTTP/1.0\r\nHost: egress-self-check.invalid\r\n\r\n")
                .is_ok()
            {
                let mut response = [0; 1];
                if matches!(stream.read(&mut response), Ok(n) if n > 0) {
                    return false;
                }
            }
        }
    }
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return false;
    };
    if socket.set_read_timeout(Some(PROBE_TIMEOUT)).is_err() {
        return false;
    }
    if socket.connect("8.8.8.8:53").is_err() || socket.send(DNS_QUESTION).is_err() {
        return true;
    }
    let mut response = [0; 512];
    match socket.recv(&mut response) {
        Ok(n) => n >= 4 && response[n - 4] == 198 && matches!(response[n - 3], 18 | 19),
        Err(_) => true,
    }
}
