//! Azure's native proxy transport and durable gateway identity mapping.
use super::AzureSandbox;
use crate::{
    error::{ErrorData, Result},
    sandbox_egress::SandboxEgressManager,
};
use alien_azure_clients::azure::sandbox_data_plane::{EgressHostRule, EgressPolicy, Sandbox};
use alien_core::{EgressPolicy as RuntimePolicy, SANDBOX_EXPIRY_LABEL};
use alien_error::AlienError;
use std::{collections::BTreeMap, sync::Arc};

const IDENTITY_LABEL: &str = "alien-egress-identity";

/// Azure echoes the effective HTTP policy twice and adds its enforcement mode.
/// Both copies must match, including rule order; unknown fields remain refused.
fn gateway_policy_holds(expected: &EgressPolicy, effective: &EgressPolicy) -> bool {
    let enforced_matches = |mut policy: EgressPolicy| {
        let mode = policy.unmodelled.remove("enforcementMode");
        mode.is_none_or(|mode| mode.as_str() == Some("Enforced")) && policy == *expected
    };
    let mut flat = effective.clone();
    let http = flat.unmodelled.remove("http");
    enforced_matches(flat)
        && match http {
            None => true,
            Some(http) => serde_json::from_value(http).is_ok_and(enforced_matches),
        }
}

/// Where the agent's failover forwarder listens in the guest (see `alien-sandbox-agent`
/// `proxy_failover`); commands' proxy variables point here when two gateway replicas exist.
const PROXY_LOOPBACK: &str = "127.0.0.1:18128";

/// Azure's platform resolver, the one non-gateway destination the sandbox subnet allows (DNS).
const AZURE_RESOLVER: &str = "168.63.129.16";

impl AzureSandbox {
    /// Use the deployment's gateway for policy writes and create-time proxy credentials.
    pub fn with_egress_manager(
        mut self,
        manager: Arc<SandboxEgressManager>,
        default_policy: Option<RuntimePolicy>,
    ) -> Self {
        self.egress_manager = Some(manager);
        self.default_policy = default_policy;
        self
    }

    pub(super) fn requested_policy(
        &self,
        requested: Option<&RuntimePolicy>,
    ) -> Result<Option<RuntimePolicy>> {
        let policy = requested.or(self.default_policy.as_ref());
        if self.egress_manager.is_none() {
            super::super::egress::require_default(policy, Some(&self.egress))?;
            return Ok(None);
        }
        let policy = policy.cloned().unwrap_or(RuntimePolicy::Internet);
        super::super::egress::validate(&policy, Some(&self.egress))?;
        Ok(Some(policy))
    }

    /// A policy (`none`, or the gateway floor under `internet`) with the persistent storage
    /// store's blob host allowed ahead of every other rule, so the agent reaches its store.
    /// Commands are kept from it inside the guest: their sockets carry the guest mark, and the
    /// agent drops their direct connections (see [`Self::guest_egress`]). No policy (open
    /// egress) stays none.
    pub(super) fn with_store_host(&self, policy: Option<EgressPolicy>) -> Option<EgressPolicy> {
        let Some(account) = self.store_account.as_ref() else {
            return policy;
        };
        let mut policy = policy?;
        policy.host_rules.insert(
            0,
            EgressHostRule {
                pattern: format!("{account}.blob.core.windows.net"),
                action: alien_azure_clients::azure::sandbox_data_plane::ALLOW.to_string(),
            },
        );
        Some(policy)
    }

