//! Validates `corsUploadOrigins` against what each storage backend's CORS can express.
//!
//! A browser compares its `Origin` header (`scheme://host[:port]`, no path, no trailing
//! slash) with each allowed origin, so anything else never matches and uploads fail with
//! a CORS error that names no cause. Without this check such a value is first noticed in
//! the browser, or rejected by the cloud while the deployment configures the bucket.
//!
//! - **AWS S3**: an allowed origin may hold one `*` wildcard, such as
//!   `https://*.example.com`:
//!   <https://docs.aws.amazon.com/AmazonS3/latest/API/API_CORSRule.html>.
//! - **GCP Cloud Storage**: an origin is `*` or matched exactly; there are no partial
//!   wildcards: <https://cloud.google.com/storage/docs/cross-origin>.
//! - **Azure Blob Storage**: an origin is `*` or matched exactly, at most 256 characters,
//!   and the service stores origins as one comma-separated list:
//!   <https://learn.microsoft.com/en-us/rest/api/storageservices/cross-origin-resource-sharing--cors--support-for-the-azure-storage-services>.
//!   The 2 KiB account-wide limit depends on every container in the account, so setup and
//!   the Terraform precondition check it.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, Storage};

const AZURE_MAX_ORIGIN_CHARS: usize = 256;

pub struct StorageCorsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for StorageCorsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("STORAGE_CORS_INVALID")
    }

    fn description(&self) -> &'static str {
        "Storage CORS upload origins are valid for the target platform"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack.resources.values().any(|entry| {
                entry
                    .config
                    .downcast_ref::<Storage>()
                    .is_some_and(|storage| !storage.cors_upload_origins.is_empty())
            })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        for (id, entry) in &stack.resources {
            let Some(storage) = entry.config.downcast_ref::<Storage>() else {
                continue;
            };
            for (index, origin) in storage.cors_upload_origins.iter().enumerate() {
                if let Some(error) = origin_error(origin, platform) {
                    result.add_error(format!(
                        "Storage '{id}': corsUploadOrigins[{index}] '{origin}' {error}"
                    ));
                }
            }
        }
        Ok(result)
    }
}

