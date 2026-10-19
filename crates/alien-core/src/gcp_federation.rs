//! Non-secret trust and access descriptions for customer-owned GCP federation.

use alien_error::AlienError;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{ErrorData, Result};

/// The exact manager identity trusted by a deployment-owned identity pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpFederationTrust {
    /// OIDC issuer; Google or the manager's Kubernetes issuer.
    pub issuer: String,
    /// Exact immutable Google account ID or Kubernetes service-account subject.
    pub subject: String,
}

impl GcpFederationTrust {
    pub fn validate(&self) -> Result<()> {
        let issuer = Url::parse(&self.issuer)
            .map_err(|_| invalid("issuer", "must be an absolute HTTPS issuer URL"))?;
        if issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || !issuer.username().is_empty()
            || issuer.password().is_some()
            || issuer.query().is_some()
            || issuer.fragment().is_some()
        {
            return Err(invalid(
                "issuer",
                "must be HTTPS without credentials, query or fragment",
            ));
        }
        if self.subject.is_empty()
            || self.subject.len() > 127
            || self.subject.chars().any(char::is_control)
        {
            return Err(invalid(
                "subject",
                "must contain 1–127 bytes without control characters",
            ));
        }
        if self.issuer == "https://accounts.google.com"
            && !self.subject.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid(
                "subject",
                "Google trust requires the immutable numeric account ID",
            ));
        }
        Ok(())
    }

    /// CEL equality against the exact subject, with string-literal escaping.
    pub fn subject_condition(&self) -> String {
        format!(
            "assertion.sub == {}",
            serde_json::Value::String(self.subject.clone())
        )
    }
}

/// Setup-owned federation access. Contains no assertions, keys or access tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpFederationAccess {
    pub project_number: String,
    pub pool_id: String,
    pub provider_id: String,
    pub service_account_email: String,
    /// Installation- and purpose-specific assertion audience, independently
    /// checked by the manager against its authorized deployment record.
    pub assertion_audience: String,
    pub trust: GcpFederationTrust,
}

impl GcpFederationAccess {
    pub fn validate(&self) -> Result<()> {
        self.trust.validate()?;
        let Some(scope_hash) = self
            .assertion_audience
            .strip_prefix("urn:deployment-access:v1:")
        else {
            return Err(invalid(
                "assertionAudience",
                "must identify an installation-scoped audience",
            ));
        };
        if scope_hash.len() != 64
            || !scope_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "assertionAudience",
                "must contain a SHA-256 installation scope",
            ));
        }
        if self.project_number.is_empty()
            || self.project_number.len() > 30
            || !self
                .project_number
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid("projectNumber", "must be a numeric project number"));
        }
        validate_federation_id("poolId", &self.pool_id)?;
        validate_federation_id("providerId", &self.provider_id)?;
        let Some((account, domain)) = self.service_account_email.split_once('@') else {
            return Err(invalid(
                "serviceAccountEmail",
                "must be a Google service-account email",
            ));
        };
        if account.is_empty()
            || !domain.ends_with(".iam.gserviceaccount.com")
            || !self.service_account_email.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"@.-".contains(&byte)
            })
            || domain.contains('@')
        {
            return Err(invalid(
                "serviceAccountEmail",
                "must be a Google service-account email",
            ));
        }
        Ok(())
    }

    /// Validates before constructing the canonical Google STS audience.
    pub fn provider_audience(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("//iam.googleapis.com/projects/{}/locations/global/workloadIdentityPools/{}/providers/{}", self.project_number, self.pool_id, self.provider_id))
    }

    /// Pool-local exact principal; providers do not introduce a subject namespace.
    pub fn principal(&self) -> Result<String> {
        self.validate()?;
        // Encode the subject as one URI path segment, including slashes.
        let mut principal = Url::parse(&format!(
            "https://iam.googleapis.com/projects/{}/locations/global/workloadIdentityPools/{}/subject/",
            self.project_number, self.pool_id
        ))
        .map_err(|_| invalid("projectNumber", "cannot construct principal URI"))?;
        principal
            .path_segments_mut()
            .map_err(|_| invalid("subject", "cannot construct principal URI"))?
            .pop_if_empty()
            .push(&self.trust.subject);
        Ok(principal.as_str().replacen("https://", "principal://", 1))
    }
}

