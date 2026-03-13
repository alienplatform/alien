/**
 * AlienContext - Main entry point for Alien bindings.
 *
 * Provides access to all bindings and control plane functionality.
 */

import type { StorageEvent } from "@aliendotdev/core"
import { AlienError } from "@aliendotdev/core"
import type { Channel } from "nice-grpc"
import { createClient } from "nice-grpc"
import { ArtifactRegistry } from "./bindings/artifact-registry.js"
import { Build } from "./bindings/build.js"
import { FunctionBinding } from "./bindings/function.js"
import { Kv } from "./bindings/kv.js"
import { Queue } from "./bindings/queue.js"
import { ServiceAccount } from "./bindings/service-account.js"
import { Storage } from "./bindings/storage.js"
import { Vault } from "./bindings/vault.js"
import { createGrpcChannel, getOrCreateChannel } from "./channel.js"
import { command } from "./commands.js"
import { InvalidBindingConfigError } from "./errors.js"
import {
  type CronEvent,
  EventLoop,
  type QueueMessageEvent,
  onCronEvent,
  onQueueMessage,
  onStorageEvent,
} from "./events.js"
import { ControlServiceDefinition } from "./generated/control.js"
import { wrapGrpcCall } from "./grpc-utils.js"
import type { AlienBindingsConfig, AlienBindingsProvider } from "./types.js"
import { type WaitUntilManager, initWaitUntilManager, waitUntil } from "./wait-until.js"

/**
 * Instance ID for this function instance.
 */
