/**
 * `@alienplatform/sdk` — the ergonomic facade for Alien Worker apps.
 *
 * It provides the Worker handler APIs (`command`, `onStorageEvent`,
 * `onCronEvent`, `onQueueMessage`, `waitUntil`) and re-exports the app-facing
 * binding factories (`storage`, `kv`, `queue`, `vault`) from
 * `@alienplatform/bindings`, so a Worker author installs one package.
 *
 * Worker protocol dependencies (nice-grpc, generated Worker protocol clients)
 * are confined to the `@alienplatform/sdk/worker-runtime` subpath; the runtime
 * bootstrap `runWorker` lives there. Importing and constructing bindings from
 * this facade does no I/O and needs no deployment or credentials.
 *
 * @example
 * ```typescript
 * import { command, kv } from "@alienplatform/sdk"
 * import { z } from "zod"
 *
 * command("greet", z.object({ name: z.string() }), async ({ name }) => {
 *   const store = kv("greetings")
 *   await store.set(name, `Hello, ${name}!`)
 *   return { ok: true }
 * })
 * ```
 *
 * @packageDocumentation
 */

// ============================================================================
// Worker handler APIs (protocol-only registrars; the runtime wiring lives
// behind ./worker-runtime)
// ============================================================================

export {
  command,
  onStorageEvent,
  onCronEvent,
  onQueueMessage,
  waitUntil,
} from "./worker-runtime/registry.js"

export type {
  StorageEvent,
  StorageEventType,
  CronEvent,
  QueueMessage,
  QueueMessageEvent,
  ScheduledEvent,
  StandardSchema,
  StandardSchemaOutput,
  WorkerCommandContext,
} from "./worker-runtime/registry.js"

// ============================================================================
// Binding factories — re-exported from @alienplatform/bindings
// ============================================================================

export { storage, kv, queue, vault, container } from "@alienplatform/bindings"
export type { Storage, Kv, Queue, Vault, Container } from "@alienplatform/bindings"

// ============================================================================
// Errors — re-exported from @alienplatform/bindings and @alienplatform/core
// ============================================================================

export { BindingNotConfiguredError } from "@alienplatform/bindings"
export { AlienError } from "@alienplatform/core"
