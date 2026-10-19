//! Non-secret trust and access descriptions for customer-owned GCP federation.

use alien_error::AlienError;
use serde::{Deserialize, Serialize};
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
    pub trust: GcpFederationTrust,
}

impl GcpFederationAccess {
    pub fn validate(&self) -> Result<()> {
        self.trust.validate()?;
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
    pub fn audience(&self) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> GcpFederationAccess {
        GcpFederationAccess {
            project_number: "123456789012".to_string(),
            pool_id: "deployment-management".to_string(),
            provider_id: "manager".to_string(),
            service_account_email: "management@example-project.iam.gserviceaccount.com".to_string(),
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
            assert!(config.audience().is_err(), "accepted project {value}");
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
            assert!(config.audience().is_err(), "accepted pool {value}");
            config = access();
            config.provider_id = value.to_string();
            assert!(config.audience().is_err(), "accepted provider {value}");
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
            assert!(config.audience().is_err());
        }
        for subject in [
            "",
            "manager@example-project.iam.gserviceaccount.com",
            "123\n456",
        ] {
            let mut config = access();
            config.trust.subject = subject.to_string();
            assert!(config.audience().is_err());
        }
    }

    #[test]
    fn kubernetes_subject_is_exact_and_pool_local() {
        let mut config = access();
        config.trust = GcpFederationTrust {
            issuer: "https://oidc.eks.example/id/cluster".to_string(),
            subject: "system:serviceaccount:runtime:manager".to_string(),
        };
        assert_eq!(config.audience().unwrap(), "//iam.googleapis.com/projects/123456789012/locations/global/workloadIdentityPools/deployment-management/providers/manager");
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
