# Application Capabilities

When you deploy a Function or Container built from source, the image includes `alien-runtime`. This gives your application a set of capabilities that work identically across AWS, GCP, Azure, Kubernetes, and local machines.

## Bindings

Access cloud resources without platform-specific code:

```typescript
import { storage, kv, queue } from "@alienplatform/bindings"

await storage("uploads").put("report.json", data)
const value = await kv("cache").get("session-123")
await queue("tasks").send({ action: "process", id: 42 })
```

Same code runs on S3, Cloud Storage, Blob Storage, MinIO, or local filesystem — depending on where the deployment runs.

## Events

React to things happening in the remote environment:

```typescript
import { onStorageEvent, onQueueMessage, onCronEvent } from "@alienplatform/bindings"

onStorageEvent("uploads", async (event) => {
  // File was uploaded to the storage bucket
})

onQueueMessage("tasks", async (message) => {
  // Message arrived in the queue
})

onCronEvent("daily-cleanup", async () => {
  // Runs on schedule
})
```

## Commands

Receive remote commands from the control plane:

```typescript
import { command } from "@alienplatform/bindings"

command("generate-report", async (params) => {
  const report = await buildReport(params.startDate, params.endDate)
  return { url: report.url }
})
```

The control plane sends a command; your app processes it and returns a response. Works even when inbound networking is blocked.

## waitUntil

Run background work that completes before the function shuts down:

```typescript
import { waitUntil } from "@alienplatform/bindings"

app.post("/ingest", async (c) => {
  waitUntil(async () => {
    await sendAnalytics(c.req)
  })
  return c.json({ accepted: true })
})
```

## Not Required

`alien-runtime` is optional. You can deploy a pre-built container image (e.g. `postgres:16`, a custom Docker image) without it. Those containers run normally — they just don't have bindings, events, commands, or any of the capabilities above.

## Language Support

TypeScript, Rust, and Python are supported. Each language uses idiomatic patterns — TypeScript uses global functions, Rust uses explicit context, Python uses decorators. Full per-language reference in [08-sdk/](../08-sdk/).