/// Google pool and provider IDs share the same documented restrictions.
pub fn validate_federation_id(field: &str, value: &str) -> Result<()> {
    if !(4..=32).contains(&value.len())
        || value.starts_with("gcp-")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(invalid(
            field,
            "must contain 4–32 lowercase letters, digits or hyphens and must not start with gcp-",
        ));
    }
    Ok(())
}

fn invalid(field: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::GcpFederationInvalid {
        field: field.to_string(),
        reason: reason.to_string(),
    })
}

/// Control-plane namespace of an installation. The runtime must construct this
/// from its authenticated deployment record, not from target import metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpFederationScope {
    /// Control-plane workspace ID, not a Google Cloud organization.
    pub workspace_id: String,
    /// Control-plane project ID, not the Google Cloud project ID.
    pub project_id: String,
    pub deployment_group_id: String,
    /// Registered logical name; separate from editable cloud display metadata.
    pub deployment_name: String,
}

impl GcpFederationScope {
    /// Check setup-owned access against the runtime's authenticated installation.
    pub fn validate_access(
        &self,
        access: &GcpFederationAccess,
        resource_type: &crate::ResourceType,
    ) -> Result<()> {
        access.validate()?;
        if access.assertion_audience != self.assertion_audience(resource_type)? {
            return Err(invalid(
                "assertionAudience",
                "does not match the authorized installation and access purpose; rerun setup",
            ));
        }
        Ok(())
    }

    /// Binds an assertion to one logical installation and access purpose.
    /// Base64 components keep the cross-language hash unambiguous without
    /// depending on JSON escaping rules in setup engines.
    pub fn assertion_audience(&self, resource_type: &crate::ResourceType) -> Result<String> {
        let name = self.deployment_name.trim();
        if self.workspace_id.is_empty()
            || self.project_id.is_empty()
            || self.deployment_group_id.is_empty()
            || name.is_empty()
        {
            return Err(invalid(
                "scope",
                "requires workspace, project, deployment group and logical deployment name",
            ));
        }
        if resource_type != &crate::RemoteStackManagement::RESOURCE_TYPE
            && resource_type != &crate::RemoteBindings::RESOURCE_TYPE
        {
            return Err(invalid(
                "scope",
                "requires a management or published-resource access purpose",
            ));
        }
        let encoded = [
            self.workspace_id.as_str(),
            self.project_id.as_str(),
            self.deployment_group_id.as_str(),
            name,
            resource_type.as_ref(),
        ]
        .map(|part| STANDARD.encode(part));
        let hash = Sha256::digest(format!("gcp-deployment-access-v1:{}", encoded.join(":")));
        Ok(format!("urn:deployment-access:v1:{hash:x}"))
    }
}

