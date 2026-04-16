use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use glob::glob;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tracing::{info, warn};

/// Generate hash from file patterns (e.g., "**/Cargo.lock")
pub async fn hash_files(patterns: &[&str], base_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut found_any = false;

    for pattern in patterns {
        let full_pattern = base_dir.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        let matches =
            glob(&pattern_str)
                .into_alien_error()
                .context(ErrorData::InvalidGlobPattern {
                    pattern: pattern.to_string(),
                    function_name: "cache_hash".to_string(),
                    reason: "Invalid glob pattern for cache hashing".to_string(),
                })?;

        for entry in matches {
            let path = entry
                .into_alien_error()
                .context(ErrorData::InvalidGlobPattern {
                    pattern: pattern.to_string(),
                    function_name: "cache_hash".to_string(),
                    reason: "Glob matching error".to_string(),
                })?;

            if path.is_file() {
                found_any = true;
                let mut file = fs::File::open(&path).await.into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "open".to_string(),
                        file_path: path.display().to_string(),
                        reason: "Failed to open file for hashing".to_string(),
                    },
                )?;

                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)
                    .await
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "read".to_string(),
                        file_path: path.display().to_string(),
                        reason: "Failed to read file for hashing".to_string(),
                    })?;

                hasher.update(&buffer);
                hasher.update(path.to_string_lossy().as_bytes()); // Include file path in hash
            }
        }
    }

    if !found_any {
        info!("No files matched patterns {:?} for cache hashing", patterns);
    }

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Restore cached directories from object storage
/// Returns Ok(true) if cache was restored, Ok(false) if no cache store or cache miss
pub async fn restore_cache(
    store: Option<&dyn object_store::ObjectStore>,
    cache_key: &str,
    local_paths: &[PathBuf],
) -> Result<bool> {
    let store = match store {
        Some(s) => s,
        None => {
            info!("No cache store available, skipping cache restore");
            return Ok(false);
        }
    };

    let cache_path = object_store::path::Path::from(format!("cache/{}.tar.gz", cache_key));

    match store.get(&cache_path).await {
        Ok(result) => {
            info!("Cache hit for key: {}", cache_key);

            let bytes =
                result
                    .bytes()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Failed to read cache data".to_string(),
                        url: None,
                    })?;

            // Extract tar.gz to local paths
            let decoder = flate2::read::GzDecoder::new(bytes.as_ref());
            let mut archive = tar::Archive::new(decoder);

            // Create directories for cache restoration
            for path in local_paths {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .await
                        .into_alien_error()
                        .context(ErrorData::FileOperationFailed {
                            operation: "create directory".to_string(),
                            file_path: parent.display().to_string(),
                            reason: "Failed to create directory for cache restore".to_string(),
                        })?;
                }
            }

            // Validate tar entries before extraction to prevent path traversal attacks.
            // A poisoned cache archive could contain entries with absolute paths or "../"
            // that overwrite files outside the intended directories.
            for entry in archive.entries().into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "read entries".to_string(),
                    file_path: "(cache archive)".to_string(),
                    reason: "Failed to read cache archive entries".to_string(),
                },
            )? {
                let entry = entry.into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "read entry".to_string(),
                        file_path: "(cache archive)".to_string(),
                        reason: "Failed to read cache archive entry".to_string(),
                    },
                )?;
                let path = entry.path().into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "read entry path".to_string(),
                        file_path: "(cache archive)".to_string(),
                        reason: "Failed to read entry path".to_string(),
                    },
                )?;
                if path.is_absolute()
                    || path
                        .components()
                        .any(|c| c == std::path::Component::ParentDir)
                {
                    return Err(alien_error::AlienError::new(
                        ErrorData::FileOperationFailed {
                            operation: "extract".to_string(),
                            file_path: path.display().to_string(),
                            reason: "Cache archive contains path traversal entry".to_string(),
                        },
                    ));
                }
            }

            // Re-create the archive (entries() consumed the reader) and extract.
            // Entries were validated above, so we know the archive is safe.
            let decoder = flate2::read::GzDecoder::new(bytes.as_ref());
            let mut archive = tar::Archive::new(decoder);
            archive
                .unpack("/")
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "extract".to_string(),
                    file_path: "/".to_string(),
                    reason: "Failed to extract cache archive".to_string(),
                })?;

            info!("Successfully restored cache for key: {}", cache_key);
            Ok(true)
        }
        Err(object_store::Error::NotFound { .. }) => {
            info!("Cache miss for key: {}", cache_key);
            Ok(false)
        }
        Err(e) => {
            warn!("Failed to retrieve cache for key {}: {}", cache_key, e);
            Ok(false) // Don't fail the build for cache errors
        }
    }
}

/// Archive and upload directories to object storage
/// Does nothing if no cache store is available
pub async fn save_cache(
    store: Option<&dyn object_store::ObjectStore>,
    cache_key: &str,
    local_paths: &[PathBuf],
) -> Result<()> {
    let store = match store {
        Some(s) => s,
        None => {
            info!("No cache store available, skipping cache save");
            return Ok(());
        }
    };

    info!("Saving cache for key: {}", cache_key);

    // Create tar.gz archive
    let mut archive_data = Vec::new();
    {
        let encoder =
            flate2::write::GzEncoder::new(&mut archive_data, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        for local_path in local_paths {
            if local_path.exists() {
                if local_path.is_dir() {
                    tar.append_dir_all(local_path, local_path)
                        .into_alien_error()
                        .context(ErrorData::FileOperationFailed {
                            operation: "archive directory".to_string(),
                            file_path: local_path.display().to_string(),
                            reason: "Failed to add directory to cache archive".to_string(),
                        })?;
                } else if local_path.is_file() {
                    let mut file = std::fs::File::open(local_path).into_alien_error().context(
                        ErrorData::FileOperationFailed {
                            operation: "open".to_string(),
                            file_path: local_path.display().to_string(),
                            reason: "Failed to open file for caching".to_string(),
                        },
                    )?;

                    tar.append_file(local_path, &mut file)
                        .into_alien_error()
                        .context(ErrorData::FileOperationFailed {
                            operation: "archive file".to_string(),
                            file_path: local_path.display().to_string(),
                            reason: "Failed to add file to cache archive".to_string(),
                        })?;
                }
            }
        }

        tar.finish()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "finalize archive".to_string(),
                file_path: "cache".to_string(),
                reason: "Failed to finalize cache archive".to_string(),
            })?;
    }

    let cache_path = object_store::path::Path::from(format!("cache/{}.tar.gz", cache_key));

    match store.put(&cache_path, archive_data.into()).await {
        Ok(_) => {
            info!("Successfully saved cache for key: {}", cache_key);
        }
        Err(e) => {
            warn!("Failed to save cache for key {}: {}", cache_key, e);
            // Don't fail the build for cache save errors
        }
    }

    Ok(())
}
