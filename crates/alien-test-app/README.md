# alien-test-app

Minimal test application for testing the Alien runtime, local development, and build systems.

## Purpose

This is a lightweight Rust application used for testing core Alien functionality in:
- `alien-runtime` - Testing transport event routing (HTTP forwarding, event handling, ARC protocol)
- `alien-local` - Testing function lifecycle (extract, start, stop, health checks)
- `dockdash` - Testing OCI image building and registry operations

## Features

### HTTP Endpoints

- `GET /health` - Health check endpoint
- `POST /inspect` - Request inspection (echoes back received data)
- `GET /events/storage/{key}` - Retrieve stored storage event for verification
- `GET /events/queue/{message_id}` - Retrieve stored queue message for verification

### Event Handlers

- **Storage events** - Listens to all storage events and stores them in KV for verification
- **Queue messages** - Processes queue messages and stores them in KV for verification

### ARC Commands

- `arc-test-small` - Tests ARC protocol with small inline responses
- `arc-test-large` - Tests ARC protocol with large responses (>48KB, triggers storage mode)

### Bindings

- **KV** (`test-kv`) - For storing event verification data
- **Storage** (`test-storage`) - For testing storage operations and events
- **Queue** (`test-queue`) - For testing queue operations

## Differences from alien-test-server

The `alien-test-server` is a comprehensive test application with all features for E2E testing:
- All bindings (Storage, KV, Queue, Vault, Build, ArtifactRegistry)
- All event types (Storage, Queue, Cron)
- Complex HTTP endpoints for testing each binding
- SSE, wait_until, and other advanced features

The `alien-test-app` is intentionally minimal (~300 LOC vs 1000+ LOC):
- Only Storage and KV bindings
- Only Storage and Queue event handlers (no Cron)
- Basic HTTP endpoints for health checks and event verification
- Focused on runtime testing, not comprehensive feature testing

## Building

```bash
cd crates/alien-test-app
cargo build --release
```

## Running Locally

```bash
PORT=8080 cargo run --release
```

## Usage in Tests

The test app is used in:

1. **Runtime tests** (`alien-runtime/tests/`) - Testing transport-level functionality
2. **Local tests** (`alien-local/tests/`) - Testing local function lifecycle
3. **Dockdash tests** (`dockdash/tests/`) - Testing OCI image building

For comprehensive E2E testing of all features, see `tests/e2e/test-apps/comprehensive-rust/`.



