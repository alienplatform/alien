/**
 * `@alienplatform/sdk` — the ergonomic facade for Alien Worker apps.
 *
 * It provides the Worker handler APIs (`command`, `onStorageEvent`,
 * `onCronEvent`, `onQueueMessage`, `waitUntil`) and re-exports the app-facing
 * binding factories (`storage`, `kv`, `queue`, `vault`, `container`, `postgres`,
 * `sandbox`) from
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

export type {
  CronEvent,
  QueueMessage,
  QueueMessageEvent,
  ScheduledEvent,
  StandardSchema,
  StandardSchemaOutput,
  StorageEvent,
  StorageEventType,
  WorkerCommandContext,
} from "./worker-runtime/registry.js"
export {
  command,
  onCronEvent,
  onQueueMessage,
  onStorageEvent,
  waitUntil,
} from "./worker-runtime/registry.js"

// ============================================================================
// Binding factories — re-exported from @alienplatform/bindings
// ============================================================================

export type {
  CommandFrame,
  Container,
  Kv,
  Postgres,
  PostgresConnection,
  PostgresSslMode,
  Queue,
  RunCommandOptions,
  Sandbox,
  SandboxSession,
  Storage,
  Vault,
} from "@alienplatform/bindings"
export { container, kv, postgres, queue, sandbox, storage, vault } from "@alienplatform/bindings"

// ============================================================================
// AI: re-exported from @alienplatform/ai-gateway (a spawned Rust gateway process)
// ============================================================================

export type {
  AiBinding,
  AiConnection,
  AiModel,
  AmbientAiBinding,
  ChatCompletionCreateParams,
  ExternalAiBinding,
  ResponseCreateParams,
} from "@alienplatform/ai-gateway"
export {
  Ai,
  ai,
  getAiConnection,
  isExternalAiBinding,
  parseAiBinding,
  startAiGateway,
} from "@alienplatform/ai-gateway"

// ============================================================================
// Errors — re-exported from @alienplatform/bindings and @alienplatform/core
// ============================================================================

export { AiTransportError, AiUpstreamError } from "@alienplatform/ai-gateway"
export { BindingNotConfiguredError } from "@alienplatform/bindings"
export { AlienError, BindingNotFoundError, InvalidBindingConfigError } from "@alienplatform/core"
