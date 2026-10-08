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
    /// Already gone, or no longer carrying this resource's tags.
    Gone,
}

/// Interprets a delete of an object behind a tag-conditioned grant.
///
/// The role may delete and read such an object only while it carries the stack's tags. An
/// object that is gone has no tags, so AWS answers a delete of it (deleted out of band, or by an
/// earlier attempt whose response was lost) with AccessDenied, not NotFound. A denied delete is
/// checked with `probe`, a read under the same grant: when the read is denied or not found too,
/// nothing this resource may delete is left; when it succeeds, the object is there and the
/// denial is real.
pub(crate) async fn delete_tag_scoped<D, P, F>(
    result: std::result::Result<D, AlienError<CloudClientErrorData>>,
    probe: F,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>>
where
    F: FnOnce() -> P,
    P: Future<Output = std::result::Result<(), AlienError<CloudClientErrorData>>>,
{
    match result {
        Ok(_) => Ok(TagScopedDelete::Deleted),
        Err(error) if is_remote_not_found(&error) => Ok(TagScopedDelete::Gone),
        Err(error) if is_remote_access_denied(&error) => match probe().await {
            Ok(()) => Err(error),
            Err(probe_error)
                if is_remote_not_found(&probe_error) || is_remote_access_denied(&probe_error) =>
            {
                Ok(TagScopedDelete::Gone)
            }
            Err(probe_error) => Err(probe_error),
        },
        Err(error) => Err(error),
    }
}

/// Deletes an ACM certificate this resource imported. DescribeCertificate, granted under the
/// same tag condition, tells a certificate that is gone from a real denial.
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
