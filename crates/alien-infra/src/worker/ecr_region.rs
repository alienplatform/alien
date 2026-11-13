//! Lambda pulls its image from ECR in its own region. When the image lives in
//! another region, the copy there comes from ECR replication. Before Lambda is
//! created or updated, check that the exact image is present in the Lambda's
//! region, and fail with a clear error if it is not.

use alien_aws_clients::ecr::{BatchGetImageRequest, EcrApi, ImageIdentifier};
use alien_error::{AlienError, Context, ContextError};

use crate::error::{ErrorData, Result};

/// The parts of an ECR image reference that the region check needs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EcrImageRef {
    pub account_id: String,
    pub region: String,
    pub repository: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
}

/// Parses `{account}.dkr.ecr.{region}.amazonaws.com/{repo}[:tag][@sha256:...]`.
pub(crate) fn parse_ecr_image_ref(image_uri: &str) -> Option<EcrImageRef> {
    let (host, rest) = image_uri.split_once('/')?;
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 6 || parts[1] != "dkr" || parts[2] != "ecr" || parts[4] != "amazonaws" {
        return None;
    }
    let (name, digest) = match rest.split_once('@') {
        Some((name, digest)) => (name, Some(digest.to_string())),
        None => (rest, None),
    };
    let (repository, tag) = match name.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo, Some(tag.to_string())),
        _ => (name, None),
    };
    if repository.is_empty() || (tag.is_none() && digest.is_none()) {
        return None;
    }
    Some(EcrImageRef {
        account_id: parts[0].to_string(),
        region: parts[3].to_string(),
        repository: repository.to_string(),
        tag,
        digest,
    })
}

fn not_replicated(message: String, resource_id: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message,
        resource_id: Some(resource_id.to_string()),
    })
}

fn identifier(image: &EcrImageRef) -> ImageIdentifier {
    // A digest pins the exact image; a tag is only used when there is no digest.
    match &image.digest {
        Some(digest) => ImageIdentifier {
            image_tag: None,
            image_digest: Some(digest.clone()),
        },
        None => ImageIdentifier {
            image_tag: image.tag.clone(),
            image_digest: None,
        },
    }
}

/// AWS names a service control policy in the message of an explicit deny.
fn is_region_policy_denial(lowercase_detail: &str) -> bool {
    lowercase_detail.contains("service control policy")
        || lowercase_detail.contains("explicit deny in a service control")
}

async fn digest_in_region(
    client: &dyn EcrApi,
    image: &EcrImageRef,
    region: &str,
    resource_id: &str,
) -> Result<Option<String>> {
    let response = match client
        .batch_get_image(BatchGetImageRequest {
            repository_name: image.repository.clone(),
            image_ids: vec![identifier(image)],
            registry_id: Some(image.account_id.clone()),
            accepted_media_types: None,
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let detail = format!("{err:?}").to_ascii_lowercase();
            let message = if is_region_policy_denial(&detail) {
                format!(
                    "An organization policy (service control policy) denies access to {region}, so \
                     the image repository '{}' cannot be read there. Allow {region} in the \
                     aws:RequestedRegion condition of the policy, or remove {region} from the \
                     deployment regions.",
                    image.repository
                )
            } else {
                format!(
                    "Cannot read image repository '{}' in {region} to check that the image was replicated",
                    image.repository
                )
            };
            return Err(err.context(ErrorData::ResourceConfigInvalid {
                message,
                resource_id: Some(resource_id.to_string()),
            }));
        }
    };
    Ok(response
        .images
        .into_iter()
        .next()
        .and_then(|found| found.image_id.image_digest))
}

