# Endpoint Agent Example

A Rust agent that runs on employee devices, monitors system activity, and stores data in encrypted local storage.

**Note:** This example currently uses `Function` with a minimal HTTP health endpoint. The implementation is ready to transition to `Worker` resource once it's available - Workers are designed specifically for endpoint agents and don't require HTTP.

## Overview

This example demonstrates an endpoint security monitoring agent that:

- Monitors filesystem activity (file creation, modification, deletion)
- Detects PII in clipboard and file content
- Stores events in an encrypted SQLite database (Turso with AEGIS-256)
- Exposes monitoring data via commands (no HTTP exposure)
- Updates reliably without MDM conflicts

## Key Features

- **Encrypted Storage**: Uses Turso with AEGIS-256 encryption for data at rest
- **Command-Only Interface**: No HTTP endpoints - all interaction via Alien commands
- **PII Detection**: Basic pattern matching for emails, SSNs, credit cards, phone numbers
- **File Monitoring**: Cross-platform filesystem watching via `notify` crate
- **Hash-Only Clipboard**: Never stores actual clipboard content, only hashes
- **Demo Mode**: Includes simulation commands for testing without real monitoring

## Project Structure

```
endpoint-agent/
├── alien.ts      # Alien stack configuration
├── Cargo.toml           # Rust dependencies
├── src/
│   ├── main.rs          # Entry point and initialization
│   ├── db.rs            # Encrypted Turso database
│   ├── monitor.rs       # File and clipboard monitoring
│   ├── commands.rs      # Command handlers
│   ├── pii.rs           # PII detection patterns
│   └── error.rs         # Error types
└── tests/
    └── endpoint-agent.test.ts  # Integration tests
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `get-events` | Retrieve recent events | `since` (duration like "5m", "1h"), `limit` (optional, default 100) |
| `get-config` | Get current monitoring config | none |
| `scan-path` | Scan directory for sensitive files | `path` (directory path) |
| `simulate-clipboard` | Simulate clipboard write (testing only) | `content` (string) |

## Development

### Prerequisites

- Rust 1.70+
- Node.js 18+
- Alien CLI (`alien` binary available on your `PATH`)

### Build

```bash
cargo build
```

### Test

```bash
npm test
```

The tests use `@alienplatform/testing` with the `dev` deployer for local testing.

### Deploy

```bash
alien deploy --platform local
```

## Usage

### Using Commands Client (TypeScript)

```typescript
import { CommandsClient } from "@alienplatform/commands-client"

const client = new CommandsClient({
  managerUrl: "https://am.example.com",
  deploymentId: "dep_123",
  token: "your_token",
})

// Query recent events
const events = await client.invoke("get-events", {
  since: "5m",
  limit: 10,
})

// Get configuration
const config = await client.invoke("get-config", {})

// Scan directory
const scanResult = await client.invoke("scan-path", {
  path: "/tmp",
})
```

### Using Alien CLI

```bash
# Query events
alien commands invoke get-events --deployment <deployment-id> --params '{"since": "5m", "limit": 10}'

# Get config
alien commands invoke get-config --deployment <deployment-id>

# Scan path
alien commands invoke scan-path --deployment <deployment-id> --params '{"path": "/tmp"}'
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MONITORED_PATHS` | Colon-separated list of paths to monitor | System temp directory |
| `DB_ENCRYPTION_KEY` | 64-character hex key for database encryption | Random (ephemeral) |
| `DATA_DIR` | Directory for database storage | `./.data` |
| `RUST_LOG` | Logging level | `info` |

## Production Considerations

### Encryption Key Management

The example generates an ephemeral encryption key on startup. In production:

- Derive key from TPM/Secure Enclave + Alien-managed secret
- Use proper KDF like PBKDF2 or Argon2
- Store key in platform keychain (macOS Keychain, Windows DPAPI, etc.)

### Clipboard Monitoring

Real clipboard monitoring requires platform-specific APIs:

- **macOS**: NSPasteboard with change count polling
- **Windows**: Win32 clipboard change notifications
- **Linux**: X11 clipboard events or Wayland protocols

The example includes a simulation command for testing.

### Performance

- Add batching for high-frequency events
- Implement event aggregation
- Use background cleanup tasks for old events
- Consider indexing for faster queries

## License

ISC

