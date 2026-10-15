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
