//! Whether a command can obtain cloud credentials or an instance identity from the node's
//! metadata service.
//!
//! On Kubernetes the agent has no packet redirect, so a NetworkPolicy is the only thing between a
//! command and the metadata address, and some clusters leave that address reachable. What must
//! hold is that nothing useful comes back, so this asks for credentials the way each cloud's
//! tooling would and treats only data as a leak. Run it as the identity that runs commands.
use std::{
    fmt,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

/// The metadata service address shared by AWS, GCP and Azure.
const METADATA_ADDRESS: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(169, 254, 169, 254)),
    80,
);
/// Per request. Nothing answering costs one connect timeout, so the whole check stays near 1 s.
const TIMEOUT: Duration = Duration::from_millis(500);
const AWS_CREDENTIALS: &str = "/latest/meta-data/iam/security-credentials/";
const AWS_INSTANCE_ID: &str = "/latest/meta-data/instance-id";

/// What a command could read, and how an operator closes it.
#[derive(Debug, PartialEq, Eq)]
pub struct Leak {
    pub cloud: &'static str,
    pub path: &'static str,
    pub fix: &'static str,
}

impl fmt::Display for Leak {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} metadata service returned data for {} to an unprivileged command; {}",
            self.cloud, self.path, self.fix
        )
    }
}

const AWS_FIX: &str = "require IMDSv2 with hop limit 1 on the cluster nodes";
const GCP_FIX: &str = "use Workload Identity with no binding for the sandbox ServiceAccount";
const AZURE_FIX: &str = "remove node-assigned identities the sandbox must not use";

/// Ok when no request obtained credentials or identity data.
pub fn credentials_obtainable() -> Result<(), Leak> {
    check(METADATA_ADDRESS)
}

fn check(address: SocketAddr) -> Result<(), Leak> {
    let aws = |path, token: Option<&str>| {
        let headers: Vec<(&str, &str)> = token
            .map(|token| vec![("X-aws-ec2-metadata-token", token)])
            .unwrap_or_default();
        matches!(request(address, "GET", path, &headers), Ok(Some(_)))
    };
    // A refused or timed-out connection means nothing answers: nothing to leak.
    let token = match request(
        address,
        "PUT",
        "/latest/api/token",
        &[("X-aws-ec2-metadata-token-ttl-seconds", "60")],
    ) {
        Err(_) => return Ok(()),
        Ok(token) => token.map(|body| body.trim().to_string()),
    };
    if let Some(token) = token.as_deref() {
        for path in [AWS_CREDENTIALS, AWS_INSTANCE_ID] {
            if aws(path, Some(token)) {
                return Err(Leak {
                    cloud: "AWS",
                    path,
                    fix: AWS_FIX,
                });
            }
        }
    }
    for path in [AWS_CREDENTIALS, AWS_INSTANCE_ID] {
        if aws(path, None) {
            return Err(Leak {
                cloud: "AWS (IMDSv1, no session token)",
                path,
                fix: AWS_FIX,
            });
        }
    }
    let gcp = "/computeMetadata/v1/instance/service-accounts/default/token";
    if matches!(
        request(address, "GET", gcp, &[("Metadata-Flavor", "Google")]),
        Ok(Some(_))
    ) {
        return Err(Leak {
            cloud: "GCP",
            path: gcp,
            fix: GCP_FIX,
        });
    }
    let azure = "/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/";
    if matches!(
        request(address, "GET", azure, &[("Metadata", "true")]),
        Ok(Some(_))
    ) {
        return Err(Leak {
            cloud: "Azure",
            path: "/metadata/identity/oauth2/token",
            fix: AZURE_FIX,
        });
    }
    Ok(())
}

/// One HTTP/1.0 exchange. `Err`: the connection failed. `Ok(Some(body))`: a 2xx status with a
/// non-empty body. `Ok(None)`: anything else (401, 403, 404, empty, unreadable).
fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<Option<String>> {
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut message = format!("{method} {path} HTTP/1.0\r\nHost: 169.254.169.254\r\n");
    for (name, value) in headers {
        message.push_str(&format!("{name}: {value}\r\n"));
    }
    message.push_str("Content-Length: 0\r\n\r\n");
    if stream.write_all(message.as_bytes()).is_err() {
        return Ok(None);
    }
    let mut raw = Vec::new();
    // A timeout or reset after partial data still leaves what arrived to inspect.
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn an_unauthorized_reply_passes() {
        let address = serve(vec![("PUT", "/latest/api/token", false, 401, "")]);
        assert_eq!(check(address), Ok(()));
    }

    #[test]
    fn a_token_with_no_data_behind_it_passes() {
        assert_eq!(check(serve(vec![TOKEN])), Ok(()));
    }

    #[test]
    fn nothing_listening_passes() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(check(address), Ok(()));
    }

    #[test]
    fn aws_credentials_obtained_with_a_token_are_a_leak() {
        let address = serve(vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 200, "role")]);
        let leak = check(address).unwrap_err();
        assert_eq!((leak.cloud, leak.path), ("AWS", AWS_CREDENTIALS));
        let message = leak.to_string();
        assert!(message.contains("IMDSv2") && message.contains("hop limit 1"));
    }

    #[test]
    fn the_aws_instance_id_obtained_with_a_token_is_a_leak() {
        let address = serve(vec![TOKEN, ("GET", AWS_INSTANCE_ID, true, 200, "i-1")]);
        assert_eq!(check(address).unwrap_err().path, AWS_INSTANCE_ID);
    }

    #[test]
    fn aws_data_without_a_token_is_a_leak() {
        let address = serve(vec![("GET", AWS_CREDENTIALS, false, 200, "role")]);
        assert!(check(address).unwrap_err().cloud.contains("IMDSv1"));
    }

    #[test]
    fn a_gcp_service_account_token_is_a_leak() {
        let address = serve(vec![(
            "GET",
            "/computeMetadata/v1/instance/service-accounts/default/token",
            false,
            200,
            "{\"access_token\":\"x\"}",
        )]);
        assert_eq!(check(address).unwrap_err().cloud, "GCP");
    }

    #[test]
    fn an_azure_identity_token_is_a_leak() {
        let address = serve(vec![(
            "GET",
            "/metadata/identity/oauth2/token",
            false,
            200,
            "{\"access_token\":\"x\"}",
        )]);
        assert_eq!(check(address).unwrap_err().cloud, "Azure");
    }
}