/// Trusted manager-local assertion source. Never read this from target setup outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type", deny_unknown_fields)]
pub enum GcpFederationSource {
    /// The container caller uses its existing scoped grant on a linked account.
    GoogleServiceAccount {
        #[cfg_attr(feature = "openapi", schema(value_type = Object))]
        source: Box<crate::GcpClientConfig>,
        service_account_email: String,
        service_account_unique_id: String,
    },
    /// Audience-specific TokenRequest using rotating in-cluster API credentials.
    KubernetesServiceAccount {
        issuer: String,
        api_server: String,
        api_token_file: String,
        ca_file: String,
        namespace: String,
        service_account: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> GcpFederationAccess {
        GcpFederationAccess {
            project_number: "123456789012".to_string(),
            pool_id: "deployment-management".to_string(),
            provider_id: "manager".to_string(),
            service_account_email: "management@example-project.iam.gserviceaccount.com".to_string(),
            assertion_audience: GcpFederationScope {
                workspace_id: "workspace".to_string(),
                deployment_name: "production".to_string(),
                project_id: "project".to_string(),
                deployment_group_id: "group".to_string(),
            }
            .assertion_audience(&crate::RemoteStackManagement::RESOURCE_TYPE)
            .unwrap(),
            trust: GcpFederationTrust {
                issuer: "https://accounts.google.com".to_string(),
                subject: "123456789012345678901".to_string(),
            },
        }
    }

    #[test]
    fn invalid_target_identifiers_cannot_construct_google_endpoints() {
        for value in [
            "",
            "../other",
            "123/../456",
            "123?host=other",
            "https://other.invalid",
        ] {
            let mut config = access();
            config.project_number = value.to_string();
            assert!(
                config.provider_audience().is_err(),
                "accepted project {value}"
            );
            assert!(config.principal().is_err());
        }
        for value in [
            "abc",
            "UPPER",
            "gcp-reserved",
            "other/provider",
            "other?query",
            "other#fragment",
        ] {
            let mut config = access();
            config.pool_id = value.to_string();
            assert!(config.provider_audience().is_err(), "accepted pool {value}");
            config = access();
            config.provider_id = value.to_string();
            assert!(
                config.provider_audience().is_err(),
                "accepted provider {value}"
            );
        }
    }

    #[test]
    fn issuer_and_subject_cannot_implicitly_broaden_trust() {
        for issuer in [
            "http://issuer.example",
            "https://user:password@issuer.example",
            "https://issuer.example?x=y",
            "https://issuer.example/#fragment",
        ] {
            let mut config = access();
            config.trust.issuer = issuer.to_string();
            assert!(config.provider_audience().is_err());
        }
        for subject in [
            "",
            "manager@example-project.iam.gserviceaccount.com",
            "123\n456",
        ] {
            let mut config = access();
            config.trust.subject = subject.to_string();
            assert!(config.provider_audience().is_err());
        }
    }

    #[test]
    fn kubernetes_subject_is_exact_and_pool_local() {
        let mut config = access();
        config.trust = GcpFederationTrust {
            issuer: "https://oidc.eks.example/id/cluster".to_string(),
            subject: "system:serviceaccount:runtime:manager".to_string(),
        };
        assert_eq!(config.provider_audience().unwrap(), "//iam.googleapis.com/projects/123456789012/locations/global/workloadIdentityPools/deployment-management/providers/manager");
        assert_eq!(config.principal().unwrap(), "principal://iam.googleapis.com/projects/123456789012/locations/global/workloadIdentityPools/deployment-management/subject/system:serviceaccount:runtime:manager");
        assert_eq!(
            config.trust.subject_condition(),
            "assertion.sub == \"system:serviceaccount:runtime:manager\""
        );
        let principal = config.principal().unwrap();
        config.pool_id = "different-deployment".to_string();
        assert_ne!(config.principal().unwrap(), principal);
    }

    #[test]
    fn subject_quotes_cannot_turn_equality_into_a_cel_expression() {
        let mut config = access();
        config.trust.issuer = "https://issuer.example".to_string();
        config.trust.subject = "subject\" || true || assertion.sub == \"other".to_string();
        config.validate().unwrap();
        let condition = config.trust.subject_condition();
        let literal = condition.strip_prefix("assertion.sub == ").unwrap();
        assert_eq!(
            serde_json::from_str::<String>(literal).unwrap(),
            config.trust.subject
        );
    }

    #[test]
    fn assertion_scope_separates_authorization_realms_names_and_access_purposes() {
        let scope = GcpFederationScope {
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            deployment_group_id: "customer-a".to_string(),
            deployment_name: "production".to_string(),
        };
        let management = &crate::RemoteStackManagement::RESOURCE_TYPE;
        let expected = scope.assertion_audience(management).unwrap();
        let trimmed = GcpFederationScope {
            deployment_name: " production ".to_string(),
            ..scope.clone()
        };
        assert_eq!(expected, trimmed.assertion_audience(management).unwrap());
        for other in [
            GcpFederationScope {
                workspace_id: "other-workspace".to_string(),
                ..scope.clone()
            },
            GcpFederationScope {
                project_id: "other-project".to_string(),
                ..scope.clone()
            },
            GcpFederationScope {
                deployment_group_id: "customer-b".to_string(),
                ..scope.clone()
            },
            GcpFederationScope {
                deployment_name: "other-installation".to_string(),
                ..scope.clone()
            },
        ] {
            assert_ne!(expected, other.assertion_audience(management).unwrap());
        }
        assert_ne!(
            expected,
            scope
                .assertion_audience(&crate::RemoteBindings::RESOURCE_TYPE)
                .unwrap()
        );
    }

    #[test]
    fn plain_legacy_email_is_not_a_federation_descriptor() {
        assert!(serde_json::from_str::<GcpFederationAccess>(
            "\"management@example-project.iam.gserviceaccount.com\""
        )
        .is_err());
        let mut value = serde_json::to_value(access()).unwrap();
        value["tokenUrl"] = "https://other.invalid".into();
        assert!(serde_json::from_value::<GcpFederationAccess>(value).is_err());
    }
}
