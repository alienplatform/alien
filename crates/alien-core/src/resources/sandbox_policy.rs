//! Runtime sandbox destination policies and public-address validation.

use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;

use alien_error::{AlienError, Context, IntoAlienError};
use ipnet::Ipv4Net;
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "openapi")]
use utoipa::openapi::schema::{ArrayBuilder, ObjectBuilder, OneOfBuilder, Type};

use crate::error::{ErrorData, Result};

/// Per-sandbox outbound policy enforced by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EgressPolicy {
    /// Public internet access. Private and link-local destinations remain forbidden.
    Internet,
    /// Explicit destinations; unspecified ports match any port and wildcards include the apex.
    #[serde(untagged)]
    Allow {
        /// Hostnames, wildcard hostnames, IPv4 literals or IPv4 CIDRs, optionally with a port.
        allow: Vec<String>,
    },
}

// Utoipa does not support serde's per-variant `untagged`, so describe the wire union explicitly.
#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for EgressPolicy {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        OneOfBuilder::new()
            .item(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .enum_values(Some(["internet"])),
            )
            .item(
                ObjectBuilder::new()
                    .property(
                        "allow",
                        ArrayBuilder::new()
                            .items(ObjectBuilder::new().schema_type(Type::String))
                            .min_items(Some(0))
                            .max_items(Some(4096)),
                    )
                    .required("allow")
                    .additional_properties(Some(
                        utoipa::openapi::schema::AdditionalProperties::FreeForm(false),
                    )),
            )
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for EgressPolicy {}

impl<'de> Deserialize<'de> for EgressPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        enum Mode {
            Internet,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Allow {
            allow: Vec<String>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Mode(Mode),
            Allow(Allow),
        }
        Ok(match Input::deserialize(deserializer)? {
            Input::Mode(Mode::Internet) => Self::Internet,
            Input::Allow(Allow { allow }) => Self::Allow { allow },
        })
    }
}

impl EgressPolicy {
    /// Validates every target before a deployment or runtime request can take effect.
    pub fn validate(&self) -> Result<()> {
        self.targets().map(|_| ())
    }

    /// Returns a validated policy with ASCII, lowercase names and canonical IP networks.
    pub fn normalized(&self) -> Result<Self> {
        match self {
            Self::Allow { .. } => Ok(Self::Allow {
                allow: self.targets()?.iter().map(Target::canonical).collect(),
            }),
            _ => Ok(self.clone()),
        }
    }

    /// Canonical JSON used to authenticate gateway policy writes: normalized, sorted, no whitespace.
    /// Duplicate entries remain present; sorting makes input order irrelevant to the digest.
    pub fn canonical_json(&self) -> Result<Vec<u8>> {
        let mut policy = self.normalized()?;
        if let Self::Allow { allow } = &mut policy {
            allow.sort_unstable();
        }
        serde_json::to_vec(&policy)
            .into_alien_error()
            .context(ErrorData::SandboxEgressInvalid {
                target: "policy".to_string(),
                reason: "could not encode canonical policy".to_string(),
            })
    }

    /// SHA-256 digest authenticating the policy body in an admin JWS.
    pub fn policy_digest(&self) -> Result<String> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(self.canonical_json()?)
        ))
    }

    /// Whether a concrete public destination and port match this policy.
    pub fn allows(&self, host: &str, port: u16) -> Result<bool> {
        let target = Target::parse(&format!("{host}:{port}"))?;
        // Sandboxes share one outbound IP: unsolicited mail can get that IP blocklisted
        // for every tenant. Explicit destination:port grants retain legitimate mail use.
        let mail = matches!(port, 25 | 465 | 587);
        Ok((!mail && matches!(self, Self::Internet))
            || self
                .targets()?
                .iter()
                .any(|entry| (!mail || entry.port == Some(port)) && entry.contains(&target)))
    }

    fn targets(&self) -> Result<Vec<Target>> {
        let Self::Allow { allow } = self else {
            return Ok(Vec::new());
        };
        if allow.len() > 4096 {
            return Err(AlienError::new(ErrorData::SandboxEgressPolicyTooLarge {
                limit: "4096 targets".to_string(),
            }));
        }
        let targets = allow
            .iter()
            .map(|target| Target::parse(target))
            .collect::<Result<Vec<_>>>()?;
        // Valid targets are ASCII without JSON escape characters. Include object/array syntax,
        // two quotes per target and commas to measure the exact canonical JSON byte length.
        let canonical_bytes = 12 - usize::from(!targets.is_empty())
            + targets
                .iter()
                .map(|target| target.canonical().len() + 3)
                .sum::<usize>();
        if canonical_bytes > 256 * 1024 {
            return Err(AlienError::new(ErrorData::SandboxEgressPolicyTooLarge {
                limit: "256 KiB of canonical JSON".to_string(),
            }));
        }
        Ok(targets)
    }
}