fn origin_error(origin: &str, platform: Platform) -> Option<String> {
    if origin == "*" {
        return None;
    }
    if origin
        .chars()
        .any(|c| c == ',' || c.is_whitespace() || c.is_control())
    {
        return Some("contains a comma, whitespace or a control character".to_string());
    }
    let Some(authority) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return Some("must be '*' or start with https:// or http://".to_string());
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return Some(
            "must be scheme://host[:port] with no path, trailing slash or user name, as browsers send it"
                .to_string(),
        );
    }
    // A bracketed IPv6 host without a port ends in `]`.
    if let Some((_, port)) = authority
        .rsplit_once(':')
        .filter(|_| !authority.ends_with(']'))
    {
        if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return Some("has a port that is not a number".to_string());
        }
    }
    if authority.chars().any(|c| c.is_ascii_uppercase()) {
        return Some("has uppercase letters; browsers send the host in lowercase".to_string());
    }
    let wildcards = authority.matches('*').count();
    match platform {
        Platform::Aws if wildcards > 1 => {
            Some("has more than one '*'; AWS S3 allows one wildcard per origin".to_string())
        }
        Platform::Gcp if wildcards > 0 => Some(
            "has a partial wildcard; GCP Cloud Storage only matches '*' or an exact origin"
                .to_string(),
        ),
        Platform::Azure if wildcards > 0 => Some(
            "has a partial wildcard; Azure Blob Storage only matches '*' or an exact origin"
                .to_string(),
        ),
        Platform::Azure if origin.chars().count() > AZURE_MAX_ORIGIN_CHARS => Some(format!(
            "is longer than {AZURE_MAX_ORIGIN_CHARS} characters, the Azure Blob Storage limit"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::ResourceLifecycle;

    fn stack(origins: &[&str]) -> Stack {
        Stack::new("uploads".to_string())
            .add(
                Storage::new("st".to_string())
                    .cors_upload_origins(origins.iter().map(|o| o.to_string()).collect())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    async fn errors(origins: &[&str], platform: Platform) -> Vec<String> {
        let stack = stack(origins);
        assert!(StorageCorsCheck.should_run(&stack, platform));
        StorageCorsCheck
            .check(&stack, platform)
            .await
            .expect("the check runs")
            .errors
    }

    #[tokio::test]
    async fn exact_origins_and_star_pass_on_every_cloud() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            assert_eq!(
                errors(
                    &[
                        "*",
                        "https://app.example.com",
                        "http://localhost:5173",
                        "http://[::1]:8080"
                    ],
                    platform
                )
                .await,
                Vec::<String>::new(),
                "{platform:?}"
            );
        }
    }

    #[tokio::test]
    async fn origins_a_browser_never_sends_are_refused() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            assert_eq!(
                errors(
                    &[
                        "https://app.example.com/",
                        "https://app.example.com/upload",
                        "app.example.com",
                        "https://a.example.com,https://b.example.com",
                        "",
                        "https://user@app.example.com",
                        "https://app.example.com:",
                        "https://app.example.com:http",
                        "https://App.Example.com",
                    ],
                    platform
                )
                .await,
                vec![
                    "Storage 'st': corsUploadOrigins[0] 'https://app.example.com/' must be scheme://host[:port] with no path, trailing slash or user name, as browsers send it",
                    "Storage 'st': corsUploadOrigins[1] 'https://app.example.com/upload' must be scheme://host[:port] with no path, trailing slash or user name, as browsers send it",
                    "Storage 'st': corsUploadOrigins[2] 'app.example.com' must be '*' or start with https:// or http://",
                    "Storage 'st': corsUploadOrigins[3] 'https://a.example.com,https://b.example.com' contains a comma, whitespace or a control character",
                    "Storage 'st': corsUploadOrigins[4] '' must be '*' or start with https:// or http://",
                    "Storage 'st': corsUploadOrigins[5] 'https://user@app.example.com' must be scheme://host[:port] with no path, trailing slash or user name, as browsers send it",
                    "Storage 'st': corsUploadOrigins[6] 'https://app.example.com:' has a port that is not a number",
                    "Storage 'st': corsUploadOrigins[7] 'https://app.example.com:http' has a port that is not a number",
                    "Storage 'st': corsUploadOrigins[8] 'https://App.Example.com' has uppercase letters; browsers send the host in lowercase",
                ],
                "{platform:?}"
            );
        }
    }

    #[tokio::test]
    async fn partial_wildcards_pass_only_where_the_backend_matches_them() {
        let origins = ["https://*.example.com"];
        assert_eq!(errors(&origins, Platform::Aws).await, Vec::<String>::new());
        assert_eq!(
            errors(&origins, Platform::Gcp).await,
            vec!["Storage 'st': corsUploadOrigins[0] 'https://*.example.com' has a partial wildcard; GCP Cloud Storage only matches '*' or an exact origin"]
        );
        assert_eq!(
            errors(&origins, Platform::Azure).await,
            vec!["Storage 'st': corsUploadOrigins[0] 'https://*.example.com' has a partial wildcard; Azure Blob Storage only matches '*' or an exact origin"]
        );
        assert_eq!(
            errors(&["https://*.*.example.com"], Platform::Aws).await,
            vec!["Storage 'st': corsUploadOrigins[0] 'https://*.*.example.com' has more than one '*'; AWS S3 allows one wildcard per origin"]
        );
    }

    #[tokio::test]
    async fn azure_refuses_origins_over_its_length_limit() {
        let fits = format!("https://{}", "a".repeat(AZURE_MAX_ORIGIN_CHARS - 8));
        let too_long = format!("{fits}b");
        assert_eq!(
            errors(&[&fits], Platform::Azure).await,
            Vec::<String>::new()
        );
        assert_eq!(
            errors(&[&too_long], Platform::Aws).await,
            Vec::<String>::new()
        );
        assert_eq!(
            errors(&[&too_long], Platform::Azure).await,
            vec![format!("Storage 'st': corsUploadOrigins[0] '{too_long}' is longer than 256 characters, the Azure Blob Storage limit")]
        );
    }

    /// `alien build` and setup template rendering both run the check before any cloud call.
    #[tokio::test]
    async fn build_and_setup_refuse_an_invalid_upload_origin() {
        let stack = stack(&["https://*.example.com"]);
        let runner = PreflightRunner::new();
        for (entry_point, outcome) in [
            (
                "build",
                runner
                    .run_build_time_preflights(&stack, Platform::Gcp)
                    .await
                    .map(|_| ()),
            ),
            (
                "template",
                runner
                    .run_template_preflights(&stack, Platform::Gcp)
                    .await
                    .map(|_| ()),
            ),
        ] {
            let error = outcome.expect_err(entry_point);
            let Some(ErrorData::ValidationFailed { results, .. }) = &error.error else {
                panic!("{entry_point}: expected ValidationFailed, got {error:?}");
            };
            let failed: Vec<_> = results.iter().filter(|result| !result.success).collect();
            assert_eq!(failed.len(), 1, "{entry_point}: {failed:?}");
            assert_eq!(failed[0].code.as_deref(), Some("STORAGE_CORS_INVALID"));
        }
    }

    #[test]
    fn storage_without_upload_origins_is_not_checked() {
        let without_origins = stack(&[]);
        assert!(!StorageCorsCheck.should_run(&without_origins, Platform::Aws));
        let with_origins = stack(&["https://*.*.x"]);
        for platform in [Platform::Kubernetes, Platform::Local, Platform::Machines] {
            assert!(!StorageCorsCheck.should_run(&with_origins, platform));
        }
    }
}
