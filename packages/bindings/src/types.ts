/**
 * Common types for the Alien bindings SDK.
 */

// Re-export build types from core
export type { BuildStatus, ComputeType } from "@aliendotdev/core"

/**
 * Branded type for binding names.
 */
export type BindingName = string & { readonly __brand: unique symbol }

// ============================================================================
// Storage Types
// ============================================================================

/**
 * Options for storage get operations.
 */
export interface StorageGetOptions {
  /** Range start offset in bytes */
  rangeStart?: number
  /** Range end offset in bytes */
  rangeEnd?: number
  /** If-Match header value (ETag) */
  ifMatch?: string
  /** If-None-Match header value (ETag) */
  ifNoneMatch?: string
  /** If-Modified-Since timestamp */
  ifModifiedSince?: Date
  /** If-Unmodified-Since timestamp */
  ifUnmodifiedSince?: Date
}

/**
 * Options for storage put operations.
 */
export interface StoragePutOptions {
  /** Content type of the object */
  contentType?: string
  /** Custom metadata */
  metadata?: Record<string, string>
  /** Only write if the object doesn't already exist */
  ifNotExists?: boolean
}

/**
 * Object metadata from storage.
 */
export interface StorageObjectMeta {
  /** Full path of the object */
  location: string
  /** Last modified timestamp */
  lastModified?: Date
  /** Object size in bytes */
  size: number
  /** ETag (entity tag) for the object */
  etag?: string
  /** Object version */
  version?: string
}

/**
 * Result of a storage get operation.
 */
export interface StorageGetResult {
  /** Object metadata */
  meta: StorageObjectMeta
  /** Object data as bytes */
  data: Uint8Array
}

/**
 * Result of a storage list operation with delimiter.
 */
export interface StorageListResult {
  /** Common prefixes (directories) */
  commonPrefixes: string[]
  /** Object metadata entries */
  objects: StorageObjectMeta[]
}

/**
 * Options for signed URL generation.
 */
export interface SignedUrlOptions {
  /** Operation type: get (download), put (upload), or delete */
  operation: "get" | "put" | "delete"
  /** URL expiration duration in seconds */
  expiresInSeconds?: number
  /** Content type for put operations */
  contentType?: string
}

/**
 * Result of signed URL generation.
 */
export interface SignedUrlResult {
  /** The signed URL */
  url: string
  /** URL expiration timestamp */
  expiresAt?: Date
}

// ============================================================================
// KV Types
// ============================================================================

/**
 * Options for KV put operations.
 */
export interface KvPutOptions {
  /** Time-to-live in milliseconds */
  ttlMs?: number
  /** Only write if the key doesn't exist */
  ifNotExists?: boolean
}

/**
 * Result of a KV scan operation.
 */
export interface KvScanResult {
  /** Key-value pairs */
  items: Array<{ key: string; value: Uint8Array }>
  /** Cursor for next page */
  nextCursor?: string
}

// ============================================================================
// Queue Types
// ============================================================================

/**
 * A message received from a queue via the Queue binding.
 * (Not to be confused with QueueMessage from @aliendotdev/core which is used for event handlers)
 */
export interface ReceivedQueueMessage<T = unknown> {
  /** Message payload */
  payload: T
  /** Receipt handle for acknowledgment */
  receiptHandle: string
}

// ============================================================================
// Build Types
// ============================================================================

// BuildStatus and ComputeType are re-exported from @aliendotdev/core above

/**
 * Configuration for starting a build via the Build binding.
 * (SDK-specific - different from BuildConfig in core which is for resource definition)
 */
export interface BuildStartConfig {
  /** Build script to execute */
  script: string
  /** Environment variables */
  environment?: Record<string, string>
  /** Compute type */
  computeType?: import("@aliendotdev/core").ComputeType
  /** Timeout in seconds */
  timeoutSeconds?: number
  /** Monitoring configuration */
  monitoring?: {
    endpoint: string
    headers?: Record<string, string>
    logsUri?: string
    tlsEnabled?: boolean
    tlsVerify?: boolean
  }
}

/**
 * Information about a build execution.
 */
export interface BuildExecution {
  /** Build execution ID */
  id: string
  /** Current status */
  status: import("@aliendotdev/core").BuildStatus
  /** Start timestamp */
  startTime?: Date
  /** End timestamp */
  endTime?: Date
}

// ============================================================================
// Artifact Registry Types
// ============================================================================

/**
 * Permission levels for artifact registry access.
 */
export type ArtifactRegistryPermissions = "pull" | "push-pull"

