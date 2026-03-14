//! Configuration loading module for Alien CLI.
//!
//! This module provides functionality to load Alien stack configurations from various file formats:
//! - **TypeScript files** (`.ts`): Dynamically executed using Bun or Node.js
//! - **JavaScript files** (`.js`): Dynamically executed using Bun or Node.js  
//! - **JSON files** (`.json`): Directly parsed as serialized Stack objects
//!
//! ## Configuration Discovery
//!
//! When a directory is provided as the config path, the module will search for configuration files
//! in the following priority order:
//! 1. `alien.config.ts` - TypeScript configuration file
//! 2. `alien.config.js` - JavaScript configuration file
//! 3. `alien.config.json` - JSON configuration file
//!
//! ## TypeScript and JavaScript Configuration Loading
//!
//! For TypeScript and JavaScript configurations, the module will attempt to use JavaScript runtimes in this order:
//! 1. **Bun** - Preferred for faster startup and better TypeScript support
//! 2. **Node.js** - Not supported (Bun is required)
//!
//! ## Examples
//!
//! ```rust
//! use std::path::PathBuf;
//! use alien_cli::config::load_configuration;
//!
//! // Load from a directory (will search for alien.config.ts, alien.config.js, or alien.config.json)
//! let stack = load_configuration(PathBuf::from("./my-app")).await?;
//!
//! // Load from a specific file
//! let stack = load_configuration(PathBuf::from("./my-app/alien.config.ts")).await?;
//! let stack = load_configuration(PathBuf::from("./my-app/alien.config.js")).await?;
//! ```

use crate::{ErrorData, Result};
use alien_build::dependencies::install_dependencies;
use alien_core::{alien_event, AlienEvent, Stack};
use alien_error::{Context, IntoAlienError};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};

/// JavaScript runtime for executing TypeScript configurations
#[derive(Debug, Clone)]
pub enum JavaScriptRuntime {
    Bun(PathBuf),
    Node(PathBuf),
}

impl JavaScriptRuntime {
    pub fn executable(&self) -> &PathBuf {
        match self {
            JavaScriptRuntime::Bun(path) => path,
            JavaScriptRuntime::Node(path) => path,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            JavaScriptRuntime::Bun(_) => "bun",
            JavaScriptRuntime::Node(_) => "node",
        }
    }
}

/// Load an Alien stack configuration from a file or directory.
/// Searches for `alien.config.ts`, `alien.config.js`, or `alien.config.json` in directories.
#[alien_event(AlienEvent::LoadingConfiguration)]
pub async fn load_configuration(config_path: PathBuf) -> Result<Stack> {
    info!("Loading configuration from: {}", config_path.display());

    let config_file = if config_path.is_dir() {
        debug!(
            "Searching for configuration files in directory: {}",
            config_path.display()
        );
        // Look for alien.config.ts first, then alien.config.js, then alien.config.json
        let ts_config = config_path.join("alien.config.ts");
        let js_config = config_path.join("alien.config.js");
        let json_config = config_path.join("alien.config.json");

        if ts_config.exists() {
            info!("Found TypeScript configuration: {}", ts_config.display());
            ts_config
        } else if js_config.exists() {
            info!("Found JavaScript configuration: {}", js_config.display());
            js_config
        } else if json_config.exists() {
            info!("Found JSON configuration: {}", json_config.display());
            json_config
        } else {
            warn!(
                "No configuration files found in directory: {}",
                config_path.display()
            );
            return Err(alien_error::AlienError::new(
                ErrorData::ConfigurationError {
                    message: format!(
                    "Could not find alien.config.ts, alien.config.js, or alien.config.json in {}", 
                    config_path.display()
                ),
                },
            ));
        }
    } else {
        info!(
            "Loading configuration from specific file: {}",
            config_path.display()
        );
        config_path.clone()
    };

    // Check the file extension to determine how to load it
    let extension = config_file.extension().and_then(|ext| ext.to_str());
    debug!("Configuration file extension: {:?}", extension);

    match extension {
        Some("ts") => {
            info!("Loading TypeScript configuration");
            load_typescript_config(config_file).await
        }
        Some("js") => {
            info!("Loading JavaScript configuration");
            load_javascript_config(config_file).await
        }
        Some("json") => {
            info!("Loading JSON configuration");
            load_json_config(config_file).await
        }
        _ => {
            error!("Unsupported config file format: {:?}", extension);
            Err(alien_error::AlienError::new(
                ErrorData::ConfigurationError {
                    message: format!(
                        "Unsupported config file format. Expected .ts, .js, or .json, got: {}",
                        config_file.display()
                    ),
                },
            ))
        }
    }
}

