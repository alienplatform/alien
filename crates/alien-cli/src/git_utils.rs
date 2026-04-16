//! Git utilities for collecting metadata from the current repository
//!
//! This module provides functionality to:
//! - Collect git metadata (commit SHA, branch, author, etc.)
//! - Detect and parse git remote URLs
//! - Determine git provider type (GitHub, etc.)
//!
//! # Examples
//!
//! ```no_run
//! use std::env;
//! use alien_cli::git_utils::{collect_git_metadata, detect_git_repository};
//!
//! // Collect metadata from current directory
//! let metadata = collect_git_metadata(env::current_dir().unwrap()).unwrap();
//! if let Some(inner) = metadata.0 {
//!     println!("Current branch: {:?}", inner.commit_ref);
//! }
//!
//! // Detect git repository info
//! if let Some(repo_info) = detect_git_repository(env::current_dir().unwrap()).unwrap() {
//!     println!("Repository: {}", *repo_info.repo);
//! }
//! ```

use crate::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use alien_platform_api::types::GitMetadata;
use git_url_parse::GitUrl;
use std::path::Path;
use std::process::Command;

/// Collect git metadata from the current repository
///
/// This function collects comprehensive git metadata including:
/// - Commit SHA and message
/// - Current branch
/// - Author information  
/// - Working tree status (dirty/clean)
/// - Remote URL
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository
///
/// # Returns
///
/// Returns `GitMetadata` struct with available metadata, or empty metadata if not in a git repo
///
/// # Examples
///
/// ```no_run
/// # use alien_cli::git_utils::collect_git_metadata;
/// let metadata = collect_git_metadata(".").unwrap();
/// if let Some(inner) = metadata.0 {
///     if let Some(branch) = inner.commit_ref {
///         println!("Current branch: {:?}", branch);
///     }
/// }
/// ```
pub fn collect_git_metadata<P: AsRef<Path>>(repo_path: P) -> Result<GitMetadata> {
    let repo_path = repo_path.as_ref();

    // Check if we're in a git repository
    if !is_git_repository(repo_path) {
        return Ok(GitMetadata(None)); // Return empty metadata if not in a git repo
    }

    let mut inner = alien_platform_api::types::GitMetadataInner {
        commit_author_name: None,
        commit_author_email: None,
        commit_author_login: None,
        commit_author_avatar_url: None,
        commit_message: None,
        commit_ref: None,
        commit_sha: None,
        commit_date: None,
        dirty: None,
        remote_url: None,
        provider: None,
    };

    // Get commit SHA
    if let Ok(sha) = run_git_command(repo_path, &["rev-parse", "HEAD"]) {
        inner.commit_sha = Some(
            alien_platform_api::types::GitMetadataInnerCommitSha::try_from(sha.trim().to_string())
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "commit_sha".to_string(),
                    message: "Invalid git commit SHA format".to_string(),
                })?,
        );
    }

    // Get commit message
    if let Ok(message) = run_git_command(repo_path, &["log", "-1", "--pretty=format:%s"]) {
        inner.commit_message = Some(
            alien_platform_api::types::GitMetadataInnerCommitMessage::try_from(
                message.trim().to_string(),
            )
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "commit_message".to_string(),
                message: "Invalid git commit message format".to_string(),
            })?,
        );
    }

    // Get commit author name
    if let Ok(author) = run_git_command(repo_path, &["log", "-1", "--pretty=format:%an"]) {
        inner.commit_author_name = Some(
            alien_platform_api::types::GitMetadataInnerCommitAuthorName::try_from(
                author.trim().to_string(),
            )
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "commit_author_name".to_string(),
                message: "Invalid git author name format".to_string(),
            })?,
        );
    }

    // Get commit author email
    if let Ok(email) = run_git_command(repo_path, &["log", "-1", "--pretty=format:%ae"]) {
        let email = email.trim();
        if !email.is_empty() {
            inner.commit_author_email = Some(email.to_string());
        }
    }

    // Get commit date (ISO 8601 format)
    if let Ok(date_str) = run_git_command(repo_path, &["log", "-1", "--pretty=format:%cI"]) {
        let date_str = date_str.trim();
        inner.commit_date = Some(
            chrono::DateTime::parse_from_rfc3339(date_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "commit_date".to_string(),
                    message: format!("Invalid git commit date format: {}", date_str),
                })?,
        );
    }

    // Get current branch
    if let Ok(branch) = run_git_command(repo_path, &["branch", "--show-current"]) {
        let branch = branch.trim();
        if !branch.is_empty() {
            inner.commit_ref = Some(
                alien_platform_api::types::GitMetadataInnerCommitRef::try_from(branch.to_string())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "commit_ref".to_string(),
                        message: "Invalid git branch name format".to_string(),
                    })?,
            );
        }
    }

    // Check if working tree is dirty
    if let Ok(status) = run_git_command(repo_path, &["status", "--porcelain"]) {
        inner.dirty = Some(!status.trim().is_empty());
    }

    // Get remote URL
    if let Ok(remote_url) = get_remote_url(repo_path) {
        inner.remote_url = Some(
            alien_platform_api::types::GitMetadataInnerRemoteUrl::try_from(remote_url)
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "remote_url".to_string(),
                    message: "Invalid git remote URL format".to_string(),
                })?,
        );
    }

    Ok(GitMetadata(Some(inner)))
}

