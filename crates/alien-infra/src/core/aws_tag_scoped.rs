//! AWS objects a runtime role may delete and read only while they carry the stack's tags, and
//! ACM certificates found again by the token their import was tagged with.

use std::future::Future;

use alien_aws_clients::acm::{find_imported_certificates_by_tag, AcmApi, Tag};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::AlienError;
use tracing::warn;

/// Tag carrying the token of the create call that made an object, recorded in controller state
/// before the call so a retry after a lost response finds that object and no other.
pub(crate) const CREATE_ATTEMPT_TAG: &str = "CreateAttempt";

pub(crate) fn is_remote_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

pub(crate) fn is_remote_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

/// How a delete of an object behind a tag-conditioned grant ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TagScopedDelete {
    Deleted,
    /// Confirmed gone: the delete or the read answered NotFound.
    Gone,
}

/// Interprets a delete of an object behind a tag-conditioned grant.
///
/// The role may delete such an object only while it carries the stack's tags. A gone object
/// has no tags, so AWS answers its delete with AccessDenied, not NotFound. A denied delete is
/// checked with `probe`, a read the permission sets also grant on objects without the stack's
/// tag, so a gone object answers it NotFound:
///
/// - NotFound: the object is gone.
/// - Success: the object is there and the denial is real. It is returned as not retryable, since
///   the same call with the same grants cannot succeed.
/// - AccessDenied: a role set up before that read was granted cannot tell. The delete's denial
///   is returned and the object is never treated as gone.
pub(crate) async fn delete_tag_scoped<D, P, F>(
    result: std::result::Result<D, AlienError<CloudClientErrorData>>,
    probe: F,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>>
where
    F: FnOnce() -> P,
    P: Future<Output = std::result::Result<(), AlienError<CloudClientErrorData>>>,
{
    let mut denial = match result {
        Ok(_) => return Ok(TagScopedDelete::Deleted),
        Err(error) if is_remote_not_found(&error) => return Ok(TagScopedDelete::Gone),
        Err(error) if is_remote_access_denied(&error) => error,
        Err(error) => return Err(error),
    };
    match probe().await {
        Ok(()) => {
            denial.retryable = false;
            Err(denial)
        }
        Err(probe_error) if is_remote_not_found(&probe_error) => Ok(TagScopedDelete::Gone),
        Err(probe_error) if is_remote_access_denied(&probe_error) => {
            warn!(
                error = %denial,
                "A denied delete could not be checked: the read that tells a gone object from a real denial is denied too. Rerunning setup grants it"
            );
            Err(denial)
        }
        Err(probe_error) => Err(probe_error),
    }
}

/// Deletes an ACM certificate this resource imported. DescribeCertificate tells a certificate
/// that is gone from a real denial.
pub(crate) async fn delete_imported_certificate(
    acm: &dyn AcmApi,
    certificate_arn: &str,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>> {
    delete_tag_scoped(acm.delete_certificate(certificate_arn).await, || async {
        acm.describe_certificate(certificate_arn).await.map(|_| ())
    })
    .await
}

/// The tags of a certificate import: `tags` plus the import token.
pub(crate) fn with_import_token(mut tags: Vec<Tag>, token: &str) -> Vec<Tag> {
    tags.retain(|tag| tag.key != CREATE_ATTEMPT_TAG);
    tags.push(Tag {
        key: CREATE_ATTEMPT_TAG.to_string(),
        value: token.to_string(),
    });
    tags
}

/// The certificates imported under `token`, sorted.
///
/// `None` when the role may not list certificates: a role installed before ListCertificates was
/// part of its permission set. The caller then has only the ARNs it recorded.
pub(crate) async fn certificates_imported_with_token(
    acm: &dyn AcmApi,
    token: &str,
) -> std::result::Result<Option<Vec<String>>, AlienError<CloudClientErrorData>> {
    match find_imported_certificates_by_tag(acm, CREATE_ATTEMPT_TAG, token).await {
        Ok(mut found) => {
            found.sort();
            Ok(Some(found))
        }
        Err(error) if is_remote_access_denied(&error) => {
            warn!(
                token = %token,
                "ACM ListCertificates is denied; rerun setup to grant it. A certificate whose import response was lost cannot be found until then"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "Lambda function".to_string(),
            resource_name: "stack-api".to_string(),
        })
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Lambda function".to_string(),
            resource_name: "stack-api".to_string(),
        })
    }

    /// A role set up before the tag-absent reads were granted: the read of a gone object is
    /// denied like the delete, so nothing tells it from a real denial. The denial stands and
    /// stays retryable, never gone.
    #[tokio::test]
    async fn denied_delete_whose_read_is_denied_too_keeps_the_denial() {
        let error = delete_tag_scoped(Err::<(), _>(denied()), || async { Err(denied()) })
            .await
            .expect_err("the denial stands");
        assert!(is_remote_access_denied(&error), "{error:?}");
        assert!(error.retryable);
    }

    /// A readable object is there: its denied delete is real.
    #[tokio::test]
    async fn denied_delete_of_a_readable_object_is_a_permanent_error() {
        let error = delete_tag_scoped(Err::<(), _>(denied()), || async { Ok(()) })
            .await
            .expect_err("the denial is real");
        assert!(is_remote_access_denied(&error));
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn not_found_from_the_delete_or_the_read_is_gone() {
        let from_delete = delete_tag_scoped(Err::<(), _>(not_found()), || async {
            panic!("no read after NotFound")
        })
        .await;
        assert_eq!(from_delete.unwrap(), TagScopedDelete::Gone);
        // A gone object answers the tag-absent read NotFound.
        let from_read =
            delete_tag_scoped(Err::<(), _>(denied()), || async { Err(not_found()) }).await;
        assert_eq!(from_read.unwrap(), TagScopedDelete::Gone);
    }
}
