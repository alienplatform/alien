# `@alienplatform/commands`

Send commands to an Alien deployment, and receive them inside one. A command is a
named JSON call routed to a single resource in a deployment.

The package is pure TypeScript over `fetch`. No gRPC, no native addon, no Alien
runtime to boot. Its only runtime dependency is `@alienplatform/core`, which
supplies the shared error types and the generated wire schemas. It runs on Node
18 or newer and Bun 1.0.23 or newer, both of which ship a global `fetch`.

There are two halves.

- `CommandsClient` creates a command and waits for its response.
- `createCommandReceiver` leases commands over outbound HTTPS and dispatches
  them to handlers.

The Rust crate `alien-commands` is the protocol twin. It speaks the same wire
format and its receiver reads the same environment variables.

## What it needs

Commands are brokered by the Alien control plane. This package speaks the
protocol. It does not replace the broker. Sending needs a manager URL, a
deployment id, and a bearer token. Get those from Alien with an API key, or run
your own manager and point `managerUrl` at it. The receiver reaches the same
broker through its own environment variables.

## Install

```sh
npm install @alienplatform/commands
```

## Sending

```ts
import { CommandsClient } from "@alienplatform/commands"

const commands = new CommandsClient({
  managerUrl: process.env.ALIEN_MANAGER_URL ?? "",
  deploymentId: process.env.ALIEN_DEPLOYMENT_ID ?? "",
  token: process.env.ALIEN_TOKEN ?? "",
})

const report = await commands.invoke<{ rows: number }>("generate-report", {
  startDate: "2024-01-01",
  endDate: "2024-01-31",
})

console.log(report.rows)
```

`invoke` creates the command, polls until it reaches a terminal state, and
resolves with the decoded success body. The type parameter names the expected
shape. Nothing validates it at runtime. The input is JSON-serialized and sent
inline. Large bodies are promoted to storage-backed presigned transfers by the
server and downloaded transparently on the way back.

### Targeting a resource

A deployment can hold several command-capable resources, and two of them may
register the same command name. `target` binds a sender to one resource id.

```ts
import { CommandsClient } from "@alienplatform/commands"

const commands = new CommandsClient({
  managerUrl: "https://manager.example.com",
  deploymentId: "deployment_123",
  token: "deployment_token",
})

const api = commands.target("api")
const indexer = commands.target("indexer-daemon")

const fromApi = await api.invoke<{ role: string }>("status", {})
const fromIndexer = await indexer.invoke<{ role: string }>("status", {})

console.log(fromApi.role, fromIndexer.role)
```

`target` returns a `TargetedCommands`, which presets `targetResourceId` on every
invoke. If a caller also passes `options.targetResourceId`, the bound target
wins.

### Options

```ts
import { CommandsClient } from "@alienplatform/commands"

const commands = new CommandsClient({
  managerUrl: "https://manager.example.com",
  deploymentId: "deployment_123",
  token: "deployment_token",
  timeoutMs: 30_000,
})

await commands.invoke("generate-report", { month: "2024-01" }, {
  timeoutMs: 120_000,
  idempotencyKey: "report-2024-01",
  targetResourceId: "api",
  pollIntervalMs: 250,
  maxPollIntervalMs: 2_000,
  pollBackoff: 2,
})
```

`CommandsClientConfig`:

| Field | Default | Meaning |
| --- | --- | --- |
| `managerUrl` | required | Base URL of the command server. A query string on the base is preserved. |
| `deploymentId` | required | Deployment the commands are created against. |
| `token` | required | Bearer token. A deployment token or a workspace token. |
| `timeoutMs` | `60000` | Default wall-clock budget for one `invoke`. |
| `allowLocalStorage` | `false` | Allow reading a `local` storage backend response. Local development only. |
| `fetch` | global `fetch` | `fetch` implementation to use. |

`InvokeOptions`:

| Field | Default | Meaning |
| --- | --- | --- |
| `timeoutMs` | client `timeoutMs` | Wall-clock budget for this invoke. |
| `deadline` | none | Server-side `Date` by which the command must complete. |
| `idempotencyKey` | none | The server dedupes retried creates carrying the same key. |
| `targetResourceId` | none | Resource to route to. A `target(...)` sender overrides it. |
| `pollIntervalMs` | `500` | First status-poll interval. |
| `maxPollIntervalMs` | `5000` | Ceiling for the poll backoff. |
| `pollBackoff` | `1.5` | Poll interval multiplier. |

## Receiving

A Worker gets commands pushed to it. A Container or a Daemon cannot accept
inbound connections, so it pulls instead. `createCommandReceiver` runs that pull
loop. It leases commands addressed to its own resource, runs the matching
handler, and submits the response, all over outbound HTTPS.

