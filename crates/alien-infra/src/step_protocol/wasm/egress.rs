//! Requests a controller module sends through `alien_http_v1::send`: to an origin its extension
//! declared, such as the vendor's own API. The host attaches none of the deployment's
//! credentials to them.
//!
//! The origin must be one the extension lists, exactly. The host resolves it when the request
//! is sent, refuses it when any address is not public (loopback, private, link-local, the
//! metadata service), and connects only to the addresses it checked, so a name that resolves to
//! a public address once and a private one next reaches neither.

use super::endpoint::body_bytes;
use reqwest::Url;
use serde::{Deserialize, Serialize};

/// A request to a declared origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpRequest {
    pub method: String,
    /// An absolute `https` URL on a declared origin.
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// A body that is text.
    #[serde(default)]
    pub body: Option<String>,
    /// A body of any bytes, in base64, in place of `body`.
    #[serde(default)]
    pub body_base64: Option<String>,
}

const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// Headers a module can't set: the connection's, a proxy's, and those a front end reads as the
/// client's address or the request's host. `Authorization` is the module's to set: the key it
/// sends there is its vendor's, not the deployment's.
const REFUSED_HEADERS: [&str; 11] = [
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "upgrade",
    "proxy-authorization",
    "proxy-connection",
    "forwarded",
    "te",
    "trailer",
    "keep-alive",
];

/// The origin `declared` names, when it is written the one way an origin is: `https://`, a host
/// in lowercase, a port only when it is not 443, and nothing after. The host is one name or
/// address: an origin is matched whole, so a pattern would match nothing. `allow_local` admits
/// `http`, for a server a test runs on this machine.
pub(super) fn declared_origin(declared: &str, allow_local: bool) -> Result<String, String> {
    let url = Url::parse(declared).map_err(|e| format!("it is not a URL: {e}"))?;
    if url.scheme() != "https" && !(allow_local && url.scheme() == "http") {
        return Err(format!("it uses `{}`, not https", url.scheme()));
    }
    let host = url.host_str().unwrap_or_default();
    // The characters of a name, and of an address with its brackets.
    let named = |c: char| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | ':' | '[' | ']')
    };
    if !host.chars().all(named) {
        return Err(format!(
            "'{}' is not a host name or an address",
            host.escape_debug()
        ));
    }
    let origin = url.origin().ascii_serialization();
    if origin != declared {
        return Err(format!(
            "it is not written as an origin, which would be '{origin}'"
        ));
    }
    Ok(origin)
}

