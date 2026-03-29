# Telemetry

Deployments send OTLP logs, traces, and metrics to alien-manager. alien-manager forwards everything to your observability backend — Grafana, Datadog, Elastic, or anything that speaks OTLP.

```
Deployment (remote)              alien-manager                   Your Observability Backend
     │                               │                              │
     │── POST /v1/logs ────────────▶│── forward ──────────────────▶│  Grafana / Datadog /
     │── POST /v1/traces ─────────▶│── forward ──────────────────▶│  Elastic / etc.
     │── POST /v1/metrics ────────▶│── forward ──────────────────▶│
     │   (OTLP Protobuf)            │                              │
```

alien-manager does not store, search, or index telemetry data. It's a stateless forwarding proxy that adds scope metadata and passes data through.

## How Deployments Send Telemetry

The deployment loop injects standard OpenTelemetry environment variables into each deployment's containers:

```bash
OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://server:8080/v1/logs
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://server:8080/v1/traces
OTEL_EXPORTER_OTLP_METRICS_ENDPOINT=http://server:8080/v1/metrics
OTEL_EXPORTER_OTLP_HEADERS=authorization=Bearer ax_deploy_...
```

Any OTLP-compatible SDK or collector picks these up automatically. The deployment doesn't need to know where its telemetry ultimately goes — it sends to alien-manager, which handles routing.

## Ingestion Endpoints

```
POST /v1/logs     — OTLP log data (Protobuf)
POST /v1/traces   — OTLP trace data (Protobuf)
POST /v1/metrics  — OTLP metric data (Protobuf)
```

All three accept standard [OTLP/HTTP](https://opentelemetry.io/docs/specs/otlp/#otlphttp) Protobuf payloads and return `{ "accepted": true }`.

## Scope Tagging

The Bearer token in the request determines the scope tag added to forwarded data. This lets you filter by deployment in your observability backend.

When alien-manager forwards telemetry to `OTLP_ENDPOINT`, it adds scope metadata derived from the token's identity — the deployment ID and deployment group. Your observability backend can use this to build per-deployment dashboards, alerts, and queries.

## Configuration

```bash
OTLP_ENDPOINT=http://grafana-alloy:4318  # Forward all telemetry here
```

If `OTLP_ENDPOINT` is not set, telemetry is accepted but discarded. This is useful for testing or when you don't need observability.

## TelemetryBackend Trait

```rust
#[async_trait]
pub trait TelemetryBackend: Send + Sync {
    async fn ingest_logs(&self, scope: &str, data: Bytes) -> Result<()>;
    async fn ingest_traces(&self, scope: &str, data: Bytes) -> Result<()>;
    async fn ingest_metrics(&self, scope: &str, data: Bytes) -> Result<()>;
}
```

Two implementations:

| Implementation | Used In | Behavior |
|---|---|---|
| `OtlpForwardingBackend` | Standalone | Forwards raw OTLP to `OTLP_ENDPOINT` with scope headers |
| `InMemoryTelemetryBackend` | `alien dev` | Stores logs in memory for the plain local CLI flow and local tooling. See [Local Development](08-local-development.md) |

When running in a managed platform context, a custom `TelemetryBackend` implementation can proxy telemetry to a dedicated observability pipeline with tenant-scoped headers.

## Local Dev

In `alien dev`, the CLI wires `InMemoryTelemetryBackend` explicitly. Instead of sending telemetry to an external endpoint, logs are stored in an in-memory ring buffer for the local development flow. See [Local Development](08-local-development.md).
