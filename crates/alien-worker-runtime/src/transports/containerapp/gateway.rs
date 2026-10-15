//! Authenticate gateway metadata before restoring application HTTP routing.

use alien_error::AlienError;
use axum::http::{header::HOST, uri::Authority, HeaderMap, Request, StatusCode, Uri};
use subtle::ConstantTimeEq;

use crate::error::{ErrorData, Result};

const TOKEN: &str = "x-alien-gateway-token";
const ORIGINAL_HOST: &str = "x-alien-original-host";
const ORIGINAL_TARGET: &str = "x-alien-original-target";
const HEADERS: [&str; 3] = [TOKEN, ORIGINAL_HOST, ORIGINAL_TARGET];

// Deliberately has no Debug implementation: credentials must not enter logs.
pub(super) struct GatewayCredential([u8; 64]);

impl GatewayCredential {
    pub(super) fn new(value: &str) -> Result<Self> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "Gateway credential must encode 32 random bytes as hexadecimal".into(),
                field: Some("gatewayCredential".into()),
            }));
        }
        let mut bytes = [0; 64];
        bytes.copy_from_slice(value.as_bytes());
        Ok(Self(bytes))
    }
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> std::result::Result<&'a str, StatusCode> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or(StatusCode::FORBIDDEN)?;
    if values.next().is_some() {
        return Err(StatusCode::FORBIDDEN);
    }
    value.to_str().map_err(|_| StatusCode::FORBIDDEN)
}

/// Return true only for authenticated application HTTP; absent metadata retains
/// native trigger handling. Partial or ambiguous metadata never falls through.
pub(super) fn restore_request<B>(
    request: &mut Request<B>,
    credential: Option<&GatewayCredential>,
) -> std::result::Result<bool, StatusCode> {
    if !HEADERS
        .iter()
        .any(|name| request.headers().contains_key(*name))
    {
        return Ok(false);
    }
    let credential = credential.ok_or(StatusCode::FORBIDDEN)?;
    let token = one_header(request.headers(), TOKEN)?;
    if !bool::from(credential.0.as_slice().ct_eq(token.as_bytes())) {
        return Err(StatusCode::FORBIDDEN);
    }
    let host = one_header(request.headers(), ORIGINAL_HOST)?;
    let authority = host
        .parse::<Authority>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if authority.host().is_empty() || host.contains('@') || host.contains(',') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let target = one_header(request.headers(), ORIGINAL_TARGET)?;
    if !target.starts_with('/') || target.contains('#') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let uri = target.parse::<Uri>().map_err(|_| StatusCode::BAD_REQUEST)?;
    if uri.scheme().is_some() || uri.authority().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let host = request.headers()[ORIGINAL_HOST].clone();
    *request.uri_mut() = uri;
    request.headers_mut().insert(HOST, host);
    for name in HEADERS {
        request.headers_mut().remove(name);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn request() -> Request<()> {
        Request::builder()
            .uri("/rewritten")
            .header(TOKEN, SECRET)
            .header(ORIGINAL_HOST, "nested.private.example.test:443")
            .header(ORIGINAL_TARGET, "/a%2Fb?x=1&x=2&space=%20")
            .body(())
            .unwrap()
    }

    #[test]
    fn authenticated_metadata_restores_exact_target_and_removes_credential() {
        let mut request = request();
        let credential = GatewayCredential::new(SECRET).unwrap();
        assert_eq!(restore_request(&mut request, Some(&credential)), Ok(true));
        assert_eq!(request.uri().to_string(), "/a%2Fb?x=1&x=2&space=%20");
        assert_eq!(request.headers()[HOST], "nested.private.example.test:443");
        assert!(HEADERS
            .iter()
            .all(|name| !request.headers().contains_key(*name)));
    }

    #[test]
    fn missing_wrong_duplicate_and_disabled_credentials_are_rejected() {
        let credential = GatewayCredential::new(SECRET).unwrap();
        assert_eq!(
            restore_request(&mut request(), None),
            Err(StatusCode::FORBIDDEN)
        );
        for name in HEADERS {
            let mut missing = request();
            missing.headers_mut().remove(name);
            assert_eq!(
                restore_request(&mut missing, Some(&credential)),
                Err(StatusCode::FORBIDDEN)
            );
            let mut duplicate = request();
            let value = duplicate.headers()[name].clone();
            duplicate.headers_mut().append(name, value);
            assert_eq!(
                restore_request(&mut duplicate, Some(&credential)),
                Err(StatusCode::FORBIDDEN)
            );
        }
        let mut wrong = request();
        wrong.headers_mut().insert(
            TOKEN,
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            restore_request(&mut wrong, Some(&credential)),
            Err(StatusCode::FORBIDDEN)
        );
        let mut native = Request::new(());
        assert_eq!(restore_request(&mut native, Some(&credential)), Ok(false));
        assert!(GatewayCredential::new("").is_err());
    }

    #[test]
    fn malformed_authenticated_routing_is_rejected() {
        let credential = GatewayCredential::new(SECRET).unwrap();
        for (name, value) in [
            (ORIGINAL_HOST, "user@example.test"),
            (ORIGINAL_HOST, "one,two"),
            (ORIGINAL_TARGET, "https://foreign.test/"),
            (ORIGINAL_TARGET, "/foo#fragment"),
        ] {
            let mut request = request();
            request.headers_mut().insert(name, value.parse().unwrap());
            assert_eq!(
                restore_request(&mut request, Some(&credential)),
                Err(StatusCode::BAD_REQUEST)
            );
        }
    }
}
