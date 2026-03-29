# TypeScript SDK

The TypeScript SDK uses **global convenience functions** and a **bootstrap layer** that handles initialization automatically.

### HTTP Server

Export a fetch handler. The bootstrap starts it on a random port and registers with the runtime.

```typescript
import { Hono } from "hono"

const app = new Hono()
app.get("/", (c) => c.text("Hello!"))

export default app
```

Works with any framework that exposes a `fetch` function (Hono, Express via adapter, etc.).

### Bindings

Import and use directly. Connects lazily on first call.

```typescript
import { storage, kv } from "@alienplatform/sdk"

const bucket = storage("my-bucket")
await bucket.put("key", data)

const cache = kv("my-cache")
await cache.get("key")
```

### Events

Register handlers at module scope. Fire-and-forget.

```typescript
import { onStorageEvent, onCronEvent, onQueueMessage } from "@alienplatform/sdk"

onStorageEvent("uploads", async (event) => {
  console.log(`File ${event.key} was ${event.type}`)
})

onCronEvent("daily-cleanup", async () => {
  await cleanupOldFiles()
})

onQueueMessage("tasks", async (message) => {
  await processTask(message.payload)
})
```

### Commands

Register handlers. Request-response pattern.

```typescript
import { command } from "@alienplatform/sdk"

command("health-check", async () => {
  return { healthy: true }
})

command<{ reportType: string }>("generate-report", async (params) => {
  return { url: await generateReport(params.reportType) }
})
```

### waitUntil

Run background work without blocking the response (serverless functions).

```typescript
import { waitUntil } from "@alienplatform/sdk"

app.post("/process", async (c) => {
  waitUntil(async () => {
    await heavyProcessing()
  })
  return c.json({ accepted: true })
})
```

### AlienContext (Advanced)

For explicit control or remote bindings:

```typescript
import { AlienContext } from "@alienplatform/sdk"

// Explicit context
const ctx = await AlienContext.fromEnv()
const bucket = ctx.storage("my-bucket")

// Remote bindings (access resources from outside the runtime)
const remoteCtx = await AlienContext.forRemoteDeployment(deploymentId, token)
const customerBucket = remoteCtx.storage("customer-data")
```

### How It Works

The build process compiles the application into a single executable using `bun build --compile`. The bootstrap is bundled in:

```dockerfile
ENTRYPOINT ["alien-runtime", "--"]
CMD ["./app"]
```

At runtime, the compiled executable:
1. Imports the user module (triggers handler registrations)
2. Detects `default` export → starts `Bun.serve()` → registers port with runtime
3. Enters `WaitForEvents()` loop → dispatches events/commands to local handlers

See `00-foundations/02-build.md` for build details.

---