/**
 * Information about a repository.
 */
export interface RepositoryInfo {
  /** Repository name */
  name: string
  /** Repository URI */
  uri: string
  /** Creation timestamp */
  createdAt?: Date
}

/**
 * Credentials for repository access.
 */
export interface ArtifactRegistryCredentials {
  /** Username for authentication */
  username: string
  /** Password/token for authentication */
  password: string
  /** Credential expiration timestamp */
  expiresAt?: Date
}

/**
 * Types of compute services that can have cross-account access.
 */
export type ComputeServiceType = "function"

/**
 * AWS-specific cross-account access configuration.
 */
export interface AwsCrossAccountAccess {
  /** AWS account IDs that should have access */
  accountIds: string[]
  /** Types of compute services that should have access */
  allowedServiceTypes: ComputeServiceType[]
  /** Specific IAM role ARNs to grant access to */
  roleArns: string[]
}

/**
 * GCP-specific cross-account access configuration.
 */
export interface GcpCrossAccountAccess {
  /** GCP project numbers that should have access */
  projectNumbers: string[]
  /** Types of compute services that should have access */
  allowedServiceTypes: ComputeServiceType[]
  /** Additional service account emails to grant access to */
  serviceAccountEmails: string[]
}

/**
 * Cross-account access configuration (discriminated union).
 */
export type CrossAccountAccess =
  | { type: "aws"; aws: AwsCrossAccountAccess }
  | { type: "gcp"; gcp: GcpCrossAccountAccess }

/**
 * Current cross-account access permissions.
 */
export interface CrossAccountPermissions {
  /** Platform-specific access configuration */
  access: CrossAccountAccess
  /** Timestamp when permissions were last updated (ISO8601 format) */
  lastUpdated?: string
}

// ============================================================================
// Function Types
// ============================================================================

/**
 * Request for function invocation.
 */
export interface FunctionInvokeRequest {
  /** Target function identifier */
  targetFunction: string
  /** HTTP method */
  method: string
  /** Request path */
  path: string
  /** Request headers */
  headers?: Record<string, string>
  /** Request body */
  body?: Uint8Array
  /** Timeout in milliseconds */
  timeoutMs?: number
}

/**
 * Response from function invocation.
 */
export interface FunctionInvokeResponse {
  /** HTTP status code */
  status: number
  /** Response headers */
  headers: Record<string, string>
  /** Response body */
  body: Uint8Array
}

// ============================================================================
// Service Account Types
// ============================================================================

/**
 * AWS service account information.
 */
export interface AwsServiceAccountInfo {
  platform: "aws"
  roleName: string
  roleArn: string
}

/**
 * GCP service account information.
 */
export interface GcpServiceAccountInfo {
  platform: "gcp"
  email: string
  uniqueId: string
}

/**
 * Azure service account information.
 */
export interface AzureServiceAccountInfo {
  platform: "azure"
  clientId: string
  resourceId: string
  principalId: string
}

/**
 * Platform-specific service account information.
 */
export type ServiceAccountInfo =
  | AwsServiceAccountInfo
  | GcpServiceAccountInfo
  | AzureServiceAccountInfo

/**
 * Request for service account impersonation.
 */
export interface ImpersonationRequest {
  /** Session name for AWS */
  sessionName?: string
  /** Duration in seconds */
  durationSeconds?: number
  /** OAuth scopes for GCP */
  scopes?: string[]
}

// ============================================================================
// Configuration Types
// ============================================================================

/**
 * Configuration for connecting to the Alien bindings server.
 */
export interface AlienBindingsConfig {
  /** gRPC server address */
  grpcAddress?: string
}

/**
 * Interface for the bindings provider.
 */
export interface AlienBindingsProvider {
  /** Get a storage binding */
  storage(name: string): import("./bindings/storage.js").Storage
  /** Get a KV binding */
  kv(name: string): import("./bindings/kv.js").Kv
  /** Get a queue binding */
  queue(name: string): import("./bindings/queue.js").Queue
  /** Get a vault binding */
  vault(name: string): import("./bindings/vault.js").Vault
  /** Get a build binding */
  build(name: string): import("./bindings/build.js").Build
  /** Get an artifact registry binding */
  artifactRegistry(name: string): import("./bindings/artifact-registry.js").ArtifactRegistry
  /** Get a function binding */
  func(name: string): import("./bindings/function.js").FunctionBinding
  /** Get a service account binding */
  serviceAccount(name: string): import("./bindings/service-account.js").ServiceAccount
}
