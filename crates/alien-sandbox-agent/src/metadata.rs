//! Whether a command can obtain cloud credentials or an instance identity from the node's
//! metadata service.
//!
//! On Kubernetes the agent has no packet redirect, so whether the metadata address is reachable
//! from a command depends on the cluster. What must hold is that nothing useful comes back, so
//! this asks for credentials and instance data the way each cloud's tooling would. It fails
//! unless every reply is a refusal: only no connection (nothing answers, and a dropped connection
//! is how a NetworkPolicy blocks traffic) and an HTTP 401, 403 or 404 count as "nothing here".
//! Data, a redirect, a 429 or 5xx, an unparseable or cut-off reply, and a read timeout after the
//! connection opened all fail the check. Run it as the identity that runs commands.
use std::{
    fmt,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    time::Duration,
};

/// The metadata service address shared by AWS, GCP and Azure.
const METADATA_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), 80);
/// Per request. Nothing answering costs one connect timeout, so the whole check stays near 1 s.
const TIMEOUT: Duration = Duration::from_millis(500);
/// The top-level listing, and the only entries allowed in it. Auto Mode's metadata service serves
/// just these two to a pod; every identifying entry is a 404.
const AWS_LISTING: &str = "/latest/meta-data/";
const AWS_LISTING_ALLOWED: [&str; 2] = ["placement/", "services/"];
const AWS_CREDENTIALS: &str = "/latest/meta-data/iam/security-credentials/";
const AWS_INSTANCE_ID: &str = "/latest/meta-data/instance-id";
/// Everything else the AWS metadata service serves that identifies the instance or carries
/// credentials: the instance user data, the role summary, the instance's own credentials, and the
/// signed identity document. Asked for with and without a session token.
const AWS_OTHER_PATHS: [&str; 6] = [
    "/latest/user-data",
    "/latest/meta-data/iam/info",
    "/latest/meta-data/identity-credentials/ec2/security-credentials/ec2-instance",
    "/latest/dynamic/instance-identity/document",
    "/latest/dynamic/instance-identity/pkcs7",
    "/latest/dynamic/instance-identity/signature",
];
const GCP_TOKEN: &str = "/computeMetadata/v1/instance/service-accounts/default/token";
const AZURE_INSTANCE: &str = "/metadata/instance?api-version=2021-02-01";

/// Other addresses a node or cloud runs that hand credentials to a caller on the network: EKS Pod
/// Identity, the ECS task credential endpoint, the IPv6 form of the EC2 metadata service, the GKE
/// metadata server, and Azure's wire server. Probed with the same rule as the main address, in
/// parallel so the check still ends near one request timeout.
struct Endpoint {
    cloud: &'static str,
    address: SocketAddr,
    method: &'static str,
    path: &'static str,
    headers: &'static [(&'static str, &'static str)],
    fix: &'static str,
}

const OTHER_ENDPOINTS: [Endpoint; 5] = [
    Endpoint {
        cloud: "AWS (EKS Pod Identity)",
        address: POD_IDENTITY_ADDRESS,
        method: "GET",
        path: "/v1/credentials",
        headers: &[],
        fix: "keep the Pod Identity agent unreachable from sandbox pods",
    },
    Endpoint {
        cloud: "AWS (ECS task credentials)",
        address: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 170, 2)), 80),
        method: "GET",
        path: "/v2/credentials",
        headers: &[],
        fix: "block 169.254.170.2 for sandbox pods",
    },
    Endpoint {
        cloud: "AWS (IMDS over IPv6)",
        address: SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254)),
            80,
        ),
        method: "PUT",
        path: "/latest/api/token",
        headers: &[("X-aws-ec2-metadata-token-ttl-seconds", "60")],
        fix: "disable the IPv6 metadata endpoint on the nodes or block it for sandbox pods",
    },
    Endpoint {
        cloud: "GCP (GKE metadata server)",
        address: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 252)), 988),
        method: "GET",
        path: GCP_TOKEN,
        headers: &[("Metadata-Flavor", "Google")],
        fix: "use Workload Identity with no binding for the sandbox ServiceAccount",
    },
    Endpoint {
        cloud: "Azure (wire server)",
        address: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(168, 63, 129, 16)), 80),
        method: "GET",
        path: "/machine/?comp=goalstate",
        headers: &[("x-ms-version", "2012-11-30")],
        fix: "block 168.63.129.16 for sandbox pods",
    },
];

/// What a command could read (or why the check could not tell it could not), and how an operator
/// closes it.
#[derive(Debug, PartialEq, Eq)]
pub struct Exposure {
    pub cloud: &'static str,
    pub path: &'static str,
    pub fix: &'static str,
    /// `None`: the service returned data. `Some`: a reply that is neither data nor a refusal, so
    /// the check cannot say nothing is obtainable and refuses.
    pub unexpected: Option<String>,
}

impl fmt::Display for Exposure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.unexpected {
            None => write!(
                f,
                "{} metadata service returned data for {} to a command; {}",
                self.cloud, self.path, self.fix
            ),
            Some(reply) => write!(
                f,
                "{} metadata service answered {} with {reply}, which is neither data nor a refusal (401, 403, 404), so a command might obtain credentials; {}",
                self.cloud, self.path, self.fix
            ),
        }
    }
}

const AWS_FIX: &str = "require IMDSv2 with hop limit 1 on the cluster nodes";
const GCP_FIX: &str = "use Workload Identity with no binding for the sandbox ServiceAccount";
const AZURE_FIX: &str = "remove node-assigned identities the sandbox must not use";

