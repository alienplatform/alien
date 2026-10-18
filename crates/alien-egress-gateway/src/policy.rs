use std::net::{IpAddr, Ipv4Addr};

use alien_error::ContextError;
use ipnet::Ipv4Net;
use sha2::{Digest, Sha256};

use crate::error::{denied, invalid, Result};

pub use alien_core::SandboxEgress as Policy;

/// Maximum canonical policy payload shared by all providers.
pub const MAX_POLICY_BYTES: usize = 256 * 1024;

/// A normalized destination. IPv6 is deliberately unsupported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// ASCII lowercase DNS name or canonical IPv4 address.
    pub host: String,
    /// Explicit TCP port.
    pub port: u16,
}

impl Target {
    /// Parse a host:port authority, requiring a port when no default is supplied.
    pub fn parse(value: &str, default: Option<u16>) -> Result<Self> {
        let (host, port) = match value.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse::<u16>().map_err(|_| invalid("invalid port"))?,
            ),
            None => (value, default.ok_or_else(|| invalid("port required"))?),
        };
        if port == 0 {
            return Err(invalid("zero port"));
        }
        Ok(Self {
            host: normalize_host(host)?,
            port,
        })
    }

    /// Authority formatted for HTTP requests.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Normalize a DNS name without accepting URL/userinfo/address-parser ambiguities.
pub fn normalize_host(host: &str) -> Result<String> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 || host.contains(['/', ':', '@', '%', '\\', '[', ']']) {
        return Err(invalid("invalid host"));
    }
    let ascii = url::Host::parse(host).map_err(|_| invalid("invalid host"))?;
    match ascii {
        url::Host::Ipv4(ip) => {
            // Refuse shorthand, octal, hexadecimal, and integer IP spellings.
            if host != ip.to_string() {
                return Err(invalid("noncanonical IP literal"));
            }
            Ok(ip.to_string())
        }
        url::Host::Ipv6(_) => Err(denied("IPv6 disabled")),
        url::Host::Domain(name) => {
            if name.split('.').any(|s| {
                s.is_empty()
                    || s.len() > 63
                    || s.starts_with('-')
                    || s.ends_with('-')
                    || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }) {
                return Err(invalid("invalid DNS name"));
            }
            Ok(name)
        }
    }
}

pub trait PolicyExt {
    fn permits(&self, target: &Target) -> bool;
    fn intersection_permits(&self, other: &Self, target: &Target) -> bool;
}

impl PolicyExt for Policy {
    /// Both policies must permit the target. A literal needs an explicit rule in at least one
    /// policy; Internet is the upper bound for an explicitly narrowed literal grant.
    fn intersection_permits(&self, other: &Self, target: &Target) -> bool {
        let left = self.permits(target);
        let right = other.permits(target);
        (left && right)
            || (left && matches!(other, Self::Internet))
            || (right && matches!(self, Self::Internet))
    }

    /// Check the policy against a destination. Internet never authorizes literal IPs.
    fn permits(&self, target: &Target) -> bool {
        match self {
            Self::None => false,
            Self::Internet => target.host.parse::<IpAddr>().is_err(),
            Self::Allow { allow } => {
                allow.len() <= 4096 && allow.iter().any(|rule| rule_matches(rule, target))
            }
        }
    }
}

/// Canonical bytes used for the signed policy digest and immutable storage.
pub fn canonical(policy: &Policy) -> Result<Vec<u8>> {
    policy.canonical_json().map_err(|error| {
        let data = if error.code == "SANDBOX_EGRESS_POLICY_TOO_LARGE" {
            crate::error::ErrorData::PolicyTooLarge
        } else {
            crate::error::ErrorData::Invalid {
                reason: "invalid policy".into(),
            }
        };
        error.context(data)
    })
}

/// Digest includes its algorithm name to prevent format ambiguity.
pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn rule_matches(rule: &str, target: &Target) -> bool {
    let (host, port) = match rule.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(port) => (host, port),
            Err(_) => return false,
        },
        None => (rule, 443),
    };
    if port != target.port {
        return false;
    }
    if let Ok(ip) = target.host.parse::<Ipv4Addr>() {
        return host.parse::<Ipv4Net>().is_ok_and(|net| net.contains(&ip))
            || host.parse::<Ipv4Addr>() == Ok(ip);
    }
    if let Some(suffix) = host.strip_prefix("*.") {
        return normalize_host(suffix)
            .is_ok_and(|suffix| target.host.ends_with(&format!(".{suffix}")));
    }
    normalize_host(host).is_ok_and(|host| host == target.host)
}

/// Check every answer before connecting to any answer, including mixed public/private replies.
pub fn check_addresses(addresses: &[IpAddr], extra: &[Ipv4Net]) -> Result<()> {
    if addresses.is_empty() {
        return Err(denied("DNS returned no addresses"));
    }
    for ip in addresses {
        let IpAddr::V4(ip) = ip else {
            return Err(denied("IPv6 disabled"));
        };
        let [a, b, c, d] = ip.octets();
        if a == 0
            || a == 10
            || a == 127
            || a >= 224
            || (a == 100 && (64..=127).contains(&b))
            || (a == 169 && b == 254)
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && (b == 168 || b == 0))
            || (a == 198 && (b == 18 || b == 19))
            || [a, b, c, d] == [168, 63, 129, 16]
            || extra.iter().any(|net| net.contains(ip))
        {
            return Err(denied("address is not public"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_literals_have_distinct_authority() {
        let policy = Policy::Allow {
            allow: vec!["*.example.com".into(), "8.8.8.0/24:53".into()],
        };
        for name in ["a.example.com", "deep.a.example.com"] {
            assert!(policy.permits(&Target::parse(name, Some(443)).unwrap()));
        }
        for name in [
            "example.com",
            "badexample.com",
            "a.example.com:80",
            "8.8.8.8:443",
        ] {
            assert!(!policy.permits(&Target::parse(name, Some(443)).unwrap()));
        }
        assert!(policy.permits(&Target::parse("8.8.8.8:53", None).unwrap()));
        assert!(!Policy::Internet.permits(&Target::parse("8.8.8.8:443", None).unwrap()));
        let literal = Target::parse("8.8.8.8:53", None).unwrap();
        let internet = Policy::Internet;
        assert!(internet.intersection_permits(&policy, &literal));
        assert!(policy.intersection_permits(&internet, &literal));
        assert!(!internet.intersection_permits(&internet, &literal));
    }

    #[test]
    fn ambiguous_authorities_are_refused() {
        for input in [
            "127.1:443",
            "2130706433:443",
            "0x7f000001:443",
            "example.com@evil.com:443",
            "example.com/path:443",
            "[::1]:443",
            "example.com:0",
            "example..com:443",
        ] {
            assert!(Target::parse(input, None).is_err(), "{input}");
        }
        assert_eq!(
            Target::parse("EXAMPLE.com.:443", None).unwrap().host,
            "example.com"
        );
    }

    #[test]
    fn one_private_answer_rejects_the_whole_resolution() {
        let public = "8.8.8.8".parse().unwrap();
        check_addresses(&[public], &[]).unwrap();
        for ip in [
            "10.0.0.1",
            "169.254.169.254",
            "127.0.0.1",
            "100.64.0.1",
            "168.63.129.16",
            "::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(
                check_addresses(&[public, ip.parse().unwrap()], &[]).is_err(),
                "{ip}"
            );
        }
        assert!(check_addresses(&[public], &["8.8.8.0/24".parse().unwrap()]).is_err());
    }
}
