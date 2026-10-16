//! Durable ownership of one ACM import attempt. Keep one instance per certificate generation.

use std::collections::BTreeMap;

use alien_aws_clients::acm::{
    AcmApi, ImportCertificateRequest, Tag, find_imported_certificates_by_tag_strict,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, ContextError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::aws_tag_scoped::{is_remote_access_denied, is_remote_not_found, with_import_token};
use crate::{ErrorData, Result};

const CERTIFICATE_FINGERPRINT_TAG: &str = "CertificateFingerprint";

/// Persist this state whenever a step returns pending, before invoking it again. The first
/// step records an import token and public-certificate fingerprint without calling ACM.
/// No private key or certificate payload is stored here. A lost import response is recovered
/// through its token; discovery must have unconditioned certificate-tag metadata permission.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsAcmImport {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    ownership_tags: Vec<Tag>,
    #[serde(default)]
    certificate_arns: Vec<String>,
    #[serde(default)]
    selected_arn: Option<String>,
}

impl AwsAcmImport {
    /// Returns `None` when the caller must checkpoint this attempt, and the selected ARN
    /// only after a subsequent read confirms its ownership. Performs at most one mutation.
    /// Use a new attempt for a different certificate; retain the old attempt until all
    /// listeners have stopped referencing it, then call `step_delete` on the old attempt.
    pub async fn step_import(
        &mut self,
        acm: &dyn AcmApi,
        mut request: ImportCertificateRequest,
    ) -> Result<Option<String>> {
        let fingerprint = certificate_fingerprint(&request);
        let ownership = normalized_ownership_tags(request.tags.take().unwrap_or_default())?;
        if self.token.is_none() {
            if !self.certificate_arns.is_empty() || self.selected_arn.is_some() {
                return Err(invalid("ACM import ownership is missing its attempt token"));
            }
            self.token = Some(Uuid::new_v4().to_string());
            self.fingerprint = Some(fingerprint);
            self.ownership_tags = ownership;
            return Ok(None);
        }
        if self.fingerprint.as_ref() != Some(&fingerprint) || self.ownership_tags != ownership {
            return Err(invalid(
                "ACM import inputs changed; retain this attempt for cleanup and start a new certificate generation",
            ));
        }
        let token = self.token.as_deref().expect("token initialized above");
        let found = self.discover(acm).await?;
        if !found.is_empty() {
            let mut changed = false;
            for arn in found {
                if !self.certificate_arns.contains(&arn) {
                    self.certificate_arns.push(arn);
                    changed = true;
                }
            }
            if self.selected_arn.is_none() {
                self.certificate_arns.sort();
                self.selected_arn = self.certificate_arns.first().cloned();
                changed = true;
            }
            let selected = self
                .selected_arn
                .as_deref()
                .expect("nonempty discovered import");
            if !self.verify_owned(acm, selected).await? {
                return Err(invalid("The selected ACM import disappeared"));
            }
            return Ok(if changed {
                None
            } else {
                self.selected_arn.clone()
            });
        }
        if let Some(selected) = self.selected_arn.as_deref() {
            if self.verify_owned(acm, selected).await? {
                return Ok(self.selected_arn.clone());
            }
            return Err(invalid(
                "A recorded ACM import disappeared; refusing to replace it implicitly",
            ));
        }
        if !self.certificate_arns.is_empty() {
            return Err(invalid("Recorded ACM imports have no selected certificate"));
        }
        let mut tags = with_import_token(self.ownership_tags.clone(), token);
        tags.push(Tag {
            key: CERTIFICATE_FINGERPRINT_TAG.to_string(),
            value: fingerprint,
        });
        request.tags = Some(tags);
        let imported = acm
            .import_certificate(request)
            .await
            .map_err(|error| cloud_error(error, "Import managed TLS certificate", false))?;
        self.certificate_arns.push(imported.certificate_arn.clone());
        self.selected_arn = Some(imported.certificate_arn);
        Ok(None)
    }

