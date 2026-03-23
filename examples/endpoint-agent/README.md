# Endpoint Agent Example

A Rust agent that runs on employee devices, monitors system activity, and stores data in encrypted local storage.

**Note:** This example currently uses `Function` with a minimal HTTP health endpoint. The implementation is ready to transition to `Worker` resource once it's available - Workers are designed specifically for endpoint agents and don't require HTTP.

## Overview

This example demonstrates an endpoint security monitoring agent that:

- Monitors filesystem activity (file creation, modification, deletion)
- Detects PII in clipboard and file content
- Stores events in an encrypted SQLite database (Turso with AEGIS-256)
- Exposes monitoring data via ARC commands (no HTTP exposure)
- Updates reliably without MDM conflicts

## Key Features

- **Encrypted Storage**: Uses Turso with AEGIS-256 encryption for data at rest
- **ARC-Only Interface**: No HTTP endpoints - all commands via Alien Remote Call protocol
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
│   ├── commands.rs      # ARC command handlers
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

### Using ARC Client (TypeScript)

```typescript
import { ArcClient } from "@alienplatform/arc-client"

const arc = new ArcClient({
  managerUrl: "https://am.example.com",
  agentId: "agent_123",
  token: "your_token",
})

// Query recent events
const events = await arc.invoke("get-events", {
  since: "5m",
  limit: 10,
})

// Get configuration
const config = await arc.invoke("get-config", {})

// Scan directory
const scanResult = await arc.invoke("scan-path", {
  path: "/tmp",
})
```

### Using Alien CLI

```bash
# Query events
alien arc invoke get-events --agent <agent-id> --params '{"since": "5m", "limit": 10}'

# Get config
alien arc invoke get-config --agent <agent-id>

# Scan path
alien arc invoke scan-path --agent <agent-id> --params '{"path": "/tmp"}'
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

