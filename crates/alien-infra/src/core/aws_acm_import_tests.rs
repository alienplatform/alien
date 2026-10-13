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
    certificate_in_use: AtomicBool,
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
        if world.certificate_in_use.load(Ordering::SeqCst) {
            return Err(AlienError::new(ClientErrorData::RemoteResourceConflict {
                message: "ResourceInUseException: listener association still draining".to_string(),
                resource_type: "Certificate".to_string(),
                resource_name: arn.to_string(),
            }));
        }
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
    for cause in std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref()) {
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

#[tokio::test]
async fn abandoned_lost_response_generation_is_retired_while_serving_certificate_is_retained() {
    let world = Arc::new(World::default());
    let acm = client(world.clone());
    let mut imports = AwsAcmImports::default();
    assert!(imports
        .step_import(&acm, request())
        .await
        .unwrap()
        .is_none());
    assert!(imports
        .step_import(&acm, request())
        .await
        .unwrap()
        .is_none());
    let serving = imports.step_import(&acm, request()).await.unwrap().unwrap();
    let mut renewal = request();
    renewal.certificate = "new-public-certificate".to_string();
    assert!(imports
        .step_import(&acm, renewal.clone())
        .await
        .unwrap()
        .is_none());
    let checkpoint = serde_json::to_string(&imports).unwrap();
    world.lose_import.store(true, Ordering::SeqCst);
    imports
        .step_import(&acm, renewal)
        .await
        .expect_err("renewal import committed without a response");
    let mut restored: AwsAcmImports = serde_json::from_str(&checkpoint).unwrap();
    // Desired configuration reverted before the replacement could be published.
    assert_eq!(
        restored.step_import(&acm, request()).await.unwrap(),
        Some(serving.clone())
    );
    assert!(!restored
        .step_retire(&acm, &[serving.clone()])
        .await
        .unwrap());
    assert_eq!(world.certificates.lock().unwrap().len(), 1);
    assert!(world.certificates.lock().unwrap().contains_key(&serving));
    assert!(!restored
        .step_retire(&acm, &[serving.clone()])
        .await
        .unwrap());
    assert!(restored
        .step_retire(&acm, &[serving.clone()])
        .await
        .unwrap());
    assert_eq!(world.imports.load(Ordering::SeqCst), 2);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
    assert!(!restored.step_retire(&acm, &[]).await.unwrap());
    assert!(!restored.step_retire(&acm, &[]).await.unwrap());
    assert!(restored.step_retire(&acm, &[]).await.unwrap());
    assert!(world.certificates.lock().unwrap().is_empty());
}

#[tokio::test]
async fn certificate_association_drain_keeps_ownership_until_deletion_is_confirmed() {
    let world = Arc::new(World::default());
    let acm = client(world.clone());
    let mut attempt = AwsAcmImport::default();
    attempt.step_import(&acm, request()).await.unwrap();
    attempt.step_import(&acm, request()).await.unwrap();
    world.certificate_in_use.store(true, Ordering::SeqCst);
    assert!(!attempt.step_delete(&acm).await.unwrap());
    assert_eq!(world.certificates.lock().unwrap().len(), 1);
    let mut restored = checkpoint(&attempt);
    world.certificate_in_use.store(false, Ordering::SeqCst);
    assert!(!restored.step_delete(&acm).await.unwrap());
    assert!(restored.step_delete(&acm).await.unwrap());
    assert!(world.certificates.lock().unwrap().is_empty());
}