    /// Deletes at most one owned certificate per invocation and returns true only after
    /// reads prove all attempt imports absent. A denied discovery/delete must not let the
    /// executor skip the remaining cleanup. Never use this state for a customer-owned ARN.
    pub async fn step_delete(&mut self, acm: &dyn AcmApi) -> Result<bool> {
        self.step_delete_except(acm, &[]).await
    }

    // Retire duplicate imports without deleting certificates still used by listeners.
    // Preserve each receipt until a later ownership read confirms absence.
    async fn step_delete_except(
        &mut self,
        acm: &dyn AcmApi,
        retained_arns: &[String],
    ) -> Result<bool> {
        if self.token.is_none() {
            if !self.certificate_arns.is_empty() || self.selected_arn.is_some() {
                return Err(invalid(
                    "Cannot delete ACM imports without their ownership token",
                ));
            }
            return Ok(true);
        }
        let found = self.discover(acm).await.map_err(keep_delete_failure)?;
        for arn in found {
            if !self.certificate_arns.contains(&arn) {
                self.certificate_arns.push(arn);
            }
        }
        for arn in self.certificate_arns.clone() {
            if retained_arns.contains(&arn) {
                continue;
            }
            match self.verify_owned(acm, &arn).await {
                Ok(true) => {}
                Ok(false) => {
                    self.certificate_arns.retain(|candidate| candidate != &arn);
                    if self.selected_arn.as_ref() == Some(&arn) {
                        self.selected_arn = None;
                    }
                    continue;
                }
                Err(error) => return Err(keep_delete_failure(error)),
            }
            match acm.delete_certificate(&arn).await {
                Ok(()) => {}
                Err(error) if is_remote_not_found(&error) => {}
                Err(error)
                    if matches!(&error.error, Some(ClientErrorData::RemoteResourceConflict { message, .. })
                    if message.contains("ResourceInUseException")) =>
                {
                    return Ok(false);
                }
                Err(error) => {
                    return Err(cloud_error(error, "Delete managed TLS certificate", true));
                }
            }
            // Keep the ARN until a later read proves absence, including after a lost response.
            return Ok(false);
        }
        if retained_arns.is_empty() {
            *self = Self::default();
        }
        Ok(true)
    }

    async fn discover(&self, acm: &dyn AcmApi) -> Result<Vec<String>> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| invalid("ACM import token missing"))?;
        let found = find_imported_certificates_by_tag_strict(acm, "CreateAttempt", token)
            .await.map_err(|error| cloud_error(error, "Discover managed TLS certificate imports; rerun setup if certificate metadata access is denied", false))?;
        let mut verified = Vec::new();
        for arn in found {
            if self.verify_owned(acm, &arn).await? {
                verified.push(arn);
            }
        }
        Ok(verified)
    }

    async fn verify_owned(&self, acm: &dyn AcmApi, arn: &str) -> Result<bool> {
        let tags = match acm.list_tags_for_certificate(arn).await {
            Ok(tags) => tags,
            Err(error) if is_remote_not_found(&error) => return Ok(false),
            Err(error) => {
                return Err(cloud_error(
                    error,
                    "Verify managed TLS certificate ownership",
                    false,
                ));
            }
        };
        let matches =
            |key: &str, value: &str| tags.iter().any(|tag| tag.key == key && tag.value == value);
        if !self
            .token
            .as_deref()
            .is_some_and(|token| matches("CreateAttempt", token))
            || !self
                .fingerprint
                .as_deref()
                .is_some_and(|value| matches(CERTIFICATE_FINGERPRINT_TAG, value))
            || !self
                .ownership_tags
                .iter()
                .all(|tag| matches(&tag.key, &tag.value))
        {
            return Err(invalid(
                "Refusing an ACM certificate with foreign or changed ownership tags",
            ));
        }
        Ok(true)
    }
}

