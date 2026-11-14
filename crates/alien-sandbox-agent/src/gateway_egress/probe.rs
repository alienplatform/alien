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
        return Err(AlienError::new(ErrorData::egress_unverified(
            "udp-dns",
            "8.8.8.8:53 answered without the gateway",
        )));
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
    let (mut child, _registration) = crate::children::spawn(&mut command)
        .into_alien_error()
        .context(failed("spawn command path check"))?;
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .into_alien_error()
        .context(failed("command path check timeout"))?
        .into_alien_error()
        .context(failed("run command path check"))?;
    if !status.success() {
        let reason = status.code().map_or("the check was killed by a signal", path_check_reason);
        return Err(AlienError::new(ErrorData::egress_unverified(
            "command-identity",
            format!("the command-identity path check did not confirm the path is closed: {reason} ({status})"),
        )));
    }
    Ok(())
}

/// Why the command-identity check failed, as the child's exit code. The parent reads the code
/// into the refusal it reports, so the operator sees which of the checks failed.
pub const PATH_CHECK_CLOSED: i32 = 0;
const PATH_CHECK_METADATA_ANSWERED: i32 = 10;
const PATH_CHECK_INTERNET_ANSWERED: i32 = 11;
const PATH_CHECK_TIMEOUT_SETUP: i32 = 12;
const PATH_CHECK_UDP_UNAVAILABLE: i32 = 13;
const PATH_CHECK_DNS_UNEXPECTED: i32 = 14;

fn path_check_reason(code: i32) -> &'static str {
    match code {
        PATH_CHECK_METADATA_ANSWERED => "169.254.169.254:80 answered an HTTP request",
        PATH_CHECK_INTERNET_ANSWERED => "1.1.1.1:443 answered an HTTP request",
        PATH_CHECK_TIMEOUT_SETUP => "a socket timeout could not be set",
        PATH_CHECK_UDP_UNAVAILABLE => "a UDP socket could not be opened",
        PATH_CHECK_DNS_UNEXPECTED => {
            "8.8.8.8:53 answered with something other than the gateway's address"
        }
        _ => "the check ended abnormally",
    }
}

/// Internal child mode. It receives no token or credentials and has the command's exact identity.
/// Returns [`PATH_CHECK_CLOSED`] when the path is closed, else the code naming the failed check.
pub fn command_path_check() -> i32 {
    for (address, code) in [
        (
            SocketAddr::from(([169, 254, 169, 254], 80)),
            PATH_CHECK_METADATA_ANSWERED,
        ),
        (
            SocketAddr::from(([1, 1, 1, 1], 443)),
            PATH_CHECK_INTERNET_ANSWERED,
        ),
    ] {
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, PROBE_TIMEOUT) {
            if stream.set_read_timeout(Some(PROBE_TIMEOUT)).is_err()
                || stream.set_write_timeout(Some(PROBE_TIMEOUT)).is_err()
            {
                return PATH_CHECK_TIMEOUT_SETUP;
            }
            // A transparent proxy can accept TCP and then deny it. Only an actual HTTP reply
            // demonstrates a usable bypass; metadata error responses count as reachable too.
            if stream
                .write_all(b"GET / HTTP/1.0\r\nHost: egress-self-check.invalid\r\n\r\n")
                .is_ok()
            {
                let mut response = [0; 1];
                if matches!(stream.read(&mut response), Ok(n) if n > 0) {
                    return code;
                }
            }
        }
    }
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return PATH_CHECK_UDP_UNAVAILABLE;
    };
    if socket.set_read_timeout(Some(PROBE_TIMEOUT)).is_err() {
        return PATH_CHECK_TIMEOUT_SETUP;
    }
    if socket.connect("8.8.8.8:53").is_err() || socket.send(DNS_QUESTION).is_err() {
        return PATH_CHECK_CLOSED;
    }
    let mut response = [0; 512];
    match socket.recv(&mut response) {
        Ok(n) if n >= 4 && response[n - 4] == 198 && matches!(response[n - 3], 18 | 19) => {
            PATH_CHECK_CLOSED
        }
        Ok(_) => PATH_CHECK_DNS_UNEXPECTED,
        Err(_) => PATH_CHECK_CLOSED,
    }
}

/// Whether the command identity's path is closed.
pub fn command_path_is_closed() -> bool {
    command_path_check() == PATH_CHECK_CLOSED
}

#[cfg(test)]
mod path_check_tests {
    use super::*;

    #[test]
    fn every_failure_code_names_its_own_check() {
        let codes = [
            PATH_CHECK_METADATA_ANSWERED,
            PATH_CHECK_INTERNET_ANSWERED,
            PATH_CHECK_TIMEOUT_SETUP,
            PATH_CHECK_UDP_UNAVAILABLE,
            PATH_CHECK_DNS_UNEXPECTED,
        ];
        let reasons: std::collections::BTreeSet<_> =
            codes.iter().map(|code| path_check_reason(*code)).collect();
        assert_eq!(reasons.len(), codes.len());
        assert_eq!(path_check_reason(1), "the check ended abnormally");
    }
}
