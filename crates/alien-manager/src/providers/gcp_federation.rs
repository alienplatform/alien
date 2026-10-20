//! Trusted manager-local assertion sources for deployment-owned GCP federation.

use std::collections::HashMap;

use alien_bindings::{BindingsProviderApi, ImpersonationRequest, ServiceAccountInfo};
use alien_core::{ClientConfig, GcpCredentials, GcpFederationSource, GcpFederationTrust};
use alien_error::AlienError;

/// Older manager metadata can omit the capability. New GCP setup must require
/// it; absence never selects direct target-account impersonation.
///
/// A configured Kubernetes issuer selects the manager's own Kubernetes service
/// account (the hosted manager). Without one, the linked Google account is the
/// trusted identity, including for private managers that run in Kubernetes.
pub fn federation_trust(
    google_unique_id: Option<&str>,
    env: &HashMap<String, String>,
) -> alien_core::Result<Option<GcpFederationTrust>> {
    let trust = match kubernetes_trust(env)? {
        Some(trust) => trust,
        None => {
            let Some(subject) = google_unique_id else {
                return Ok(None);
            };
            GcpFederationTrust {
                issuer: "https://accounts.google.com".to_string(),
                subject: subject.to_string(),
            }
        }
    };
    trust.validate()?;
    Ok(Some(trust))
}

/// The Kubernetes identity named by `GCP_MANAGEMENT_OIDC_ISSUER` and
/// `GCP_MANAGEMENT_OIDC_SUBJECT`, or `None` when neither is set.
fn kubernetes_trust(env: &HashMap<String, String>) -> alien_core::Result<Option<GcpFederationTrust>> {
    match (env.get("GCP_MANAGEMENT_OIDC_ISSUER"), env.get("GCP_MANAGEMENT_OIDC_SUBJECT")) {
        (None, None) => Ok(None),
        (Some(issuer), Some(subject)) => {
            kubernetes_account(subject)?;
            Ok(Some(GcpFederationTrust { issuer: issuer.clone(), subject: subject.clone() }))
        }
        _ => Err(invalid("managerSource", "Kubernetes federation requires both GCP_MANAGEMENT_OIDC_ISSUER and GCP_MANAGEMENT_OIDC_SUBJECT")),
    }
}

/// A configured Kubernetes identity (the hosted manager) does not load or
/// impersonate a vendor Google account. Otherwise acquisition uses the linked
/// Google account through the existing container-to-linked-account grant.
pub async fn assertion_source(
    provider: &dyn BindingsProviderApi,
    env: &HashMap<String, String>,
) -> Result<GcpFederationSource, AlienError> {
    if let Some(trust) = kubernetes_trust(env).map_err(|error| error.into_generic())? {
        trust.validate().map_err(|error| error.into_generic())?;
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

    fn in_cluster(extra: &[(&str, &str)]) -> HashMap<String, String> {
        let mut env = HashMap::from([(
            "KUBERNETES_SERVICE_HOST".to_string(),
            "10.0.0.1".to_string(),
        )]);
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        env
    }

    #[tokio::test]
    async fn manager_in_kubernetes_without_an_issuer_uses_its_linked_google_account() {
        let bindings = provider(HashMap::from([("management".to_string(), account())]));
        let env = in_cluster(&[]);
        let source = assertion_source(&bindings, &env).await.unwrap();
        assert!(matches!(
            source,
            GcpFederationSource::GoogleServiceAccount { .. }
        ));
        let trust = federation_trust(Some("123456789012345678901"), &env)
            .unwrap()
            .expect("the linked account is advertised");
        assert_eq!(trust.issuer, "https://accounts.google.com");
        assert_eq!(trust.subject, "123456789012345678901");
    }

    #[tokio::test]
    async fn configured_kubernetes_issuer_selects_the_manager_service_account() {
        let bindings = provider(HashMap::from([("management".to_string(), account())]));
        let env = in_cluster(&[
            ("GCP_MANAGEMENT_OIDC_ISSUER", "https://oidc.example.com/cluster"),
            ("GCP_MANAGEMENT_OIDC_SUBJECT", "system:serviceaccount:platform:manager"),
        ]);
        let GcpFederationSource::KubernetesServiceAccount {
            issuer,
            namespace,
            service_account,
            ..
        } = assertion_source(&bindings, &env).await.unwrap()
        else {
            panic!("Kubernetes source required")
        };
        assert_eq!(issuer, "https://oidc.example.com/cluster");
        assert_eq!((namespace.as_str(), service_account.as_str()), ("platform", "manager"));
        let trust = federation_trust(Some("123456789012345678901"), &env)
            .unwrap()
            .unwrap();
        assert_eq!(trust.subject, "system:serviceaccount:platform:manager");
    }

    #[tokio::test]
    async fn half_configured_kubernetes_issuer_is_refused() {
        let bindings = provider(HashMap::from([("management".to_string(), account())]));
        let env = in_cluster(&[("GCP_MANAGEMENT_OIDC_ISSUER", "https://oidc.example.com/cluster")]);
        assert!(assertion_source(&bindings, &env).await.is_err());
        assert!(federation_trust(Some("123456789012345678901"), &env).is_err());
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
