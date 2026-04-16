# Guidelines

**Always follow the coding guidelines in `AGENTS.md`** — it covers error handling, testing, imports, and all Rust conventions for this project.

- **First principles thinking** — ask "why" until you truly understand the problem
- **Root cause analysis** — don't patch symptoms, find and fix the real bug
- **No hacks** — implement things properly, even if it takes longer
- **Quality over speed** — write code your future self will thank you for
- **Keep it simple** — the best code is the code you don't write
- **Leave code better than you found it** — improve something small in every change
- **Explicit over implicit** — if it's not obvious, make it obvious
- **Fail fast, fail loud** — surface errors immediately, don't swallow them

# Rust Guidelines

## Dependencies

Use `workspace = true` for all dependencies defined in the workspace `Cargo.toml`. Never duplicate versions.

## Code Style

### Import Organization

Place all `use` statements at the top of the file or module. Never scatter imports throughout the code.

```rust
// Good: All imports at the top
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{info, warn};

async fn do_work() {
    // ...
}

// Bad: Import in the middle of the code
async fn do_work() {
    use std::collections::HashMap; // Don't do this
    // ...
}
```

### Consolidate Imports

Avoid repeating the same module path multiple times. Group related imports together.

```rust
// Good
use alien_core::{Platform, ResourceType, StackConfig};

// Bad
use alien_core::Platform;
use alien_core::ResourceType;
use alien_core::StackConfig;
```

### Serde Serialization

Always use `camelCase` for JSON serialization and document fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentConfig {
    /// Unique identifier for the deployment
    pub deployment_id: String,
    /// Target environment (production, staging, etc.)
    pub target_environment: String,
}
```

### Use Builders for Multiple Parameters

Use the `bon` crate's builder pattern for functions or structs with more than a few parameters instead of long parameter lists.

---

## Preferred Crates

| Purpose | Crate | Notes |
|---------|-------|-------|
| Time & Date | `chrono` | Never use `time` crate |
| Logging | `tracing` | Use `info!`, `warn!`, `error!`, `debug!` macros |
| Serialization | `serde` | Always with `rename_all = "camelCase"` |
| Async Runtime | `tokio` | Our standard async runtime |
| Error Handling | `alien-error` | Our custom error library (see below) |

---

## Git

### Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `<type>: <short summary>`

**Types**: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `ci`, `perf`

- Imperative mood, lowercase, no period, max ~72 chars
- Body (optional): explain *why*, not *what*; wrap at 72 chars
- Breaking changes: add `!` after type (e.g. `feat!: remove v1 API`)

### Branches

- Branch from `main`; use `<type>/<short-description>` (e.g. `feat/azure-containers`, `fix/duplicate-cleanup`)
- Keep branches short-lived; rebase on `main` before merging

### Pushing and PRs

- Never force-push to `main`
- Push feature branches and open a PR for review
- Squash-merge PRs into `main`

---

## Alien Errors

### Why Not `thiserror` or `anyhow`?

We use our custom `alien-error` library instead of the popular alternatives because:

1. **Structured metadata**: Every error carries machine-readable `code`, `retryable`, `internal`, and `http_status_code` fields—essential for API responses and automated retry logic.

2. **Error chaining with context**: Like `anyhow`, we support chaining errors, but each layer preserves structured metadata, not just string messages.

3. **API-ready**: Errors serialize to JSON with consistent schema, and the `internal` flag controls whether sensitive details are exposed to external clients.

4. **Inheritance**: The `retryable` and `internal` flags can be inherited from source errors using `"inherit"`, so wrapper errors don't need to guess.

### The Two Aspects of Error Handling

There are two distinct aspects to working with errors:

1. **Designing errors** - Defining error enums for your crate/module
2. **Using errors** - Propagating and creating errors in your code

---

## Designing Error Enums

Each crate should have an `error.rs` file that defines its error types.

### Design Principles

1. **Name variants after the problem, not dependencies**
   Variants should reflect domain-level issues, not internal implementation details or external libraries.

2. **Embed actionable, self-contained data**
   Include sufficient context directly in variants so callers don't need to downcast or guess. Almost always include a human-readable `message` field.

3. **Provide useful context for recovery**
   Include fields like paths, URLs, IDs, or parameters to assist in debugging and handling errors effectively.

4. **Use a generic catch-all variant when needed**
   If there are too many possible errors, provide a generic variant (e.g., `Other` or `InfrastructureError`) for unexpected or uncommon cases.

5. **Always document each variant concisely**
   Include brief, one-line doc comments to help maintainers understand each error.

6. **Set appropriate metadata**
   Use the `code`, `message`, `retryable`, `internal`, and optionally `http_status_code` attributes.

### Required Attributes

Every variant must specify:

| Attribute | Description |
|-----------|-------------|
| `code` | Machine-readable identifier (e.g., `"RESOURCE_NOT_FOUND"`) |
| `message` | Human-readable template with field interpolation |
| `retryable` | `"true"`, `"false"`, or `"inherit"` |
| `internal` | `"true"`, `"false"`, or `"inherit"` |
| `http_status_code` | (Optional) HTTP status code, defaults to 500 |

### Example Error Enum

```rust
use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors related to deployment operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Deployment target configuration is invalid.
    #[error(
        code = "DEPLOYMENT_TARGET_INVALID",
        message = "Deployment target '{target_id}' is invalid: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    DeploymentTargetInvalid {
        /// The target ID that failed validation
        target_id: String,
        /// Specific reason why the target is invalid
        reason: String,
    },

    /// Cloud provider API call failed.
    #[error(
        code = "CLOUD_API_FAILED",
        message = "Cloud API call failed: {message}",
        retryable = "inherit",  // Inherit from source error
        internal = "inherit"
    )]
    CloudApiFailed {
        /// Human-readable description of the failure
        message: String,
        /// The cloud provider involved
        provider: String,
    },

    /// Resource was not found.
    #[error(
        code = "RESOURCE_NOT_FOUND",
        message = "Resource '{resource_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ResourceNotFound {
        /// ID of the missing resource
        resource_id: String,
    },

    /// Generic catch-all for uncommon errors.
    #[error(
        code = "DEPLOYMENT_ERROR",
        message = "Deployment operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    Other {
        /// Human-readable description
        message: String,
    },
}

