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
    let account = match provider.load_service_account("management").await {
        Ok(account) => account,
        // Both names are used by manager stacks. Do not change identities
        // when a configured primary binding is malformed or inaccessible.
        Err(error) if error.code == "BINDING_NOT_CONFIGURED" => provider
            .load_service_account("management-sa")
            .await
            .map_err(|error| error.into_generic())?,
        Err(error) => return Err(error.into_generic()),
    };
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

#[cfg(test)]
mod binding_tests {
    use super::*;
    use alien_bindings::BindingsProvider;
    use alien_core::{bindings::ServiceAccountBinding, GcpClientConfig};

    fn provider(bindings: HashMap<String, serde_json::Value>) -> BindingsProvider {
        BindingsProvider::new(
            ClientConfig::Gcp(Box::new(GcpClientConfig {
                project_id: "source-project".to_string(),
                region: "us-central1".to_string(),
                project_number: None,
                service_overrides: None,
                credentials: GcpCredentials::ServiceMetadata,
            })),
            bindings,
        )
        .unwrap()
    }

    fn account() -> serde_json::Value {
        serde_json::to_value(ServiceAccountBinding::gcp_service_account(
            "manager@source-project.iam.gserviceaccount.com".to_string(),
            "123456789012345678901".to_string(),
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn linked_account_names_preserve_the_container_credential_source() {
        for name in ["management", "management-sa"] {
            let bindings = provider(HashMap::from([(name.to_string(), account())]));
            let source = assertion_source(&bindings, &HashMap::new()).await.unwrap();
            let GcpFederationSource::GoogleServiceAccount {
                source,
                service_account_email,
                service_account_unique_id,
            } = source
            else {
                panic!("Google source required")
            };
            assert!(matches!(
                source.credentials,
                GcpCredentials::ServiceMetadata
            ));
            assert_eq!(
                service_account_email,
                "manager@source-project.iam.gserviceaccount.com"
            );
            assert_eq!(service_account_unique_id, "123456789012345678901");
        }
    }

    #[tokio::test]
    async fn malformed_primary_binding_does_not_switch_to_another_account() {
        let bindings = provider(HashMap::from([
            (
                "management".to_string(),
                serde_json::json!({"type":"invalid"}),
            ),
            ("management-sa".to_string(), account()),
        ]));
        assert!(assertion_source(&bindings, &HashMap::new()).await.is_err());
    }
}
