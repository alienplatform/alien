//! Sandbox egress declarations and the shared runtime narrowing rule.

use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;

use alien_error::{AlienError, Context, IntoAlienError};
use ipnet::Ipv4Net;
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "openapi")]
use utoipa::openapi::schema::{ArrayBuilder, ObjectBuilder, OneOfBuilder, Type};

use crate::error::{ErrorData, Result};

/// Outbound network policy, bounded by the deployment's declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxEgress {
    /// No outbound network access.
    None,
    /// Public internet access. Private and link-local destinations remain forbidden.
    Internet,
    /// Explicit destinations; ports default to 443 and wildcards exclude the apex.
    #[serde(untagged)]
    Allow {
        /// Hostnames, wildcard hostnames, IPv4 literals or IPv4 CIDRs, optionally with a port.
        allow: Vec<String>,
    },
}

// Utoipa does not support serde's per-variant `untagged`, so describe the wire union explicitly.
#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for SandboxEgress {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        OneOfBuilder::new()
            .item(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .enum_values(Some(["none", "internet"])),
            )
            .item(
                ObjectBuilder::new()
                    .property(
                        "allow",
                        ArrayBuilder::new()
                            .items(ObjectBuilder::new().schema_type(Type::String))
                            .min_items(Some(1))
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
impl utoipa::ToSchema for SandboxEgress {}

impl<'de> Deserialize<'de> for SandboxEgress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", tag = "mode", deny_unknown_fields)]
        enum Legacy {
            Deny {},
            Allow {},
            AllowDomains { domains: Vec<String> },
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        enum Mode {
            None,
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
            Legacy(Legacy),
        }
        Ok(match Input::deserialize(deserializer)? {
            Input::Mode(Mode::None) | Input::Legacy(Legacy::Deny {}) => Self::None,
            Input::Mode(Mode::Internet) | Input::Legacy(Legacy::Allow {}) => Self::Internet,
            Input::Allow(Allow { allow }) => Self::Allow { allow },
            Input::Legacy(Legacy::AllowDomains { domains }) => Self::Allow { allow: domains },
        })
    }
}

impl SandboxEgress {
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

    /// Azure's native proxy supports HTTP(S) matches and at most 500 rules, including
    /// the deny rules needed to keep wildcard apices outside the allowed set.
    pub fn validate_for_azure(&self) -> Result<()> {
        let targets = self.targets()?;
        let too_large = || {
            AlienError::new(ErrorData::SandboxEgressPolicyTooLarge {
                limit: "Azure's 500 native rules, including wildcard apex guards".to_string(),
            })
        };
        if targets.len() > 500 {
            return Err(too_large());
        }
        let mut rules = targets.len();
        for target in &targets {
            if !matches!(target.port, 80 | 443) {
                return Err(invalid(
                    &target.canonical(),
                    "Azure native egress supports HTTP port 80 and HTTPS port 443 only",
                ));
            }
            if matches!(&target.host, Host::Network(network) if network.prefix_len() != 32) {
                return Err(invalid(
                    &target.canonical(),
                    "Azure native egress cannot enforce IPv4 CIDR targets",
                ));
            }
            if let Host::Name {
                name,
                wildcard: true,
            } = &target.host
            {
                let apex = Target {
                    host: Host::Name {
                        name: name.clone(),
                        wildcard: false,
                    },
                    port: target.port,
                };
                if !targets.iter().any(|parent| parent.contains(&apex)) {
                    rules += 1;
                }
            }
        }
        if rules > 500 {
            return Err(too_large());
        }
        Ok(())
    }

    /// Canonical JSON used by the gateway policy reference: normalized, sorted, no whitespace.
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

    /// SHA-256 reference carried in proxy and admin JWS claims.
    pub fn policy_digest(&self) -> Result<String> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(self.canonical_json()?)
        ))
    }

    /// Checks the declaration bound, naming the first target that would widen it.
    /// Invalid targets are refused even when the bound permits internet access.
    pub fn ensure_subset_of(&self, bound: &Self) -> Result<()> {
        let requested = self.targets()?;
        let declared = bound.targets()?;
        let offending = match (self, bound) {
            (Self::None, _) | (_, Self::Internet) => None,
            (Self::Internet, _) => Some("internet".to_string()),
            (Self::Allow { allow }, Self::None) => allow.first().cloned(),
            (Self::Allow { allow }, Self::Allow { .. }) => requested
                .iter()
                .zip(allow)
                .find(|(target, _)| !declared.iter().any(|parent| parent.contains(target)))
                .map(|(_, original)| original.clone()),
        };
        match offending {
            Some(target) => Err(AlienError::new(ErrorData::SandboxEgressWidens { target })),
            None => Ok(()),
        }
    }

    // Setup authorizations predate the JSON rename. Keep only their hash projection stable;
    // declarations and stored state use the current format but retain the original spelling.
    // Normalizing at storage time would invalidate approvals after saving and reloading a stack.
    pub(crate) fn setup_digest_value(&self) -> serde_json::Value {
        match self {
            Self::None => serde_json::json!({ "mode": "deny" }),
            Self::Internet => serde_json::json!({ "mode": "allow" }),
            Self::Allow { allow } => {
                serde_json::json!({ "mode": "allowDomains", "domains": allow })
            }
        }
    }

    /// Boolean backend switch, or `None` when the backend needs a destination matcher.
    pub fn internet_access_switch(&self) -> Option<bool> {
        match self {
            Self::None => Some(false),
            Self::Internet => Some(true),
            Self::Allow { .. } => None,
        }
    }

    fn targets(&self) -> Result<Vec<Target>> {
        let Self::Allow { allow } = self else {
            return Ok(Vec::new());
        };
        if allow.is_empty() {
            return Err(invalid("allow", "must contain at least one target"));
        }
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
        let canonical_bytes = 11
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
    port: u16,
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
                (host, port)
            }
            None => (target, 443),
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
        if self.port == 443 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    fn contains(&self, target: &Self) -> bool {
        if self.port != target.port {
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
                    (*child_wildcard && child == parent)
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
