# command-routing-ts

One deployment, two command-capable resources, **the same command names on both** — routed by target.

| Resource         | Kind   | How it serves commands                              | `status` answers |
| ---------------- | ------ | --------------------------------------------------- | ---------------- |
| `api`            | Worker | push — SDK `command()`, invoked over HTTP           | `role: "worker"` |
| `indexer-daemon` | Daemon | pull — `@alienplatform/commands` receiver (leasing) | `role: "daemon"` |

Both register `status` and `search`. Because the names collide, a caller **must** name the target resource. That is the whole point of this example: the command server routes by target resource id, not by command name.

## Layout

- `services/api` — the Worker. Registers `status`/`search` with the SDK and also serves an HTTP app.
- `services/indexer` — the Daemon. Runs the pull receiver (`createCommandReceiver`), reads the shared `index` **kv binding** directly (in-process, no runtime), and keeps it populated in a background loop.
- `services/sender` — a standalone client script (not deployed) that invokes `status` on each target and checks the answers differ.

## Run it

Deploy the stack, then point the sender at it:

```sh
alien dev            # or: alien deploy --platform aws
```

The sender reads the deployment's command endpoint, id, and token from the environment:

```sh
ALIEN_MANAGER_URL=<commands url> \
ALIEN_DEPLOYMENT_ID=<deployment id> \
ALIEN_TOKEN=<deployment token> \
  bun services/sender/src/index.ts
```

Expected output — the same `status` command resolving to two different resources:

```
api            -> { resource: 'api', role: 'worker', model: 'push', documents: 4, ... }
indexer-daemon -> { resource: 'indexer-daemon', role: 'daemon', model: 'pull', documents: 4, ... }
routing verified: overlapping `status` resolved by target
```

## The routing API

```ts
import { CommandsClient } from "@alienplatform/commands"

const client = new CommandsClient({ managerUrl, deploymentId, token })

await client.target("api").invoke("status", {}) //            -> the Worker
await client.target("indexer-daemon").invoke("status", {}) // -> the Daemon
```

From a test using `@alienplatform/testing`, the same routing is expressed with the `target` option:

```ts
const worker = await deployment.invokeCommand("status", {}, { target: "api" })
const daemon = await deployment.invokeCommand("status", {}, { target: "indexer-daemon" })
expect(worker.role).toBe("worker")
expect(daemon.role).toBe("daemon")
```
