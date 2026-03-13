/**
 * Commands Protocol Types
 *
 * Core command types are re-exported from @aliendotdev/core,
 * which auto-generates them from the Rust implementation.
 */

// Re-export core command types from @aliendotdev/core
export type {
  BodySpec,
  CommandState,
  CommandResponse,
  CommandStatusResponse,
  CreateCommandResponse,
  PresignedRequest,
  PresignedRequestBackend,
  StorageUpload,
} from "@aliendotdev/core"

/**
 * Options for invoking a command
 */
export interface InvokeOptions {
  /** Timeout in milliseconds (default: 60000) */
  timeout?: number
  /** Optional deadline for command completion */
  deadline?: Date
  /** Optional idempotency key */
  idempotencyKey?: string
  /** Polling interval in milliseconds (default: 500) */
  pollInterval?: number
  /** Maximum polling interval in milliseconds (default: 5000) */
  maxPollInterval?: number
  /** Polling backoff multiplier (default: 1.5) */
  pollBackoff?: number
}

/**
 * Configuration for CommandsClient
 */
export interface CommandsClientConfig {
  /** Manager URL (e.g., "https://manager.example.com") */
  managerUrl: string
  /** Deployment ID to invoke commands on */
  deploymentId: string
  /** Authentication token (deployment token or workspace token) */
  token: string
  /** Default timeout in milliseconds (default: 60000) */
  timeout?: number
  /** Allow reading local files for storage responses (default: false, only for local dev) */
  allowLocalStorage?: boolean
}
