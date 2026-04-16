# alien-commands

Remote commands protocol — lets you invoke code on deployments without requiring inbound network connections. This is the primary way a control plane communicates with customer deployments.

Use cases: AI agents sending tool calls to workers in a customer's VPC, data connectors running queries against private databases, dashboards pulling real-time metrics — all without open ports, VPNs, or firewall changes.

## How It Works

1. **Create** — Client invokes a command via CLI or SDK. Server stores params in KV (auto-promotes to blob storage if oversized).
2. **Dispatch** — Push: direct invocation via Lambda, Pub/Sub, or Service Bus. Pull: deployment polls via lease.
3. **Lease** — Deployment acquires a lease, receives an `Envelope` with params and response instructions.
4. **Response** — Deployment submits response (inline or storage-uploaded). Registry transitions to terminal state.

Payloads are transparently handled — clients never worry about size limits. Small payloads go inline, large ones auto-promote to presigned storage URLs.

## Architecture

Three feature-gated modules:

- **Core types** (always available) — `Envelope`, `CommandResponse`, `BodySpec`, protocol constants
- **`server` feature** — `CommandServer`, `CommandRegistry`, Axum handlers, dispatchers. Used by alien-manager.
- **`runtime` feature** — Envelope decoding, response submission. Used by alien-runtime.

## Key Types

- `CommandServer` — Orchestrates the full command lifecycle
- `CommandRegistry` trait — Source of truth for command metadata (state, timestamps, attempts)
- `CommandDispatcher` trait — Push-model transports (Lambda invoke, Pub/Sub message, Service Bus message)
- `Envelope` — Wire format sent to deployments
- `BodySpec` — Inline (base64) or storage-backed (presigned URL) payload