    /// What the agent enforces on guest sockets when persistent storage opens the store's host:
    /// `none` under `none`, and under `internet` the gateway replicas and the platform resolver
    /// alone, which are the direct destinations commands have today. Nothing for a sandbox
    /// without persistent storage or with open egress.
    pub(super) fn guest_egress(&self) -> Result<Option<String>> {
        if self.store_account.is_none() {
            return Ok(None);
        }
        let Some(manager) = &self.egress_manager else {
            return Ok(match self.egress {
                alien_core::SandboxEgress::None => Some("none".to_string()),
                _ => None,
            });
        };
        let mut allowed = Vec::new();
        for authority in manager.proxy_authorities() {
            let address = authority
                .rsplit_once(':')
                .map_or(authority.as_str(), |(host, _)| host)
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| {
                    not_as_declared(
                        "new",
                        &format!("gateway address '{authority}' is not an IPv4 address"),
                    )
                })?;
            allowed.push(address.to_string());
        }
        allowed.push(AZURE_RESOLVER.to_string());
        Ok(Some(format!("only:{}", allowed.join(","))))
    }

    /// What the agent needs of the persistent storage declaration at boot: its half of the
    /// declaration (the store and the rights arrive at attach), and the root the platform's
    /// egress proxy re-signs the store's TLS with. Empty when nothing is declared.
    pub(super) fn storage_environment(&self) -> Vec<(String, String)> {
        let Some(storage) = &self.persistent_storage else {
            return Vec::new();
        };
        vec![
            (
                alien_core::sandbox_egress::ENV_PERSISTENT_STORAGE.to_string(),
                serde_json::json!({
                    "path": storage.path,
                    "exclude": storage.exclude,
                    "maxSizeBytes": storage.max_size_bytes,
                })
                .to_string(),
            ),
            (
                alien_core::ENV_ALIEN_SANDBOX_STORE_TRUST_ROOTS.to_string(),
                alien_core::AZURE_EGRESS_PROXY_CA_PATH.to_string(),
            ),
        ]
    }

    pub(super) fn creation_deadline(&self) -> Option<i64> {
        self.egress_manager.as_ref().map(|_| {
            chrono::Utc::now().timestamp_millis()
                + i64::from(alien_core::AZURE_GATEWAY_MAX_LIFETIME_SECONDS) * 1000
        })
    }

    pub(super) fn native_policy(&self) -> Option<EgressPolicy> {
        if self.egress_manager.is_none() {
            return self.with_store_host(super::egress_policy(&self.egress));
        }
        // Full/default-Deny inspects the inner CONNECT destination and rejects the gateway.
        // Partial requires default-Allow. The one ordered catch-all closes the :80/:443
        // interposer. There is no gateway rule: Azure refuses a host:port pattern, and a host
        // rule for an IP does not open a TCP port. Setup's NSG allows the linked gateway ports
        // and denies platform DNS/IMDS and other traffic; the admin port has no rule and is not
        // reachable from sandboxes.
        self.with_store_host(Some(EgressPolicy {
            default_action: "Allow".into(),
            traffic_inspection: Some("Partial".into()),
            host_rules: vec![EgressHostRule {
                pattern: "*".into(),
                action: "Deny".into(),
            }],
            rules: Vec::new(),
            unmodelled: Default::default(),
        }))
    }

    pub(super) async fn prepare_identity(
        &self,
        environment: &mut BTreeMap<String, String>,
        labels: &mut BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        let Some(manager) = &self.egress_manager else {
            return Ok(None);
        };
        let expires = labels
            .get(SANDBOX_EXPIRY_LABEL)
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                not_as_declared("new", "gateway identity requires an expiry deadline")
            })?;
        // Azure chooses its vendor id after create; the label persists the pre-created identity.
        let identity = uuid::Uuid::new_v4().to_string();
        let token = manager.mint_identity(&identity, expires / 1000).await?;
        // An agent image can run the loopback forwarder, so with several replicas its proxy
        // address is the loopback and the agent fails over between the replicas per connection.
        // Without the agent there is no forwarder: the one gateway address stays.
        let replicas = manager.proxy_authorities();
        let failover = self.agent && replicas.len() > 1;
        let proxy_authority = if failover {
            PROXY_LOOPBACK
        } else {
            manager.gateway_authority()
        };
        let proxy = format!("http://alien:{}@{}", token, proxy_authority);
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            environment.insert(name.into(), proxy.clone());
        }
        for name in ["NO_PROXY", "no_proxy"] {
            environment.insert(name.into(), "localhost,127.0.0.1".into());
        }
        // The gateway answers its first connect slowly (measured 7-20 s), so a packaged agent
        // proves the path itself before the ENTRYPOINT starts: the gateway must connect and the
        // public canaries must drop, retried for up to 60 s, and the workload never starts if
        // they do not. The variables are reserved, so a caller cannot have set them.
        if self.agent {
            for (name, value) in [
                ("ALIEN_SANDBOX_STARTUP_EGRESS_GATE", "1"),
                ("ALIEN_SANDBOX_STARTUP_GATE_VNET", "1"),
                ("ALIEN_SANDBOX_STARTUP_GATE_BOUND_SECS", "60"),
                (
                    "ALIEN_SANDBOX_STARTUP_GATE_CONTROL",
                    replicas.join(",").as_str(),
                ),
                (
                    alien_core::sandbox_image::ENV_MUST_DROP_PUBLIC,
                    // Not :443 or :80: Azure's interposer accepts TCP there and answers 403, so a
                    // connect proves nothing. Those ports are contained by the interposer and the
                    // gateway's policy; the NSG drops every other public port.
                    "1.1.1.1:8443,1.1.1.1:8080",
                ),
            ] {
                environment.insert(name.into(), value.into());
            }
            if failover {
                environment.insert("ALIEN_SANDBOX_PROXY_REPLICAS".into(), replicas.join(","));
                environment.insert("ALIEN_SANDBOX_PROXY_LISTEN".into(), PROXY_LOOPBACK.into());
            }
        }
        labels.insert(IDENTITY_LABEL.into(), identity.clone());
        // The gateway has no policy for this identity yet and denies all outbound requests.
        // Create publishes the policy only after Azure returns the expected floor and label.
        Ok(Some(identity))
    }

    pub(super) fn check_gateway_floor(&self, sandbox: &Sandbox) -> Result<()> {
        if !self
            .native_policy()
            .as_ref()
            .zip(sandbox.egress_policy.as_ref())
            .is_some_and(|(expected, effective)| gateway_policy_holds(expected, effective))
        {
            return Err(not_as_declared(
                &sandbox.id,
                "native gateway-only policy differs from the binding",
            ));
        }
        identity(sandbox)?;
        let expires = sandbox
            .labels
            .get(SANDBOX_EXPIRY_LABEL)
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                not_as_declared(&sandbox.id, "gateway expiry label is missing or invalid")
            })?;
        if expires <= chrono::Utc::now().timestamp_millis() {
            return Err(not_as_declared(
                &sandbox.id,
                "gateway identity has expired; create a new sandbox",
            ));
        }
        Ok(())
    }

    pub(super) async fn revoke_identity(&self, sandbox_id: &str) -> Result<()> {
        let Some(manager) = &self.egress_manager else {
            return Ok(());
        };
        let Some(sandbox) = self.read_sandbox("sandbox.terminate", sandbox_id).await? else {
            return Ok(());
        };
        manager.delete(identity(&sandbox)?).await
    }

    pub(super) async fn discard_with_policy(
        &self,
        sandbox_id: &str,
        identity: Option<&str>,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        // A failed revocation is logged, not wrapped around the reason: wrapping would replace the
        // publish failure's cause and retryability with the cleanup's. The identity is bound to the
        // sandbox being deleted below and expires with it.
        if let (Some(manager), Some(identity)) = (&self.egress_manager, identity) {
            if let Err(error) = manager.delete(identity).await {
                tracing::warn!(
                    sandbox = %sandbox_id,
                    %error,
                    "could not revoke the gateway identity of a failed create; terminating its sandbox"
                );
            }
        }
        self.discard(sandbox_id, reason).await
    }
}

pub(super) fn identity(sandbox: &Sandbox) -> Result<&str> {
    let identity = sandbox
        .labels
        .get(IDENTITY_LABEL)
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .ok_or_else(|| {
            not_as_declared(&sandbox.id, "gateway identity label is missing or invalid")
        })?;
    Ok(identity)
}

pub(super) fn not_as_declared(id: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxNotAsDeclared {
        sandbox_id: id.into(),
        restriction: "gateway identity and transport".into(),
        reason: reason.into(),
    })
}

#[cfg(all(test, feature = "local"))]
#[path = "azure_egress_tests.rs"]
mod tests;
