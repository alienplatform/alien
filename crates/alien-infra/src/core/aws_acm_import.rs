//! Durable ownership of one ACM import attempt. Keep one instance per certificate generation.

use std::collections::BTreeMap;

use alien_aws_clients::acm::{
    find_imported_certificates_by_tag_strict, AcmApi, ImportCertificateRequest, Tag,
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
            return Err(invalid("ACM import inputs changed; retain this attempt for cleanup and start a new certificate generation"));
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
            match self.verify_owned(acm, &arn).await {
                Ok(true) => {}
                Ok(false) => {
                    self.certificate_arns.retain(|candidate| candidate != &arn);
                    continue;
                }
                Err(error) => return Err(keep_delete_failure(error)),
            }
            match acm.delete_certificate(&arn).await {
                Ok(()) => {}
                Err(error) if is_remote_not_found(&error) => {}
                Err(error) => {
                    return Err(cloud_error(error, "Delete managed TLS certificate", true))
                }
            }
            // Keep the ARN until a later read proves absence, including after a lost response.
            return Ok(false);
        }
        *self = Self::default();
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
                ))
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

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use alien_aws_clients::acm::{
        CertificateSummary, ImportCertificateResponse, ListCertificatesResponse, MockAcmApi,
    };

    use super::*;

    #[derive(Default)]
    struct World {
        certificates: Mutex<BTreeMap<String, Vec<Tag>>>,
        imports: AtomicUsize,
        deletes: AtomicUsize,
        hide_list: AtomicBool,
        deny_tags: AtomicBool,
        deny_delete: AtomicBool,
        lose_import: AtomicBool,
        lose_delete: AtomicBool,
    }

    fn denied() -> AlienError<ClientErrorData> {
        AlienError::new(ClientErrorData::RemoteAccessDenied {
            resource_type: "Certificate".to_string(),
            resource_name: "test".to_string(),
        })
    }

    fn missing(arn: &str) -> AlienError<ClientErrorData> {
        AlienError::new(ClientErrorData::RemoteResourceNotFound {
            resource_type: "Certificate".to_string(),
            resource_name: arn.to_string(),
        })
    }

    fn client(world: Arc<World>) -> MockAcmApi {
        let mut acm = MockAcmApi::new();
        let reads = world.clone();
        acm.expect_list_certificates().returning(move |request| {
            assert_eq!(request.includes.unwrap().key_types.unwrap().len(), 7);
            let certificates = if reads.hide_list.load(Ordering::SeqCst) {
                Vec::new()
            } else {
                reads
                    .certificates
                    .lock()
                    .unwrap()
                    .keys()
                    .map(|arn| CertificateSummary {
                        certificate_arn: Some(arn.clone()),
                        certificate_type: Some("IMPORTED".to_string()),
                        domain_name: None,
                        status: None,
                        key_algorithm: None,
                        in_use: None,
                        imported_at: None,
                    })
                    .collect()
            };
            Ok(ListCertificatesResponse {
                certificate_summary_list: certificates,
                next_token: None,
            })
        });
        let reads = world.clone();
        acm.expect_list_tags_for_certificate()
            .returning(move |arn| {
                if reads.deny_tags.load(Ordering::SeqCst) {
                    return Err(denied());
                }
                reads
                    .certificates
                    .lock()
                    .unwrap()
                    .get(arn)
                    .cloned()
                    .ok_or_else(|| missing(arn))
            });
        let imports = world.clone();
        acm.expect_import_certificate().returning(move |request| {
            let number = imports.imports.fetch_add(1, Ordering::SeqCst) + 1;
            let arn = format!("arn:aws:acm:us-east-1:111122223333:certificate/import-{number}");
            imports
                .certificates
                .lock()
                .unwrap()
                .insert(arn.clone(), request.tags.unwrap());
            if imports.lose_import.swap(false, Ordering::SeqCst) {
                return Err(denied());
            }
            Ok(ImportCertificateResponse {
                certificate_arn: arn,
            })
        });
        acm.expect_delete_certificate().returning(move |arn| {
            if world.deny_delete.load(Ordering::SeqCst) {
                return Err(denied());
            }
            world.deletes.fetch_add(1, Ordering::SeqCst);
            if world.certificates.lock().unwrap().remove(arn).is_none() {
                return Err(missing(arn));
            }
            if world.lose_delete.swap(false, Ordering::SeqCst) {
                return Err(denied());
            }
            Ok(())
        });
        acm
    }

    fn request() -> ImportCertificateRequest {
        ImportCertificateRequest::builder()
            .certificate("public-certificate".to_string())
            .private_key("never-persist-this-private-key".to_string())
            .tags(vec![Tag {
                key: "Owner".to_string(),
                value: "test-resource".to_string(),
            }])
            .build()
    }

    fn checkpoint(attempt: &AwsAcmImport) -> AwsAcmImport {
        let serialized = serde_json::to_string(attempt).unwrap();
        assert!(!serialized.contains("never-persist-this-private-key"));
        assert!(!serialized.contains("public-certificate"));
        serde_json::from_str(&serialized).unwrap()
    }

    fn assert_delete_failure_retained(error: &AlienError<ErrorData>) {
        assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
        for cause in std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
        {
            assert!(!matches!(cause.http_status_code, Some(401 | 403 | 404)));
            assert_ne!(cause.code, "REMOTE_ACCESS_DENIED");
            assert_ne!(cause.code, "REMOTE_RESOURCE_NOT_FOUND");
        }
    }

    #[tokio::test]
    async fn checkpoint_recovers_committed_import_and_delete_with_lost_responses() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        assert!(attempt
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_none());
        assert_eq!(world.imports.load(Ordering::SeqCst), 0);
        let intent = checkpoint(&attempt);
        world.lose_import.store(true, Ordering::SeqCst);
        attempt
            .step_import(&acm, request())
            .await
            .expect_err("committed import loses response");
        assert_eq!(world.certificates.lock().unwrap().len(), 1);
        let mut restored = intent;
        assert!(restored
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_none());
        let mut restored = checkpoint(&restored);
        assert!(restored
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_some());
        assert_eq!(world.imports.load(Ordering::SeqCst), 1);
        world.lose_delete.store(true, Ordering::SeqCst);
        let before_delete = checkpoint(&restored);
        let error = restored
            .step_delete(&acm)
            .await
            .expect_err("committed deletion loses response");
        assert_delete_failure_retained(&error);
        let mut restored = before_delete;
        assert!(restored.step_delete(&acm).await.unwrap());
        assert!(restored.token.is_none());
        assert!(world.certificates.lock().unwrap().is_empty());
        assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recorded_import_survives_list_visibility_lag_without_reimport() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        assert!(attempt
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_none());
        assert!(attempt
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_none());
        world.hide_list.store(true, Ordering::SeqCst);
        let mut restored = checkpoint(&attempt);
        assert_eq!(
            restored.step_import(&acm, request()).await.unwrap(),
            restored.selected_arn
        );
        assert_eq!(world.imports.load(Ordering::SeqCst), 1);
        assert!(!restored.step_delete(&acm).await.unwrap());
        assert!(restored.step_delete(&acm).await.unwrap());
    }

    #[tokio::test]
    async fn changed_certificate_requires_a_separate_owned_generation() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        attempt.step_import(&acm, request()).await.unwrap();
        let mut next = request();
        next.certificate = "new-certificate".to_string();
        let error = attempt.step_import(&acm, next).await.unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert_eq!(world.imports.load(Ordering::SeqCst), 0);
        assert!(attempt.step_delete(&acm).await.unwrap());
    }

    #[tokio::test]
    async fn foreign_ownership_is_never_adopted_or_deleted_even_with_a_matching_token() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        attempt.step_import(&acm, request()).await.unwrap();
        attempt.step_import(&acm, request()).await.unwrap();
        let selected = attempt.selected_arn.clone().unwrap();
        world
            .certificates
            .lock()
            .unwrap()
            .get_mut(&selected)
            .unwrap()
            .iter_mut()
            .find(|tag| tag.key == "Owner")
            .unwrap()
            .value = "foreign-resource".to_string();
        attempt
            .step_import(&acm, request())
            .await
            .expect_err("foreign matching token rejected");
        attempt
            .step_delete(&acm)
            .await
            .expect_err("foreign certificate retained");
        assert_eq!(world.imports.load(Ordering::SeqCst), 1);
        assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
        assert_eq!(world.certificates.lock().unwrap().len(), 1);
        assert!(attempt.token.is_some());
    }

    #[tokio::test]
    async fn denied_metadata_or_delete_keeps_cleanup_pending() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        attempt.step_import(&acm, request()).await.unwrap();
        attempt.step_import(&acm, request()).await.unwrap();
        world.deny_tags.store(true, Ordering::SeqCst);
        assert_delete_failure_retained(&attempt.step_delete(&acm).await.unwrap_err());
        assert!(attempt.token.is_some());
        world.deny_tags.store(false, Ordering::SeqCst);
        world.deny_delete.store(true, Ordering::SeqCst);
        assert_delete_failure_retained(&attempt.step_delete(&acm).await.unwrap_err());
        assert_eq!(world.certificates.lock().unwrap().len(), 1);
        world.deny_delete.store(false, Ordering::SeqCst);
        assert!(!attempt.step_delete(&acm).await.unwrap());
        assert!(attempt.step_delete(&acm).await.unwrap());
    }

    #[tokio::test]
    async fn all_imports_from_the_attempt_are_deleted_one_per_step() {
        let world = Arc::new(World::default());
        let acm = client(world.clone());
        let mut attempt = AwsAcmImport::default();
        attempt.step_import(&acm, request()).await.unwrap();
        attempt.step_import(&acm, request()).await.unwrap();
        let tags = world
            .certificates
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        world
            .certificates
            .lock()
            .unwrap()
            .insert("duplicate-committed-import".to_string(), tags);
        assert!(attempt
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_none());
        assert_eq!(attempt.certificate_arns.len(), 2);
        assert!(attempt
            .step_import(&acm, request())
            .await
            .unwrap()
            .is_some());
        assert!(!attempt.step_delete(&acm).await.unwrap());
        assert_eq!(world.certificates.lock().unwrap().len(), 1);
        let mut restored = checkpoint(&attempt);
        assert!(!restored.step_delete(&acm).await.unwrap());
        assert!(world.certificates.lock().unwrap().is_empty());
        assert!(restored.step_delete(&acm).await.unwrap());
        assert_eq!(world.deletes.load(Ordering::SeqCst), 2);
    }
}
