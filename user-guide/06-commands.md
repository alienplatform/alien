# Remote Commands

Invoke code on remote deployments. Zero inbound networking. Zero open ports. Works across all platforms.

## Why Commands?

Your software runs in the customer's cloud. You can't SSH in. You can't call an API endpoint — it might be behind a firewall, or there might not be one. But you need to generate reports, run migrations, trigger syncs, debug issues.

Commands solve this. Define handlers in your deployed code, invoke them from anywhere.

## Define a Handler

```typescript
import { command } from "@alienplatform/bindings"

command("generate-report", async ({ startDate, endDate }) => {
  const data = await fetchData(startDate, endDate)
  return { report: aggregate(data), rowCount: data.length }
})

command("run-migration", async ({ version }) => {
  await migrate(version)
  return { status: "completed" }
})
```

## Invoke from Your App

```typescript
import { CommandsClient } from "@alienplatform/commands-client"

const commands = new CommandsClient({
  deploymentId: "deployment_123",
  token: "your_api_key",
})

const result = await commands.invoke("generate-report", {
  startDate: "2025-01-01",
  endDate: "2025-03-31",
})
```

## Invoke from the CLI

```bash
alien commands invoke \
  --deployment acme-corp \
  --command generate-report \
  --params '{"startDate": "2025-01-01"}'
```

## How It Works

1. You invoke a command (via SDK or CLI)
2. The manager stores the request payload in the developer's storage
3. **Push model**: dispatched to the customer's cloud (Lambda invoke, Pub/Sub message, Service Bus message)
4. **Pull model**: the agent picks it up on the next sync cycle
5. The handler runs, produces a response
6. You read the result

## Next

- [Managing Deployments](07-deployments.md) — releases, updates, teardowns, telemetry