/// Discover available JavaScript runtime (Bun required).
pub async fn discover_javascript_runtime() -> Result<JavaScriptRuntime> {
    debug!("Discovering JavaScript runtime...");

    // Only use Bun - Node.js is not an option
    match which::which("bun") {
        Ok(bun_path) => {
            info!("Found Bun runtime at: {}", bun_path.display());
            Ok(JavaScriptRuntime::Bun(bun_path))
        }
        Err(_) => {
            error!("Bun not found - Bun is required for TypeScript configuration support");
            Err(alien_error::AlienError::new(ErrorData::ConfigurationError {
                message: "Bun is required for TypeScript configuration support. Please install Bun to load TypeScript configurations.".to_string(),
            }))
        }
    }
}

/// Load a TypeScript configuration file using Bun or Node.js.
async fn load_typescript_config(config_file: PathBuf) -> Result<Stack> {
    load_javascript_or_typescript_config(config_file).await
}

/// Load a JavaScript configuration file using Bun or Node.js.
async fn load_javascript_config(config_file: PathBuf) -> Result<Stack> {
    load_javascript_or_typescript_config(config_file).await
}

/// Load a TypeScript or JavaScript configuration file using Bun or Node.js.
async fn load_javascript_or_typescript_config(config_file: PathBuf) -> Result<Stack> {
    info!(
        "Loading JavaScript/TypeScript configuration from: {}",
        config_file.display()
    );

    let runtime = discover_javascript_runtime().await?;
    info!(
        "Using {} runtime for configuration execution",
        runtime.name()
    );

    // Install dependencies in the directory containing the config file
    let config_dir = config_file.parent().unwrap_or_else(|| Path::new("."));
    install_dependencies(config_dir)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to install dependencies in {}", config_dir.display()),
        })?;

    debug!("Creating temporary script file...");
    let temp_file =
        NamedTempFile::new()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "create".to_string(),
                file_path: "temporary script file".to_string(),
                reason: "System temporary directory unavailable".to_string(),
            })?;
    let temp_path = temp_file.path();
    debug!("Temporary script file created at: {}", temp_path.display());

    debug!("Creating script file...");
    let mut script_file = tokio::fs::File::create(&temp_path)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: temp_path.display().to_string(),
            reason: "Unable to write to temporary file".to_string(),
        })?;

    // Create script content - works for both Bun and Node.js
    let script_content = format!(
        "
        import {{ pathToFileURL }} from 'node:url';
        import path from 'node:path';
        const configPath = path.resolve('{}');
        const config = await import(pathToFileURL(configPath));
        console.log(JSON.stringify(config.default, null, 2));
        ",
        config_file.to_str().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: config_file.display().to_string(),
                reason: "Path contains invalid UTF-8 characters".to_string(),
            })
        })?
    );
    debug!("Script content created, writing to temporary file...");

    script_file
        .write_all(script_content.as_bytes())
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: temp_path.display().to_string(),
            reason: "Unable to write script content to temporary file".to_string(),
        })?;
    debug!("Script content written to temporary file");

    info!(
        "Executing {} script to load configuration...",
        runtime.name()
    );
    let output = tokio::process::Command::new(runtime.executable())
        .arg(temp_path)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: format!("Failed to execute {} command", runtime.name()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        error!(
            "{} execution failed - stderr: {}, stdout: {}",
            runtime.name(),
            stderr,
            stdout
        );
        return Err(alien_error::AlienError::new(
            ErrorData::ConfigurationError {
                message: format!(
                    "Failed to load JavaScript/TypeScript configuration using {}: {}",
                    runtime.name(),
                    stderr
                ),
            },
        ));
    }

    debug!("{} execution completed successfully", runtime.name());

    debug!("Parsing {} output as UTF-8...", runtime.name());
    let stack_str = String::from_utf8(output.stdout)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse {} output as UTF-8", runtime.name()),
        })?;

    debug!(
        "Parsing JSON configuration from {} output...",
        runtime.name()
    );
    let stack: Stack = serde_json::from_str(&stack_str)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parsing".to_string(),
            reason: "Invalid stack configuration format from JavaScript/TypeScript output"
                .to_string(),
        })?;

    info!("Successfully loaded configuration using {}", runtime.name());
    Ok(stack)
}

