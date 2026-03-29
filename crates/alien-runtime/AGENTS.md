# alien-runtime

Application runtime — starts user code with injected bindings, handles request routing across platforms.

## Architecture

The runtime:
1. Starts a gRPC server (bindings + control service)
2. Loads secrets from vault (including commands token)
3. Starts the application as a subprocess
4. Waits for the app to register its HTTP port
5. Starts commands polling (if enabled)
6. Starts the platform-appropriate transport

## Key Files

- `runtime.rs` — Main `run()` function, subprocess management, gRPC server setup
- `traits.rs` — `Request`/`Response` types for transport abstraction
- `config/` — `RuntimeConfig`, `TransportType`, `CommandsPollingConfig`
- `secrets.rs` — Vault secret loading at startup
- `otlp.rs` — OTLP log and trace forwarding
- `tracing_init.rs` — Tracing initialization
- `events/` — Event parsing (storage, cron, queue triggers)
- `transports/` — Platform-specific request routing:
  - `local.rs` — Local development transport
  - `commands_polling.rs` — Pull-based command polling from manager
  - `cloudrun.rs` — Cloud Run HTTP transport
  - `containerapp.rs` — Azure Container Apps transport
  - `lambda.rs` — AWS Lambda transport
- `bin/` — Binary entry points

## Bindings Integration

The runtime creates a `BindingsProvider` (from `alien-bindings`) and exposes it via gRPC. Applications call the gRPC server to access storage, KV, vault, build, and other bindings.

`BindingsSource` enum controls how bindings are obtained:
- `FromEnvironment` — Production: create providers from env vars (cloud platform)
- `Provided` — Dev/test: use pre-built `BindingsProvider`

## Don't

- Don't add business logic here — the runtime is a process host, not a control plane
- Don't import alien-manager types — runtime is independent of the manager
- Don't use "agent" — use "deployment"
- Don't use "ARC" — use "commands"