/// Check if a directory is part of a git repository
///
/// Uses `git rev-parse --git-dir` which works even in subdirectories of a git repository
fn is_git_repository<P: AsRef<Path>>(repo_path: P) -> bool {
    run_git_command(repo_path, &["rev-parse", "--git-dir"]).is_ok()
}

/// Get the remote URL from a git repository (try origin first, then any remote)
fn get_remote_url<P: AsRef<Path>>(repo_path: P) -> Result<String> {
    // Try to get origin remote first
    run_git_command(&repo_path, &["remote", "get-url", "origin"])
        .or_else(|_| {
            // If no origin, try to get any remote
            run_git_command(&repo_path, &["remote"]).and_then(|remotes| {
                let first_remote = remotes.lines().next().unwrap_or("").trim();
                if first_remote.is_empty() {
                    Err(alien_error::AlienError::new(
                        ErrorData::ConfigurationError {
                            message: "No git remotes found in repository".to_string(),
                        },
                    ))
                } else {
                    run_git_command(&repo_path, &["remote", "get-url", first_remote])
                }
            })
        })
        .map(|url| url.trim().to_string())
}

/// Run a git command and return the output
fn run_git_command<P: AsRef<Path>>(repo_path: P, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: format!("Failed to execute git command: git {}", args.join(" ")),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(alien_error::AlienError::new(ErrorData::GenericError {
            message: format!("Git command failed: {}", stderr),
        }));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// Re-export the SDK types for convenience
pub use alien_platform_api::types::CreateProjectBodyGitRepository as GitRepositoryInfo;

/// Extract git repository information from the current directory
///
/// This function detects if the directory is a git repository and extracts
/// information about the remote repository, including the provider type
/// and repository path.
///
/// # Arguments
///
/// * `repo_path` - Path to check for git repository
///
/// # Returns
///
/// Returns `Some(GitRepositoryInfo)` if a supported git repository is detected,
/// `None` if not a git repository or the provider is not supported.
///
/// # Examples
///
/// ```no_run
/// # use alien_cli::git_utils::detect_git_repository;
/// if let Some(repo_info) = detect_git_repository(".").unwrap() {
///     println!("Found repository: {}", *repo_info.repo);
/// } else {
///     println!("No supported git repository found");
/// }
/// ```
pub fn detect_git_repository<P: AsRef<Path>>(repo_path: P) -> Result<Option<GitRepositoryInfo>> {
    let repo_path = repo_path.as_ref();

    // Check if we're in a git repository
    if !is_git_repository(repo_path) {
        return Ok(None);
    }

    // Get the remote URL
    let remote_url = get_remote_url(repo_path)?;

    parse_git_remote_url(&remote_url)
}

/// Parse a git remote URL to extract provider and repository information
///
/// Supports both SSH and HTTPS git URLs and handles various git providers.
/// Currently only GitHub is fully supported.
///
/// # Arguments
///
/// * `remote_url` - The git remote URL to parse
///
/// # Returns
///
/// Returns `Some(GitRepositoryInfo)` if the URL is valid and the provider is supported,
/// `None` if the URL cannot be parsed or the provider is not supported.
fn parse_git_remote_url(remote_url: &str) -> Result<Option<GitRepositoryInfo>> {
    let url = remote_url.trim();

    // Parse the git URL using git-url-parse crate
    let parsed_url = match GitUrl::parse(url) {
        Ok(parsed) => parsed,
        Err(_) => {
            // If parsing fails, return None instead of an error
            return Ok(None);
        }
    };

    // Extract host and fullname (owner/repo)
    let host = match parsed_url.host {
        Some(host) => host,
        None => return Ok(None),
    };

    let repo_path = parsed_url.fullname;

    // Validate that we have a proper owner/repo format
    if repo_path.is_empty() || !repo_path.contains('/') {
        return Ok(None);
    }

    // Determine provider type from host
    let provider_type = match determine_provider_from_host(&host) {
        Some(provider) => provider,
        None => return Ok(None), // Unsupported provider
    };

    Ok(Some(GitRepositoryInfo {
        repo: alien_platform_api::types::CreateProjectBodyGitRepositoryRepo::try_from(
            repo_path.clone(),
        )
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "repository_path".to_string(),
            message: format!("Invalid repository path format: {}", repo_path),
        })?,
        type_: provider_type,
    }))
}