#[derive(Debug)]
struct Target {
    host: Host,
    port: Option<u16>,
}

#[derive(Debug)]
enum Host {
    Name { name: String, wildcard: bool },
    Network(Ipv4Net),
}

fn invalid(target: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressInvalid {
        target: target.to_string(),
        reason: reason.to_string(),
    })
}

impl Target {
    fn parse(target: &str) -> Result<Self> {
        let (host, port) = match target.rsplit_once(':') {
            Some((host, port)) => {
                if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(invalid(
                        target,
                        "port must be an integer between 1 and 65535",
                    ));
                }
                let port: u16 =
                    port.parse()
                        .into_alien_error()
                        .context(ErrorData::SandboxEgressInvalid {
                            target: target.to_string(),
                            reason: "port must be between 1 and 65535".to_string(),
                        })?;
                if port == 0 {
                    return Err(invalid(target, "port must be between 1 and 65535"));
                }
                (host, Some(port))
            }
            None => (target, None),
        };
        let host = host.strip_suffix('.').unwrap_or(host);
        let network = if host.contains('/') {
            Some(
                host.parse::<Ipv4Net>()
                    .into_alien_error()
                    .context(ErrorData::SandboxEgressInvalid {
                        target: target.to_string(),
                        reason: "expected an IPv4 CIDR".to_string(),
                    })?
                    .trunc(),
            )
        } else {
            host.parse::<Ipv4Addr>().ok().map(Ipv4Net::from)
        };
        if let Some(network) = network {
            // Test intersection, not just the first address: a broad CIDR can include private IPs.
            if DENIED_RANGES.iter().any(|&(address, prefix)| {
                let denied = Ipv4Net::new_assert(Ipv4Addr::from(address), prefix);
                denied.contains(&network) || network.contains(&denied)
            }) {
                return Err(invalid(
                    target,
                    "private, link-local and special-use IPv4 ranges are forbidden",
                ));
            }
            return Ok(Self {
                host: Host::Network(network),
                port,
            });
        }
        let (name, wildcard) = match host.strip_prefix("*.") {
            Some(name) => (name, true),
            None => (host, false),
        };
        let name = idna::domain_to_ascii_strict(name)
            .into_alien_error()
            .context(ErrorData::SandboxEgressInvalid {
                target: target.to_string(),
                reason: "expected a DNS hostname".to_string(),
            })?
            .to_ascii_lowercase();
        if name.len() > 253
            || !name.contains('.')
            || name.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            || name
                .rsplit('.')
                .next()
                .is_some_and(|label| label.bytes().all(|c| c.is_ascii_digit()))
        {
            return Err(invalid(
                target,
                "expected a hostname, *.hostname, IPv4 literal or IPv4 CIDR",
            ));
        }
        if psl::domain_str(&name).is_none() {
            return Err(invalid(
                target,
                "public suffixes cannot be allowed as destinations",
            ));
        }
        Ok(Self {
            host: Host::Name { name, wildcard },
            port,
        })
    }

    fn canonical(&self) -> String {
        let host = match &self.host {
            Host::Name { name, wildcard } => format!("{}{name}", if *wildcard { "*." } else { "" }),
            Host::Network(network) if network.prefix_len() == 32 => network.addr().to_string(),
            Host::Network(network) => network.to_string(),
        };
        match self.port {
            Some(port) => format!("{host}:{port}"),
            None => host,
        }
    }

    fn contains(&self, target: &Self) -> bool {
        if self.port.is_some() && self.port != target.port {
            return false;
        }
        match (&self.host, &target.host) {
            (Host::Network(parent), Host::Network(child)) => parent.contains(child),
            (
                Host::Name {
                    name: parent,
                    wildcard: parent_wildcard,
                },
                Host::Name {
                    name: child,
                    wildcard: child_wildcard,
                },
            ) => {
                if *parent_wildcard {
                    child == parent
                        || child
                            .strip_suffix(parent)
                            .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1)
                } else {
                    !child_wildcard && child == parent
                }
            }
            _ => false,
        }
    }
}

/// Whether an IPv4 address is public unicast, excluding metadata and special-use ranges.
pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !DENIED_RANGES.iter().any(|&(address, prefix)| {
        Ipv4Net::new_assert(Ipv4Addr::from(address), prefix).contains(&ip)
    })
}

// Includes shared address space, metadata endpoints and non-unicast/reserved ranges.
const DENIED_RANGES: &[(u32, u8)] = &[
    (0x00000000, 8),
    (0x0a000000, 8),
    (0x64400000, 10),
    (0x7f000000, 8),
    (0xa9fe0000, 16),
    (0xac100000, 12),
    (0xc0a80000, 16),
    (0xc0000000, 24),
    (0xc0000200, 24),
    (0xc6120000, 15),
    (0xc6336400, 24),
    (0xcb007100, 24),
    (0xe0000000, 3),
    (0xa83f8110, 32),
];

#[cfg(test)]
mod tests;
