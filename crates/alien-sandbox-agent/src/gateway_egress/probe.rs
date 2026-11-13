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

pub(super) async fn command(identity: crate::exec::ExecIdentity, privileged: bool) -> Result<()> {
    let executable = std::env::current_exe()
        .into_alien_error()
        .context(failed("locate path check executable"))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--egress-path-check")
        .args(privileged.then_some("--privileged"))
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
        let reason = status
            .code()
            .map_or("the check was killed by a signal", path_check_reason);
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
const PATH_CHECK_AWS_CREDENTIALS: i32 = 15;
const PATH_CHECK_AWS_INSTANCE_IDENTITY: i32 = 16;
const PATH_CHECK_AWS_IMDSV1: i32 = 17;
const PATH_CHECK_GCP_TOKEN: i32 = 18;
const PATH_CHECK_AZURE_TOKEN: i32 = 19;

fn path_check_reason(code: i32) -> &'static str {
    match code {
        PATH_CHECK_METADATA_ANSWERED => "169.254.169.254:80 answered an HTTP request",
        PATH_CHECK_INTERNET_ANSWERED => "1.1.1.1:443 answered an HTTP request",
        PATH_CHECK_TIMEOUT_SETUP => "a socket timeout could not be set",
        PATH_CHECK_UDP_UNAVAILABLE => "a UDP socket could not be opened",
        PATH_CHECK_DNS_UNEXPECTED => {
            "8.8.8.8:53 answered with something other than the gateway's address"
        }
        PATH_CHECK_AWS_CREDENTIALS => {
            "the command could read IAM credentials from the instance metadata service; require IMDSv2 with hop limit 1 on the nodes"
        }
        PATH_CHECK_AWS_INSTANCE_IDENTITY => {
            "the command could read the instance identity from the instance metadata service; require IMDSv2 with hop limit 1 on the nodes"
        }
        PATH_CHECK_AWS_IMDSV1 => {
            "the instance metadata service answered without a session token (IMDSv1); require IMDSv2 with hop limit 1 on the nodes"
        }
        PATH_CHECK_GCP_TOKEN => {
            "the command could obtain a service account token from the metadata server; use Workload Identity with no binding for the sandbox ServiceAccount"
        }
        PATH_CHECK_AZURE_TOKEN => {
            "the command could obtain an identity token from the instance metadata service; remove node identities the sandbox must not use"
        }
        _ => "the check ended abnormally",
    }
}

