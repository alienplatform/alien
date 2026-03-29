# alien-commands

Transport-agnostic command protocol for sending commands to deployments without requiring inbound connections. Formerly called "ARC."

## Architecture

Three feature-gated modules:

- **Core types** (always available) — `types.rs`, protocol constants, `Envelope`, `CommandResponse`, `BodySpec`
- **`server` feature** — `CommandServer`, `CommandRegistry`, Axum handlers, dispatchers. Used by alien-manager.
- **`runtime` feature** — Envelope decoding, response submission. Used by alien-runtime.

## Key Types

- `CommandServer` — Orchestrates the full command lifecycle: create, dispatch, lease, response
- `CommandRegistry` trait — Source of truth for command metadata (state, timestamps, attempts)
- `InMemoryCommandRegistry` — Default in-process registry
- `CommandDispatcher` trait — Push-model transport (e.g., notify deployment of new command)
- `Envelope` — Wire format sent to deployments, contains command name, params, response handling info
- `BodySpec` — Inline (base64) or storage-backed (presigned URL) payload

## Command Lifecycle

1. **Create** — Registry stores metadata, KV stores params blob
2. **Dispatch** — Push: dispatcher notifies deployment. Pull: pending index in KV, deployment polls via lease
3. **Lease** — Deployment acquires a lease, receives envelope with params + response handling
4. **Response** — Deployment submits response (inline or storage-uploaded), registry transitions to terminal state

## Key Files

- `server/mod.rs` — `CommandServer` implementation
- `server/command_registry.rs` — `CommandRegistry` trait + `InMemoryCommandRegistry`
- `server/axum_handlers.rs` — `/v1/commands/*` HTTP routes
- `server/dispatchers.rs` — `CommandDispatcher` trait + `NullCommandDispatcher`
- `runtime/mod.rs` — `parse_envelope`, `submit_response`, `decode_params`
- `types.rs` — Re-exports from `alien-core::commands_types`

## Don't

- Don't call it "ARC" — use "commands" everywhere
- Don't put transport-specific logic in core types — use the dispatcher trait
- Don't bypass the `CommandRegistry` — it is the source of truth for state, not KV
