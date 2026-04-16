# alien-error

Structured error library for the Alien platform. Every error carries machine-readable metadata: `code`, `retryable`, `internal`, and `http_status_code`.

## Why Not thiserror or anyhow?

- **Structured metadata** — errors carry code, retryable, internal, and HTTP status fields for API responses and retry logic
- **Error chaining with context** — like `anyhow`, but each layer preserves structured metadata
- **API-ready** — errors serialize to JSON; the `internal` flag controls whether sensitive details reach external clients
- **Inheritance** — `retryable` and `internal` can inherit from source errors

## Core Types

- `AlienErrorData` trait — metadata interface for error enums
- `AlienError<T>` — generic error container with source chain, context, hints
- `Context` / `IntoAlienError` — extension traits for error propagation

## Features

- `openapi` — OpenAPI schema generation
- `axum` — `IntoResponse` with `into_internal_response()` / `into_external_response()`
- `anyhow` — Interop with the `anyhow` crate

See the workspace [CLAUDE.md](../../CLAUDE.md) for detailed error design and usage guidelines.
