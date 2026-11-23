//! Per-sandbox credential rules, set at runtime by the app that owns the sandbox.
//!
//! A signed record carries the whole rule set of one sandbox, values included. The gateway
//! holds it only in memory, replicates it to its peers over the authenticated admin channel,
//! and replaces it as a whole: the newest version wins, and an empty list removes every rule.
//! Values never reach a log line, an error message, the audit or a metric: every type that
//! holds one prints `<redacted>`, and every parse failure returns fixed text.
use crate::{
    error::{denied, invalid, unavailable, ErrorData, Result},
    policy::normalize_host,
    record::{Summary, Version},
};
use alien_error::AlienError;
use ed25519_compact::PublicKey;
use http::{HeaderName, HeaderValue, Method};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

/// Record type in the JWS header; distinct from policy records and admin grants.
pub const RECORD_TYPE: &str = "alien-egress-credentials";
/// Most rules one sandbox may hold.
pub const MAX_RULES: usize = 32;
/// Largest value of one rule, prefix included.
pub const MAX_VALUE_BYTES: usize = 8 * 1024;
/// Largest signed record. Base64 of 32 full values plus their scopes fits.
pub const MAX_RECORD_BYTES: usize = 400 * 1024;
const SNAPSHOT_PAGE_BYTES: usize = 8 * 1024 * 1024;

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

/// One rule as the binding signs it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuleClaims {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    path: Option<String>,
    header: String,
    #[serde(default)]
    value_prefix: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    v: u64,
    aud: String,
    sub: String,
    ver: Version,
    rules: Vec<RuleClaims>,
}

/// Where a rule applies: an internet host over HTTPS, or a declared link over plain HTTP.
#[derive(Clone, PartialEq, Eq)]
pub enum RuleTarget {
    Host(String),
    Link(String),
}

/// A validated rule. The header value is marked sensitive and never printed.
#[derive(Clone)]
pub struct Rule {
    pub target: RuleTarget,
    /// `None` for a link means every declared port of that link.
    pub port: Option<u16>,
    pub method: Option<Method>,
    /// Exact path, or a subtree when it ends in `/`.
    pub path: String,
    pub header: HeaderName,
    pub value: HeaderValue,
}
impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rule")
            .field("header", &self.header)
            .field("value", &"<redacted>")
            .finish_non_exhaustive()
    }
}
impl Rule {
    pub fn applies_to(&self, target: &RuleTarget, port: u16) -> bool {
        self.target == *target && self.port.is_none_or(|own| own == port)
    }
    /// Whether this rule scopes a request with this method and (already unambiguous) path.
    pub fn matches_request(&self, method: &Method, path: &str) -> bool {
        self.method.as_ref().is_none_or(|own| own == method)
            && (path == self.path || (self.path.ends_with('/') && path.starts_with(&self.path)))
    }
}

/// A verified credential record: the signed form for peers, and its parsed rules.
#[derive(Clone)]
pub struct CredentialRecord {
    pub signed: String,
    pub aud: String,
    pub sub: String,
    pub ver: Version,
    pub rules: Arc<Vec<Rule>>,
}
impl fmt::Debug for CredentialRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialRecord")
            .field("aud", &self.aud)
            .field("sub", &self.sub)
            .field("ver", &self.ver)
            .field("rules", &self.rules.len())
            .finish_non_exhaustive()
    }
}

impl CredentialRecord {
    /// Verify the signature, the identity and every rule. `links` are the declared link ids of
    /// the record's audience; a rule naming another link is refused.
    pub fn verify(
        signed: String,
        keys: &HashMap<String, PublicKey>,
        audiences: &[String],
        links: impl Fn(&str) -> Vec<String>,
        now_ms: u64,
    ) -> Result<Self> {
        let claims: Claims =
            crate::token::verify_signed_limit(&signed, keys, Some(RECORD_TYPE), MAX_RECORD_BYTES)?;
        if claims.v != 1 {
            return Err(AlienError::new(ErrorData::RecordFormatUnsupported));
        }
        if claims.aud.is_empty()
            || claims.sub.is_empty()
            || claims.aud.len() > 256
            || claims.sub.len() > 256
            || !audiences.contains(&claims.aud)
            || claims.ver.1.len() != 32
            || !claims.ver.1.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(denied("invalid credential record identity"));
        }
        if claims.ver.0 > now_ms.saturating_add(60_000) {
            return Err(AlienError::new(ErrorData::VersionInFuture));
        }
        if claims.rules.len() > MAX_RULES {
            return Err(invalid("too many credential rules"));
        }
        let declared = links(&claims.aud);
        let rules = claims
            .rules
            .into_iter()
            .map(|rule| parse_rule(rule, &declared))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            signed,
            aud: claims.aud,
            sub: claims.sub,
            ver: claims.ver,
            rules: Arc::new(rules),
        })
    }
}

