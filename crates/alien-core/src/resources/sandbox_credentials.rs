//! Per-sandbox credential rules: the app that owns a sandbox sets them at runtime, and the
//! egress gateway puts the value on matching requests on the way out. The sandbox holds only
//! a placeholder.

use serde::Deserialize;
use std::fmt;

/// Most rules one sandbox may hold.
pub const MAX_SANDBOX_CREDENTIAL_RULES: usize = 32;
/// Largest value of one rule, `value_prefix` included.
pub const MAX_SANDBOX_CREDENTIAL_VALUE_BYTES: usize = 8 * 1024;

/// Headers a rule cannot set: they route or frame the request, or authenticate to the gateway.
const RESERVED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "proxy-authorization",
    "upgrade",
    "trailer",
    "te",
];

/// A credential value. It prints as `<redacted>` and is never serialized by this type.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct CredentialValue(String);

impl CredentialValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// The value itself, for the signed record sent to the gateway.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CredentialValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for CredentialValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// One credential rule of a sandbox.
///
/// A rule targets exactly one internet `host` (HTTPS, through the gateway's TLS interception
/// and the sandbox's CA) or one `link` the stack declares for the sandbox (plain HTTP inside
/// the deployment). The egress policy still decides what the sandbox reaches: a rule for a
/// destination the policy refuses has no effect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxCredentialRule {
    /// Exact DNS name, lowercase, no wildcard and no IP address.
    #[serde(default)]
    pub host: Option<String>,
    /// Id of a link the stack declares for this sandbox (`link:<id>` in a policy).
    #[serde(default)]
    pub link: Option<String>,
    /// Port. Default: 443 for a host, every declared port for a link.
    #[serde(default)]
    pub port: Option<u16>,
    /// Uppercase HTTP method. Default: any.
    #[serde(default)]
    pub method: Option<String>,
    /// Exact path, or a subtree when it ends in `/`. Default: `/` (every path).
    #[serde(default)]
    pub path: Option<String>,
    /// Header the gateway sets, replacing whatever the sandbox sent.
    pub header: String,
    /// Prepended to the value, for example `Bearer `.
    #[serde(default)]
    pub value_prefix: Option<String>,
    /// The real value. Never logged, never shown to the sandbox.
    pub value: CredentialValue,
}

impl SandboxCredentialRule {
    /// The rule as JSON, value included, for the one place it travels: the request to the
    /// gateway (or the local sandbox route). Field names are the wire names.
    pub fn to_wire(&self) -> serde_json::Value {
        let mut wire = serde_json::json!({ "header": self.header, "value": self.value.0 });
        for (name, field) in [
            ("host", self.host.clone().map(serde_json::Value::from)),
            ("link", self.link.clone().map(serde_json::Value::from)),
            ("port", self.port.map(serde_json::Value::from)),
            ("method", self.method.clone().map(serde_json::Value::from)),
            ("path", self.path.clone().map(serde_json::Value::from)),
            (
                "valuePrefix",
                self.value_prefix.clone().map(serde_json::Value::from),
            ),
        ] {
            if let Some(field) = field {
                wire[name] = field;
            }
        }
        wire
    }

    /// Checks the fields the gateway checks again. Messages never contain the value.
    pub fn validate(&self) -> Result<(), String> {
        match (&self.host, &self.link) {
            (Some(host), None) => {
                let valid = !host.is_empty()
                    && host.len() <= 253
                    && host.parse::<std::net::IpAddr>().is_err()
                    && host.split('.').all(|label| {
                        !label.is_empty()
                            && label.len() <= 63
                            && label
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    });
                if !valid {
                    return Err(format!(
                        "credential host '{host}' must be an exact lowercase DNS name"
                    ));
                }
            }
            (None, Some(link)) if !link.is_empty() => {}
            _ => return Err("a credential rule names exactly one host or link".into()),
        }
        if self.port == Some(0) {
            return Err("credential port must be between 1 and 65535".into());
        }
        if let Some(method) = &self.method {
            if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(format!(
                    "credential method '{method}' must be an uppercase HTTP method"
                ));
            }
        }
        if let Some(path) = &self.path {
            if !path.starts_with('/')
                || path.contains(['?', '#', '%', '\\'])
                || path.split('/').any(|part| part == "." || part == "..")
                || path.len() > 2048
            {
                return Err(format!(
                    "credential path '{path}' must be an absolute path without query, escapes or dot segments"
                ));
            }
        }
        let header = self.header.to_ascii_lowercase();
        if header.is_empty()
            || !header
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return Err(format!(
                "credential header '{}' is not a valid header name",
                self.header
            ));
        }
        if RESERVED_HEADERS.contains(&header.as_str()) {
            return Err(format!(
                "credential header '{}' routes or frames the request and cannot be set",
                self.header
            ));
        }
        let prefix = self.value_prefix.as_deref().unwrap_or("");
        let value = self.value.expose();
        if value.is_empty() || prefix.len() + value.len() > MAX_SANDBOX_CREDENTIAL_VALUE_BYTES {
            return Err(format!(
                "credential value must be 1 to {MAX_SANDBOX_CREDENTIAL_VALUE_BYTES} bytes"
            ));
        }
        if prefix
            .bytes()
            .chain(value.bytes())
            .any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
        {
            return Err("credential value contains a control character".into());
        }
        Ok(())
    }
}

/// Checks a whole rule set: its size and every rule.
pub fn validate_sandbox_credentials(rules: &[SandboxCredentialRule]) -> Result<(), String> {
    if rules.len() > MAX_SANDBOX_CREDENTIAL_RULES {
        return Err(format!(
            "a sandbox holds at most {MAX_SANDBOX_CREDENTIAL_RULES} credential rules"
        ));
    }
    for (index, rule) in rules.iter().enumerate() {
        rule.validate()
            .map_err(|reason| format!("credential rule {index}: {reason}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUE: &str = "fixture-value-31c7";

    fn rule(json: serde_json::Value) -> SandboxCredentialRule {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn debug_and_errors_never_contain_the_value() {
        let valid = rule(
            serde_json::json!({"host":"api.example.com","header":"Authorization","valuePrefix":"Bearer ","value":VALUE}),
        );
        assert!(valid.validate().is_ok());
        assert!(!format!("{valid:?}").contains(VALUE));
        for invalid in [
            serde_json::json!({"host":"*.example.com","header":"Authorization","value":VALUE}),
            serde_json::json!({"host":"1.2.3.4","header":"Authorization","value":VALUE}),
            serde_json::json!({"link":"connector","host":"a.example.com","header":"Authorization","value":VALUE}),
            serde_json::json!({"link":"connector","header":"Host","value":VALUE}),
            serde_json::json!({"link":"connector","header":"Authorization","path":"/a/../b","value":VALUE}),
            serde_json::json!({"link":"connector","header":"Authorization","method":"get","value":VALUE}),
            serde_json::json!({"link":"connector","header":"Authorization","value":format!("{VALUE}\r\nX: y")}),
        ] {
            let error = rule(invalid).validate().unwrap_err();
            assert!(!error.contains(VALUE), "{error}");
        }
    }
}