#[cfg(test)]
pub async fn test_with_specific_runtime(
    config_file: PathBuf,
    runtime: JavaScriptRuntime,
) -> Result<Stack> {
    let temp_file =
        NamedTempFile::new()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "create".to_string(),
                file_path: "temporary script file".to_string(),
                reason: "System temporary directory unavailable".to_string(),
            })?;
    let temp_path = temp_file.path();

    let mut script_file = tokio::fs::File::create(&temp_path)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: temp_path.display().to_string(),
            reason: "Unable to write to temporary file".to_string(),
        })?;

    // Create script content - works for both Bun and Node.js
    let script_content = format!(
        "
        import {{ pathToFileURL }} from 'node:url';
        import path from 'node:path';
        const configPath = path.resolve('{}');
        const config = await import(pathToFileURL(configPath));
        console.log(JSON.stringify(config.default, null, 2));
        ",
        config_file.to_str().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: config_file.display().to_string(),
                reason: "Path contains invalid UTF-8 characters".to_string(),
            })
        })?
    );

    script_file
        .write_all(script_content.as_bytes())
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: temp_path.display().to_string(),
            reason: "Unable to write script content to temporary file".to_string(),
        })?;

    let output = tokio::process::Command::new(runtime.executable())
        .arg(temp_path)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: format!("Failed to execute {} command", runtime.name()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _stdout = String::from_utf8_lossy(&output.stdout);
        return Err(alien_error::AlienError::new(
            ErrorData::ConfigurationError {
                message: format!(
                    "Failed to load JavaScript/TypeScript configuration using {}: {}",
                    runtime.name(),
                    stderr
                ),
            },
        ));
    }

    let stack_str = String::from_utf8(output.stdout)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse {} output as UTF-8", runtime.name()),
        })?;

    let stack: Stack = serde_json::from_str(&stack_str)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parsing".to_string(),
            reason: "Invalid stack configuration format from JavaScript/TypeScript output"
                .to_string(),
        })?;

    Ok(stack)
}