/// Checks `request` against `declared`, the origins the extension lists as [`declared_origin`]
/// returns them, and returns its URL and its body. `allow_local` admits `http`, for a server a test runs on this machine.
pub(super) fn check(
    request: &HttpRequest,
    declared: &[String],
    allow_local: bool,
) -> Result<(Url, Option<Vec<u8>>), String> {
    let url = Url::parse(&request.url)
        .map_err(|e| format!("'{}' is not a URL: {e}", request.url.escape_debug()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the URL carries credentials".to_string());
    }
    if url.fragment().is_some() {
        return Err("the URL has a fragment".to_string());
    }
    if url.scheme() != "https" && !(allow_local && url.scheme() == "http") {
        return Err(format!("the URL uses `{}`, not https", url.scheme()));
    }
    let origin = url.origin().ascii_serialization();
    if !declared.contains(&origin) {
        return Err(format!("{origin} is not an origin the extension declares"));
    }
    if !METHODS.contains(&request.method.as_str()) {
        return Err(format!(
            "the method '{}' is not allowed",
            request.method.escape_debug()
        ));
    }
    for (name, value) in &request.headers {
        let lowered = name.to_ascii_lowercase();
        if REFUSED_HEADERS.contains(&lowered.as_str()) || lowered.starts_with("x-forwarded-") {
            return Err(format!(
                "the '{}' header is the host's to set",
                name.escape_debug()
            ));
        }
        if value.chars().any(|c| c.is_control()) {
            return Err(format!(
                "the '{}' header has a control character",
                name.escape_debug()
            ));
        }
    }
    let body = body_bytes(request.body.as_deref(), request.body_base64.as_deref())?;
    Ok((url, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![],
            body: None,
            body_base64: None,
        }
    }

    fn declared() -> Vec<String> {
        ["https://api.acme.dev", "https://api.vendor.example:8443"]
            .map(|origin| declared_origin(origin, false).unwrap())
            .to_vec()
    }

    fn checked(request: &HttpRequest) -> Result<Url, String> {
        check(request, &declared(), false).map(|(url, _)| url)
    }

    #[test]
    fn only_declared_origins_pass() {
        assert!(checked(&get("https://api.acme.dev/v1/zones?x=1")).is_ok());
        assert!(checked(&get("https://api.vendor.example:8443/v4/zones")).is_ok());
        // Hosts compare as the URL parser reads them.
        assert!(checked(&get("https://API.acme.dev/")).is_ok());
        assert!(checked(&get("https://api.acme.dev:443/")).is_ok());
        for url in [
            "https://api.acme.dev.evil.com/",
            "https://evil.com/?https://api.acme.dev",
            "https://evil.com/https://api.acme.dev",
            "https://user:pass@api.acme.dev/",
            "https://api.acme.dev@evil.com/",
            "https://api.acme.dev:8443/",
            "https://api.vendor.example/",
            "http://api.acme.dev/",
            "https://169.254.169.254/latest/meta-data/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://api.acme.dev/#x",
            "file:///etc/passwd",
            "api.acme.dev/v1",
            "",
        ] {
            assert!(checked(&get(url)).is_err(), "{url}");
        }
    }

    /// With no origin declared, no request passes.
    #[test]
    fn an_extension_that_declares_no_origin_reaches_none() {
        let refusal = check(&get("https://api.acme.dev/"), &[], false).unwrap_err();
        assert!(refusal.contains("not an origin the extension declares"));
    }

    #[test]
    fn a_declared_origin_is_written_one_way() {
        for origin in ["https://api.acme.dev", "https://api.acme.dev:8443"] {
            assert_eq!(declared_origin(origin, false).unwrap(), origin);
        }
        for origin in [
            "http://api.acme.dev",
            "https://api.acme.dev/",
            "https://api.acme.dev/v1",
            "https://API.acme.dev",
            "https://api.acme.dev:443",
            "https://user@api.acme.dev",
            "https://api.acme.dev?x=1",
            "https://*.acme.dev/",
            "https://*.acme.dev",
            "https://api_internal.acme.dev",
            "api.acme.dev",
            "file:///etc",
            "",
        ] {
            assert!(declared_origin(origin, false).is_err(), "{origin}");
        }
        // A test's server on this machine is declared over http only where the host allows it.
        assert!(declared_origin("http://127.0.0.1:8080", true).is_ok());
        assert!(declared_origin("http://127.0.0.1:8080", false).is_err());
    }

    #[test]
    fn routing_and_framing_headers_are_refused_and_the_vendor_s_key_passes() {
        for name in [
            "Host",
            "X-Forwarded-For",
            "Forwarded",
            "Transfer-Encoding",
            "Proxy-Authorization",
            "Content-Length",
            "Upgrade",
        ] {
            let mut request = get("https://api.acme.dev/");
            request.headers = vec![(name.into(), "x".into())];
            assert!(checked(&request).is_err(), "{name}");
        }
        let mut request = get("https://api.acme.dev/");
        request.headers = vec![("authorization".into(), "Bearer vendor-token".into())];
        assert!(checked(&request).is_ok());
        request.headers = vec![("x-api-key".into(), "a\r\nHost: evil".into())];
        assert!(checked(&request).is_err());
    }

    #[test]
    fn a_body_is_text_or_bytes_and_bounded() {
        let mut request = get("https://api.acme.dev/");
        request.method = "POST".into();
        request.body_base64 = Some("AJ//".into());
        let (_, body) = check(&request, &declared(), false).unwrap();
        assert_eq!(body, Some(vec![0, 159, 255]));
        request.body = Some("both".into());
        assert!(check(&request, &declared(), false).is_err());
        request.body_base64 = None;
        request.body = Some("x".repeat(super::super::endpoint::MAX_REQUEST_BODY + 1));
        assert!(check(&request, &declared(), false).is_err());
        request.method = "TRACE".into();
        request.body = None;
        assert!(check(&request, &declared(), false).is_err());
    }
}
