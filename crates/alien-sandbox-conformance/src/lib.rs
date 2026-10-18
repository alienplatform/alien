//! Network observations for live sandbox egress checks.
//!
//! The guest needs Python 3 with its standard library. Run the same probe in a
//! control sandbox with access to the target before interpreting a denial.

use serde::{Deserialize, Serialize};

/// Standard-library-only program run through the sandbox's command API.
pub const GUEST_PROBE: &str = include_str!("probe.py");

/// A single network operation executed inside the guest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Probe {
    /// Direct TCP, bypassing all proxy environment variables.
    Tcp {
        /// Address or DNS name of a controlled listener.
        host: String,
        /// Listener port.
        port: u16,
    },
    /// HTTP with optional CONNECT and an independently selected TLS name or Host.
    #[serde(rename_all = "camelCase")]
    Http {
        /// Origin URL. Redirects are not followed.
        url: String,
        /// Use HTTP(S)_PROXY if present. NO_PROXY is deliberately ignored.
        use_proxy: bool,
        /// Override the ClientHello name to exercise domain fronting.
        server_name: Option<String>,
        /// Override the HTTP Host to exercise authority disagreement.
        host_header: Option<String>,
    },
    /// Query the guest's configured resolver; authoritative logs prove exfiltration.
    Dns {
        /// Unique label under a controlled, query-logged zone.
        host: String,
    },
}

/// What actually happened, without response bodies or proxy credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase", deny_unknown_fields)]
pub enum Observation {
    /// TCP connection or DNS resolution completed.
    Connected,
    /// An HTTP response arrived. Its digest permits byte-exact assertions.
    #[serde(rename_all = "camelCase")]
    Response {
        /// HTTP response status.
        status: u16,
        /// SHA-256 of the complete response body.
        body_sha256: String,
        /// Number of response body bytes.
        bytes: usize,
    },
    /// The explicit proxy rejected CONNECT before forwarding.
    ProxyDenied {
        /// Proxy response status.
        status: u16,
    },
    /// A socket deadline elapsed.
    Timeout,
    /// The peer refused or reset a connection.
    Refused,
    /// DNS did not resolve. This alone is not evidence of policy enforcement.
    DnsFailed,
    /// TLS failed. This alone is not evidence of policy enforcement.
    TlsFailed,
    /// A route was unavailable.
    Unreachable,
    /// Invalid HTTP or a response exceeding the probe's bound.
    ProtocolFailed,
}

/// The claim being checked against the subject and positive control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Expectation {
    /// The subject must return the known fixture body, like the successful control.
    #[serde(rename_all = "camelCase")]
    Response {
        /// Expected SHA-256 supplied by the endpoint fixture, not learned from a response.
        body_sha256: String,
    },
    /// The network must refuse traffic to a listener proven reachable by the control.
    NetworkDenied,
    /// CONNECT must be explicitly rejected by the gateway.
    ProxyDenied,
}

/// Conformance verdict; missing or broken controls never pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// The observations support the claim.
    Pass,
    /// The subject demonstrably violates the claim.
    Fail,
    /// The observations do not prove or disprove the claim.
    Unproven,
}

/// Evaluate observations only after proving the endpoint works independently.
///
/// A random HTTP 200 page is not a positive control: response checks require the
/// fixture's known digest before comparing the subject and control.
pub fn evaluate(expected: &Expectation, control: &Observation, subject: &Observation) -> Verdict {
    if !matches!(
        control,
        Observation::Connected
            | Observation::Response {
                status: 200..=299,
                ..
            }
    ) {
        return Verdict::Unproven;
    }
    match expected {
        Expectation::Response { body_sha256 } => {
            if !matches!(control, Observation::Response { body_sha256: actual, .. } if actual == body_sha256)
            {
                Verdict::Unproven
            } else if control == subject {
                Verdict::Pass
            } else {
                Verdict::Fail
            }
        }
        Expectation::NetworkDenied => match subject {
            Observation::Timeout | Observation::Refused | Observation::Unreachable => Verdict::Pass,
            Observation::Connected | Observation::Response { .. } => Verdict::Fail,
            _ => Verdict::Unproven,
        },
        Expectation::ProxyDenied => match subject {
            Observation::ProxyDenied { status: 403 } => Verdict::Pass,
            Observation::Connected | Observation::Response { .. } => Verdict::Fail,
            _ => Verdict::Unproven,
        },
    }
}
