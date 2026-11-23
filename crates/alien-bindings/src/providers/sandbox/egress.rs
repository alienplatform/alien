//! Shared validation and explicit refusal for backends without a gateway.
use crate::error::{ErrorData, Result};
use alien_core::{EgressPolicy, SandboxEgress};
use alien_error::{AlienError, ContextError};

pub(crate) fn unsupported(operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressGatewayUnavailable {
        reason: format!("{operation} requires gateway enforcement"),
    })
}
/// Refuse an invalid rule set before any backend is contacted. Messages never carry a value.
pub(crate) fn validate_credentials(rules: &[alien_core::SandboxCredentialRule]) -> Result<()> {
    alien_core::validate_sandbox_credentials(rules).map_err(|reason| {
        AlienError::new(ErrorData::InvalidInput {
            operation_context: "sandbox.updateCredentials".into(),
            details: reason,
            field_name: Some("rules".into()),
        })
    })
}
/// Message for host rules on a sandbox that never received the per-sandbox CA.
pub(crate) const SANDBOX_WITHOUT_CA: &str = "this sandbox was created without the gateway's CA \
(on a gateway that predates per-sandbox credentials), so credential rules for an internet host \
cannot be served (HTTPS to the host could not be verified); rules for a link work. Create the \
sandbox again";

/// Refuse host rules for a sandbox known not to hold the CA (a platform with no agent health
/// endpoint to ask). Link rules are never refused.
pub(crate) fn require_known_ca_for_host_rules(
    rules: &[alien_core::SandboxCredentialRule],
    has_ca: bool,
) -> Result<()> {
    if has_ca || rules.iter().all(|rule| rule.host.is_none()) {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::OperationNotSupported {
        operation: "sandbox.updateCredentials".to_string(),
        reason: SANDBOX_WITHOUT_CA.to_string(),
    }))
}

/// Refuse host rules for a sandbox whose agent reports no installed CA. `health` is the agent's
/// health request. Link rules need no CA and are never refused here.
pub(crate) async fn require_ca_for_host_rules(
    rules: &[alien_core::SandboxCredentialRule],
    health: impl std::future::Future<Output = Result<reqwest::RequestBuilder>>,
) -> Result<()> {
    if rules.iter().all(|rule| rule.host.is_none()) {
        return Ok(());
    }
    let refusal = || {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "sandbox.updateCredentials".to_string(),
            reason: SANDBOX_WITHOUT_CA.to_string(),
        })
    };
    let unreachable = |reason: &str| {
        AlienError::new(ErrorData::SandboxUnreachable {
            operation: "sandbox.updateCredentials".to_string(),
            reason: reason.to_string(),
        })
    };
    let response = health
        .await?
        .send()
        .await
        .map_err(|_| unreachable("the sandbox agent did not answer its health check"))?;
    if !response.status().is_success() {
        return Err(unreachable("the sandbox agent's health check failed"));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| unreachable("the sandbox agent's health answer was not readable"))?;
    if body["egressCaPath"].is_string() {
        Ok(())
    } else {
        Err(refusal())
    }
}
pub(crate) fn policy_error(error: AlienError<alien_core::ErrorData>) -> AlienError<ErrorData> {
    let context = match &error.error {
        Some(alien_core::ErrorData::SandboxEgressPolicyTooLarge { limit }) => {
            ErrorData::SandboxEgressPolicyTooLarge {
                limit: limit.clone(),
            }
        }
        _ => ErrorData::SandboxEgressInvalid {
            reason: error.message.clone(),
        },
    };
    error.context(context)
}
pub(crate) fn validate(
    requested: &EgressPolicy,
    declaration: Option<&SandboxEgress>,
) -> Result<()> {
    if !matches!(declaration, Some(SandboxEgress::Internet)) {
        return Err(AlienError::new(ErrorData::SandboxEgressRequiresInternet));
    }
    requested.validate().map_err(policy_error)
}
pub(crate) fn require_default(
    requested: Option<&EgressPolicy>,
    declaration: Option<&SandboxEgress>,
) -> Result<()> {
    if let Some(requested) = requested {
        validate(requested, declaration)?;
        return Err(unsupported("runtime policy"));
    }
    Ok(())
}

#[cfg(test)]
mod ca_gate_tests {
    use super::*;

    fn rule(host: Option<&str>) -> alien_core::SandboxCredentialRule {
        serde_json::from_value(serde_json::json!({
            "host": host, "link": host.is_none().then_some("connector"),
            "header": "Authorization", "value": "v"
        }))
        .unwrap()
    }

    #[test]
    fn host_rules_need_a_known_ca_where_the_agent_cannot_be_asked() {
        let host = [rule(Some("api.example.com"))];
        let link = [rule(None)];
        let error = require_known_ca_for_host_rules(&host, false).unwrap_err();
        assert!(error.message.contains("created without the gateway's CA"));
        assert!(require_known_ca_for_host_rules(&host, true).is_ok());
        assert!(require_known_ca_for_host_rules(&link, false).is_ok());
        assert!(require_known_ca_for_host_rules(&[], false).is_ok());
    }

    async fn agent(body: &'static str) -> String {
        let router = axum::Router::new().route(
            "/v1/health",
            axum::routing::get(move || async move { body }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/health", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    #[tokio::test]
    async fn host_rules_need_an_installed_ca_and_link_rules_do_not() {
        let without = agent(r#"{"status":"ready"}"#).await;
        let with = agent(r#"{"status":"ready","egressCaPath":"/tmp/x.pem"}"#).await;
        let ask = |url: String| async move { Ok(reqwest::Client::new().get(url)) };
        // A link rule never contacts the agent.
        require_ca_for_host_rules(&[rule(None)], async { Err(unsupported("never called")) })
            .await
            .unwrap();
        let error = require_ca_for_host_rules(&[rule(Some("api.example.com"))], ask(without))
            .await
            .unwrap_err();
        assert!(
            error.message.contains("created without the gateway's CA"),
            "{}",
            error.message
        );
        require_ca_for_host_rules(&[rule(Some("api.example.com"))], ask(with))
            .await
            .unwrap();
    }
}