function generateInstanceId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 11)}`
}

/**
 * AlienContext provides the main entry point for accessing Alien bindings
 * and control plane functionality.
 *
 * @example
 * ```typescript
 * import { AlienContext } from "@aliendotdev/bindings"
 *
 * // Create context from environment
 * const ctx = AlienContext.fromEnv()
 *
 * // Access bindings
 * const bucket = ctx.storage("my-bucket")
 * await bucket.put("hello.txt", "Hello, World!")
 *
 * // Register event handlers
 * ctx.onStorageEvent("uploads", async (event) => {
 *   console.log("File uploaded:", event.key)
 * })
 *
 * // Start the runtime
 * await ctx.run()
 * ```
 */
export class AlienContext implements AlienBindingsProvider {
  private readonly channel: Channel
  private readonly instanceId: string
  private readonly bindingCache = new Map<string, unknown>()
  private eventLoop: EventLoop | undefined
  private waitUntilManager: WaitUntilManager | undefined

  private constructor(channel: Channel, instanceId?: string) {
    this.channel = channel
    this.instanceId = instanceId ?? generateInstanceId()
  }

  /**
   * Create a context from environment variables.
   *
   * Reads the ALIEN_BINDINGS_GRPC_ADDRESS environment variable.
   */
  static async fromEnv(): Promise<AlienContext> {
    const address = process.env.ALIEN_BINDINGS_GRPC_ADDRESS

    if (!address) {
      throw new AlienError(
        InvalidBindingConfigError.create({
          message: "ALIEN_BINDINGS_GRPC_ADDRESS environment variable is not set",
          suggestion:
            "Make sure you're running inside an Alien function or set the variable manually",
        }),
      )
    }

    const channel = await getOrCreateChannel(address)
    return new AlienContext(channel)
  }

  /**
   * Create a context with explicit configuration.
   */
  static async create(config: AlienBindingsConfig): Promise<AlienContext> {
    if (!config.grpcAddress) {
      throw new AlienError(
        InvalidBindingConfigError.create({
          message: "gRPC address is required",
          suggestion: "Provide grpcAddress in the config",
        }),
      )
    }

    const channel = await createGrpcChannel(config.grpcAddress)
    return new AlienContext(channel)
  }

  /**
   * Create a context for connecting to a remote agent.
   *
   * @param address - gRPC address of the remote agent
   */
  // TODO: Wtf? this is completely hallucinated :) See 05-bindings.md.
  static async forRemoteAgent(address: string): Promise<AlienContext> {
    const channel = await createGrpcChannel(address)
    return new AlienContext(channel)
  }

  /**
   * Get or create a cached binding instance.
   */
  private getBinding<T>(key: string, factory: () => T): T {
    let binding = this.bindingCache.get(key) as T | undefined
    if (!binding) {
      binding = factory()
      this.bindingCache.set(key, binding)
    }
    return binding
  }

  // ============================================================================
  // Binding Accessors
  // ============================================================================

  /**
   * Get a storage binding.
   *
   * @param name - Binding name
   * @returns Storage binding instance
   */
  storage(name: string): Storage {
    return this.getBinding(`storage:${name}`, () => new Storage(this.channel, name))
  }

  /**
   * Get a KV binding.
   *
   * @param name - Binding name
   * @returns KV binding instance
   */
  kv(name: string): Kv {
    return this.getBinding(`kv:${name}`, () => new Kv(this.channel, name))
  }

  /**
   * Get a queue binding.
   *
   * @param name - Binding name
   * @returns Queue binding instance
   */
  queue(name: string): Queue {
    return this.getBinding(`queue:${name}`, () => new Queue(this.channel, name))
  }

  /**
   * Get a vault binding.
   *
   * @param name - Binding name
   * @returns Vault binding instance
   */
  vault(name: string): Vault {
    return this.getBinding(`vault:${name}`, () => new Vault(this.channel, name))
  }

  /**
   * Get a build binding.
   *
   * @param name - Binding name
   * @returns Build binding instance
   */
  build(name: string): Build {
    return this.getBinding(`build:${name}`, () => new Build(this.channel, name))
  }

  /**
   * Get an artifact registry binding.
   *
   * @param name - Binding name
   * @returns ArtifactRegistry binding instance
   */
  artifactRegistry(name: string): ArtifactRegistry {
    return this.getBinding(
      `artifact-registry:${name}`,
      () => new ArtifactRegistry(this.channel, name),
    )
  }

  /**
   * Get a function binding.
   *
   * @param name - Binding name
   * @returns Function binding instance
   */
  func(name: string): FunctionBinding {
    return this.getBinding(`function:${name}`, () => new FunctionBinding(this.channel, name))
  }

  /**
   * Get a service account binding.
   *
   * @param name - Binding name
   * @returns ServiceAccount binding instance
   */
  serviceAccount(name: string): ServiceAccount {
    return this.getBinding(`service-account:${name}`, () => new ServiceAccount(this.channel, name))
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
   */
  onStorageEvent(
    bucket: string,
    handler: (event: StorageEvent) => Promise<void>,
    options?: { prefix?: string },
  ): () => void {
    return onStorageEvent(bucket, handler, options)
  }

  /**
   * Register a cron event handler.
   *
   * @param schedule - Cron schedule expression
   * @param handler - Event handler
   * @returns Unsubscribe function
   */
  onCronEvent(schedule: string, handler: (event: CronEvent) => Promise<void>): () => void {
    return onCronEvent(schedule, handler)
  }

  /**
   * Register a queue message handler.
   *
   * @param queueName - Queue name
   * @param handler - Message handler
   * @returns Unsubscribe function
   */
  onQueueMessage<T = unknown>(
    queueName: string,
    handler: (message: QueueMessageEvent<T>) => Promise<void>,
  ): () => void {
    return onQueueMessage(queueName, handler)
  }

  // ============================================================================
  // Commands
  // ============================================================================

  /**
   * Register an ARC command.
   *
   * @param name - Command name
   * @param handler - Command handler that receives params and returns a result
   */
  command<TParams = unknown, TResult = unknown>(
    name: string,
    handler: (params: TParams) => Promise<TResult>,
  ): void {
    command(name, handler)
  }

  // ============================================================================
  // HTTP Server Registration
  // ============================================================================

  /**
   * Register an HTTP server with the control plane.
   *
   * @param port - Port the server is listening on
   */
  async registerHttpServer(port: number): Promise<void> {
    const client = createClient(ControlServiceDefinition, this.channel)

    await wrapGrpcCall(
      "ControlService",
      "RegisterHttpServer",
      async () => {
        await client.registerHttpServer({ port })
      },
      {},
    )
  }

  // ============================================================================
  // WaitUntil
  // ============================================================================

  /**
   * Register a background task.
   *
   * @param promise - The promise to track
   */
  waitUntil(promise: Promise<unknown>): void {
    waitUntil(promise)
  }

  // ============================================================================
  // Lifecycle
  // ============================================================================

  /**
   * Start the Alien runtime.
   *
   * This initializes event handlers, registers with the control plane,
   * and starts processing events.
   */
  async run(): Promise<void> {
    // Initialize wait-until manager
    this.waitUntilManager = initWaitUntilManager(this.channel, this.instanceId)

    // Initialize event loop
    this.eventLoop = new EventLoop(this.channel, this.instanceId)

    // Register event handlers with the control plane
    await this.eventLoop.registerHandlers()

    // Start event loop (handles ARC commands, events, etc.)
    await this.eventLoop.start()
  }

  /**
   * Stop the runtime gracefully.
   */
  async shutdown(): Promise<void> {
    // Stop event loop
    this.eventLoop?.stop()

    // Wait for all background tasks
    await this.waitUntilManager?.waitForAll()

    // Notify drain complete
    const tasks = this.waitUntilManager?.getTasks()
    if (tasks) {
      const taskArray = Array.from(tasks.values())
      const tasksDrained = taskArray.length
      const allSuccess = taskArray.every(t => !t.error)
      const errorMessage = taskArray.find(t => t.error)?.error?.message
      await this.waitUntilManager?.notifyDrainComplete(tasksDrained, allSuccess, errorMessage)
    }
  }
}
