//! Base images pinned by digest.
//!
//! A signed release must build only from base images whose content is fixed
//! by the CLI that signs it, so that a moved tag or a substituted registry
//! can't change what the signature covers. The table maps each base image
//! reference this CLI uses (`<repository>:<tag>`) to its digest. The CLI
//! release workflow fills it at build time through
//! `ALIEN_BASE_IMAGE_DIGESTS` (the same JSON object); otherwise it comes from
//! `base-image-digests.json` next to this crate's manifest.
//!
//! Every build resolves its base images through the table, so a pinned
//! image is always built from its digest. A build that will be signed also
//! refuses an image the table doesn't pin.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

const CHECKED_IN: &str = include_str!("../base-image-digests.json");

fn table() -> &'static BTreeMap<String, String> {
    static TABLE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let source = option_env!("ALIEN_BASE_IMAGE_DIGESTS").unwrap_or(CHECKED_IN);
        parse(source).expect("the base image digest table compiled into this CLI is valid")
    })
}

fn parse(source: &str) -> std::result::Result<BTreeMap<String, String>, String> {
    let table: BTreeMap<String, String> =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    for (image, digest) in &table {
        if !is_sha256_digest(digest) {
            return Err(format!("{image}: '{digest}' is not a sha256 digest"));
        }
    }
    Ok(table)
}

fn is_sha256_digest(digest: &str) -> bool {
    digest.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// `<repository>@<digest>` for `image` from `table`, or why it can't be pinned.
fn pin_with(table: &BTreeMap<String, String>, image: &str) -> Result<String> {
    if image.contains('@') {
        // A digest reference still has to be one this CLI pins: otherwise any
        // image could be named by its digest.
        let pinned = table.iter().any(|(reference, digest)| {
            repository(reference)
                .is_some_and(|repository| image == format!("{repository}@{digest}"))
        });
        return if pinned {
            Ok(image.to_string())
        } else {
            Err(refused(image))
        };
    }
    let digest = table.get(image).ok_or_else(|| refused(image))?;
    let repository = repository(image).ok_or_else(|| refused(image))?;
    Ok(format!("{repository}@{digest}"))
}

/// The repository of `<repository>:<tag>`. A colon in the registry's port
/// isn't a tag separator.
fn repository(reference: &str) -> Option<&str> {
    let (repository, tag) = reference.rsplit_once(':')?;
    (!tag.contains('/')).then_some(repository)
}

fn refused(image: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BaseImageNotPinned {
        image: image.to_string(),
    })
}

/// `<repository>@<digest>` for `image`, from the table compiled into this CLI.
pub fn pin(image: &str) -> Result<String> {
    pin_with(table(), image)
}

/// The image a build uses for `image`: its pinned digest when this CLI
/// pins it; otherwise `image` itself, unless `required`.
pub fn resolve(image: &str, required: bool) -> Result<String> {
    resolve_with(table(), image, required)
}

fn resolve_with(table: &BTreeMap<String, String>, image: &str, required: bool) -> Result<String> {
    match pin_with(table, image) {
        Ok(pinned) => Ok(pinned),
        Err(_) if !required => Ok(image.to_string()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn table() -> BTreeMap<String, String> {
        parse(&format!(
            r#"{{"ghcr.io/acme/base:v1": "{DIGEST}", "localhost:5000/base:v2": "{DIGEST}"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_pinned_tag_resolves_to_its_digest() {
        assert_eq!(
            pin_with(&table(), "ghcr.io/acme/base:v1").unwrap(),
            format!("ghcr.io/acme/base@{DIGEST}")
        );
        assert_eq!(
            pin_with(&table(), "localhost:5000/base:v2").unwrap(),
            format!("localhost:5000/base@{DIGEST}")
        );
        assert_eq!(
            pin_with(&table(), &format!("ghcr.io/acme/base@{DIGEST}")).unwrap(),
            format!("ghcr.io/acme/base@{DIGEST}")
        );
    }

    #[test]
    fn a_tag_only_or_unknown_image_is_refused() {
        for image in [
            "ghcr.io/acme/base:latest",
            "ghcr.io/acme/base",
            "ghcr.io/attacker/base:v1",
            "localhost:5000/base",
        ] {
            let error = pin_with(&table(), image).unwrap_err();
            assert_eq!(error.code, "BASE_IMAGE_NOT_PINNED", "{image}: {error}");
        }
        let other = "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        assert!(pin_with(&table(), &format!("ghcr.io/acme/base@{other}")).is_err());
        assert!(pin_with(&table(), &format!("ghcr.io/attacker/base@{DIGEST}")).is_err());
    }

    #[test]
    fn a_pinned_image_is_always_built_from_its_digest_and_others_only_unsigned() {
        assert_eq!(
            resolve_with(&table(), "ghcr.io/acme/base:v1", false).unwrap(),
            format!("ghcr.io/acme/base@{DIGEST}")
        );
        assert_eq!(
            resolve_with(&table(), "ghcr.io/other/base:v1", false).unwrap(),
            "ghcr.io/other/base:v1"
        );
        let error = resolve_with(&table(), "ghcr.io/other/base:v1", true).unwrap_err();
        assert_eq!(error.code, "BASE_IMAGE_NOT_PINNED");
    }

    #[test]
    fn a_table_with_a_tag_for_a_digest_is_rejected() {
        assert!(parse(r#"{"ghcr.io/acme/base:v1": "v1"}"#).is_err());
        assert!(parse(CHECKED_IN).is_ok());
    }
}
