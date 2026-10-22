//! AWS objects a runtime role may delete and read only while they carry the stack's tags, and
//! ACM certificates found again by the token their import was tagged with.

use std::future::Future;

use alien_aws_clients::acm::{find_imported_certificates_by_tag, AcmApi, Tag};
use alien_aws_clients::{resourcegroupstagging::GetResourcesRequest, AwsClientConfig};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_STACK_TAG_KEY};
use alien_error::{AlienError, Context};
use tracing::warn;

use crate::core::PlatformServiceProvider;
use crate::error::ErrorData;

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
    /// Confirmed gone: NotFound, or no longer listed with this stack's tags by a lookup that is
    /// not tag-scoped.
    Gone,
}

/// Interprets a delete of an object behind a tag-conditioned grant.
///
/// The role may delete and read such an object only while it carries the stack's tags. An
/// object that is gone has no tags, so AWS answers a delete of it (deleted out of band, or by an
/// earlier attempt whose response was lost) with AccessDenied, not NotFound. A denied delete is
/// checked with `probe`, a read under the same grant: when it succeeds, the object is there and
/// the denial is real. When the read is denied too, `exists` decides with a lookup that is not
/// tag-scoped and answers `None` when it cannot tell. Only a confirmed absence counts as gone:
/// when the lookup cannot tell, for example on a role set up before it was granted, the
/// delete's denial is returned.
///
/// A denial of an object confirmed to exist is returned as not retryable: the same call with
/// the same grants cannot succeed.
pub(crate) async fn delete_tag_scoped<D, P, F, E, X>(
    result: std::result::Result<D, AlienError<CloudClientErrorData>>,
    probe: F,
    exists: X,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>>
where
    F: FnOnce() -> P,
    P: Future<Output = std::result::Result<(), AlienError<CloudClientErrorData>>>,
    X: FnOnce() -> E,
    E: Future<Output = Option<bool>>,
{
    let denial = match result {
        Ok(_) => return Ok(TagScopedDelete::Deleted),
        Err(error) if is_remote_not_found(&error) => return Ok(TagScopedDelete::Gone),
        Err(error) if is_remote_access_denied(&error) => error,
        Err(error) => return Err(error),
    };
    match probe().await {
        Ok(()) => return Err(confirmed_denial(denial)),
        Err(probe_error) if is_remote_not_found(&probe_error) => return Ok(TagScopedDelete::Gone),
        Err(probe_error) if is_remote_access_denied(&probe_error) => {}
        Err(probe_error) => return Err(probe_error),
    }
    match exists().await {
        Some(false) => Ok(TagScopedDelete::Gone),
        Some(true) => Err(confirmed_denial(denial)),
        None => Err(denial),
    }
}

fn confirmed_denial(
    mut denial: AlienError<CloudClientErrorData>,
) -> AlienError<CloudClientErrorData> {
    denial.retryable = false;
    denial
}

/// Whether `arn` still exists and carries this stack's tags (`stack_prefix` and runtime
/// ownership), the condition of the grant that was denied. Looked up through the Resource Groups
/// Tagging API, which is not tag-scoped. An object it no longer lists is gone; one that lost
/// these tags is no longer this stack's to delete. `None` when the lookup fails, for example on a
/// role set up before `tag:GetResources` was part of its permission set.
pub(crate) async fn owned_resource_exists(
    service_provider: &dyn PlatformServiceProvider,
    config: &AwsClientConfig,
    stack_prefix: &str,
    arn: String,
) -> Option<bool> {
    let lookup = async {
        let tagging = service_provider
            .get_aws_resource_groups_tagging_client(config)
            .await?;
        let response = tagging
            .get_resources(
                GetResourcesRequest::builder()
                    .resource_arn_list(vec![arn.clone()])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to look up '{arn}' through the tagging API"),
                resource_id: None,
            })?;
        let has_tag = |tags: &[alien_aws_clients::resourcegroupstagging::Tag], key: &str, value: &str| {
            tags.iter().any(|tag| tag.key == key && tag.value == value)
        };
        Ok::<_, AlienError<ErrorData>>(response.resource_tag_mapping_list.iter().any(|mapping| {
            mapping.resource_arn == arn
                && has_tag(&mapping.tags, ALIEN_STACK_TAG_KEY, stack_prefix)
                && has_tag(&mapping.tags, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE)
        }))
    };
    match lookup.await {
        Ok(exists) => Some(exists),
        Err(error) => {
            warn!(
                arn = %arn,
                error = %error,
                "Could not confirm whether a denied object still exists; keeping the denial. Rerunning setup grants tag:GetResources"
            );
            None
        }
    }
}

