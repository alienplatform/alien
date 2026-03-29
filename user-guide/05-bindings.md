# Cloud-Agnostic Bindings

Optional bindings that work on every platform. Use them, or use any existing libraries directly.

```bash
npm install @alienplatform/bindings
```

## Storage

```typescript
import { storage } from "@alienplatform/bindings"

// S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure, filesystem locally
const store = storage("data")

await store.put("reports/2025-01.json", JSON.stringify(report))
const data = await store.get("reports/2025-01.json")
const files = await store.list({ prefix: "reports/" })
```

## KV

```typescript
import { kv } from "@alienplatform/bindings"

// DynamoDB on AWS, Firestore on GCP, Table Storage on Azure, SQLite locally
const cache = kv("cache")

await cache.set("user:123", { name: "Alice", plan: "pro" })
const user = await cache.get("user:123")
await cache.delete("user:123")
```

## Queue

```typescript
import { queue } from "@alienplatform/bindings"

// SQS on AWS, Pub/Sub on GCP, Service Bus on Azure, in-memory locally
const tasks = queue("tasks")

await tasks.send({ type: "process-report", reportId: "abc" })

tasks.subscribe(async (message) => {
  await processReport(message.reportId)
})
```

## AI

```typescript
import { ai } from "@alienplatform/bindings"

// Bedrock on AWS, Vertex AI on GCP, Azure OpenAI on Azure
const result = await ai().generate("Summarize this quarter's sales data")
```

## How It Works

Bindings connect to the resources defined in your `alien.ts` stack. When you declare a `Storage("data")` and grant a permission profile access to it, `storage("data")` in your app code resolves to the correct cloud resource with the correct credentials. No connection strings, no credential management.

## Next

- [Remote Commands](06-commands.md) — invoke code without inbound networking
