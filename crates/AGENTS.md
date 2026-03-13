# Rust Guidelines (Summary)

## Dependencies

Use `workspace = true` for all dependencies defined in the workspace `Cargo.toml`. Never duplicate versions.

## Code Style

- All `use` statements at file top, never scattered
- Consolidate imports: `use crate::{A, B, C}` not separate lines
- Always `#[serde(rename_all = "camelCase")]` for JSON

## Preferred Crates

| Purpose | Use | Avoid |
|---------|-----|-------|
| Time | `chrono` | `time` |
| Logging | `tracing` | |
| Errors | `alien-error` | `thiserror`, `anyhow` |
| Async | `tokio` | |

## Error Handling

Use `alien-error` for structured metadata (`code`, `retryable`, `internal`, `http_status_code`).

**Designing errors**: Name variants after problems, not dependencies. Include `message` field. Use `inherit` for wrapped errors.

```rust
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    #[error(
        code = "RESOURCE_NOT_FOUND",
        message = "Resource '{resource_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ResourceNotFound { resource_id: String },
}
```

**Using errors**:
- New error: `Err(AlienError::new(ErrorData::Variant { ... }))`
- Any `AlienError` (same crate or different): `.context(ErrorData::Variant { ... })?`
- Third-party errors (`std::io::Error`, `serde_json::Error`, etc.): `.into_alien_error().context(ErrorData::Variant { ... })?`

**Key distinction**: `.into_alien_error()` is ONLY for non-Alien errors. For any `AlienError<A>` → `AlienError<B>` conversion (even across crates), just use `.context()`.

## Fail Fast

Return errors immediately. No silent fallbacks, no `warn!` then continue, no hidden retries. Let callers handle retry logic.

```rust
// ❌ Bad
if let Err(e) = operation().await {
    warn!("Failed: {}", e);  // continues!
}

// ✅ Good
operation().await.context(ErrorData::OperationFailed { ... })?;
```

## Testing

We use **nextest** for running tests. If tests need to run serially, add a test group in `.config/nextest.toml`.

Tests must be strict—assert everything, fail on any unexpected outcome.

```rust
// ❌ Bad: swallows errors
let _ = operation().await.ok();
let result = operation().await.unwrap_or_default();

// ✅ Good: explicit assertions
let result = operation().await.expect("should succeed");
assert_eq!(result.status, "ready");
```

## Debugging

Find root causes. No workarounds, no retry-until-pass, no added sleeps.

