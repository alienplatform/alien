/**
 * `@alienplatform/sdk/worker-runtime` — the Worker protocol runtime.
 *
 * This subpath is the ONLY home for `nice-grpc` and the generated Worker
 * protocol clients. It exports {@link runWorker}, the ~10-line bootstrap entry
 * that generated Worker bootstraps call, plus the protocol internals those
 * bootstraps build against.
 *
 * `runWorker` connects to the runtime over the Worker protocol
 * (`ALIEN_WORKER_GRPC_ADDRESS`), serves the app's HTTP handler (if any) and
 * registers its port, then registers the app's handlers and dispatches Worker
 * tasks to them. Each `waitUntil` background task is reported to the runtime as
 * it is registered (graceful drain-on-shutdown is a planned future feature).
 */

import { AlienError } from "@alienplatform/core"
import { createClient } from "nice-grpc"
import { getOrCreateChannel } from "./channel.js"
import { MissingEnvVarError } from "./errors.js"
import { EventLoop } from "./event-loop.js"
import { ControlServiceDefinition } from "./generated/control.js"
import { wrapGrpcCall } from "./grpc-utils.js"
import { WaitUntilManager } from "./wait-until-manager.js"

// Minimal ambient declaration for the Bun runtime global. Worker binaries run
// under Bun; the SDK is type-checked by tsc, which has no Bun types. We use
// only `Bun.serve`, so declare just that.
declare const Bun: {
  serve(options: {
    fetch: (request: Request) => Response | Promise<Response>
    hostname?: string
    port?: number
    idleTimeout?: number
  }): { port: number }
}

/** Instance ID for this worker process. */
function generateInstanceId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 11)}`
}

/**
 * Extract a `fetch` handler from the app's default export, if present.
 * Recognizes the `{ fetch }` shape used by Hono, Elysia, Express adapters, etc.
 */
function resolveFetchHandler(
  app: unknown,
): ((request: Request) => Response | Promise<Response>) | undefined {
  const defaultExport =
    app && typeof app === "object" && "default" in app ? (app as { default: unknown }).default : app
  if (!defaultExport || typeof defaultExport !== "object" || !("fetch" in defaultExport)) {
    return undefined
  }
  const fetchHandler = (defaultExport as { fetch: unknown }).fetch
  if (typeof fetchHandler !== "function") return undefined
  return (fetchHandler as (request: Request) => Response | Promise<Response>).bind(defaultExport)
}

/**
 * Run a Worker application.
 *
 * The generated Worker bootstrap imports the user module (which registers
 * handlers via `command`/`onStorageEvent`/… as a side effect) and passes its
 * default export here. `runWorker`:
 *
 * 1. Connects to the runtime over the Worker protocol (`ALIEN_WORKER_GRPC_ADDRESS`).
 * 2. Serves the app's HTTP `fetch` handler (or a minimal readiness server) and
 *    registers the port with the runtime.
 * 3. Registers the app's handlers and enters the task-dispatch loop, keeping the
 *    process alive and reporting `waitUntil` background tasks to the runtime as
 *    they are registered.
 *
 * @param app - The user module's default export (an object with a `fetch`
 *   method for HTTP apps), or `undefined` for handler-only Workers.
 */
export async function runWorker(app?: unknown): Promise<void> {
  const address = process.env.ALIEN_WORKER_GRPC_ADDRESS
  if (!address) {
    throw new AlienError(
      MissingEnvVarError.create({
        variable: "ALIEN_WORKER_GRPC_ADDRESS",
        description:
          "This variable is set by alien-worker-runtime when running inside the Alien environment.",
      }),
    )
  }

  const channel = await getOrCreateChannel(address)
  const instanceId = generateInstanceId()

  // Serve the HTTP handler (or a minimal readiness server) and register the
  // port. Workers always listen on loopback with a dynamic port:
  // alien-worker-runtime is co-located (same container or host), proxies all
  // external traffic, and learns the port via RegisterHttpServer. Nothing else
  // may reach this server, so 127.0.0.1 is the only correct interface.
  const fetchHandler = resolveFetchHandler(app)
  const server = Bun.serve({
    // No HTTP framework — a minimal server so the runtime can probe readiness
    // and route health checks. Commands and events are delivered over gRPC.
    fetch: fetchHandler ?? (() => new Response("ok")),
    hostname: "127.0.0.1",
    port: 0,
    idleTimeout: 255,
  })

  await registerHttpServer(channel, server.port)

  // Report each waitUntil background task to the runtime as it is registered.
  // Graceful drain-on-shutdown (waiting for tracked tasks before exit) is a
  // planned future feature — see wait-until-manager.ts.
  const waitUntilManager = new WaitUntilManager(channel, instanceId)
  waitUntilManager.install()

  // Register handlers and enter the dispatch loop (runs until the process exits).
  const eventLoop = new EventLoop(channel, instanceId)
  await eventLoop.registerHandlers()
  await eventLoop.start()
}

/**
 * Register the HTTP server's port with the runtime's control plane.
 */
async function registerHttpServer(
  channel: Awaited<ReturnType<typeof getOrCreateChannel>>,
  port: number,
): Promise<void> {
  const client = createClient(ControlServiceDefinition, channel)
  await wrapGrpcCall("ControlService", "RegisterHttpServer", async () => {
    await client.registerHttpServer({ port })
  })
}
