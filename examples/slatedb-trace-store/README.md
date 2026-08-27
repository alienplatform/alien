# SlateDB trace store

This example deploys durable AI trace history into one customer's cloud. A horizontally scalable HTTP API accepts and queries traces. One background writer commits them to [SlateDB](https://slatedb.io), backed by the customer's object storage.

The deployment is the tenant boundary. There is intentionally no organization ID in the API, queue messages, or storage keys. Deploy another stack for another customer.

## Architecture

| Resource | Lifecycle | Purpose |
| --- | --- | --- |
| `data` Storage | Frozen | SlateDB files, staged traces, and rejected-ingestion metadata |
| `ingestion` Queue | Frozen | Durable handoff from API replicas to the writer |
| `api` Container | Live, 2–4 replicas | `POST` and indexed `GET` requests |
| `writer` Container | Live, 1 replica | The single SlateDB writer and queue consumer |

The API returns `202 Accepted` after the canonical trace object and queue pointer are durable. The trace becomes queryable after the writer commits it and a reader observes the latest SlateDB manifest and WAL. Readers poll once per second.

## Run locally

```bash
alien dev
```

Submit a trace:

```bash
curl -i http://localhost:8080/v1/traces \
  -H 'content-type: application/json' \
  -d '{
    "traceId": "run-01",
    "agent": "researcher",
    "status": "completed",
    "model": "claude-sonnet",
    "startedAt": "2026-08-26T18:00:00Z",
    "finishedAt": "2026-08-26T18:00:04Z",
    "payload": {"events": [{"type": "tool", "name": "search"}]}
  }'
```

Read it after it becomes visible:

```bash
curl http://localhost:8080/v1/traces/run-01
curl 'http://localhost:8080/v1/traces?agent=researcher&status=completed&limit=25'
```

Run the behavior tests:

```bash
cargo nextest run -p slatedb-trace-store
pnpm test
```

See the [complete guide](https://alien.dev/docs/examples/trace-history) for the data model, guarantees, failure behavior, and production integration points.