fn parse_rule(rule: RuleClaims, links: &[String]) -> Result<Rule> {
    let target = match (rule.host, rule.link) {
        (Some(host), None) => {
            // Exact DNS name: no wildcard, no IP literal, already in normal form.
            if normalize_host(&host).ok().as_deref() != Some(host.as_str())
                || host.contains('*')
                || host.parse::<std::net::IpAddr>().is_ok()
            {
                return Err(invalid("credential host must be an exact DNS name"));
            }
            RuleTarget::Host(host)
        }
        (None, Some(link)) => {
            if !links.contains(&link) {
                return Err(invalid("credential link is not declared for this sandbox"));
            }
            RuleTarget::Link(link)
        }
        _ => return Err(invalid("a credential rule names exactly one host or link")),
    };
    let port = match (&target, rule.port) {
        (_, Some(0)) => return Err(invalid("invalid credential port")),
        (RuleTarget::Host(_), None) => Some(443),
        (_, port) => port,
    };
    let method = match rule.method {
        None => None,
        Some(method) => {
            if method.is_empty() || !method.bytes().all(|byte| byte.is_ascii_uppercase()) {
                return Err(invalid(
                    "credential method must be an uppercase HTTP method",
                ));
            }
            Some(
                method
                    .parse::<Method>()
                    .map_err(|_| invalid("credential method must be an uppercase HTTP method"))?,
            )
        }
    };
    let path = rule.path.unwrap_or_else(|| "/".to_string());
    if !path.starts_with('/')
        || path.contains(['?', '#', '%', '\\'])
        || path.split('/').any(|part| part == "." || part == "..")
        || path.len() > 2048
    {
        return Err(invalid("invalid credential path"));
    }
    let header = HeaderName::from_bytes(rule.header.as_bytes())
        .map_err(|_| invalid("invalid credential header name"))?;
    if RESERVED_HEADERS.contains(&header.as_str()) {
        return Err(invalid(
            "a credential cannot replace routing or framing headers",
        ));
    }
    if rule.value.is_empty() || rule.value_prefix.len() + rule.value.len() > MAX_VALUE_BYTES {
        return Err(invalid("credential value is empty or too large"));
    }
    let mut value = HeaderValue::from_str(&format!("{}{}", rule.value_prefix, rule.value))
        .map_err(|_| invalid("credential value is not a valid HTTP header value"))?;
    value.set_sensitive(true);
    Ok(Rule {
        target,
        port,
        method,
        path,
        header,
        value,
    })
}

struct Stored {
    record: Arc<CredentialRecord>,
    bytes: usize,
}