/// Convenient type alias for this module's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
```

### When to Use `inherit`

Use `retryable = "inherit"` or `internal = "inherit"` when your error wraps another error and should preserve its characteristics:

```rust
/// Wraps cloud platform errors without losing their retry/internal semantics.
#[error(
    code = "CLOUD_PLATFORM_ERROR",
    message = "Cloud platform operation failed: {message}",
    retryable = "inherit",
    internal = "inherit"
)]
CloudPlatformError {
    message: String,
},
```

---

## Using Errors in Code

### Creating New Errors

When there's no source error to wrap:

```rust
use alien_error::AlienError;

fn validate_input(input: &str) -> Result<()> {
    if input.is_empty() {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: "Input cannot be empty".to_string(),
            field_name: Some("input".to_string()),
        }));
    }
    Ok(())
}
```

### Adding Context to Alien Errors

When propagating errors from other Alien crates, use `.context()`:

```rust
use alien_error::Context;

async fn deploy_function(&self, config: &FunctionConfig) -> Result<()> {
    self.cloud_client
        .create_function(config)
        .await
        .context(ErrorData::CloudApiFailed {
            message: format!("Failed to create function '{}'", config.name),
            provider: "AWS".to_string(),
        })?;

    Ok(())
}
```

### Converting Third-Party Errors

For errors from non-Alien crates (std, tokio, serde, etc.), first convert with `.into_alien_error()`:

```rust
use alien_error::{Context, IntoAlienError};

async fn read_config(path: &Path) -> Result<Config> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .into_alien_error()  // Convert std::io::Error to AlienError
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read configuration file".to_string(),
        })?;

    serde_json::from_str(&contents)
        .into_alien_error()  // Convert serde_json::Error to AlienError
        .context(ErrorData::ConfigParseError {
            message: "Invalid JSON in configuration file".to_string(),
        })
}
```

> **Important**: Only use `.into_alien_error()` for third-party errors. Errors from Alien crates already implement the correct traits—just use `.context()` directly.

### Avoid Redundant Messages

Don't repeat what the error template already says:

```rust
// Bad: Redundant message
.context(ErrorData::CloudApiFailed {
    message: "Cloud API call failed".to_string(),  // Template already says this!
    provider: "AWS".to_string(),
})

// Good: Specific details
.context(ErrorData::CloudApiFailed {
    message: "CreateFunction returned 403 Forbidden".to_string(),
    provider: "AWS".to_string(),
})
```

---

## Fail Fast, Don't Fall Back Silently

When a required condition cannot be met, return an error immediately.

### Bad Patterns

```rust
// Bad: Silent fallback hides failures
fn get_config_value(key: &str) -> String {
    match config.get(key) {
        Some(value) => value.clone(),
        None => {
            warn!("Config key '{}' not found, using default", key);
            "default".to_string()  // Silently proceeding with wrong value
        }
    }
}

// Bad: Logging and continuing
async fn sync_resource(&self) -> Result<()> {
    if let Err(e) = self.cloud_client.update().await {
        warn!("Failed to update resource: {}", e);
        // Continues execution as if nothing happened!
    }
    Ok(())
}
```

### Good Patterns

```rust
// Good: Fail immediately when required value is missing
fn get_config_value(key: &str) -> Result<String> {
    config.get(key)
        .cloned()
        .ok_or_else(|| AlienError::new(ErrorData::ConfigKeyMissing {
            key: key.to_string(),
        }))
}