```ts
import { createCommandReceiver } from "@alienplatform/commands"

const receiver = createCommandReceiver()

receiver.command("status", async () => ({ ok: true, at: new Date().toISOString() }))

receiver.command("search", async (input, ctx) => {
  if (typeof input !== "object" || input === null || !("term" in input)) {
    throw new TypeError("term is required")
  }
  return { term: String(input.term), attempt: ctx.attempt }
})

process.on("SIGTERM", () => receiver.stop())

await receiver.run()
```

`command` parses the payload as JSON and hands it to the handler as `unknown`.
The handler's return value is JSON-encoded and becomes the sender's resolved
value. `run` drives the loop until `stop` is called. `command` and `handleRaw`
both return the receiver, so registrations chain.

### Environment

`createCommandReceiver` reads its configuration from `process.env`, or from the
`env` option if you pass one. Validation is synchronous and fails fast. A
missing, empty, or invalid value throws `CommandReceiverConfigInvalidError`
(code `COMMAND_RECEIVER_CONFIG_INVALID`) naming the offending variable in
`context.envVar`.

Required identity, plus one token source:

| Variable | Meaning |
| --- | --- |
| `ALIEN_COMMANDS_URL` | Base URL of the command server. Must parse as a URL. |
| `ALIEN_COMMANDS_TOKEN` | Bearer token for outbound lease and submit requests. |
| `ALIEN_COMMANDS_TOKEN_FILE` | File holding that token. Supply this or `ALIEN_COMMANDS_TOKEN`. |
| `ALIEN_DEPLOYMENT_ID` | Deployment the leased commands belong to. |
| `ALIEN_COMMANDS_TARGET_RESOURCE_ID` | This resource's id within the deployment. |
| `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` | `container` or `daemon`, lowercase. Anything else is rejected. |

`ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` does not accept `worker`. Worker apps
receive pushed commands and never lease. A receiver will not guess its own type.

At least one token source is required. `ALIEN_COMMANDS_TOKEN` wins when both are
set, and the file is then never read. `ALIEN_COMMANDS_TOKEN_FILE` is re-read once
after a 401, which lets a rotated token take effect without a restart.

Optional tuning:

| Variable | Default | Meaning |
| --- | --- | --- |
| `ALIEN_COMMANDS_POLL_INTERVAL_MS` | `5000` | Lease poll interval. |
| `ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS` | `30000` | Ceiling for the empty-poll backoff. Must be at least the poll interval. |
| `ALIEN_COMMANDS_POLL_JITTER` | `0.1` | Fractional randomization on poll sleeps, from 0 to 1. |
| `ALIEN_COMMANDS_LEASE_SECONDS` | `60` | Requested lease duration. |
| `ALIEN_COMMANDS_MAX_LEASES` | `1` | Commands leased per poll, and therefore run concurrently. |
| `ALIEN_COMMANDS_DRAIN_TIMEOUT_MS` | `30000` | Time in-flight handlers get after `stop`. |

The same values are settable on `createCommandReceiver`, where they win over the
environment.

```ts
import { createCommandReceiver } from "@alienplatform/commands"

const receiver = createCommandReceiver({
  maxLeases: 4,
  leaseSeconds: 120,
  pollIntervalMs: 1_000,
  pollMaxIntervalMs: 15_000,
  pollJitter: 0.2,
  drainTimeoutMs: 10_000,
})

receiver.command("ping", async () => "pong")

await receiver.run()
```

### Raw payload bytes

`ctx.input` is a `Uint8Array` holding the decoded command payload exactly as it
arrived. `command` decodes and JSON-parses it for you. `handleRaw` does not, so
the handler owns the decode.

```ts
import { createCommandReceiver } from "@alienplatform/commands"

const receiver = createCommandReceiver()

receiver.handleRaw("ingest", ctx => {
  const text = new TextDecoder().decode(ctx.input)
  const payload: unknown = JSON.parse(text)
  return { bytes: ctx.input.byteLength, payload, commandId: ctx.commandId }
})

receiver.handleRaw("slow-scan", async ctx => {
  const budgetMs = ctx.deadline.getTime() - Date.now()
  await new Promise(resolve => setTimeout(resolve, 100))
  if (ctx.signal.aborted) {
    throw new Error("budget expired")
  }
  return { budgetMs, target: ctx.target.resourceId }
})

await receiver.run()
```

`CommandContext` carries `input`, `signal`, `deadline`, `commandId`, `attempt`,
`target`, and an optional `traceContext` with the envelope's W3C `traceparent`
and `tracestate`.

### Validated input

