/**
 * Global convenience functions for Alien bindings.
 *
 * These functions use a singleton AlienContext for easy access.
 */

import type { ArtifactRegistry } from "./bindings/artifact-registry.js"
import type { Build } from "./bindings/build.js"
import type { FunctionBinding } from "./bindings/function.js"
import type { Kv } from "./bindings/kv.js"
import type { Queue } from "./bindings/queue.js"
import type { ServiceAccount } from "./bindings/service-account.js"
import type { Storage } from "./bindings/storage.js"
import type { Vault } from "./bindings/vault.js"

import { AlienContext } from "./context.js"
import {
  type CronEvent,
  type QueueMessageEvent,
  type StorageEvent,
  onCronEvent as onCronEventImpl,
  onQueueMessage as onQueueMessageImpl,
  onStorageEvent as onStorageEventImpl,
} from "./events.js"
import { waitUntil as waitUntilImpl } from "./wait-until.js"

// Lazy-initialized global context
let globalContext: AlienContext | undefined

/**
 * Get or create the global AlienContext.
 */
async function getGlobalContext(): Promise<AlienContext> {
  if (!globalContext) {
    globalContext = await AlienContext.fromEnv()
  }
  return globalContext
}

// ============================================================================
// Binding Accessors
// ============================================================================

/**
 * Get a storage binding.
 *
 * @param name - Binding name
 * @returns Storage binding instance
 *
 * @example
 * ```typescript
 * import { storage } from "@alienplatform/bindings"
 *
 * const bucket = await storage("my-bucket")
 * await bucket.put("hello.txt", "Hello, World!")
 * ```
 */
export async function storage(name: string): Promise<Storage> {
  return (await getGlobalContext()).storage(name)
}

/**
 * Get a KV binding.
 *
 * @param name - Binding name
 * @returns KV binding instance
 *
 * @example
 * ```typescript
 * import { kv } from "@alienplatform/bindings"
 *
 * const cache = await kv("my-cache")
 * await cache.set("key", "value")
 * const value = await cache.getText("key")
 * ```
 */
export async function kv(name: string): Promise<Kv> {
  return (await getGlobalContext()).kv(name)
}

/**
 * Get a queue binding.
 *
 * @param name - Binding name
 * @returns Queue binding instance
 *
 * @example
 * ```typescript
 * import { queue } from "@alienplatform/bindings"
 *
 * const tasks = await queue("task-queue")
 * await tasks.send("job-type", { jobId: "123" })
 * ```
 */
export async function queue(name: string): Promise<Queue> {
  return (await getGlobalContext()).queue(name)
}

/**
 * Get a vault binding.
 *
 * @param name - Binding name
 * @returns Vault binding instance
 *
 * @example
 * ```typescript
 * import { vault } from "@alienplatform/bindings"
 *
 * const secrets = await vault("app-secrets")
 * const apiKey = await secrets.get("API_KEY")
 * ```
 */
export async function vault(name: string): Promise<Vault> {
  return (await getGlobalContext()).vault(name)
}

/**
 * Get a build binding.
 *
 * @param name - Binding name
 * @returns Build binding instance
 *
 * @example
 * ```typescript
 * import { build } from "@alienplatform/bindings"
 *
 * const builder = await build("my-builder")
 * const execution = await builder.start({ script: "npm run build" })
 * ```
 */
export async function build(name: string): Promise<Build> {
  return (await getGlobalContext()).build(name)
}

/**
 * Get an artifact registry binding.
 *
 * @param name - Binding name
 * @returns ArtifactRegistry binding instance
 *
 * @example
 * ```typescript
 * import { artifactRegistry } from "@alienplatform/bindings"
 *
 * const registry = await artifactRegistry("my-registry")
 * const repo = await registry.createRepository("my-app")
 * ```
 */
export async function artifactRegistry(name: string): Promise<ArtifactRegistry> {
  return (await getGlobalContext()).artifactRegistry(name)
}

/**
 * Get a function binding.
 *
 * @param name - Binding name
 * @returns Function binding instance
 *
 * @example
 * ```typescript
 * import { func } from "@alienplatform/bindings"
 *
 * const processor = await func("image-processor")
 * const result = await processor.invokeJson("resize", { width: 800 })
 * ```
 */
export async function func(name: string): Promise<FunctionBinding> {
  return (await getGlobalContext()).func(name)
}

/**
 * Get a service account binding.
 *
 * @param name - Binding name
 * @returns ServiceAccount binding instance
 *
 * @example
 * ```typescript
 * import { serviceAccount } from "@alienplatform/bindings"
 *
 * const sa = await serviceAccount("deployment-account")
 * const info = await sa.getInfo()
 * ```
 */
export async function serviceAccount(name: string): Promise<ServiceAccount> {
  return (await getGlobalContext()).serviceAccount(name)
}

// ============================================================================
// Event Handlers
// ============================================================================

/**
 * Register a storage event handler.
 *
 * @param bucket - Bucket name
 * @param handler - Event handler
 * @param options - Handler options
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onStorageEvent } from "@alienplatform/bindings"
 *
 * onStorageEvent("uploads", async (event) => {
 *   console.log("File uploaded:", event.objectKey)
 * })
 * ```
 */
export function onStorageEvent(
  bucket: string,
  handler: (event: StorageEvent) => Promise<void>,
  options?: { prefix?: string },
): () => void {
  return onStorageEventImpl(bucket, handler, options)
}

/**
 * Register a cron event handler.
 *
 * @param schedule - Cron schedule expression
 * @param handler - Event handler
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onCronEvent } from "@alienplatform/bindings"
 *
 * onCronEvent("0 * * * *", async (event) => {
 *   console.log("Hourly task running at:", event.timestamp)
 * })
 * ```
 */
export function onCronEvent(
  schedule: string,
  handler: (event: CronEvent) => Promise<void>,
): () => void {
  return onCronEventImpl(schedule, handler)
}

/**
 * Register a queue message handler.
 *
 * @param queueName - Queue name
 * @param handler - Message handler
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onQueueMessage } from "@alienplatform/bindings"
 *
 * onQueueMessage("tasks", async (message) => {
 *   console.log("Processing:", message.payload)
 * })
 * ```
 */
export function onQueueMessage<T = unknown>(
  queueName: string,
  handler: (message: QueueMessageEvent<T>) => Promise<void>,
): () => void {
  return onQueueMessageImpl(queueName, handler)
}

// ============================================================================
// WaitUntil
// ============================================================================

/**
 * Register a background task to continue after the response.
 *
 * @param promise - The promise to track
 *
 * @example
 * ```typescript
 * import { waitUntil } from "@alienplatform/bindings"
 *
 * export default {
 *   async fetch(request: Request): Promise<Response> {
 *     waitUntil(sendAnalytics(request))
 *     return Response.json({ status: "ok" })
 *   }
 * }
 * ```
 */
export function waitUntil(promise: Promise<unknown>): void {
  waitUntilImpl(promise)
}
