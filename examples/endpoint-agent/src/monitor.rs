use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde_json::json;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::db::EncryptedDb;
use crate::error::{Error, Result};
use crate::pii;

/// Watch filesystem for changes
pub async fn watch_filesystem(db: &EncryptedDb) -> Result<()> {
    // Monitored paths - in production, this would be configurable
    let monitored_paths = get_monitored_paths();

    tracing::info!(
        "Starting filesystem monitoring for {} paths",
        monitored_paths.len()
    );

    let (tx, mut rx) = mpsc::channel(100);

    // Spawn watcher in blocking thread
    let tx_clone = tx.clone();
    std::thread::spawn(move || {
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx_clone.blocking_send(event);
            }
        })
        .expect("Failed to create filesystem watcher");

        for path in monitored_paths {
            if std::path::Path::new(&path).exists() {
                if let Err(e) = watcher.watch(&PathBuf::from(&path), RecursiveMode::Recursive) {
                    tracing::error!("Failed to watch path {}: {}", path, e);
                } else {
                    tracing::info!("Watching path: {}", path);
                }
            } else {
                tracing::warn!("Path does not exist: {}", path);
            }
        }

        // Keep watcher alive
        loop {
            std::thread::park();
        }
    });

    // Process events
    while let Some(event) = rx.recv().await {
        if let Err(e) = handle_filesystem_event(db, event).await {
            tracing::error!("Error handling filesystem event: {}", e);
        }
    }

    Ok(())
}

async fn handle_filesystem_event(db: &EncryptedDb, event: Event) -> Result<()> {
    match event.kind {
        EventKind::Create(_) => {
            for path in &event.paths {
                if path.is_file() {
                    tracing::debug!("File created: {:?}", path);

                    let event_data = json!({
                        "path": path.to_string_lossy(),
                    });

                    db.insert_event("file_created", &event_data).await?;
                }
            }
        }
        EventKind::Modify(_) => {
            for path in &event.paths {
                if path.is_file() {
                    tracing::debug!("File modified: {:?}", path);

                    let event_data = json!({
                        "path": path.to_string_lossy(),
                    });

                    db.insert_event("file_modified", &event_data).await?;
                }
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                tracing::debug!("File removed: {:?}", path);

                let event_data = json!({
                    "path": path.to_string_lossy(),
                });

                db.insert_event("file_removed", &event_data).await?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Watch clipboard for changes
///
/// Note: Real clipboard monitoring requires platform-specific APIs.
/// This is a simplified version for demo purposes.
pub async fn watch_clipboard(_db: &EncryptedDb) -> Result<()> {
    tracing::info!("Clipboard monitoring started (demo mode)");

    // In production, this would use:
    // - macOS: NSPasteboard
    // - Windows: Win32 API clipboard monitoring
    // - Linux: X11 clipboard or Wayland protocols
    //
    // For demo purposes, we'll just log that monitoring is active

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        tracing::debug!("Clipboard monitoring active");
    }
}

/// Simulate clipboard write detection (for demo/testing)
pub async fn simulate_clipboard_write(db: &EncryptedDb, content: &str) -> Result<()> {
    // Compute hash of content (don't store actual content)
    let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

    // Detect PII
    let pii_detection = pii::scan_text(content);

    let event_data = json!({
        "contentHash": content_hash,
        "hasPII": pii_detection.has_pii,
        "patternsFound": pii_detection.patterns_found,
    });

    db.insert_event("clipboard_write", &event_data).await?;

    tracing::info!(
        "Clipboard write logged (hash: {}, PII: {})",
        content_hash,
        pii_detection.has_pii
    );

    Ok(())
}

/// Get monitored paths based on configuration
fn get_monitored_paths() -> Vec<String> {
    // Check environment variable for custom paths
    if let Ok(paths) = std::env::var("MONITORED_PATHS") {
        return paths.split(':').map(|s| s.to_string()).collect();
    }

    // Default paths for demo (safe directories that likely exist)
    vec![std::env::temp_dir().to_string_lossy().to_string()]
}

/// Scan a directory for sensitive files
pub async fn scan_directory(path: &str) -> Result<ScanResult> {
    use std::fs;

    let path_obj = std::path::Path::new(path);

    if !path_obj.exists() {
        return Err(Error::InvalidPath(format!("Path does not exist: {}", path)));
    }

    if !path_obj.is_dir() {
        return Err(Error::InvalidPath(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    let mut files_scanned = 0;
    let mut sensitive_files = Vec::new();

    // Walk directory
    for entry in fs::read_dir(path)
        .map_err(|e| Error::Monitoring(format!("Failed to read directory {}: {}", path, e)))?
    {
        let entry = entry
            .map_err(|e| Error::Monitoring(format!("Failed to read directory entry: {}", e)))?;

        let path = entry.path();

        if path.is_file() {
            files_scanned += 1;

            // Only scan text files (skip large binaries)
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy();
                if matches!(ext_str.as_ref(), "txt" | "md" | "json" | "csv" | "log") {
                    // Read file content (with size limit)
                    if let Ok(metadata) = fs::metadata(&path) {
                        if metadata.len() < 1_000_000 {
                            // < 1MB
                            if let Ok(content) = fs::read_to_string(&path) {
                                if let Some(result) =
                                    pii::scan_file_content(&path.to_string_lossy(), &content)
                                {
                                    sensitive_files.push(result);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(ScanResult {
        files_scanned,
        sensitive_files,
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub files_scanned: usize,
    pub sensitive_files: Vec<pii::FileScanResult>,
}
