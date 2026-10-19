//! Trusted manager-local assertion sources for deployment-owned GCP federation.

use std::collections::HashMap;

use alien_bindings::{BindingsProviderApi, ImpersonationRequest, ServiceAccountInfo};
use alien_core::{ClientConfig, GcpCredentials, GcpFederationSource, GcpFederationTrust};
use alien_error::AlienError;

/// Older manager metadata can omit the capability. New GCP setup must require
/// it; absence never selects direct target-account impersonation.
pub fn federation_trust(
    google_unique_id: Option<&str>,
    env: &HashMap<String, String>,
) -> alien_core::Result<Option<GcpFederationTrust>> {
    let trust = if env.contains_key("KUBERNETES_SERVICE_HOST") {
        match (env.get("GCP_MANAGEMENT_OIDC_ISSUER"), env.get("GCP_MANAGEMENT_OIDC_SUBJECT")) {
            (None, None) => return Ok(None),
            (Some(issuer), Some(subject)) => {
                kubernetes_account(subject)?;
                GcpFederationTrust { issuer: issuer.clone(), subject: subject.clone() }
            }
            _ => return Err(invalid("managerSource", "Kubernetes federation requires both GCP_MANAGEMENT_OIDC_ISSUER and GCP_MANAGEMENT_OIDC_SUBJECT")),
        }
    } else {
        let Some(subject) = google_unique_id else {
            return Ok(None);
        };
        GcpFederationTrust {
            issuer: "https://accounts.google.com".to_string(),
            subject: subject.to_string(),
        }
    };
    trust.validate()?;
    Ok(Some(trust))
}

/// Hosted acquisition does not load or impersonate a vendor Google account.
/// Private GCP acquisition retains the existing container-to-linked-account grant.
pub async fn assertion_source(
    provider: &dyn BindingsProviderApi,
    env: &HashMap<String, String>,
) -> Result<GcpFederationSource, AlienError> {
    if env.contains_key("KUBERNETES_SERVICE_HOST") {
        let trust = federation_trust(None, env).map_err(|error| error.into_generic())?
            .ok_or_else(|| invalid("managerSource", "the selected Kubernetes manager does not advertise GCP federation; update its deployment configuration").into_generic())?;
        let (namespace, service_account) =
            kubernetes_account(&trust.subject).map_err(|error| error.into_generic())?;
        return Ok(GcpFederationSource::KubernetesServiceAccount {
            issuer: trust.issuer,
            api_server: "https://kubernetes.default.svc".to_string(),
            api_token_file: "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string(),
            ca_file: "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt".to_string(),
            namespace: namespace.to_string(),
            service_account: service_account.to_string(),
        });
    }
    let account = provider
        .load_service_account("management")
        .await
        .map_err(|error| error.into_generic())?;
    let ServiceAccountInfo::Gcp(info) = account
        .get_info()
        .await
        .map_err(|error| error.into_generic())?
    else {
        return Err(invalid(
            "managerSource",
            "GCP federation requires the linked Google management identity",
        )
        .into_generic());
    };
    let trust = federation_trust(Some(&info.unique_id), env)
        .map_err(|error| error.into_generic())?
        .ok_or_else(|| {
            invalid(
                "managerSource",
                "Google manager identity does not advertise federation",
            )
            .into_generic()
        })?;
    // The binding returns a refreshable configuration without minting a token.
    // Extract the existing caller so generateIdToken does not require a self-grant.
    let config = account
        .impersonate(ImpersonationRequest {
            session_name: None,
            duration_seconds: Some(3600),
            scopes: None,
        })
        .await
        .map_err(|error| error.into_generic())?;
    let ClientConfig::Gcp(config) = config else {
        return Err(invalid(
            "managerSource",
            "linked Google identity returned a different cloud configuration",
        )
        .into_generic());
    };
    let GcpCredentials::ImpersonatedServiceAccount { source, config } = config.credentials else {
        return Err(invalid(
            "managerSource",
            "linked Google identity did not supply its container caller configuration",
        )
        .into_generic());
    };
    if config.service_account_email != info.email {
        return Err(invalid(
            "managerSource",
            "linked Google identity changed while acquiring its assertion source",
        )
        .into_generic());
    }
    Ok(GcpFederationSource::GoogleServiceAccount {
        source,
        service_account_email: info.email,
        service_account_unique_id: trust.subject,
    })
}

fn kubernetes_account(subject: &str) -> alien_core::Result<(&str, &str)> {
    let account = subject
        .strip_prefix("system:serviceaccount:")
        .and_then(|rest| rest.split_once(':'))
        .filter(|(namespace, name)| {
            !namespace.is_empty() && !name.is_empty() && !name.contains(':')
        })
        .ok_or_else(|| {
            invalid(
                "managerSource.subject",
                "requires an exact Kubernetes service-account subject",
            )
        })?;
    Ok(account)
}

fn invalid(field: &str, reason: &str) -> AlienError<alien_core::ErrorData> {
    AlienError::new(alien_core::ErrorData::GcpFederationInvalid {
        field: field.to_string(),
        reason: reason.to_string(),
    })
}
