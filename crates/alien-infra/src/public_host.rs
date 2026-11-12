//! Clients for URLs someone other than Alien chooses, such as a log export's destination or an
//! extension's controller server. Each client reaches only the public addresses the URL's host
//! resolves to when the client is built, so the URL can't point Alien's calls into a private
//! network or at a cloud's metadata service, before or after a DNS change.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// How long resolving the host may take.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether `ip` is a public unicast address. Private, loopback, link-local, carrier-grade NAT,
/// benchmarking, multicast and reserved IPv4 addresses are not, and neither are IPv6 addresses
/// outside global unicast or in a transition range that can route to an embedded IPv4 address.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0)
                && !(a == 198 && matches!(b, 18 | 19))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && matches!(segments[1], 0 | 0xdb8))
        }
    }
}

/// Why a client for a URL wasn't built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The URL breaks a rule, or no client can be built for it: the same way on every try.
    Policy(String),
    /// Its host didn't resolve, which another try may fix.
    Unreachable(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Policy(reason) | Refusal::Unreachable(reason) => f.write_str(reason),
        }
    }
}

/// A client from `builder` that connects only to the addresses `url`'s host resolves to now,
/// with no proxy and no redirects, so it reaches no other host. Refused, with the reason, when
/// the URL isn't `https`, carries credentials, or its host resolves to any address that isn't
/// public.
///
/// `allow_local` admits `localhost`, `127.0.0.1` and `::1` over `http` or `https`, for a server
/// a test runs on this machine.
pub async fn pinned_client(
    url: &reqwest::Url,
    builder: reqwest::ClientBuilder,
    allow_local: bool,
) -> Result<reqwest::Client, Refusal> {
    use Refusal::{Policy, Unreachable};
    let host = url
        .host_str()
        .ok_or_else(|| Policy("the URL has no host".to_string()))?
        .trim_matches(['[', ']']);
    let local = allow_local && matches!(host, "localhost" | "127.0.0.1" | "::1");
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Policy("the URL carries credentials".to_string()));
    }
    if url.scheme() != "https" && !(local && url.scheme() == "http") {
        return Err(Policy(format!(
            "the URL uses `{}`, not https",
            url.scheme()
        )));
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Policy("the URL has no port".to_string()))?;
    let addresses: Vec<SocketAddr> =
        tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| {
                Unreachable(format!(
                    "resolving {host} took longer than {RESOLVE_TIMEOUT:?}"
                ))
            })?
            .map_err(|e| Unreachable(format!("cannot resolve {host}: {e}")))?
            .collect();
    if addresses.is_empty() {
        return Err(Unreachable(format!("{host} resolves to no address")));
    }
    if !local {
        if let Some(address) = addresses.iter().find(|address| !public_ip(address.ip())) {
            return Err(Policy(format!(
                "{host} resolves to {}, which is not a public address",
                address.ip()
            )));
        }
    }
    builder
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| Policy(format!("cannot build the client: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn refusal(url: &str) -> String {
        pinned_client(
            &reqwest::Url::parse(url).unwrap(),
            reqwest::Client::builder(),
            false,
        )
        .await
        .expect_err(url)
        .to_string()
    }

    /// A URL that breaks a rule is refused for good; a host that doesn't resolve may resolve
    /// on another try.
    #[tokio::test]
    async fn a_rule_is_a_policy_refusal_and_a_lookup_failure_is_not() {
        let refuse = |url: &'static str| async move {
            pinned_client(
                &reqwest::Url::parse(url).unwrap(),
                reqwest::Client::builder(),
                false,
            )
            .await
            .expect_err(url)
        };
        for url in [
            "http://example.com",
            "https://10.0.0.5",
            "https://u:p@example.com",
        ] {
            assert!(matches!(refuse(url).await, Refusal::Policy(_)), "{url}");
        }
        assert!(matches!(
            refuse("https://no-such-host.invalid").await,
            Refusal::Unreachable(_)
        ));
    }

    /// Private, loopback and link-local addresses, by address and by a name that resolves to
    /// one, are refused before any connection.
    #[tokio::test]
    async fn a_private_loopback_or_link_local_host_is_refused() {
        for url in [
            "https://10.0.0.5",
            "https://172.16.3.4",
            "https://192.168.1.1",
            "https://127.0.0.1",
            "https://localhost",
            "https://[::1]",
            "https://169.254.169.254",
            "https://[fe80::1]",
            "https://[fd00::1]",
            "https://100.64.0.1",
            "https://[::ffff:10.0.0.1]",
            "https://[2002:a00:1::]",
        ] {
            let refusal = refusal(url).await;
            assert!(refusal.contains("not a public address"), "{url}: {refusal}");
        }
    }

    #[tokio::test]
    async fn only_https_without_credentials_is_admitted() {
        assert!(refusal("http://198.41.0.4").await.contains("not https"));
        assert!(refusal("https://user:pw@198.41.0.4")
            .await
            .contains("credentials"));
        pinned_client(
            &reqwest::Url::parse("https://198.41.0.4:8443/step").unwrap(),
            reqwest::Client::builder(),
            false,
        )
        .await
        .unwrap();
    }

    /// A test's local server is reachable only when the caller allows it.
    #[tokio::test]
    async fn a_local_host_needs_allow_local() {
        let url = reqwest::Url::parse("http://127.0.0.1:9").unwrap();
        pinned_client(&url, reqwest::Client::builder(), true)
            .await
            .unwrap();
        assert!(pinned_client(&url, reqwest::Client::builder(), false)
            .await
            .is_err());
    }

    #[test]
    fn transition_and_public_addresses() {
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
        assert!(public_ip("198.41.0.4".parse().unwrap()));
        assert!(!public_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!public_ip(
            "2001:0:4136:e378:8000:63bf:f5ff:fffe".parse().unwrap()
        ));
        assert!(!public_ip("169.254.169.254".parse().unwrap()));
    }

    /// NAT64, IPv4-compatible, Teredo, documentation, multicast and reserved ranges, and the
    /// IPv4 special-purpose ranges, are not public; the addresses just outside them are.
    #[test]
    fn transition_ranges_and_special_v4_are_not_public() {
        for ip in [
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b:1::a00:1",
            "::127.0.0.1",
            "::ffff:169.254.169.254",
            "::",
            "::1",
            "fec0::1",
            "ff02::1",
            "100::1",
            "2002:7f00:1::",
            "2001::1",
            "2001:db8::1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.0.1",
            "198.18.0.1",
            "100.127.255.255",
            "172.31.255.255",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip} must not be public");
        }
        for ip in ["8.8.8.8", "100.128.0.1", "2a00:1450:4001::1"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip} is public");
        }
    }

    /// Integer, hex and short spellings of loopback, and mapped or NAT64 forms of loopback and
    /// the metadata address, are refused.
    #[tokio::test]
    async fn other_spellings_of_loopback_and_metadata_are_refused() {
        for url in [
            "https://2130706433",
            "https://0x7f000001",
            "https://127.1",
            "https://[::ffff:7f00:1]",
            "https://[64:ff9b::a9fe:a9fe]",
            "https://[::]",
            "https://0.0.0.0",
            "http://localhost",
        ] {
            let refusal = refusal(url).await;
            assert!(
                refusal.contains("not a public address") || refusal.contains("not https"),
                "{url}: {refusal}"
            );
        }
    }
}