/// Ok when no request obtained credentials or identity data and every reply was a refusal.
///
/// Timing: nothing answering costs one connect timeout (about 0.5 s). A service that accepts and
/// never replies is a read timeout, which fails the check (it cannot show nothing comes back).
pub fn credentials_obtainable() -> Result<(), Exposure> {
    check(METADATA_ADDRESS)?;
    let tokens = pod_identity_tokens();
    let results: Vec<Result<(), Exposure>> = std::thread::scope(|scope| {
        let tokens = &tokens;
        let handles: Vec<_> = OTHER_ENDPOINTS
            .iter()
            .map(|endpoint| {
                scope.spawn(move || {
                    if endpoint.address == POD_IDENTITY_ADDRESS {
                        check_pod_identity(endpoint, tokens)
                    } else {
                        check_endpoint(endpoint)
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("endpoint probe panicked"))
            .collect()
    });
    results.into_iter().collect()
}

/// What one request showed.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    /// The connection could not be made: nothing to return (a dropped connection is a policy).
    Unreachable,
    /// A complete status line, whatever body arrived, and the raw head (status line and headers).
    Status(u16, String, String),
    /// The connection opened but no usable reply came back (timeout, reset, garbage).
    Broken(String),
}

/// How a reply is judged.
enum Verdict {
    Refused,
    Data,
    Unexpected(String),
}

fn judge(reply: &Reply) -> Verdict {
    match reply {
        Reply::Unreachable => Verdict::Refused,
        Reply::Status(401 | 403 | 404, ..) => Verdict::Refused,
        Reply::Status(code, body, _) if (200..300).contains(code) && !body.trim().is_empty() => {
            Verdict::Data
        }
        Reply::Status(code, ..) => Verdict::Unexpected(format!("HTTP {code}")),
        Reply::Broken(why) => Verdict::Unexpected(why.clone()),
    }
}

/// The AWS-shaped requests reach other clouds' metadata services on their clusters, and those
/// answer them with a plain error: the GKE metadata server marks every reply with
/// `Metadata-Flavor: Google` (a 405 for the token PUT), and Azure's service answers a request
/// without a `Metadata: true` header with a 400 "Required metadata header not specified". That
/// recognition changes how only a 400 or 405 is read (a status that carries no data): the request
/// is not an AWS request there, so it is not "neither data nor a refusal". It never forgives data:
/// a 2xx with a body, a redirect or a server error is judged as it would be anywhere else, whatever
/// the headers claim, and every later probe is judged on its own reply.
fn judge_aws(reply: &Reply) -> Verdict {
    if let Reply::Status(400 | 405, body, head) = reply {
        let head = head.to_ascii_lowercase();
        if head.contains("\r\nmetadata-flavor: google")
            || body
                .to_ascii_lowercase()
                .contains("required metadata header not specified")
        {
            return Verdict::Refused;
        }
    }
    judge(reply)
}

fn outcome(
    cloud: &'static str,
    path: &'static str,
    fix: &'static str,
    verdict: Verdict,
) -> Result<(), Exposure> {
    match verdict {
        Verdict::Refused => Ok(()),
        Verdict::Data => Err(Exposure {
            cloud,
            path,
            fix,
            unexpected: None,
        }),
        Verdict::Unexpected(reply) => Err(Exposure {
            cloud,
            path,
            fix,
            unexpected: Some(reply),
        }),
    }
}

/// EKS Pod Identity answers a request that carries no token with a 400: nothing is handed out. A
/// token the command can read changes that, so each readable token is presented and a 2xx with a
/// body then counts as credentials obtainable.
const POD_IDENTITY_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 170, 23)), 80);
const POD_IDENTITY_TOKEN_PATH: &str =
    "/var/run/secrets/pods.eks.amazonaws.com/serviceaccount/eks-pod-identity-token";
const SERVICE_ACCOUNT_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

/// Tokens the command identity can read that the Pod Identity agent accepts or might accept: the
/// token or token file named in the environment, the default Pod Identity token file, and a
/// mounted service account token.
fn pod_identity_tokens() -> Vec<String> {
    read_tokens(
        std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN").ok(),
        std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE").ok(),
        &[POD_IDENTITY_TOKEN_PATH, SERVICE_ACCOUNT_TOKEN_PATH],
    )
}

fn read_tokens(
    token: Option<String>,
    token_file: Option<String>,
    defaults: &[&str],
) -> Vec<String> {
    let mut tokens = Vec::new();
    tokens.extend(token);
    let mut files = Vec::new();
    files.extend(token_file);
    files.extend(defaults.iter().map(|path| path.to_string()));
    for path in files {
        if let Ok(contents) = std::fs::read_to_string(path) {
            tokens.push(contents);
        }
    }
    tokens
        .into_iter()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .collect()
}

fn check_pod_identity(endpoint: &Endpoint, tokens: &[String]) -> Result<(), Exposure> {
    if tokens.is_empty() {
        // No token to present: a 400 means the agent wants one, and none is available.
        let reply = exchange(
            endpoint.address,
            endpoint.method,
            endpoint.path,
            endpoint.headers,
        );
        return match reply {
            Reply::Status(400, ..) => Ok(()),
            other => outcome(endpoint.cloud, endpoint.path, endpoint.fix, judge(&other)),
        };
    }
    for token in tokens {
        let reply = exchange(
            endpoint.address,
            endpoint.method,
            endpoint.path,
            &[("Authorization", token.as_str())],
        );
        outcome(endpoint.cloud, endpoint.path, endpoint.fix, judge(&reply))?;
    }
    Ok(())
}

