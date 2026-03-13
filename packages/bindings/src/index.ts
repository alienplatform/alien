/**
 * @alienplatform/bindings - TypeScript SDK for Alien bindings.
 *
 * This package provides type-safe access to Alien resources including
 * Storage, KV, Queue, Vault, Build, ArtifactRegistry, Function, and ServiceAccount.
 *
 * @example
 * ```typescript
 * // Global convenience functions (most common usage)
 * import { storage, kv, onStorageEvent, waitUntil } from "@alienplatform/bindings"
 *
 * const bucket = storage("my-bucket")
 * await bucket.put("hello.txt", "Hello, World!")
 *
 * // Or use AlienContext for more control
 * import { AlienContext } from "@alienplatform/bindings"
 *
 * const ctx = AlienContext.fromEnv()
 * const bucket = ctx.storage("my-bucket")
 * ```
 *
 * @packageDocumentation
 */

// ============================================================================
// Main Entry Point
// ============================================================================

export { AlienContext } from "./context.js"

// ============================================================================
// Global Convenience Functions
// ============================================================================

export {
  // Binding accessors
  storage,
  kv,
  queue,
  vault,
  build,
  artifactRegistry,
  func,
  serviceAccount,
  // Event handlers
  onStorageEvent,
  onCronEvent,
  onQueueMessage,
  // Commands
  command,
  // WaitUntil
  waitUntil,
} from "./global.js"

// Event types - StorageEvent, QueueMessage, ScheduledEvent come from @alienplatform/core
// CronEvent and QueueMessageEvent are bindings-specific extensions
export type {
  StorageEvent,
  StorageEventType,
  QueueMessage,
  ScheduledEvent,
  CronEvent,
  QueueMessageEvent,
} from "./events.js"

// ============================================================================
// Binding Classes (for advanced usage)
// ============================================================================

export { Storage } from "./bindings/storage.js"
export { Kv } from "./bindings/kv.js"
export { Queue } from "./bindings/queue.js"
export { Vault } from "./bindings/vault.js"
export { Build } from "./bindings/build.js"
export { ArtifactRegistry } from "./bindings/artifact-registry.js"
export { FunctionBinding } from "./bindings/function.js"
export { ServiceAccount } from "./bindings/service-account.js"

// ============================================================================
// Types
// ============================================================================

export type {
  // Storage
  StorageGetOptions,
  StoragePutOptions,
  StorageObjectMeta,
  StorageGetResult,
  StorageListResult,
  SignedUrlOptions,
  SignedUrlResult,
  // KV
  KvPutOptions,
  KvScanResult,
  // Queue
  ReceivedQueueMessage,
  // Build (BuildStatus and ComputeType come from @alienplatform/core)
  BuildStartConfig,
  BuildExecution,
  // Artifact Registry
  ArtifactRegistryPermissions,
  RepositoryInfo,
  ArtifactRegistryCredentials,
  ComputeServiceType,
  AwsCrossAccountAccess,
  GcpCrossAccountAccess,
  CrossAccountAccess,
  CrossAccountPermissions,
  // Function
  FunctionInvokeRequest,
  FunctionInvokeResponse,
  // Service Account
  ServiceAccountInfo,
  AwsServiceAccountInfo,
  GcpServiceAccountInfo,
  AzureServiceAccountInfo,
  ImpersonationRequest,
  // Configuration
  AlienBindingsConfig,
  AlienBindingsProvider,
} from "./types.js"

// ============================================================================
// Errors
// ============================================================================

export {
  GrpcConnectionError,
  GrpcCallError,
  BindingNotFoundError,
  ContextNotInitializedError,
  MissingEnvVarError,
  StorageObjectNotFoundError,
  StoragePreconditionError,
  StorageObjectExistsError,
  KvKeyNotFoundError,
  KvInvalidKeyError,
  KvInvalidValueError,
  SecretNotFoundError,
  QueueOperationError,
  EventHandlerAlreadyRegisteredError,
  CommandAlreadyRegisteredError,
  InvalidBindingConfigError,
} from "./errors.js"

// ============================================================================
// Commands
// ============================================================================

export { runCommand, getCommands, type CommandDefinition } from "./commands.js"

// ============================================================================
// Re-export from core for convenience
// ============================================================================

// Note: BuildStatus, ComputeType are re-exported via types.ts