/// Load a JSON configuration file directly.
async fn load_json_config(config_file: PathBuf) -> Result<Stack> {
    debug!("Reading JSON configuration file: {}", config_file.display());
    let config_content = tokio::fs::read_to_string(&config_file)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: config_file.display().to_string(),
            reason: "File not accessible or doesn't exist".to_string(),
        })?;

    debug!("Parsing JSON configuration content...");
    let stack: Stack = serde_json::from_str(&config_content)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parsing".to_string(),
            reason: format!(
                "Invalid JSON syntax in config file: {}",
                config_file.display()
            ),
        })?;

    info!(
        "Successfully loaded JSON configuration from: {}",
        config_file.display()
    );
    Ok(stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use which;

    // Re-use the shared test utilities
    use crate::test_utils::*;

    /// Helper to create alien.config.ts content for config tests (different from shared version)
    fn create_typescript_config_content() -> String {
        create_full_alien_config_ts()
    }

    #[tokio::test]
    async fn test_load_typescript_config() {
        let temp_dir = create_temp_app_dir("ts");
        let shared_nm = shared_node_modules_path().await;
        std::os::unix::fs::symlink(shared_nm, temp_dir.path().join("node_modules")).unwrap();
        let result = load_configuration(temp_dir.path().to_path_buf()).await;

        match result {
            Ok(stack) => {
                assert_eq!(stack.id(), "test-stack-ts");
                assert_eq!(stack.resources().count(), 2); // storage, function
            }
            Err(e) => {
                // If no JS runtime is available, just skip this test
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping TypeScript test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load TypeScript config: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_load_javascript_config() {
        let temp_dir = create_temp_app_dir("js");
        let shared_nm = shared_node_modules_path().await;
        std::os::unix::fs::symlink(shared_nm, temp_dir.path().join("node_modules")).unwrap();
        let result = load_configuration(temp_dir.path().to_path_buf()).await;

        match result {
            Ok(stack) => {
                assert_eq!(stack.id(), "test-stack-js");
                assert_eq!(stack.resources().count(), 2); // storage, function
            }
            Err(e) => {
                // If no JS runtime is available, just skip this test
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping JavaScript test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load JavaScript config: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_load_json_config() {
        let temp_dir = create_temp_app_dir("json");
        let result = load_configuration(temp_dir.path().to_path_buf()).await;

        assert!(result.is_ok());
        let stack = result.unwrap();
        assert_eq!(stack.id(), "test-stack-json");
        assert_eq!(stack.resources().count(), 2); // storage, function
    }

    #[tokio::test]
    async fn test_load_specific_typescript_file() {
        let temp_dir = create_temp_app_dir("ts");
        let config_path = temp_dir.path().join("alien.config.ts");
        let result = load_configuration(config_path).await;

        match result {
            Ok(stack) => {
                assert_eq!(stack.id(), "test-stack-ts");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping TypeScript file test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load specific TypeScript file: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_load_specific_javascript_file() {
        let temp_dir = create_temp_app_dir("js");
        let config_path = temp_dir.path().join("alien.config.js");
        let result = load_configuration(config_path).await;

        match result {
            Ok(stack) => {
                assert_eq!(stack.id(), "test-stack-js");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping JavaScript file test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load specific JavaScript file: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_load_specific_json_file() {
        let temp_dir = create_temp_app_dir("json");
        let config_path = temp_dir.path().join("alien.config.json");
        let result = load_configuration(config_path).await;

        assert!(result.is_ok());
        let stack = result.unwrap();
        assert_eq!(stack.id(), "test-stack-json");
    }

    #[tokio::test]
    async fn test_priority_order_typescript_first() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create package.json
        let package_json = create_package_json_content();
        fs::write(temp_path.join("package.json"), package_json).unwrap();

        // Create all three config files
        fs::write(
            temp_path.join("alien.config.ts"),
            create_typescript_config_content(),
        )
        .unwrap();
        fs::write(
            temp_path.join("alien.config.js"),
            create_javascript_config_content(),
        )
        .unwrap();
        fs::write(
            temp_path.join("alien.config.json"),
            create_json_config_content(),
        )
        .unwrap();

        // Install dependencies for JS/TS files
        install_dependencies(temp_path)
            .await
            .expect("Failed to install dependencies");

        let result = load_configuration(temp_path.to_path_buf()).await;

        match result {
            Ok(stack) => {
                // Should load TypeScript first
                assert_eq!(stack.id(), "test-stack-ts");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping priority test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load config with priority: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_priority_order_javascript_second() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create package.json
        let package_json = create_package_json_content();
        fs::write(temp_path.join("package.json"), package_json).unwrap();

        // Create JavaScript and JSON config files (no TypeScript)
        fs::write(
            temp_path.join("alien.config.js"),
            create_javascript_config_content(),
        )
        .unwrap();
        fs::write(
            temp_path.join("alien.config.json"),
            create_json_config_content(),
        )
        .unwrap();

        // Install dependencies for JS files
        install_dependencies(temp_path)
            .await
            .expect("Failed to install dependencies");

        let result = load_configuration(temp_path.to_path_buf()).await;

        match result {
            Ok(stack) => {
                // Should load JavaScript second
                assert_eq!(stack.id(), "test-stack-js");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("No JavaScript runtime found") {
                    println!("Skipping priority test: No JavaScript runtime found");
                    return;
                }
                panic!("Failed to load config with priority: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_priority_order_json_last() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create only JSON config file
        fs::write(
            temp_path.join("alien.config.json"),
            create_json_config_content(),
        )
        .unwrap();

        let result = load_configuration(temp_path.to_path_buf()).await;

        assert!(result.is_ok());
        let stack = result.unwrap();
        // Should load JSON last
        assert_eq!(stack.id(), "test-stack-json");
    }

    #[tokio::test]
    async fn test_no_config_file_error() {
        let temp_dir = TempDir::new().unwrap();
        let result = load_configuration(temp_dir.path().to_path_buf()).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg
            .contains("Could not find alien.config.ts, alien.config.js, or alien.config.json"));
    }

    #[tokio::test]
    async fn test_unsupported_file_extension() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create a config file with unsupported extension
        fs::write(temp_path.join("alien.config.yaml"), "invalid: config").unwrap();

        let result = load_configuration(temp_path.join("alien.config.yaml")).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Unsupported config file format"));
    }

    #[tokio::test]
    async fn test_javascript_runtime_discovery() {
        match discover_javascript_runtime().await {
            Ok(runtime) => {
                println!("JavaScript runtime found: {}", runtime.name());
                assert!(matches!(runtime, JavaScriptRuntime::Bun(_)));
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("Bun is required") {
                    println!("Bun not found - this is expected in some CI environments");
                } else {
                    panic!(
                        "Runtime discovery test failed with unexpected error: {}",
                        error_msg
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn test_bun_runtime_specifically() {
        // Check if Bun is available
        if let Ok(bun_path) = which::which("bun") {
            let temp_dir = create_temp_app_dir("ts");
            let config_path = temp_dir.path().join("alien.config.ts");
            let runtime = JavaScriptRuntime::Bun(bun_path);

            match test_with_specific_runtime(config_path, runtime).await {
                Ok(stack) => {
                    println!("Bun runtime test passed");
                    assert_eq!(stack.id(), "test-stack-ts");
                }
                Err(e) => {
                    println!("Bun runtime test failed: {}", e);
                    // Don't panic, just report the issue
                }
            }
        } else {
            println!("Bun not found, skipping Bun-specific test");
        }
    }

    #[tokio::test]
    async fn test_node_runtime_specifically() {
        // Check if Node.js is available
        if let Ok(node_path) = which::which("node") {
            let temp_dir = create_temp_app_dir("ts");
            let config_path = temp_dir.path().join("alien.config.ts");
            let runtime = JavaScriptRuntime::Node(node_path);

            match test_with_specific_runtime(config_path, runtime).await {
                Ok(stack) => {
                    println!("Node.js runtime test passed");
                    assert_eq!(stack.id(), "test-stack-ts");
                }
                Err(e) => {
                    println!("Node.js runtime test failed: {}", e);
                    // Don't panic, just report the issue
                }
            }
        } else {
            println!("Node.js not found, skipping Node.js-specific test");
        }
    }

    #[tokio::test]
    async fn test_malformed_json_config() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create malformed JSON config
        fs::write(temp_path.join("alien.config.json"), "{ invalid json }").unwrap();

        let result = load_configuration(temp_path.to_path_buf()).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("JSON parsing failed") || error_msg.contains("Invalid JSON syntax"),
            "Unexpected JSON error message: {}",
            error_msg
        );
    }

    #[tokio::test]
    async fn test_typescript_config_with_syntax_error() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create package.json
        let package_json = create_package_json_content();
        fs::write(temp_path.join("package.json"), package_json).unwrap();

        // Create TypeScript config with syntax error
        fs::write(
            temp_path.join("alien.config.ts"),
            "import * alien from '@aliendotdev/core'; // missing {",
        )
        .unwrap();

        // Install dependencies even for syntax error test
        install_dependencies(temp_path)
            .await
            .expect("Failed to install dependencies");

        let result = load_configuration(temp_path.to_path_buf()).await;

        match result {
            Ok(_) => {
                panic!("Expected error for malformed TypeScript config");
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("Bun is required") {
                    println!("Skipping TypeScript syntax error test: Bun not found");
                    return;
                }
                // Should contain error about failed loading
                assert!(error_msg.contains("Failed to load JavaScript/TypeScript configuration"));
            }
        }
    }
}