// Good: Propagate errors properly
async fn sync_resource(&self) -> Result<()> {
    self.cloud_client
        .update()
        .await
        .context(ErrorData::SyncFailed {
            message: "Failed to update resource".to_string(),
        })?;
    Ok(())
}
```

### When `warn!` Is Appropriate

Use `warn!` only for **non-critical issues that don't affect correctness**:

```rust
// Appropriate: Optional optimization failed, core functionality works
if let Err(e) = cache.invalidate(&key).await {
    warn!("Failed to invalidate cache for '{}': {}. Proceeding without cache.", key, e);
}
// Core operation continues and will work correctly, just slower
```

### Retries Should Be External

Don't implement retry logic inside functions that can fail. Let the caller decide:

```rust
// Bad: Hidden retry logic
async fn fetch_data(&self) -> Result<Data> {
    for attempt in 1..=3 {
        match self.client.get().await {
            Ok(data) => return Ok(data),
            Err(e) => {
                warn!("Attempt {} failed: {}", attempt, e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Err(AlienError::new(ErrorData::FetchFailed { ... }))
}

// Good: Return error, let caller handle retries
async fn fetch_data(&self) -> Result<Data> {
    self.client
        .get()
        .await
        .context(ErrorData::FetchFailed { ... })
}
```

The retry logic belongs at a higher level (e.g., the executor, job runner, or API handler) where retry policies can be configured consistently.

---

## Writing Tests

Tests must be **strict**. Assert as much as possible—the test should fail if the entire flow didn't pass.

### Dangerous Patterns to Avoid

```rust
// Bad: Using warn! without failing
#[tokio::test]
async fn test_deployment() {
    let result = deploy().await;
    if let Err(e) = result {
        warn!("Deployment failed: {}", e);  // Test passes even on failure!
    }
}

// Bad: Conditional skipping
#[tokio::test]
async fn test_resource_creation() {
    let resource = create_resource().await.unwrap();
    if resource.status == "ready" {  // What if it's not ready?
        assert_eq!(resource.name, "expected");
    }
    // Test passes even if status wasn't "ready"!
}

// Bad: Swallowing errors silently
#[tokio::test]
async fn test_cleanup() {
    let _ = delete_resource().await.ok();  // Silently ignores failures
    // or
    let result = operation().await.unwrap_or_default();  // Hides errors
}
```

### Good Testing Patterns

```rust
// Good: Assert everything
#[tokio::test]
async fn test_deployment() {
    let result = deploy().await.expect("deployment should succeed");

    assert_eq!(result.status, DeploymentStatus::Complete);
    assert_eq!(result.resources.len(), 3);
    assert!(result.url.starts_with("https://"));
}

// Good: Explicit assertions for all branches
#[tokio::test]
async fn test_resource_creation() {
    let resource = create_resource().await.expect("resource creation should succeed");

    assert_eq!(resource.status, "ready", "resource should be ready immediately");
    assert_eq!(resource.name, "expected");
}

// Good: Test error cases explicitly
#[tokio::test]
async fn test_invalid_input_fails() {
    let result = create_resource_with_invalid_input().await;

    let error = result.expect_err("should fail with invalid input");
    assert_eq!(error.code, "INVALID_INPUT");
}
```

### Test Structure

1. **Arrange**: Set up test fixtures and mock services
2. **Act**: Perform the operation being tested
3. **Assert**: Verify ALL expected outcomes

```rust
#[tokio::test]
async fn test_function_deployment() {
    // Arrange
    let mock_provider = MockServiceProvider::new();
    mock_provider.expect_create_function().returning(|_| Ok(created_function()));

    let executor = SingleControllerExecutor::builder()
        .resource(function_resource())
        .controller(AwsFunctionController::default())
        .service_provider(mock_provider)
        .build()
        .await
        .expect("executor should build");

    // Act
    executor.run_until_terminal().await.expect("execution should complete");

    // Assert
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert!(executor.controller().function_arn.is_some());
    assert!(executor.controller().role_arn.is_some());
}
```

---

## Debugging Issues

### Never Apply Workarounds

When a test fails or behavior is unexpected, find the **real root cause**. Don't patch symptoms.

```rust
// Bad: Workaround that hides the real issue
#[tokio::test]
async fn test_sync() {
    let result = sync().await;
    // "Sometimes this fails, just retry"
    let result = if result.is_err() { sync().await } else { result };
    assert!(result.is_ok());
}

// Good: Investigate why it fails sometimes
// - Is there a race condition?
// - Is the mock not set up correctly?
// - Is the timeout too short?
// Fix the actual issue, don't hide it.
```

### Debugging Checklist

1. **Reproduce consistently**: If a test is flaky, make it fail reliably first
2. **Check assumptions**: Verify mocks are configured correctly
3. **Add logging**: Use `tracing` to understand the flow
4. **Isolate the issue**: Create a minimal reproduction
5. **Fix the root cause**: Don't add retries or sleeps as fixes