fn check_endpoint(endpoint: &Endpoint) -> Result<(), Exposure> {
    let reply = exchange(
        endpoint.address,
        endpoint.method,
        endpoint.path,
        endpoint.headers,
    );
    outcome(endpoint.cloud, endpoint.path, endpoint.fix, judge(&reply))
}

fn check(address: SocketAddr) -> Result<(), Exposure> {
    // The token request first: nothing listening means nothing to ask the rest of this address.
    let token_reply = exchange(
        address,
        "PUT",
        "/latest/api/token",
        &[("X-aws-ec2-metadata-token-ttl-seconds", "60")],
    );
    if token_reply == Reply::Unreachable {
        return Ok(());
    }
    let token = match &token_reply {
        Reply::Status(code, body, _) if (200..300).contains(code) && !body.trim().is_empty() => {
            Some(body.trim().to_string())
        }
        // A refusal means no token; anything else is not a refusal.
        other => {
            outcome("AWS", "/latest/api/token", AWS_FIX, judge_aws(other))?;
            None
        }
    };
    let aws = |path: &'static str, token: Option<&str>, cloud: &'static str| {
        let headers: Vec<(&str, &str)> = token
            .map(|token| vec![("X-aws-ec2-metadata-token", token)])
            .unwrap_or_default();
        outcome(
            cloud,
            path,
            AWS_FIX,
            judge_aws(&exchange(address, "GET", path, &headers)),
        )
    };
    // The listing is read as a list: only the entries a node with no instance data serves
    // (`placement/`, `services/`) are allowed, so an entry added later (`tags/`, `hostname`, ...)
    // fails the check without a path being added here first.
    let listing = |token: Option<&str>, cloud: &'static str| {
        let headers: Vec<(&str, &str)> = token
            .map(|token| vec![("X-aws-ec2-metadata-token", token)])
            .unwrap_or_default();
        match exchange(address, "GET", AWS_LISTING, &headers) {
            Reply::Status(code, body, _)
                if (200..300).contains(&code)
                    && body
                        .lines()
                        .map(str::trim)
                        .all(|entry| entry.is_empty() || AWS_LISTING_ALLOWED.contains(&entry))
                    && !body.trim().is_empty() =>
            {
                Ok(())
            }
            other => outcome(cloud, AWS_LISTING, AWS_FIX, judge_aws(&other)),
        }
    };
    let aws_paths = || {
        [AWS_CREDENTIALS, AWS_INSTANCE_ID]
            .into_iter()
            .chain(AWS_OTHER_PATHS)
    };
    if let Some(token) = token.as_deref() {
        listing(Some(token), "AWS")?;
        for path in aws_paths() {
            aws(path, Some(token), "AWS")?;
        }
    }
    listing(None, "AWS (IMDSv1, no session token)")?;
    for path in aws_paths() {
        aws(path, None, "AWS (IMDSv1, no session token)")?;
    }
    // GKE Workload Identity serves the instance id to every pod (it is not a credential), so only
    // the service-account token is asked for.
    outcome(
        "GCP",
        GCP_TOKEN,
        GCP_FIX,
        judge(&exchange(
            address,
            "GET",
            GCP_TOKEN,
            &[("Metadata-Flavor", "Google")],
        )),
    )?;
    outcome(
        "Azure",
        "/metadata/instance",
        AZURE_FIX,
        judge(&exchange(
            address,
            "GET",
            AZURE_INSTANCE,
            &[("Metadata", "true")],
        )),
    )?;
    let azure = "/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/";
    let azure_reply = exchange(address, "GET", azure, &[("Metadata", "true")]);
    let verdict = match &azure_reply {
        // "Identity not found": the node has no identity to hand out.
        Reply::Status(400, body, _) if body.to_ascii_lowercase().contains("identity not found") => {
            Verdict::Refused
        }
        // Any other 400 that talks about identities means the node has several user-assigned
        // identities and wants a `client_id`: a command that knows one can still get a token.
        Reply::Status(400, body, _) if body.to_ascii_lowercase().contains("identit") => {
            Verdict::Data
        }
        other => judge(other),
    };
    outcome(
        "Azure",
        "/metadata/identity/oauth2/token",
        AZURE_FIX,
        verdict,
    )
}

/// Nothing could be reached: the connection was refused, timed out, or had no route. Anything
/// else (descriptor exhaustion, a missing address family, a permission error) is a failure to
/// ask, not an answer.
fn is_unreachable(error: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        error.kind(),
        ConnectionRefused | TimedOut | NetworkUnreachable | HostUnreachable
    )
}