fn normalized_ownership_tags(tags: Vec<Tag>) -> Result<Vec<Tag>> {
    let mut normalized = BTreeMap::new();
    for tag in tags {
        if tag.key == "CreateAttempt"
            || tag.key == CERTIFICATE_FINGERPRINT_TAG
            || normalized.insert(tag.key, tag.value).is_some()
        {
            return Err(invalid(
                "ACM ownership tags contain a duplicate or reserved key",
            ));
        }
    }
    if normalized.is_empty() {
        return Err(invalid("Managed ACM imports require ownership tags"));
    }
    Ok(normalized
        .into_iter()
        .map(|(key, value)| Tag { key, value })
        .collect())
}

fn certificate_fingerprint(request: &ImportCertificateRequest) -> String {
    let mut hash = Sha256::new();
    hash.update((request.certificate.len() as u64).to_be_bytes());
    hash.update(request.certificate.as_bytes());
    hash.update(
        request
            .certificate_chain
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    format!("{:x}", hash.finalize())
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: None,
    })
}

fn cloud_error(
    error: AlienError<ClientErrorData>,
    message: &str,
    deleting: bool,
) -> AlienError<ErrorData> {
    if deleting && is_remote_access_denied(&error) {
        return AlienError::new(ErrorData::CloudPlatformError {
            message: format!("{message} denied; rerun setup before retrying cleanup"),
            resource_id: None,
        });
    }
    error.context(ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: None,
    })
}

fn keep_delete_failure(error: AlienError<ErrorData>) -> AlienError<ErrorData> {
    if std::iter::successors(error.source.as_deref(), |source| source.source.as_deref()).any(
        |cause| {
            cause.code == "REMOTE_ACCESS_DENIED"
                || matches!(cause.http_status_code, Some(401 | 403))
        },
    ) || error.code == "REMOTE_ACCESS_DENIED"
        || matches!(error.http_status_code, Some(401 | 403))
    {
        AlienError::new(ErrorData::CloudPlatformError {
            message: error.message,
            resource_id: None,
        })
    } else {
        error
    }
}

/// Managed certificate generations, including abandoned imports whose responses were lost.
/// Keep the serving and prepared ARNs while retiring other generations after listener updates.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsAcmImports {
    #[serde(default)]
    attempts: Vec<AwsAcmImport>,
}

impl AwsAcmImports {
    /// Whether any managed import attempt still needs ownership tracking.
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
    }

    /// Checkpoint pending results before calling again or changing a listener.
    pub async fn step_import(
        &mut self,
        acm: &dyn AcmApi,
        request: ImportCertificateRequest,
    ) -> Result<Option<String>> {
        normalized_ownership_tags(request.tags.clone().unwrap_or_default())?;
        let fingerprint = certificate_fingerprint(&request);
        let index = match self
            .attempts
            .iter()
            .position(|attempt| attempt.fingerprint.as_ref() == Some(&fingerprint))
        {
            Some(index) => index,
            None => {
                self.attempts.push(AwsAcmImport::default());
                self.attempts.len() - 1
            }
        };
        self.attempts[index].step_import(acm, request).await
    }

    /// Returns true once every unretained generation is proven absent. Pass all still-serving
    /// or prepared ARNs. An import with no recorded selected ARN was never published to a
    /// listener, so it is safe to recover and retire after the desired certificate changes.
    /// Pass an empty list only after all managed listeners are deleted.
    pub async fn step_retire(
        &mut self,
        acm: &dyn AcmApi,
        retained_arns: &[String],
    ) -> Result<bool> {
        for index in 0..self.attempts.len() {
            if !self.attempts[index]
                .step_delete_except(acm, retained_arns)
                .await?
            {
                return Ok(false);
            }
            if !self.attempts[index]
                .certificate_arns
                .iter()
                .any(|arn| retained_arns.contains(arn))
            {
                self.attempts.remove(index);
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "aws_acm_import_tests.rs"]
mod tests;