/// The current rule set of each sandbox, in memory only.
pub struct CredentialStore {
    entries: RwLock<BTreeMap<(String, String), Stored>>,
    max_bytes: usize,
    /// False while a joining replica may still miss writes acknowledged by the others: it
    /// holds sets but injects none, so a removed value is never put on a request.
    serving: AtomicBool,
}
impl CredentialStore {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            max_bytes,
            serving: AtomicBool::new(true),
        }
    }

    /// Start (or end) the join window: while it is closed `rules_for` returns nothing.
    pub fn set_serving(&self, serving: bool) {
        self.serving.store(serving, Ordering::Release);
    }
    pub fn serving(&self) -> bool {
        self.serving.load(Ordering::Acquire)
    }

    fn lock_error() -> AlienError<ErrorData> {
        AlienError::new(unavailable("credential state lock"))
    }

    /// Apply a verified record if it is newer than the held one. Equal and older versions are
    /// no-ops, so a retried or reordered write never brings back an older set.
    pub fn apply(&self, record: CredentialRecord) -> Result<bool> {
        // Signed bytes plus parsed values, doubled for allocator overhead as policies are.
        let bytes = (record.signed.len()
            + record
                .rules
                .iter()
                .map(|rule| rule.value.len() + rule.path.len() + 128)
                .sum::<usize>()
            + record.aud.len()
            + record.sub.len()
            + 512)
            .saturating_mul(2);
        let key = (record.aud.clone(), record.sub.clone());
        let mut entries = self.entries.write().map_err(|_| Self::lock_error())?;
        let previous = match entries.get(&key) {
            Some(current)
                if !crate::record::should_replace(Some(&current.record.ver), &record.ver) =>
            {
                return Ok(false)
            }
            Some(current) => current.bytes,
            None => 0,
        };
        let used: usize = entries.values().map(|entry| entry.bytes).sum();
        if used - previous + bytes > self.max_bytes {
            return Err(AlienError::new(ErrorData::StoreFull));
        }
        entries.insert(
            key,
            Stored {
                record: Arc::new(record),
                bytes,
            },
        );
        Ok(true)
    }

    /// Drop a sandbox's rules: its policy was deleted.
    pub fn remove(&self, aud: &str, sub: &str) -> Result<()> {
        self.entries
            .write()
            .map_err(|_| Self::lock_error())?
            .remove(&(aud.to_string(), sub.to_string()));
        Ok(())
    }

    /// Keep only the sandboxes `keep` accepts (they still have a live policy).
    pub fn retain(&self, keep: impl Fn(&str, &str) -> bool) -> Result<()> {
        self.entries
            .write()
            .map_err(|_| Self::lock_error())?
            .retain(|(aud, sub), _| keep(aud, sub));
        Ok(())
    }

    pub fn version(&self, aud: &str, sub: &str) -> Result<Option<Version>> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Self::lock_error())?
            .get(&(aud.to_string(), sub.to_string()))
            .map(|entry| entry.record.ver.clone()))
    }

    /// The rules of one sandbox that apply to `target:port`, read at the moment of the request.
    pub fn rules_for(&self, aud: &str, sub: &str, target: &RuleTarget, port: u16) -> Vec<Rule> {
        if !self.serving() {
            return Vec::new();
        }
        let Ok(entries) = self.entries.read() else {
            return Vec::new();
        };
        entries
            .get(&(aud.to_string(), sub.to_string()))
            .map(|entry| {
                entry
                    .record
                    .rules
                    .iter()
                    .filter(|rule| rule.applies_to(target, port))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Result<Vec<Summary>> {
        Ok(self
            .entries
            .read()
            .map_err(|_| Self::lock_error())?
            .values()
            .map(|entry| Summary {
                aud: entry.record.aud.clone(),
                sub: entry.record.sub.clone(),
                ver: entry.record.ver.clone(),
            })
            .collect())
    }

    /// Records a peer lacks or holds older, in bounded pages.
    pub fn newer_than(&self, versions: &[Summary], audiences: &[String]) -> Result<Vec<String>> {
        let mut known: BTreeMap<(&str, &str), &Version> = BTreeMap::new();
        for version in versions {
            let current = known
                .entry((version.aud.as_str(), version.sub.as_str()))
                .or_insert(&version.ver);
            if crate::record::compare_versions(current, &version.ver).is_lt() {
                *current = &version.ver;
            }
        }
        let mut page_bytes = 0;
        Ok(self
            .entries
            .read()
            .map_err(|_| Self::lock_error())?
            .values()
            .filter(|entry| audiences.contains(&entry.record.aud))
            .filter(|entry| {
                known
                    .get(&(entry.record.aud.as_str(), entry.record.sub.as_str()))
                    .is_none_or(|ver| {
                        crate::record::compare_versions(ver, &entry.record.ver).is_lt()
                    })
            })
            .take_while(|entry| {
                let size = entry.record.signed.len() + 4;
                if page_bytes + size > SNAPSHOT_PAGE_BYTES {
                    return false;
                }
                page_bytes += size;
                true
            })
            .map(|entry| entry.record.signed.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use ed25519_compact::{KeyPair, Seed};
    use serde_json::json;

    const VALUE: &str = "fixture-credential-value-7f3a";

    fn sign(pair: &KeyPair, claims: serde_json::Value) -> String {
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(
                json!({"alg":"EdDSA","kid":"test","typ":RECORD_TYPE})
                    .to_string()
                    .as_bytes()
            ),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(pair.sk.sign(input.as_bytes(), None).as_ref())
        )
    }

    fn verify(pair: &KeyPair, rules: serde_json::Value, ver: u64) -> Result<CredentialRecord> {
        let keys = HashMap::from([("test".to_string(), pair.pk)]);
        CredentialRecord::verify(
            sign(
                pair,
                json!({"v":1,"aud":"p/r","sub":"box","ver":[ver,"0".repeat(32)],"rules":rules}),
            ),
            &keys,
            &["p/r".to_string()],
            |_| vec!["connector".to_string()],
            ver,
        )
    }

    #[test]
    fn refused_rules_and_their_errors_never_contain_the_value() {
        let pair = KeyPair::from_seed(Seed::new([3; 32]));
        for rule in [
            json!({"host":"*.example.com","header":"Authorization","value":VALUE}),
            json!({"host":"10.0.0.1","header":"Authorization","value":VALUE}),
            json!({"host":"Example.com","header":"Authorization","value":VALUE}),
            json!({"link":"unknown","header":"Authorization","value":VALUE}),
            json!({"host":"a.example.com","link":"connector","header":"Authorization","value":VALUE}),
            json!({"host":"a.example.com","header":"Host","value":VALUE}),
            json!({"host":"a.example.com","header":"Proxy-Authorization","value":VALUE}),
            json!({"host":"a.example.com","header":"Authorization","value":format!("{VALUE}\r\nX: y")}),
            json!({"host":"a.example.com","header":"Authorization","path":"/v1/../admin","value":VALUE}),
            json!({"host":"a.example.com","header":"Authorization","path":"/v1/%2e","value":VALUE}),
            json!({"host":"a.example.com","header":"Authorization","method":"get","value":VALUE}),
            json!({"host":"a.example.com","header":"Authorization","value":"x".repeat(MAX_VALUE_BYTES + 1)}),
            json!({"host":"a.example.com","header":"Authorization","value":VALUE,"extra":VALUE}),
            json!({"host":"a.example.com","header":"Authorization","value":{"nested":VALUE}}),
        ] {
            let error = verify(&pair, json!([rule]), 1).unwrap_err();
            let text = format!("{error:?} {error}");
            assert!(!text.contains(VALUE), "{text}");
        }
        let too_many: Vec<_> = (0..=MAX_RULES)
            .map(|i| json!({"host":format!("h{i}.example.com"),"header":"Authorization","value":VALUE}))
            .collect();
        assert!(verify(&pair, json!(too_many), 1).is_err());
    }

    #[test]
    fn rules_match_their_own_target_port_method_and_path() {
        let pair = KeyPair::from_seed(Seed::new([3; 32]));
        let record = verify(
            &pair,
            json!([
                {"host":"api.example.com","header":"Authorization","valuePrefix":"Bearer ","value":VALUE,"method":"GET","path":"/v1/"},
                {"link":"connector","header":"X-Turn","value":VALUE}
            ]),
            1,
        )
        .unwrap();
        assert!(!format!("{record:?} {:?}", record.rules).contains(VALUE));
        assert_eq!(
            record.rules[0].value.to_str().unwrap(),
            format!("Bearer {VALUE}")
        );
        assert!(record.rules[0].value.is_sensitive());
        let store = CredentialStore::new(1 << 20);
        assert!(store.apply(record).unwrap());
        let host = RuleTarget::Host("api.example.com".into());
        let link = RuleTarget::Link("connector".into());
        assert_eq!(store.rules_for("p/r", "box", &host, 443).len(), 1);
        assert!(store.rules_for("p/r", "box", &host, 8443).is_empty());
        assert!(store.rules_for("p/r", "other", &host, 443).is_empty());
        assert!(store
            .rules_for(
                "p/r",
                "box",
                &RuleTarget::Host("other.example.com".into()),
                443
            )
            .is_empty());
        assert_eq!(store.rules_for("p/r", "box", &link, 8080).len(), 1);
        let rule = &store.rules_for("p/r", "box", &host, 443)[0];
        assert!(rule.matches_request(&Method::GET, "/v1/messages"));
        assert!(!rule.matches_request(&Method::POST, "/v1/messages"));
        assert!(!rule.matches_request(&Method::GET, "/v2"));
        assert!(!rule.matches_request(&Method::GET, "/v1"));
    }

    #[test]
    fn the_newest_set_replaces_the_whole_set_and_older_ones_are_ignored() {
        let pair = KeyPair::from_seed(Seed::new([3; 32]));
        let store = CredentialStore::new(1 << 20);
        let host = RuleTarget::Host("api.example.com".into());
        let rule = json!([{"host":"api.example.com","header":"Authorization","value":VALUE}]);
        assert!(store
            .apply(verify(&pair, rule.clone(), 10).unwrap())
            .unwrap());
        assert!(!store
            .apply(verify(&pair, rule.clone(), 10).unwrap())
            .unwrap());
        assert!(store.apply(verify(&pair, json!([]), 11).unwrap()).unwrap());
        assert!(store.rules_for("p/r", "box", &host, 443).is_empty());
        assert!(!store.apply(verify(&pair, rule, 9).unwrap()).unwrap());
        assert!(store.rules_for("p/r", "box", &host, 443).is_empty());
        assert_eq!(store.version("p/r", "box").unwrap().unwrap().0, 11);
    }

    #[test]
    fn a_full_store_refuses_instead_of_evicting() {
        let pair = KeyPair::from_seed(Seed::new([3; 32]));
        let store = CredentialStore::new(4096);
        let rule =
            json!([{"host":"api.example.com","header":"Authorization","value":"x".repeat(4000)}]);
        let error = store.apply(verify(&pair, rule, 1).unwrap()).unwrap_err();
        assert_eq!(error.code, "SANDBOX_EGRESS_POLICY_STORE_FULL");
    }
}