/// One HTTP/1.1 exchange (`Connection: close`; Azure's service requires 1.1).
fn exchange(address: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
    let mut stream = match TcpStream::connect_timeout(&address, TIMEOUT) {
        Ok(stream) => stream,
        Err(error) if is_unreachable(&error) => return Reply::Unreachable,
        // Out of file descriptors, an unsupported address family, a permission error: the check
        // could not ask, which is not the same as nothing answering.
        Err(error) => {
            return Reply::Broken(format!("the connection could not be attempted ({error})"))
        }
    };
    let broken = |why: &str| Reply::Broken(why.to_string());
    if stream.set_read_timeout(Some(TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(TIMEOUT)).is_err()
    {
        return broken("a socket timeout could not be set");
    }
    let host = match address.ip() {
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    let mut message = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        message.push_str(&format!("{name}: {value}\r\n"));
    }
    message.push_str("Content-Length: 0\r\n\r\n");
    if stream.write_all(message.as_bytes()).is_err() {
        return broken("the request could not be sent");
    }
    let mut raw = Vec::new();
    let read = (&stream).take(MAX_REPLY as u64).read_to_end(&mut raw);
    let Some(split) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        return match read {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                broken("no reply before the read timeout")
            }
            _ => broken("an incomplete or unparseable reply"),
        };
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body_bytes = &raw[split + 4..];
    let status_line = head.lines().next().unwrap_or("");
    if !status_line.starts_with("HTTP/") {
        return broken("an unparseable status line");
    }
    let Some(code) = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
    else {
        return broken("an unparseable status line");
    };
    // A reply counts only when it is complete. One that fills the read limit is not: the rest was
    // never seen, so it cannot be shown to hold nothing.
    if raw.len() >= MAX_REPLY {
        return broken("a reply larger than the read limit");
    }
    // A read error or timeout after the headers means the body may be cut off.
    if let Err(error) = read {
        return Reply::Broken(format!("the reply was cut off after its headers ({error})"));
    }
    let body_complete = match framing(&head) {
        Framing::Length(Some(length)) => body_bytes.len() == length,
        Framing::Length(None) => true, // delimited by a clean end of stream
        Framing::Chunked => dechunk(body_bytes).is_some(),
        Framing::Invalid => false,
    };
    if !body_complete {
        return broken("a body that does not match its declared framing");
    }
    let body = match framing(&head) {
        Framing::Chunked => dechunk(body_bytes).unwrap_or_default(),
        _ => body_bytes.to_vec(),
    };
    Reply::Status(code, String::from_utf8_lossy(&body).to_string(), head)
}

/// The most of a reply that is read; a reply that reaches it fails the check.
const MAX_REPLY: usize = 64 * 1024;

enum Framing {
    /// `Content-Length` (or none: the body runs to the end of the stream).
    Length(Option<usize>),
    Chunked,
    /// A `Content-Length` that is not a number, or two that disagree.
    Invalid,
}

fn framing(head: &str) -> Framing {
    let mut length = None;
    let mut chunked = false;
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "content-length" {
            match value.parse::<usize>() {
                Ok(parsed) if length.is_none() || length == Some(parsed) => length = Some(parsed),
                _ => return Framing::Invalid,
            }
        } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    if chunked {
        Framing::Chunked
    } else {
        Framing::Length(length)
    }
}