/// Deletes an ACM certificate this resource imported. DescribeCertificate, granted under the
/// same tag condition, tells a certificate that is gone from a real denial; when it is denied
/// too, the tagging lookup decides.
pub(crate) async fn delete_imported_certificate(
    acm: &dyn AcmApi,
    service_provider: &dyn PlatformServiceProvider,
    config: &AwsClientConfig,
    stack_prefix: &str,
    certificate_arn: &str,
) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>> {
    delete_tag_scoped(
        acm.delete_certificate(certificate_arn).await,
        || async { acm.describe_certificate(certificate_arn).await.map(|_| ()) },
        || {
            owned_resource_exists(
                service_provider,
                config,
                stack_prefix,
                certificate_arn.to_string(),
            )
        },
    )
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

    async fn double_denial(
        exists: Option<bool>,
    ) -> std::result::Result<TagScopedDelete, AlienError<CloudClientErrorData>> {
        delete_tag_scoped(
            Err::<(), _>(denied()),
            || async { Err(denied()) },
            || async move { exists },
        )
        .await
    }

    /// Deleted out of band, or by an attempt whose response was lost: the lookup no longer
    /// lists it, so it is gone.
    #[tokio::test]
    async fn double_denial_of_an_object_the_lookup_no_longer_lists_is_gone() {
        assert_eq!(double_denial(Some(false)).await.unwrap(), TagScopedDelete::Gone);
    }

    /// The object still exists, so the denial is real and retrying cannot help.
    #[tokio::test]
    async fn double_denial_of_an_object_that_still_exists_is_a_permanent_error() {
        let error = double_denial(Some(true)).await.expect_err("the denial is real");
        assert!(is_remote_access_denied(&error), "{error:?}");
        assert!(!error.retryable);
    }

    /// A role set up before the lookup was granted cannot tell: the denial stands and stays
    /// retryable, never gone.
    #[tokio::test]
    async fn double_denial_the_lookup_cannot_decide_keeps_the_denial() {
        let error = double_denial(None).await.expect_err("the denial stands");
        assert!(is_remote_access_denied(&error), "{error:?}");
        assert!(error.retryable);
    }

    /// A readable object is there: its denied delete is real, and the lookup is not needed.
    #[tokio::test]
    async fn denied_delete_of_a_readable_object_is_a_permanent_error() {
        let error = delete_tag_scoped(
            Err::<(), _>(denied()),
            || async { Ok(()) },
            || async { panic!("a readable object needs no lookup") },
        )
        .await
        .expect_err("the denial is real");
        assert!(is_remote_access_denied(&error));
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn not_found_from_the_delete_or_the_read_is_gone() {
        let from_delete = delete_tag_scoped(
            Err::<(), _>(not_found()),
            || async { panic!("no read after NotFound") },
            || async { panic!("no lookup after NotFound") },
        )
        .await;
        assert_eq!(from_delete.unwrap(), TagScopedDelete::Gone);
        let from_read = delete_tag_scoped(
            Err::<(), _>(denied()),
            || async { Err(not_found()) },
            || async { panic!("no lookup after NotFound") },
        )
        .await;
        assert_eq!(from_read.unwrap(), TagScopedDelete::Gone);
    }
}
