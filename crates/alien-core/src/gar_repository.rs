//! Project repository names shared by GAR provisioning and OCI routing.

/// Derive a GAR resource name from a lowercase project ID without changing its identity.
/// The original project ID remains in the image path for OCI routing.
pub fn project_repository(prefix: &str, project_id: &str) -> Option<String> {
    let suffix = project_id
        .strip_prefix("prj_")
        .or_else(|| (project_id == "default").then_some(project_id))?;
    if suffix.is_empty()
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return None;
    }
    let name = if project_id == "default" {
        "default".to_string()
    } else {
        format!("prj-{suffix}")
    };
    let repository = format!("{prefix}-{name}");
    (repository.len() <= 63
        && repository.starts_with(|c: char| c.is_ascii_lowercase())
        && repository
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(repository)
}

/// The Artifact Registry repository that holds a GCP sandbox's image inside the deployment's own
/// project. It is dedicated to that one sandbox: never the stack's registry (which has a push
/// service account and workload bindings) and never the manager's. Setup, the controller and the
/// verifier all derive it from here, so they cannot disagree. `None` when the name would not be a
/// valid repository id.
pub fn sandbox_image_repository(prefix: &str, sandbox_id: &str) -> Option<String> {
    let repository = format!("{prefix}-{sandbox_id}-img");
    (repository.len() <= 63
        && repository.starts_with(|c: char| c.is_ascii_lowercase())
        && repository
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(repository)
}

/// What the manager's identity may do in a sandbox's image repository: read it, upload and tag. No
/// tag update and no delete of any kind (tags are immutable and made from the digest), so a compromised manager cannot remove a version a live template uses;
/// untagged versions are cleaned by the repository's own policy. One list for the Terraform
/// emitter and direct setup, so the two grant the same thing.
pub const SANDBOX_IMAGE_WRITE_PERMISSIONS: [&str; 7] = [
    "artifactregistry.repositories.get",
    "artifactregistry.repositories.downloadArtifacts",
    "artifactregistry.repositories.uploadArtifacts",
    "artifactregistry.tags.create",
    "artifactregistry.tags.get",
    "artifactregistry.tags.list",
    "artifactregistry.versions.get",
];

/// Id of the custom role carrying [`SANDBOX_IMAGE_WRITE_PERMISSIONS`] that a direct setup creates
/// for one sandbox: letters, digits and underscores, at most 64 characters. `None` when it would
/// not fit.
pub fn sandbox_image_write_role_id(prefix: &str, sandbox_id: &str) -> Option<String> {
    let id = format!("{prefix}_{sandbox_id}_image_write").replace('-', "_");
    (id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).then_some(id)
}

/// Return the original project ID only when its path agrees with its repository name.
pub fn project_id_for_path<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let (repository, remainder) = path.split_once('/')?;
    let project_id = remainder.split('/').next()?;
    (project_repository(prefix, project_id)?.as_str() == repository).then_some(project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_image_repository_is_valid_or_none() {
        assert_eq!(
            sandbox_image_repository("alien-abc", "sbx").as_deref(),
            Some("alien-abc-sbx-img")
        );
        assert_eq!(sandbox_image_repository("alien-abc", "Sbx_1"), None);
        assert_eq!(sandbox_image_repository("p", &"x".repeat(60)), None);
    }

    #[test]
    fn project_paths_require_the_matching_project_id() {
        for id in [
            "default",
            "prj_a",
            "prj_b",
            "prj_0123456789abcdefghijklmnopqr",
        ] {
            let repository = project_repository("alien-artifacts", id).expect("valid name");
            assert!(repository.len() <= 63);
            let path = format!("{repository}/{id}/worker");
            assert_eq!(project_id_for_path(&path, "alien-artifacts"), Some(id));
            assert_eq!(
                project_id_for_path(&format!("{repository}/prj_other"), "alien-artifacts"),
                None
            );
        }
        assert_eq!(project_repository("artifacts", "prj_A"), None);
        assert_eq!(
            project_repository("artifacts", "prj_a").as_deref(),
            Some("artifacts-prj-a")
        );
        assert!(project_repository(&"p".repeat(30), &format!("prj_{}", "a".repeat(28))).is_some());
        assert!(project_repository(&"p".repeat(31), &format!("prj_{}", "a".repeat(28))).is_none());
        assert_eq!(project_repository(&"p".repeat(64), "prj_a"), None);
        assert_eq!(project_repository("artifacts", "prj_../x"), None);
    }
}