/// The body of a chunked reply, or `None` when it is malformed or has no terminating chunk.
fn dechunk(mut raw: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = raw.windows(2).position(|window| window == b"\r\n")?;
        let size_line = std::str::from_utf8(&raw[..end]).ok()?;
        let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
        raw = &raw[end + 2..];
        if size == 0 {
            // Optional trailers, then the blank line that ends the message.
            return (raw == b"\r\n" || raw.ends_with(b"\r\n\r\n")).then_some(body);
        }
        if raw.len() < size + 2 || &raw[size..size + 2] != b"\r\n" {
            return None;
        }
        body.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake metadata service. A route is `(method, path prefix, needs token, status, body)`;
    /// a request no route matches gets a 404.
    fn serve(routes: Vec<(&'static str, &'static str, bool, u16, &'static str)>) -> SocketAddr {
        serve_with_head(routes, "")
    }

    /// Like `serve`, with extra header lines (each ending in CRLF) on every reply.
    fn serve_with_head(
        routes: Vec<(&'static str, &'static str, bool, u16, &'static str)>,
        extra_head: &'static str,
    ) -> SocketAddr {
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
                        *m == method
                            && (if p.ends_with('/') {
                                path == *p
                            } else {
                                path.starts_with(p)
                            })
                            && (!*needs_token || has_token)
                    })
                    .map_or((404, ""), |(_, _, _, status, body)| (*status, *body));
                let _ = write!(
                    stream,
                    "HTTP/1.0 {status} X\r\n{extra_head}Content-Length: {}\r\n\r\n{body}",
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
    fn aws_credentials_obtained_with_a_token_fail_the_check() {
        let address = serve(vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 200, "role")]);
        let found = check(address).unwrap_err();
        assert_eq!((found.cloud, found.path), ("AWS", AWS_CREDENTIALS));
        let message = found.to_string();
        assert!(message.contains("IMDSv2") && message.contains("hop limit 1"));
    }

    #[test]
    fn the_aws_instance_id_obtained_with_a_token_fails_the_check() {
        let address = serve(vec![TOKEN, ("GET", AWS_INSTANCE_ID, true, 200, "i-1")]);
        assert_eq!(check(address).unwrap_err().path, AWS_INSTANCE_ID);
    }

    #[test]
    fn aws_data_without_a_token_fails_the_check() {
        let address = serve(vec![("GET", AWS_CREDENTIALS, false, 200, "role")]);
        assert!(check(address).unwrap_err().cloud.contains("IMDSv1"));
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
        assert_eq!(check(address).unwrap_err().cloud, "GCP");
    }

    /// GKE Workload Identity serves `instance/id` to every pod; it is not a credential, so it
    /// alone must not refuse the sandbox.
    #[test]
    fn a_gke_workload_identity_instance_id_alone_passes() {
        let address = serve(vec![(
            "GET",
            "/computeMetadata/v1/instance/id",
            false,
            200,
            "1234567890",
        )]);
        assert_eq!(check(address), Ok(()));
    }

    #[test]
    fn an_azure_instance_document_fails_the_check() {
        let address = serve(vec![(
            "GET",
            "/metadata/instance",
            false,
            200,
            "{\"compute\":{}}",
        )]);
        let found = check(address).unwrap_err();
        assert_eq!((found.cloud, found.path), ("Azure", "/metadata/instance"));
    }

    #[test]
    fn an_azure_instance_refusal_passes() {
        let address = serve(vec![("GET", "/metadata/instance", false, 403, "no")]);
        assert_eq!(check(address), Ok(()));
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
        assert_eq!(check(address).unwrap_err().cloud, "Azure");
    }

    #[test]
    fn an_azure_node_that_wants_an_identity_selector_fails_the_check() {
        let address = serve(vec![(
            "GET",
            "/metadata/identity/oauth2/token",
            false,
            400,
            "{\"error\":\"invalid_request\",\"error_description\":\"Multiple user assigned identities exist, please specify the clientId\"}",
        )]);
        assert_eq!(check(address).unwrap_err().cloud, "Azure");
    }

    /// A node with no identity answers the token request with a 400 "Identity not found": there
    /// is nothing to obtain, so that is a refusal and must not refuse the sandbox.
    #[test]
    fn an_azure_identity_not_found_passes() {
        let address = serve(vec![(
            "GET",
            "/metadata/identity/oauth2/token",
            false,
            400,
            "{\"error\":\"invalid_request\",\"error_description\":\"Identity not found\"}",
        )]);
        assert_eq!(check(address), Ok(()));
    }

    /// Fail closed: a 400 that says nothing about identities is not one of the refusals either.
    #[test]
    fn an_unexplained_azure_400_fails_closed() {
        let address = serve(vec![(
            "GET",
            "/metadata/identity/oauth2/token",
            false,
            400,
            "bad",
        )]);
        assert!(check(address).unwrap_err().unexpected.is_some());
    }

    /// A service that accepts the connection and never answers: no reply can be read, so the
    /// check cannot say nothing comes back. It fails, for the token request and for any later one.
    #[test]
    fn a_service_that_never_replies_fails_closed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream);
            }
        });
        let found = check(address).unwrap_err();
        assert_eq!(found.path, "/latest/api/token");
        assert!(found.unexpected.unwrap().contains("read timeout"));
    }

    #[test]
    fn a_token_request_answered_by_a_server_error_fails_closed() {
        let address = serve(vec![("PUT", "/latest/api/token", false, 503, "try later")]);
        assert_eq!(
            check(address).unwrap_err().unexpected.as_deref(),
            Some("HTTP 503")
        );
        let address = serve(vec![("PUT", "/latest/api/token", false, 429, "slow down")]);
        assert_eq!(
            check(address).unwrap_err().unexpected.as_deref(),
            Some("HTTP 429")
        );
    }

    #[test]
    fn a_redirect_fails_closed() {
        let address = serve(vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 302, "")]);
        let found = check(address).unwrap_err();
        assert_eq!(
            (found.path, found.unexpected.as_deref()),
            (AWS_CREDENTIALS, Some("HTTP 302"))
        );
    }

    #[test]
    fn a_later_request_answered_by_a_server_error_fails_closed() {
        let address = serve(vec![TOKEN, ("GET", AWS_INSTANCE_ID, false, 500, "boom")]);
        // The token path is fine; the unauthenticated instance id answers 500 with a body.
        let found = check(address).unwrap_err();
        assert_eq!(found.unexpected.as_deref(), Some("HTTP 500"));
    }

    #[test]
    fn an_unparseable_reply_fails_closed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"\x00\x01not http at all\r\n\r\nbody");
            }
        });
        assert!(check(address).unwrap_err().unexpected.is_some());
    }

    #[test]
    fn a_reply_cut_off_before_the_end_of_the_head_fails_closed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-le");
            }
        });
        assert!(check(address).unwrap_err().unexpected.is_some());
    }

    // The other credential endpoints, with the same rule.
    fn endpoint_at(address: SocketAddr) -> Endpoint {
        Endpoint {
            cloud: "test",
            address,
            method: "GET",
            path: "/v1/credentials",
            headers: &[],
            fix: "block it",
        }
    }

    #[test]
    fn another_endpoint_that_returns_credentials_fails_the_check() {
        let address = serve(vec![(
            "GET",
            "/v1/credentials",
            false,
            200,
            "{\"AccessKeyId\":\"x\"}",
        )]);
        let found = check_endpoint(&endpoint_at(address)).unwrap_err();
        assert_eq!((found.cloud, found.unexpected), ("test", None));
    }

    #[test]
    fn another_endpoint_that_refuses_passes() {
        for status in [401, 403, 404] {
            let address = serve(vec![("GET", "/v1/credentials", false, status, "no")]);
            assert_eq!(check_endpoint(&endpoint_at(address)), Ok(()), "{status}");
        }
    }

    #[test]
    fn another_endpoint_with_nothing_listening_passes() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(check_endpoint(&endpoint_at(address)), Ok(()));
    }

    #[test]
    fn another_endpoint_that_answers_something_else_fails_closed() {
        for status in [400, 500] {
            let address = serve(vec![("GET", "/v1/credentials", false, status, "?")]);
            let found = check_endpoint(&endpoint_at(address)).unwrap_err();
            assert_eq!(found.unexpected, Some(format!("HTTP {status}")));
        }
    }

    /// The five other endpoints are probed (so a node that exposes one is refused), by address.
    #[test]
    fn the_other_endpoints_cover_pod_identity_ecs_ipv6_gke_and_the_azure_wire_server() {
        let addresses: Vec<String> = OTHER_ENDPOINTS
            .iter()
            .map(|e| e.address.to_string())
            .collect();
        assert_eq!(
            addresses,
            [
                "169.254.170.23:80",
                "169.254.170.2:80",
                "[fd00:ec2::254]:80",
                "169.254.169.252:988",
                "168.63.129.16:80"
            ]
        );
    }

    /// An IPv6 endpoint is reached with a bracketed Host header.
    #[test]
    fn an_ipv6_endpoint_is_asked_with_a_bracketed_host() {
        let Ok(listener) = std::net::TcpListener::bind("[::1]:0") else {
            return; // no IPv6 loopback here
        };
        let address = listener.local_addr().unwrap();
        let seen = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 2048];
            let n = stream.read(&mut buffer).unwrap();
            let _ = stream.write_all(b"HTTP/1.0 404 X\r\nContent-Length: 0\r\n\r\n");
            String::from_utf8_lossy(&buffer[..n]).to_string()
        });
        assert_eq!(check_endpoint(&endpoint_at(address)), Ok(()));
        assert!(seen.join().unwrap().contains("Host: [::1]\r\n"));
    }
    // The other AWS metadata paths that identify the instance or carry credentials.
    #[test]
    fn user_data_with_content_fails_the_check() {
        let address = serve(vec![
            TOKEN,
            ("GET", "/latest/user-data", true, 200, "#!/bin/sh\necho x"),
        ]);
        assert_eq!(check(address).unwrap_err().path, "/latest/user-data");
    }

    #[test]
    fn the_instance_role_summary_fails_the_check() {
        let address = serve(vec![
            TOKEN,
            (
                "GET",
                "/latest/meta-data/iam/info",
                true,
                200,
                "{\"Code\":\"Success\"}",
            ),
        ]);
        assert_eq!(
            check(address).unwrap_err().path,
            "/latest/meta-data/iam/info"
        );
    }

    #[test]
    fn the_instances_own_credentials_fail_the_check() {
        let path = "/latest/meta-data/identity-credentials/ec2/security-credentials/ec2-instance";
        let address = serve(vec![
            TOKEN,
            ("GET", path, true, 200, "{\"AccessKeyId\":\"x\"}"),
        ]);
        assert_eq!(check(address).unwrap_err().path, path);
    }

    #[test]
    fn the_signed_identity_document_fails_the_check() {
        for path in [
            "/latest/dynamic/instance-identity/document",
            "/latest/dynamic/instance-identity/pkcs7",
            "/latest/dynamic/instance-identity/signature",
        ] {
            let address = serve(vec![TOKEN, ("GET", path, true, 200, "signed")]);
            assert_eq!(check(address).unwrap_err().path, path);
        }
    }

    #[test]
    fn those_paths_without_a_token_fail_the_check_too() {
        let address = serve(vec![("GET", "/latest/user-data", false, 200, "data")]);
        assert!(check(address)
            .unwrap_err()
            .cloud
            .contains("no session token"));
    }

    #[test]
    fn refused_user_data_and_identity_paths_pass() {
        let routes = AWS_OTHER_PATHS
            .iter()
            .map(|path| ("GET", *path, true, 404, ""))
            .collect();
        assert_eq!(check(serve(routes)), Ok(()));
    }

    // Pod Identity.
    fn pod_identity_at(address: SocketAddr) -> Endpoint {
        Endpoint {
            cloud: "pod identity",
            address,
            method: "GET",
            path: "/v1/credentials",
            headers: &[],
            fix: "block it",
        }
    }

    #[test]
    fn a_400_from_pod_identity_passes_when_no_token_is_readable() {
        let address = serve(vec![(
            "GET",
            "/v1/credentials",
            false,
            400,
            "missing token",
        )]);
        assert_eq!(check_pod_identity(&pod_identity_at(address), &[]), Ok(()));
    }

    #[test]
    fn credentials_from_pod_identity_with_a_readable_token_fail_the_check() {
        let address = serve(vec![(
            "GET",
            "/v1/credentials",
            false,
            200,
            "{\"AccessKeyId\":\"x\"}",
        )]);
        let found =
            check_pod_identity(&pod_identity_at(address), &["a-token".to_string()]).unwrap_err();
        assert_eq!(found.unexpected, None);
    }

    #[test]
    fn a_refusal_from_pod_identity_with_a_readable_token_passes() {
        let address = serve(vec![("GET", "/v1/credentials", false, 403, "no")]);
        assert_eq!(
            check_pod_identity(&pod_identity_at(address), &["a-token".to_string()]),
            Ok(())
        );
    }

    #[test]
    fn a_400_from_pod_identity_with_a_readable_token_fails_closed() {
        let address = serve(vec![("GET", "/v1/credentials", false, 400, "?")]);
        let found =
            check_pod_identity(&pod_identity_at(address), &["a-token".to_string()]).unwrap_err();
        assert_eq!(found.unexpected.as_deref(), Some("HTTP 400"));
    }

    #[test]
    fn the_token_is_presented_as_the_authorization_header() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let seen = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 2048];
            let n = stream.read(&mut buffer).unwrap();
            let _ = stream.write_all(b"HTTP/1.0 403 X\r\nContent-Length: 0\r\n\r\n");
            String::from_utf8_lossy(&buffer[..n]).to_string()
        });
        assert_eq!(
            check_pod_identity(&pod_identity_at(address), &["tok-123".to_string()]),
            Ok(())
        );
        assert!(seen.join().unwrap().contains("Authorization: tok-123\r\n"));
    }

    // The listing is an allow list.
    const LISTING: &str = "/latest/meta-data/";

    /// The measured Auto Mode listing: only `placement/` and `services/`.
    #[test]
    fn the_measured_auto_mode_listing_passes() {
        let address = serve(vec![
            TOKEN,
            ("GET", LISTING, true, 200, "placement/\nservices/"),
            (
                "GET",
                "/latest/meta-data/placement/region",
                true,
                200,
                "us-east-2",
            ),
        ]);
        assert_eq!(check(address), Ok(()));
    }

    #[test]
    fn a_listing_with_an_added_entry_fails_the_check() {
        for entry in ["tags/", "hostname", "iam/", "instance-id"] {
            let body: &'static str =
                Box::leak(format!("placement/\nservices/\n{entry}").into_boxed_str());
            let address = serve(vec![TOKEN, ("GET", LISTING, true, 200, body)]);
            let found = check(address).unwrap_err();
            assert_eq!((found.path, found.unexpected), (LISTING, None), "{entry}");
        }
    }

    #[test]
    fn an_unauthenticated_listing_with_an_added_entry_fails_the_check() {
        let address = serve(vec![("GET", LISTING, false, 200, "placement/\ntags/")]);
        assert!(check(address)
            .unwrap_err()
            .cloud
            .contains("no session token"));
    }

    // Replies that carry nothing, or too much.
    #[test]
    fn a_2xx_with_an_empty_body_fails_closed() {
        let address = serve(vec![("PUT", "/latest/api/token", false, 200, "")]);
        assert_eq!(
            check(address).unwrap_err().unexpected.as_deref(),
            Some("HTTP 200")
        );
        let address = serve(vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 204, "")]);
        assert_eq!(
            check(address).unwrap_err().unexpected.as_deref(),
            Some("HTTP 204")
        );
    }

    #[test]
    fn an_oversized_body_is_data() {
        let big: &'static str = Box::leak("x".repeat(200_000).into_boxed_str());
        let address = serve(vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 200, big)]);
        assert_eq!(check(address).unwrap_err().path, AWS_CREDENTIALS);
    }

    #[test]
    fn a_reply_that_does_not_start_with_http_fails_closed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(b"SIP/2.0 404 Not Found\r\n\r\n");
            }
        });
        assert!(check(address)
            .unwrap_err()
            .unexpected
            .unwrap()
            .contains("status line"));
    }

    // Local connect errors are not "unreachable".
    #[test]
    fn only_refused_timeout_and_no_route_count_as_unreachable() {
        use std::io::{Error, ErrorKind};
        for kind in [
            ErrorKind::ConnectionRefused,
            ErrorKind::TimedOut,
            ErrorKind::NetworkUnreachable,
            ErrorKind::HostUnreachable,
        ] {
            assert!(is_unreachable(&Error::from(kind)), "{kind:?}");
        }
        for error in [
            Error::from_raw_os_error(24), // EMFILE: too many open files
            Error::from_raw_os_error(97), // EAFNOSUPPORT: address family not supported
            Error::from_raw_os_error(99), // EADDRNOTAVAIL
            Error::from(ErrorKind::PermissionDenied),
            Error::from(ErrorKind::Other),
        ] {
            assert!(!is_unreachable(&error), "{error}");
        }
    }

    // Other clouds' services answer the AWS-shaped requests with their own errors.
    const GOOGLE: &str = "Metadata-Flavor: Google\r\n";

    /// Recorded shape (GCE/GKE metadata server documentation): every reply carries
    /// `Metadata-Flavor: Google`; a request it does not serve is an error, here a 405 for the PUT.
    #[test]
    fn a_gke_metadata_server_answering_the_aws_requests_with_errors_passes() {
        let address = serve_with_head(
            vec![("PUT", "/latest/api/token", false, 405, "Method Not Allowed")],
            GOOGLE,
        );
        assert_eq!(check(address), Ok(()));
    }

    /// Recorded shape (Azure Instance Metadata Service documentation): a request without the
    /// `Metadata: true` header gets a 400 "Required metadata header not specified".
    #[test]
    fn an_azure_service_answering_the_aws_requests_with_a_400_passes() {
        const MISSING: &str = "{\"error\":\"Bad request. Required metadata header not specified\"}";
        let address = serve(vec![
            ("PUT", "/latest", false, 400, MISSING),
            ("GET", "/latest", false, 400, MISSING),
        ]);
        assert_eq!(check(address), Ok(()));
    }

    /// A 400 with no recognizable cloud behind it is still not a refusal.
    #[test]
    fn an_unexplained_400_on_the_token_request_fails_closed() {
        let address = serve(vec![("PUT", "/latest/api/token", false, 400, "bad")]);
        assert_eq!(
            check(address).unwrap_err().unexpected.as_deref(),
            Some("HTTP 400")
        );
    }

    /// The header never forgives data: a reply that claims to be Google's but serves a token and
    /// then credentials, or instance data, or a redirect or server error, fails.
    #[test]
    fn a_google_header_does_not_excuse_data_or_other_statuses() {
        let address = serve_with_head(
            vec![TOKEN, ("GET", AWS_CREDENTIALS, true, 200, "role")],
            GOOGLE,
        );
        assert_eq!(check(address).unwrap_err().path, AWS_CREDENTIALS);
        let address = serve_with_head(
            vec![
                TOKEN,
                ("GET", LISTING, true, 200, "placement/\nservices/\ntags/"),
            ],
            GOOGLE,
        );
        assert_eq!(check(address).unwrap_err().path, LISTING);
        let address = serve_with_head(
            vec![("GET", "/latest/user-data", false, 200, "secret")],
            GOOGLE,
        );
        assert_eq!(check(address).unwrap_err().path, "/latest/user-data");
        for status in [302, 500, 503] {
            let address = serve_with_head(
                vec![("PUT", "/latest/api/token", false, status, "x")],
                GOOGLE,
            );
            assert!(check(address).unwrap_err().unexpected.is_some(), "{status}");
        }
    }

    // Pod Identity token sources.
    fn temp_file(name: &str, contents: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("alien-metadata-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn pod_identity_tokens_come_from_the_environment_and_the_files() {
        let named = temp_file("named", "  from-named-file \n");
        let default = temp_file("default", "from-default-file");
        let empty = temp_file("empty", " \n");
        let tokens = read_tokens(
            Some(" from-env ".to_string()),
            Some(named.clone()),
            &[default.as_str(), empty.as_str(), "/nonexistent/token"],
        );
        assert_eq!(tokens, ["from-env", "from-named-file", "from-default-file"]);
        assert!(read_tokens(None, None, &["/nonexistent/token"]).is_empty());
        for path in [named, default, empty] {
            let _ = std::fs::remove_file(path);
        }
    }

    // A reply is only accepted when it is complete.
    /// Sends `reply` for the first request on every connection, then either closes or stalls.
    fn raw_server(reply: &'static [u8], stall: bool) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0; 2048];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(reply);
                if stall {
                    held.push(stream);
                }
            }
        });
        address
    }

    #[test]
    fn a_body_shorter_than_its_content_length_fails_closed_after_a_close() {
        // The listing declares 22 bytes, sends `placement/` and closes.
        let address = raw_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 22\r\n\r\nplacement/",
            false,
        );
        assert!(check(address)
            .unwrap_err()
            .unexpected
            .unwrap()
            .contains("declared framing"));
    }

    #[test]
    fn a_body_shorter_than_its_content_length_fails_closed_after_a_stall() {
        let address = raw_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 22\r\n\r\nplacement/",
            true,
        );
        assert!(check(address)
            .unwrap_err()
            .unexpected
            .unwrap()
            .contains("cut off"));
    }

    #[test]
    fn a_chunked_body_without_its_terminating_chunk_fails_closed() {
        let address = raw_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\na\r\nplacement/\r\n",
            false,
        );
        assert!(check(address)
            .unwrap_err()
            .unexpected
            .unwrap()
            .contains("declared framing"));
        let address = raw_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\na\r\nplacement/\r\n",
            true,
        );
        assert!(check(address).unwrap_err().unexpected.is_some());
    }

    #[test]
    fn a_refusal_with_a_cut_off_body_fails_closed_too() {
        // Every probe, not only the listing: a 404 that declares more than it sends.
        let address = raw_server(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 50\r\n\r\nno",
            false,
        );
        assert!(check(address).unwrap_err().unexpected.is_some());
    }

    #[test]
    fn complete_replies_still_pass() {
        // Content-Length exactly met, a chunked 404 with its terminator, and a body to the end of
        // the stream with no length.
        for reply in [
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\n\r\nno"[..],
            b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nno\r\n0\r\n\r\n",
            b"HTTP/1.0 404 Not Found\r\n\r\nno",
        ] {
            assert_eq!(check(raw_server(reply, false)), Ok(()), "{reply:?}");
        }
    }

    #[test]
    fn a_complete_chunked_listing_is_read_as_a_list() {
        let address = raw_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1a\r\nplacement/\nservices/\ntags/\r\n0\r\n\r\n",
            false,
        );
        // The unauthenticated listing is this server's only reply: the extra entry fails it.
        assert_eq!(check(address).unwrap_err().path, LISTING);
    }

    #[test]
    fn dechunk_reads_chunks_and_rejects_malformed_input() {
        assert_eq!(dechunk(b"3\r\nabc\r\n0\r\n\r\n"), Some(b"abc".to_vec()));
        assert_eq!(
            dechunk(b"3\r\nabc\r\n0\r\nX-T: 1\r\n\r\n"),
            Some(b"abc".to_vec())
        );
        assert_eq!(dechunk(b"3\r\nabc\r\n"), None);
        assert_eq!(dechunk(b"zz\r\nabc\r\n0\r\n\r\n"), None);
        assert_eq!(dechunk(b"5\r\nabc\r\n0\r\n\r\n"), None);
    }

    #[test]
    fn a_reply_that_fills_the_read_limit_fails_closed() {
        // A 2xx listing that is only `placement/` lines but does not fit the limit: the rest was
        // never seen.
        let body = "placement/\n".repeat(MAX_REPLY / 11 + 10);
        let reply: &'static [u8] = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
            .into_boxed_slice(),
        );
        let found = check(raw_server(reply, false)).unwrap_err();
        assert!(found.unexpected.unwrap().contains("read limit"));
        // A refusal that large fails too.
        let reply: &'static [u8] = Box::leak(
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
            .into_boxed_slice(),
        );
        assert!(check(raw_server(reply, false))
            .unwrap_err()
            .unexpected
            .is_some());
    }
}