/// Fails unless the exact image `source` is present in `target_region`.
/// `source_client` reads the source region, `target_client` the target region.
/// Does nothing when the image is not in ECR or already in the target region.
pub(crate) async fn ensure_image_in_region(
    source: &str,
    target_region: &str,
    source_client: &dyn EcrApi,
    target_client: &dyn EcrApi,
    resource_id: &str,
) -> Result<()> {
    let Some(image) = parse_ecr_image_ref(source) else {
        return Ok(());
    };
    if image.region == target_region {
        return Ok(());
    }
    let wanted = match &image.digest {
        Some(digest) => digest.clone(),
        None => digest_in_region(source_client, &image, &image.region, resource_id)
            .await?
            .ok_or_else(|| {
                not_replicated(
                    format!(
                        "Image '{}:{}' was not found in {}",
                        image.repository,
                        image.tag.as_deref().unwrap_or_default(),
                        image.region
                    ),
                    resource_id,
                )
            })?,
    };
    let present = digest_in_region(target_client, &image, target_region, resource_id).await?;
    if present.as_deref() == Some(wanted.as_str()) {
        return Ok(());
    }
    Err(not_replicated(
        format!(
            "Image {wanted} of repository '{}' is not in {target_region}, so Lambda cannot pull it. \
             The instance must replicate its registry to {target_region} (declare {target_region} \
             as a deployment region of the instance). Images pushed before the replication rule \
             existed are copied by releasing again.",
            image.repository
        ),
        resource_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::ecr::{BatchGetImageResponse, EcrImage, MockEcrApi};

    const SRC: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/app";
    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn found(digest: &str) -> BatchGetImageResponse {
        BatchGetImageResponse {
            images: vec![EcrImage {
                registry_id: "123456789012".into(),
                repository_name: "app".into(),
                image_id: ImageIdentifier {
                    image_tag: None,
                    image_digest: Some(digest.into()),
                },
                image_manifest: "{}".into(),
            }],
            failures: vec![],
        }
    }

    fn missing() -> BatchGetImageResponse {
        BatchGetImageResponse {
            images: vec![],
            failures: vec![],
        }
    }

    #[test]
    fn parses_tags_digests_and_nested_repositories() {
        let tagged = parse_ecr_image_ref(&format!("{SRC}:v1")).unwrap();
        assert_eq!(tagged.tag.as_deref(), Some("v1"));
        assert_eq!(tagged.region, "us-east-1");
        let digested = parse_ecr_image_ref(&format!("{SRC}@{DIGEST}")).unwrap();
        assert_eq!(digested.digest.as_deref(), Some(DIGEST));
        let nested =
            parse_ecr_image_ref("1.dkr.ecr.eu-west-1.amazonaws.com/team/app:latest").unwrap();
        assert_eq!(nested.repository, "team/app");
        assert!(parse_ecr_image_ref("docker.io/library/app:1").is_none());
        assert!(parse_ecr_image_ref(SRC).is_none());
    }

    #[tokio::test]
    async fn missing_digest_in_target_region_fails_with_a_clear_error() {
        let source = MockEcrApi::new();
        let mut target = MockEcrApi::new();
        target
            .expect_batch_get_image()
            .times(1)
            .returning(|_| Ok(missing()));
        let err = ensure_image_in_region(
            &format!("{SRC}@{DIGEST}"),
            "us-east-2",
            &source,
            &target,
            "worker",
        )
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("us-east-2") && text.contains(DIGEST), "{text}");
    }

    #[test]
    fn a_service_control_policy_denial_is_recognised() {
        assert!(is_region_policy_denial(
            "user is not authorized ... with an explicit deny in a service control policy"
        ));
        assert!(!is_region_policy_denial("accessdeniedexception: no identity-based policy"));
    }

    #[tokio::test]
    async fn a_region_denied_by_policy_names_the_region_and_the_policy() {
        let source = MockEcrApi::new();
        let mut target = MockEcrApi::new();
        target.expect_batch_get_image().returning(|_| {
            Err(alien_client_core::Error::new(
                alien_client_core::ErrorData::GenericError {
                    message: "not authorized to perform ecr:BatchGetImage with an explicit deny in a service control policy".into(),
                },
            ))
        });
        let err = ensure_image_in_region(
            &format!("{SRC}@{DIGEST}"),
            "us-east-2",
            &source,
            &target,
            "worker",
        )
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("us-east-2") && text.contains("service control policy"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_different_digest_behind_the_same_tag_is_refused() {
        let mut source = MockEcrApi::new();
        source.expect_batch_get_image().returning(|_| Ok(found(DIGEST)));
        let mut target = MockEcrApi::new();
        target.expect_batch_get_image().returning(|_| {
            Ok(found(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            ))
        });
        assert!(ensure_image_in_region(
            &format!("{SRC}:v1"),
            "us-east-2",
            &source,
            &target,
            "worker"
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn the_exact_digest_in_the_target_region_passes() {
        let source = MockEcrApi::new();
        let mut target = MockEcrApi::new();
        target.expect_batch_get_image().returning(|_| Ok(found(DIGEST)));
        ensure_image_in_region(
            &format!("{SRC}@{DIGEST}"),
            "us-east-2",
            &source,
            &target,
            "worker",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn same_region_and_non_ecr_images_make_no_calls() {
        let source = MockEcrApi::new();
        let target = MockEcrApi::new();
        ensure_image_in_region(&format!("{SRC}:v1"), "us-east-1", &source, &target, "w")
            .await
            .unwrap();
        ensure_image_in_region("ghcr.io/a/b:1", "us-east-2", &source, &target, "w")
            .await
            .unwrap();
    }
}