/// Internal child mode. It receives no token or credentials and has the command's exact identity.
/// Returns [`PATH_CHECK_CLOSED`] when the path is closed, else the code naming the failed check.
///
/// With `privileged` (a VM backend with the redirect), nothing may answer on the metadata
/// address. Unprivileged (Kubernetes), the cluster's NetworkPolicy may leave that address
/// reachable, so the check is that the command cannot obtain credentials or identity from it.
pub fn command_path_check(privileged: bool) -> i32 {
    if !privileged {
        let metadata = SocketAddr::from(([169, 254, 169, 254], 80));
        if let Some(code) = metadata_data_check(metadata) {
            return code;
        }
    }
    let mut addresses = vec![(
        SocketAddr::from(([1, 1, 1, 1], 443)),
        PATH_CHECK_INTERNET_ANSWERED,
    )];
    if privileged {
        addresses.insert(
            0,
            (
                SocketAddr::from(([169, 254, 169, 254], 80)),
                PATH_CHECK_METADATA_ANSWERED,
            ),
        );
    }
    for (address, code) in addresses {
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
pub fn command_path_is_closed(privileged: bool) -> bool {
    command_path_check(privileged) == PATH_CHECK_CLOSED
}

/// One HTTP/1.0 exchange. `Err` means the connection failed; `Ok(None)` an unusable reply.
/// A reply counts as data only for a 2xx status with a non-empty body.
fn metadata_request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<Option<String>> {
    let mut stream = TcpStream::connect_timeout(&address, PROBE_TIMEOUT)?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;
    let mut request = format!("{method} {path} HTTP/1.0\r\nHost: 169.254.169.254\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("Content-Length: 0\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Ok(None);
    }
    let mut raw = Vec::new();
    // A read timeout or reset after partial data still leaves what arrived to inspect.
    let _ = stream.take(64 * 1024).read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return Ok(None);
    };
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    Ok(((200..300).contains(&status) && !body.trim().is_empty()).then(|| body.to_string()))
}

/// Whether the command can obtain credentials or instance identity from the metadata service
/// at `address`, as the failing check's code. Status 401/403/404, a timeout, or a token with no
/// data behind it all pass.
fn metadata_data_check(address: SocketAddr) -> Option<i32> {
    // AWS: IMDSv2 token first, then data with it; then both again without a token (IMDSv1).
    let token = match metadata_request(
        address,
        "PUT",
        "/latest/api/token",
        &[("X-aws-ec2-metadata-token-ttl-seconds", "60")],
    ) {
        Err(_) => return None,
        Ok(token) => token.map(|body| body.trim().to_string()),
    };
    if let Some(token) = &token {
        let headers = [("X-aws-ec2-metadata-token", token.as_str())];
        for (path, code) in [
            (
                "/latest/meta-data/iam/security-credentials/",
                PATH_CHECK_AWS_CREDENTIALS,
            ),
            (
                "/latest/meta-data/instance-id",
                PATH_CHECK_AWS_INSTANCE_IDENTITY,
            ),
        ] {
            if matches!(
                metadata_request(address, "GET", path, &headers),
                Ok(Some(_))
            ) {
                return Some(code);
            }
        }
    }
    for path in [
        "/latest/meta-data/iam/security-credentials/",
        "/latest/meta-data/instance-id",
    ] {
        if matches!(metadata_request(address, "GET", path, &[]), Ok(Some(_))) {
            return Some(PATH_CHECK_AWS_IMDSV1);
        }
    }
    if matches!(
        metadata_request(
            address,
            "GET",
            "/computeMetadata/v1/instance/service-accounts/default/token",
            &[("Metadata-Flavor", "Google")],
        ),
        Ok(Some(_))
    ) {
        return Some(PATH_CHECK_GCP_TOKEN);
    }
    if matches!(
        metadata_request(
            address,
            "GET",
            "/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/",
            &[("Metadata", "true")],
        ),
        Ok(Some(_))
    ) {
        return Some(PATH_CHECK_AZURE_TOKEN);
    }
    None
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
            PATH_CHECK_AWS_CREDENTIALS,
            PATH_CHECK_AWS_INSTANCE_IDENTITY,
            PATH_CHECK_AWS_IMDSV1,
            PATH_CHECK_GCP_TOKEN,
            PATH_CHECK_AZURE_TOKEN,
        ];
        let reasons: std::collections::BTreeSet<_> =
            codes.iter().map(|code| path_check_reason(*code)).collect();
        assert_eq!(reasons.len(), codes.len());
        assert_eq!(path_check_reason(1), "the check ended abnormally");
    }

    /// A fake metadata service. A route is `(method, path prefix, needs token, status, body)`;
    /// a request no route matches gets a 404.
    fn serve(routes: Vec<(&'static str, &'static str, bool, u16, &'static str)>) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0; 4096];
                let n = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let line = request.lines().next().unwrap_or("").to_string();
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let has_token = request.contains("X-aws-ec2-metadata-token: tok");
                let (status, body) = routes
                    .iter()
                    .find(|(m, p, needs_token, ..)| {
                        *m == method && path.starts_with(p) && (!*needs_token || has_token)
                    })
                    .map_or((404, ""), |(_, _, _, status, body)| (*status, *body));
                let _ = write!(
                    stream,
                    "HTTP/1.0 {status} X\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        address
    }

    const TOKEN: (&str, &str, bool, u16, &str) = ("PUT", "/latest/api/token", false, 200, "tok");
    const CREDENTIALS: &str = "/latest/meta-data/iam/security-credentials/";

    #[test]
    fn a_metadata_service_that_answers_but_gives_nothing_passes() {
        let unauthorized = serve(vec![("PUT", "/latest/api/token", false, 401, "")]);
        assert_eq!(metadata_data_check(unauthorized), None);
        // A token with no data behind it: every data path is 404.
        assert_eq!(metadata_data_check(serve(vec![TOKEN])), None);
        // Nothing listening: connection refused.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(metadata_data_check(address), None);
    }

    #[test]
    fn aws_credentials_obtained_with_a_token_fail_the_check() {
        let address = serve(vec![TOKEN, ("GET", CREDENTIALS, true, 200, "role")]);
        assert_eq!(
            metadata_data_check(address),
            Some(PATH_CHECK_AWS_CREDENTIALS)
        );
        let reason = path_check_reason(PATH_CHECK_AWS_CREDENTIALS);
        assert!(reason.contains("IMDSv2") && reason.contains("hop limit 1"));
    }

    #[test]
    fn the_instance_identity_obtained_with_a_token_fails_the_check() {
        let address = serve(vec![
            TOKEN,
            ("GET", "/latest/meta-data/instance-id", true, 200, "i-1"),
        ]);
        assert_eq!(
            metadata_data_check(address),
            Some(PATH_CHECK_AWS_INSTANCE_IDENTITY)
        );
    }

    #[test]
    fn a_metadata_service_answering_without_a_token_fails_the_check() {
        let address = serve(vec![("GET", CREDENTIALS, false, 200, "role")]);
        assert_eq!(metadata_data_check(address), Some(PATH_CHECK_AWS_IMDSV1));
    }

    #[test]
    fn a_gcp_service_account_token_fails_the_check() {
        let address = serve(vec![(
            "GET",
            "/computeMetadata/v1/instance/service-accounts/default/token",
            false,
            200,
            "{\"access_token\":\"x\"}",
        )]);
        assert_eq!(metadata_data_check(address), Some(PATH_CHECK_GCP_TOKEN));
    }

    #[test]
    fn an_azure_identity_token_fails_the_check() {
        let address = serve(vec![(
            "GET",
            "/metadata/identity/oauth2/token",
            false,
            200,
            "{\"access_token\":\"x\"}",
        )]);
        assert_eq!(metadata_data_check(address), Some(PATH_CHECK_AZURE_TOKEN));
    }
}
