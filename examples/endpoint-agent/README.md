# Endpoint Agent

A Rust daemon that runs on employee devices, monitors system activity, and stores events in encrypted local storage. It has no HTTP endpoints at all -- all interaction happens over Alien commands, and you push updates through Alien releases without MDM conflicts.

The daemon:

- Monitors filesystem activity (file creation, modification, deletion)
- Detects PII in clipboard and file content (emails, SSNs, credit cards, phone numbers)
- Stores events in an encrypted SQLite database (Turso with AEGIS-256)
- Never stores actual clipboard content, only hashes
- Includes simulation commands for testing without real monitoring

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `agent` | Daemon (live) | Long-running Rust process with commands enabled |
| `events` | Storage (frozen) | Durable storage linked to the daemon |

### Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `get-events` | Retrieve recent events | `since` (duration like "5m", "1h"), `limit` (optional, default 100) |
| `get-config` | Get current monitoring config | none |
| `scan-path` | Scan a directory for sensitive files | `path` (directory path) |
| `simulate-clipboard` | Simulate a clipboard write (testing only) | `content` (string) |

## Local development

Requires Rust 1.70+, Node.js 18+, and the [Alien CLI](https://alien.dev/docs/quickstart).

```bash
git clone https://github.com/alienplatform/alien
cd alien/examples/endpoint-agent

cargo build
alien deploy --platform local
```

### Send a command

```bash
# Query recent events
alien commands invoke get-events --deployment <deployment-id> --params '{"since": "5m", "limit": 10}'

# Get config
alien commands invoke get-config --deployment <deployment-id>

# Scan a path
alien commands invoke scan-path --deployment <deployment-id> --params '{"path": "/tmp"}'
```

### From TypeScript

```typescript
import { CommandsClient } from "@alienplatform/sdk/commands"

const client = new CommandsClient({
  managerUrl: "https://am.example.com",
  deploymentId: "dep_123",
  token: "your_token",
})

const events = await client.invoke("get-events", { since: "5m", limit: 10 })
const config = await client.invoke("get-config", {})
const scanResult = await client.invoke("scan-path", { path: "/tmp" })
```

## Running tests

```bash
npm test
```

The tests use `@alienplatform/testing` with the `dev` deployer for local testing.

## Environment variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MONITORED_PATHS` | Colon-separated list of paths to monitor | System temp directory |
| `DB_ENCRYPTION_KEY` | 64-character hex key for database encryption | Random (ephemeral) |
| `DATA_DIR` | Directory for database storage | `./.data` |
| `RUST_LOG` | Logging level | `info` |

## Production considerations

This example simplifies several things for clarity:

- **Encryption key management**: the example generates an ephemeral key on startup. Production would derive the key from TPM/Secure Enclave plus an Alien-managed secret, use a proper KDF (PBKDF2, Argon2), and store it in the platform keychain (macOS Keychain, Windows DPAPI).
- **Clipboard monitoring**: real clipboard monitoring requires platform-specific APIs (NSPasteboard on macOS, Win32 change notifications on Windows, X11/Wayland events on Linux). The example includes a simulation command instead.
- **Performance**: production would batch high-frequency events, aggregate, index for faster queries, and clean up old events in the background.

## Learn more

- [Remote Commands](https://alien.dev/docs/commands)
- [Local Development](https://alien.dev/docs/local-development)
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