/// Determine the git provider type from the hostname
///
/// Currently supports:
/// - GitHub (github.com and enterprise instances)
///
/// Future support planned for GitLab, Bitbucket, and other providers.
///
/// # Arguments
///
/// * `host` - The hostname from the git URL
///
/// # Returns
///
/// Returns `Some(Type)` if the provider is supported, `None` otherwise.
fn determine_provider_from_host(
    host: &str,
) -> Option<alien_platform_api::types::CreateProjectBodyGitRepositoryType> {
    match host {
        "github.com" => Some(alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github),
        // For enterprise/self-hosted GitHub instances
        host if host.contains("github") => {
            Some(alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github)
        }
        // TODO: Add support for other providers in the future
        // "gitlab.com" => Some(Type::Gitlab),
        // "bitbucket.org" => Some(Type::Bitbucket),
        _ => None,
    }
}

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_ssh_url() {
        let result = parse_git_remote_url("git@github.com:owner/repo.git").unwrap();
        assert!(result.is_some());
        let repo_info = result.unwrap();
        assert_eq!(*repo_info.repo, "owner/repo");
        assert_eq!(
            repo_info.type_,
            alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github
        );
    }

    #[test]
    fn test_parse_github_https_url() {
        let result = parse_git_remote_url("https://github.com/owner/repo.git").unwrap();
        assert!(result.is_some());
        let repo_info = result.unwrap();
        assert_eq!(*repo_info.repo, "owner/repo");
        assert_eq!(
            repo_info.type_,
            alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github
        );
    }

    #[test]
    fn test_parse_github_https_url_without_git_suffix() {
        let result = parse_git_remote_url("https://github.com/owner/repo").unwrap();
        assert!(result.is_some());
        let repo_info = result.unwrap();
        assert_eq!(*repo_info.repo, "owner/repo");
        assert_eq!(
            repo_info.type_,
            alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github
        );
    }

    #[test]
    fn test_parse_github_enterprise_url() {
        let result = parse_git_remote_url("git@github.company.com:owner/repo.git").unwrap();
        assert!(result.is_some());
        let repo_info = result.unwrap();
        assert_eq!(*repo_info.repo, "owner/repo");
        assert_eq!(
            repo_info.type_,
            alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github
        );
    }

    #[test]
    fn test_parse_unsupported_provider() {
        let result = parse_git_remote_url("git@gitlab.com:owner/repo.git").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_invalid_url() {
        let result = parse_git_remote_url("not-a-valid-url").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_url_without_owner_repo() {
        let result = parse_git_remote_url("https://github.com/").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_determine_provider_from_host() {
        assert_eq!(
            determine_provider_from_host("github.com"),
            Some(alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github)
        );
        assert_eq!(
            determine_provider_from_host("github.company.com"),
            Some(alien_platform_api::types::CreateProjectBodyGitRepositoryType::Github)
        );
        assert_eq!(determine_provider_from_host("gitlab.com"), None);
        assert_eq!(determine_provider_from_host("bitbucket.org"), None);
        assert_eq!(determine_provider_from_host("unknown.com"), None);
    }

    #[test]
    fn test_is_git_repository() {
        use std::process::Command;
        use tempfile::TempDir;

        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Should not be a git repository initially
        assert!(!is_git_repository(temp_path));

        // Initialize a git repository
        let output = Command::new("git")
            .args(&["init"])
            .current_dir(temp_path)
            .output()
            .expect("Failed to execute git init");

        if output.status.success() {
            // Now it should be detected as a git repository
            assert!(is_git_repository(temp_path));
        } else {
            // If git is not available, skip the positive test
            println!("Git not available, skipping git repository detection test");
        }
    }
}
