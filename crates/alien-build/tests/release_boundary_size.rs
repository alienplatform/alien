//! `alien build`, which `alien release` runs for each platform, rejects an AWS stack whose setup
//! permissions boundary cannot fit IAM's managed-policy limit before building anything.

use alien_build::{
    build_stack,
    settings::{BuildSettings, PlatformBuildSettings},
};
use alien_core::{PermissionProfile, PermissionSetReference, Stack};
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn aws_build_fails_when_the_setup_boundary_does_not_fit() {
    // Reads of many buckets outside the stack's prefix, each of which the boundary must list.
    let references = (0..60).map(|index| {
        PermissionSetReference::Inline(
            serde_json::from_value(json!({
                "id": "custom/partner-read",
                "description": "Read a partner's exports",
                "platforms": {"aws": [{
                    "grant": {"actions": ["s3:GetObject"]},
                    "binding": {"stack": {"resources": [
                        format!("arn:aws:s3:::partner-exports-{index}/*")
                    ]}}
                }]}
            }))
            .expect("inline permission set"),
        )
    });
    let stack = Stack::new("partner-sync".to_string())
        .permission("application", PermissionProfile::new().global(references))
        .build();
    let output = tempdir().expect("temp output dir");
    let settings = BuildSettings {
        output_directory: output.path().to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Aws {
            managing_account_id: None,
        },
        targets: None,
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
        rebuild: false,
        pull_base_images: false,
    };

    let error = build_stack(stack, &settings)
        .await
        .expect_err("the boundary does not fit");

    assert_eq!(error.code, "STACK_PROCESSOR_FAILED", "{error}");
    let preflights = error.source.as_ref().expect("preflight failure");
    assert_eq!(preflights.code, "VALIDATION_FAILED");
    let boundary = preflights.source.as_ref().expect("boundary failure");
    assert_eq!(boundary.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
    assert!(
        boundary.message.contains(
            "Largest contributors: profile 'application' stack-wide (custom/partner-read): "
        ),
        "{}",
        boundary.message
    );
    // Nothing was built or saved.
    assert!(!output.path().join("build").exists());
}