Pass a [Standard Schema](https://standardschema.dev) validator between the name
and the handler. Valid input is handed to the handler with the schema's output
type inferred. Invalid input never reaches the handler.

```ts
import { createCommandReceiver } from "@alienplatform/commands"
import * as z from "zod"

const SearchInput = z.object({
  term: z.string(),
  limit: z.number().int().positive().default(10),
})

const receiver = createCommandReceiver()

receiver.command("search", SearchInput, async input => {
  return { term: input.term, limit: input.limit }
})

await receiver.run()
```

The validator is a plain argument, so no schema library is bundled or required.
Zod is shown here because it implements Standard Schema.

### Execution budget and delivery

Each command runs under `min(envelope deadline, lease expiry - 5 seconds)`.
There is no lease-renewal call, so the safety-margined lease expiry always
bounds the budget and leaves room to submit before the lease lapses.
`ctx.deadline` is that effective budget. When it expires, `ctx.signal` fires and
the receiver submits a `HANDLER_TIMEOUT` error response.

Delivery is at-least-once. A lease that expires without a submitted response is
redelivered. `ctx.attempt` starts at 1, and anything higher means a redelivery,
so handlers must tolerate running more than once for the same command.

The receiver submits `UNKNOWN_COMMAND` when no handler is registered for a
leased name, a thrown error's non-empty string `code` when it has one,
`HANDLER_ERROR` otherwise, and `HANDLER_TIMEOUT` on budget expiry.

### Shutdown

`stop` starts a drain. No new lease poll begins, though a poll already in flight
completes and its leases are dispatched. In-flight handlers get
`drainTimeoutMs` to finish. Whatever is still running is then aborted and its
lease released. `run` resolves once the drain finishes. It rejects instead if a
non-retryable `AlienError` ended the loop.

## Errors

Every error is an `AlienError` from `@alienplatform/core`, which the package
re-exports along with `defineError`. Each definition carries a stable string
code. Use `error.code`, or `error.hasErrorCode(...)` to search a wrapped chain.

```ts
import {
  AlienError,
  CommandsClient,
  CommandTimeoutError,
  DeploymentCommandError,
} from "@alienplatform/commands"

const commands = new CommandsClient({
  managerUrl: "https://manager.example.com",
  deploymentId: "deployment_123",
  token: "deployment_token",
})

try {
  await commands.target("api").invoke("generate-report", {})
} catch (error) {
  if (!(error instanceof AlienError)) {
    throw error
  }
  if (error.hasErrorCode(DeploymentCommandError.metadata.code)) {
    console.error("the handler rejected", error.context)
  } else if (error.hasErrorCode(CommandTimeoutError.metadata.code)) {
    console.error("no response in time", error.context)
  } else {
    throw error
  }
}
```

| Export | Code | Raised by |
| --- | --- | --- |
| `CommandCreationFailedError` | `COMMAND_CREATION_FAILED` | Sender, when the create request fails to reach the server. |
| `CommandStatusFailedError` | `COMMAND_STATUS_FAILED` | Sender, when a status poll fails to reach the server. |
| `CommandTimeoutError` | `COMMAND_TIMEOUT` | Sender, when the invoke budget elapses before a terminal state. |
| `CommandExpiredError` | `COMMAND_EXPIRED` | Sender, when the command reaches `EXPIRED`. |
| `DeploymentCommandError` | `DEPLOYMENT_COMMAND_ERROR` | Sender, when the handler returned an error response. |
| `ResponseDecodingFailedError` | `RESPONSE_DECODING_FAILED` | Sender, when a terminal response cannot be decoded. |
| `ManagerHttpError` | `MANAGER_HTTP_ERROR` | Both, on any non-2xx from the command server. |
| `MalformedResponseError` | `MALFORMED_RESPONSE` | Both, when a 2xx body fails its wire schema. |
| `StorageOperationFailedError` | `STORAGE_OPERATION_FAILED` | Both, on a failed presigned upload or download. |
| `InvalidEnvelopeError` | `INVALID_ENVELOPE` | Receiver, on a payload it cannot decode. |
| `CommandReceiverConfigInvalidError` | `COMMAND_RECEIVER_CONFIG_INVALID` | Receiver, on invalid environment configuration. |

`UNKNOWN_COMMAND`, `HANDLER_ERROR`, and `HANDLER_TIMEOUT` are codes the receiver
submits over the wire rather than error classes it exports. A sender sees them
as the `errorCode` context field on a `DeploymentCommandError`.

## Exports

Everything ships from the package root. There are no deep imports.

- Sender: `CommandsClient`, `TargetedCommands`, and the types
  `CommandsClientConfig` and `InvokeOptions`.
- Receiver: `createCommandReceiver`, and the types `CommandReceiver`,
  `CommandReceiverOptions`, `CommandContext`, `CommandHandler`,
  `RawCommandHandler`, `StandardSchema`, and `StandardSchemaOutput`.
- Errors: the eleven definitions above, plus `AlienError` and `defineError`
  re-exported from `@alienplatform/core`.
- Wire protocol types, including `Envelope`, `BodySpec`, `CommandTarget`,
  `CommandTargetType`, `CommandState`, `LeaseRequest`, `LeaseResponse`, and
  `PresignedRequest`.

## Example

[`examples/command-routing-ts`](https://github.com/alienplatform/alien/tree/main/examples/command-routing-ts)
runs both halves against one deployment. A Worker and a Daemon register the same
two command names, the Daemon serves its half with this package's receiver, and a
sender script resolves each name to a different resource with `target`.
